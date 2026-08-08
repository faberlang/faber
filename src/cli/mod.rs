//! Clap command shapes for the `faber` binary.

mod emit;

use clap::{Parser, Subcommand};
pub use emit::{EmitArgs, FaberCliTarget};
use std::path::PathBuf;

const FABER_AFTER_HELP: &str = include_str!("../../docs/help/faber-after-help.md");

/// Root parser for the `faber` binary.
#[derive(Parser, Debug)]
#[command(
    name = "faber",
    bin_name = "faber",
    about = "Faber project and package tool",
    after_long_help = FABER_AFTER_HELP,
    args_conflicts_with_subcommands = true,
    version
)]
pub struct Cli {
    /// Execute Faber source via the MIR stepper (one-liner)
    #[arg(short = 'c', long = "command")]
    pub eval_source: Option<String>,

    /// Arguments for `-c` / `--command` (after `--`)
    #[arg(last = true, allow_hyphen_values = true, hide = true)]
    pub eval_args: Vec<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// User-facing `faber` subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Compile a file or package and write output to disk
    #[command(override_usage = "faber build [OPTIONS] <INPUT>")]
    Build(BuildArgs),

    /// Show supported targets and current capability notes
    Targets,

    /// Run semantic analysis on a file or package
    Check(CheckArgs),

    /// Run aspect verification on a single file
    Verify(radix::tool::VerifyArgs),

    /// Verify a library package's target binding manifest
    VerifyLibrary(VerifyLibraryArgs),

    /// Create a new Faber package
    Init(InitArgs),

    /// Install a Faber library package into the Cista store
    #[command(
        long_about = "Install a Faber library package into the Cista package store ($CISTAE_HOME / ~/.faber/cistae) and rewrite faber.lock when a project is present.\n\n`faber install --path <package-root>` installs a local package containing cista.toml. `faber install <git-url>` clones to a temporary checkout, requires cista.toml, and installs the same store path. `faber install <name>@<version>` installs an exact registry pin via Cista using --registry or CISTA_REGISTRY; bare names fail closed."
    )]
    Install(InstallArgs),

    /// Explain a Faber glyph, keyword, or grammar term
    Explain(ExplainArgs),

    /// Build (if needed) and run a compiled package
    Run(RunArgs),

    /// Run Faber source through the interpreter (single file, package, or archive)
    Script(ScriptArgs),

    /// Interactive MIR stepper REPL (accumulating cells, re-lowers each line)
    Repl(ReplArgs),

    /// Run proba cases on the MIR stepper (no Cargo / rustc on the package)
    Test(TestArgs),

    /// Internal FMIR image runner used by generated executable bundles
    #[command(name = "__fmir-run", hide = true)]
    FmirRun(FmirRunArgs),

    /// Tokenize source and output JSON (compatibility alias for `radix lex`)
    Lex(radix::tool::InputArgs),

    /// Parse source and output AST as JSON (compatibility alias for `radix parse`)
    Parse(radix::tool::InputArgs),

    /// Lower AST to HIR and output as JSON (compatibility alias for `radix hir`)
    Hir(radix::tool::InputArgs),

    /// Lower checked HIR to MIR and output a deterministic text dump (compatibility alias for `radix mir`)
    Mir(radix::tool::InputArgs),

    /// Validate and output normalized CLI IR as JSON (compatibility alias for `radix cli-ir`)
    CliIr(radix::tool::InputArgs),

    /// Compile to target for stdout (compatibility alias for `radix emit`)
    Emit(EmitArgs),

    /// Format Faber source (author mode by default)
    Format(FormatArgs),

    /// Script host introspection (kernel manifest)
    Host(crate::commands::host::HostArgs),

    /// Inspect model file metadata (safetensors)
    Model(ModelArgs),
}

/// Arguments for `faber format`.
#[derive(clap::Args, Debug)]
pub struct FormatArgs {
    /// Files or directories to format (default: current package directory)
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Re-emit in the given reader-locale surface (HIR-backed round-trip;
    /// `en` is the default product surface and `la` remains available for
    /// heritage sources. No flag keeps
    /// author mode.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Check formatting without writing; exit 1 if any file would change
    #[arg(long)]
    pub check: bool,

    /// Write formatted output to stdout instead of updating files
    #[arg(long, conflicts_with = "check")]
    pub stdout: bool,

    /// Path to forma.toml override (schema deferred)
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
}

/// Arguments for `faber build`.
#[derive(clap::Args, Debug)]
pub struct BuildArgs {
    /// Output target language
    #[arg(short = 't', long = "target", value_enum)]
    pub target: Option<radix::tool::CliTarget>,

    /// Output directory for generated files
    #[arg(short = 'o', long = "out-dir", default_value = ".")]
    pub out_dir: PathBuf,

    /// Force package compilation mode
    #[arg(long)]
    pub package: bool,

    /// Build release profile instead of debug
    #[arg(long)]
    pub release: bool,

    /// Run the target language's formatter on the emitted code before writing files
    #[arg(long)]
    pub format: bool,

    /// Run a linter and auto-fix issues where possible before writing files.
    /// This is independent of --format; use both flags if you want formatting + linting.
    #[arg(long)]
    pub linter: bool,

    /// Promote all warning diagnostics to errors
    #[arg(long)]
    pub deny_warnings: bool,

    /// Promote diagnostics matching this catalog code to errors (repeatable)
    #[arg(long = "deny", value_name = "CODE")]
    pub deny: Vec<String>,

    /// Code locale used to select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

    /// Input file or package path
    #[arg(value_name = "INPUT")]
    pub input: String,
}

/// Arguments for `faber check`.
#[derive(clap::Args, Debug)]
pub struct CheckArgs {
    /// Print expanded phase-aware diagnostics instead of normal check output
    #[arg(long)]
    pub diagnostics: bool,

    /// Code locale used to select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

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

/// Arguments for `faber init`.
#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Target directory for the new package
    #[arg(default_value = ".")]
    pub path: PathBuf,
}

/// Arguments for `faber install`.
#[derive(clap::Args, Debug)]
pub struct InstallArgs {
    /// Local package root containing `cista.toml` (product path → Cista store)
    #[arg(long, conflicts_with = "library")]
    pub path: Option<PathBuf>,

    /// Shared cista package store; falls back to `CISTAE_HOME`, then `~/.faber/cistae`
    #[arg(long)]
    pub store: Option<PathBuf>,

    /// Project root for faber.lock rewrite; defaults to cwd when faber.toml exists
    #[arg(long)]
    pub project: Option<PathBuf>,

    /// Target language for store install (default: rust)
    #[arg(long, default_value = "rust")]
    pub target_language: String,

    /// Local/dev Cista registry root for exact name@version installs; falls back to `CISTA_REGISTRY`
    #[arg(long)]
    pub registry: Option<PathBuf>,

    /// Exact registry package pin (name@version) or git URL; default installs into the Cista store
    #[arg(required_unless_present = "path")]
    pub library: Option<String>,
}

/// Arguments for `faber verify-library`.
#[derive(clap::Args, Debug)]
pub struct VerifyLibraryArgs {
    /// Target binding surface to verify.
    #[arg(long, default_value = "rust")]
    pub target: String,

    /// Library package root or faber.toml path.
    pub input: PathBuf,
}

/// Arguments for `faber explain`.
#[derive(clap::Args, Debug)]
pub struct ExplainArgs {
    /// Emit a machine-readable JSON explanation
    #[arg(long, requires = "term")]
    pub json: bool,

    /// Message language for diagnostic explanation text
    #[arg(
        long = "diagnostics-locale",
        value_name = "LOCALE",
        requires = "term",
        conflicts_with_all = ["search", "list", "category"]
    )]
    pub diagnostics_locale: Option<String>,

    /// Search across explain entries and show ranked matches
    #[arg(long, conflicts_with_all = ["list", "category", "term", "json"])]
    pub search: Option<String>,

    /// List canonical explain terms
    #[arg(long, conflicts_with_all = ["search", "category", "term", "json"])]
    pub list: bool,

    /// List canonical and legacy entries in a category
    #[arg(long, conflicts_with_all = ["search", "list", "term", "json"])]
    pub category: Option<String>,

    /// Term, alias, or legacy spelling to explain
    #[arg(conflicts_with_all = ["search", "list", "category"])]
    pub term: Option<String>,
}

/// Arguments for `faber repl`.
#[derive(clap::Args, Debug)]
pub struct ReplArgs {
    /// Arguments available to script cells via `processus.argumenta()`
    #[arg(allow_hyphen_values = true, last = true)]
    pub args: Vec<String>,
}

/// Arguments for `faber run`.
#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// Package path to run (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Code locale used to select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

    /// Runtime target to build and execute. When omitted, the manifest
    /// `[build] target` wins; otherwise the implicit portable default
    /// (FHIR package → FMIR run) applies.
    #[arg(short = 't', long = "target", value_enum)]
    pub target: Option<radix::tool::CliTarget>,

    /// Backend selection for device-capable packages (differentiable-GPU
    /// campaign N1.1). When omitted, the manifest `[device] backend` wins;
    /// otherwise the default `auto` applies. Precedence: CLI > manifest >
    /// `auto`. An explicit `metal`/`cuda` request never silently falls back:
    /// it fails closed before launch (`E_BACKEND_UNAVAILABLE`) when the
    /// backend is not admitted on this machine, and a package with no device
    /// program fails closed ("package has no device program").
    #[arg(long = "backend", value_enum, value_name = "BACKEND")]
    pub backend: Option<BackendSelection>,

    /// Run the release binary
    #[arg(long)]
    pub release: bool,

    /// Force in-process MIR interpretation for scripts or supported packages
    #[arg(long, conflicts_with = "compile")]
    pub interpret: bool,

    /// Force compile-to-Rust execution even for single `.fab` files
    #[arg(long, conflicts_with = "interpret")]
    pub compile: bool,

    /// Promote all warning diagnostics to errors
    #[arg(long)]
    pub deny_warnings: bool,

    /// Promote diagnostics matching this catalog code to errors (repeatable)
    #[arg(long = "deny", value_name = "CODE")]
    pub deny: Vec<String>,

    /// Arguments passed to the executed program (after --)
    #[arg(allow_hyphen_values = true, last = true)]
    pub args: Vec<String>,
}

/// Device backend selection for `faber run --backend` (N1.1).
///
/// Mirrors the frozen FMIR `device.selection` surface
/// (`faber::device::DeviceSelection: auto | metal | cuda`); the CLI converts
/// through [`BackendSelection::selection`].
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendSelection {
    /// Resolve against host capability probes; fail closed when the machine
    /// admits zero or more than one backend.
    Auto,
    /// Select Metal explicitly; never silently falls back.
    Metal,
    /// Select CUDA explicitly; never silently falls back.
    Cuda,
}

impl BackendSelection {
    /// Convert to the packaged `faber::device` selection request.
    #[must_use]
    pub fn selection(self) -> faber::device::DeviceSelection {
        match self {
            Self::Auto => faber::device::DeviceSelection::Auto,
            Self::Metal => faber::device::DeviceSelection::Metal,
            Self::Cuda => faber::device::DeviceSelection::Cuda,
        }
    }
}

/// Arguments for the hidden FMIR image runner.
#[derive(clap::Args, Debug)]
pub struct FmirRunArgs {
    /// FMIR image path to execute.
    pub image: PathBuf,

    /// Backend selection override for the image-runner route (N1.1). When
    /// omitted, the image's declared `device.selection` wins; otherwise
    /// `auto` applies.
    #[arg(long = "backend", value_enum, value_name = "BACKEND")]
    pub backend: Option<BackendSelection>,

    /// Arguments passed to the FMIR program.
    #[arg(allow_hyphen_values = true, last = true)]
    pub args: Vec<String>,
}

/// Arguments for `faber script`.
///
/// `script` always interprets source through the MIR stepper or package-MIR
/// runner; it never compiles to Rust or invokes Cargo. See `commands::script`.
#[derive(clap::Args, Debug)]
pub struct ScriptArgs {
    /// Source path to interpret: a `.fab` file, package directory, `faber.toml`,
    /// package entry file, or `.zip` archive
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Arguments passed to the interpreted program (after --)
    #[arg(allow_hyphen_values = true, last = true)]
    pub args: Vec<String>,
}

/// Arguments for `faber test` (MIR stepper — no Cargo / rustc on the package).
#[derive(clap::Args, Debug)]
pub struct TestArgs {
    /// Package path or single `.fab` / `.proba` file to test
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Code locale used to select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

    /// Promote all warning diagnostics to errors
    #[arg(long)]
    pub deny_warnings: bool,

    /// Promote diagnostics matching this catalog code to errors (repeatable)
    #[arg(long = "deny", value_name = "CODE")]
    pub deny: Vec<String>,

    /// Substring filter on case path (`suite/name`) or title.
    /// Positional form: `faber test . smoke`. Long form: `faber test . --filter smoke`.
    #[arg(long = "filter", value_name = "FILTER")]
    pub filter_flag: Option<String>,

    /// Positional harness filter (same meaning as `--filter`)
    #[arg(value_name = "FILTER")]
    pub filter: Option<String>,

    /// Load only `*.proba` sources matching this path pattern (repeatable).
    /// Patterns are relative to the package source root (`*` wildcards allowed).
    /// Bare stems match `name.proba` (e.g. `--include math`).
    #[arg(long = "include", value_name = "PATTERN")]
    pub include: Vec<String>,

    /// Skip `*.proba` sources matching this path pattern (repeatable).
    #[arg(long = "exclude", value_name = "PATTERN")]
    pub exclude: Vec<String>,

    /// Select tests by source-level proba name
    #[arg(long)]
    pub name: Option<String>,

    /// Select tests by source-level probandum suite path, joined with `/`
    #[arg(long)]
    pub suite: Option<String>,

    /// Select tests by source-level tag modifier
    #[arg(long)]
    pub tag: Option<String>,

    /// Require the filter to match the full case path or title exactly
    #[arg(long)]
    pub exact: bool,

    /// Show test output (do not capture stdout/stderr from test bodies)
    #[arg(long)]
    pub nocapture: bool,

    /// Reserved (serial execution only; ignored by the stepper runner)
    #[arg(long, value_name = "N")]
    pub test_threads: Option<usize>,

    /// Not supported on the stepper runner (exits with an error)
    #[arg(long, conflicts_with = "include_ignored")]
    pub ignored: bool,

    /// Include selection-filtered cases as skips in the report
    #[arg(long, conflicts_with = "ignored")]
    pub include_ignored: bool,
}

/// Arguments for `faber model`.
#[derive(clap::Args, Debug)]
pub struct ModelArgs {
    /// Subcommand for model operations.
    #[command(subcommand)]
    pub command: ModelCommand,
}

/// Model subcommands.
#[derive(clap::Subcommand, Debug)]
pub enum ModelCommand {
    /// Inspect safetensors file header metadata
    Inspect(ModelInspectArgs),
}

/// Arguments for `faber model inspect`.
#[derive(clap::Args, Debug)]
pub struct ModelInspectArgs {
    /// Path to a .safetensors model file
    pub path: std::path::PathBuf,
}
