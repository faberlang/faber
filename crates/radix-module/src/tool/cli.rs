//! Clap argument grammar and normalized command payload types for `radix`.
//!
//! Owns the `RadixCli` parser, subcommand enum, and per-command payload structs
//! (`CheckArgs`, `EmitArgs`, `VerifyArgs`, etc.). Shapes are defined with clap
//! derives and consumed by the handler layer. Delegates command execution and
//! all compiler work to the command handler functions in [`commands`].

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

const RADIX_AFTER_HELP: &str = include_str!("../../../../docs/help/radix-after-help.md");

/// Clap parser for the developer-facing `radix` binary.
#[derive(Parser, Debug)]
#[command(
    name = "radix",
    bin_name = "radix",
    about = "Faber compiler developer tool",
    after_long_help = RADIX_AFTER_HELP,
    version
)]
pub struct RadixCli {
    /// Developer command selected by the `radix` binary.
    #[command(subcommand)]
    pub command: RadixCommand,
}

/// Developer CLI command grammar for direct compiler phase inspection.
#[derive(Subcommand, Debug)]
pub enum RadixCommand {
    /// Tokenize source and output JSON
    Lex(InputArgs),
    /// Parse source and output AST as JSON
    Parse(InputArgs),
    /// Lower AST to `HIR` and output as JSON
    Hir(InputArgs),
    /// Lower checked `HIR` to `MIR` and output a deterministic text dump
    Mir(InputArgs),
    /// Validate and output normalized CLI IR as JSON
    CliIr(InputArgs),
    /// Run semantic analysis
    Check(CheckArgs),
    /// Run experimental `HIR` aspect verification
    Verify(VerifyArgs),
    /// Compile to target (rust, faber, ts, go, wasm-text, llvm-text, metal-text, wgsl-text)
    Emit(EmitArgs),
    /// Show supported targets and current capability notes
    Targets,
    /// Print the host ABI contract this Radix binary emits (`radix abi`)
    Abi(AbiArgs),
}

/// Shared source-input grammar for phase inspection commands.
#[derive(Args, Debug)]
pub struct InputArgs {
    /// Load a pre-analyzed HIR artifact from a .fhir file
    #[arg(long, value_name = "FILE")]
    pub load: Option<PathBuf>,

    /// Input file path, or '-' / omitted for stdin
    pub input: Vec<String>,
}

/// Parsed `abi` command payload after clap normalization.
#[derive(Args, Debug)]
pub struct AbiArgs {
    /// Output format for the ABI contract.
    #[arg(short = 'f', long = "format", value_enum, default_value_t = AbiFormat::Json)]
    pub format: AbiFormat,

    /// Optional output path. When omitted, writes to stdout.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,
}

/// ABI artifact format selected by `radix abi --format`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum AbiFormat {
    /// Canonical pretty `JSON` with a computed fingerprint.
    #[default]
    Json,
    /// Runtime-facing Rust constants artifact.
    Rust,
}

/// Parsed `check` command payload after clap normalization.
#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Print expanded phase-aware diagnostics instead of normal check output
    #[arg(long)]
    pub diagnostics: bool,

    /// TOML reader pack used to render diagnostics.
    #[arg(long = "diagnostics-locale", value_name = "FILE")]
    pub diagnostics_locale_pack: Option<PathBuf>,

    /// Code locale resolved to a reader pack; wins over source frontmatter.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Downgrade unresolved/import-driven semantic errors to warnings
    #[arg(long)]
    pub permissive: bool,

    /// Promote all warning diagnostics to errors
    #[arg(long)]
    pub deny_warnings: bool,

    /// Promote diagnostics matching this catalog code to errors (repeatable)
    #[arg(long = "deny", value_name = "CODE")]
    pub deny: Vec<String>,

    /// Force package checking mode
    #[arg(long)]
    pub package: bool,

    /// Input file or package path, or '-' / omitted for stdin
    pub input: Vec<String>,
}

/// Parsed `verify` command payload after clap normalization.
#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// Force package verification mode
    #[arg(long)]
    pub package: bool,

    /// Input file, or '-' / omitted for stdin
    pub input: Vec<String>,
}

/// Parsed `emit` command payload after clap normalization.
#[derive(Args, Debug)]
pub struct EmitArgs {
    /// Print expanded phase-aware diagnostics instead of normal emit diagnostics
    #[arg(long)]
    pub diagnostics: bool,

    /// TOML reader pack used to render diagnostics.
    #[arg(long = "diagnostics-locale", value_name = "FILE")]
    pub diagnostics_locale_pack: Option<PathBuf>,

    /// Code locale resolved to a reader pack; wins over source frontmatter.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Output target language
    #[arg(short = 't', long = "target", value_enum, default_value_t = CliTarget::HirRust)]
    pub target: CliTarget,

    /// Force package compilation mode
    #[arg(long)]
    pub package: bool,

    /// Print `GPU` reflection JSON instead of generated source for targets that expose a reflection sidecar.
    #[arg(long)]
    pub reflection: bool,

    /// Write the NVVM kernel descriptor JSON sidecar to this path (`llvm-text` target only).
    #[cfg(feature = "mir-llvm")]
    #[arg(long = "cuda-descriptor", value_name = "FILE")]
    pub cuda_descriptor: Option<PathBuf>,

    /// Optional output path for binary targets (`wasm`). When set, writes bytes instead of stdout.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Output mode: application (default, with @main) or library (public API, no entry point).
    #[arg(long = "output-mode", value_enum, default_value_t = CliOutputMode::Application)]
    pub output_mode: CliOutputMode,

    /// Explicit Swift module name for library-mode output.
    #[arg(long = "module-name")]
    pub module_name: Option<String>,

    /// Input file or package path, or '-' / omitted for stdin
    pub input: Vec<String>,
}

/// Diagnostic reporting style for command paths that support expanded records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticMode {
    /// Preserve the command's existing human terminal output.
    Normal,

    /// Print one expanded phase-aware record per normalized diagnostic.
    Diagnostics,
}

/// Process-independent check command contract used by command dispatch.
#[derive(Debug)]
pub struct CheckCommand {
    /// Input file path, `-`, or empty for stdin.
    pub input: Vec<String>,

    /// Whether the caller requested package mode.
    pub package: bool,

    /// Whether selected unresolved/import-driven semantic errors become warnings.
    pub permissive: bool,

    /// Promote all warning diagnostics to errors.
    pub deny_warnings: bool,

    /// Promote diagnostics matching these catalog codes to errors.
    pub deny_codes: Vec<String>,

    /// Reporting style for normalized diagnostics.
    pub diagnostic_mode: DiagnosticMode,

    /// Optional TOML reader pack used to render normalized diagnostics.
    pub diagnostics_locale_pack: Option<PathBuf>,

    /// Product-facing code locale selected by callers that resolve packs outside `radix`.
    pub locale: Option<String>,

    /// Project-wide diagnostic message locale; independent of code locale.
    pub diagnostics_locale: Option<String>,
}

/// Process-independent emit command contract used by command dispatch.
#[derive(Debug)]
pub struct EmitCommand {
    /// Input file path, `-`, or empty for stdin.
    pub input: Vec<String>,

    /// Whether the caller requested package mode.
    pub package: bool,

    /// Backend target for emitted source.
    pub target: crate::codegen::Target,

    /// Whether to print reflection JSON instead of generated source.
    pub reflection: bool,

    /// Optional path for the NVVM kernel descriptor JSON sidecar (`llvm-text` target only).
    #[cfg(feature = "mir-llvm")]
    pub cuda_descriptor: Option<PathBuf>,

    /// Optional output path for binary targets (`wasm`). When set, writes bytes instead of stdout.
    pub output: Option<PathBuf>,

    /// Reporting style for normalized diagnostics.
    pub diagnostic_mode: DiagnosticMode,

    /// Optional TOML reader pack used to render normalized diagnostics.
    pub diagnostics_locale_pack: Option<PathBuf>,

    /// Product-facing code locale selected by callers that resolve packs outside `radix`.
    pub locale: Option<String>,

    /// Project-wide diagnostic message locale; independent of code locale.
    pub diagnostics_locale: Option<String>,

    /// Output mode controlling access modifiers and entry-point emission.
    pub output_mode: crate::codegen::OutputMode,

    /// Explicit Swift module name for library-mode output.
    pub module_name: Option<String>,
}

/// Process-independent verify command contract used by command dispatch.
#[derive(Debug)]
pub struct VerifyCommand {
    /// Input file path, `-`, or empty for stdin.
    pub input: Vec<String>,

    /// Whether the caller requested package mode.
    pub package: bool,
}

/// Process-independent build command contract used by command dispatch.
#[derive(Debug)]
pub struct BuildCommand {
    /// Input file or package path.
    pub input: String,

    /// Directory where generated source should be written.
    pub out_dir: PathBuf,

    /// Whether the caller requested package mode.
    pub package: bool,

    /// Whether to request a release build profile when applicable.
    pub release: bool,

    /// Backend target for generated source.
    pub target: crate::codegen::Target,

    /// Whether the target came from an explicit CLI/config selection rather
    /// than a command default.
    pub target_explicit: bool,

    /// Promote all warning diagnostics to errors.
    pub deny_warnings: bool,

    /// Promote diagnostics matching these catalog codes to errors.
    pub deny_codes: Vec<String>,

    /// Product-facing code locale selected by callers that resolve packs outside `radix`.
    pub locale: Option<String>,

    /// Project-wide diagnostic message locale; independent of code locale.
    pub diagnostics_locale: Option<String>,
}

/// Output mode accepted by CLI flags for Swift target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum CliOutputMode {
    /// Standard application mode with `@main` entry point.
    #[default]
    Application,
    /// Library mode with `public` declarations and no `@main`.
    #[value(name = "library")]
    Library,
}

impl From<CliOutputMode> for crate::codegen::OutputMode {
    fn from(value: CliOutputMode) -> Self {
        match value {
            CliOutputMode::Application => crate::codegen::OutputMode::Application,
            CliOutputMode::Library => crate::codegen::OutputMode::Library,
        }
    }
}

/// Target names accepted by CLI flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum CliTarget {
    /// Rust backend.
    #[default]
    #[value(name = "rust")]
    HirRust,

    /// TypeScript backend.
    #[value(name = "ts", alias = "typescript")]
    HirTypeScript,

    /// Go backend.
    #[value(name = "go")]
    HirGo,

    /// Swift backend.
    #[value(name = "swift")]
    HirSwift,

    /// Python backend.
    #[value(name = "python")]
    HirPython,

    /// Haskell backend.
    #[value(name = "haskell")]
    HirHaskell,

    /// Canonical Faber re-emission backend.
    #[value(name = "faber", alias = "fab")]
    HirFaber,

    /// Supported-with-limitations `MIR`-backed WebAssembly text target.
    /// Emits `.wat` text only; `wasm` is an alias kept for compatibility
    /// (binary conversion is an external-tool step).
    #[value(name = "wasm-text", alias = "wat", alias = "wasm")]
    MirWasm,

    /// Supported-with-limitations `MIR`-backed `LLVM` text staging target.
    #[value(name = "llvm-text", alias = "llvm-ir", alias = "llvm")]
    MirLlvm,

    /// Native host executable product target. Distinct target name with NO
    /// alias spelling: `faber build/run --target llvm-host` owns the
    /// native-LLVM build/run orchestration; `-t llvm-text` stays emit-only.
    #[value(name = "llvm-host")]
    MirLlvmHost,

    /// Supported-with-limitations `MIR`-backed Metal Shading Language source target.
    #[value(name = "metal-text", alias = "metal")]
    MirMetal,

    /// Supported-with-limitations `MIR`-backed `WGSL` compute-shader source target.
    #[value(name = "wgsl-text", alias = "wgsl")]
    MirWgsl,

    /// Supported-with-limitations `MIR`-backed Racket validation target.
    #[value(name = "sexp", alias = "racket", alias = "lisp")]
    MirSexp,

    /// Legacy source-backed `MIR` package artifact runtime.
    ///
    /// This remains a compatibility target for `faber build/run`; it is not
    /// part of the ordinary source-emission target list.
    #[value(name = "scena", hide = true)]
    MirScena,

    /// Source-independent package `MIR` text image.
    #[value(name = "fmir-text")]
    MirFmir,

    /// Source-independent package `MIR` binary image.
    #[value(name = "fmir")]
    MirFmirBinary,

    /// Direct runnable binary wrapper with embedded package `MIR` image bytes.
    #[value(name = "fmir-bin")]
    MirFmirBundle,

    /// HIR artifact serialization target (postcard binary, not a codegen backend).
    #[value(name = "fhir")]
    HirFhir,
}

impl From<CliTarget> for crate::codegen::Target {
    fn from(value: CliTarget) -> Self {
        match value {
            CliTarget::HirRust => crate::codegen::Target::HirRust,
            CliTarget::HirTypeScript => crate::codegen::Target::HirTypeScript,
            CliTarget::HirGo => crate::codegen::Target::HirGo,
            CliTarget::HirSwift => crate::codegen::Target::HirSwift,
            CliTarget::HirPython => crate::codegen::Target::HirPython,
            CliTarget::HirHaskell => crate::codegen::Target::HirHaskell,
            CliTarget::HirFaber => crate::codegen::Target::HirFaber,
            CliTarget::MirWasm => crate::codegen::Target::MirWasm,
            CliTarget::MirLlvm => crate::codegen::Target::MirLlvm,
            CliTarget::MirLlvmHost => crate::codegen::Target::MirLlvmHost,
            CliTarget::MirMetal => crate::codegen::Target::MirMetal,
            CliTarget::MirWgsl => crate::codegen::Target::MirWgsl,
            CliTarget::MirSexp => crate::codegen::Target::MirSexp,
            CliTarget::MirScena => crate::codegen::Target::MirScena,
            CliTarget::MirFmir => crate::codegen::Target::MirFmir,
            CliTarget::MirFmirBinary => crate::codegen::Target::MirFmirBinary,
            CliTarget::MirFmirBundle => crate::codegen::Target::MirFmirBundle,
            CliTarget::HirFhir => crate::codegen::Target::HirFhir,
        }
    }
}
