//! `faber emit` CLI shapes, including the forma-backed Faber target.

use clap::{Args, ValueEnum};
use std::path::PathBuf;

/// Parsed `faber emit` command payload after clap normalization.
#[derive(Args, Debug)]
pub struct EmitArgs {
    /// Print expanded phase-aware diagnostics instead of normal emit diagnostics
    #[arg(long)]
    pub diagnostics: bool,

    /// Reader locale used to select a package-local reader pack.
    #[arg(long = "reader-locale", value_name = "LOCALE")]
    pub reader_locale: Option<String>,

    /// Output target language
    #[arg(short = 't', long = "target", value_enum, default_value_t = FaberCliTarget::Rust)]
    pub target: FaberCliTarget,

    /// Force package compilation mode
    #[arg(long)]
    pub package: bool,

    /// Run the target language's formatter on the emitted code (requires the formatter to be installed: rustfmt, gofmt, prettier, etc.)
    #[arg(long)]
    pub format: bool,

    /// Run a linter and auto-fix issues where possible.
    /// This is independent of --format; use both flags if you want formatting + linting.
    #[arg(long)]
    pub linter: bool,

    /// Emit GPU reflection sidecar JSON (WebGPU probe targets such as `wgsl-text`)
    #[arg(long)]
    pub reflection: bool,

    /// Optional output path for binary targets (`wasm`). When set, writes bytes instead of stdout.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Input file or package path, or '-' / omitted for stdin
    pub input: Vec<String>,
}

/// Target names accepted by `faber emit`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum FaberCliTarget {
    /// Rust backend.
    #[default]
    Rust,

    /// Canonical Faber re-emission.
    #[value(name = "faber", alias = "fab")]
    Canonical,

    /// TypeScript backend.
    #[value(name = "ts", alias = "typescript")]
    TypeScript,

    /// Go backend.
    Go,

    /// Experimental MIR-backed WebAssembly text target.
    #[value(name = "wasm-text", alias = "wat")]
    WasmText,

    /// Experimental MIR-backed WebAssembly binary target.
    #[value(name = "wasm")]
    Wasm,

    /// Experimental MIR-backed LLVM text target.
    #[value(name = "llvm-text", alias = "llvm-ir", alias = "llvm")]
    LlvmText,

    /// Experimental MIR-backed Metal Shading Language source probe.
    #[value(name = "metal-text", alias = "metal")]
    MetalText,

    /// Experimental MIR-backed WGSL compute-shader source probe.
    #[value(name = "wgsl-text", alias = "wgsl")]
    WgslText,

    /// Experimental MIR-backed Racket s-expression probe.
    #[value(name = "sexp", alias = "racket", alias = "lisp")]
    Sexp,
}

impl FaberCliTarget {
    /// Return true when emit should reject package mode for canonical Faber output.
    pub fn is_faber(self) -> bool {
        matches!(self, Self::Canonical)
    }

    /// Convert to the radix backend target.
    pub fn to_radix(self) -> radix::codegen::Target {
        match self {
            FaberCliTarget::Canonical => radix::codegen::Target::Faber,
            FaberCliTarget::Rust => radix::codegen::Target::Rust,
            FaberCliTarget::TypeScript => radix::codegen::Target::TypeScript,
            FaberCliTarget::Go => radix::codegen::Target::Go,
            FaberCliTarget::WasmText => radix::codegen::Target::WasmText,
            FaberCliTarget::Wasm => radix::codegen::Target::Wasm,
            FaberCliTarget::LlvmText => radix::codegen::Target::LlvmText,
            FaberCliTarget::MetalText => radix::codegen::Target::MetalText,
            FaberCliTarget::WgslText => radix::codegen::Target::WgslText,
            FaberCliTarget::Sexp => radix::codegen::Target::Sexp,
        }
    }
}
