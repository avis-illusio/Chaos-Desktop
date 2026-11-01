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
    pub const CONTINUE_UNLOAD: &str = "E006";
    pub const LEADING_INDENT: &str = "E007";
    pub const UNKNOWN_IDENTITY: &str = "E008";
    // 路径 / 存在性
    pub const BAD_PATH: &str = "E101";
    // 执行
    pub const SPAWN_FAILED: &str = "E201";
    pub const NON_ZERO_EXIT: &str = "E202";
    // 警告
    pub const EMPTY_UNLOAD: &str = "W001";
    pub const UNCLOSED_IDENTITY: &str = "W002";
    pub const UNLOAD_INHERITED: &str = "W003";
    pub const CONTINUE_EMPTY: &str = "W004";
    pub const SKIPPED_UNSAFE: &str = "W005";
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandKind {
    /// `@name` 或 `@name!`
    Identity { name: String, unload: bool },
    /// `@continue`
    Continue,
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
            Self::Continue => "continue",
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
