//! Driver configuration and per-compilation session state.
//!
//! The driver accepts a [`Session`] rather than a loose set of options so phase
//! entrypoints have one stable place to read target policy and future
//! compilation state. Today the session is intentionally small: [`Config`]
//! captures user-facing choices, while mutable caches and incremental state have
//! not yet earned a place in the contract.
//!
//! BOUNDARY
//! ========
//! `Config` describes compilation policy supplied by callers. `Session` is the
//! object passed through the driver and later phases. Keeping those concepts
//! distinct lets future work add cached module state, diagnostics sinks, or
//! package context without changing every public driver function signature.
//!
//! INVARIANTS
//! ==========
//! - Rust remains the default backend because it is the primary executable
//!   target and changing it would silently change the public compiler API.
//!   Minimal builds select their intended target explicitly.
//! - Comments are emitted by default so generated code remains inspectable.
//! - `locale_root` is optional; package-mode library resolution owns provider
//!   repos through `library_home`, not this field.

use super::target_bindings::TargetBindings;
use crate::codegen::{OutputMode, Target};
use crate::diagnostics::Severity;
use crate::locale::LocalePack;
use handroll::FxHashMap;
use radix_types::KernelLowering;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Policy for promoting warning diagnostics to errors at the compilation boundary.
///
/// Phases keep emitting `Severity::Warning` unchanged. One driver step applies
/// this policy to the final diagnostic list before `CompileResult::success()`
/// and rendering, flipping matching warnings to `Severity::Error` while leaving
/// code, span, and help text intact. `Severity::Info` is never promoted.
///
/// Defaults to fully permissive (today's behavior: warnings non-fatal).
#[derive(Debug, Clone, Default)]
pub struct WarnPolicy {
    /// Promote every `Warning`-severity diagnostic to `Error`.
    pub deny_all_warnings: bool,

    /// Promote diagnostics whose catalog code matches one of these entries to
    /// `Error`. Codes are validated against the diagnostic catalog at CLI parse
    /// time, so an unknown code never silently fails to fire.
    pub deny_codes: Vec<String>,
}

impl WarnPolicy {
    /// Return whether a diagnostic with the given severity and code should be
    /// promoted to `Error`. Non-warning severities are never promoted.
    #[must_use]
    pub fn promotes(&self, severity: Severity, code: Option<&str>) -> bool {
        if severity != Severity::Warning {
            return false;
        }
        self.deny_all_warnings || code.is_some_and(|c| self.deny_codes.iter().any(|dc| dc == c))
    }
}

/// User-facing compilation policy consumed by the driver.
///
/// The builder methods keep call sites readable while preserving a single
/// structure that can grow with optimization levels, feature gates, or target
/// capabilities.
#[derive(Debug, Clone)]
pub struct Config {
    /// Backend language selected for code generation and target policy checks.
    pub target: Target,

    /// Whether generated output should preserve explanatory comments.
    pub emit_comments: bool,

    /// Optional language-pack root (`radix/locale`) for frontmatter locale load.
    pub locale_root: Option<PathBuf>,

    /// Optional library-home override (provider repos such as `norma/`).
    ///
    /// Independent of [`Config::locale_root`]. Package-mode resolution owns the
    /// richer story; this slot is the test and custom-layout override.
    pub library_home: Option<PathBuf>,

    /// Warning-promotion policy applied at the compilation boundary.
    pub warn_policy: WarnPolicy,

    /// Optional reader-locale pack for explicit source-surface activation.
    pub locale_pack: Option<LocalePack>,

    /// Whether [`Config::locale_pack`] was selected explicitly by the caller
    /// (e.g. a CLI `--locale`) and must win over any source frontmatter
    /// locale. When `false`, the source's `+++ locale = "…" +++` frontmatter
    /// takes precedence over the session pack.
    pub locale_pack_explicit: bool,

    /// Permit signature-only top-level functions for library contract analysis.
    pub allow_bodyless_functions: bool,

    /// Output mode controlling access modifiers and entry-point emission (Swift only for now).
    pub output_mode: OutputMode,

    /// Explicit Swift module name for library-mode output.
    pub module_name: Option<String>,

    /// Disable the AIR-level op-fusion pass (on by default in all builds).
    pub no_fuse: bool,

    /// How `@ kernel` functions lower for the in-process run routes. `Device`
    /// (the default) keeps the device-translatable form; `Host` lowers
    /// kernels as ordinary functions for the CPU route (`--device cpu`).
    /// Emit and build routes never read it.
    pub kernel_lowering: KernelLowering,

    /// When true, `llvm-text` emit for modules with device roles writes the
    /// NVVM device module (kernel bodies, `; @nucleum device-entry`) instead
    /// of the host-partition CPU stub. Set by `radix emit --cuda-descriptor`.
    pub emit_nvvm_device_module: bool,

    /// Import path templates for `§name/rest` validation and resolution.
    ///
    /// Package mode populates this from `[paths.templates]` (values are package-root
    /// relative paths as authored; the map keys are template names). When `None`,
    /// `§` forms are rejected by the import-path pass.
    pub import_path_templates: Option<std::collections::BTreeMap<String, String>>,

    /// Per-function cyclomatic-complexity warning budget. `None` uses the
    /// baked semantic default; warn-only, never fails compilation alone.
    pub complexity_budget: Option<u32>,

    /// Advisory per-function complexity budget for `@ nucleum` kernel
    /// entries. `None` uses the baked kernel default; warn-only, and never
    /// gates kernel admission (kernel-purity owns that).
    pub kernel_complexity_budget: Option<u32>,

    /// The kernel safe switch (`faber run --safe`, `faber build --safe`,
    /// `safe = true` in `faber.toml`): every kernel runs at the default tier
    /// whatever its annotation says. It can only lower a tier. No environment
    /// variable sets it.
    pub deny_kernel_tiers: bool,

    /// Keep the author's constant expressions as written: analysis does not
    /// fold literal-only float expressions to one literal (F9 ruling 31). Set
    /// by the Faber re-emit behind `faber format` and `faber convert`, which
    /// must print the author's text; every compile path leaves it `false`.
    pub keep_authored_constants: bool,

    /// Leave `praefixum` bodies that reach an import for the package layer.
    /// Analysis never runs another module's code: with this set, a module
    /// with such a body keeps every `praefixum` initializer unevaluated; with
    /// it clear, each such body is refused (`praefixum_import_unavailable`).
    /// Bodies that reach no import evaluate either way.
    pub defer_imported_praefixum: bool,

    /// Opaque route-to-spelling maps for the selected target. The WGSL
    /// graphics path resolves each bound library call in a stage body to its
    /// spelling through these maps; every other path ignores them. Empty by
    /// default, in which case a stage-body library call fails closed.
    pub target_bindings: TargetBindings,

    /// How the `clif` target produces code: an object file for the linker
    /// (the default) or an in-memory image for `faber run --target clif-jit`.
    #[cfg(feature = "mir-clif")]
    pub clif_engine: crate::ClifEngine,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target: Target::HirRust,
            emit_comments: true,
            // Frontmatter-locale sources resolve their reader pack through the
            // language-pack root, so the default config discovers the workspace
            // `locale/` tree when one is present (no-op for installed builds).
            locale_root: discover_dev_locale_root(),
            library_home: None,
            warn_policy: WarnPolicy::default(),
            locale_pack: None,
            locale_pack_explicit: false,
            allow_bodyless_functions: false,
            output_mode: OutputMode::Application,
            module_name: None,
            no_fuse: false,
            kernel_lowering: KernelLowering::Device,
            emit_nvvm_device_module: false,
            import_path_templates: None,
            complexity_budget: None,
            kernel_complexity_budget: None,
            deny_kernel_tiers: false,
            keep_authored_constants: false,
            defer_imported_praefixum: false,
            target_bindings: TargetBindings::default(),
            #[cfg(feature = "mir-clif")]
            clif_engine: crate::ClifEngine::default(),
        }
    }
}

impl Config {
    /// Create a default configuration for ordinary Rust-target compilation.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Select the backend target for this configuration.
    #[must_use]
    pub fn with_target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }

    /// Defer `praefixum` bodies that reach an import to the package layer
    /// instead of refusing them.
    #[must_use]
    pub fn with_defer_imported_praefixum(mut self) -> Self {
        self.defer_imported_praefixum = true;
        self
    }

    /// Override the language-pack root used by frontmatter locale load.
    #[must_use]
    pub fn with_locale_root(mut self, path: PathBuf) -> Self {
        self.locale_root = Some(path);
        self
    }

    /// Override the library home used by package-mode provider resolution.
    #[must_use]
    pub fn with_library_home(mut self, path: PathBuf) -> Self {
        self.library_home = Some(path);
        self
    }

    /// Install the opaque target-binding maps (route to spelling).
    #[must_use]
    pub fn with_target_bindings(mut self, bindings: TargetBindings) -> Self {
        self.target_bindings = bindings;
        self
    }

    /// Set the warning-promotion policy applied at the compilation boundary.
    #[must_use]
    pub fn with_warn_policy(mut self, policy: WarnPolicy) -> Self {
        self.warn_policy = policy;
        self
    }

    /// Activate reader-pack keyword recognition for source lexing.
    #[must_use]
    pub fn with_locale_pack(mut self, pack: LocalePack) -> Self {
        self.locale_pack = Some(pack);
        self
    }

    /// Install the built-in Latin (`la`) reader pack as a session default.
    ///
    /// This is a test-support builder: production code never calls it. Tests
    /// of the canonical Latin surface install the pack explicitly through
    /// this helper instead of leaving the source pack-less.
    ///
    /// The pack resolves through the same frontmatter loader as a
    /// source-declared locale, so it matches the shipped `locale/la/pack.toml`
    /// when the workspace locale root is present; otherwise it falls back to
    /// the registry-assembled [`latin_locale_pack`](crate::locale::latin_locale_pack),
    /// the same pack the compiler uses when no session pack is installed.
    /// It is installed non-explicit, so a source's own frontmatter locale
    /// still wins, exactly like any other session default.
    #[must_use]
    pub fn with_latin_locale(mut self) -> Self {
        let pack =
            super::resolve_frontmatter_locale_pack(&self, "la").unwrap_or_else(|_| crate::locale::latin_locale_pack());
        self.locale_pack = Some(pack);
        self.locale_pack_explicit = false;
        self
    }

    /// Install a resolved reader pack as an exact caller selection that wins
    /// over any source frontmatter locale (CLI `--locale` semantics).
    #[must_use]
    pub fn with_resolved_locale_pack(mut self, pack: LocalePack) -> Self {
        self.locale_pack = Some(pack);
        self.locale_pack_explicit = true;
        self
    }

    /// Enable signature-only top-level declarations for library analysis.
    #[must_use]
    pub fn with_bodyless_functions(mut self) -> Self {
        self.allow_bodyless_functions = true;
        self
    }

    /// Install import path templates for `§name/rest` forms.
    #[must_use]
    pub fn with_import_path_templates(mut self, templates: std::collections::BTreeMap<String, String>) -> Self {
        self.import_path_templates = Some(templates);
        self
    }

    /// Set the per-function cyclomatic-complexity warning budget.
    #[must_use]
    pub fn with_complexity_budget(mut self, budget: u32) -> Self {
        self.complexity_budget = Some(budget);
        self
    }

    /// Set the advisory kernel-entry complexity warning budget.
    #[must_use]
    pub fn with_kernel_complexity_budget(mut self, budget: u32) -> Self {
        self.kernel_complexity_budget = Some(budget);
        self
    }

    /// Run every kernel at the default tier (the `--safe` switch).
    #[must_use]
    pub fn with_safe_kernels(mut self) -> Self {
        self.deny_kernel_tiers = true;
        self
    }

    /// Select the output mode for codegen (Swift only).
    #[must_use]
    pub fn with_output_mode(mut self, mode: OutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    /// Set the explicit Swift module name for library-mode output.
    #[must_use]
    pub fn with_module_name(mut self, module_name: Option<String>) -> Self {
        self.module_name = module_name;
        self
    }

    /// Select how the `clif` target produces code.
    #[cfg(feature = "mir-clif")]
    #[must_use]
    pub fn with_clif_engine(mut self, engine: crate::ClifEngine) -> Self {
        self.clif_engine = engine;
        self
    }

    /// Skip the AIR-level op-fusion pass.
    #[must_use]
    pub fn with_no_fuse(mut self) -> Self {
        self.no_fuse = true;
        self
    }

    /// Lower `@ kernel` functions as ordinary host functions, for the CPU
    /// route that runs them on the MIR runner.
    #[must_use]
    pub fn with_host_kernels(mut self) -> Self {
        self.kernel_lowering = KernelLowering::Host;
        self
    }

    /// Emit the NVVM device module from `llvm-text` when device roles exist.
    ///
    /// Default llvm-text stays the host-partition CPU stub. This flag is the
    /// `--cuda-descriptor` route used by the CUDA proof.
    #[must_use]
    pub fn with_nvvm_device_module(mut self) -> Self {
        self.emit_nvvm_device_module = true;
        self
    }

    /// Ensure the workspace language-pack root is configured for this session.
    ///
    /// Most sources now carry `+++ locale = "…" +++` frontmatter whose reader
    /// pack is loaded through [`Config::locale_root`], so dev/test sessions
    /// must reach the workspace `locale/` tree. [`Config::default`] already
    /// performs this discovery; this builder re-applies it for configs that
    /// were constructed with an empty locale-root slot.
    #[must_use]
    pub fn with_dev_locale_root(mut self) -> Self {
        if let Some(candidate) = discover_dev_locale_root() {
            self.locale_root = Some(candidate);
        }
        self
    }
}

/// Discover the workspace language-pack root from the source checkout layout.
///
/// Returns [`crate::locale::workspace_locale_root`] when that directory exists.
/// Returns `None` when the binary is installed outside the workspace or the
/// directory is absent, so released builds keep their pack-less default.
fn discover_dev_locale_root() -> Option<PathBuf> {
    let candidate = crate::locale::workspace_locale_root();
    candidate.is_dir().then_some(candidate)
}

/// Driver-run-scoped parse-once memo for import targets.
///
/// Keyed by `(reader-pack id, target path)`. Each unique import target is
/// read+parsed at most once per driver run; later imports of the same physical
/// file return the cached exports and [`FileInterface`](crate::file_interface::FileInterface).
pub(crate) type ImportTargetMemo = crate::program::compile::ImportTargetCache;

/// Driver-run-scoped memo for package-root walks.
///
/// Keyed by the directory that started the walk. Every intermediate directory
/// visited during a successful (or failed) walk is filled so later imports
/// from the same package pay O(1) instead of O(depth) `faber.toml` probes.
pub(crate) type PackageRootMemo = FxHashMap<PathBuf, Option<PathBuf>>;

/// Shared interior-mutable memo cells for one driver run (ARCH-1 Ruling 1,
/// tier 1; ARCH-U3 / S01-9).
///
/// One [`Session`] = one driver run. Sites that mint per-module `Session`
/// values (a file-scoped locale pack, a provider analysis config) clone the
/// `Rc` handles through [`Session::re_wrap`], so every re-wrap shares memo
/// identity and cross-module parse-once behavior survives a re-wrap instead of
/// being silently lost per module. Dropping the last handle is the
/// invalidation: nothing outlives the driver run that filled it.
///
/// `RefCell` keeps the current single-threaded compile contract explicit at
/// the carrier — `Session` stays `!Sync`, so the compiler cannot accidentally
/// share it across threads. A future parallel-compile admission is field-local
/// (`RefCell` → `Mutex` on the named cells), not carrier surgery.
#[derive(Clone, Default)]
pub struct SessionMemos {
    /// Import-target parse-once memo (absorbs the retired
    /// `IMPORT_TARGET_CACHE` thread-local of `program/compile.rs`).
    pub(crate) import_targets: Rc<RefCell<ImportTargetMemo>>,
    /// Package-root walk memo (absorbs the retired `PACKAGE_ROOT_CACHE`
    /// thread-local of `import_resolve.rs`).
    pub(crate) package_roots: Rc<RefCell<PackageRootMemo>>,
}

/// Per-compilation driver state.
///
/// The session pairs immutable [`Config`] policy with the driver-run-scoped
/// [`SessionMemos`] cells: config travels by value across re-wraps, memos
/// travel by shared identity, so every `Session` value minted for one driver
/// run — the root session, per-module re-wraps, provider re-wraps — parses
/// each import target and walks each package root at most once.
pub struct Session {
    /// Configuration shared by all phases of this compilation.
    pub config: Config,
    /// Driver-run-scoped memo cells shared across re-wraps.
    pub(crate) memos: SessionMemos,
}

impl Session {
    /// Create a session from caller-supplied configuration.
    ///
    /// Mints fresh memo cells: a new session is a new driver run. Use
    /// [`Session::re_wrap`] to mint the next per-module session value for the
    /// same run.
    #[must_use]
    // public constructor: moves config into session
    pub fn new(config: Config) -> Self {
        Self { config, memos: SessionMemos::default() }
    }

    /// Mint the next per-module session value for the same driver run.
    ///
    /// The returned `Session` owns `config` (a file-scoped locale pack, a
    /// provider analysis contract) but shares memo identity with `self`, so
    /// import-target and package-root lookups issued through the re-wrap hit
    /// the same parse-once cells the rest of the run fills.
    #[must_use]
    pub fn re_wrap(&self, config: Config) -> Self {
        Self { config, memos: self.memos.clone() }
    }
}

#[cfg(test)]
#[path = "session_test.rs"]
mod tests;
