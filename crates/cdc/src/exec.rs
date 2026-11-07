//! `.cdc` 执行器。
//!
//! 身份不再是「栈上的状态」，而是**带作用域的属性**：
//! `@X{作用域}{正则}` 决定 X 作用在本执行单元的哪些语句上。
//! 执行前先为每条语句算出它携带的身份集合，再按集合执行。
//!
//! - `@continue` 的作用是把「调用点父层的身份快照」注入被它覆盖的语句。
//! - `@X!` 结束 X 的作用区间（把它之后开始的区间一律截断）。
//! - `@synchronizer` 覆盖到的语句并发触发。

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::Scope;

use cdc_parser::{CommandKind, Diagnostic, Node, Program, ScopeSpec, Span, codes};
use chaos_path::{PathError, resolve_file};

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub safe: bool,
    pub verbose: bool,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub diagnostics: Vec<Diagnostic>,
    /// 真正创建了进程的命令数。
    pub executed: usize,
    /// 失败的命令数（spawn 失败或退出码非 0）。
    pub failed: usize,
}

pub struct Executor {
    opts: Options,
    diags: Arc<Mutex<Vec<Diagnostic>>>,
    executed: Arc<Mutex<usize>>,
    failed: Arc<Mutex<usize>>,
}

impl Executor {
    pub fn new(opts: Options) -> Self {
        Self {
            opts,
            diags: Arc::new(Mutex::new(Vec::new())),
            executed: Arc::new(Mutex::new(0)),
            failed: Arc::new(Mutex::new(0)),
        }
    }

    pub fn run(self, program: &Program) -> Outcome {
        self.run_block(&program.nodes, &[]);
        let diags = self.diags.lock().map(|d| d.clone()).unwrap_or_default();
        let executed = self.executed.lock().map(|v| *v).unwrap_or(0);
        let failed = self.failed.lock().map(|v| *v).unwrap_or(0);
        let mut out = Outcome {
            diagnostics: diags,
            executed,
            failed,
        };
        out.diagnostics.sort_by_key(|d| (d.span.line, d.span.col));
        out
    }

    fn emit(&self, d: Diagnostic) {
        if let Ok(mut v) = self.diags.lock() {
            v.push(d);
        }
    }

    fn note_executed(&self, ok: bool) {
        if let Ok(mut v) = self.executed.lock() {
            *v += 1;
        }
        if !ok && let Ok(mut v) = self.failed.lock() {
            *v += 1;
        }
    }

    /// 一个缩进块 = 一个执行单元。
    fn run_block(&self, nodes: &[Node], parent_scope: &[String]) {
        std::thread::scope(|s| self.run_unit(nodes, parent_scope, s));
    }

    fn run_unit<'scope>(
        &'scope self,
        nodes: &'scope [Node],
        parent_scope: &[String],
        scope: &'scope Scope<'scope, '_>,
    ) {
        let plan = self.plan(nodes, parent_scope);

        for (i, node) in nodes.iter().enumerate() {
            let ids = &plan[i];

            match &node.command.kind {
                // 身份指令本身只贡献属性，执行阶段不做事。
                CommandKind::Identity { .. } | CommandKind::Invalid => {}
                kind => {
                    let span = node.command.span;
                    if ids.iter().any(|s| s == "synchronizer") {
                        // 并发触发：命令在后台跑，主序列不等待。
                        scope.spawn(move || self.exec_spawn(kind, span));
                    } else {
                        self.exec_spawn(kind, span);
                    }
                }
            }

            // 子块是独立执行单元：只继承「父语句在这一刻携带的身份」。
            if !node.children.is_empty() {
                self.run_unit(&node.children, ids, scope);
            }
        }
    }

    /// 为执行单元里的每条语句算出它携带的身份集合。
    fn plan(&self, nodes: &[Node], parent_scope: &[String]) -> Vec<Vec<String>> {
        let len = nodes.len();
        let mut plan: Vec<Vec<String>> = vec![Vec::new(); len];

        for (i, node) in nodes.iter().enumerate() {
            let CommandKind::Identity {
                name,
                unload,
                scope,
                filter,
            } = &node.command.kind
            else {
                continue;
            };
            if *unload {
                continue; // 注销只用于截断别人的区间
            }

            let Some((start, end)) = resolve_scope(*scope, i, len) else {
                self.emit(Diagnostic::error(
                    codes::SCOPE_RANGE,
                    node.command.span,
                    format!("`@{name}` 的作用域超出本执行单元（共 {len} 条语句）"),
                ));
                continue;
            };

            // 同一身份的注销会把后续区间截断。
            let stop = (i + 1..len)
                .find(|&j| {
                    matches!(
                        &nodes[j].command.kind,
                        CommandKind::Identity { name: n2, unload: true, .. } if n2 == name
                    )
                })
                .unwrap_or(len);
            let end = end.min(stop);
            if start >= end {
                continue;
            }

            let regex = filter.as_ref().and_then(|f| regex::Regex::new(f).ok());
            let mut matched = 0usize;
            for j in start..end {
                // 身份指令自身不参与筛选：不能把属性加到一条指令上。
                if matches!(nodes[j].command.kind, CommandKind::Identity { .. }) {
                    continue;
                }
                if let Some(re) = &regex
                    && !re.is_match(&nodes[j].command.raw)
                {
                    continue;
                }
                matched += 1;
                if name == "continue" {
                    for id in parent_scope {
                        push_unique(&mut plan[j], id);
                    }
                } else {
                    push_unique(&mut plan[j], name);
                }
            }

            if matched == 0 {
                self.emit(Diagnostic::warning(
                    codes::SCOPE_NO_MATCH,
                    node.command.span,
                    format!("`@{name}` 的作用域里没有匹配的语句"),
                ));
            }
            if name == "continue" && parent_scope.is_empty() && matched > 0 {
                self.emit(Diagnostic::warning(
                    codes::CONTINUE_EMPTY,
                    node.command.span,
                    "`@continue` 的父层没有身份可继承（等同常态启动）",
                ));
            }
        }

        plan
    }

    fn exec_spawn(&self, kind: &CommandKind, span: Span) {
        if self.opts.safe {
            self.emit(Diagnostic::warning(
                codes::SKIPPED_UNSAFE,
                span,
                format!("安全模式：跳过 `{}`", kind.name()),
            ));
            return;
        }

        let mut cmd = match kind {
            CommandKind::Xrun { arg } => match resolve_executable(arg) {
                Ok(path) => Command::new(path),
                Err(d) => {
                    self.emit(d.with_span(span));
                    self.note_executed(false);
                    return;
                }
            },
            CommandKind::RunFile { arg } => match resolve_script(arg) {
                Ok(path) => {
                    let mut c = Command::new("/bin/sh");
                    c.arg(path);
                    c
                }
                Err(d) => {
                    self.emit(d.with_span(span));
                    self.note_executed(false);
                    return;
                }
            },
            CommandKind::RunCommand { arg } => {
                let mut c = Command::new("/bin/sh");
                c.arg("-c").arg(arg);
                c
            }
            _ => return,
        };

        // setpgid(0,0)：脱离 cdc 的进程组，Ctrl+C 不会连带打死后台命令。
        cmd.process_group(0);

        if self.opts.verbose {
            eprintln!("→ {} ({})", kind.name(), span);
        }

        match cmd.status() {
            Ok(status) if status.success() => {
                self.note_executed(true);
            }
            Ok(status) => {
                self.note_executed(false);
                self.emit(Diagnostic::error(
                    codes::NON_ZERO_EXIT,
                    span,
                    format!("`{}` 退出码 {:?}", kind.name(), status.code()),
                ));
            }
            Err(err) => {
                self.note_executed(false);
                self.emit(Diagnostic::error(
                    codes::SPAWN_FAILED,
                    span,
                    format!("`{}` 启动失败：{err}", kind.name()),
                ));
            }
        }
    }
}

fn push_unique(list: &mut Vec<String>, id: &str) {
    if !list.iter().any(|s| s == id) {
        list.push(id.to_string());
    }
}

/// 把 `{…}` 作用域解析成 0 基半开区间 `[start, end)`；越界返回 `None`。
fn resolve_scope(scope: ScopeSpec, directive: usize, len: usize) -> Option<(usize, usize)> {
    let pos = |p: i64| -> Option<usize> {
        let raw = if p > 0 { p } else { len as i64 + p + 1 };
        if raw < 1 || raw > len as i64 {
            return None;
        }
        Some(raw as usize - 1)
    };

    match scope {
        ScopeSpec::All => Some((directive, len)),
        ScopeSpec::Point(p) => {
            let at = pos(p)?;
            Some((at, at + 1))
        }
        ScopeSpec::To(p) => {
            let end = pos(p)?;
            Some((directive, end + 1))
        }
        ScopeSpec::From(p) => {
            let start = pos(p)?;
            Some((start, len))
        }
        ScopeSpec::Between(a, b) => {
            let (x, y) = (pos(a)?, pos(b)?);
            let (lo, hi) = if x <= y { (x, y) } else { (y, x) };
            Some((lo, hi + 1))
        }
    }
}

/// 带 span 的延迟诊断（路径解析在 exec 阶段才做，错误码已定）。
#[derive(Debug)]
pub struct PathDiag {
    code: &'static str,
    message: String,
}

impl PathDiag {
    fn with_span(self, span: Span) -> Diagnostic {
        Diagnostic::error(self.code, span, self.message)
    }
}

fn resolve_executable(arg: &str) -> Result<String, PathDiag> {
    let path = resolve_file(arg).map_err(|e| path_diag(arg, e))?;
    let p = path.as_path();
    if !p.exists() {
        return Err(PathDiag {
            code: codes::BAD_PATH,
            message: format!("文件不存在：{path}"),
        });
    }
    let meta = std::fs::metadata(p).map_err(|e| PathDiag {
        code: codes::BAD_PATH,
        message: format!("无法读取 {path}：{e}"),
    })?;
    if !meta.is_file() {
        return Err(PathDiag {
            code: codes::BAD_PATH,
            message: format!("不是普通文件：{path}"),
        });
    }
    if meta.permissions().mode() & 0o111 == 0 {
        return Err(PathDiag {
            code: codes::BAD_PATH,
            message: format!("没有执行权限：{path}"),
        });
    }
    Ok(p.to_string_lossy().into_owned())
}

fn resolve_script(arg: &str) -> Result<String, PathDiag> {
    let path = resolve_file(arg).map_err(|e| path_diag(arg, e))?;
    let p = path.as_path();
    if !p.is_file() {
        return Err(PathDiag {
            code: codes::BAD_PATH,
            message: format!("脚本不存在：{path}"),
        });
    }
    Ok(p.to_string_lossy().into_owned())
}

fn path_diag(arg: &str, err: PathError) -> PathDiag {
    PathDiag {
        code: codes::BAD_PATH,
        message: format!("路径 `{arg}` 无法解析：{err}"),
    }
}

/// 只做路径校验，不执行任何命令（`cdc validate` 用）。
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    program.walk(&mut |node| {
        let span = node.command.span;
        match &node.command.kind {
            CommandKind::Xrun { arg } => {
                if let Err(d) = resolve_executable(arg) {
                    out.push(d.with_span(span));
                }
            }
            CommandKind::RunFile { arg } => {
                if let Err(d) = resolve_script(arg) {
                    out.push(d.with_span(span));
                }
            }
            _ => {}
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_resolution() {
        // 单元有 5 条语句，指令在 0 号位。
        assert_eq!(resolve_scope(ScopeSpec::All, 0, 5), Some((0, 5)));
        assert_eq!(resolve_scope(ScopeSpec::Point(3), 0, 5), Some((2, 3)));
        assert_eq!(resolve_scope(ScopeSpec::To(3), 0, 5), Some((0, 3)));
        assert_eq!(resolve_scope(ScopeSpec::From(3), 0, 5), Some((2, 5)));
        assert_eq!(resolve_scope(ScopeSpec::Between(4, 2), 0, 5), Some((1, 4)));
        assert_eq!(resolve_scope(ScopeSpec::Point(-1), 0, 5), Some((4, 5)));
        assert_eq!(resolve_scope(ScopeSpec::Point(9), 0, 5), None);
        assert_eq!(resolve_scope(ScopeSpec::From(-9), 0, 5), None);
    }
}
