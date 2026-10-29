//! Clap command shapes for the `faber` binary.

mod emit;

use clap::{Parser, Subcommand, ValueEnum};
pub use emit::{EmitArgs, FaberCliTarget};
use faber_cli::package::DeviceChoice;
use std::path::PathBuf;

const FABER_AFTER_HELP: &str = include_str!("../../help/faber-after-help.md");

const CHECK_LONG_ABOUT: &str = "\
Run semantic analysis on a file or package.

`faber check` owns package checking, including auto-detected packages.
`radix check` is single-file compiler-dev checks.

Package directories and faber.toml inputs use package mode. A directory
without a faber.toml manifest is treated as a legacy package whose entry is
<main.fab>; named libraries without that manifest use `faber check-library`
instead.

Library resolution: --library-home (or `[paths] library_home` in faber.toml)
overrides the library home. Otherwise the Norma library embedded in the binary
is used. `faber doctor` prints the
resolved home and per-library availability.

Availability taxonomy: `norma` is the builtin identity row (LibraryProvider::Builtin);
other provider names are package identity. On disk a checkout is a faber
package (faber.toml), a cista source package (cista.toml / src fallback), or
not found. Identity always follows the import specifier, never the manifest.

Packages that pin locale = \"la\" parse on the Latin keyword surface. English
sources in those packages fail until the pin matches the source locale.
";

const FORMAT_LONG_ABOUT: &str = "\
Format complete Faber source units through the canonical HIR re-emitter. Each
file selects its source reader locale independently: frontmatter, then the
package manifest, then the product default en. --stdin has no package context
and therefore uses en when it has no frontmatter locale. Format preserves the
frontmatter metadata and does not translate between reader locales. For source
conversion use `faber convert --to <LOCALE>`. A local declaration must be inside
a complete function or `main` unit; format does not accept fragments. Does not
write unless `--write` is given: the default reports each file that would change
and exits 1 when any would.

Output is not wrapped: canonical lines keep no width limit. `--width N` (or the
package manifest key `[format] width = N`) opts in to wrapping lines wider than
N display columns, indentation included. `--width` wins over the manifest key,
and with neither set nothing wraps. Wrapping only moves existing token
boundaries onto a new line; it never splits a token, string or comment, and
never breaks where a newline changes meaning (annotation sugar, schema
columns, the `if` of a guarded `throw`/`panic`, the `from` of copy-with-changes, a
`return` value, `write` and extraction lists). An unbreakable span wider than N
overflows. `--stdin` has no package, so only the flag applies.
";

const CONVERT_LONG_ABOUT: &str = "\
Convert complete Faber source units between reader-locale surfaces. --from
forces the source reader pack for every selected file; without it, each file
uses frontmatter, then its package manifest, then the product default en. --to
is one command-wide target pack. Successful output replaces or inserts
frontmatter `locale = \"<target>\"` while preserving unrelated metadata. Use a
wrapper such as `main { let list<u32> values ← empty }` when probing a
local declaration; fragments are not accepted. --stdout and --stdin emit one
complete converted document. Does not write unless `--write` is given: the
default (spelled `--check`) reports files that would change and exits 1 if any
would.
";

const CHECK_LIBRARY_LONG_ABOUT: &str = "\
Check a named library's complete interface graph from the library home.

v1 resolves <name> from the library home only (--library-home or
[paths] library_home, else the Norma library embedded in the binary). The
source root mirrors the import resolver: top-level faber.toml if present,
otherwise
<home>/<name>/src. v1 does not parse
cista.toml, does not discover CISTAE_HOME or faber.lock store roots, and
does not replace `scripta/check-source` or run .proba tests.

Identity is the import specifier (`norma` stays builtin). This command
does not classify a library as Package from manifest metadata and is not
an alias for application `faber check`.
";

const VERIFY_LIBRARY_LONG_ABOUT: &str = "\
Verify a library package's target binding manifest.

Requires a faber.toml manifest at the library root. Pass the package
directory or that faber.toml path. A directory without faber.toml fails
when reading the sought <root>/faber.toml.
";

const COST_LONG_ABOUT: &str = "\
Report static runtime-cost classes over a package's lowered MIR.

`faber cost` classifies conversion sites and uncosted extern calls as
relative classes (C0 register op, C1 scan/format, C2 allocate+copy, C3
walk+transform), plus foldable (F) and uncosted (U) buckets. It does not
measure wall time. Cost units are relative classes, not time.

The same package discovery front half as `faber check` compiles to
ValidatedMir and stops before emit. Default output includes C0 sites.
`--min-class C1` hides cheaper C-class sites; F and U buckets stay
visible. `--json` emits the same records as a versioned machine contract.
";

/// Library home selection shared by commands that resolve provider checkouts.
#[derive(clap::Args, Debug, Default, Clone)]
pub struct LibraryHomeArgs {
    /// Directory holding provider checkouts such as `<DIR>/gradus/src`;
    /// overrides `[paths] library_home` in faber.toml; norma needs none
    /// (embedded)
    #[arg(long = "library-home", value_name = "DIR")]
    pub library_home: Option<PathBuf>,
}

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
    /// Interpret Faber source through the runner (one-liner)
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

    /// Verify a corpus case or suite against its expected output
    Corpus(CorpusArgs),

    /// Show supported targets and current capability notes
    Targets,

    /// Run semantic analysis on a file or package
    #[command(long_about = CHECK_LONG_ABOUT)]
    Check(CheckArgs),

    /// Check a named library from the library home (dev checkout root only)
    #[command(name = "check-library", long_about = CHECK_LIBRARY_LONG_ABOUT)]
    CheckLibrary(CheckLibraryArgs),

    /// Show resolved library home, probing provenance, and availability
    Doctor(DoctorArgs),

    /// Check for or install a newer released faber
    Update(crate::commands::update::UpdateArgs),

    /// Remove installed faber versions (product files only)
    Uninstall(crate::commands::update::UninstallArgs),

    /// List installed faber versions and the active one
    Versions(crate::commands::update::VersionsArgs),

    /// Report static runtime-cost classes over lowered MIR
    #[command(long_about = COST_LONG_ABOUT)]
    Cost(CostArgs),

    /// Run aspect verification on a single file
    Verify(VerifyArgs),

    /// Verify a library package's target binding manifest
    #[command(long_about = VERIFY_LIBRARY_LONG_ABOUT)]
    VerifyLibrary(VerifyLibraryArgs),

    /// Create a new Faber package
    Init(InitArgs),

    /// Explain a Faber glyph, keyword, or grammar term
    Explain(ExplainArgs),

    /// Interpret and run Faber source (single file, package, or archive)
    Run(RunArgs),

    /// Interactive runner REPL (accumulating cells, re-lowers each line)
    Repl(ReplArgs),

    /// Run test cases on the MIR runner (no Cargo / rustc on the package)
    Test(TestArgs),

    /// Internal FMIR image runner used by generated executable bundles
    #[command(name = "__fmir-run", hide = true)]
    FmirRun(FmirRunArgs),

    /// Tokenize source and output JSON (compatibility alias for `radix lex`)
    Lex(InputArgs),

    /// Parse source and output AST as JSON (compatibility alias for `radix parse`)
    Parse(InputArgs),

    /// Lower AST to HIR and output as JSON (compatibility alias for `radix hir`)
    Hir(InputArgs),

    /// Lower checked HIR to MIR and output a deterministic text dump (compatibility alias for `radix mir`)
    Mir(InputArgs),

    /// Validate and output normalized CLI IR as JSON (compatibility alias for `radix cli-ir`)
    CliIr(InputArgs),

    /// Compile to target for stdout (compatibility alias for `radix emit`)
    Emit(EmitArgs),

    /// Format Faber source (HIR re-emit at the source's own locale)
    #[command(long_about = FORMAT_LONG_ABOUT)]
    Format(FormatArgs),

    /// Render exported declarations with their signatures and doc comments
    /// (plain text, or a stable machine-readable surface with --json)
    Doc(DocArgs),

    /// Convert source between reader-locale surfaces (en, la, zh-Hans, …) —
    /// HIR-backed round-trip; not a formatter; report-only unless `--write`
    /// is given (see `faber convert --help`)
    #[command(long_about = CONVERT_LONG_ABOUT)]
    Convert(ConvertArgs),

    /// Insert `mut` before every unmarked class field (D16.1 migration;
    /// span-based, every other byte unchanged; report-only unless `--write`
    /// is given)
    #[command(name = "field-modifier-codemod")]
    FieldModifierCodemod(FieldModifierCodemodArgs),

    /// Script host introspection (kernel manifest)
    Host(crate::commands::host::HostArgs),

    /// Inspect model file metadata (safetensors)
    Model(ModelArgs),
}

impl Command {
    /// The `--library-home` flag value plus the path whose nearest `faber.toml`
    /// may carry `[paths] library_home`. `None` means the command carries no
    /// such input.
    pub fn library_home_inputs(&self) -> (Option<PathBuf>, Option<PathBuf>) {
        match self {
            Self::Build(args) => (
                args.library.library_home.clone(),
                Some(PathBuf::from(&args.input)),
            ),
            Self::Check(args) => (
                args.library.library_home.clone(),
                args.input.first().map(PathBuf::from),
            ),
            Self::Emit(args) => (
                args.library.library_home.clone(),
                args.input.first().map(PathBuf::from),
            ),
            Self::Run(args) => (args.library.library_home.clone(), Some(args.path.clone())),
            Self::Test(args) => (args.library.library_home.clone(), Some(args.path.clone())),
            Self::CheckLibrary(args) => (args.library.library_home.clone(), None),
            Self::Doctor(args) => (args.library.library_home.clone(), None),
            _ => (None, None),
        }
    }
}

/// Arguments for `faber doctor`.
#[derive(clap::Args, Debug, Default)]
pub struct DoctorArgs {
    #[command(flatten)]
    pub library: LibraryHomeArgs,
}

/// Arguments for `faber doc`.
#[derive(clap::Args, Debug)]
pub struct DocArgs {
    /// A `.fab` file, or a package directory / `faber.toml` (every package
    /// source file under its source root)
    #[arg(value_name = "PATH")]
    pub input: PathBuf,

    /// Emit one JSON document (`schema_version` 1) instead of plain text
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `faber format`.
#[derive(clap::Args, Debug)]
pub struct FormatArgs {
    /// Use compact diagnostics instead of the default message and help
    #[arg(long)]
    pub no_diagnostics: bool,

    /// Files or directories to format (default: current package directory).
    ///
    /// Each root resolves the package that contains it (nearest faber.toml,
    /// walking up).
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Report files that would change without writing (the default; explicit
    /// spelling); exit 1 if any file would change
    #[arg(long, conflicts_with_all = ["write", "stdout"])]
    pub check: bool,

    /// Write formatted output to stdout instead of reporting (exactly one
    /// input file)
    #[arg(long, conflicts_with_all = ["check", "write"])]
    pub stdout: bool,

    /// Write formatted source back to the file (default is report-only)
    #[arg(long, conflicts_with_all = ["check", "stdout"])]
    pub write: bool,

    /// Read exactly one source document from stdin and print the formatted
    /// result to stdout (incompatible with path arguments and the other modes)
    #[arg(long, conflicts_with_all = ["check", "write", "stdout", "paths"])]
    pub stdin: bool,

    /// Wrap lines wider than N display columns (indentation included) at
    /// legal token boundaries. Wins over the manifest `[format] width`; with
    /// neither set output is not wrapped. A token or protected span wider
    /// than N overflows rather than breaking
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub width: Option<u32>,
}

/// Arguments for `faber convert`.
#[derive(clap::Args, Debug)]
pub struct ConvertArgs {
    /// Use compact diagnostics instead of the default message and help
    #[arg(long)]
    pub no_diagnostics: bool,

    /// Files or directories to convert (default: current directory)
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Target reader locale (required; e.g. en, la, zh-Hans). One target pack
    /// is used for every selected complete source file.
    #[arg(long = "to", value_name = "LOCALE")]
    pub to: String,

    /// Force the source reader locale for every selected file. Without this,
    /// each file uses frontmatter, package manifest, then product default en.
    #[arg(long = "from", value_name = "LOCALE")]
    pub from: Option<String>,

    /// Report files that would change without writing (the default; explicit
    /// spelling); exit 1 if any file would change
    #[arg(long, conflicts_with_all = ["write", "stdout"])]
    pub check: bool,

    /// Write converted output to stdout instead of reporting (exactly one
    /// input file)
    #[arg(long, conflicts_with_all = ["check", "write"])]
    pub stdout: bool,

    /// Write converted source back to the file (default is report-only)
    #[arg(long, conflicts_with_all = ["check", "stdout"])]
    pub write: bool,

    /// Read exactly one source document from stdin and print the converted
    /// result to stdout (incompatible with path arguments and the other modes)
    #[arg(long, conflicts_with_all = ["check", "write", "stdout", "paths"])]
    pub stdin: bool,
}

/// Arguments for `faber field-modifier-codemod`.
#[derive(clap::Args, Debug)]
pub struct FieldModifierCodemodArgs {
    /// Files or directories to codemod (default: current directory)
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Report files that would change without writing (the default; explicit
    /// spelling); exit 1 if any file would change
    #[arg(long, conflicts_with_all = ["write", "stdout"])]
    pub check: bool,

    /// Write the codemodded source to stdout instead of reporting (exactly
    /// one input file)
    #[arg(long, conflicts_with_all = ["check", "write"])]
    pub stdout: bool,

    /// Write codemodded source back to the file (default is report-only)
    #[arg(long, conflicts_with_all = ["check", "stdout"])]
    pub write: bool,
}

/// Arguments for `faber build`.
#[derive(clap::Args, Debug)]
pub struct BuildArgs {
    /// Use compact diagnostics instead of the default message and help
    #[arg(long)]
    pub no_diagnostics: bool,

    /// Output target language
    #[arg(short = 't', long = "target", value_enum)]
    pub target: Option<FaberCliTarget>,

    /// Run every kernel at the default tier, whatever its `@ kernel relaxed`
    /// or `@ kernel unsafe` annotation says. It can only lower a tier. Use it
    /// to check whether a relaxed or unsafe kernel changes answers, and for
    /// release or verification builds. `[device] safe = true` in `faber.toml`
    /// sets it for the package; no environment variable does.
    #[arg(long)]
    pub safe: bool,

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

    /// Code locale used to read source and select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

    /// Input file or package path
    #[arg(value_name = "INPUT")]
    pub input: String,

    #[command(flatten)]
    pub library: LibraryHomeArgs,
}

/// Arguments for `faber corpus`.
#[derive(clap::Args, Debug)]
pub struct CorpusArgs {
    /// Faber source, package directory, or suite directory containing corpus.toml
    pub input: PathBuf,

    /// Target to verify (runner, rust, or a target named in corpus.toml)
    #[arg(long, default_value = "runner")]
    pub target: String,

    /// Golden stdout file for a single input (defaults to its .expected sidecar)
    #[arg(long, value_name = "PATH")]
    pub expected: Option<PathBuf>,

    /// Only run suite cases whose id contains this substring. Selectors
    /// --domain, --category, --case and --filter all add up: a case runs when
    /// any one of them names it (union; nothing narrows or excludes)
    #[arg(long, value_name = "SUBSTRING")]
    pub filter: Option<String>,

    /// Run every case in these domains (comma list; adds to the other selectors).
    /// The suite's corpus.toml [domains] table defines them; a suite without one
    /// rejects this flag
    #[arg(long, value_name = "DOMAIN,...", value_delimiter = ',')]
    pub domain: Vec<String>,

    /// Run every case in these categories (comma list; adds to the other
    /// selectors). A suite without a [domains] table rejects this flag
    #[arg(long, value_name = "CATEGORY,...", value_delimiter = ',')]
    pub category: Vec<String>,

    /// Run these exact case ids (comma list; adds to the other selectors). An
    /// unknown id is an error
    #[arg(long = "case", value_name = "ID,...", value_delimiter = ',')]
    pub case: Vec<String>,
}

/// Shared source-input grammar for phase inspection commands.
#[derive(clap::Args, Debug)]
pub struct InputArgs {
    /// Load a pre-analyzed HIR artifact from a .fhir file
    #[arg(long, value_name = "FILE")]
    pub load: Option<PathBuf>,

    /// Input file path, or '-' / omitted for stdin
    pub input: Vec<String>,
}

/// Parsed `verify` command payload after clap normalization.
#[derive(clap::Args, Debug)]
pub struct VerifyArgs {
    /// Force package verification mode
    #[arg(long)]
    pub package: bool,

    /// Input file, or '-' / omitted for stdin
    pub input: Vec<String>,
}

/// Arguments for `faber check-library`.
#[derive(clap::Args, Debug)]
pub struct CheckLibraryArgs {
    /// Provider name resolved from the library home (`norma`, `triga`, …).
    /// v1 does not accept a path; use `faber check` for a package directory.
    pub name: String,

    #[command(flatten)]
    pub library: LibraryHomeArgs,
}

/// Arguments for `faber cost`.
#[derive(clap::Args, Debug)]
pub struct CostArgs {
    /// Emit the versioned JSON contract instead of the human report
    #[arg(long)]
    pub json: bool,

    /// Hide C-class sites cheaper than this class (default C0 includes all)
    #[arg(long = "min-class", value_name = "CLASS", default_value = "C0")]
    pub min_class: String,

    /// Code locale used to select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

    /// Force package mode
    #[arg(long)]
    pub package: bool,

    /// Input file or package path
    pub input: Vec<String>,
}

/// Arguments for `faber check`.
#[derive(clap::Args, Debug)]
pub struct CheckArgs {
    /// Use compact diagnostics instead of the default message and help
    #[arg(long)]
    pub no_diagnostics: bool,

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

    /// Per-function cyclomatic complexity budget for this check (>= 1);
    /// overrides the baked-in default for this run
    #[arg(
        long = "complexity-budget",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub complexity_budget: Option<u32>,

    /// Per-function cyclomatic complexity budget for `@ kernel` functions in
    /// this check (>= 1); advisory-only (never gates kernel admission);
    /// overrides the baked-in kernel default for this run
    #[arg(
        long = "kernel-complexity-budget",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub kernel_complexity_budget: Option<u32>,

    /// Print an advisory AIR-lane compatibility report (report-only); rows
    /// carry the census tier labels ADMISSIBLE-KERNEL, ADMISSIBLE-ONE-AWAY,
    /// WOULD-REJECT, and NOT-A-TARGET
    #[arg(long)]
    pub air: bool,

    /// Emit the advisory AIR report as versioned machine rows (requires --air)
    #[arg(long, requires = "air")]
    pub json: bool,

    /// Also compile the program to Faber's reference form without running
    /// it; reports errors semantic analysis cannot see (a program that
    /// checks but cannot run). Needs a file or package path
    #[arg(long, conflicts_with_all = ["air", "permissive"])]
    pub lower: bool,

    /// Advise where the package's kernels run serialized on this device
    /// (`cpu`, `metal` or `cuda`): a single-lane kernel, a kernel the device
    /// runs as one sequential call, a kernel it cannot place, and with
    /// `--partitions` a kernel that will not split across the partitions.
    /// With `--device cuda --partitions N` (N above 1) it also prints the
    /// planner audit: each kernel whose cross-partition syncs are avoidable,
    /// and why. The rows are `info` advice (INFO001), never errors; the package is
    /// lowered and its device image built as `faber run --device` builds it,
    /// and nothing runs. Needs a file or package path
    #[arg(
        long = "device",
        value_enum,
        value_name = "DEVICE",
        conflicts_with_all = ["air", "permissive"]
    )]
    pub device: Option<DeviceChoice>,

    /// With `--device`: the partition count of the one spanned logical
    /// device to advise for (default 1), as
    /// `faber run --partitions N`
    #[arg(
        long = "partitions",
        value_name = "N",
        requires = "device",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub partitions: Option<u32>,

    /// Force package checking mode
    #[arg(long)]
    pub package: bool,

    /// Input file or package path, or '-' / omitted for stdin
    pub input: Vec<String>,

    #[command(flatten)]
    pub library: LibraryHomeArgs,
}

/// Arguments for `faber init`.
#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Target directory for the new package
    #[arg(default_value = ".")]
    pub path: PathBuf,
}

/// Arguments for `faber verify-library`.
#[derive(clap::Args, Debug)]
pub struct VerifyLibraryArgs {
    /// Target binding surface to verify.
    #[arg(long, default_value = "rust")]
    pub target: String,

    /// Library package root or faber.toml path. Requires a faber.toml at the library root.
    pub input: PathBuf,
}

/// Arguments for `faber explain`.
#[derive(clap::Args, Debug)]
#[command(group(clap::ArgGroup::new("json_target").args(["term", "list"]).multiple(true)))]
pub struct ExplainArgs {
    /// Emit a machine-readable JSON explanation (one term, or the --list index)
    #[arg(long, requires = "json_target")]
    pub json: bool,

    /// Reader locale whose spellings terms are listed and looked up by (default: the
    /// current package's faber.toml `[locale]`, else en)
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

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
    #[arg(long, conflicts_with_all = ["search", "category", "term"])]
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
    /// Arguments available to script cells via `process.argv()`
    #[arg(allow_hyphen_values = true, last = true)]
    pub args: Vec<String>,
}

/// Compiled targets `faber run --target` builds and then runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum RunTarget {
    /// Build the package as a Node project and run it with `node`.
    #[value(name = "ts")]
    Ts,
    /// Build the package as a Go module and run the built program.
    #[value(name = "go")]
    Go,
    /// Build the package as a native LLVM program and run the built program.
    #[value(name = "llvm-host")]
    LlvmHost,
    /// Compile the package to native code in memory (Cranelift JIT) and run it
    /// in this process: no object file, no linker, nothing else installed.
    #[value(name = "clif-jit")]
    ClifJit,
}

/// Arguments for `faber run`.
///
/// Without `--target`, `run` is interpreted execution: a single `.fab` file,
/// package directory, `faber.toml`, or archive is routed through the runner —
/// never compiled. With `--target`, `run` builds the package for that target
/// and runs the result with the arguments after `--`, standard streams and
/// exit code passed straight through. Device packages keep a host-driver
/// execution route.
#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// Use compact diagnostics instead of the default message and help
    #[arg(long)]
    pub no_diagnostics: bool,

    /// Path to run (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    #[command(flatten)]
    pub library: LibraryHomeArgs,

    /// Build the package for a compiled target, then run the built program.
    #[arg(long = "target", short = 't', value_enum, value_name = "TARGET")]
    pub target: Option<RunTarget>,

    /// Code locale used to select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

    /// Device selection for device-capable packages (differentiable-GPU
    /// campaign N1.1): `auto`, `reference`, `cpu`, `metal`, or `cuda`. When omitted, the
    /// manifest `[device] device` wins; otherwise the default `auto` applies.
    /// Precedence: CLI > manifest > `auto`. An explicit device request never
    /// silently falls back: it fails closed before launch
    /// (`E_DEVICE_UNAVAILABLE` when this machine has no runtime for it,
    /// `E_DEVICE_NONE_ADMITTED` when the runtime finds no usable device), and
    /// a package with no device program fails closed ("package has no device
    /// program"). `reference` is plain sequential code on the
    /// MIR runner, the oracle device results are checked against: it lowers
    /// `@ kernel` functions as ordinary functions and runs them
    /// single-threaded. `cpu` is processor cores as one device: the kernels go
    /// through the same device session as `metal` and `cuda`, run by
    /// `--threads` workers shared across `--partitions` logical partitions, and the result is
    /// the same at any worker or partition count; a
    /// package without kernels runs on the reference route. `auto` never picks
    /// `cpu`, and without an explicit `reference` or `cpu` a program that calls
    /// a kernel on a machine with no GPU is refused. `metal` and `cuda` run
    /// kernels in process: Metal on Apple GPUs, CUDA through the NVIDIA driver
    /// with a PTX image built at run time (`clang` with the NVPTX target on
    /// `PATH`).
    #[arg(long = "device", value_enum, value_name = "DEVICE")]
    pub device: Option<DeviceChoice>,

    /// Worker count for the `cpu` device. Without it the manifest
    /// `[device] threads` applies, and without that the default is half the
    /// available cores, minimum one. CPU workers run at reduced priority,
    /// below interactive programs, whether the count is the default or given
    /// here. Results are identical at any worker count. Valid only with
    /// `--device cpu`.
    #[arg(long = "threads", value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub threads: Option<u32>,

    /// Virtual partition count for the `cpu`, `metal` or `cuda` device (default 1). The CPU's
    /// logical device spans N partitions sharing one memory budget, one
    /// buffer store, and one pool of `--threads` workers. Each launch's
    /// workgroups are split into N contiguous ranges scheduled on that pool;
    /// increasing partitions never increases the worker count.
    /// Results are identical at any partition count. Valid with
    /// `--device cpu`, `--device metal` and `--device cuda`. Metal partitions
    /// share one buffer store and one command queue on the GPU. Split kernels
    /// dispatch each nonempty share in order; single-lane kernels and other
    /// whole-call stages execute once. No data moves between Metal
    /// partitions. On CUDA each partition is its own physical GPU: N takes
    /// the N lowest-ordinal healthy devices (`CUDA_VISIBLE_DEVICES` is
    /// applied first), each with its own memory, and fewer devices than N
    /// fails closed naming both counts. CUDA partitions copy every buffer to
    /// every GPU through the host and do not yet split kernels, so this
    /// proves placement and fail-closed admission, not a speedup.
    #[arg(long = "partitions", value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub partitions: Option<u32>,

    /// Run every kernel at the default tier, whatever its `@ kernel relaxed`
    /// or `@ kernel unsafe` annotation says. It can only lower a tier. Use it
    /// to check whether a relaxed or unsafe kernel changes answers, and for
    /// release or verification builds. `[device] safe = true` in `faber.toml`
    /// sets it for the package; no environment variable does.
    #[arg(long)]
    pub safe: bool,

    /// Emit diagnostic runner statistics after a package run.
    #[arg(long)]
    pub stats: bool,

    /// Write diagnostic runner statistics to a JSON file.
    #[arg(long = "stats-file", value_name = "PATH")]
    pub stats_file: Option<PathBuf>,

    /// Seed for the runner's map/set iteration-order shuffle; 0 turns it off;
    /// default is a random seed per run
    #[arg(long = "shuffle-seed", value_name = "N")]
    pub shuffle_seed: Option<u64>,

    /// Print developer diagnostics on stderr (device driver counters); never
    /// changes program output
    #[arg(long)]
    pub debug: bool,

    /// Promote all warning diagnostics to errors
    #[arg(long)]
    pub deny_warnings: bool,

    /// Promote diagnostics matching this catalog code to errors (repeatable)
    #[arg(long = "deny", value_name = "CODE")]
    pub deny: Vec<String>,

    /// Arguments passed to the interpreted program (after --)
    #[arg(allow_hyphen_values = true, last = true)]
    pub args: Vec<String>,
}

/// Arguments for the hidden FMIR image runner.
#[derive(clap::Args, Debug)]
pub struct FmirRunArgs {
    /// FMIR image path to execute.
    pub image: PathBuf,

    /// Device selection override for the image-runner route (N1.1). When
    /// omitted, the image's declared `device.selection` wins; otherwise
    /// `auto` applies.
    #[arg(long = "device", value_enum, value_name = "DEVICE")]
    pub device: Option<DeviceChoice>,

    /// Arguments passed to the FMIR program.
    #[arg(allow_hyphen_values = true, last = true)]
    pub args: Vec<String>,
}

/// Output format for `faber test` reports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum TestOutputFormat {
    /// Human-readable one-line-per-case output.
    #[default]
    Text,
    /// Machine-readable JSON report.
    Json,
}

/// Arguments for `faber test` (MIR runner — no Cargo / rustc on the package).
#[derive(clap::Args, Debug)]
pub struct TestArgs {
    /// Use compact diagnostics instead of the default message and help
    #[arg(long)]
    pub no_diagnostics: bool,

    /// Package path or single `.fab` / `.proba` file to test
    #[arg(default_value = ".")]
    pub path: PathBuf,

    #[command(flatten)]
    pub library: LibraryHomeArgs,

    /// Code locale used to select a package-local locale pack.
    #[arg(long = "locale", value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Message language for diagnostics (independent of code locale).
    #[arg(long = "diagnostics-locale", value_name = "LOCALE")]
    pub diagnostics_locale: Option<String>,

    /// Device selection for cases that call `@ kernel` functions:
    /// `reference` runs them in host form, in sequence, on the MIR runner (the
    /// oracle); `cpu` runs a package's kernels on the CPU device, through the
    /// same device session as `faber run --device cpu`, by `--threads`
    /// workers (a single `.fab` / `.proba` file outside a package runs them
    /// on the reference route, as `faber run` does); `auto` or absent keeps
    /// refusing kernels. `metal` and `cuda` are not served by `faber test` and
    /// fail closed. Precedence: CLI > manifest `[device] device` > `auto`.
    #[arg(long = "device", value_enum, value_name = "DEVICE")]
    pub device: Option<DeviceChoice>,

    /// Worker count for the `cpu` device, as for `faber run`. Precedence:
    /// `--threads` > manifest `[device] threads` > half the available cores,
    /// minimum one. Results are identical at any worker count. Valid only
    /// with `--device cpu`.
    #[arg(long = "threads", value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub threads: Option<u32>,

    /// Environment name that `only_in` modifiers match, in addition to the
    /// OS name.
    #[arg(long = "only-in", value_name = "NAME")]
    pub only_in: Option<String>,

    /// Promote all warning diagnostics to errors
    #[arg(long)]
    pub deny_warnings: bool,

    /// Promote diagnostics matching this catalog code to errors (repeatable)
    #[arg(long = "deny", value_name = "CODE")]
    pub deny: Vec<String>,

    /// Repeatable substring filter on case path (`suite/name`) or title.
    /// Matching filters are combined with OR. File-scoped filters use
    /// `path.proba::name`.
    #[arg(long = "filter", value_name = "FILTER")]
    pub filter_flag: Vec<String>,

    /// Positional harness filter (same meaning as one `--filter`)
    #[arg(value_name = "FILTER")]
    pub filter: Option<String>,

    /// Repeatable negated filter. Matching cases are excluded after filters.
    #[arg(long = "skip", value_name = "FILTER")]
    pub skip: Vec<String>,

    /// Load only `*.proba` sources matching this path pattern (repeatable).
    /// Patterns are relative to the package source root (`*` wildcards allowed).
    /// Bare stems match `name.proba` (e.g. `--include math`).
    #[arg(long = "include", value_name = "PATTERN")]
    pub include: Vec<String>,

    /// Skip `*.proba` sources matching this path pattern (repeatable).
    #[arg(long = "exclude", value_name = "PATTERN")]
    pub exclude: Vec<String>,

    /// Select tests by source-level test name
    #[arg(long)]
    pub name: Option<String>,

    /// Select tests by source-level suite path, joined with `/`.
    /// Repeatable values are combined with OR.
    #[arg(long, value_name = "SUITE")]
    pub suite: Vec<String>,

    /// Select tests by source-level tag modifier. Repeatable values are
    /// combined with OR.
    #[arg(long, value_name = "TAG")]
    pub tag: Vec<String>,

    /// List selected cases without executing them
    #[arg(long)]
    pub list: bool,

    /// Report format
    #[arg(long, value_enum, default_value_t = TestOutputFormat::Text)]
    pub format: TestOutputFormat,

    /// Stop after the first failed or blocked case
    #[arg(long = "fail-fast")]
    pub fail_fast: bool,

    /// Select only cases marked with the source-level `only` focus modifier
    #[arg(long)]
    pub focused: bool,

    /// Require the filter to match the full case path or title exactly
    #[arg(long)]
    pub exact: bool,

    /// Show test output (do not capture stdout/stderr from test bodies)
    #[arg(long)]
    pub nocapture: bool,

    /// Reserved (serial execution only; ignored by the runner)
    #[arg(long, value_name = "N")]
    pub test_threads: Option<usize>,

    /// Not supported on the runner (exits with an error)
    #[arg(long, conflicts_with = "include_ignored")]
    pub ignored: bool,

    /// Include selection-filtered cases as skips in the report
    #[arg(long, conflicts_with = "ignored")]
    pub include_ignored: bool,

    /// Interpret a single `.fab` / `.proba` file without package analysis
    #[arg(long = "no-package")]
    pub no_package: bool,

    /// Exit 0 when no cases are discovered or matched after filtering
    #[arg(long = "allow-no-tests")]
    pub allow_no_tests: bool,

    /// Wall-clock timeout (seconds) applied to every test case that has no
    /// per-test `timeout` modifier. 0 is rejected fail-closed.
    #[arg(long = "test-timeout", value_name = "SECONDS", value_parser = parse_test_timeout_secs)]
    pub test_timeout: Option<u64>,

    /// Emit diagnostic runner statistics after the test report.
    #[arg(long)]
    pub stats: bool,

    /// Write diagnostic runner statistics to a JSON file.
    #[arg(long = "stats-file", value_name = "PATH")]
    pub stats_file: Option<PathBuf>,
}

/// `--test-timeout` parser: seconds, strictly positive (0 is fail-closed).
fn parse_test_timeout_secs(value: &str) -> Result<u64, String> {
    let parsed: u64 = value
        .parse()
        .map_err(|_| format!("invalid --test-timeout seconds: {value}"))?;
    if parsed == 0 {
        Err("--test-timeout requires a positive number of seconds".to_owned())
    } else {
        Ok(parsed)
    }
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
    pub path: PathBuf,
}
