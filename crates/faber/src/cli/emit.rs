//! `faber emit` CLI shapes, including the forma-backed Faber target.

use clap::{Args, ValueEnum};
use std::path::PathBuf;

/// Parsed `faber emit` command payload after clap normalization.
#[derive(Args, Debug)]
pub struct EmitArgs {
    /// Print expanded phase-aware diagnostics instead of normal emit diagnostics
    #[arg(long)]
    pub diagnostics: bool,

    /// Code locale used to select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

    /// Output target language
    #[arg(short = 't', long = "target", value_enum, default_value_t = FaberCliTarget::HirRust)]
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

    /// Optional output path for target artifacts. When set, writes to the
    /// path instead of stdout.
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
    #[value(name = "rust")]
    HirRust,

    /// Canonical Faber re-emission.
    #[value(name = "faber", alias = "fab")]
    HirFaber,

    /// TypeScript backend.
    #[value(name = "ts", alias = "typescript")]
    HirTypeScript,

    /// Go backend.
    #[value(name = "go")]
    HirGo,

    /// Python backend.
    #[value(name = "python")]
    HirPython,

    /// Haskell backend.
    #[value(name = "haskell")]
    HirHaskell,

    /// Novelty Free Pascal program (default-off `hir-pascal` feature).
    #[value(name = "pascal", alias = "pas")]
    HirPascal,
    /// Novelty GnuCOBOL 3.2 free-format source (default-off `hir-cobol` feature).
    #[value(name = "cobol", alias = "cob")]
    HirCobol,
    /// Novelty Fortran 2008 program (default-off `hir-fortran` feature).
    #[value(name = "fortran", alias = "f90")]
    HirFortran,

    /// Experimental MIR-backed WebAssembly text target. Emits `.wat` text
    /// only; `wasm` is an alias kept for compatibility (binary conversion is
    /// an external-tool step).
    #[value(name = "wasm-text", alias = "wat", alias = "wasm")]
    MirWasm,

    /// Experimental MIR-backed LLVM text target.
    #[value(name = "llvm-text", alias = "llvm-ir", alias = "llvm")]
    MirLlvm,

    /// Experimental MIR-backed Metal Shading Language source probe.
    #[value(name = "metal-text", alias = "metal")]
    MirMetal,

    /// Experimental MIR-backed WGSL compute-shader source probe.
    #[value(name = "wgsl-text", alias = "wgsl")]
    MirWgsl,

    /// Experimental MIR-backed Racket s-expression probe.
    #[value(name = "sexp", alias = "racket", alias = "lisp")]
    MirSexp,

    /// Novelty MIR-backed classic line-numbered BASIC listing (default-off `mir-basic` feature).
    #[value(name = "basic", alias = "bas")]
    MirBasic,

    /// Swift backend.
    #[value(name = "swift")]
    HirSwift,

    /// HIR artifact serialization target.
    #[value(name = "fhir")]
    HirFhir,

    /// Native host executable product target.
    #[value(name = "llvm-host")]
    MirLlvmHost,

    /// Legacy source-backed MIR package artifact runtime.
    #[value(name = "scena", hide = true)]
    MirScena,

    /// Source-independent package MIR text image.
    #[value(name = "fmir-text")]
    MirFmir,

    /// Source-independent package MIR binary image.
    #[value(name = "fmir")]
    MirFmirBinary,

    /// Direct runnable binary wrapper with embedded package MIR image bytes.
    #[value(name = "fmir-bin")]
    MirFmirBundle,
}

impl FaberCliTarget {
    /// Return true when emit should reject package mode for canonical Faber output.
    pub fn is_faber(self) -> bool {
        matches!(self, Self::HirFaber)
    }

    /// Convert to the radix backend target.
    pub fn to_radix(self) -> radix::module::codegen::Target {
        match self {
            FaberCliTarget::HirFaber => radix::module::codegen::Target::HirFaber,
            FaberCliTarget::HirRust => radix::module::codegen::Target::HirRust,
            FaberCliTarget::HirTypeScript => radix::module::codegen::Target::HirTypeScript,
            FaberCliTarget::HirGo => radix::module::codegen::Target::HirGo,
            FaberCliTarget::HirPython => radix::module::codegen::Target::HirPython,
            FaberCliTarget::HirHaskell => radix::module::codegen::Target::HirHaskell,
            FaberCliTarget::HirPascal => radix::module::codegen::Target::HirPascal,
            FaberCliTarget::HirCobol => radix::module::codegen::Target::HirCobol,
            FaberCliTarget::HirFortran => radix::module::codegen::Target::HirFortran,
            FaberCliTarget::MirWasm => radix::module::codegen::Target::MirWasm,
            FaberCliTarget::MirLlvm => radix::module::codegen::Target::MirLlvm,
            FaberCliTarget::MirMetal => radix::module::codegen::Target::MirMetal,
            FaberCliTarget::MirWgsl => radix::module::codegen::Target::MirWgsl,
            FaberCliTarget::MirSexp => radix::module::codegen::Target::MirSexp,
            FaberCliTarget::MirBasic => radix::module::codegen::Target::MirBasic,
            FaberCliTarget::HirSwift => radix::module::codegen::Target::HirSwift,
            FaberCliTarget::HirFhir => radix::module::codegen::Target::HirFhir,
            FaberCliTarget::MirLlvmHost => radix::module::codegen::Target::MirLlvmHost,
            FaberCliTarget::MirScena => radix::module::codegen::Target::MirScena,
            FaberCliTarget::MirFmir => radix::module::codegen::Target::MirFmir,
            FaberCliTarget::MirFmirBinary => radix::module::codegen::Target::MirFmirBinary,
            FaberCliTarget::MirFmirBundle => radix::module::codegen::Target::MirFmirBundle,
        }
    }
}
