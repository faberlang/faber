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
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub eval_args: Vec<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// User-facing `faber` subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Compile a file or package and write output to disk
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

    /// Install or update a public Faber source library under FABER_LIBRARY_HOME
    Install(InstallArgs),

    /// Explain a Faber glyph, keyword, or grammar term
    Explain(ExplainArgs),

    /// Build (if needed) and run a compiled package
    Run(RunArgs),

    /// Run Faber source through the interpreter (single file, package, or archive)
    Script(ScriptArgs),

    /// Interactive MIR stepper REPL (accumulating cells, re-lowers each line)
    Repl(ReplArgs),

    /// Run package tests via the generated Rust test harness (Cargo-backed)
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

    /// Validate and output normalized CLI IR as JSON (compatibility alias for `radix cli-ir`)
    CliIr(radix::tool::InputArgs),

    /// Compile to target for stdout (compatibility alias for `radix emit`)
    Emit(EmitArgs),

    /// Format Faber source (author mode by default)
    Format(FormatArgs),

    /// Script host introspection (kernel manifest)
    Host(crate::commands::host::HostArgs),
}

/// Arguments for `faber format`.
#[derive(clap::Args, Debug)]
pub struct FormatArgs {
    /// Files or directories to format (default: current package directory)
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Emit canonical HIR-backed surface (trivia-free round-trip tooling)
    #[arg(long)]
    pub canonical: bool,

    /// Reader locale used to select a package-local reader pack.
    #[arg(long = "reader-locale", value_name = "LOCALE")]
    pub reader_locale: Option<String>,

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

    /// Reader locale used to select a package-local reader pack.
    #[arg(long = "reader-locale", value_name = "LOCALE")]
    pub reader_locale: Option<String>,

    /// Input file or package path
    pub input: String,
}

/// Arguments for `faber check`.
#[derive(clap::Args, Debug)]
pub struct CheckArgs {
    /// Print expanded phase-aware diagnostics instead of normal check output
    #[arg(long)]
    pub diagnostics: bool,

    /// Reader locale used to select a package-local reader pack.
    #[arg(long = "reader-locale", value_name = "LOCALE")]
    pub reader_locale: Option<String>,

    /// Downgrade unresolved/import-driven semantic errors to warnings
    #[arg(long)]
    pub permissive: bool,

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
    /// Public Faber source library name, such as `norma`
    pub library: String,
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

    /// Reader locale used to select diagnostic explanation text
    #[arg(
        long = "reader-locale",
        value_name = "LOCALE",
        requires = "term",
        conflicts_with_all = ["search", "list", "category"]
    )]
    pub reader_locale: Option<String>,

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
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Arguments for `faber run`.
#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// Package path to run (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Runtime target to build and execute.
    #[arg(short = 't', long = "target", value_enum, default_value_t = radix::tool::CliTarget::Rust)]
    pub target: radix::tool::CliTarget,

    /// Run the release binary
    #[arg(long)]
    pub release: bool,

    /// Force in-process MIR interpretation for scripts or supported packages
    #[arg(long, conflicts_with = "compile")]
    pub interpret: bool,

    /// Force compile-to-Rust execution even for single `.fab` files
    #[arg(long, conflicts_with = "interpret")]
    pub compile: bool,

    /// Arguments passed to the executed program (after --)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Arguments for the hidden FMIR image runner.
#[derive(clap::Args, Debug)]
pub struct FmirRunArgs {
    /// FMIR image path to execute.
    pub image: PathBuf,

    /// Arguments passed to the FMIR program.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
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
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Arguments for `faber test`.
#[derive(clap::Args, Debug)]
pub struct TestArgs {
    /// Package path to test
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Test name filter passed to the Rust test harness (matches on generated proba_* names)
    #[arg(value_name = "FILTER")]
    pub filter: Option<String>,

    /// Select tests by source-level proba name
    #[arg(long)]
    pub name: Option<String>,

    /// Select tests by source-level probandum suite path, joined with `/`
    #[arg(long)]
    pub suite: Option<String>,

    /// Select tests by source-level tag modifier
    #[arg(long)]
    pub tag: Option<String>,

    /// Run only tests whose name exactly matches the filter
    #[arg(long)]
    pub exact: bool,

    /// Show test output (do not capture stdout/stderr from test bodies)
    #[arg(long)]
    pub nocapture: bool,

    /// Limit the number of test threads used by the harness
    #[arg(long, value_name = "N")]
    pub test_threads: Option<usize>,

    /// Only run Rust-ignored tests, including `omitte` / `futurum` and selection-ignored cases
    #[arg(long, conflicts_with = "include_ignored")]
    pub ignored: bool,

    /// Run normal tests and Rust-ignored tests
    #[arg(long, conflicts_with = "ignored")]
    pub include_ignored: bool,
}
