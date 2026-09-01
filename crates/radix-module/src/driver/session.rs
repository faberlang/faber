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

use crate::codegen::{OutputMode, Target};
use crate::diagnostics::Severity;
use crate::locale::LocalePack;
use std::path::PathBuf;

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
    /// Independent of [`Config::locale_root`]. Package-mode resolution and
    /// `FABER_LIBRARY_HOME` own the richer story; this slot is the test and
    /// custom-layout override.
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
            emit_nvvm_device_module: false,
            import_path_templates: None,
            complexity_budget: None,
            kernel_complexity_budget: None,
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

    /// Skip the AIR-level op-fusion pass.
    #[must_use]
    pub fn with_no_fuse(mut self) -> Self {
        self.no_fuse = true;
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

/// Per-compilation driver state.
///
/// The session currently wraps immutable config only. It exists as a deliberate
/// extension point so future cached or incremental state can travel with the
/// same driver API instead of widening phase signatures.
pub struct Session {
    /// Configuration shared by all phases of this compilation.
    pub config: Config,
}

impl Session {
    /// Create a session from caller-supplied configuration.
    #[must_use]
    // public constructor: moves config into session
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

#[cfg(test)]
#[path = "session_test.rs"]
mod tests;
