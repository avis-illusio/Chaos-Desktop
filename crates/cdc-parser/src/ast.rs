//! `.cdc` 的 AST 与诊断类型。
//!
//! 解析器只产出结构，不做任何 IO、不执行命令。

use std::fmt;

/// 源码位置（1 基）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub const fn new(line: u32, col: u32) -> Self {
        Self { line, col }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// 诊断码。码不可改，文案可改。
pub mod codes {
    // 语法 / 结构
    pub const INDENT_MISMATCH: &str = "E001";
    pub const INDENT_MIXED: &str = "E002";
    pub const UNKNOWN_COMMAND: &str = "E003";
    pub const MISSING_ARGUMENT: &str = "E004";
    pub const IDENTITY_SYNTAX: &str = "E005";
    pub const LEADING_INDENT: &str = "E007";
    pub const UNKNOWN_IDENTITY: &str = "E008";
    pub const REGEX_INVALID: &str = "E009";
    pub const SCOPE_SYNTAX: &str = "E010";
    pub const SCOPE_RANGE: &str = "E011";
    // 路径 / 存在性
    pub const BAD_PATH: &str = "E101";
    // 执行
    pub const SPAWN_FAILED: &str = "E201";
    pub const NON_ZERO_EXIT: &str = "E202";
    // 警告
    pub const CONTINUE_EMPTY: &str = "W004";
    pub const SKIPPED_UNSAFE: &str = "W005";
    pub const SCOPE_NO_MATCH: &str = "W006";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            span,
        }
    }

    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{}] {}: {}",
            self.severity.as_str(),
            self.code,
            self.span,
            self.message
        )
    }
}

/// 已定义的身份。未列出的身份名在解析期报 `E008`。
pub const KNOWN_IDENTITIES: &[&str] = &["continue", "synchronizer"];

/// 身份的作用域，`{…}` 里的写法。位置按执行单元内 1 基计，指令自身算第 1 条。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScopeSpec {
    /// `{:}` 或不写：指令自身到单元结束。
    #[default]
    All,
    /// `{5}`：单元内第 5 条。
    Point(i64),
    /// `{:5}`：指令自身到第 5 条。
    To(i64),
    /// `{5:}`：第 5 条到身份注销 / 单元结束。
    From(i64),
    /// `{3:7}`：第 3 条到第 7 条（端点无序）。
    Between(i64, i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandKind {
    /// `@name` / `@name{scope}{regex}` / `@name!`
    Identity {
        name: String,
        unload: bool,
        scope: ScopeSpec,
        /// 正则，匹配语句原文（去掉缩进的整行）。
        filter: Option<String>,
    },
    /// `xrun <file>` —— 直接执行一个可执行文件。
    Xrun { arg: String },
    /// `run file <path>` —— 用 `/bin/sh` 执行脚本文件。
    RunFile { arg: String },
    /// `run command <line>` —— fork 一个 `sh -c`。
    RunCommand { arg: String },
    /// 解析失败的占位。带 `Invalid` 的文件整体拒绝执行，执行器永远不会走到它。
    Invalid,
}

impl CommandKind {
    /// 是否会创建进程（安全模式要拦的就是这些）。
    pub const fn spawns_process(&self) -> bool {
        matches!(
            self,
            Self::Xrun { .. } | Self::RunFile { .. } | Self::RunCommand { .. }
        )
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Identity { .. } => "identity",
            Self::Xrun { .. } => "xrun",
            Self::RunFile { .. } => "run-file",
            Self::RunCommand { .. } => "run-command",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// 解析顺序编号（前序），诊断用。
    pub id: usize,
    pub span: Span,
    pub kind: CommandKind,
    /// 去掉缩进后的整行原文，供身份正则筛选使用。
    pub raw: String,
}

/// 一个节点 = 一条命令 + 它下面缩进的块（独立执行单元）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub command: Command,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Program {
    pub nodes: Vec<Node>,
}

impl Program {
    /// 前序遍历所有命令。
    pub fn walk(&self, f: &mut impl FnMut(&Node)) {
        fn go(nodes: &[Node], f: &mut impl FnMut(&Node)) {
            for n in nodes {
                f(n);
                go(&n.children, f);
            }
        }
        go(&self.nodes, f);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn sorted_diagnostics(&self) -> Vec<Diagnostic> {
        let mut d = self.diagnostics.clone();
        d.sort_by_key(|d| (d.span.line, d.span.col));
        d
    }
}
