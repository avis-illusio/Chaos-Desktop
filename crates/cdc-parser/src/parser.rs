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
    let kind = parse_kind(text, span, diagnostics);
    Command {
        id,
        span,
        kind,
        raw: text.to_string(),
    }
}

fn parse_kind(text: &str, span: Span, diagnostics: &mut Vec<Diagnostic>) -> CommandKind {
    if let Some(rest) = text.strip_prefix('@') {
        return parse_identity(rest, span, diagnostics);
    }

    if text == "xrun" {
        diagnostics.push(Diagnostic::error(
            codes::MISSING_ARGUMENT,
            span,
            "`xrun` 需要一个文件路径",
        ));
        return CommandKind::Invalid;
    }
    if let Some(rest) = text.strip_prefix("xrun")
        && rest.starts_with([' ', '\t'])
    {
        let arg = rest.trim();
        if arg.is_empty() {
            diagnostics.push(Diagnostic::error(
                codes::MISSING_ARGUMENT,
                span,
                "`xrun` 需要一个文件路径",
            ));
            return CommandKind::Invalid;
        }
        return CommandKind::Xrun {
            arg: arg.to_string(),
        };
    }

    if text == "run" {
        diagnostics.push(Diagnostic::error(
            codes::MISSING_ARGUMENT,
            span,
            "`run` 需要 `file` 或 `command`",
        ));
        return CommandKind::Invalid;
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
            "file" if !tail.is_empty() => CommandKind::RunFile {
                arg: tail.to_string(),
            },
            "command" if !tail.is_empty() => CommandKind::RunCommand {
                arg: tail.to_string(),
            },
            "file" | "command" => {
                diagnostics.push(Diagnostic::error(
                    codes::MISSING_ARGUMENT,
                    span,
                    "`run file` / `run command` 需要一个参数",
                ));
                CommandKind::Invalid
            }
            _ => unknown_command(diagnostics, span),
        };
    }

    unknown_command(diagnostics, span)
}

fn parse_identity(rest: &str, span: Span, diagnostics: &mut Vec<Diagnostic>) -> CommandKind {
    let (body, unload) = match rest.strip_suffix('!') {
        Some(b) => (b, true),
        None => (rest, false),
    };

    let name_end = body.find('{').unwrap_or(body.len());
    let name = &body[..name_end];
    let mut tail = &body[name_end..];

    let mut groups: Vec<&str> = Vec::new();
    while let Some(r) = tail.strip_prefix('{') {
        let Some(close) = r.find('}') else {
            diagnostics.push(Diagnostic::error(
                codes::SCOPE_SYNTAX,
                span,
                "`{` 没有配对的 `}`",
            ));
            return CommandKind::Invalid;
        };
        groups.push(&r[..close]);
        tail = &r[close + 1..];
    }
    if !tail.trim().is_empty() {
        diagnostics.push(Diagnostic::error(
            codes::IDENTITY_SYNTAX,
            span,
            "身份后面只能跟 `{作用域}{正则}` 或 `!`",
        ));
        return CommandKind::Invalid;
    }
    if groups.len() > 2 {
        diagnostics.push(Diagnostic::error(
            codes::SCOPE_SYNTAX,
            span,
            "身份最多只能有两个 `{…}`：先作用域、后正则",
        ));
        return CommandKind::Invalid;
    }

    let mut scope = ScopeSpec::All;
    let mut filter: Option<String> = None;
    match groups.as_slice() {
        [] => {}
        [g] if looks_like_scope(g) => match parse_scope(g) {
            Ok(s) => scope = s,
            Err(()) => {
                diagnostics.push(Diagnostic::error(
                    codes::SCOPE_SYNTAX,
                    span,
                    format!("作用域 `{{{g}}}` 写法不对"),
                ));
                return CommandKind::Invalid;
            }
        },
        [g] => filter = Some((*g).to_string()),
        [g0, g1] => {
            if !looks_like_scope(g0) {
                diagnostics.push(Diagnostic::error(
                    codes::SCOPE_SYNTAX,
                    span,
                    format!("第一个 `{{{g0}}}` 应该是作用域"),
                ));
                return CommandKind::Invalid;
            }
            match parse_scope(g0) {
                Ok(s) => scope = s,
                Err(()) => {
                    diagnostics.push(Diagnostic::error(
                        codes::SCOPE_SYNTAX,
                        span,
                        format!("作用域 `{{{g0}}}` 写法不对"),
                    ));
                    return CommandKind::Invalid;
                }
            }
            filter = Some((*g1).to_string());
        }
        _ => unreachable!(),
    }

    if name.is_empty() || name.contains(char::is_whitespace) {
        diagnostics.push(Diagnostic::error(
            codes::IDENTITY_SYNTAX,
            span,
            "身份写法必须是 `@name`、`@name{…}` 或 `@name!`，且一行只能有一个",
        ));
        return CommandKind::Invalid;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        diagnostics.push(Diagnostic::error(
            codes::IDENTITY_SYNTAX,
            span,
            "身份名只能包含字母、数字、`_`、`-`",
        ));
        return CommandKind::Invalid;
    }
    if !KNOWN_IDENTITIES.contains(&name) {
        let known = KNOWN_IDENTITIES.join("`, `@");
        diagnostics.push(Diagnostic::error(
            codes::UNKNOWN_IDENTITY,
            span,
            format!("未定义的身份 `@{name}`（当前只有 `@{known}`）"),
        ));
        return CommandKind::Invalid;
    }

    if let Some(f) = &filter
        && let Err(err) = regex::Regex::new(f)
    {
        diagnostics.push(Diagnostic::error(
            codes::REGEX_INVALID,
            span,
            format!("正则 `{f}` 无法编译：{err}"),
        ));
        return CommandKind::Invalid;
    }

    CommandKind::Identity {
        name: name.to_string(),
        unload,
        scope,
        filter,
    }
}

fn looks_like_scope(g: &str) -> bool {
    let t = g.trim();
    if !t
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | ',' | '*' | ' '))
    {
        return false;
    }
    t.contains([':', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9'])
}

fn parse_scope(g: &str) -> Result<ScopeSpec, ()> {
    fn num(s: &str) -> Result<i64, ()> {
        let t = s.trim();
        if t.is_empty() || t.contains('*') || t.contains(',') {
            return Err(());
        }
        t.parse::<i64>().map_err(|_| ())
    }

    let t = g.trim();
    if t.is_empty() || t == ":" {
        return Ok(ScopeSpec::All);
    }
    if let Some(rest) = t.strip_prefix(':') {
        return Ok(ScopeSpec::To(num(rest)?));
    }
    if let Some(rest) = t.strip_suffix(':') {
        return Ok(ScopeSpec::From(num(rest)?));
    }
    if let Some((a, b)) = t.split_once(':') {
        return Ok(ScopeSpec::Between(num(a)?, num(b)?));
    }
    Ok(ScopeSpec::Point(num(t)?))
}

fn unknown_command(diagnostics: &mut Vec<Diagnostic>, span: Span) -> CommandKind {
    diagnostics.push(Diagnostic::error(
        codes::UNKNOWN_COMMAND,
        span,
        "未知命令。当前支持：`@name` / `@name{…}` / `@name!` / `xrun <文件>` / `run file <脚本>` / `run command <命令>`",
    ));
    CommandKind::Invalid
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
                unload: false,
                scope: ScopeSpec::All,
                filter: None,
            }
        );
        assert_eq!(
            p.program.nodes[1].command.kind,
            CommandKind::Identity {
                name: "synchronizer".into(),
                unload: true,
                scope: ScopeSpec::All,
                filter: None,
            }
        );
    }

    #[test]
    fn continue_is_a_command() {
        let p = ok("@continue\n");
        assert_eq!(
            p.program.nodes[0].command.kind,
            CommandKind::Identity {
                name: "continue".into(),
                unload: false,
                scope: ScopeSpec::All,
                filter: None,
            }
        );
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
        assert_eq!(err_codes("@continue!\n"), Vec::<&str>::new());
        assert_eq!(err_codes("@nope\n"), vec![codes::UNKNOWN_IDENTITY]);
    }

    #[test]
    fn scoped_identity_parses() {
        let p = ok("@continue{:}{xrun}\nxrun /bin/true\nrun command echo hi\n");
        assert_eq!(
            p.program.nodes[0].command.kind,
            CommandKind::Identity {
                name: "continue".into(),
                unload: false,
                scope: ScopeSpec::All,
                filter: Some("xrun".into()),
            }
        );
    }

    #[test]
    fn scope_shapes() {
        let cases = [
            ("@synchronizer{:5}", ScopeSpec::To(5)),
            ("@synchronizer{5:}", ScopeSpec::From(5)),
            ("@synchronizer{3:7}", ScopeSpec::Between(3, 7)),
            ("@synchronizer{5}", ScopeSpec::Point(5)),
            ("@synchronizer{:}", ScopeSpec::All),
        ];
        for (src, want) in cases {
            let p = ok(&format!("{src}\n"));
            assert_eq!(
                p.program.nodes[0].command.kind,
                CommandKind::Identity {
                    name: "synchronizer".into(),
                    unload: false,
                    scope: want,
                    filter: None,
                },
                "{src}"
            );
        }
    }

    #[test]
    fn bad_regex_and_scope_are_errors() {
        assert_eq!(
            err_codes("@synchronizer{:}{[}\n"),
            vec![codes::REGEX_INVALID]
        );
        assert_eq!(
            err_codes("@synchronizer{1:2:3}\n"),
            vec![codes::SCOPE_SYNTAX]
        );
        assert_eq!(
            err_codes("@synchronizer{:}{(}\n"),
            vec![codes::REGEX_INVALID]
        );
        assert_eq!(
            err_codes("@synchronizer{a}{b}{c}\n"),
            vec![codes::SCOPE_SYNTAX]
        );
    }

    #[test]
    fn spawn_commands_are_detected() {
        assert!(CommandKind::Xrun { arg: String::new() }.spawns_process());
        assert!(CommandKind::RunFile { arg: String::new() }.spawns_process());
        assert!(CommandKind::RunCommand { arg: String::new() }.spawns_process());
        assert!(
            !CommandKind::Identity {
                name: "continue".into(),
                unload: false,
                scope: ScopeSpec::All,
                filter: None,
            }
            .spawns_process()
        );
        assert!(!CommandKind::Invalid.spawns_process());
    }
}
