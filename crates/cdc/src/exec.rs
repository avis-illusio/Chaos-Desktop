//! `.cdc` 执行器。
//!
//! 执行模型：
//! - 一个块（文件顶层或某个缩进块）= 一个执行单元，身份栈独立初始化。
//! - 身份命令只影响**当前层**；`@continue` 把调用点父层的身份快照注入 `inherited`。
//! - `@synchronizer` 生效期间，块内的进程命令并发触发；`@synchronizer!` 之后恢复顺序，
//!   已在后台的命令不等待（块结束时统一收尾）。
//! - 安全模式（`--safe`）跳过所有会创建进程的命令（`W005`）。

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::Scope;

use cdc_parser::{CommandKind, Diagnostic, Node, Program, Span, codes};
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
        let mut frame = Frame::default();

        for node in nodes {
            let span = node.command.span;

            match &node.command.kind {
                CommandKind::Identity { name, unload } => {
                    if *unload {
                        frame.unload(name, span, self);
                    } else {
                        frame.attach(name);
                    }
                }
                CommandKind::Continue => {
                    if parent_scope.is_empty() {
                        self.emit(Diagnostic::warning(
                            codes::CONTINUE_EMPTY,
                            span,
                            "`@continue` 的父层没有身份可继承（等同常态启动）",
                        ));
                    } else {
                        // 快照在调用点捕获，且不去重：重复 @continue 会重复叠加。
                        frame.inherited.extend(parent_scope.iter().cloned());
                    }
                }
                CommandKind::Invalid => {
                    // 解析已报错，执行前就会被拦下；这里兜底不做事。
                }
                kind => {
                    if frame.synchronized() {
                        // 并发触发：命令在后台跑，主序列不等待。
                        scope.spawn(move || self.exec_spawn(kind, span));
                    } else {
                        self.exec_spawn(kind, span);
                    }
                }
            }

            // 子块是独立执行单元：身份栈重新初始化，只能通过 @continue 继承。
            if !node.children.is_empty() {
                let snapshot = frame.effective();
                self.run_unit(&node.children, &snapshot, scope);
            }
        }

        for name in &frame.attached {
            self.emit(Diagnostic::warning(
                codes::UNCLOSED_IDENTITY,
                Span::new(0, 0),
                format!("身份 `@{name}` 在块结束时仍未卸载，已隐式关闭"),
            ));
        }
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

/// 当前层的身份栈。
#[derive(Debug, Default)]
struct Frame {
    /// 本层自己附加的身份，可卸载。
    attached: Vec<String>,
    /// `@continue` 注入的身份，不可卸载。
    inherited: Vec<String>,
}

impl Frame {
    fn attach(&mut self, name: &str) {
        self.attached.push(name.to_string());
    }

    fn unload(&mut self, name: &str, span: Span, ex: &Executor) {
        if let Some(i) = self.attached.iter().rposition(|n| n == name) {
            self.attached.remove(i);
        } else if self.inherited.iter().any(|n| n == name) {
            ex.emit(Diagnostic::warning(
                codes::UNLOAD_INHERITED,
                span,
                format!("`@{name}` 是 `@continue` 继承来的，不能在本层卸载"),
            ));
        } else {
            ex.emit(Diagnostic::warning(
                codes::EMPTY_UNLOAD,
                span,
                format!("本层没有 `@{name}` 可以卸载（空操作）"),
            ));
        }
    }

    fn effective(&self) -> Vec<String> {
        let mut v = self.attached.clone();
        v.extend(self.inherited.iter().cloned());
        v
    }

    fn synchronized(&self) -> bool {
        self.attached.iter().any(|n| n == "synchronizer")
            || self.inherited.iter().any(|n| n == "synchronizer")
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
