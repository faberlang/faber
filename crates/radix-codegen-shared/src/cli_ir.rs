//! CLI IR types consumed by Rust codegen (target-independent surface).
//!
//! Owns `CliProgram`, `CliMode`, `CliOption`, `CliOperand`, `CliCommand`, and
//! related types. Target-independent: the same types serve both the CLI backend
//! and any future target that consumes CLI IR.

use radix_lexer::{Span, Symbol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliMode {
    NotCli,
    SingleCommand,
    Subcommand,
}

#[derive(Debug, Clone)]
pub struct CliProgram {
    pub name: String,
    pub entry_args: String,
    pub mode: CliMode,
    pub version: Option<String>,
    pub description: Option<String>,
    pub global_options: Vec<CliOption>,
    pub global_operands: Vec<CliOperand>,
    pub options: Vec<CliOption>,
    pub operands: Vec<CliOperand>,
    pub commands: Vec<CliCommand>,
    pub exit: Option<CliExit>,
}

#[derive(Debug, Clone)]
pub enum CliExit {
    Fixed(i64),
    Binding(String),
    Field { object: String, field: String },
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct CliCommand {
    pub path: Vec<String>,
    pub module_path: Option<Vec<String>>,
    pub function: String,
    pub function_symbol: Symbol,
    pub args_binding: Option<String>,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub options: Vec<CliOption>,
    pub operands: Vec<CliOperand>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CliOption {
    pub binding: String,
    pub binding_symbol: Symbol,
    pub ty: CliType,
    pub short: Option<String>,
    pub long: Option<String>,
    pub description: Option<String>,
    pub global: bool,
    pub default: Option<CliDefault>,
    pub flag: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CliOperand {
    pub binding: String,
    pub binding_symbol: Symbol,
    pub ty: CliType,
    pub rest: bool,
    pub description: Option<String>,
    pub global: bool,
    pub default: Option<CliDefault>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliType {
    Textus,
    Numerus,
    Fractus,
    Bivalens,
    Octeti,
    Ignotum,
    ListaTextus,
    ListaNumerus,
}

#[derive(Debug, Clone)]
pub enum CliDefault {
    Text(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    Nil,
    Expr(String),
}
