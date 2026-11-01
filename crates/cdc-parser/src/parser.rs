//! 缩进敏感的 `.cdc` 解析器。
//!
//! 语法要点：
//! - 缩进即结构：缩进更深的行属于上一条命令的子块，子块是**独立执行单元**。
//! - 缩进宽度不限，但同层必须完全一致；tab 与空格不得混用。
//! - `#` 独占行首（缩进之后）才是注释；参数一律取到行尾，参数里的 `#` 不当注释。
//! - 一行最多一条命令；空行忽略。

use crate::ast::*;

struct Line {
    no: u32,
    indent: String,
    content: String,
}

impl Line {
    fn span(&self) -> Span {
        // 列号 = 缩进宽度 + 1（按字符计，缩进只可能是空格或 tab）
        Span::new(self.no, self.indent.chars().count() as u32 + 1)
    }
}

/// 解析一份 `.cdc` 源码。永不 panic，所有问题进 `diagnostics`。
pub fn parse(source: &str) -> Parsed {
    let mut diagnostics = Vec::new();
    let lines = lex(source, &mut diagnostics);

    let mut nodes = Vec::new();
    if let Some(first) = lines.first() {
        if !first.indent.is_empty() {
            diagnostics.push(Diagnostic::error(
                codes::LEADING_INDENT,
                first.span(),
                "文件第一行不能带缩进",
            ));
        }
        // 即使首行带缩进也按它作为根层级，尽量多解析出结构。
        let root_indent = first.indent.clone();
        let mut pos = 0;
        let mut next_id = 0;
        nodes = parse_block(
            &lines,
            &mut pos,
            &root_indent,
            &mut diagnostics,
            &mut next_id,
        );
    }

    Parsed {
        program: Program { nodes },
        diagnostics,
    }
}

fn lex(source: &str, diagnostics: &mut Vec<Diagnostic>) -> Vec<Line> {
    let mut out = Vec::new();
    for (idx, raw) in source.lines().enumerate() {
        let no = idx as u32 + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let indent_len = line
            .char_indices()
            .find(|(_, c)| *c != ' ' && *c != '\t')
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        let (indent, rest) = line.split_at(indent_len);
        let rest = rest.trim_end();

        if rest.is_empty() || rest.starts_with('#') {
            continue;
        }

        if indent.contains(' ') && indent.contains('\t') {
            diagnostics.push(Diagnostic::error(
                codes::INDENT_MIXED,
                Span::new(no, 1),
                "缩进里不能同时出现空格和 tab",
            ));
        }

        out.push(Line {
            no,
            indent: indent.to_string(),
            content: rest.to_string(),
        });
    }
    out
}

fn parse_block(
    lines: &[Line],
    pos: &mut usize,
    indent: &str,
    diagnostics: &mut Vec<Diagnostic>,
    next_id: &mut usize,
) -> Vec<Node> {
    let mut out = Vec::new();

    while *pos < lines.len() {
        let line = &lines[*pos];

        if line.indent == indent {
            *pos += 1;
            let id = *next_id;
            *next_id += 1;
            let command = parse_command(line, id, diagnostics);

            let mut children = Vec::new();
            if let Some(next) = lines.get(*pos)
                && next.indent.len() > indent.len()
                && next.indent.starts_with(indent)
            {
                let child_indent = next.indent.clone();
                children = parse_block(lines, pos, &child_indent, diagnostics, next_id);
            }

            out.push(Node { command, children });
        } else if line.indent.len() > indent.len() && line.indent.starts_with(indent) {
            // 只可能出现在「上一条命令不存在」的位置：缩进跳跃。
            diagnostics.push(Diagnostic::error(
                codes::INDENT_MISMATCH,
                line.span(),
                "缩进跳跃：这一行没有可归属的父命令",
            ));
            *pos += 1;
        } else if indent.len() > line.indent.len() && indent.starts_with(&line.indent) {
            break; // 正常回退到父层
        } else {
            diagnostics.push(Diagnostic::error(
                codes::INDENT_MISMATCH,
                line.span(),
                "缩进与上层不一致（同层必须完全相同的缩进）",
            ));
            *pos += 1;
        }
    }

    out
}

fn parse_command(line: &Line, id: usize, diagnostics: &mut Vec<Diagnostic>) -> Command {
    let text = line.content.as_str();
    let span = line.span();
    let invalid = |diagnostics: &mut Vec<Diagnostic>, code: &'static str, msg: &str| {
        diagnostics.push(Diagnostic::error(code, span, msg));
        Command {
            id,
            span,
            kind: CommandKind::Invalid,
        }
    };

    if let Some(rest) = text.strip_prefix('@') {
        if rest == "continue" {
            return Command {
                id,
                span,
                kind: CommandKind::Continue,
            };
        }

        let (name, unload) = match rest.strip_suffix('!') {
            Some(n) => (n, true),
            None => (rest, false),
        };

        if unload && name == "continue" {
            return invalid(
                diagnostics,
                codes::CONTINUE_UNLOAD,
                "`@continue!` 没有意义：继承来的身份不能卸载",
            );
        }
        if name.is_empty() || name.contains(char::is_whitespace) {
            return invalid(
                diagnostics,
                codes::IDENTITY_SYNTAX,
                "身份写法必须是 `@name` 或 `@name!`，且一行只能有一个",
            );
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return invalid(
                diagnostics,
                codes::IDENTITY_SYNTAX,
                "身份名只能包含字母、数字、`_`、`-`",
            );
        }
        if !KNOWN_IDENTITIES.contains(&name) {
            let known = KNOWN_IDENTITIES.join("`, `@");
            return invalid(
                diagnostics,
                codes::UNKNOWN_IDENTITY,
                &format!("未定义的身份 `@{name}`（当前只有 `@{known}`）"),
            );
        }

        return Command {
            id,
            span,
            kind: CommandKind::Identity {
                name: name.to_string(),
                unload,
            },
        };
    }

    if text == "xrun" {
        return invalid(
            diagnostics,
            codes::MISSING_ARGUMENT,
            "`xrun` 需要一个文件路径",
        );
    }
    if let Some(rest) = text.strip_prefix("xrun")
        && rest.starts_with([' ', '\t'])
    {
        let arg = rest.trim();
        if arg.is_empty() {
            return invalid(
                diagnostics,
                codes::MISSING_ARGUMENT,
                "`xrun` 需要一个文件路径",
            );
        }
        return Command {
            id,
            span,
            kind: CommandKind::Xrun {
                arg: arg.to_string(),
            },
        };
    }

    if text == "run" {
        return invalid(
            diagnostics,
            codes::MISSING_ARGUMENT,
            "`run` 需要 `file` 或 `command`",
        );
    }
    if let Some(rest) = text.strip_prefix("run")
        && rest.starts_with([' ', '\t'])
    {
        let rest = rest.trim_start();
        let (keyword, tail) = match rest.split_once([' ', '\t']) {
            Some((k, t)) => (k, t.trim()),
            None => (rest, ""),
        };
        return match keyword {
            "file" if !tail.is_empty() => Command {
                id,
                span,
                kind: CommandKind::RunFile {
                    arg: tail.to_string(),
                },
            },
            "command" if !tail.is_empty() => Command {
                id,
                span,
                kind: CommandKind::RunCommand {
                    arg: tail.to_string(),
                },
            },
            "file" | "command" => invalid(
                diagnostics,
                codes::MISSING_ARGUMENT,
                "`run file` / `run command` 需要一个参数",
            ),
            _ => unknown_command(diagnostics, span, id),
        };
    }

    unknown_command(diagnostics, span, id)
}

fn unknown_command(diagnostics: &mut Vec<Diagnostic>, span: Span, id: usize) -> Command {
    diagnostics.push(Diagnostic::error(
        codes::UNKNOWN_COMMAND,
        span,
        "未知命令。当前支持：`@name` / `@name!` / `@continue` / `xrun <文件>` / `run file <脚本>` / `run command <命令>`",
    ));
    Command {
        id,
        span,
        kind: CommandKind::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> Parsed {
        let p = parse(src);
        assert!(!p.has_errors(), "unexpected errors: {:?}", p.diagnostics);
        p
    }

    fn err_codes(src: &str) -> Vec<&'static str> {
        let p = parse(src);
        p.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.code)
            .collect()
    }

    #[test]
    fn flat_commands() {
        let p = ok("xrun /bin/true\nrun command echo hi\nrun file a.sh\n");
        assert_eq!(p.program.nodes.len(), 3);
        assert_eq!(
            p.program.nodes[0].command.kind,
            CommandKind::Xrun {
                arg: "/bin/true".into()
            }
        );
        assert_eq!(
            p.program.nodes[1].command.kind,
            CommandKind::RunCommand {
                arg: "echo hi".into()
            }
        );
        assert_eq!(
            p.program.nodes[2].command.kind,
            CommandKind::RunFile { arg: "a.sh".into() }
        );
    }

    #[test]
    fn nesting_is_attached_to_previous_command() {
        let p =
            ok("@synchronizer\n  run command a\n  run command b\n@synchronizer!\nxrun /bin/true\n");
        assert_eq!(p.program.nodes.len(), 3);
        assert_eq!(p.program.nodes[0].children.len(), 2);
        assert_eq!(p.program.nodes[2].children.len(), 0);
    }

    #[test]
    fn identity_attach_and_unload() {
        let p = ok("@synchronizer\n@synchronizer!\n");
        assert_eq!(
            p.program.nodes[0].command.kind,
            CommandKind::Identity {
                name: "synchronizer".into(),
                unload: false
            }
        );
        assert_eq!(
            p.program.nodes[1].command.kind,
            CommandKind::Identity {
                name: "synchronizer".into(),
                unload: true
            }
        );
    }

    #[test]
    fn continue_is_a_command() {
        let p = ok("@continue\n");
        assert_eq!(p.program.nodes[0].command.kind, CommandKind::Continue);
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let p = ok("# 注释\n\n   \nxrun /bin/true\n");
        assert_eq!(p.program.nodes.len(), 1);
    }

    #[test]
    fn hash_inside_argument_is_not_a_comment() {
        let p = ok("run command echo 'a # b'\n");
        assert_eq!(
            p.program.nodes[0].command.kind,
            CommandKind::RunCommand {
                arg: "echo 'a # b'".into()
            }
        );
    }

    #[test]
    fn command_argument_keeps_inner_spacing() {
        let p = ok("run command  echo   a    b  \n");
        assert_eq!(
            p.program.nodes[0].command.kind,
            CommandKind::RunCommand {
                arg: "echo   a    b".into()
            }
        );
    }

    #[test]
    fn indentation_width_can_be_anything_but_must_match() {
        assert!(
            parse("xrun /bin/true\n    run command a\n    run command b\n")
                .diagnostics
                .is_empty()
        );
        let codes = err_codes("xrun /bin/true\n    run command a\n  run command b\n");
        assert!(codes.contains(&codes::INDENT_MISMATCH), "{codes:?}");
    }

    #[test]
    fn tabs_and_spaces_cannot_mix() {
        let codes = err_codes("xrun /bin/true\n \trun command a\n");
        assert!(codes.contains(&codes::INDENT_MIXED), "{codes:?}");
    }

    #[test]
    fn nested_blocks_are_legal() {
        let p = ok("@synchronizer\n    run command a\n        run command b\n    run command c\n");
        assert_eq!(p.program.nodes.len(), 1);
        assert_eq!(p.program.nodes[0].children.len(), 2);
        assert_eq!(p.program.nodes[0].children[0].children.len(), 1);
        let codes = err_codes("xrun /bin/true\n    run command a\n  run command b\n");
        assert!(codes.contains(&codes::INDENT_MISMATCH), "{codes:?}");
    }

    #[test]
    fn leading_indent_is_an_error() {
        let codes = err_codes("  xrun /bin/true\n  run command a\n");
        assert_eq!(codes, vec![codes::LEADING_INDENT]);
    }

    #[test]
    fn unknown_command_is_an_error() {
        let codes = err_codes("launch kitty\n");
        assert_eq!(codes, vec![codes::UNKNOWN_COMMAND]);
    }

    #[test]
    fn missing_arguments_are_errors() {
        assert_eq!(err_codes("xrun\n"), vec![codes::MISSING_ARGUMENT]);
        assert_eq!(err_codes("run file\n"), vec![codes::MISSING_ARGUMENT]);
        assert_eq!(err_codes("run command\n"), vec![codes::MISSING_ARGUMENT]);
        assert_eq!(err_codes("run\n"), vec![codes::MISSING_ARGUMENT]);
    }

    #[test]
    fn identity_syntax_is_checked() {
        assert_eq!(err_codes("@A @B\n"), vec![codes::IDENTITY_SYNTAX]);
        assert_eq!(err_codes("@\n"), vec![codes::IDENTITY_SYNTAX]);
        assert_eq!(err_codes("@continue!\n"), vec![codes::CONTINUE_UNLOAD]);
        assert_eq!(err_codes("@nope\n"), vec![codes::UNKNOWN_IDENTITY]);
    }

    #[test]
    fn spawn_commands_are_detected() {
        assert!(CommandKind::Xrun { arg: String::new() }.spawns_process());
        assert!(CommandKind::RunFile { arg: String::new() }.spawns_process());
        assert!(CommandKind::RunCommand { arg: String::new() }.spawns_process());
        assert!(!CommandKind::Continue.spawns_process());
        assert!(!CommandKind::Invalid.spawns_process());
    }
}
