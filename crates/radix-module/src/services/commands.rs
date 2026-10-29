//! Clap-free command request types shared by Faber and the developer CLI.

use std::path::PathBuf;

/// Diagnostic reporting style for command paths that support expanded records.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DiagnosticMode {
    /// Preserve the command's existing human terminal output.
    #[default]
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

    /// Reporting style for source diagnostics.
    pub diagnostic_mode: DiagnosticMode,
}
