//! `faber emit` CLI shapes, including the HIR-backed Faber target.

use clap::{Args, ValueEnum};
use radix::module::codegen::Target;
use std::path::PathBuf;

/// Parsed `faber emit` command payload after clap normalization.
#[derive(Args, Debug)]
pub struct EmitArgs {
    /// Use compact diagnostics instead of the default message and help
    #[arg(long)]
    pub no_diagnostics: bool,

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

    #[command(flatten)]
    pub library: super::LibraryHomeArgs,
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
    /// Novelty Odin program (default-off `hir-odin` feature).
    #[value(name = "odin")]
    HirOdin,
    /// Novelty C11 program (default-off `hir-c` feature).
    #[value(name = "c")]
    HirC,

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

    /// Experimental EVM deployment bytecode (default-off mir-evm feature).
    #[value(name = "evm")]
    MirEvm,

    /// Swift backend.
    #[value(name = "swift")]
    HirSwift,

    /// HIR artifact serialization target.
    #[value(name = "fhir")]
    HirFhir,

    /// Native host executable product target.
    #[value(name = "llvm-host")]
    MirLlvmHost,

    /// Public Cranelift native object (host triple); `faber build --target clif` links it.
    #[value(name = "clif", alias = "cranelift")]
    MirClif,

    /// In-memory Cranelift engine: `faber run --target clif-jit` only. Named
    /// here so `build` and `emit` can refuse it by name; it lowers exactly as
    /// `clif` does.
    #[value(name = "clif-jit", hide = true)]
    ClifJit,

    /// The MIR interpreter: `faber run --target runner` only, the default
    /// `faber run` route. Named here so `build` and `emit` can refuse it by
    /// name; it interprets MIR and emits nothing.
    #[value(name = "runner", hide = true)]
    Runner,

    /// Legacy source-backed MIR package artifact runtime.
    #[value(name = "scena", hide = true)]
    MirScena,

    /// Source-independent package MIR text image.
    #[value(name = "fmir-text")]
    MirFmir,

    /// Source-independent package MIR binary image.
    #[value(name = "fmir")]
    MirFmirBinary,
}

impl FaberCliTarget {
    /// Return true when emit should reject package mode for canonical Faber output.
    pub fn is_faber(self) -> bool {
        matches!(self, Self::HirFaber)
    }

    /// Whether this spelling names an engine that runs in memory and has no
    /// build or emit output: `clif-jit` (Cranelift, in this process) and
    /// `runner` (the MIR interpreter).
    pub fn is_in_memory_engine(self) -> bool {
        matches!(self, Self::ClifJit | Self::Runner)
    }

    /// Convert to the radix backend target.
    pub fn to_radix(self) -> Target {
        match self {
            FaberCliTarget::HirFaber => Target::HirFaber,
            FaberCliTarget::HirRust => Target::HirRust,
            FaberCliTarget::HirTypeScript => Target::HirTypeScript,
            FaberCliTarget::HirGo => Target::HirGo,
            FaberCliTarget::HirPython => Target::HirPython,
            FaberCliTarget::HirHaskell => Target::HirHaskell,
            FaberCliTarget::HirPascal => Target::HirPascal,
            FaberCliTarget::HirCobol => Target::HirCobol,
            FaberCliTarget::HirFortran => Target::HirFortran,
            FaberCliTarget::HirOdin => Target::HirOdin,
            FaberCliTarget::HirC => Target::HirC,
            FaberCliTarget::MirWasm => Target::MirWasm,
            FaberCliTarget::MirLlvm => Target::MirLlvm,
            FaberCliTarget::MirMetal => Target::MirMetal,
            FaberCliTarget::MirWgsl => Target::MirWgsl,
            FaberCliTarget::MirSexp => Target::MirSexp,
            FaberCliTarget::MirBasic => Target::MirBasic,
            FaberCliTarget::MirEvm => Target::MirEvm,
            FaberCliTarget::HirSwift => Target::HirSwift,
            FaberCliTarget::HirFhir => Target::HirFhir,
            FaberCliTarget::MirLlvmHost => Target::MirLlvmHost,
            FaberCliTarget::MirClif | FaberCliTarget::ClifJit => Target::MirClif,
            // The interpreter route: `faber run --target runner` and plain
            // `faber run` share it. Unreachable through `build`/`emit`, which
            // refuse the spelling before mapping (see `is_in_memory_engine`).
            FaberCliTarget::Runner => Target::MirStepper,
            FaberCliTarget::MirScena => Target::MirScena,
            FaberCliTarget::MirFmir => Target::MirFmir,
            FaberCliTarget::MirFmirBinary => Target::MirFmirBinary,
        }
    }
}
