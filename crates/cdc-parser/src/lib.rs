//! Chaos Desktop Command（`.cdc`）解析器。
//!
//! 只做：词法 → 缩进树 → 命令 AST → 诊断。
//! 不做：IO、路径存在性、执行。执行器由调用方注入。

pub mod ast;
pub mod parser;

pub use ast::{Command, CommandKind, Diagnostic, Node, Parsed, Program, Severity, Span, codes};
pub use parser::parse;
