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
//! - `stdlib_path` is optional; package-mode library resolution owns the richer
//!   stdlib provider story.

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

    /// Optional explicit stdlib path for callers that do not use package mode.
    pub stdlib_path: Option<PathBuf>,

    /// Whether [`Config::stdlib_path`] was supplied explicitly by the caller
    /// (via [`Config::with_stdlib`]) rather than auto-discovered from the
    /// workspace checkout. An explicit stdlib roots library provider
    /// resolution (custom layouts); the auto-discovered dev stdlib is the
    /// reader-pack directory only.
    pub stdlib_explicit: bool,

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target: Target::HirRust,
            emit_comments: true,
            // Frontmatter-locale sources resolve their reader pack through the
            // stdlib path, so the default config discovers the workspace
            // stdlib tree when one is present (no-op for installed builds).
            stdlib_path: discover_dev_stdlib(),
            stdlib_explicit: false,
            warn_policy: WarnPolicy::default(),
            locale_pack: None,
            locale_pack_explicit: false,
            allow_bodyless_functions: false,
            output_mode: OutputMode::Application,
            module_name: None,
            no_fuse: false,
            emit_nvvm_device_module: false,
            import_path_templates: None,
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

    /// Override the stdlib root used by callers outside package resolution.
    ///
    /// An explicit stdlib is a custom layout: it also roots library provider
    /// resolution for consumers that derive the library home from the config
    /// (see the faber `library_resolver_from_config`).
    #[must_use]
    pub fn with_stdlib(mut self, path: PathBuf) -> Self {
        self.stdlib_path = Some(path);
        self.stdlib_explicit = true;
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

    /// Mark the configured reader pack as an explicit caller selection that
    /// wins over any source frontmatter locale (CLI `--locale` semantics).
    #[must_use]
    pub fn with_locale_pack_explicit(mut self) -> Self {
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

    /// Ensure the workspace stdlib directory is configured for this session.
    ///
    /// Most sources now carry `+++ locale = "…" +++` frontmatter whose reader
    /// pack is loaded through [`Config::stdlib_path`], so dev/test sessions
    /// must reach the workspace `stdlib/` tree. [`Config::default`] already
    /// performs this discovery; this builder re-applies it for configs that
    /// were constructed with an explicit empty stdlib slot.
    ///
    /// The dev stdlib is the auto-discovered reader-pack directory, never a
    /// custom layout, so it does not mark the stdlib explicit.
    #[must_use]
    pub fn with_dev_stdlib(mut self) -> Self {
        if let Some(candidate) = discover_dev_stdlib() {
            self.stdlib_path = Some(candidate);
            self.stdlib_explicit = false;
        }
        self
    }
}

/// Discover the workspace stdlib directory from the source checkout layout.
///
/// Returns the `stdlib/` directory under the crate root when the compiled-in
/// `CARGO_MANIFEST_DIR` points to a workspace checkout that contains the
/// expected `stdlib/locale/` tree (`<root>/../../stdlib` in the faberlang
/// container layout). Returns `None` when the binary is installed outside the
/// workspace or the directory is absent, so released builds keep their
/// stdlib-less default.
fn discover_dev_stdlib() -> Option<PathBuf> {
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib");
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
