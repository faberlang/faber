//! Compilation pipeline boundary for `radix`.
//!
//! The driver is the narrow public entrypoint that turns one Faber source unit
//! plus a [`Session`] into either target code or diagnostics. It owns phase
//! ordering, diagnostic accumulation, target-policy checks that require parsed
//! syntax, and the CLI-program handoff used by package builds. Individual
//! compiler phases still own their domain models; this module decides when a
//! later phase has enough trustworthy input to run.
//!
//! INVARIANTS
//! ==========
//! - Lexing, parsing, CLI analysis, semantic analysis, and codegen run in that
//!   order; later phases are skipped after errors from their prerequisites.
//! - Diagnostics are accumulated through successful phases so warnings survive
//!   into a successful [`CompileResult`].
//! - Target-specific policy that depends on syntax is enforced before semantic
//!   lowering, keeping unsupported constructs out of backends that cannot model
//!   them safely.
//! - CLI overrides supplied by package compilation are retargeted into the
//!   current parser interner before semantic analysis observes them.
//!
//! ERROR STRATEGY
//! ==============
//! The driver returns diagnostics rather than panicking when a phase rejects user
//! input. A "successful" phase that violates its internal contract, such as a
//! parse result without a program, is converted into an explicit diagnostic so
//! callers still receive the same failure surface.

mod evm_contract;
#[cfg(feature = "hir-faber")]
pub mod faber_format;
mod file_frontmatter;
mod frontmatter;
mod insere;
mod praefixum;
mod qualify;
mod session;
#[cfg(feature = "mir-core")]
mod shader_contract;
mod source;
pub(crate) mod source_inventory;
mod target_bindings;
mod target_policy;

pub use crate::semantic::{AnalyzedModuleApplicationView, PackageImportIdentities};
pub use evm_contract::{forbid_contract_environment, require_contract_execution_target};
#[cfg(feature = "hir-faber")]
pub use faber_format::{FaberReemitResult, emit_from_analyzed, reemit_faber};
pub use file_frontmatter::{FileFrontmatter, parse_file_frontmatter};
pub use frontmatter::{FrontmatterError, SplitFrontmatter, split_frontmatter};
#[cfg(feature = "runner")]
pub use praefixum::{evaluate_deferred, run_linked_function};
pub use qualify::{evaluate_praefixum_for_importers, keeps_authored_form, qualify_for_target};
pub use radix_types::KernelLowering;
pub use session::{Config, LocalePackLoader, Session, SessionMemos, WarnPolicy};
pub use source::{PeeledSource, SourceFile, SourceLoadError, peel_raw_source, source_load_diagnostic};
pub use target_bindings::TargetBindings;

use crate::CompileResult;
use crate::codegen::input::{CodegenAnalysis, CodegenProvider};
use crate::codegen::{self, Target};
use crate::diagnostics::DiagnosticConvert;
use crate::diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase, Severity, Span, locale_unknown_spec};
use crate::hir::visit::{HirVisitor, walk_expr};
use crate::hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirModule, HirModuleEnd,
    HirPresentation, HirSourceAnchorId, LibraryRegistry,
};
use crate::lexer;
use crate::lexer::Interner;
use crate::locale::{KeywordSurface, LocalePack, latin_locale_pack};
use crate::parser;
use crate::semantic::{self, PassConfig, Primitive, RadixLane, RadixLaneMetadata, Resolver, Type};
#[allow(clippy::wildcard_imports)]
// syntax is a pure re-export barrel from radix-syntax
use crate::syntax::*;
use std::collections::BTreeMap;

pub(crate) const MISSING_PARSE_PROGRAM_MESSAGE: &str = "successful parse result missing program";
const MISSING_SEMANTIC_HIR_MESSAGE: &str = "successful semantic result missing lowered HIR";

/// Promote warning diagnostics to errors according to a [`WarnPolicy`].
///
/// This is the boundary-promotion step: phases keep emitting `Severity::Warning`
/// unchanged, and this function reclassifies matching warnings to
/// `Severity::Error` in place. Code, span, and help text are preserved; only
/// severity flips. `Severity::Info` and `Severity::Error` are never changed.
pub fn apply_warn_policy(diags: &mut [Diagnostic], policy: &WarnPolicy) {
    for d in diags.iter_mut() {
        if policy.promotes(d.severity, d.code) {
            d.severity = Severity::Error;
        }
    }
}

/// Caller-supplied import contracts for fail-closed module analysis.
///
/// Empty maps stay empty. Module analysis never derives seams, exports, or
/// file interfaces from a path-shaped name.
#[derive(Debug, Clone, Default)]
pub struct ModuleImportContract {
    /// Importable names per binding, with their visibility tier.
    pub namespace_exports:
        BTreeMap<String, BTreeMap<String, crate::semantic::passes::visibility_annotation::VisibilityTier>>,
    /// Portable typed export interfaces keyed by import binding name.
    pub file_interfaces: BTreeMap<String, crate::file_interface::FileInterface>,
    /// Per-binding same-package flag. Missing bindings are cross-package.
    pub namespace_seams: BTreeMap<String, bool>,
    /// All target top-level names including module-private, keyed by binding.
    pub namespace_declarations:
        BTreeMap<String, BTreeMap<String, crate::semantic::passes::visibility_annotation::VisibilityTier>>,
    /// Import path spelling → the target's declared `regio` name (D7.7).
    /// Default-named imports of these paths bind the declared name instead of
    /// the last path segment; the keys of every map above already use it.
    pub default_import_names: BTreeMap<String, String>,
}

/// The analyzed facts HIR provider-module emission maps over (ARCH-U4 /
/// S01-3): one import target's unit, analyzed once per driver run where the
/// import-contract walk already parses it, under the contract codegen needs
/// (bodyless functions + library-registry defaults). This is the same fact
/// set the Rust provider-module emitter used to re-derive from disk at every
/// codegen call.
pub struct ProviderUnit {
    /// Lowered program of the provider module.
    pub hir: HirModule,
    /// Type information produced by the provider's semantic analysis.
    pub types: semantic::TypeTable,
    /// Symbol table owning the provider's names.
    pub interner: Interner,
    /// Resolver metadata carrying the provider's own file interfaces.
    pub resolver: Resolver,
    /// Provider dispatch facts retained from this unit's semantic analysis.
    pub tensor_edge_policies: radix_hir::TensorEdgePolicyTable,
    /// Runtime tensor shape guards retained from this unit's semantic analysis.
    pub shape_obligations: semantic::ShapeObligationTable,
    /// Warnings and informational diagnostics from the provider's analysis;
    /// emission fails closed when any is an error.
    pub diagnostics: Vec<Diagnostic>,
    /// Carried units for the provider's own nested namespace imports.
    pub nested: ProviderUnitView,
}

/// One analyzed provider module carried for HIR emission (ARCH-U4 / S01-3).
///
/// The unit is shared by every import edge that resolved to the same target
/// in one driver run. It sits behind a [`std::sync::Mutex`] for one reason:
/// [`AnalyzedModule`] is `Send` (the `faber test` budget thread sends whole
/// analyzed programs across a channel), and shared ownership of the
/// `!Sync` analysis arenas would take that contract away. The lock is
/// uncontended in the single-threaded compile — it preserves the existing
/// `Send` surface, it does not introduce parallelism. A target that resolved
/// and parsed but failed contract analysis carries its failure instead —
/// emission fails closed on the marker rather than re-entering the front half
/// to rediscover the failure.
#[derive(Clone)]
pub enum CarriedProviderUnit {
    /// Analyzed unit shared across the driver run's import edges.
    Unit(std::sync::Arc<std::sync::Mutex<ProviderUnit>>),
    /// Contract discovery or analysis failed for this target; `message` is
    /// the fail-closed emission error's tail clause.
    Failed {
        /// What failed and the leading diagnostic message.
        message: String,
    },
}

impl CarriedProviderUnit {
    /// Lock and borrow the analyzed unit, or `None` for a failed-carriage
    /// marker. Poison-tolerant: a panic inside emission does not cascade
    /// into later locks (the compile contract is single-threaded).
    #[must_use]
    pub fn lock_unit(&self) -> Option<ProviderUnitGuard<'_>> {
        match self {
            Self::Unit(unit) => Some(ProviderUnitGuard(
                unit.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            )),
            Self::Failed { .. } => None,
        }
    }
}

/// Borrowed view of one carried provider unit (see
/// [`CarriedProviderUnit::lock_unit`]).
pub struct ProviderUnitGuard<'a>(std::sync::MutexGuard<'a, ProviderUnit>);

impl std::ops::Deref for ProviderUnitGuard<'_> {
    type Target = ProviderUnit;

    fn deref(&self) -> &ProviderUnit {
        &self.0
    }
}

/// Per-module view of analyzed provider units, keyed by import-path spelling
/// (ARCH-U4 / S01-3).
///
/// One entry per direct import edge that resolved and parsed during this
/// module's contract walk; the driver carries the view on the analyzed output
/// and HIR emission maps over it instead of re-reading providers from disk —
/// the O(consumers × providers) front-half re-runs end and the codegen tree
/// loses its last filesystem dependency.
#[derive(Clone, Default)]
pub struct ProviderUnitView {
    /// Carried units by import-path spelling as written by the importer.
    pub(crate) units: BTreeMap<String, CarriedProviderUnit>,
}

impl ProviderUnitView {
    /// The carried unit for one import-path spelling, when the edge resolved.
    #[must_use]
    pub fn get(&self, import_path: &str) -> Option<&CarriedProviderUnit> {
        self.units.get(import_path)
    }

    /// Record the carried unit for one import edge. Writers are the
    /// program-layer contract walk and the package-analysis carriage
    /// (radix-program installs per-file views after minting through
    /// [`crate::program::provider_unit_for_target`]).
    pub fn insert(&mut self, import_path: &str, unit: CarriedProviderUnit) {
        self.units.insert(import_path.to_owned(), unit);
    }

    /// Whether no provider units are carried (plain single-file units).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
}

impl std::fmt::Debug for ProviderUnitView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deliberately lock-free: Debug must not block on (or panic through)
        // a unit some emission thread is holding.
        f.debug_map()
            .entries(self.units.iter().map(|(path, unit)| {
                let kind = match unit {
                    CarriedProviderUnit::Unit(_) => "Unit",
                    CarriedProviderUnit::Failed { .. } => "Failed",
                };
                (path, kind)
            }))
            .finish()
    }
}

impl PartialEq for ProviderUnitView {
    /// Carriage identity, not surface equality: two views are equal when they
    /// carry the same edges, with `Unit` entries compared by shared identity
    /// (the same driver run parses a target once) and `Failed` entries by
    /// message.
    fn eq(&self, other: &Self) -> bool {
        self.units.len() == other.units.len()
            && self
                .units
                .iter()
                .all(|(path, unit)| match other.units.get(path) {
                    Some(CarriedProviderUnit::Unit(other_unit)) => match unit {
                        CarriedProviderUnit::Unit(unit) => std::sync::Arc::ptr_eq(unit, other_unit),
                        CarriedProviderUnit::Failed { .. } => false,
                    },
                    Some(CarriedProviderUnit::Failed { message }) => match unit {
                        CarriedProviderUnit::Unit(_) => false,
                        CarriedProviderUnit::Failed { message: own } => own == message,
                    },
                    None => false,
                })
    }
}

/// Caller-supplied inputs for the single [`compile`] entry.
///
/// The contract drives fail-closed module analysis; package-aware callers that
/// resolved local sibling imports also pass canonical identity facts so MIR
/// lowering records `(product, module_path, item)` identities for user-code
/// cross-module calls instead of the raw import spelling.
#[derive(Debug, Clone, Default)]
pub struct ModuleCompileRequest {
    /// Fail-closed import contract for the resolver.
    pub contract: ModuleImportContract,
    /// Package-local import identity facts, when already resolved.
    pub package_identities: Option<PackageImportIdentities>,
    /// Analyzed provider units for this module's import edges (ARCH-U4 /
    /// S01-3), carried from the program-layer contract walk so backend
    /// emission maps over them instead of re-analyzing providers from disk.
    pub provider_units: ProviderUnitView,
}

/// Run analysis and code generation for one source unit.
///
/// This is the user-facing driver contract used by CLIs and package tooling
/// when they want emitted target code. It delegates the shared front half to
/// [`analyze_source`] so callers that only need HIR, type tables, or CLI
/// analysis can stop before backend generation, and the analysis-to-output
/// tail to [`compile_analyzed`], which is also public for callers that
/// already hold an analyzed module.
///
/// Errors from any phase are returned as diagnostics with no output. Warnings
/// collected before codegen are preserved on success.
#[must_use]
pub fn compile(session: &Session, name: &str, source: &str, request: ModuleCompileRequest) -> CompileResult {
    let ModuleCompileRequest { contract, package_identities, provider_units } = request;
    let analysis = match analyze_source(session, name, source, AnalyzeRequest { cli_program: None, contract }) {
        Ok(mut analysis) => {
            analysis.package_import_identities = package_identities;
            analysis.provider_units = provider_units;
            analysis
        }
        Err(mut diagnostics) => {
            apply_warn_policy(&mut diagnostics, &session.config.warn_policy);
            return CompileResult { output: None, diagnostics };
        }
    };
    compile_analyzed(session, name, source, analysis)
}

/// Compile an already-analyzed module to output.
///
/// This is the analysis-to-output tail of [`compile`]: the warn policy, target
/// qualification ([`qualify_for_target`]), the CLI-program handoff, and backend
/// generation, run over an [`AnalyzedModule`]
/// from [`analyze_source`] (or loaded from an artifact). The result is
/// identical to [`compile`] on the same analyzed facts, so callers that
/// analyzed through a different route hand the module here instead of
/// re-running the front half. The source text stays part of the contract: it
/// only feeds diagnostic source context and CLI-program lowering.
///
/// Errors are returned as diagnostics with no output. Warnings collected
/// before codegen are preserved on success.
#[must_use]
pub fn compile_analyzed(session: &Session, name: &str, source: &str, analyzed: AnalyzedModule) -> CompileResult {
    let mut analysis = analyzed;

    // Promote warnings to errors per the session's deny policy before any
    // downstream path assembles a CompileResult from analysis.diagnostics.
    apply_warn_policy(&mut analysis.diagnostics, &session.config.warn_policy);
    if analysis.diagnostics.iter().any(Diagnostic::is_error) {
        return CompileResult { output: None, diagnostics: analysis.diagnostics };
    }

    if let Err(error) = evm_contract::reject_wrong_target(&analysis, session.config.target) {
        return compile_codegen_error(analysis, name, source, error);
    }

    // A unit that arrives here without passing through `analyze_source` (loaded
    // from a FHIR artifact) is target-neutral until it is qualified; one that
    // did is unchanged by a second pass.
    if let Err(qualify_errors) = qualify::qualify_for_target(&mut analysis, &session.config, name, source) {
        analysis.diagnostics.extend(qualify_errors);
        return CompileResult { output: None, diagnostics: analysis.diagnostics };
    }

    if analysis.cli_program.is_some() {
        return compile_cli_program(&session.config, name, source, analysis);
    }

    // -------------------------------------------------------------------------
    // PHASE 4: CODE GENERATION
    // Emit target-specific source code
    // -------------------------------------------------------------------------
    let latin_pack;
    let surface = KeywordSurface::new(if let Some(pack) = &session.config.locale_pack {
        pack
    } else {
        latin_pack = latin_locale_pack();
        &latin_pack
    });
    match generate_output(
        session.config.target,
        &mut analysis,
        &surface,
        session.config.output_mode,
        session.config.module_name.as_deref(),
        session.config.no_fuse,
        session.config.emit_nvvm_device_module,
        &session.config.target_bindings,
        #[cfg(feature = "mir-clif")]
        &session.config.clif_engine,
    ) {
        Ok(output) => CompileResult { output: Some(output), diagnostics: analysis.diagnostics },
        Err(err) => compile_codegen_error(analysis, name, source, err),
    }
}

fn missing_cli_program(analysis: AnalyzedModule, name: &str) -> CompileResult {
    compile_error(
        analysis,
        codegen_diagnostic(name, "CODEGEN003", "CLI code generation requires an analyzed CLI program"),
    )
}

fn compile_cli_program(config: &Config, name: &str, source: &str, analysis: AnalyzedModule) -> CompileResult {
    let target = config.target;
    let no_fuse = config.no_fuse;
    if let Some(diagnostic) = cli_codegen_diagnostic(target, name, &analysis) {
        return compile_error(analysis, diagnostic);
    }

    match target {
        // Wasm text: the shared MIR CLI-program lowering (P1, a5e4b69f1)
        // already produced the CLI entry/argv/exit shapes in MIR, so the
        // CLI program routes through the ordinary wasm emit path
        // (wasm-host-parity P1 residual — the ten CLI fixtures emit and
        // wasm-tools validate). The emitter fails closed on any shape it
        // does not support.
        // Cranelift: the shared MIR CLI-program lowering already produced the
        // entry/argv/exit shapes; the classifier declines any it cannot lower.
        #[cfg(feature = "mir-clif")]
        Target::MirClif => {
            let mut analysis = analysis;
            match generate_clif_output(&mut analysis, no_fuse, &config.clif_engine) {
                Ok(output) => CompileResult { output: Some(output), diagnostics: analysis.diagnostics },
                Err(err) => compile_codegen_error(analysis, name, source, err),
            }
        }
        #[cfg(feature = "mir-wasm")]
        Target::MirWasm => {
            let mut analysis = analysis;
            match generate_wasm_text_output(&mut analysis, no_fuse) {
                Ok(output) => CompileResult { output: Some(output), diagnostics: analysis.diagnostics },
                Err(err) => compile_codegen_error(analysis, name, source, err),
            }
        }
        Target::HirRust => {
            let Some(cli_program) = analysis.cli_program.as_ref() else {
                return missing_cli_program(analysis, name);
            };
            match codegen::generate_rust_cli(
                &analysis.hir,
                &analysis.types,
                &analysis.interner,
                cli_program,
                &analysis.display_tokens,
            ) {
                Ok(output) => {
                    CompileResult { output: Some(crate::Output::Rust(output)), diagnostics: analysis.diagnostics }
                }
                Err(err) => compile_codegen_error(analysis, name, source, err.into()),
            }
        }
        Target::HirGo => {
            let Some(cli_program) = analysis.cli_program.as_ref() else {
                return missing_cli_program(analysis, name);
            };
            match codegen::generate_go_cli(
                &analysis.hir,
                &analysis.types,
                &analysis.interner,
                cli_program,
                analysis.display_tokens().clone(),
            ) {
                Ok(output) => {
                    CompileResult { output: Some(crate::Output::Go(output)), diagnostics: analysis.diagnostics }
                }
                Err(err) => compile_codegen_error(analysis, name, source, err.into()),
            }
        }
        Target::HirTypeScript => {
            let Some(cli_program) = analysis.cli_program.as_ref() else {
                return missing_cli_program(analysis, name);
            };
            match codegen::generate_ts_cli(&analysis.hir, &analysis.types, &analysis.interner, cli_program) {
                Ok(output) => {
                    CompileResult { output: Some(crate::Output::TypeScript(output)), diagnostics: analysis.diagnostics }
                }
                Err(err) => compile_codegen_error(analysis, name, source, err.into()),
            }
        }
        Target::HirPython => {
            let Some(cli_program) = analysis.cli_program.as_ref() else {
                return missing_cli_program(analysis, name);
            };
            match codegen::generate_python_cli(&analysis.hir, &analysis.types, &analysis.interner, cli_program) {
                Ok(output) => {
                    CompileResult { output: Some(crate::Output::Python(output)), diagnostics: analysis.diagnostics }
                }
                Err(err) => compile_codegen_error(analysis, name, source, err.into()),
            }
        }
        // Fail-closed backstop: a target that declares `lowers_cli_entry`
        // but has no arm above (or whose feature is disabled) is refused.
        _ => compile_error(analysis, cli_codegen_refusal(target, name)),
    }
}

/// `CODEGEN002` `cli_codegen_rust_only`: the target does not lower a CLI
/// entry program. Shared by the capability gate and the dispatch backstop.
fn cli_codegen_refusal(target: Target, name: &str) -> Diagnostic {
    codegen_diagnostic(
        name,
        "CODEGEN002",
        "runnable CLI code generation is only implemented for Rust and the Go narrow slice",
    )
    .with_args(vec![
        DiagnosticArg::new("issue", "cli_codegen_rust_only"),
        DiagnosticArg::new("target", target.as_str()),
    ])
}

fn cli_codegen_diagnostic(target: Target, name: &str, analysis: &AnalyzedModule) -> Option<Diagnostic> {
    let cli_program = analysis.cli_program.as_ref()?;

    if !target.lowers_cli_entry() {
        return Some(cli_codegen_refusal(target, name));
    }

    match target {
        Target::HirRust => {
            if matches!(cli_program.exit, Some(crate::cli::CliExit::Unsupported)) {
                return Some(codegen_diagnostic(
                    name,
                    "CODEGEN004",
                    "CLI exitus code generation supports numeric literals, bindings, and args fields in Phase 03",
                ));
            }
            phase03_cli_codegen_gap(cli_program).map(|(issue, message)| {
                codegen_diagnostic(name, "CODEGEN005", message).with_args(vec![DiagnosticArg::new("issue", issue)])
            })
        }
        Target::HirGo => go_cli_codegen_diagnostic(name, cli_program),
        // TypeScript lowers the narrow CLI slice inside its own emitter; a
        // surface beyond the slice fails closed there with a stable issue.
        // An unsupported `exitus` expression is reported early here so the
        // diagnostic is identical to the Rust arm's CODEGEN004.
        Target::HirTypeScript => {
            if matches!(cli_program.exit, Some(crate::cli::CliExit::Unsupported)) {
                return Some(codegen_diagnostic(
                    name,
                    "CODEGEN004",
                    "CLI exitus code generation supports numeric literals, bindings, and args fields in Phase 03",
                ));
            }
            None
        }
        // Python lowers the narrow CLI slice inside its own emitter; a
        // surface beyond the slice fails closed there with a stable issue.
        // An unsupported `exitus` expression is reported early here so the
        // diagnostic is identical to the Rust arm's CODEGEN004.
        Target::HirPython => {
            if matches!(cli_program.exit, Some(crate::cli::CliExit::Unsupported)) {
                return Some(
                    codegen_diagnostic(
                        name,
                        "CODEGEN004",
                        "CLI exitus code generation supports numeric literals, bindings, and args fields in Phase 03",
                    )
                    .with_args(vec![
                        DiagnosticArg::new("issue", "py_cli_exit_unsupported"),
                        DiagnosticArg::new("target", "python"),
                    ]),
                );
            }
            None
        }
        // Wasm text routes CLI programs through the ordinary emit path (the
        // shared MIR CLI lowering already produced the entry/argv/exit
        // shapes); the emitter fails closed on unsupported shapes itself.
        // Any other capable target reaches the `compile_cli_program`
        // backstop when it has no dispatch arm.
        _ => None,
    }
}

#[cfg(feature = "hir-go")]
fn go_cli_codegen_diagnostic(name: &str, cli_program: &crate::cli::CliProgram) -> Option<Diagnostic> {
    match codegen::go::qualify_go_cli(Some(cli_program)) {
        codegen::go::GoCliQualification::SupportedNarrow { .. } => None,
        codegen::go::GoCliQualification::NotCli => Some(
            codegen_diagnostic(name, "CODEGEN003", "CLI code generation requires an analyzed CLI program")
                .with_args(vec![DiagnosticArg::new("issue", "go_cli_missing_program")]),
        ),
        codegen::go::GoCliQualification::Rejected(rejections) => {
            let first = rejections.first().map_or("go_cli_unsupported", |r| r.issue);
            let surfaces = rejections
                .iter()
                .map(|r| format!("{} ({})", r.issue, r.surface))
                .collect::<Vec<_>>()
                .join("; ");
            Some(
                codegen_diagnostic(name, "CODEGEN002", format!("Go CLI surface rejected: {surfaces}")).with_args(vec![
                    DiagnosticArg::new("issue", first),
                    DiagnosticArg::new("target", "go"),
                    DiagnosticArg::new("surface", surfaces),
                ]),
            )
        }
    }
}

#[cfg(not(feature = "hir-go"))]
fn go_cli_codegen_diagnostic(name: &str, _cli_program: &crate::cli::CliProgram) -> Option<Diagnostic> {
    Some(
        codegen_diagnostic(name, "CODEGEN002", "Go target is not available in this radix build").with_args(vec![
            DiagnosticArg::new("issue", "target_unavailable"),
            DiagnosticArg::new("target", "go"),
            DiagnosticArg::new("feature", "hir-go"),
        ]),
    )
}

fn codegen_diagnostic(name: &str, code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(message)
        .with_phase(DiagnosticPhase::Codegen)
        .with_code(code)
        .with_file(name.to_owned())
}

fn compile_error(mut analysis: AnalyzedModule, diagnostic: Diagnostic) -> CompileResult {
    analysis.diagnostics.push(diagnostic);
    CompileResult { output: None, diagnostics: analysis.diagnostics }
}

/// Codegen error with an optional source span recovered from MIR/AIR lowering.
pub struct SpannedCodegenError {
    pub message: String,
    pub args: Vec<DiagnosticArg>,
    pub span: Option<Span>,
}

impl From<codegen::CodegenError> for SpannedCodegenError {
    fn from(err: codegen::CodegenError) -> Self {
        Self { message: err.message, args: err.args, span: None }
    }
}

// `not(feature = "mir-amd")` included because the MirAmd fail-closed arm is
// gated only on that feature, while faber's full-targets enables the other
// four emitters without mir-amd (P1: E0425 without this condition).
#[cfg(any(
    not(feature = "mir-amd"),
    not(feature = "mir-llvm"),
    not(feature = "mir-metal"),
    not(feature = "mir-sexp"),
    not(feature = "mir-basic"),
    not(feature = "mir-wasm"),
    not(feature = "mir-wgsl")
))]
fn unavailable_target_spanned(target: Target) -> SpannedCodegenError {
    let feature = target.required_feature().unwrap_or("full-targets");
    SpannedCodegenError {
        message: format!("target `{target}` is not available in this radix build; rebuild with feature `{feature}`"),
        args: vec![
            DiagnosticArg::new("issue", "target_unavailable"),
            DiagnosticArg::new("target", target.to_string()),
            DiagnosticArg::new("feature", feature),
        ],
        span: None,
    }
}

#[cfg(feature = "mir-core")]
fn mir_errors_to_spanned(errors: Vec<crate::mir::MirError>) -> SpannedCodegenError {
    let span = errors.first().map(|e| e.span);
    let message = errors
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("; ");
    let args = errors
        .iter()
        .map(|e| DiagnosticArg::new("issue", e.issue.clone()))
        .collect();
    SpannedCodegenError { message, args, span }
}

#[cfg(feature = "device")]
fn air_lower_errors_to_spanned(errors: Vec<crate::air::AirLowerError>) -> SpannedCodegenError {
    let span = errors.first().map(|e| e.span);
    let message = errors
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("; ");
    let args = errors
        .iter()
        .map(|e| DiagnosticArg::new("issue", e.issue.clone()))
        .collect();
    SpannedCodegenError { message, args, span }
}

#[cfg(feature = "device")]
fn air_validation_errors_to_spanned(errors: Vec<crate::air::AirValidationError>) -> SpannedCodegenError {
    let span = errors.first().map(|e| e.span);
    let message = errors
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("; ");
    let args = errors
        .iter()
        .map(|e| DiagnosticArg::new("issue", e.issue))
        .collect();
    SpannedCodegenError { message, args, span }
}

#[cfg(feature = "device")]
fn air_to_mir_errors_to_spanned(errors: Vec<radix_air_bridge::AirToMirError>) -> SpannedCodegenError {
    let span = errors.first().map(|e| e.span);
    let message = errors
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("; ");
    let args = errors
        .iter()
        .map(|e| DiagnosticArg::new("issue", e.issue.clone()))
        .collect();
    SpannedCodegenError { message, args, span }
}

#[cfg(feature = "mir-core")]
fn mir_validation_errors_to_spanned(errors: Vec<crate::mir::MirValidationError>) -> SpannedCodegenError {
    let span = errors.first().map(|e| e.span);
    let message = errors
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("; ");
    let args = errors
        .iter()
        .map(|e| DiagnosticArg::new("issue", e.issue.clone()))
        .collect();
    SpannedCodegenError { message, args, span }
}

fn compile_codegen_error(
    analysis: AnalyzedModule,
    name: &str,
    source: &str,
    err: SpannedCodegenError,
) -> CompileResult {
    let is_evm_refusal = err
        .args
        .iter()
        .any(|arg| arg.name == "issue" && matches!(arg.value.as_str(), "evm_unsupported" | "contract_target_mismatch"));
    let mut diagnostic = Diagnostic::codegen_error(&err.message).with_args(err.args);
    // CODEGEN001 is an uncatalogued backend envelope. The experimental EVM
    // refusal uses the existing catalogued lowering/package family so packs
    // can render its structured reason and shape.
    if is_evm_refusal {
        diagnostic = diagnostic.with_code("PKG001");
    }
    if let Some(span) = err.span {
        diagnostic = diagnostic.with_source_context(name, source, span);
    } else {
        diagnostic = diagnostic.with_file(name);
    }
    compile_error(analysis, diagnostic)
}

#[cfg_attr(not(feature = "mir-llvm"), allow(unused_variables))]
fn generate_output(
    target: Target,
    analysis: &mut AnalyzedModule,
    surface: &KeywordSurface<'_>,
    output_mode: codegen::OutputMode,
    module_name: Option<&str>,
    no_fuse: bool,
    emit_nvvm_device_module: bool,
    target_bindings: &TargetBindings,
    #[cfg(feature = "mir-clif")] clif_engine: &crate::ClifEngine,
) -> Result<crate::Output, SpannedCodegenError> {
    // Raw-bits types (b8..b64) have a carrier only on the Faber source emitter
    // and the MIR runner; every other target fails closed, named, before emit.
    radix_codegen_shared::reject_bits_type(&analysis.types, target)?;
    // A ledger (`ledger<K, V>`) likewise has no carrier on any target yet.
    radix_codegen_shared::reject_ledger_type(&analysis.types, target)?;
    // A ranged store policy is applied only by the Faber re-emitter, the MIR
    // runner and the Rust target; every other target refuses it by name.
    radix_codegen_shared::reject_ranged_int_type(&analysis.types, target)?;
    radix_codegen_shared::reject_ranged_list_type(&analysis.types, target)?;
    crate::codegen::reject_unsupported_leaves(analysis, target)?;
    // A list window `xs[lo‥hi]` likewise has only those two executors; every
    // other target refuses it by one named issue until its emission lands.
    radix_codegen_shared::reject_list_window(&analysis.hir, &analysis.types, target)?;
    match target {
        #[cfg(feature = "mir-evm")]
        Target::MirEvm => generate_evm_output(analysis, no_fuse),
        #[cfg(not(feature = "mir-evm"))]
        Target::MirEvm => Err(unavailable_target_spanned(Target::MirEvm)),
        Target::MirStepper => Err(codegen::CodegenError {
            message: "stepper is the core interpreter target; run through the MIR stepper, not codegen emit".to_owned(),
            args: vec![
                DiagnosticArg::new("issue", "stepper_not_codegen_target"),
                DiagnosticArg::new("target", "mir-stepper"),
            ],
        }
        .into()),
        #[cfg(feature = "mir-wasm")]
        Target::MirWasm => {
            // wasm-host-parity emit boundary: the Wasm text target now lowers
            // the modular-word family (modulus<u8|u16|u32|u64>) through the
            // sized-numeric carrier policy (see radix-mir-wasm operand.rs
            // `scalar_ty`); the shared MIR layout classifies ModularWord with
            // `int_layout`. All other unsupported targets keep the gate.
            generate_wasm_text_output(analysis, no_fuse)
        }
        #[cfg(not(feature = "mir-wasm"))]
        Target::MirWasm => Err(unavailable_target_spanned(Target::MirWasm)),
        #[cfg(feature = "mir-llvm")]
        Target::MirLlvm | Target::MirLlvmHost => {
            // llvm-host-parity emit boundary: the LLVM text target now lowers
            // the modular-word family (modulus<u8|u16|u32|u64>) as unsigned
            // integral scalars. All other unsupported targets keep the gate.
            // `llvm-host` is the product routing name for the SAME emitter.
            generate_llvm_text_output(analysis, no_fuse, emit_nvvm_device_module)
        }
        #[cfg(not(feature = "mir-llvm"))]
        Target::MirLlvm | Target::MirLlvmHost => Err(unavailable_target_spanned(Target::MirLlvm)),
        #[cfg(feature = "mir-clif")]
        Target::MirClif => generate_clif_output(analysis, no_fuse, clif_engine),
        #[cfg(not(feature = "mir-clif"))]
        Target::MirClif => Err(unavailable_target_spanned(Target::MirClif)),
        Target::MirAmd => {
            // AMD-A2 fail-closed lane diagnostic (goal AC1 / DDCP0
            // feature-gate policy): without the `mir-amd` feature the AMD
            // lane is not compiled and fails closed with the stable
            // `target_unavailable` diagnostic naming the missing feature —
            // never a silent fallback. With the feature, the emit route is a
            // follow-on unit (the leaf emitter is the leaf-probe surface);
            // the driver still fails closed honestly instead of emitting
            // nothing.
            #[cfg(not(feature = "mir-amd"))]
            {
                Err(unavailable_target_spanned(Target::MirAmd))
            }
            #[cfg(feature = "mir-amd")]
            {
                Err(codegen::CodegenError {
                    message: "the amdgpu emit lane is not yet wired into the radix driver (the `mir-amd` leaf compiles under the feature; the emit route is a follow-on unit)".to_owned(),
                    args: vec![
                        DiagnosticArg::new("issue", "amdgpu_emit_lane_not_wired"),
                        DiagnosticArg::new("target", "mir-amd"),
                    ],
                }
                .into())
            }
        }
        #[cfg(feature = "mir-metal")]
        Target::MirMetal => {
            reject_kernel_target_requirements(analysis, "Metal text", Target::MirMetal)?;
            reject_saturating_word_target(analysis, "Metal text")?;
            generate_metal_text_output(analysis, no_fuse)
        }
        #[cfg(not(feature = "mir-metal"))]
        Target::MirMetal => Err(unavailable_target_spanned(Target::MirMetal)),
        #[cfg(feature = "mir-wgsl")]
        Target::MirWgsl => {
            reject_kernel_target_requirements(analysis, "WGSL text", Target::MirWgsl)?;
            reject_saturating_word_target(analysis, "WGSL text")?;
            generate_wgsl_text_output(analysis, no_fuse, target_bindings)
        }
        #[cfg(not(feature = "mir-wgsl"))]
        Target::MirWgsl => Err(unavailable_target_spanned(Target::MirWgsl)),
        #[cfg(feature = "mir-sexp")]
        Target::MirSexp => {
            // D11.6 ss5: Racket integers are exact; word results reduce or
            // clamp at the operation and the store, so the word gates are lifted.
            generate_sexp_output(analysis, no_fuse)
        }
        #[cfg(not(feature = "mir-sexp"))]
        Target::MirSexp => Err(unavailable_target_spanned(Target::MirSexp)),
        #[cfg(feature = "mir-basic")]
        Target::MirBasic => generate_basic_output(analysis, no_fuse),
        #[cfg(not(feature = "mir-basic"))]
        Target::MirBasic => Err(unavailable_target_spanned(Target::MirBasic)),
        Target::MirScena => Err(codegen::CodegenError {
            message: "scena package artifacts must be built through `faber build --target scena`".to_owned(),
            args: vec![
                DiagnosticArg::new("issue", "package_artifact_route_required"),
                DiagnosticArg::new("target", "scena"),
            ],
        }
        .into()),
        Target::MirFmir => Err(codegen::CodegenError {
            message: "fmir-text package images must be built through `faber build --target fmir-text`".to_owned(),
            args: vec![
                DiagnosticArg::new("issue", "package_artifact_route_required"),
                DiagnosticArg::new("target", "fmir-text"),
            ],
        }
        .into()),
        Target::MirFmirBinary => Err(codegen::CodegenError {
            message: "fmir package images must be built through `faber build --target fmir`".to_owned(),
            args: vec![
                DiagnosticArg::new("issue", "package_artifact_route_required"),
                DiagnosticArg::new("target", "fmir"),
            ],
        }
        .into()),
        Target::HirFhir => Err(codegen::CodegenError {
            message: "Target::HirFhir is a serialization dispatch, not a codegen backend. Use the emit workflow."
                .to_owned(),
            args: vec![
                DiagnosticArg::new("issue", "fhir_serialization_dispatch_only"),
                DiagnosticArg::new("target", "fhir"),
            ],
        }
        .into()),
        // S7-U5: Swift's fixed-width carriers and wrapping operators lifted its
        // modular-word gate. S7-U6: TS's per-width wrapping and passing fixtures
        // lifted its stale gate. Both targets now share the forwarding path.
        // D11.6 U6go: Go's exact-integer helpers and typed wrapping carriers
        // lifted its modular-word gate as well.
        Target::HirRust
        | Target::HirFaber
        | Target::HirSwift
        | Target::HirHaskell
        | Target::HirPascal
        | Target::HirCobol
        | Target::HirFortran
        | Target::HirOdin
        | Target::HirC
        | Target::HirTypeScript
        | Target::HirGo
        // D11.6 py4: Python ints are exact; the clamp runs at the store.
        | Target::HirPython => {
            codegen::generate_from_analyzed_with_options(target, analysis, surface, output_mode, module_name)
                .map_err(std::convert::Into::into)
        }
    }
}

fn reject_kernel_target_requirements(
    analysis: &AnalyzedModule,
    target_name: &str,
    target: Target,
) -> Result<(), codegen::CodegenError> {
    // Kernel numerics are admitted only when the same typed-HIR snapshot
    // carries a positive per-target requirement row. Unsupported widths and
    // unproved stores stay closed here; conditional extent rows remain
    // admitted for the launch boundary to evaluate.
    for item in &analysis.hir.items {
        let mut kernels = Vec::new();
        match &item.kind {
            HirItemKind::Function(function) if function.is_kernel => {
                kernels.push((item.def_id, function.name));
            }
            HirItemKind::Struct(strukt) => {
                kernels.extend(
                    strukt
                        .methods
                        .iter()
                        .filter(|method| method.func.is_kernel)
                        .map(|method| (method.def_id, method.func.name)),
                );
            }
            _ => {}
        }
        for (def_id, name) in kernels {
            let Some(requirement) = analysis.kernel_requirements.target(def_id, target) else {
                return Err(codegen::CodegenError {
                    message: format!(
                        "kernel `{}` has no inferred device requirements for {target_name}",
                        analysis.interner.resolve(name)
                    ),
                    args: vec![
                        DiagnosticArg::new("issue", "kernel_requirements_missing"),
                        DiagnosticArg::new("target", target_name),
                    ],
                });
            };
            if !requirement.admitted {
                return Err(codegen::CodegenError {
                    message: format!(
                        "kernel `{}` is not admitted for {target_name}: {:?}",
                        analysis.interner.resolve(name),
                        requirement.failures
                    ),
                    args: vec![
                        DiagnosticArg::new("issue", "kernel_target_requirements_not_met"),
                        DiagnosticArg::new("target", target_name),
                        DiagnosticArg::new("kernel", analysis.interner.resolve(name)),
                    ],
                });
            }
        }
    }
    Ok(())
}

/// D11.1: targets without a clamping lowering for `saturatus<W>` fail closed.
fn reject_saturating_word_target(analysis: &AnalyzedModule, target_name: &str) -> Result<(), codegen::CodegenError> {
    if let Some(width) = analysis.types.first_saturating_word_width() {
        let marker = width.faber_marker();
        return Err(codegen::CodegenError {
            message: format!("saturatus<{marker}> is not supported by the {target_name} target"),
            args: vec![DiagnosticArg::new("issue", "saturating_word_target_unsupported")],
        });
    }
    Ok(())
}

#[cfg(feature = "mir-wasm")]
fn generate_wasm_text_output(
    analysis: &mut AnalyzedModule,
    no_fuse: bool,
) -> Result<crate::Output, SpannedCodegenError> {
    prepare_mir_lowering_interner(analysis);
    let mir = lower_mir_with_context_for_target(analysis, no_fuse)?;
    let code = crate::mir::emit_wasm_text_probe(&mir.validated, &mir.interner).map_err(wasm_text_codegen_error)?;
    Ok(crate::Output::WasmText(crate::WasmTextOutput { code }))
}

/// KPC-EMIT: translate one package-lowering registration row into its
/// target-admission fact. The launch decision comes from the registration's
/// carried v2 contract alone (role, visibility, body channel) — never from
/// symbol spelling, emitted text, or a fallback carrier.
#[cfg(feature = "mir-core")]
fn imported_target_entry_fact(
    registration: &crate::mir::ImportedDeviceRegistration,
) -> radix_mir::device::MirImportedEntryFact {
    use crate::file_interface::{InterfaceDeviceRole, InterfaceDeviceVisibility};
    let contract = &registration.route.contract;
    let role = match contract.role {
        InterfaceDeviceRole::Kernel => crate::mir::MirDeviceRole::Kernel,
        InterfaceDeviceRole::Fragment => crate::mir::MirDeviceRole::Fragment,
    };
    radix_mir::device::MirImportedEntryFact {
        source: registration.key.source,
        function: registration.function,
        role,
        launchable: contract.role == InterfaceDeviceRole::Kernel
            && contract.visibility == InterfaceDeviceVisibility::Public
            && contract.is_composable(),
    }
}

/// KPC-EMIT target-role handoff: merge the concrete imported registrations
/// carried by the lowered unit into the local entry-role map. Only public,
/// composable Kernel registrations admit their template sources as launch
/// entries; private helpers and Fragment roles stay composition-only; an
/// unresolvable registration fails closed before any emitter runs. With no
/// registrations (the single-unit route) the local map passes through
/// unchanged.
#[cfg(feature = "mir-core")]
fn merged_target_entry_roles(
    local: handroll::FxHashMap<crate::hir::DefId, crate::mir::MirDeviceRole>,
    lowered: &crate::mir::LoweredMirUnit<'_>,
) -> Result<handroll::FxHashMap<crate::hir::DefId, crate::mir::MirDeviceRole>, radix_mir::device::MirImportedEntryError>
{
    if lowered.imported_device_registrations.is_empty() {
        return Ok(local);
    }
    let facts = lowered
        .imported_device_registrations
        .iter()
        .map(imported_target_entry_fact)
        .collect::<Vec<_>>();
    radix_mir::device::merge_imported_entry_roles(local, &facts, lowered.validated.program())
}

/// Spanned carrier for a KPC-EMIT merge failure (the `issue` diagnostic arg
/// keeps the machine-readable identity, e.g. `imported_device_instance_unresolved`).
#[cfg(feature = "mir-core")]
fn imported_entry_error_spanned(error: radix_mir::device::MirImportedEntryError) -> SpannedCodegenError {
    SpannedCodegenError { message: error.message(), args: vec![DiagnosticArg::new("issue", error.issue())], span: None }
}

#[cfg(feature = "mir-llvm")]
fn generate_llvm_text_output(
    analysis: &mut AnalyzedModule,
    no_fuse: bool,
    emit_nvvm_device_module: bool,
) -> Result<crate::Output, SpannedCodegenError> {
    prepare_mir_lowering_interner(analysis);
    let local_device_roles = crate::mir::device_entry_roles_from_hir(&analysis.hir, &analysis.interner);
    let mir = lower_mir_with_context_for_target(analysis, no_fuse)?;
    // KPC-EMIT target-role handoff: the merged map (local entries plus
    // concrete imported registrations) is what the text paths consume, so
    // an import-only device package no longer reports the zero-entry red.
    let device_roles = merged_target_entry_roles(local_device_roles, &mir).map_err(imported_entry_error_spanned)?;
    // Source metadata is the package constructor's explicit admission
    // carrier. LLVM's direct device-module route already receives the
    // launch-filtered `device_roles`; pass only source-less closure rows
    // here so a private named helper cannot be promoted back into an LLVM
    // kernel annotation merely because it is a device-composition fact.
    let llvm_metadata = mir
        .device_function_metadata
        .iter()
        .filter(|(_, metadata)| metadata.origin == crate::mir::MirFunctionOrigin::SyntheticKernelClosure)
        .map(|(function, metadata)| (*function, *metadata))
        .collect::<crate::mir::MirDeviceFunctionMetadataMap>();
    // DDCP3-U2 (b) mixed-path disambiguation: default `llvm-text`/`llvm-host`
    // is the CPU-module path. A package that carries device roles routes
    // through the HOST-PARTITION lane (direct declarations/calls, no device
    // bodies). The ordinary `LlvmEmitRequest` with roles is the NVVM
    // device-artifact path: `--cuda-descriptor` selects it so the emitted
    // `.ll` matches the sidecar. No default route silently emits device
    // bodies into a CPU module.
    let code = if emit_nvvm_device_module || device_roles.is_empty() {
        crate::mir::emit_llvm_text_probe(crate::mir::LlvmEmitRequest {
            device_roles: Some(device_roles),
            device_function_metadata: Some(llvm_metadata),
            ..crate::mir::LlvmEmitRequest::new(&mir.validated, &mir.interner)
        })
        .map_err(llvm_text_codegen_error)?
    } else {
        // DDCP3-U5 (gate 6): the host-partition lane runs the pre-emission
        // reject gate BEFORE any output byte — unsupported callsite shapes,
        // unresolved region symbols, effect-crossing regions, missing
        // layout/argument facts, and invalid cross-boundary values fail with
        // stable structured diagnostics naming the rule; a rejected package
        // produces zero output bytes. The legality facts (effect-crossing
        // regions) ride the DDPP1-U6 materializer seam — empty-default here,
        // the gate + record stay live regardless. The fmir→ArtifactPacketFacts
        // adapter (crate::mir::fmir_artifact_packet_facts, U4 residual) is
        // available to that seam at this route.
        crate::mir::emit_llvm_text_probe(crate::mir::LlvmEmitRequest {
            device_roles: Some(device_roles),
            device_function_metadata: Some(llvm_metadata),
            host_partition: true,
            host_partition_legality: crate::mir::device_semantics::DeviceLegalityFacts::default(),
            ..crate::mir::LlvmEmitRequest::new(&mir.validated, &mir.interner)
        })
        .map_err(llvm_text_codegen_error)?
    };
    Ok(crate::Output::LlvmText(crate::LlvmTextOutput { code }))
}

#[cfg(feature = "mir-evm")]
fn generate_evm_output(analysis: &mut AnalyzedModule, no_fuse: bool) -> Result<crate::Output, SpannedCodegenError> {
    let metadata = evm_contract::resolve_metadata(analysis)?;
    prepare_mir_lowering_interner(analysis);
    let mir = lower_mir_with_context_for_target(analysis, no_fuse)?;
    let contract = evm_contract::lower_metadata(metadata, &mir.validated)?;
    let artifact = match contract {
        Some(contract) => radix_mir_evm::emit_evm_contract(&mir.validated, &mir.interner, &contract),
        None => radix_mir_evm::emit_evm(&mir.validated, &mir.interner),
    }
    .map_err(evm_contract::backend_error)?;
    Ok(crate::Output::Evm(crate::EvmOutput {
        init: artifact.init,
        runtime: artifact.runtime,
    }))
}

/// Lower one unit with the Cranelift backend (public `clif` target): a native
/// object for the linker, or an in-memory image, per the configured engine.
#[cfg(feature = "mir-clif")]
fn generate_clif_output(
    analysis: &mut AnalyzedModule,
    no_fuse: bool,
    engine: &crate::ClifEngine,
) -> Result<crate::Output, SpannedCodegenError> {
    prepare_mir_lowering_interner(analysis);
    let mir = lower_mir_with_context_for_target(analysis, no_fuse)?;
    let codegen_error = |error: radix_mir_clif::ClifError| codegen::CodegenError {
        message: error.message,
        args: vec![
            DiagnosticArg::new("category", error.category),
            DiagnosticArg::new("shape", error.shape),
            DiagnosticArg::new("detail", error.detail),
        ],
    };
    match engine {
        crate::ClifEngine::Object => {
            let object = radix_mir_clif::emit_object(&mir.validated, &mir.interner).map_err(codegen_error)?;
            Ok(crate::Output::ClifObject(crate::ClifObjectOutput { bytes: object.bytes }))
        }
        crate::ClifEngine::Jit(runtime) => {
            let program =
                radix_mir_clif::emit_jit(&mir.validated, &mir.interner, &runtime.symbols()).map_err(codegen_error)?;
            Ok(crate::Output::ClifJit(crate::ClifJitOutput { program }))
        }
    }
}

/// Build the NVVM kernel descriptor JSON sidecar for a source unit targeting
/// `llvm-text` (CUDA lane C4-3).
///
/// The sidecar is re-derived from the same source through the public
/// front-half pipeline (`analyze_source` → MIR lowering → device context), so
/// `radix emit --cuda-descriptor <path>` can write it without the compiled
/// [`crate::Output`] carrying MIR. With `--cuda-descriptor`, llvm-text stdout
/// is the NVVM device module, so the descriptor matches the emitted text.
/// Fail closed: a session whose target is not `llvm-text`, a failed
/// analysis/lowering, or a module with no kernel-role functions rejects with
/// diagnostics.
///
/// # Errors
///
/// Returns diagnostics when the session target is not `llvm-text`, when the
/// front half fails to analyze or lower the unit, when no `@ nucleum` kernel
/// is present, or when the descriptor cannot be serialized.
#[cfg(feature = "mir-llvm")]
#[cfg(feature = "mir-core")]
pub fn cuda_kernel_descriptor_json(session: &Session, name: &str, source: &str) -> Result<String, Vec<Diagnostic>> {
    if session.config.target != Target::MirLlvm {
        return Err(vec![codegen_diagnostic(
            name,
            "CUDA_DESCRIPTOR_TARGET",
            "--cuda-descriptor requires the llvm-text target",
        )]);
    }

    let mut analysis = analyze_source(session, name, source, AnalyzeRequest::default())?;

    // Mirror the llvm-text emit path exactly (generate_llvm_text_output) so
    // the descriptor is built from the same MIR snapshot as the emitted code.
    prepare_mir_lowering_interner(&mut analysis);
    let local_device_roles = crate::mir::device_entry_roles_from_hir(&analysis.hir, &analysis.interner);
    let mir = match lower_mir_with_context_for_target(&mut analysis, session.config.no_fuse) {
        Ok(mir) => mir,
        Err(err) => return Err(compile_codegen_error(analysis, name, source, err).diagnostics),
    };
    // KPC-EMIT: same merged target-role handoff as the text paths — the
    // descriptor's entry set is derived from the same merged map.
    let device_roles = merged_target_entry_roles(local_device_roles, &mir).map_err(|error| {
        vec![codegen_diagnostic(name, "CUDA_DESCRIPTOR_BUILD", error.message()).with_arg("issue", error.issue())]
    })?;

    let mut device = crate::mir::MirDeviceContext::from_hir_roles(device_roles);
    device.attach_program(mir.validated.program());
    let descriptor = radix_mir_llvm::nvvm_kernel_descriptor(&mir.validated, &mir.interner, &device)
        .map_err(|error| vec![codegen_diagnostic(name, "CUDA_DESCRIPTOR_BUILD", error.message)])?;
    serde_json::to_string_pretty(&descriptor)
        .map_err(|error| vec![codegen_diagnostic(name, "CUDA_DESCRIPTOR_SERIALIZE", error.to_string())])
}

/// DDCP3-U3 (c) — derive the typed partition/callsite record for an analyzed
/// unit and serialize it as the JSON compiler artifact (the evidence for DDCP3
/// gate bullets 1 + 2: exact membership + callsite mapping + explicit effect
/// boundaries). The record rides the revised FMIR packet **read-only** — no
/// packet schema change here; a missing typed field routes through U4, never a
/// silent schema extension. The legality input is empty at this radix driver
/// entry until the DDPP1-U6 materializer seam wires MIR-body inspection into
/// `DeviceLegalityFacts` (DDCP1 closeout §5.1 residual) — the record's
/// effect-boundary section and the named-rule gate stay live regardless.
///
/// # Errors
///
/// Returns diagnostics when analysis/lowering fails, the record derivation
/// fails the legality gate, or the record cannot be serialized.
#[cfg(feature = "mir-core")]
pub fn partition_record_json(session: &Session, name: &str, source: &str) -> Result<String, Vec<Diagnostic>> {
    let mut analysis = analyze_source(session, name, source, AnalyzeRequest::default())?;
    prepare_mir_lowering_interner(&mut analysis);
    let mir = match lower_mir_with_context_for_target(&mut analysis, session.config.no_fuse) {
        Ok(mir) => mir,
        Err(err) => return Err(compile_codegen_error(analysis, name, source, err).diagnostics),
    };
    let legality = crate::mir::device_semantics::DeviceLegalityFacts::default();
    let record = crate::mir::derive_partition_record_with_metadata(
        &mir.validated,
        &mir.interner,
        name,
        &legality,
        &mir.device_function_metadata,
    )
    .map_err(|error| vec![codegen_diagnostic(name, "PARTITION_RECORD", error.to_string())])?;
    serde_json::to_string_pretty(&record).map_err(|error| {
        vec![codegen_diagnostic(
            name,
            "PARTITION_RECORD_SERIALIZE",
            error.to_string(),
        )]
    })
}

#[cfg(feature = "mir-metal")]
fn generate_metal_text_output(
    analysis: &mut AnalyzedModule,
    no_fuse: bool,
) -> Result<crate::Output, SpannedCodegenError> {
    prepare_mir_lowering_interner(analysis);
    let local_device_roles = crate::mir::device_entry_roles_from_hir(&analysis.hir, &analysis.interner);
    let mir = lower_mir_with_context_for_target(analysis, no_fuse)?;
    // KPC-EMIT target-role handoff: concrete imported registrations join
    // the local entry map before the probe — the spike-3 zero-entry red
    // is the omission this merge removes.
    let device_roles = merged_target_entry_roles(local_device_roles, &mir).map_err(imported_entry_error_spanned)?;
    let output = radix_mir_metal::emit_metal_text_probe_with_reflection_and_function_metadata(
        &device_roles,
        &mir.device_function_metadata,
        &mir.validated,
        &mir.interner,
    )
    .map_err(|error| codegen::CodegenError {
        message: error.message,
        args: vec![
            DiagnosticArg::new("issue", error.issue),
            DiagnosticArg::new("shape", error.shape),
        ],
    })?;
    Ok(crate::Output::MetalText(crate::MetalTextOutput {
        code: output.source,
        reflection: Some(output.reflection),
    }))
}

#[cfg(feature = "mir-wgsl")]
fn generate_wgsl_text_output(
    analysis: &mut AnalyzedModule,
    no_fuse: bool,
    target_bindings: &TargetBindings,
) -> Result<crate::Output, SpannedCodegenError> {
    // SPIKE (shader-annotation-stages): stages declared through
    // manifest-bound annotation contracts take their own path beside the
    // built-in `@ vertex` / `@ fragment` one.
    let stage_bindings =
        target_bindings::wgsl_stage_bindings(target_bindings, &analysis.hir, &analysis.resolver, &analysis.interner)
            .map_err(|message| SpannedCodegenError {
                message,
                args: vec![DiagnosticArg::new("issue", "wgsl_annotation_binding_invalid")],
                span: None,
            })?;
    if !stage_bindings.stages.is_empty() {
        return emit_wgsl_annotated_stage_output(analysis, &stage_bindings);
    }
    let graphics_source = analysis.graphics_source.clone();
    let has_vertex = !graphics_source.vertex_entries.is_empty();
    let has_fragment = !graphics_source.fragment_entries.is_empty();

    if has_vertex || has_fragment {
        return emit_wgsl_graphics_output(analysis, graphics_source, has_vertex, has_fragment, target_bindings);
    }

    prepare_mir_lowering_interner(analysis);
    let local_device_roles = crate::mir::device_entry_roles_from_hir(&analysis.hir, &analysis.interner);
    let mir = lower_mir_with_context_for_target(analysis, no_fuse)?;
    // KPC-EMIT target-role handoff: same merged entry map as Metal/LLVM.
    let device_roles = merged_target_entry_roles(local_device_roles, &mir).map_err(imported_entry_error_spanned)?;
    let output = radix_mir_wgsl::emit_wgsl_text_probe_with_reflection_and_function_metadata(
        &device_roles,
        &mir.device_function_metadata,
        &mir.validated,
        &mir.interner,
    )
    .map_err(|error| codegen::CodegenError {
        message: error.message,
        args: vec![
            DiagnosticArg::new("issue", error.issue),
            DiagnosticArg::new("shape", error.shape),
        ],
    })?;
    Ok(crate::Output::WgslText(crate::WgslTextOutput {
        code: output.source,
        reflection: Some(output.reflection),
    }))
}

/// SPIKE (shader-annotation-stages): lower with the manifest-resolved stage
/// map, then emit the annotation-bound stages straight from the lowered
/// functions (typed parameters in, return value out, no injected names).
#[cfg(feature = "mir-wgsl")]
fn emit_wgsl_annotated_stage_output(
    analysis: &mut AnalyzedModule,
    bindings: &target_bindings::WgslStageBindings,
) -> Result<crate::Output, SpannedCodegenError> {
    prepare_mir_lowering_interner(analysis);
    let options = crate::mir::MirLoweringOptions {
        stage_bindings: std::sync::Arc::new(
            bindings
                .stages
                .iter()
                .map(|(def, stage)| (*def, *stage))
                .collect(),
        ),
        ..crate::mir::MirLoweringOptions::default()
    };
    let mir = crate::mir::lower_analyzed_unit(analysis.application_view(), options).map_err(mir_errors_to_spanned)?;
    let stages: Vec<crate::mir::WgslAnnotatedStage<'_>> = mir
        .validated
        .program()
        .functions
        .iter()
        .filter_map(|function| {
            let stage = *bindings.stages.get(&function.source?)?;
            (function.shader_stage == Some(stage)).then_some(crate::mir::WgslAnnotatedStage { function, stage })
        })
        .collect();
    if stages.len() != bindings.stages.len() {
        return Err(wgsl_text_codegen_error(wgsl_vertex_source_error(
            "a manifest-bound stage function did not reach MIR with its stage",
            "mir_wgsl_annotated_stage_missing",
        ))
        .into());
    }
    let builtins: crate::mir::WgslFieldBuiltins = bindings
        .builtin_fields
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    let source = crate::mir::emit_wgsl_annotated_stages(&stages, &builtins, &mir.validated, &mir.interner)
        .map_err(wgsl_text_codegen_error)?;
    Ok(crate::Output::WgslText(crate::WgslTextOutput {
        code: source,
        reflection: None,
    }))
}

#[cfg(feature = "mir-wgsl")]
fn emit_wgsl_graphics_output(
    analysis: &mut AnalyzedModule,
    graphics_source: GraphicsSourceFacts,
    has_vertex: bool,
    has_fragment: bool,
    target_bindings: &TargetBindings,
) -> Result<crate::Output, SpannedCodegenError> {
    // Run MIR lowering for the graphics path so lowered MirFunctions
    // are available for body emission. If the source has a non-empty
    // vertex or fragment body, a lowering failure is a structured
    // diagnostic — never a silent fallback to reflection defaults.
    // Truly empty bodies may still select reflection-generated bodies.
    prepare_mir_lowering_interner(analysis);
    let has_non_empty_vertex = graphics_source
        .vertex_entries
        .iter()
        .any(|e| e.body_shape == GraphicsVertexBodyShape::Unsupported);
    let has_non_empty_fragment = graphics_source
        .fragment_entries
        .iter()
        .any(|e| e.body_shape == GraphicsVertexBodyShape::Unsupported);
    let bound_types =
        target_bindings::wgsl_bound_type_table(target_bindings, &analysis.hir, &analysis.resolver, &analysis.interner);
    let mir_result = lower_graphics_mir_with_context_for_target(analysis);
    let bound_calls = mir_result.as_ref().map_or_else(
        |_| crate::mir::WgslBoundCallTable::new(),
        |mir| target_bindings::wgsl_bound_call_table(target_bindings, mir.validated.program(), &mir.interner),
    );
    let (vertex_mir, fragment_mir, mir_validation, mir_interner) = match &mir_result {
        Ok(mir) => {
            let v = mir
                .validated
                .program()
                .functions
                .iter()
                .find(|f| f.shader_stage == Some(crate::mir::MirKernelShaderStage::Vertex));
            let f = mir
                .validated
                .program()
                .functions
                .iter()
                .find(|f| f.shader_stage == Some(crate::mir::MirKernelShaderStage::Fragment));
            (v, f, Some(&mir.validated), Some(&mir.interner))
        }
        Err(error) if has_non_empty_vertex || has_non_empty_fragment => {
            return Err(SpannedCodegenError {
                message: error.message.clone(),
                args: error.args.clone(),
                span: error.span,
            });
        }
        Err(_) => (None, None, None, None),
    };

    if has_non_empty_vertex && vertex_mir.is_none() {
        return Err(mir_errors_to_spanned(vec![crate::mir::MirError {
            message: "MIR lowering produced no vertex function for a non-empty graphics shader body".to_owned(),
            issue: "mir_lower_graphics_body_missing".to_owned(),
            span: crate::lexer::Span::default(),
            definition_span: None,
        }]));
    }
    if has_non_empty_fragment && fragment_mir.is_none() {
        return Err(mir_errors_to_spanned(vec![crate::mir::MirError {
            message: "MIR lowering produced no fragment function for a non-empty graphics shader body".to_owned(),
            issue: "mir_lower_graphics_body_missing".to_owned(),
            span: crate::lexer::Span::default(),
            definition_span: None,
        }]));
    }

    if has_vertex && has_fragment {
        let output = emit_wgsl_graphics_pipeline_output(
            &graphics_source,
            vertex_mir,
            fragment_mir,
            mir_validation,
            mir_interner,
            &bound_calls,
            &bound_types,
        )
        .map_err(wgsl_text_codegen_error)?;
        Ok(crate::Output::WgslText(crate::WgslTextOutput {
            code: output.source,
            reflection: Some(output.reflection),
        }))
    } else if has_vertex {
        let output = emit_wgsl_vertex_source_output(
            &graphics_source,
            vertex_mir,
            mir_validation,
            mir_interner,
            &bound_calls,
            &bound_types,
        )
        .map_err(wgsl_text_codegen_error)?;
        Ok(crate::Output::WgslText(crate::WgslTextOutput {
            code: output.source,
            reflection: Some(output.reflection),
        }))
    } else {
        let output = emit_wgsl_fragment_source_output(
            &graphics_source,
            fragment_mir,
            mir_validation,
            mir_interner,
            &bound_calls,
            &bound_types,
        )
        .map_err(wgsl_text_codegen_error)?;
        Ok(crate::Output::WgslText(crate::WgslTextOutput {
            code: output.source,
            reflection: Some(output.reflection),
        }))
    }
}

#[cfg(feature = "mir-wgsl")]
fn emit_wgsl_vertex_source_output(
    graphics_source: &GraphicsSourceFacts,
    vertex_mir: Option<&crate::mir::MirFunction>,
    mir_validation: Option<&crate::mir::ValidatedMir<'_>>,
    mir_interner: Option<&crate::lexer::Interner>,
    bound_calls: &crate::mir::WgslBoundCallTable,
    bound_types: &target_bindings::BoundTypeTable,
) -> Result<crate::mir::MirWgslTextProbeOutput, crate::mir::MirWgslTextProbeError> {
    if graphics_source.vertex_entries.len() != 1 {
        return Err(wgsl_vertex_source_error(
            "wgsl-text source vertex path requires exactly one @ vertex function",
            "mir_wgsl_vertex_source_entry_count",
        ));
    }
    if graphics_source.shader_vertex_attributes.is_empty() {
        return Err(wgsl_vertex_source_error(
            "wgsl vertex entry requires source vertex layout facts",
            "mir_wgsl_vertex_source_layout_missing",
        ));
    }

    let entry = &graphics_source.vertex_entries[0];
    validate_wgsl_vertex_source_entry(entry, bound_types)?;
    let (bound_params, bound_resources) = wgsl_bound_stage_params(graphics_source, &entry.params, bound_types)?;
    let varyings: Vec<crate::mir::MirVaryingReflection> = graphics_source
        .shader_varyings
        .iter()
        .map(radix_mir::varying_reflection_from_shader_contract)
        .collect();
    let resources: Vec<crate::mir::MirKernelResourceReflection> = graphics_source
        .shader_resource_bindings
        .iter()
        .map(radix_mir::resource_reflection_from_shader_contract)
        .collect();
    let vertex = radix_mir::source_vertex_entry_from_shader_contract(
        entry.def_id,
        entry.entry_name.clone(),
        &graphics_source.shader_vertex_attributes,
        resources,
    )
    .map_err(wgsl_vertex_abi_error)?;
    let vertex_with_varyings = crate::mir::MirKernelReflection { varyings, bound_resources, ..vertex };
    let source = crate::mir::emit_wgsl_vertex_entry_contract(
        &vertex_with_varyings,
        &crate::mir::WgslEntryContext {
            bound_calls: bound_calls_with_params(bound_calls, bound_params),
            ..crate::mir::WgslEntryContext::with_body(vertex_mir, mir_validation, mir_interner)
        },
    )?;
    Ok(crate::mir::MirWgslTextProbeOutput {
        source,
        reflection: crate::mir::MirGpuReflection { kernels: vec![vertex_with_varyings], pipeline: None },
    })
}

#[cfg(feature = "mir-wgsl")]
fn emit_wgsl_fragment_source_output(
    graphics_source: &GraphicsSourceFacts,
    fragment_mir: Option<&crate::mir::MirFunction>,
    mir_validation: Option<&crate::mir::ValidatedMir<'_>>,
    mir_interner: Option<&crate::lexer::Interner>,
    bound_calls: &crate::mir::WgslBoundCallTable,
    bound_types: &target_bindings::BoundTypeTable,
) -> Result<crate::mir::MirWgslTextProbeOutput, crate::mir::MirWgslTextProbeError> {
    if graphics_source.fragment_entries.len() != 1 {
        return Err(wgsl_vertex_source_error(
            "wgsl-text source fragment path requires exactly one @ fragment function",
            "mir_wgsl_fragment_source_entry_count",
        ));
    }

    let entry = &graphics_source.fragment_entries[0];
    validate_wgsl_fragment_source_entry(entry, bound_types)?;
    let (bound_params, bound_resources) = wgsl_bound_stage_params(graphics_source, &entry.params, bound_types)?;
    let fragment_outputs: Vec<crate::mir::MirFragmentOutputReflection> = graphics_source
        .shader_fragment_outputs
        .iter()
        .map(radix_mir::fragment_output_reflection_from_shader_contract)
        .collect();
    if fragment_outputs.is_empty() {
        return Err(wgsl_vertex_source_error(
            "wgsl fragment entry requires at least one fragment output fact",
            "mir_wgsl_fragment_output_missing",
        ));
    }
    let resources: Vec<crate::mir::MirKernelResourceReflection> = graphics_source
        .shader_resource_bindings
        .iter()
        .map(radix_mir::resource_reflection_from_shader_contract)
        .collect();
    let mut fragment = radix_mir::source_fragment_entry_from_shader_contract(
        entry.def_id,
        entry.entry_name.clone(),
        &graphics_source.shader_varyings,
        &graphics_source.shader_fragment_outputs,
        resources,
    )
    .map_err(wgsl_vertex_abi_error)?;
    fragment.bound_resources = bound_resources;
    let source = crate::mir::emit_wgsl_fragment_entry_contract(
        &fragment,
        &crate::mir::WgslEntryContext {
            bound_calls: bound_calls_with_params(bound_calls, bound_params),
            ..crate::mir::WgslEntryContext::with_body(fragment_mir, mir_validation, mir_interner)
        },
    )?;
    Ok(crate::mir::MirWgslTextProbeOutput {
        source,
        reflection: crate::mir::MirGpuReflection { kernels: vec![fragment], pipeline: None },
    })
}

#[cfg(feature = "mir-wgsl")]
fn emit_wgsl_graphics_pipeline_output(
    graphics_source: &GraphicsSourceFacts,
    vertex_mir: Option<&crate::mir::MirFunction>,
    fragment_mir: Option<&crate::mir::MirFunction>,
    mir_validation: Option<&crate::mir::ValidatedMir<'_>>,
    mir_interner: Option<&crate::lexer::Interner>,
    bound_calls: &crate::mir::WgslBoundCallTable,
    bound_types: &target_bindings::BoundTypeTable,
) -> Result<crate::mir::MirWgslTextProbeOutput, crate::mir::MirWgslTextProbeError> {
    let vertex_output = emit_wgsl_vertex_source_output(
        graphics_source,
        vertex_mir,
        mir_validation,
        mir_interner,
        bound_calls,
        bound_types,
    )?;
    let fragment_output = emit_wgsl_fragment_source_output(
        graphics_source,
        fragment_mir,
        mir_validation,
        mir_interner,
        bound_calls,
        bound_types,
    )?;

    let pipeline = graphics_source
        .shader_pipeline_states
        .first()
        .map(|state| {
            radix_mir::graphics_pipeline_from_shader_contract(
                &vertex_output.reflection.kernels[0],
                &fragment_output.reflection.kernels[0],
                state,
            )
        })
        .transpose()
        .map_err(|err| wgsl_vertex_source_error_with_detail("wgsl pipeline fact invalid", err.issue, &err.message))?;

    let mut kernels = vertex_output.reflection.kernels;
    kernels.extend(fragment_output.reflection.kernels);

    Ok(crate::mir::MirWgslTextProbeOutput {
        source: format!("{}\n{}\n", vertex_output.source, fragment_output.source),
        reflection: crate::mir::MirGpuReflection { kernels, pipeline },
    })
}

#[cfg(feature = "mir-wgsl")]
fn validate_wgsl_vertex_source_entry(
    entry: &GraphicsVertexEntry,
    bound_types: &target_bindings::BoundTypeTable,
) -> Result<(), crate::mir::MirWgslTextProbeError> {
    if entry.annotation_payload_count != 0 {
        return Err(wgsl_vertex_source_error(
            "wgsl vertex entry annotation must not carry payload fields or arguments",
            "mir_wgsl_vertex_source_annotation_payload",
        ));
    }
    if !stage_params_all_bound(entry.param_count, &entry.params, bound_types) {
        return Err(wgsl_vertex_source_error(
            "wgsl vertex entry requires zero source parameters or only bound-type parameters",
            "mir_wgsl_vertex_source_params",
        ));
    }
    if !entry.returns_vacuum {
        return Err(wgsl_vertex_source_error(
            "wgsl vertex entry requires vacuum return type",
            "mir_wgsl_vertex_source_return",
        ));
    }
    if entry.has_error_type {
        return Err(wgsl_vertex_source_error(
            "wgsl vertex entry must not declare an error type",
            "mir_wgsl_vertex_source_error_type",
        ));
    }
    // Non-empty vertex bodies are now supported via MIR lowering (U4).
    Ok(())
}

/// True when every source parameter of a stage entry has a bound type.
///
/// `param_count` also counts a CLI-args parameter, which carries no entry in
/// `params`; a count that exceeds `params` is therefore never bound.
#[cfg(feature = "mir-wgsl")]
fn stage_params_all_bound(
    param_count: usize,
    params: &[GraphicsStageParam],
    bound_types: &target_bindings::BoundTypeTable,
) -> bool {
    param_count == params.len()
        && params.iter().all(|param| {
            param
                .type_def
                .is_some_and(|def_id| bound_types.contains_key(&def_id))
        })
}

/// Match each bound stage parameter to its `bound_resource_binding` fact by
/// name. Returns the carrier facts (spelling and slot) and the reflection
/// records, both in parameter order. A parameter with no matching contract
/// call fails closed.
#[cfg(feature = "mir-wgsl")]
fn wgsl_bound_stage_params(
    graphics_source: &GraphicsSourceFacts,
    params: &[GraphicsStageParam],
    bound_types: &target_bindings::BoundTypeTable,
) -> Result<
    (Vec<radix_mir_wgsl::WgslBoundParam>, Vec<radix_mir::MirBoundResourceReflection>),
    crate::mir::MirWgslTextProbeError,
> {
    let mut facts = Vec::with_capacity(params.len());
    let mut reflections = Vec::with_capacity(params.len());
    for param in params {
        let bound_type = param.type_def.and_then(|def_id| bound_types.get(&def_id));
        let bound_type = bound_type.ok_or_else(|| {
            wgsl_vertex_source_error(
                "wgsl stage entry parameter has no bound type",
                "mir_wgsl_bound_param_unbound_type",
            )
        })?;
        let contract = graphics_source
            .bound_resource_bindings
            .iter()
            .find(|binding| binding.source_name == param.name)
            .ok_or_else(|| {
                wgsl_vertex_source_error_with_detail(
                    "wgsl bound stage parameter has no matching bound_resource_binding call",
                    "mir_wgsl_bound_param_unmatched",
                    &param.name,
                )
            })?;
        facts.push(radix_mir_wgsl::WgslBoundParam {
            name: param.name.clone(),
            group: contract.group,
            binding: contract.binding,
            spelling: bound_type.spelling.clone(),
            bound_type: bound_type.type_name.clone(),
        });
        reflections.push(radix_mir::bound_resource_reflection_from_shader_contract(
            contract,
            bound_type.type_name.clone(),
        ));
    }
    Ok((facts, reflections))
}

#[cfg(feature = "mir-wgsl")]
fn bound_calls_with_params(
    bound_calls: &crate::mir::WgslBoundCallTable,
    params: Vec<radix_mir_wgsl::WgslBoundParam>,
) -> crate::mir::WgslBoundCallTable {
    let mut table = bound_calls.clone();
    for param in params {
        table.push_param(param);
    }
    table
}

#[cfg(feature = "mir-wgsl")]
fn wgsl_vertex_source_error(shape: &'static str, issue: &'static str) -> crate::mir::MirWgslTextProbeError {
    crate::mir::MirWgslTextProbeError {
        message: format!("MIR-to-WGSL unsupported: {shape}"),
        issue,
        shape: shape.to_owned(),
        callee: None,
    }
}

#[cfg(feature = "mir-wgsl")]
fn wgsl_vertex_abi_error(error: crate::mir::MirAbiError) -> crate::mir::MirWgslTextProbeError {
    crate::mir::MirWgslTextProbeError {
        message: format!("MIR-to-WGSL unsupported: {}", error.message),
        issue: error.issue,
        shape: error.message,
        callee: None,
    }
}

#[cfg(feature = "mir-wgsl")]
fn wgsl_text_codegen_error(error: crate::mir::MirWgslTextProbeError) -> codegen::CodegenError {
    codegen::CodegenError {
        message: error.message,
        args: vec![
            DiagnosticArg::new("issue", error.issue),
            DiagnosticArg::new("shape", error.shape),
        ],
    }
}

#[cfg(feature = "mir-wgsl")]
fn wgsl_vertex_source_error_with_detail(
    shape: &'static str,
    issue: &'static str,
    detail: &str,
) -> crate::mir::MirWgslTextProbeError {
    crate::mir::MirWgslTextProbeError {
        message: format!("MIR-to-WGSL unsupported: {shape}: {detail}"),
        issue,
        shape: format!("{shape}: {detail}"),
        callee: None,
    }
}

#[cfg(feature = "mir-wgsl")]
fn validate_wgsl_fragment_source_entry(
    entry: &GraphicsFragmentEntry,
    bound_types: &target_bindings::BoundTypeTable,
) -> Result<(), crate::mir::MirWgslTextProbeError> {
    if entry.annotation_payload_count != 0 {
        return Err(wgsl_vertex_source_error(
            "wgsl fragment entry annotation must not carry payload fields or arguments",
            "mir_wgsl_fragment_source_annotation_payload",
        ));
    }
    if !stage_params_all_bound(entry.param_count, &entry.params, bound_types) {
        return Err(wgsl_vertex_source_error(
            "wgsl fragment entry requires zero source parameters or only bound-type parameters",
            "mir_wgsl_fragment_source_params",
        ));
    }
    if !entry.returns_vacuum {
        return Err(wgsl_vertex_source_error(
            "wgsl fragment entry requires vacuum return type",
            "mir_wgsl_fragment_source_return",
        ));
    }
    if entry.has_error_type {
        return Err(wgsl_vertex_source_error(
            "wgsl fragment entry must not declare an error type",
            "mir_wgsl_fragment_source_error_type",
        ));
    }
    // Non-empty fragment bodies are no longer rejected (mir_wgsl_fragment_source_body gate removed U4).
    Ok(())
}

#[cfg(feature = "mir-sexp")]
fn generate_sexp_output(analysis: &mut AnalyzedModule, no_fuse: bool) -> Result<crate::Output, SpannedCodegenError> {
    prepare_mir_lowering_interner(analysis);
    let mir = lower_mir_with_context_for_target(analysis, no_fuse)?;
    let code = crate::mir::emit_sexp_text(&mir.validated, &mir.interner).map_err(|error| codegen::CodegenError {
        message: error.message,
        args: vec![
            DiagnosticArg::new("category", error.category),
            DiagnosticArg::new("shape", error.shape),
        ],
    })?;
    Ok(crate::Output::Sexp(crate::SexpOutput { code }))
}

#[cfg(feature = "mir-basic")]
fn generate_basic_output(analysis: &mut AnalyzedModule, no_fuse: bool) -> Result<crate::Output, SpannedCodegenError> {
    prepare_mir_lowering_interner(analysis);
    let mir = lower_mir_with_context_for_target(analysis, no_fuse)?;
    let code = crate::mir::emit_basic_text(&mir.validated, &mir.interner).map_err(|error| codegen::CodegenError {
        message: error.message,
        args: vec![
            DiagnosticArg::new("category", error.category),
            DiagnosticArg::new("shape", error.shape),
        ],
    })?;
    Ok(crate::Output::Basic(crate::BasicOutput { code }))
}

/// Keep the cloned interner used by target emitters in lockstep with MIR-only
/// symbols that lowering may intern for synthesized format templates.
#[cfg(feature = "mir-core")]
fn prepare_mir_lowering_interner(analysis: &mut AnalyzedModule) {
    crate::mir::prepare_mir_lowering_interner(&mut analysis.interner);
}

/// Pre-lowered AIR MIR program and backward-generation facts, bundled for
/// installation after HIR→MIR lowering completes. Preparation runs before
/// lowering because AIR-side pre-interning (gradient tuples, transposed and
/// broadcast-reduction tensor types) must land in the analyzed unit's table
/// before lowering clones it into the lowering-owned extension arena;
/// installation runs after lowering, re-deriving each backward-annotated
/// primal's metadata from the analyzed unit — lowering holds the unit
/// immutably now, so there is no borrow conflict to snapshot around.
#[cfg(feature = "device")]
pub struct AirBackwardBundle {
    pub air_mir: crate::mir::MirProgram,
    pub air_program: crate::air::AirProgram,
    /// Global `(operand_ty → transposed_ty)` map for `MatMul` VJP operand
    /// types, merged from every AIR function (annotated primals + unannotated
    /// callees). The reverse-AD transform resolves the transposed type in
    /// O(1) via this map instead of scanning the whole `TypeTable`
    /// (D-BO-M-4-1).
    pub matmul_transpose_types: handroll::FxHashMap<crate::semantic::TypeId, crate::semantic::TypeId>,
}

/// Backward-annotated primal metadata derived live from the analyzed unit at
/// the phase that needs it (pre-interning in [`prepare_air_backward_bundle`],
/// companion installation in [`apply_air_backward_bundle`]).
#[cfg(feature = "device")]
struct AirBackwardPrimal {
    primal_def_id: crate::hir::DefId,
    companion_def_id: crate::hir::DefId,
    /// Types of selected differentiable inputs, in parameter declaration order.
    selected_input_types: Vec<crate::semantic::TypeId>,
    /// `DefIds` of selected differentiable input parameters, in declaration order.
    selected_param_sources: Vec<crate::hir::DefId>,
    ret_ty: crate::semantic::TypeId,
    /// Annotated companion name from @ radix backward "name".
    companion_name: crate::lexer::Symbol,
    /// The primal is an explicitly device-resident forward kernel (S3-A2,
    /// placement fact A5): recorded from the primal's HIR device role so the
    /// companion-aware materializer can join the companion to the same
    /// `DeviceProgram`.
    device_resident: bool,
}

/// Derive the backward-annotated primal metadata from the analyzed unit and
/// the already-lowered AIR program. Returns an empty `Vec` when no
/// backward-annotated AIR function meets the same-shape elementwise shape
/// requirements.
#[cfg(feature = "device")]
fn backward_primal_metadata(analysis: &AnalyzedModule, air_program: &crate::air::AirProgram) -> Vec<AirBackwardPrimal> {
    use crate::hir::HirItemKind;

    let mut primals = Vec::new();

    // Build a lookup from DefId to AirFunction from the already-lowered AIR.
    let air_functions: handroll::FxHashMap<crate::hir::DefId, &crate::air::AirFunction> = air_program
        .functions
        .iter()
        .map(|f| (f.source, f))
        .collect();

    for (primal_def_id, backward) in analysis.radix_lanes.iter_backward() {
        // The primal must have an AIR-lowered function (generic declarations
        // that never specialized have none).
        if !air_functions.contains_key(&primal_def_id) {
            continue;
        }

        // Get the HIR function for this backward-annotated def.
        let hir_function = analysis
            .hir
            .items
            .iter()
            .find(|item| item.def_id == primal_def_id)
            .and_then(|item| match &item.kind {
                HirItemKind::Function(f) => Some(f),
                _ => None,
            });

        let Some(hir_function) = hir_function else {
            continue;
        };

        // Same-shape elementwise requires a return type, at least 1 tensor param.
        let Some(ret_ty) = hir_function.ret_ty else {
            continue;
        };
        if hir_function.params.is_empty() {
            continue;
        }

        // Collect differentiable tensor params dynamically.
        let mut selected_input_types = Vec::new();
        let mut selected_param_sources = Vec::new();
        for param in &hir_function.params {
            if crate::semantic::tensor_type_directed::is_tensor_type(param.ty, &analysis.types) {
                selected_input_types.push(param.ty);
                selected_param_sources.push(param.def_id);
            }
        }
        if selected_input_types.is_empty() {
            continue;
        }

        primals.push(AirBackwardPrimal {
            primal_def_id,
            companion_def_id: backward.companion_def_id,
            selected_input_types,
            selected_param_sources,
            ret_ty,
            companion_name: backward.companion_name,
            device_resident: hir_function.is_kernel,
        });
    }

    primals
}

/// Prepare AIR backward companion data *before* HIR→MIR lowering.
///
/// Returns `None` when the unit has no AIR lane or no backward annotations.
/// Callers that skip this phase (or that receive `None`) have no companions
/// to install later; the lowered MIR program is complete as-is.
///
/// # Errors
///
/// Returns a [`SpannedCodegenError`] when AIR lowering or backward companion
/// preparation fails.
#[cfg(feature = "device")]
pub fn prepare_air_backward_bundle(
    analysis: &mut AnalyzedModule,
) -> Result<Option<AirBackwardBundle>, SpannedCodegenError> {
    let has_air = analysis
        .radix_lanes
        .iter()
        .any(|(_, lane)| lane == RadixLane::Air);

    let has_backward = analysis.radix_lanes.has_backward();

    if !has_air || !has_backward {
        return Ok(None);
    }

    let (air_program, air_mir) = air_mir_from_analysis(analysis)?;
    if air_program.functions.is_empty() {
        // Generic AIR declarations are source/FHIR templates.  The semantic
        // specialization pass only admits concrete, reachable instances, so
        // an annotated template with no concrete call site has no executable
        // backward bundle to prepare.
        return Ok(None);
    }

    // Pre-intern gradient-tuple types for every AIR function so that
    // `transform_reverse_ad` can find them via `find_tuple` when it runs
    // inside `generate_companions_bottom_up` (which receives an immutable
    // `&TypeTable` that cannot intern new tuples itself).
    //
    // Without this, `find_tuple` returns None for interprocedural callees
    // whose parameter-type combinations have never been interned, and the
    // sentinel `TypeId(u32::MAX)` stored in `CalleeCompanionInfo::func_sig_ret`
    // later panics in `walk_block_reverse` when `types.get(sentinel)` is
    // called.
    for function in &air_program.functions {
        let tensor_param_tys: Vec<crate::semantic::TypeId> = function
            .params
            .iter()
            .filter(|p| crate::semantic::tensor_type_directed::is_tensor_type(p.ty, &analysis.types))
            .map(|p| p.ty)
            .collect();
        if !tensor_param_tys.is_empty() {
            analysis.types.tuple(tensor_param_tys);
        }
    }

    // Pre-intern transposed tensor types for MatMul operands across ALL
    // functions (annotated primals + unannotated callees) so the reverse-AD
    // transform can resolve them during bottom-up companion generation
    // (D-BO-M-4-1). Covering every function
    // makes MatMul VJP coverage deterministic for unannotated callees that
    // today only succeed when a whole-table scan finds the type by accident.
    // Each function's map merges into one global (operand_ty → transposed_ty)
    // map carried on the bundle; TypeIds are globally interned in the shared
    // TypeTable, so a single keyed map is valid across the whole call DAG.
    let mut matmul_transpose_types: handroll::FxHashMap<crate::semantic::TypeId, crate::semantic::TypeId> =
        handroll::FxHashMap::default();
    for function in &air_program.functions {
        matmul_transpose_types.extend(crate::air::reverse_ad::ensure_matmul_transpose_types(
            function,
            &mut analysis.types,
        ));
        // Pre-intern the intermediate tensor types produced by broadcast
        // cotangent reductions (SumAxis chains and the Fill re-broadcast)
        // for the same reason as the matmul transpose types above: the
        // reverse-AD walk holds only `&TypeTable` and must resolve every
        // intermediate reduced shape already interned. A broadcast-using
        // primal (bias-broadcast body) otherwise hits
        // `reverse_ad_broadcast_reduction_missing_type` during companion
        // generation (shape-generics P3 unit-3B).
        crate::air::reverse_ad::ensure_broadcast_reduction_types(function, &mut analysis.types);
    }

    // Pre-intern axis-reduced tensor shapes for every interned tensor type so
    // reduction-style VJPs (RmsNorm, Laminatio) resolve their intermediate
    // reduced tensors during the immutable walk: `reduced_tensor_type_after_axis`
    // can only find shapes that are already interned, and a rank-2 `rms_norm`
    // primal otherwise fails companion generation with
    // `reverse_ad_rms_norm_missing_reduced_type` even though the VJP is
    // well-formed. Same invariant as the matmul/broadcast pre-interning above;
    // the cursor runs to a fixpoint over the growing table so reduced shapes
    // of freshly interned shapes are covered too. Rank-1 shapes reduce to the
    // scalar element type, which is always interned, so only rank ≥ 2 needs
    // the pass.
    let mut cursor = 0;
    while cursor < analysis.types.type_count() {
        #[expect(clippy::cast_possible_truncation)]
        let ty = radix_types::TypeId(cursor as u32);
        cursor += 1;
        let Type::Tensor(elem, shape) = analysis.types.get(ty) else {
            continue;
        };
        let (elem, shape) = (*elem, *shape);
        let dims = match analysis.types.get_index(shape) {
            radix_types::IndexExpr::Tuple(dims) if dims.len() >= 2 => dims.clone(),
            _ => continue,
        };
        for axis in 0..dims.len() {
            let mut reduced = dims.clone();
            reduced.remove(axis);
            let reduced_shape = analysis
                .types
                .intern_index(radix_types::IndexExpr::Tuple(reduced));
            analysis.types.tensor_with_shape(elem, reduced_shape);
        }
    }

    // Pre-intern types needed by the reverse AD transform: the unit type
    // (empty tuple) for the residual bundle parameter, then the gradient
    // tuple for each backward-annotated primal. `transform_reverse_ad` runs
    // against `&TypeTable` views that cannot intern, so every shape it may
    // look up must already be in the analyzed table here — lowering then
    // clones this table into its lowering-owned extension arena, keeping the
    // interned shapes visible along the whole lowering path.
    analysis.types.unit();
    for primal in backward_primal_metadata(analysis, &air_program) {
        analysis.types.tuple(primal.selected_input_types.clone());
    }

    Ok(Some(AirBackwardBundle { air_mir, air_program, matmul_transpose_types }))
}

/// Install AIR-lane MIR replacements and backward companion functions into
/// an already-lowered [`LoweredMirUnit`].
///
/// Call this *after* [`lower_analyzed_unit`] (or the CLI-aware
/// variant) has produced the HIR→MIR program and validation context. The
/// analyzed unit is read immutably here — lowering no longer borrows it
/// mutably, so each backward-annotated primal's metadata is derived live
/// from `analysis` instead of being snapshotted ahead of lowering.
///
/// # Errors
///
/// Returns a [`SpannedCodegenError`] when AIR replacement or companion
/// generation fails.
#[cfg(feature = "device")]
pub fn apply_air_backward_bundle<'a>(
    lowered: crate::mir::LoweredMirUnit<'a>,
    bundle: AirBackwardBundle,
    analysis: &AnalyzedModule,
    no_fuse: bool,
) -> Result<crate::mir::LoweredMirUnit<'a>, SpannedCodegenError> {
    use crate::air::reverse_ad_callgraph::generate_companions_bottom_up;
    use handroll::FxHashMap;

    let mut lowered = replace_air_lane_mir(lowered, bundle.air_mir)?;
    let mut fresh_ids = FreshMirFunctionIds::after(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .map(|function| function.id),
    );

    // Build maps for bottom-up companion generation from the derived primals.
    let primals = backward_primal_metadata(analysis, &bundle.air_program);
    let backward_def_ids: Vec<crate::hir::DefId> = primals.iter().map(|s| s.primal_def_id).collect();

    let companion_names: FxHashMap<crate::hir::DefId, crate::lexer::Symbol> = primals
        .iter()
        .map(|s| (s.primal_def_id, s.companion_name))
        .collect();

    let predeclared_companions: FxHashMap<crate::hir::DefId, crate::hir::DefId> = primals
        .iter()
        .map(|s| (s.primal_def_id, s.companion_def_id))
        .collect();

    // Run bottom-up companion generation.  This produces companions for all
    // functions in the call DAG (annotated primals + unannotated callees) and
    // a populated callee_info map so Call bindings are resolved.
    let mut names = crate::air::AirNameAllocator::new();
    let generated = generate_companions_bottom_up(
        &bundle.air_program,
        &backward_def_ids,
        &companion_names,
        &predeclared_companions,
        &mut names,
        lowered.validated.validation().types.as_table(),
        &bundle.matmul_transpose_types,
        &mut lowered.interner,
    )
    .map_err(|errors| {
        let messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
        SpannedCodegenError {
            message: format!("call-graph companion generation failed: {}", messages.join("; ")),
            args: vec![DiagnosticArg::new("issue", "air_companion_generation_failed")],
            span: None,
        }
    })?;

    // GT01/E07: companion generation interns generated names (`__backward_callee_N`)
    // into `lowered.interner` after `build_lowering_validation` cloned it into
    // the validated context. Re-prove the token against the refreshed table so
    // patch re-validation, FMIR image snapshots and runner name resolution
    // resolve the generated symbols instead of indexing past the stale clone.
    let (program, mut context) = lowered.validated.into_parts();
    context.interner = crate::mir::MirInterner::Owned(std::sync::Arc::new(lowered.interner.clone()));
    lowered.validated = crate::mir::ValidatedMir::new(program, context).map_err(|errors| {
        let messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
        SpannedCodegenError {
            message: format!(
                "MIR validation after backward companion addition failed: {}",
                messages.join("; ")
            ),
            args: vec![DiagnosticArg::new(
                "issue",
                "mir_validation_after_companion_addition_failed",
            )],
            span: None,
        }
    })?;

    // Index annotated companions by primal DefId for primal matching.
    let annotated_by_primal: FxHashMap<crate::hir::DefId, &crate::air::AirFunction> = {
        // Companions use their own DefId as `source` (not the primal's). Build an
        // O(1) source→companion map once (BO-D-6), then resolve each primal.
        let companions_by_source: FxHashMap<crate::hir::DefId, &crate::air::AirFunction> = generated
            .annotated_companions
            .iter()
            .map(|companion| (companion.source, companion))
            .collect();
        let mut map = FxHashMap::default();
        for primal in &primals {
            let companion_def_id = primal.companion_def_id;
            if let Some(&companion) = companions_by_source.get(&companion_def_id) {
                map.insert(primal.primal_def_id, companion);
            }
        }
        map
    };

    // For each annotated primal, run the validation/installation chain using the
    // generated companion and the populated callee_info map.
    for primal in &primals {
        let Some(companion) = annotated_by_primal.get(&primal.primal_def_id) else {
            return Err(SpannedCodegenError {
                message: format!("companion not found for annotated primal DefId({})", primal.primal_def_id.0),
                args: vec![DiagnosticArg::new("issue", "air_companion_missing_for_primal")],
                span: None,
            });
        };
        let companion = (*companion).clone();
        // S3-A2: the returned entry carries the primal→companion facts onto
        // the lowered unit (the owned carrier), so the device-program
        // constructor can materialize the companion as device kernels.
        let (next, entry) =
            generate_air_lane_backward_with_companion(primal, companion, lowered, &mut fresh_ids, no_fuse)?;
        lowered = next;
        // Fail-closed carrier insertion (Stage 3R F4): duplicate or
        // contradictory relation rows for one primal are a hard error, never
        // a silent overwrite.
        lowered
            .companions
            .try_insert(entry)
            .map_err(|error| SpannedCodegenError {
                message: format!("companion relation carrier rejected: {error}"),
                args: vec![DiagnosticArg::new("issue", "air_companion_carrier_rejected")],
                span: None,
            })?;
    }

    lowered = install_unannotated_companions(lowered, &generated.unannotated_companions, &mut fresh_ids)?;

    finalize_air_backward_shapes(lowered)
}

#[cfg(feature = "device")]
fn install_unannotated_companions<'a>(
    mut lowered: crate::mir::LoweredMirUnit<'a>,
    companions: &[crate::air::AirFunction],
    fresh_ids: &mut FreshMirFunctionIds,
) -> Result<crate::mir::LoweredMirUnit<'a>, SpannedCodegenError> {
    // Emit unannotated callee companions into the MIR program.  These are
    // companions for functions that are called but not themselves annotated
    // with @ radix backward.  They must be lowered to MIR so the backward
    // sub-companion Call targets are available at link time.
    let mut unannotated_functions = Vec::new();
    for companion in companions {
        let companion_program = crate::air::AirProgram { functions: vec![companion.clone()] };
        crate::air::validate_program(&companion_program).map_err(|errors| {
            let messages: Vec<String> = errors.iter().map(|e| format!("{e:?}")).collect();
            SpannedCodegenError {
                message: format!("unannotated callee companion AIR validation failed: {}", messages.join("; ")),
                args: vec![DiagnosticArg::new("issue", "air_companion_validation_failed")],
                span: None,
            }
        })?;
        let companion_mir = radix_air_bridge::to_mir_program(
            &companion_program,
            &radix_air_bridge::AirToMirContext::with_interner_and_types(
                &lowered.interner,
                lowered.validated.validation().types.as_table(),
            ),
        )
        .map_err(|errors| {
            let messages: Vec<String> = errors.iter().map(|e| format!("{e:?}")).collect();
            SpannedCodegenError {
                message: format!("unannotated callee companion MIR lowering failed: {}", messages.join("; ")),
                args: vec![DiagnosticArg::new("issue", "air_companion_mir_lowering_failed")],
                span: None,
            }
        })?;
        for mut function in companion_mir.functions {
            function.id = fresh_ids.next()?;
            unannotated_functions.push(function);
        }
    }
    lowered = lowered
        .patch_program(|program| {
            program.functions.extend(unannotated_functions);
            Ok::<(), std::convert::Infallible>(())
        })
        .map_err(|error| match error {
            radix_mir::validate::PatchProgramError::Patch(never) => match never {},
            radix_mir::validate::PatchProgramError::Validation(errors) => {
                let messages = errors
                    .iter()
                    .map(|error| error.message.clone())
                    .collect::<Vec<_>>();
                SpannedCodegenError {
                    message: format!(
                        "MIR validation after backward companion addition failed: {}",
                        messages.join("; ")
                    ),
                    args: vec![DiagnosticArg::new(
                        "issue",
                        "mir_validation_after_companion_addition_failed",
                    )],
                    span: None,
                }
            }
        })?;
    Ok(lowered)
}

/// Apply concrete shape bindings from existing call-site monomorphizations to
/// AIR-generated backward MIR after the bundle has been installed.
///
/// AIR companions are generated after ordinary HIR→MIR monomorphization, so
/// they cannot participate in that earlier pass. The generated companion and
/// primal carry the same symbolic tensor shapes. Matching each companion
/// against a concrete primal instance therefore recovers the exact
/// `magnitudo` bindings without a backend-specific policy.
#[cfg(feature = "device")]
fn finalize_air_backward_shapes(
    mut lowered: crate::mir::LoweredMirUnit<'_>,
) -> Result<crate::mir::LoweredMirUnit<'_>, SpannedCodegenError> {
    use crate::mir::MirMonomorphizationKey;
    use crate::mir::generic::MirShapeBindings;

    const UNRESOLVED_SHAPE_SIZE_SURVIVOR: &str = "unresolved shape-size constant survives monomorphization";

    let companion_sources = lowered
        .companions
        .iter()
        .map(|entry| entry.companion)
        .collect::<handroll::FxHashSet<_>>();
    let mut companion_templates = lowered
        .validated
        .program()
        .functions
        .iter()
        .filter(|function| {
            function
                .source
                .is_some_and(|source| companion_sources.contains(&source))
        })
        .cloned()
        .collect::<Vec<_>>();
    lowered = carry_air_backward_templates(lowered, &companion_templates);
    if companion_templates.is_empty() {
        reject_shape_size_survivors(&lowered.validated.program().functions, UNRESOLVED_SHAPE_SIZE_SURVIVOR)?;
        return Ok(lowered);
    }

    let concrete_shapes = lowered
        .monomorphizations
        .iter()
        .filter(|monomorphization| !monomorphization.key.shape_args.is_empty())
        .map(|monomorphization| monomorphization.key.shape_args.clone())
        .collect::<handroll::FxHashSet<_>>();
    if concrete_shapes.is_empty() {
        // A symbolic companion with no concrete call-site instance is a
        // template, not executable MIR. It stays available for later package
        // linking and is outside the whole-executable-program survivor net.
        return Ok(lowered);
    }

    // Clone the token's table once: AIR shape finalization interns the
    // concrete tuple/index carriers the generated companion instances carry,
    // and that extension must not write the table the current token still
    // validates against. The clone is handed back to the re-validated token
    // as its owned table below, so its lifetime is bounded by the token —
    // no compilation-lifetime leak.
    let mut types =
        radix_types::TypeTable::from_snapshot(lowered.validated.validation().types.snapshot()).map_err(|message| {
            SpannedCodegenError {
                message: format!("MIR type metadata clone failed: {message}"),
                args: vec![DiagnosticArg::new("issue", "mir_type_metadata_clone_failed")],
                span: None,
            }
        })?;
    let mut next_ids = FreshMirFunctionIds::after(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .map(|function| function.id),
    );
    let mut instances = Vec::new();
    let mut monomorphizations = Vec::new();
    let mut instance_ids = handroll::FxHashMap::default();
    let mut instance_return_types = handroll::FxHashMap::default();

    for template in &mut companion_templates {
        let symbols = companion_shape_symbols(&types, template);
        for shape_args in &concrete_shapes {
            if symbols.len() != shape_args.len() {
                continue;
            }
            let bindings = symbols
                .iter()
                .copied()
                .zip(shape_args.iter().copied())
                .collect::<MirShapeBindings>();
            let Some(mut instance) = finalize_air_backward_instance(&mut types, template, &bindings) else {
                continue;
            };
            instance.id = next_ids.next()?;
            let Some(source) = instance.source else {
                return Err(SpannedCodegenError {
                    message: "AIR companion instance has no source identity".to_owned(),
                    args: vec![DiagnosticArg::new("issue", "air_companion_missing_source")],
                    span: None,
                });
            };
            let key = MirMonomorphizationKey { source, type_args: Vec::new(), shape_args: shape_args.clone() };
            instance_ids.insert(key.clone(), instance.id);
            instance_return_types.insert(key.clone(), instance.return_ty);
            monomorphizations.push(crate::mir::MirMonomorphization { key, function: instance.id });
            instances.push(instance);
        }
    }

    if instances.is_empty() {
        reject_shape_size_survivors(&lowered.validated.program().functions, UNRESOLVED_SHAPE_SIZE_SURVIVOR)?;
        return Ok(lowered);
    }

    let template_ids = companion_templates
        .iter()
        .map(|function| function.id)
        .collect::<handroll::FxHashSet<_>>();
    let (mut program, validation) = lowered.validated.into_parts();
    rewrite_air_backward_calls(
        &mut program.functions,
        &types,
        &companion_sources,
        &instance_ids,
        &instance_return_types,
    )?;
    program
        .functions
        .retain(|function| !template_ids.contains(&function.id));
    program.functions.extend(instances);
    reject_shape_size_survivors(&program.functions, UNRESOLVED_SHAPE_SIZE_SURVIVOR)?;
    let concrete_validation = validation.with_owned_types(types);
    lowered.validated = crate::mir::ValidatedMir::new(program, concrete_validation).map_err(|errors| {
        let messages = errors
            .iter()
            .map(|error| error.message.clone())
            .collect::<Vec<_>>();
        SpannedCodegenError {
            message: format!(
                "MIR validation after backward shape finalization failed: {}",
                messages.join("; ")
            ),
            args: vec![DiagnosticArg::new(
                "issue",
                "mir_validation_after_shape_finalization_failed",
            )],
            span: errors.first().map(|error| error.span),
        }
    })?;
    lowered.monomorphizations.extend(monomorphizations);
    Ok(lowered)
}

#[cfg(feature = "device")]
fn rewrite_air_backward_calls(
    functions: &mut [crate::mir::MirFunction],
    types: &crate::semantic::TypeTable,
    companion_sources: &handroll::FxHashSet<crate::hir::DefId>,
    instances: &handroll::FxHashMap<crate::mir::MirMonomorphizationKey, crate::mir::MirFunctionId>,
    instance_return_types: &handroll::FxHashMap<crate::mir::MirMonomorphizationKey, crate::mir::MirType>,
) -> Result<(), SpannedCodegenError> {
    use crate::mir::generic::{MirShapeBindings, bind_generic_type_witness_with_shapes, canonical_index};
    use crate::mir::{
        MirCallee, MirGenericBindings, MirMonomorphizationKey, MirOperand, MirPlaceBase, MirStatementKind,
        MirTerminatorKind,
    };

    fn operand_ty(
        locals: &[crate::mir::MirLocal],
        temps: &[crate::mir::MirTemp],
        values: &handroll::FxHashMap<crate::mir::MirValueId, crate::mir::MirType>,
        operand: &MirOperand,
    ) -> Option<crate::mir::MirType> {
        match operand {
            MirOperand::Place(place) => match place.base {
                MirPlaceBase::Local(id) => locals
                    .iter()
                    .find(|local| local.id == id)
                    .map(|local| local.ty),
                MirPlaceBase::Temp(id) => temps.iter().find(|temp| temp.id == id).map(|temp| temp.ty),
            },
            MirOperand::Temp(id) => temps.iter().find(|temp| temp.id == *id).map(|temp| temp.ty),
            MirOperand::Value(id) => values.get(id).copied(),
            MirOperand::Constant(_) => None,
        }
    }

    let templates = functions
        .iter()
        .filter_map(|function| function.source.map(|source| (source, function.clone())))
        .filter(|(source, _)| companion_sources.contains(source))
        .collect::<handroll::FxHashMap<_, _>>();

    for function in functions {
        // Operand types are read from the live locals and temps: an earlier
        // companion call in this function has already retyped its destination,
        // and a later call consuming it must bind against that concrete type.
        for block in &mut function.blocks {
            let mut values = handroll::FxHashMap::default();
            for statement in &mut block.statements {
                if let MirStatementKind::Assign { value, .. } = &statement.kind {
                    values.insert(value.id, value.ty);
                }
                let MirStatementKind::Call { destination, callee, args } = &mut statement.kind else {
                    continue;
                };
                let MirCallee::Definition { source, .. } = callee else {
                    continue;
                };
                let Some(template) = templates.get(source) else {
                    continue;
                };
                let mut bindings = MirShapeBindings::default();
                let mut type_bindings = MirGenericBindings::default();
                for (expected, argument) in template.params.iter().zip(args.iter()) {
                    let Some(actual) = operand_ty(&function.locals, &function.temps, &values, argument) else {
                        continue;
                    };
                    if !bind_generic_type_witness_with_shapes(
                        types,
                        expected.ty,
                        actual,
                        &mut type_bindings,
                        &mut bindings,
                    ) {
                        return Err(SpannedCodegenError {
                            message: "call argument type mismatch".to_owned(),
                            args: vec![DiagnosticArg::new("issue", "air_companion_call_argument_type_mismatch")],
                            span: Some(statement.span),
                        });
                    }
                }
                let symbols = companion_shape_symbols(types, template);
                let shape_args = symbols
                    .iter()
                    .filter_map(|symbol| {
                        bindings
                            .get(symbol)
                            .copied()
                            .map(|shape| canonical_index(types, shape))
                    })
                    .collect::<Vec<_>>();
                let key = MirMonomorphizationKey { source: *source, type_args: Vec::new(), shape_args };
                let Some(&target) = instances.get(&key) else {
                    return Err(SpannedCodegenError {
                        message: "generic call signature has unresolved type parameter".to_owned(),
                        args: vec![DiagnosticArg::new("issue", "air_companion_unresolved_type_parameter")],
                        span: Some(statement.span),
                    });
                };
                if let Some(destination) = destination {
                    let Some(&concrete_return) = instance_return_types.get(&key) else {
                        return Err(SpannedCodegenError {
                            message: "generic companion instance is missing".to_owned(),
                            args: vec![DiagnosticArg::new("issue", "air_companion_instance_missing")],
                            span: Some(statement.span),
                        });
                    };
                    match destination.base {
                        MirPlaceBase::Local(id) => {
                            if let Some(local) = function.locals.iter_mut().find(|local| local.id == id) {
                                local.ty = concrete_return;
                            }
                        }
                        MirPlaceBase::Temp(id) => {
                            if let Some(temp) = function.temps.iter_mut().find(|temp| temp.id == id) {
                                temp.ty = concrete_return;
                            }
                        }
                    }
                }
                *callee = MirCallee::Function(target);
            }

            if let MirTerminatorKind::TryCall { callee: MirCallee::Definition { source, .. }, .. } =
                &block.terminator.kind
                && templates.contains_key(source)
            {
                return Err(SpannedCodegenError {
                    message: "generic call signature has unresolved type parameter".to_owned(),
                    args: vec![DiagnosticArg::new("issue", "air_companion_unresolved_type_parameter")],
                    span: Some(block.terminator.span),
                });
            }
        }
    }
    Ok(())
}

#[cfg(feature = "device")]
fn carry_air_backward_templates<'a>(
    mut lowered: crate::mir::LoweredMirUnit<'a>,
    templates: &[crate::mir::MirFunction],
) -> crate::mir::LoweredMirUnit<'a> {
    // AIR companions are generated after ordinary unit monomorphization, so
    // the lowerer cannot place them in `generic_templates` itself. Package
    // linking sees only that sidecar when it must instantiate a companion for
    // a caller in another unit. Carry the generated body and its shape-key
    // declaration into the same sidecar before the package merge. The body
    // stays in `program` for the standalone route; package instantiation uses
    // this copy as the template and its canonical key discipline.
    let carried = {
        let types = lowered.validated.validation().types.as_table();
        templates
            .iter()
            .filter_map(|template| {
                let source = template.source?;
                let symbols = companion_shape_symbols(types, template);
                (!symbols.is_empty()).then_some((source, template.clone(), symbols))
            })
            .collect::<Vec<_>>()
    };
    for (source, template, symbols) in carried {
        if lowered
            .generic_params
            .iter()
            .any(|(existing, _)| *existing == source)
        {
            continue;
        }
        lowered.generic_templates.push(template);
        lowered.generic_params.push((
            source,
            symbols
                .into_iter()
                .map(|name| crate::mir::MirGenericParam { name, is_shape: true, runtime_witness: None })
                .collect(),
        ));
    }
    lowered
}

#[cfg(feature = "device")]
fn companion_shape_symbols(
    types: &crate::semantic::TypeTable,
    function: &crate::mir::MirFunction,
) -> Vec<crate::lexer::Symbol> {
    fn collect_index(
        types: &crate::semantic::TypeTable,
        index: crate::semantic::IndexId,
        symbols: &mut Vec<crate::lexer::Symbol>,
    ) {
        match types.get_index(index) {
            crate::semantic::IndexExpr::Param(symbol) => {
                if !symbols.contains(symbol) {
                    symbols.push(*symbol);
                }
            }
            crate::semantic::IndexExpr::Tuple(dims) => {
                for dim in dims {
                    collect_index(types, *dim, symbols);
                }
            }
            crate::semantic::IndexExpr::Affine { base, .. } => collect_index(types, *base, symbols),
            crate::semantic::IndexExpr::Divide { numerator, .. } => collect_index(types, *numerator, symbols),
            crate::semantic::IndexExpr::Literal(_)
            | crate::semantic::IndexExpr::Runtime(_)
            | crate::semantic::IndexExpr::Infer(_)
            | crate::semantic::IndexExpr::Unspecified => {}
        }
    }

    fn collect_type(
        types: &crate::semantic::TypeTable,
        ty: crate::semantic::TypeId,
        symbols: &mut Vec<crate::lexer::Symbol>,
    ) {
        match types.get(ty) {
            crate::semantic::Type::Tensor(inner, shape)
            | crate::semantic::Type::Vector(inner, shape)
            | crate::semantic::Type::Matrix(inner, shape)
            | crate::semantic::Type::Sparsa(inner, shape) => {
                collect_type(types, *inner, symbols);
                collect_index(types, *shape, symbols);
            }
            crate::semantic::Type::Tuple(elements) => {
                for element in elements {
                    collect_type(types, *element, symbols);
                }
            }
            _ => {}
        }
    }

    let mut symbols = Vec::new();
    for param in &function.params {
        collect_type(types, param.ty.semantic_id(), &mut symbols);
    }
    collect_type(types, function.return_ty.semantic_id(), &mut symbols);
    symbols
}

#[cfg(feature = "device")]
fn finalize_air_backward_instance(
    types: &mut crate::semantic::TypeTable,
    template: &crate::mir::MirFunction,
    bindings: &crate::mir::generic::MirShapeBindings,
) -> Option<crate::mir::MirFunction> {
    use crate::mir::generic::substitute_shape_size_constants;

    let mut function = template.clone();
    let span = function.span;
    for param in &mut function.params {
        param.ty = instantiate_air_backward_type(types, param.ty, bindings, span)?;
    }
    for local in &mut function.locals {
        local.ty = instantiate_air_backward_type(types, local.ty, bindings, span)?;
    }
    for temp in &mut function.temps {
        temp.ty = instantiate_air_backward_type(types, temp.ty, bindings, span)?;
    }
    function.return_ty = instantiate_air_backward_type(types, function.return_ty, bindings, span)?;
    function.error_ty = match function.error_ty {
        Some(ty) => Some(instantiate_air_backward_type(types, ty, bindings, span)?),
        None => None,
    };
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            match &mut statement.kind {
                crate::mir::MirStatementKind::Assign { value, .. } => {
                    value.ty = instantiate_air_backward_type(types, value.ty, bindings, span)?;
                }
                crate::mir::MirStatementKind::RuntimeCall { call, .. } => {
                    call.return_ty = instantiate_air_backward_type(types, call.return_ty, bindings, span)?;
                    if let crate::mir::MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
                        conversion.source_ty =
                            instantiate_air_backward_type(types, conversion.source_ty, bindings, span)?;
                        conversion.target_ty =
                            instantiate_air_backward_type(types, conversion.target_ty, bindings, span)?;
                    }
                }
                crate::mir::MirStatementKind::Construct { aggregate, .. } => {
                    aggregate.ty = instantiate_air_backward_type(types, aggregate.ty, bindings, span)?;
                }
                crate::mir::MirStatementKind::Call { .. } => {}
            }
        }
    }
    if !substitute_shape_size_constants(types, &mut function, bindings) {
        return None;
    }
    // The instance is executable: expand its deferred vector ops now, and
    // refuse an instance whose ops cannot expand.
    let mut errors = Vec::new();
    crate::mir::expand_deferred_vector_ops(types, &mut function, &mut errors);
    errors.is_empty().then_some(function)
}

#[cfg(feature = "device")]
fn instantiate_air_backward_type(
    types: &mut crate::semantic::TypeTable,
    ty: crate::mir::MirType,
    bindings: &crate::mir::generic::MirShapeBindings,
    span: crate::lexer::Span,
) -> Option<crate::mir::MirType> {
    if let crate::semantic::Type::Tuple(elements) = types.get(ty.semantic_id()).clone() {
        let elements = elements
            .into_iter()
            .map(|element| {
                instantiate_air_backward_type(types, crate::mir::MirType::semantic(element), bindings, span)
                    .map(crate::mir::MirType::semantic_id)
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(crate::mir::MirType::semantic(
            types.intern(crate::semantic::Type::Tuple(elements)),
        ));
    }
    crate::mir::generic::instantiate_generic_type_with_shapes(
        types,
        ty,
        &crate::mir::MirGenericBindings::default(),
        bindings,
        span,
    )
    .ok()
}

#[cfg(feature = "device")]
fn reject_shape_size_survivors(
    functions: &[crate::mir::MirFunction],
    message: &str,
) -> Result<(), SpannedCodegenError> {
    for function in functions {
        let mut function = function.clone();
        if crate::mir::generic::function_contains_shape_size(&mut function) {
            return Err(SpannedCodegenError {
                message: message.to_owned(),
                args: vec![DiagnosticArg::new(
                    "issue",
                    "unresolved_shape_size_constant_survives_monomorphization",
                )],
                span: Some(function.span),
            });
        }
    }
    Ok(())
}

#[cfg(feature = "mir-wgsl")]
#[cfg(feature = "mir-core")]
fn lower_graphics_mir_with_context_for_target(
    analysis: &mut AnalyzedModule,
) -> Result<crate::mir::LoweredMirUnit<'_>, SpannedCodegenError> {
    // Do NOT filter the HIR to shader-only entries — the graphics path only
    // extracts vertex/fragment MIR functions from the lowered result, and an
    // in-place HIR mutation would corrupt the program for any fallback path
    // that continues to compute lowering (F-107). The full HIR lowers safely
    // to target-neutral MIR; graphics-specific filtering is a selection on
    // the output, not a precondition on the input.
    crate::mir::lower_analyzed_unit(analysis.application_view(), crate::mir::MirLoweringOptions::default())
        .map_err(mir_errors_to_spanned)
}

#[cfg_attr(not(feature = "device"), allow(unused_variables))]
#[cfg(feature = "mir-core")]
fn lower_mir_with_context_for_target(
    analysis: &mut AnalyzedModule,
    no_fuse: bool,
) -> Result<crate::mir::LoweredMirUnit<'_>, SpannedCodegenError> {
    #[cfg(feature = "device")]
    let bundle = prepare_air_backward_bundle(analysis)?;
    #[cfg_attr(not(feature = "device"), allow(unused_mut))]
    let mut lowered =
        crate::mir::lower_analyzed_unit(analysis.application_view(), crate::mir::MirLoweringOptions::default())
            .map_err(mir_errors_to_spanned)?;
    #[cfg(feature = "device")]
    if let Some(bundle) = bundle {
        lowered = apply_air_backward_bundle(lowered, bundle, analysis, no_fuse)?;
        // Re-construct the proof token after backward companions were added.
        let (program, context) = lowered.validated.into_parts();
        lowered.validated = crate::mir::ValidatedMir::new(program, context).map_err(|errors| {
            let messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
            SpannedCodegenError {
                message: format!(
                    "MIR validation after backward companion addition failed: {}",
                    messages.join("; ")
                ),
                args: vec![DiagnosticArg::new(
                    "issue",
                    "mir_validation_after_companion_addition_failed",
                )],
                span: None,
            }
        })?;
    }
    Ok(lowered)
}

#[cfg(feature = "device")]
fn air_mir_from_analysis(
    analysis: &AnalyzedModule,
) -> Result<(crate::air::AirProgram, crate::mir::MirProgram), SpannedCodegenError> {
    let air = crate::air::lower_program(&analysis.hir, &analysis.radix_lanes, &analysis.interner, &analysis.types)
        .map_err(air_lower_errors_to_spanned)?;
    crate::air::validate_program(&air).map_err(air_validation_errors_to_spanned)?;
    let air_mir = radix_air_bridge::to_mir_program(
        &air,
        &radix_air_bridge::AirToMirContext::with_interner_and_types(&analysis.interner, &analysis.types),
    )
    .map_err(air_to_mir_errors_to_spanned)?;

    Ok((air, air_mir))
}

struct FreshMirFunctionIds {
    next: u64,
}

#[cfg(feature = "device")]
impl FreshMirFunctionIds {
    fn after(ids: impl IntoIterator<Item = crate::mir::MirFunctionId>) -> Self {
        let next = ids
            .into_iter()
            .map(|id| u64::from(id.0))
            .max()
            .map_or(0, |id| id + 1);
        Self { next }
    }

    fn next(&mut self) -> Result<crate::mir::MirFunctionId, codegen::CodegenError> {
        let id = u32::try_from(self.next).map_err(|_| codegen::CodegenError {
            message: "MIR function id space exhausted".to_owned(),
            args: vec![DiagnosticArg::new("issue", "mir_function_id_space_exhausted")],
        })?;
        self.next += 1;
        Ok(crate::mir::MirFunctionId(id))
    }
}

#[cfg(feature = "device")]
fn replace_air_lane_mir(
    lowered: crate::mir::LoweredMirUnit<'_>,
    air_mir: crate::mir::MirProgram,
) -> Result<crate::mir::LoweredMirUnit<'_>, SpannedCodegenError> {
    lowered
        .patch_program(|program| {
            let mut fresh_ids = FreshMirFunctionIds::after(program.functions.iter().map(|function| function.id));
            for mut function in air_mir.functions {
                let Some(source) = function.source else {
                    continue;
                };
                if let Some(index) = program
                    .functions
                    .iter()
                    .position(|candidate| candidate.source == Some(source))
                {
                    function.id = program.functions[index].id;
                    // S3-A2: a differentiable device kernel (nucleum + `@ radix
                    // backward`) is replaced by its AIR-lowered form but must KEEP its
                    // kernel shader stage — the forward kernel is the primal itself.
                    if function.shader_stage.is_none() {
                        function.shader_stage = program.functions[index].shader_stage;
                    }
                    program.functions[index] = function;
                } else {
                    function.id = fresh_ids.next()?;
                    program.functions.push(function);
                }
            }
            Ok::<(), codegen::CodegenError>(())
        })
        .map_err(|error| match error {
            radix_mir::validate::PatchProgramError::Patch(error) => SpannedCodegenError::from(error),
            radix_mir::validate::PatchProgramError::Validation(errors) => mir_validation_errors_to_spanned(errors),
        })
}

/// Accepts a pre-built companion (from [`generate_companions_bottom_up`])
/// and runs the validation/installation chain instead of calling
/// [`transform_reverse_ad`] directly.
///
/// Interprocedural Call bindings were already resolved by bottom-up companion
/// generation in the caller, so this function receives the finished companion
/// and needs no callee map of its own.
#[cfg(feature = "device")]
fn generate_air_lane_backward_with_companion<'a>(
    primal: &AirBackwardPrimal,
    companion: crate::air::AirFunction,
    lowered: crate::mir::LoweredMirUnit<'a>,
    fresh_ids: &mut FreshMirFunctionIds,
    no_fuse: bool,
) -> Result<(crate::mir::LoweredMirUnit<'a>, crate::mir::device::LosslessMirCompanionEntry), codegen::CodegenError> {
    use crate::air::fusion::{FusionTable, fuse_companion};
    use crate::air::{
        AirCompanionDerivativeKind, AirCompanionErrorChannel, AirCompanionOracleRef, AirCompanionRelation,
        AirCompanionSelectedInput, AirCompanionSelectedOutput,
    };
    use radix_air_bridge::{
        AirGradientCheckAdmissionInputs, admit_generated_gradient_check, air_gradient_check_eligibility_witness,
        validate_generated_backward_to_mir_from_function, validate_mir_backward_result_contract,
    };

    let types_ref = lowered.validated.validation().types.as_table();

    // Run fusion pass on the companion body (unless disabled).
    let (fused_body, fusion_table) = if no_fuse {
        (companion.body, FusionTable::default())
    } else {
        fuse_companion(companion.body, false)
    };
    let companion = crate::air::AirFunction { body: fused_body, ..companion };
    let air_to_mir_context = radix_air_bridge::AirToMirContext {
        interner: None,
        types: Some(types_ref),
        fusion_table: if fusion_table.is_empty() {
            None
        } else {
            Some(&fusion_table)
        },
    };

    // Build selected inputs dynamically from the derived primal metadata.
    let selected_inputs: Vec<AirCompanionSelectedInput> = primal
        .selected_param_sources
        .iter()
        .zip(primal.selected_input_types.iter())
        .enumerate()
        .map(|(position, (&source, &ty))| {
            Ok(AirCompanionSelectedInput {
                source,
                position: u32::try_from(position).map_err(|_| codegen::CodegenError {
                    message: "selected parameter count exceeds u32".to_owned(),
                    args: vec![DiagnosticArg::new("issue", "air_companion_selected_parameter_count")],
                })?,
                ty,
                gradient_ty: ty,
            })
        })
        .collect::<Result<_, codegen::CodegenError>>()?;
    let selected_inputs: &'static [AirCompanionSelectedInput] = selected_inputs.leak();

    let selected_outputs: &'static [AirCompanionSelectedOutput] =
        vec![AirCompanionSelectedOutput { position: 0, ty: primal.ret_ty, upstream_gradient_ty: primal.ret_ty }].leak();

    let relation = AirCompanionRelation {
        primal: primal.primal_def_id,
        companion: primal.companion_def_id,
        derivative: AirCompanionDerivativeKind::ReverseModeVjp,
        selected_inputs,
        selected_outputs,
        return_ty: companion.return_ty,
        oracle: AirCompanionOracleRef { packet: "air-same-shape-derivative-oracle", case_family: "reverse_ad" },
        // Tier 1 (AT1-1): driver-generated reverse-AD companions are
        // success-only; a failable boundary must state its posture and
        // record its typed error facts explicitly instead of defaulting
        // silently.
        error_channel: AirCompanionErrorChannel {
            posture: crate::air::AirCompanionBoundaryPosture::SuccessOnly,
            error: None,
        },
    };

    // Run the validation chain: AIR-to-MIR → result contract → gradient check.
    let mir_validation =
        validate_generated_backward_to_mir_from_function(&companion, &relation, types_ref, &air_to_mir_context)
            .map_err(|errors| air_proof_errors_to_codegen("AIR-to-MIR backward validation", &errors))?;

    let result_contract = validate_mir_backward_result_contract(&mir_validation, &relation, types_ref)
        .map_err(|errors| air_proof_errors_to_codegen("MIR backward result contract", &errors))?;

    let eligibility = air_gradient_check_eligibility_witness()
        .map_err(|errors| air_proof_errors_to_codegen("gradient check eligibility", &errors))?;

    // Build a dummy AirSameShapeTransformProof for the admission gate.
    let dummy_proof = radix_air_bridge::AirSameShapeTransformProof {
        relation,
        oracle_case: "",
        function: companion,
        output_value: crate::air::AirValueId(0),
        lhs_gradient_value: crate::air::AirValueId(0),
        rhs_gradient_value: crate::air::AirValueId(0),
        result_value: crate::air::AirValueId(0),
        expected_output: vec![],
        expected_lhs_gradient: vec![],
        expected_rhs_gradient: vec![],
    };

    let admission_inputs = AirGradientCheckAdmissionInputs {
        generated_differentiable_eligibility: Some(&eligibility),
        transform: Some(&dummy_proof),
        mir_validation: Some(&mir_validation),
        result_contract: Some(&result_contract),
    };
    admit_generated_gradient_check(admission_inputs)
        .map_err(|errors| air_proof_errors_to_codegen("gradient check admission", &errors))?;

    // Prepare the generated backward MIR functions for the consuming patch.
    let backward_mir = mir_validation.mir;
    let mut backward_functions = Vec::with_capacity(backward_mir.functions.len());
    for mut function in backward_mir.functions {
        function.id = fresh_ids.next()?;
        backward_functions.push(function);
    }

    // S3-A2 / Stage 3R F4: the relation facts are CARRIED forward (not
    // discarded with the dummy proof) as the LOSSLESS entry — selected
    // inputs (with the gradient-to-primal identity: each parameter names the
    // companion result-tuple slot that carries its gradient), selected
    // outputs, and device-residency — so the companion-aware materializer
    // can lower the generated companion into a multi-output device kernel
    // without reconstructing the relation from names, shapes, or positions.
    // The selected-input positions reuse the checked conversions already
    // performed for the AIR relation (no new `expect` sites).
    let (lowered, entry) = build_lossless_companion_entry(primal, selected_inputs, backward_functions, lowered)?;
    Ok((lowered, entry))
}

#[cfg(feature = "device")]
fn build_lossless_companion_entry<'a>(
    primal: &AirBackwardPrimal,
    selected_inputs: &'static [crate::air::AirCompanionSelectedInput],
    mut backward_functions: Vec<crate::mir::MirFunction>,
    lowered: crate::mir::LoweredMirUnit<'a>,
) -> Result<(crate::mir::LoweredMirUnit<'a>, crate::mir::device::LosslessMirCompanionEntry), codegen::CodegenError> {
    use crate::mir::device::{
        LosslessMirCompanionEntry, MirCompanionDerivativeKind, MirCompanionSelectedInput, MirCompanionSelectedOutput,
    };
    let selected_inputs: Vec<MirCompanionSelectedInput> = selected_inputs
        .iter()
        .map(|air| MirCompanionSelectedInput {
            param: air.source,
            position: air.position,
            ty: air.ty,
            gradient_slot: air.position,
        })
        .collect();
    let selected_outputs: Vec<MirCompanionSelectedOutput> =
        vec![MirCompanionSelectedOutput { position: 0, ty: primal.ret_ty, upstream_gradient_ty: primal.ret_ty }];
    // GT12 E15: a generated companion of a device-resident primal joins the
    // same `DeviceProgram` as its primal, so it must carry the kernel launch
    // stage the AIR-to-MIR form never states (`to_mir` installs `None`). This
    // mirrors the S3-A2 stage-keep the primal itself gets in
    // `replace_air_lane_mir`: ordinary dispatch keys a call on
    // `shader_stage == Some(Compute)`, so a stage-free companion would be
    // host-interpreted inside a device run. Host primals keep `None` — their
    // companions stay host helpers and reference-only execution stays
    // explicit (unannotated callee companions are unaffected; they are
    // installed by `install_unannotated_companions`).
    if primal.device_resident {
        for function in &mut backward_functions {
            function.shader_stage = Some(crate::mir::MirKernelShaderStage::Compute);
        }
    }
    let entry = LosslessMirCompanionEntry {
        primal: primal.primal_def_id,
        companion: primal.companion_def_id,
        derivative: MirCompanionDerivativeKind::ReverseModeVjp,
        device_resident: primal.device_resident,
        selected_inputs,
        selected_outputs,
    };
    let lowered = lowered
        .patch_program(|program| {
            program.functions.extend(backward_functions);
            Ok::<(), std::convert::Infallible>(())
        })
        .map_err(|error| match error {
            radix_mir::validate::PatchProgramError::Patch(never) => match never {},
            radix_mir::validate::PatchProgramError::Validation(errors) => {
                let messages = errors
                    .iter()
                    .map(|error| error.message.clone())
                    .collect::<Vec<_>>();
                codegen::CodegenError {
                    message: format!(
                        "MIR validation after backward companion addition failed: {}",
                        messages.join("; ")
                    ),
                    args: vec![DiagnosticArg::new(
                        "issue",
                        "mir_validation_after_companion_addition_failed",
                    )],
                }
            }
        })?;
    Ok((lowered, entry))
}

/// Convert AIR proof-chain error vectors into a single `CodegenError`.
///
/// Each error type in the proof chain has a `kind` with a `label()` method
/// and a `detail` field. This helper joins them into a diagnostic message.
fn air_proof_errors_to_codegen<E: std::fmt::Debug>(phase: &str, errors: &[E]) -> codegen::CodegenError {
    let messages: Vec<String> = errors.iter().map(|error| format!("{error:?}")).collect();
    codegen::CodegenError {
        message: format!("{phase} failed: {}", messages.join("; ")),
        args: messages
            .iter()
            .map(|m| DiagnosticArg::new("issue", m.as_str()))
            .collect(),
    }
}

#[cfg(feature = "mir-wasm")]
#[cfg(feature = "mir-core")]
fn wasm_text_codegen_error(error: crate::mir::MirWasmTextProbeError) -> codegen::CodegenError {
    codegen::CodegenError {
        message: error.message,
        args: vec![
            DiagnosticArg::new("issue", error.issue),
            DiagnosticArg::new("shape", error.shape),
        ],
    }
}

#[cfg(feature = "mir-llvm")]
#[cfg(feature = "mir-core")]
fn llvm_text_codegen_error(error: crate::mir::MirLlvmTextProbeError) -> codegen::CodegenError {
    codegen::CodegenError {
        message: error.message,
        args: vec![
            DiagnosticArg::new("category", error.category),
            DiagnosticArg::new("shape", error.shape),
            DiagnosticArg::new("detail", error.detail),
        ],
    }
}

// CLI analysis can already describe more surface than the Phase 03 Rust CLI
// generator accepts. Keep the gap explicit here so unsupported programs fail
// with CLI-shaped diagnostics instead of leaking backend assumptions.
fn phase03_cli_codegen_gap(program: &crate::cli::CliProgram) -> Option<(&'static str, String)> {
    for option in program
        .global_options
        .iter()
        .chain(program.options.iter())
        .chain(
            program
                .commands
                .iter()
                .flat_map(|command| command.options.iter()),
        )
    {
        if !is_cli_scalar_type(&option.ty) {
            return Some((
                "cli_phase03_option_type_unsupported",
                format!(
                    "Phase 03 Rust CLI code generation does not support option '{}' with type {:?}",
                    option.binding, option.ty
                ),
            ));
        }
    }

    for operand in program
        .global_operands
        .iter()
        .chain(program.operands.iter())
        .chain(
            program
                .commands
                .iter()
                .flat_map(|command| command.operands.iter()),
        )
    {
        let supported_scalar = is_cli_scalar_type(&operand.ty);
        let supported_rest_list =
            operand.rest && matches!(operand.ty, crate::cli::CliType::ListaTextus | crate::cli::CliType::ListaNumerus);
        if !supported_scalar && !supported_rest_list {
            return Some((
                "cli_phase03_operand_type_unsupported",
                format!(
                    "Phase 03 Rust CLI code generation does not support operand '{}' with type {:?}",
                    operand.binding, operand.ty
                ),
            ));
        }
    }

    None
}

/// The scalar CLI types the Phase 03 Rust CLI generator supports.
///
/// Options and operands of any other type are rejected by
/// [`phase03_cli_codegen_gap`] with a CLI-shaped diagnostic.
fn is_cli_scalar_type(ty: &crate::cli::CliType) -> bool {
    matches!(
        ty,
        crate::cli::CliType::Textus
            | crate::cli::CliType::Numerus
            | crate::cli::CliType::Fractus
            | crate::cli::CliType::Bivalens
    )
}

/// One coherent module analysis shared by codegen and package-level tooling.
///
/// This is the driver-facing carrier for a single source module: HIR, types,
/// resolver, annotation metadata, function effects, resolved uses, qualified
/// identities, CLI entry, and diagnostics from one analysis stamp. Consumers
/// must not mix fields across source modules or analysis runs.
#[derive(Clone)]
pub struct AnalyzedModule {
    /// Coherence stamp shared by every fact family on this unit.
    pub analysis_stamp: semantic::AnalysisStamp,

    /// Symbol table that owns names referenced by the parsed and lowered forms.
    pub interner: Interner,

    /// Type information produced by semantic analysis.
    pub types: semantic::TypeTable,

    /// Runtime shape equalities retained by semantic analysis for MIR
    /// boundary checks. Kept beside this unit's HIR/type arena so call-site
    /// identities and index terms cannot drift across analysis stamps.
    pub shape_obligations: semantic::ShapeObligationTable,

    /// Per-target kernel width and range requirements inferred from this
    /// module's semantic HIR snapshot.
    pub kernel_requirements: semantic::passes::kernel_requirements::KernelRequirementsReport,

    /// Shifted-view dispatch and generic provider facts from the same typed-HIR run.
    pub tensor_edge_policies: radix_hir::TensorEdgePolicyTable,

    /// Resolver metadata produced by semantic analysis.
    pub resolver: Resolver,

    /// Lowered program consumed by backend code generators.
    pub hir: HirModule,

    /// Presentation sidecar produced by the same lowering pass that produced
    /// `hir`: source anchors, typed trivia owners, attachments, structural
    /// block ends, the program end, and the explicit entry-start anchor.
    /// Inseparable from the HIR value passed to codegen; semantic consumers
    /// stay trivia-blind by construction.
    pub presentation: HirPresentation,

    /// Runnable CLI contract discovered from source or supplied by package mode.
    pub cli_program: Option<crate::cli::CliProgram>,

    /// Target-neutral provenance for library imports and imported declarations.
    pub libraries: LibraryRegistry,

    /// Normalized annotation-contract registry and applications for this unit.
    pub annotation_contracts: semantic::AnnotationContractMetadata,

    /// Upstream function effect/capture/call facts for HIR backends.
    pub function_facts: semantic::FunctionFactTable,

    /// Target-neutral resolved definition and import-use index.
    pub resolved_uses: semantic::ResolvedUseIndex,

    /// Portable qualified identities for imported declarations.
    pub qualified_identities: semantic::QualifiedIdentityTable,

    /// Durable borrow-provenance and last-use summaries (SCR-04) carried
    /// from the analyzed application; the durable F-CAP-04/05 authority.
    pub provenance: semantic::FunctionProvenanceTable,

    /// Package-local import identity facts for user-code cross-module calls
    /// (see [`PackageImportIdentities`]). `None` for plain single-file
    /// compiles.
    pub package_import_identities: Option<PackageImportIdentities>,

    /// Analyzed provider units for this module's import edges, keyed by
    /// import-path spelling (ARCH-U4 / S01-3; see [`ProviderUnitView`]).
    /// Populated by the program-layer contract walk where each target already
    /// parses; backend emission maps over the view instead of re-entering the
    /// front half from disk. Empty for plain single-file compiles; carried
    /// on load from the unit artifact (wire schema 14).
    pub provider_units: ProviderUnitView,

    /// Compiler lane metadata collected from `@ radix lane ...` annotations.
    pub radix_lanes: RadixLaneMetadata,

    /// Compiler-owned GPU builtin call identities keyed by resolved definition.
    pub gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)>,

    /// Source-stage graphics facts collected before backend emission.
    #[cfg(feature = "mir-core")]
    pub graphics_source: GraphicsSourceFacts,

    /// Files read at build time by `insere` embeds (D8.10), canonical paths.
    /// They are build inputs: a package driver that caches analysis must
    /// rebuild when one changes. Empty for loaded artifacts.
    pub embedded_inputs: Vec<std::path::PathBuf>,

    /// Words this file's print statements render `true`, `false`, `none` and
    /// `tuple` as: the file's own locale pack (frontmatter locale, else the
    /// session pack), Latin when the file reads with no pack and for loaded
    /// artifacts.
    pub display_tokens: crate::runtime_contract::display_tokens::DisplayTokens,

    /// Warnings and informational diagnostics accumulated before codegen.
    pub diagnostics: Vec<Diagnostic>,
}

impl AnalyzedModule {
    /// Construct an [`AnalyzedModule`] from individually reconstructed arenas.
    ///
    /// This is the non-analysis entry point used by the HIR artifact loader
    /// (U2.1–U2.4). Each arena is assembled by the loader from its wire
    /// snapshot and then passed here; this constructor performs no
    /// deserialization, parsing, or semantic analysis.
    #[must_use]
    pub fn from_loaded(
        analysis_stamp: semantic::AnalysisStamp,
        interner: Interner,
        types: semantic::TypeTable,
        resolver: Resolver,
        hir: HirModule,
        cli_program: Option<crate::cli::CliProgram>,
        libraries: LibraryRegistry,
        annotation_contracts: semantic::AnnotationContractMetadata,
        function_facts: semantic::FunctionFactTable,
        resolved_uses: semantic::ResolvedUseIndex,
        qualified_identities: semantic::QualifiedIdentityTable,
        provenance: semantic::FunctionProvenanceTable,
        radix_lanes: RadixLaneMetadata,
        gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)>,
        tensor_edge_policies: radix_hir::TensorEdgePolicyTable,
        #[cfg(feature = "mir-core")] graphics_source: GraphicsSourceFacts,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self::from_loaded_with_presentation(
            analysis_stamp,
            interner,
            types,
            resolver,
            hir,
            empty_presentation(),
            cli_program,
            libraries,
            annotation_contracts,
            function_facts,
            resolved_uses,
            qualified_identities,
            provenance,
            radix_lanes,
            gpu_builtins,
            tensor_edge_policies,
            #[cfg(feature = "mir-core")]
            graphics_source,
            diagnostics,
        )
    }

    /// Construct an [`AnalyzedModule`] from individually reconstructed arenas
    /// plus the presentation sidecar.
    ///
    /// This is the artifact-loader constructor (U-4): the loader reconstructs
    /// HIR and its presentation together so the unit always carries both.
    #[must_use]
    pub fn from_loaded_with_presentation(
        analysis_stamp: semantic::AnalysisStamp,
        interner: Interner,
        types: semantic::TypeTable,
        resolver: Resolver,
        hir: HirModule,
        presentation: HirPresentation,
        cli_program: Option<crate::cli::CliProgram>,
        libraries: LibraryRegistry,
        annotation_contracts: semantic::AnnotationContractMetadata,
        function_facts: semantic::FunctionFactTable,
        resolved_uses: semantic::ResolvedUseIndex,
        qualified_identities: semantic::QualifiedIdentityTable,
        provenance: semantic::FunctionProvenanceTable,
        radix_lanes: RadixLaneMetadata,
        gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)>,
        tensor_edge_policies: radix_hir::TensorEdgePolicyTable,
        #[cfg(feature = "mir-core")] graphics_source: GraphicsSourceFacts,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            analysis_stamp,
            interner,
            types,
            shape_obligations: semantic::ShapeObligationTable::default(),
            kernel_requirements: semantic::passes::kernel_requirements::KernelRequirementsReport::default(),
            tensor_edge_policies,
            resolver,
            hir,
            presentation,
            cli_program,
            libraries,
            annotation_contracts,
            function_facts,
            resolved_uses,
            qualified_identities,
            provenance,
            package_import_identities: None,
            provider_units: ProviderUnitView::default(),
            radix_lanes,
            gpu_builtins,
            #[cfg(feature = "mir-core")]
            graphics_source,
            embedded_inputs: Vec::new(),
            display_tokens: crate::runtime_contract::display_tokens::DisplayTokens::latin(),
            diagnostics,
        }
    }

    /// Borrow the coherent analyzed-application fact families on this unit.
    #[must_use]
    pub fn application_view(&self) -> AnalyzedModuleApplicationView<'_> {
        AnalyzedModuleApplicationView {
            stamp: self.analysis_stamp,
            hir: &self.hir,
            types: &self.types,
            shape_obligations: &self.shape_obligations,
            kernel_requirements: &self.kernel_requirements,
            tensor_edge_policies: &self.tensor_edge_policies,
            resolver: &self.resolver,
            annotation_contracts: &self.annotation_contracts,
            libraries: &self.libraries,
            function_facts: &self.function_facts,
            resolved_uses: &self.resolved_uses,
            qualified_identities: &self.qualified_identities,
            package_import_identities: self.package_import_identities.as_ref(),
            provenance: &self.provenance,
            radix_lanes: &self.radix_lanes,
            gpu_builtins: &self.gpu_builtins,
            cli_program: self.cli_program.as_ref(),
            display_tokens: &self.display_tokens,
            interner: &self.interner,
        }
    }
}

/// Codegen-facing view of one analyzed module (C-D1a).
///
/// The driver owns this implementation; codegen reads the same facts through
/// [`CodegenAnalysis`] without naming `AnalyzedModule`. Every accessor borrows
/// the unit's own arena, so emission never copies or re-derives a fact.
impl CodegenAnalysis for AnalyzedModule {
    fn interner(&self) -> &Interner {
        &self.interner
    }

    fn types(&self) -> &semantic::TypeTable {
        &self.types
    }

    fn hir(&self) -> &HirModule {
        &self.hir
    }

    fn resolver(&self) -> &Resolver {
        &self.resolver
    }

    fn function_facts(&self) -> &semantic::FunctionFactTable {
        &self.function_facts
    }

    fn presentation(&self) -> &HirPresentation {
        &self.presentation
    }

    fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    fn libraries(&self) -> &LibraryRegistry {
        &self.libraries
    }

    fn tensor_edge_policies(&self) -> &radix_hir::TensorEdgePolicyTable {
        &self.tensor_edge_policies
    }

    fn resolved_uses(&self) -> &semantic::ResolvedUseIndex {
        &self.resolved_uses
    }

    fn gpu_builtins(&self) -> &[(DefId, radix_types::MirKernelBuiltin)] {
        &self.gpu_builtins
    }

    fn shape_obligations(&self) -> &semantic::ShapeObligationTable {
        &self.shape_obligations
    }

    fn provenance(&self) -> &semantic::FunctionProvenanceTable {
        &self.provenance
    }

    fn display_tokens(&self) -> &crate::runtime_contract::display_tokens::DisplayTokens {
        &self.display_tokens
    }

    fn provider(&self, import_path: &str) -> Option<CodegenProvider<'_>> {
        match self.provider_units.get(import_path)? {
            CarriedProviderUnit::Failed { message } => Some(CodegenProvider::Failed { message: message.clone() }),
            carried @ CarriedProviderUnit::Unit(_) => {
                let guard = carried.lock_unit()?;
                Some(CodegenProvider::Unit(Box::new(provider_analysis_unit(guard))))
            }
        }
    }

    fn provider_keys(&self) -> Vec<String> {
        self.provider_units.units.keys().cloned().collect()
    }
}

/// Owned codegen view of one locked provider unit (C-D1a).
///
/// [`CodegenAnalysis::provider`] hands the provider walk a value that owns the
/// guard, so the walk boxes this wrapper. Provider units carry only the
/// program, symbol/type arenas, resolver, dispatch facts, shape guards, and
/// diagnostics; provider emission passes defaults for the remaining facts, so
/// those accessors answer with the same empty defaults the emitter uses.
struct ProviderAnalysisUnit<'a> {
    guard: ProviderUnitGuard<'a>,
    presentation: HirPresentation,
    libraries: LibraryRegistry,
    function_facts: semantic::FunctionFactTable,
    resolved_uses: semantic::ResolvedUseIndex,
    provenance: semantic::FunctionProvenanceTable,
    display_tokens: crate::runtime_contract::display_tokens::DisplayTokens,
}

/// Build the owned view of one locked provider unit.
fn provider_analysis_unit(guard: ProviderUnitGuard<'_>) -> ProviderAnalysisUnit<'_> {
    ProviderAnalysisUnit {
        guard,
        presentation: empty_presentation(),
        libraries: LibraryRegistry::default(),
        function_facts: semantic::FunctionFactTable::default(),
        resolved_uses: semantic::ResolvedUseIndex::default(),
        provenance: semantic::FunctionProvenanceTable::default(),
        display_tokens: crate::runtime_contract::display_tokens::DisplayTokens::latin(),
    }
}

impl CodegenAnalysis for ProviderAnalysisUnit<'_> {
    fn interner(&self) -> &Interner {
        &self.guard.interner
    }

    fn types(&self) -> &semantic::TypeTable {
        &self.guard.types
    }

    fn hir(&self) -> &HirModule {
        &self.guard.hir
    }

    fn resolver(&self) -> &Resolver {
        &self.guard.resolver
    }

    fn function_facts(&self) -> &semantic::FunctionFactTable {
        &self.function_facts
    }

    fn presentation(&self) -> &HirPresentation {
        &self.presentation
    }

    fn diagnostics(&self) -> &[Diagnostic] {
        &self.guard.diagnostics
    }

    fn libraries(&self) -> &LibraryRegistry {
        &self.libraries
    }

    fn tensor_edge_policies(&self) -> &radix_hir::TensorEdgePolicyTable {
        &self.guard.tensor_edge_policies
    }

    fn resolved_uses(&self) -> &semantic::ResolvedUseIndex {
        &self.resolved_uses
    }

    fn gpu_builtins(&self) -> &[(DefId, radix_types::MirKernelBuiltin)] {
        &[]
    }

    fn shape_obligations(&self) -> &semantic::ShapeObligationTable {
        &self.guard.shape_obligations
    }

    fn provenance(&self) -> &semantic::FunctionProvenanceTable {
        &self.provenance
    }

    fn display_tokens(&self) -> &crate::runtime_contract::display_tokens::DisplayTokens {
        &self.display_tokens
    }

    fn provider(&self, import_path: &str) -> Option<CodegenProvider<'_>> {
        match self.guard.nested.get(import_path)? {
            CarriedProviderUnit::Failed { message } => Some(CodegenProvider::Failed { message: message.clone() }),
            carried @ CarriedProviderUnit::Unit(_) => {
                let guard = carried.lock_unit()?;
                Some(CodegenProvider::Unit(Box::new(provider_analysis_unit(guard))))
            }
        }
    }

    fn provider_keys(&self) -> Vec<String> {
        self.guard.nested.units.keys().cloned().collect()
    }
}

/// Source graphics metadata admitted through the ordinary frontend before MIR-backed emission.
///
/// Contract facts are the generic [`radix_mir::shader_contract`] records,
/// ingested from resolved contract-role calls (see [`shader_contract`]); no
/// Triga-shaped types or invented defaults live here.
#[derive(Debug, Clone, Default)]
#[cfg(feature = "mir-core")]
pub struct GraphicsSourceFacts {
    pub vertex_entries: Vec<GraphicsVertexEntry>,
    pub fragment_entries: Vec<GraphicsFragmentEntry>,
    pub shader_vertex_attributes: Vec<radix_mir::ShaderVertexAttribute>,
    pub shader_varyings: Vec<radix_mir::ShaderVarying>,
    pub shader_fragment_outputs: Vec<radix_mir::ShaderFragmentOutput>,
    pub shader_pipeline_states: Vec<radix_mir::ShaderPipelineState>,
    pub shader_resource_bindings: Vec<radix_mir::ShaderResourceBinding>,
    pub bound_resource_bindings: Vec<radix_mir::ShaderBoundResourceBinding>,
}

/// One stage-entry source parameter: its name and, when its type is a nominal
/// (`Type::Struct`), that nominal's identity. Identity is the map key a bound
/// type is resolved by; it is `None` for every other type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsStageParam {
    pub name: String,
    pub type_def: Option<crate::hir::DefId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsVertexEntry {
    pub def_id: crate::hir::DefId,
    pub entry_name: String,
    pub annotation_payload_count: usize,
    pub param_count: usize,
    pub params: Vec<GraphicsStageParam>,
    pub returns_vacuum: bool,
    pub has_error_type: bool,
    pub body_shape: GraphicsVertexBodyShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsVertexBodyShape {
    EmptyBlock,
    Missing,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsFragmentEntry {
    pub def_id: crate::hir::DefId,
    pub entry_name: String,
    pub annotation_payload_count: usize,
    pub param_count: usize,
    pub params: Vec<GraphicsStageParam>,
    pub returns_vacuum: bool,
    pub has_error_type: bool,
    pub body_shape: GraphicsVertexBodyShape,
}

/// Caller-supplied inputs for the single [`analyze_source`] entry.
#[derive(Debug, Clone, Default)]
pub struct AnalyzeRequest {
    /// Optional package-level CLI program discovered while assembling the
    /// import graph. When present, the driver interns the CLI symbols into
    /// this source unit before semantic analysis so CLI bindings and lowered
    /// HIR share the same symbol space.
    pub cli_program: Option<crate::cli::CliProgram>,
    /// Fail-closed import contract for the resolver.
    pub contract: ModuleImportContract,
}

/// Explicit package admission facts for exported size-generic kernel
/// templates (PKG-ADM, `device-fragments-v2`).
///
/// Package analysis builds this from the unit's own package export surface:
/// a top-level `@ publica`-tier export is demand-admissible as a package
/// template even with no same-unit caller, because consumers instantiate it
/// from their own units. Admission is check-stage and diagnostic-only — it
/// suppresses the `ADM001` orphan diagnostic and mints no MIR
/// instance, imported registration, reflection row, or launch entry. Empty
/// admissions (every standalone entry) stay fail-closed.
#[derive(Debug, Clone, Default)]
pub struct PackageKernelTemplateAdmissions {
    /// This unit's package export surface: exported name → visibility tier.
    exports: BTreeMap<String, crate::semantic::passes::visibility_annotation::VisibilityTier>,
}

impl PackageKernelTemplateAdmissions {
    /// Wrap a package export surface as the per-unit admission input.
    #[must_use]
    pub fn from_export_surface(
        exports: &BTreeMap<String, crate::semantic::passes::visibility_annotation::VisibilityTier>,
    ) -> Self {
        Self { exports: exports.clone() }
    }

    /// A name is admissible only as a `@ publica`-tier package export;
    /// `@ interna` and `@ privata` stay fail-closed.
    fn admits_template(&self, name: &str) -> bool {
        matches!(
            self.exports.get(name),
            Some(crate::semantic::passes::visibility_annotation::VisibilityTier::Publica)
        )
    }
}

/// Run the frontend pipeline without emitting target code.
///
/// This is the analysis-only contract for compiler internals that need the
/// typed HIR and diagnostics but not a backend artifact. It performs the same
/// lex, parse, target-policy, CLI, and semantic phases as [`compile`].
/// Standalone analysis carries no package admission facts, so the
/// orphaned-annotation boundary stays fail-closed; package-aware
/// callers use [`analyze_source_with_kernel_template_admissions`].
///
/// # Errors
///
/// Returns diagnostic errors when lexing, parsing, target-policy checks,
/// CLI analysis, or semantic analysis fail. Internal phase-contract violations
/// are also reported as diagnostics.
pub fn analyze_source(
    session: &Session,
    name: &str,
    source: &str,
    request: AnalyzeRequest,
) -> Result<AnalyzedModule, Vec<Diagnostic>> {
    analyze_source_with_kernel_template_admissions(
        session,
        name,
        source,
        request,
        &PackageKernelTemplateAdmissions::default(),
    )
}

/// [`analyze_source`] with explicit package kernel-template admissions.
///
/// Identical pipeline; the admissions only widen the orphaned-annotation
/// boundary for exported size-generic `@ kernel` templates (PKG-ADM), so a
/// package API with no same-unit caller is not mistaken for an orphan.
/// The admission is demand-only and produces no concrete instantiation.
///
/// # Errors
///
/// Returns diagnostic errors when lexing, parsing, target-policy checks,
/// CLI analysis, or semantic analysis fail. Internal phase-contract violations
/// are also reported as diagnostics.
pub fn analyze_source_with_kernel_template_admissions(
    session: &Session,
    name: &str,
    source: &str,
    request: AnalyzeRequest,
    admissions: &PackageKernelTemplateAdmissions,
) -> Result<AnalyzedModule, Vec<Diagnostic>> {
    let peeled = peel_raw_source(name, source).map_err(|error| vec![source_load_diagnostic(name, &error)])?;
    let body_byte_offset = peeled.body_byte_offset;
    // Reader-locale suggestions wait here until the analysis result is known:
    // an edit-distance guess is shown only where an error sits at its span.
    let mut locale_candidates = Vec::new();
    let result = analyze_source_with_kernel_template_admissions_unadjusted(
        session,
        name,
        &peeled,
        request,
        admissions,
        &mut locale_candidates,
    );
    match result {
        Ok(mut analysis) => {
            push_filtered_locale_suggestions(&mut analysis.diagnostics, locale_candidates);
            adjust_diagnostic_lines_for_body(&mut analysis.diagnostics, source, body_byte_offset);
            Ok(analysis)
        }
        Err(mut diagnostics) => {
            push_filtered_locale_suggestions(&mut diagnostics, locale_candidates);
            note_other_locale_reading(session, name, &peeled, &mut diagnostics);
            adjust_diagnostic_lines_for_body(&mut diagnostics, source, body_byte_offset);
            Err(diagnostics)
        }
    }
}

/// Append the LOCALE002 candidates the shared suggestion filter allows.
fn push_filtered_locale_suggestions(diagnostics: &mut Vec<Diagnostic>, candidates: Vec<Diagnostic>) {
    let kept = Diagnostic::filter_locale_suggestions(candidates, diagnostics);
    diagnostics.extend(kept);
}

/// The product default code locale id: the reading a source gets when nothing declares one.
const DEFAULT_CODE_LOCALE_ID: &str = "en";

/// Reader locales probed when a source that declares none fails to check or analyze.
const DETECTABLE_LOCALE_IDS: [&str; 8] = ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];

/// Distinct declared keyword and type spellings the pack claims in a lexed source.
fn declared_surface_hits(lex_result: &lexer::LexResult) -> usize {
    lex_result.reader_keyword_latin_keys.len() + lex_result.reader_type_latin_keys.len()
}

/// The built-in locale whose keywords match `source` better than `baseline`,
/// the pack the source was read with. A missing baseline falls back to the
/// configured no-locale default ([`default_code_pack`]).
fn detect_other_locale(config: &Config, source: &str, baseline: Option<&LocalePack>) -> Option<&'static str> {
    // Advisory only: when the default pack cannot load, the load error itself
    // is reported by the read path, so no second-locale hint is offered.
    let baseline_pack = match baseline {
        Some(pack) => pack.clone(),
        None => default_code_pack(config).ok()?,
    };
    let baseline_hits = declared_surface_hits(&lexer::lex_with_locale_pack(source, &baseline_pack));
    DETECTABLE_LOCALE_IDS
        .iter()
        .filter_map(|id| {
            let pack = resolve_frontmatter_locale_pack(config, id).ok()?;
            let hits = declared_surface_hits(&lexer::lex_with_locale_pack(source, &pack));
            (hits > baseline_hits).then_some((hits, *id))
        })
        .max_by_key(|(hits, _)| *hits)
        .map(|(_, id)| id)
}

fn source_reads_as_other_locale_diagnostic(
    config: &Config,
    name: &str,
    body: &str,
    baseline: Option<&LocalePack>,
) -> Option<Diagnostic> {
    let detected = detect_other_locale(config, body, baseline)?;
    Some(
        Diagnostic::warning(format!("source has no declared locale and its keywords read as '{detected}'"))
            .with_phase(DiagnosticPhase::Tool)
            .with_code("LOCALE002")
            .with_args(vec![
                DiagnosticArg::new("issue", "source_reads_as_other_locale"),
                DiagnosticArg::new("detected", detected),
            ])
            .with_file(name.to_owned()),
    )
}

/// Advisory for a source read as Latin (no reader pack) that reads better as
/// another built-in locale. The caller decides whether a locale was declared.
pub(crate) fn latin_source_reads_as_other_locale(config: &Config, name: &str, body: &str) -> Option<Diagnostic> {
    source_reads_as_other_locale_diagnostic(config, name, body, None)
}

/// Advisory for a source read with the product-default `en` pack that reads
/// better as another built-in locale; never selects a locale.
///
/// WHY: a resolved pack reaches the driver the same way whether it is the
/// default or was declared by a manifest, `--locale`, or the source, so only
/// the default `en` reading is second-guessed. The caller decides whether the
/// source declared a locale itself.
#[must_use]
pub fn source_reads_as_other_locale(
    config: &Config,
    name: &str,
    body: &str,
    baseline: &LocalePack,
) -> Option<Diagnostic> {
    if baseline.metadata.id != DEFAULT_CODE_LOCALE_ID {
        return None;
    }
    source_reads_as_other_locale_diagnostic(config, name, body, Some(baseline))
}

/// Put the detected-locale advisory ahead of the errors of a failed analysis.
///
/// WHY: a wrong-locale read hides behind unknown-identifier errors. The note
/// is advisory only; it never changes the locale the source was read in.
fn note_other_locale_reading(session: &Session, name: &str, peeled: &PeeledSource, diagnostics: &mut Vec<Diagnostic>) {
    // A pack resolved from a source declaration, a package manifest, or
    // `--locale` is a caller decision, never a guess, so the failed-analysis
    // advisory does not apply. Package analysis hands the per-file session a
    // body with the frontmatter already stripped, so the declaration check
    // below cannot see it; the explicit flag still can.
    if session.config.locale_pack_explicit {
        return;
    }
    if peeled
        .frontmatter
        .as_ref()
        .and_then(FileFrontmatter::locale_result)
        .is_some()
    {
        return;
    }
    let Some(baseline) = session.config.locale_pack.as_ref() else {
        return;
    };
    if let Some(note) = source_reads_as_other_locale(&session.config, name, peeled.body, baseline) {
        diagnostics.insert(0, note);
    }
}

/// Translate diagnostics whose spans were calculated against a peeled body
/// back to physical source lines in the original document.
pub fn adjust_diagnostic_lines_for_body(diagnostics: &mut [Diagnostic], raw_source: &str, body_byte_offset: u32) {
    let Some(prefix) = raw_source.get(..body_byte_offset as usize) else {
        return;
    };
    let body_line_offset = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    if body_line_offset == 0 {
        return;
    }
    for diagnostic in diagnostics {
        if let Some(line) = diagnostic.line.as_mut() {
            *line = line.saturating_add(body_line_offset);
        }
    }
}

fn analyze_source_with_kernel_template_admissions_unadjusted(
    session: &Session,
    name: &str,
    peeled: &PeeledSource,
    request: AnalyzeRequest,
    admissions: &PackageKernelTemplateAdmissions,
    locale_candidates: &mut Vec<Diagnostic>,
) -> Result<AnalyzedModule, Vec<Diagnostic>> {
    let AnalyzeRequest { cli_program: cli_program_override, contract } = request;
    let mut diagnostics = Vec::new();

    let Ok(ParsedSourceBody { body, mut program, mut interner, lex_pack }) =
        parse_frontend_source_deferring_locale(session, name, peeled, &mut diagnostics, locale_candidates)
    else {
        return Err(diagnostics);
    };
    crate::program::apply_default_import_names(&mut program, &mut interner, &contract.default_import_names);

    collect_target_policy_diagnostics(session.config.target, &program, name, &interner, &mut diagnostics);
    // Recovered-parse continuation: parse-phase diagnostics must not stop the
    // pipeline before semantic analysis — the recovered items' semantic errors
    // co-report with them. Target-policy (and below, CLI) errors still stop.
    if has_non_parse_diagnostic_errors(&diagnostics) {
        return Err(diagnostics);
    }

    let Ok(cli_program) =
        analyze_cli_program_for_source(cli_program_override, &program, &mut interner, name, body, &mut diagnostics)
    else {
        return Err(diagnostics);
    };

    let ModuleImportContract {
        namespace_exports,
        file_interfaces,
        namespace_seams,
        namespace_declarations,
        default_import_names: _,
    } = contract;
    let insere = insere::resolve_insere_files(&program, &interner, name);
    // Read the file's display words before the pack moves into the analysis.
    let display_tokens = lex_pack.as_ref().map_or_else(
        crate::runtime_contract::display_tokens::DisplayTokens::latin,
        crate::runtime_contract::display_tokens_from_pack,
    );

    let Ok(semantic_result) = analyze_semantic_program(SemanticAnalysisInput {
        session,
        program: &program,
        interner: &mut interner,
        cli_program: cli_program.as_ref(),
        namespace_exports,
        namespace_seams,
        namespace_declarations,
        file_interfaces,
        name,
        body,
        diagnostics: &mut diagnostics,
        insere_files: insere.files,
        lex_pack,
    }) else {
        return Err(diagnostics);
    };

    let rust_omit = collect_lane_policy_diagnostics(
        &semantic_result.radix_lanes,
        semantic_result.hir.as_ref(),
        session.config.target,
        name,
        &mut diagnostics,
        cli_program.as_ref(),
    );
    if has_diagnostic_errors(&diagnostics) {
        return Err(diagnostics);
    }

    let mut unit =
        analyzed_unit_from_semantic(interner, cli_program, semantic_result, display_tokens, name, &mut diagnostics)
            .map_err(|()| diagnostics)?;
    unit.embedded_inputs = insere.inputs;
    // Target qualification (D8.5): the build target's import binding, float
    // folds and `praefixum` evaluation, so every backend receives literals.
    // Faber re-emission and the FHIR artifact keep the authored form.
    if let Err(qualify_errors) = qualify::qualify_for_target(&mut unit, &session.config, name, body) {
        unit.diagnostics.extend(qualify_errors);
        return Err(unit.diagnostics);
    }
    if !rust_omit.is_empty() {
        unit.hir
            .items
            .retain(|item| !rust_omit.contains(&item.def_id));
    }

    // Rust-only borrow/lifetime checks are target qualification, not base analysis.
    if session.config.target == Target::HirRust
        && let Err(borrow_errors) = semantic::qualify_rust_borrow(&unit.hir, &unit.resolver, &unit.types)
    {
        for err in &borrow_errors {
            unit.diagnostics
                .push(Diagnostic::from_semantic_error(name, body, err));
        }
        if unit.diagnostics.iter().any(Diagnostic::is_error) {
            return Err(unit.diagnostics);
        }
    }

    // DFV2-2: orphaned-annotation admission — fail closed at check stage, with
    // the named issue leading the diagnostic list. PKG-ADM: package-supplied
    // template admissions are the one package-mode widening of that boundary.
    let orphan_diagnostics = nucleum_orphan_diagnostics(&unit, name, body, admissions);
    if !orphan_diagnostics.is_empty() {
        unit.diagnostics.splice(0..0, orphan_diagnostics);
        return Err(unit.diagnostics);
    }

    Ok(unit)
}

/// Resolve a frontmatter `locale` id to a `LocalePack`.
///
/// Pack sources in order: the configured language-pack root's own
/// `<id>/pack.toml` (dev checkouts, planted test roots), then the
/// config's product-supplied loader ([`Config::locale_pack_loader`], faber's
/// embedded reader packs), so an installed binary resolves a declared locale
/// with no checkout beside it. With no loader installed the root remains the
/// only source and keeps its `no_root` / `load_failed` diagnostics.
///
/// # Errors
///
/// Returns a `Diagnostic` when neither source can load the pack.
pub(crate) fn resolve_frontmatter_locale_pack(config: &Config, locale_id: &str) -> Result<LocalePack, Box<Diagnostic>> {
    // Reject empty locale ids
    if locale_id.is_empty() {
        return Err(Box::new(
            Diagnostic::error("frontmatter declares an empty locale")
                .with_phase(DiagnosticPhase::Lex)
                .with_code(locale_unknown_spec().code)
                .with_arg("locale_id", locale_id)
                .with_arg("issue", "invalid_id"),
        ));
    }

    // Reject locale ids that look like paths: absolute, relative traversal, or
    // containing path separators.  PathBuf::join replaces the base when the
    // appended component is absolute, so an id like "/etc" would resolve to
    // /etc/pack.toml instead of <stdlib>/reader/etc/pack.toml.
    if locale_id.contains('/') || locale_id.contains('\\') || locale_id == "." || locale_id == ".." {
        return Err(Box::new(
            Diagnostic::error(format!(
                "frontmatter locale '{locale_id}' is not a valid locale identifier; locale ids must be simple directory names"
            ))
            .with_phase(DiagnosticPhase::Lex)
            .with_code(locale_unknown_spec().code)
            .with_arg("locale_id", locale_id)
            .with_arg("issue", "invalid_id"),
        ));
    }

    let root = config.locale_root.as_deref();

    // The root serves the id only when its pack file is present; a root
    // without the file falls through to the loader (or the root's own
    // load-failed diagnostic when no loader is installed).
    if let Some(root) = root.filter(|root| root.join(locale_id).join("pack.toml").is_file()) {
        return load_frontmatter_pack_from_root(root, locale_id);
    }

    if let Some(loader) = config.locale_pack_loader.as_ref() {
        return loader
            .load(locale_id)
            .map_err(|detail| frontmatter_locale_load_failed(locale_id, detail));
    }

    let Some(root) = root else {
        return Err(Box::new(
            Diagnostic::error(format!(
                "frontmatter declares locale '{locale_id}' but no language-pack root is configured"
            ))
            .with_phase(DiagnosticPhase::Lex)
            .with_code(locale_unknown_spec().code)
            .with_arg("locale_id", locale_id)
            .with_arg("issue", "no_root"),
        ));
    };
    load_frontmatter_pack_from_root(root, locale_id)
}

/// Load `<root>/<locale_id>/pack.toml`, with the root's containment check.
///
/// # Errors
///
/// Returns a `Diagnostic` when the resolved path escapes the root after
/// symlink resolution or the pack cannot be loaded.
fn load_frontmatter_pack_from_root(root: &std::path::Path, locale_id: &str) -> Result<LocalePack, Box<Diagnostic>> {
    let pack_path = root.join(locale_id).join("pack.toml");

    // Defense-in-depth containment check: verify the resolved path stays within
    // the language-pack root after symlink resolution. Input filtering already
    // blocks '/' and '..', so the primary remaining escape is a symlink inside
    // the locale directory.
    if let Ok(canonical_root) = std::fs::canonicalize(root)
        && let Ok(canonical_pack_path) = std::fs::canonicalize(&pack_path)
        && !canonical_pack_path.starts_with(&canonical_root)
    {
        return Err(Box::new(
            Diagnostic::error(format!(
                "frontmatter locale '{locale_id}' resolves outside the language-pack root"
            ))
            .with_phase(DiagnosticPhase::Lex)
            .with_code(locale_unknown_spec().code)
            .with_arg("locale_id", locale_id)
            .with_arg("issue", "outside_root"),
        ));
    }

    LocalePack::from_toml_path(&pack_path).map_err(|err| frontmatter_locale_load_failed(locale_id, err.to_string()))
}

/// The `LOCALE003` `load_failed` diagnostic for a frontmatter locale whose
/// pack could not be loaded from any source.
fn frontmatter_locale_load_failed(locale_id: &str, detail: String) -> Box<Diagnostic> {
    Box::new(
        Diagnostic::error(format!("unknown reader locale '{locale_id}' in frontmatter: {detail}"))
            .with_phase(DiagnosticPhase::Lex)
            .with_code(locale_unknown_spec().code)
            .with_arg("locale_id", locale_id)
            .with_arg("issue", "load_failed")
            .with_arg("detail", detail),
    )
}

/// The reader pack a source with no declared locale is read with: `en`.
///
/// This is the single place the no-locale default is decided. The pack comes
/// from the configured language-pack root like any source-declared locale.
///
/// # Errors
///
/// Returns a `Diagnostic` when the `en` pack cannot be loaded. A pack that
/// fails to load is an error, never a silent fall back to the Latin surface.
pub fn default_code_pack(config: &Config) -> Result<LocalePack, Box<Diagnostic>> {
    resolve_frontmatter_locale_pack(config, DEFAULT_CODE_LOCALE_ID)
}

struct ParsedSourceBody<'a> {
    body: &'a str,
    program: Program,
    interner: Interner,
    /// The reader pack used for lexing this source (frontmatter locale > session config).
    /// Threaded to the semantic analyzer so type spellings match the input locale.
    lex_pack: Option<LocalePack>,
}

/// Resolve the reader pack for a peeled source body: frontmatter locale >
/// session config > no-locale default ([`default_code_pack`]).
///
/// Shared by the analysis front end and the forma author path so both honor a
/// source's `+++ locale = "…" +++` declaration (e.g. the English exempla
/// corpus). Pushes diagnostics and returns `Err(())` when the frontmatter
/// locale key is malformed or the declared or default pack cannot be loaded.
pub(crate) fn resolve_peeled_lex_pack(
    session: &Session,
    peeled: &PeeledSource,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<LocalePack>, ()> {
    // An explicitly caller-selected pack (CLI `--locale`) is authoritative and
    // wins over a source-declared frontmatter locale.
    if session.config.locale_pack_explicit {
        return Ok(session.config.locale_pack.clone());
    }
    match peeled
        .frontmatter
        .as_ref()
        .and_then(FileFrontmatter::locale_result)
    {
        Some(Err(())) => {
            diagnostics.push(
                Diagnostic::error("frontmatter 'locale' key must be a string")
                    .with_phase(DiagnosticPhase::Lex)
                    .with_code(locale_unknown_spec().code)
                    .with_arg("issue", "not_a_string"),
            );
            Err(())
        }
        Some(Ok(locale_id)) => match resolve_frontmatter_locale_pack(&session.config, locale_id) {
            Ok(pack) => Ok(Some(pack)),
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                Err(())
            }
        },
        None => match session.config.locale_pack.clone() {
            Some(pack) => Ok(Some(pack)),
            None => match default_code_pack(&session.config) {
                Ok(pack) => Ok(Some(pack)),
                Err(diagnostic) => {
                    diagnostics.push(*diagnostic);
                    Err(())
                }
            },
        },
    }
}

fn parse_frontend_source<'a>(
    session: &Session,
    name: &str,
    source: &'a str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<ParsedSourceBody<'a>, ()> {
    let peeled = match peel_raw_source(name, source) {
        Ok(peeled) => peeled,
        Err(error) => {
            diagnostics.push(source_load_diagnostic(name, &error));
            return Err(());
        }
    };
    let mut locale_candidates = Vec::new();
    let parsed = parse_frontend_source_deferring_locale(session, name, &peeled, diagnostics, &mut locale_candidates);
    push_filtered_locale_suggestions(diagnostics, locale_candidates);
    parsed
}

/// Parse a source body, leaving the lexer's LOCALE002 candidates in
/// `locale_candidates` for the caller to filter once its error set is known.
fn parse_frontend_source_deferring_locale<'a>(
    session: &Session,
    name: &str,
    peeled: &PeeledSource<'a>,
    diagnostics: &mut Vec<Diagnostic>,
    locale_candidates: &mut Vec<Diagnostic>,
) -> Result<ParsedSourceBody<'a>, ()> {
    let body = peeled.body;

    // Resolve reader pack: frontmatter locale > session config > Latin default.
    let Ok(lex_pack) = resolve_peeled_lex_pack(session, peeled, diagnostics) else {
        return Err(());
    };

    let lex_result = match lex_pack.as_ref() {
        Some(pack) => lexer::lex_with_locale_pack(body, pack),
        None => lexer::lex(body),
    };
    locale_candidates.extend(
        lex_result
            .locale_suggestions
            .iter()
            .map(|suggestion| Diagnostic::from_locale_suggestion(name, body, suggestion)),
    );
    diagnostics.extend(
        lex_result
            .mixed_script_advisories
            .iter()
            .map(|advisory| Diagnostic::from_mixed_script_advisory(name, body, advisory)),
    );
    if !lex_result.success() {
        for err in &lex_result.errors {
            diagnostics.push(Diagnostic::from_lex_error(name, body, err));
        }
        return Err(());
    }

    let parse_result = parser::parse_with_options(
        lex_result,
        parser::ParseOptions { allow_bodyless_functions: session.config.allow_bodyless_functions },
    );
    for err in &parse_result.errors {
        diagnostics.push(Diagnostic::from_parse_error(name, body, err));
    }
    let had_parse_errors = !parse_result.success();
    let parse_error_spans: Vec<Span> = parse_result.errors.iter().map(|err| err.span).collect();
    let parser::ParseResult { program, interner, .. } = parse_result;
    let Some(program) = program else {
        diagnostics.push(
            Diagnostic::error(MISSING_PARSE_PROGRAM_MESSAGE)
                .with_phase(DiagnosticPhase::Parse)
                .with_code("INTERNAL001")
                .with_args(vec![DiagnosticArg::new("issue", "parse_result_missing_program")])
                .with_file(name.to_owned()),
        );
        return Err(());
    };
    // Recovered-parse continuation (em-u3): when the parser recovered a partial
    // AST, keep the successfully-parsed top-level items for semantic analysis
    // so parse diagnostics and the recovered items' semantic diagnostics
    // co-report instead of the parse error masking the rest of the module.
    // Items whose span contains a parse-error span are dropped — their AST is
    // cascade-corrupted (e.g. `make_beta` survives with a partial body) and
    // analyzing them would report duplicate/cascade diagnostics.
    let program = if had_parse_errors {
        recovered_program_items(program, &parse_error_spans)
    } else {
        program
    };

    Ok(ParsedSourceBody { body, program, interner, lex_pack })
}

/// Keep only the recovered program's successfully-parsed top-level items.
///
/// A recovered parse (`success() == false`, `program.is_some()`) preserves
/// statements whose parse succeeded plus whatever survived the error recovery.
/// A top-level item whose span contains a parse-error span is cascade-
/// corrupted (its body/signature may hold partial constructs that never
/// existed in source), so semantic analysis of it would emit spurious
/// diagnostics. Dropping those items lets the analysis entry run the clean
/// remainder — parse errors co-report with the clean items' semantic
/// diagnostics (the em-u3 continuation).
pub(crate) fn recovered_program_items(mut program: Program, parse_error_spans: &[Span]) -> Program {
    program.statements.retain(|stmt| {
        !parse_error_spans
            .iter()
            .any(|err| err.start >= stmt.span.start && err.end <= stmt.span.end)
    });
    program
}

fn collect_target_policy_diagnostics(
    target: Target,
    program: &Program,
    name: &str,
    interner: &Interner,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match target {
        Target::HirGo => diagnostics.extend(target_policy::collect_go_unsupported_errors(program, name, interner)),
        Target::HirRust => {
            diagnostics.extend(target_policy::collect_rust_unsupported_errors(program, name));
        }
        // Remaining targets have no syntax-level target-policy checks yet.
        // Listed explicitly so new Target variants force consideration here.
        Target::HirFaber
        | Target::HirFhir
        | Target::MirStepper
        | Target::MirFmirBinary
        | Target::MirFmir
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirClif
        | Target::MirAmd
        | Target::MirMetal
        | Target::MirScena
        | Target::MirSexp
        | Target::MirEvm
        | Target::MirBasic
        | Target::HirSwift
        | Target::HirPython
        | Target::HirHaskell
        | Target::HirPascal
        | Target::HirCobol
        | Target::HirFortran
        | Target::HirOdin
        | Target::HirC
        | Target::HirTypeScript
        | Target::MirWasm
        | Target::MirWgsl => {}
    }
}

fn analyze_cli_program_for_source(
    cli_program_override: Option<crate::cli::CliProgram>,
    program: &Program,
    interner: &mut Interner,
    name: &str,
    body: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<crate::cli::CliProgram>, ()> {
    let cli_program = if let Some(cli_program) = cli_program_override {
        Some(retarget_cli_program_symbols(cli_program, interner))
    } else {
        let cli_analysis = crate::cli::analyze(program, interner);
        for err in &cli_analysis.errors {
            diagnostics.push(Diagnostic::from_semantic_error(name, body, err));
        }

        cli_analysis.program
    };

    // Recovered-parse continuation: parse-phase diagnostics already collected
    // (the recovered program's PARSE errors) must not stop analysis here;
    // CLI-analysis errors still do.
    if has_non_parse_diagnostic_errors(diagnostics) {
        return Err(());
    }

    Ok(cli_program)
}

struct SemanticAnalysisInput<'a> {
    session: &'a Session,
    program: &'a Program,
    interner: &'a mut Interner,
    cli_program: Option<&'a crate::cli::CliProgram>,
    namespace_exports:
        BTreeMap<String, BTreeMap<String, crate::semantic::passes::visibility_annotation::VisibilityTier>>,
    namespace_seams: BTreeMap<String, bool>,
    namespace_declarations:
        BTreeMap<String, BTreeMap<String, crate::semantic::passes::visibility_annotation::VisibilityTier>>,
    file_interfaces: BTreeMap<String, crate::file_interface::FileInterface>,
    name: &'a str,
    body: &'a str,
    diagnostics: &'a mut Vec<Diagnostic>,
    /// Driver-resolved build-time embed files (`insere`, D8.10).
    insere_files: semantic::insere::InsereFiles,
    /// The reader pack used for lexing (frontmatter locale > session config).
    /// Used for type-spelling registration and HIR lowering so they match the
    /// input locale, not the output locale.
    lex_pack: Option<LocalePack>,
}

fn analyze_semantic_program(input: SemanticAnalysisInput<'_>) -> Result<semantic::SemanticResult, ()> {
    let SemanticAnalysisInput {
        session,
        program,
        interner,
        cli_program,
        namespace_exports,
        namespace_seams,
        namespace_declarations,
        file_interfaces,
        name,
        body,
        diagnostics,
        insere_files,
        lex_pack,
    } = input;

    // Shared analysis without Rust-only borrow. `target` is still recorded so
    // existing target-tagged checks (e.g. `@ nondum rs`) keep their authority
    // until G2/G5 move them fully into TargetQualification. Borrow analysis is
    // applied after the analyzed-application snapshot via `qualify_rust_borrow`.
    let mut pass_config = PassConfig::base();
    pass_config.target = session.config.target;
    pass_config.namespace_exports = namespace_exports;
    pass_config.namespace_seams = namespace_seams;
    pass_config.namespace_declarations = namespace_declarations;
    pass_config.file_interfaces = file_interfaces;
    pass_config.locale_pack = lex_pack;
    pass_config.import_path_templates = session.config.import_path_templates.clone();
    pass_config.complexity_budget = session.config.complexity_budget;
    pass_config.kernel_complexity_budget = session.config.kernel_complexity_budget;
    pass_config.deny_kernel_tiers = session.config.deny_kernel_tiers;
    pass_config.fold_literal_floats = !session.config.keep_authored_constants;
    pass_config.insere_files = insere_files;
    pass_config.source_text = Some(std::sync::Arc::from(body));
    let semantic_result = semantic::analyze_with_cli(program, &pass_config, interner, cli_program);

    for err in &semantic_result.diagnostics {
        diagnostics.push(Diagnostic::from_semantic_error(name, body, err));
    }

    if !semantic_result.success() {
        return Err(());
    }

    Ok(semantic_result)
}

fn collect_lane_policy_diagnostics(
    lanes: &RadixLaneMetadata,
    hir: Option<&HirModule>,
    target: Target,
    name: &str,
    diagnostics: &mut Vec<Diagnostic>,
    cli_program: Option<&crate::cli::CliProgram>,
) -> handroll::FxHashSet<DefId> {
    // CLI codegen on HIR-direct targets (Rust, Go) never needs MIR lowering.
    // Skip lane policy checks that would reject MIR/AIR lane annotations
    // when the CLI codegen path is active (F-109). Do not expand this skip
    // to libraries — omit is a separate HirRust library path.
    if cli_program.is_some() && matches!(target, Target::HirRust | Target::HirGo) {
        return handroll::FxHashSet::default();
    }
    // Faber-surface emit (canonical re-emit, `faber format`, `radix emit -t
    // faber`) preserves lane annotations as annotations and never lowers to
    // MIR, so the MIR-backed-target policy (a codegen gate) does not apply.
    if matches!(target, Target::HirFaber) {
        return handroll::FxHashSet::default();
    }
    // HirRust library / package units omit AIR-lane items, companions, and
    // same-unit wrappers that reach them instead of TARGETLANE001. Single-file
    // AIR-as-rust and a kept rust-emitted caller of an omitted item still reject.
    if target == Target::HirRust
        && let Some(hir) = hir
    {
        let omit = rust_library_omit_defs(lanes, hir);
        if rust_library_omit_applies(hir, &omit) {
            if rust_kept_item_reaches_omit(hir, &omit) {
                if let Some(diag) = radix_lane_target_error(RadixLane::Air, target, name) {
                    diagnostics.push(diag);
                }
                return handroll::FxHashSet::default();
            }
            return omit;
        }
    }
    diagnostics.extend(collect_radix_lane_target_errors(lanes, target, name));
    handroll::FxHashSet::default()
}

/// Strip AIR-lane items, `@ radix backward` companions, and same-unit rust
/// wrappers that reach them. Used again at rust crate assembly so emit cannot
/// keep an item analysis already classified as omitted.
pub fn omit_rust_library_air_items(hir: &mut HirModule, lanes: &RadixLaneMetadata) {
    let omit = rust_library_omit_defs(lanes, hir);
    if omit.is_empty() {
        return;
    }
    hir.items.retain(|item| !omit.contains(&item.def_id));
}

fn rust_library_omit_defs(lanes: &RadixLaneMetadata, hir: &HirModule) -> handroll::FxHashSet<DefId> {
    let mut omit = handroll::FxHashSet::default();
    for (def_id, lane) in lanes.iter() {
        if lane == RadixLane::Air {
            omit.insert(def_id);
        }
    }
    for (_, backward) in lanes.iter_backward() {
        omit.insert(backward.companion_def_id);
    }
    if omit.is_empty() {
        return omit;
    }

    let mut callees_by_fn = handroll::FxHashMap::default();
    for item in &hir.items {
        let HirItemKind::Function(function) = &item.kind else {
            continue;
        };
        callees_by_fn.insert(item.def_id, function_path_defs(function));
    }

    let mut grew = true;
    while grew {
        grew = false;
        for (def_id, callees) in &callees_by_fn {
            if omit.contains(def_id) {
                continue;
            }
            if callees.iter().any(|callee| omit.contains(callee)) {
                omit.insert(*def_id);
                grew = true;
            }
        }
    }
    omit
}

fn rust_library_omit_applies(hir: &HirModule, omit: &handroll::FxHashSet<DefId>) -> bool {
    if omit.is_empty() || hir.entry.is_some() {
        return false;
    }
    hir.items
        .iter()
        .any(|item| !omit.contains(&item.def_id) && !matches!(item.kind, HirItemKind::Import(_)))
}

fn rust_kept_item_reaches_omit(hir: &HirModule, omit: &handroll::FxHashSet<DefId>) -> bool {
    hir.items.iter().any(|item| {
        if omit.contains(&item.def_id) {
            return false;
        }
        let HirItemKind::Function(function) = &item.kind else {
            return false;
        };
        function_path_defs(function)
            .iter()
            .any(|callee| omit.contains(callee))
    })
}

/// DFV2-2: named check-stage issue for an orphaned `@ nucleum` annotation.
const NUCLEUM_ORPHAN_ISSUE: &str = "nucleum_orphaned_annotation";

/// Canonical annotation family behind `@ nucleum` / `@ kernel`.
const NUCLEUM_FAMILY: &str = "nucleum";

/// `DefIds` that appear as the direct callee of a call expression anywhere in
/// the unit — the check-stage stand-in for "a concrete instantiation exists".
fn module_call_targets(module: &HirModule) -> handroll::FxHashSet<DefId> {
    struct CallCollector {
        callees: handroll::FxHashSet<DefId>,
    }

    impl HirVisitor for CallCollector {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Call(callee, ..) = &expr.kind
                && let HirExpressionKind::Path(def_id) = callee.kind
            {
                self.callees.insert(def_id);
            }
            walk_expr(self, expr);
        }
    }

    let mut collector = CallCollector { callees: handroll::FxHashSet::default() };
    collector.visit_program(module);
    collector.callees
}

/// DFV2-2 orphaned-annotation admission: an annotated `@ nucleum` function
/// that yields zero concrete device entries in this unit — a size-generic
/// with no same-unit call site — fails closed here, at check stage, instead
/// of surfacing the generic zero-kernel codegen message later. Distinct from
/// the v1 `nucleum_fragment_effect` effect diagnostics; never a warning.
///
/// PKG-ADM package widening: an exported `@ publica` size-generic template
/// admitted through package analysis is a package API, not an orphan. The
/// admission only skips this diagnostic; it fabricates no concrete entry.
fn nucleum_orphan_diagnostics(
    unit: &AnalyzedModule,
    name: &str,
    source: &str,
    admissions: &PackageKernelTemplateAdmissions,
) -> Vec<Diagnostic> {
    let callees = module_call_targets(&unit.hir);
    let mut diagnostics = Vec::new();
    // Package exports are top-level statements; struct methods never are, so
    // a method sharing an exported name must not inherit its admission.
    let annotated = unit.hir.items.iter().flat_map(|item| match &item.kind {
        HirItemKind::Function(function) => {
            vec![(true, item.def_id, function.as_ref(), item.annotations.as_slice(), item.span)]
        }
        HirItemKind::Struct(strukt) => strukt
            .methods
            .iter()
            .map(|method| (false, method.def_id, &method.func, method.annotations.as_slice(), method.span))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    });
    for (is_top_level, def_id, function, annotations, definition_span) in annotated {
        // Signature-only declarations defer to the DFV2-3 import/linkage channel.
        if !function.is_kernel || function.body.is_none() || function.type_params.is_empty() {
            continue;
        }
        if callees.contains(&def_id) {
            continue;
        }
        let function_name = unit.interner.resolve(function.name);
        // PKG-ADM demand admission: publica-tier package export, top-level
        // function only. Unknown, `@ interna`, and `@ privata` callees stay
        // fail-closed below.
        if is_top_level && admissions.admits_template(function_name) {
            continue;
        }
        let annotation_span = annotations
            .iter()
            .find(|annotation| unit.interner.resolve(annotation.family) == NUCLEUM_FAMILY)
            .map_or(definition_span, |annotation| annotation.span);
        diagnostics.push(
            Diagnostic::error(format!(
                "@ nucleum function `{function_name}` is size-generic but has no same-unit caller \
                 and is not exported: the annotation is orphaned (no entry reaches it)"
            ))
            .with_phase(DiagnosticPhase::Analysis)
            .with_code("ADM001")
            .with_args(vec![
                DiagnosticArg::new("issue", NUCLEUM_ORPHAN_ISSUE),
                DiagnosticArg::new("function", function_name),
                DiagnosticArg::new("definition_start", definition_span.start.to_string()),
                DiagnosticArg::new("definition_end", definition_span.end.to_string()),
            ])
            .with_file(name.to_owned())
            .with_span(annotation_span)
            .with_source_context(name, source, annotation_span)
            .with_help(
                "call the kernel from this unit or mark it `@ public`; a kernel compiles once per \
                 signature and its size extents are supplied per launch (they may be run-time \
                 values), so no concrete size arguments are required",
            ),
        );
    }
    diagnostics
}

fn function_path_defs(function: &HirFunction) -> handroll::FxHashSet<DefId> {
    struct PathCollector {
        defs: handroll::FxHashSet<DefId>,
    }

    impl HirVisitor for PathCollector {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Path(def_id) = expr.kind {
                self.defs.insert(def_id);
            }
            walk_expr(self, expr);
        }
    }

    let Some(body) = &function.body else {
        return handroll::FxHashSet::default();
    };
    let mut collector = PathCollector { defs: handroll::FxHashSet::default() };
    collector.visit_block(body);
    collector.defs
}

fn analyzed_unit_from_semantic(
    interner: Interner,
    cli_program: Option<crate::cli::CliProgram>,
    semantic_result: semantic::SemanticResult,
    display_tokens: crate::runtime_contract::display_tokens::DisplayTokens,
    name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<AnalyzedModule, ()> {
    let Some(hir) = semantic_result.hir else {
        diagnostics.push(
            Diagnostic::error(MISSING_SEMANTIC_HIR_MESSAGE)
                .with_phase(DiagnosticPhase::Lower)
                .with_code("INTERNAL001")
                .with_args(vec![DiagnosticArg::new("issue", "semantic_result_missing_hir")])
                .with_file(name.to_owned()),
        );
        return Err(());
    };
    let Some(access_facts) = semantic_result.access_facts else {
        diagnostics.push(
            Diagnostic::error("semantic analysis did not produce access facts")
                .with_phase(DiagnosticPhase::Analysis)
                .with_code("INTERNAL001")
                .with_args(vec![DiagnosticArg::new("issue", "semantic_result_missing_access_facts")])
                .with_file(name.to_owned()),
        );
        return Err(());
    };

    let libraries = LibraryRegistry::default();
    let shape_obligations = semantic_result.shape_obligations.clone();
    let parts = semantic::AnalyzedApplicationParts {
        hir,
        types: semantic_result.types,
        resolver: semantic_result.resolver,
        annotation_contracts: semantic_result.annotation_contracts,
        libraries,
        radix_lanes: semantic_result.radix_lanes,
        gpu_builtins: semantic_result.gpu_builtins,
        tensor_edge_policies: semantic_result.tensor_edge_policies,
        kernel_requirements: semantic_result.kernel_requirements,
        access_facts,
        region_facts: semantic_result.region_facts,
        semantic_diagnostics: semantic_result.diagnostics,
    };

    let application = match semantic::AnalyzedApplication::build(parts, &interner) {
        Ok(application) => application,
        Err(err) => {
            diagnostics.push(
                Diagnostic::error(err.to_string())
                    .with_phase(DiagnosticPhase::Analysis)
                    .with_file(name.to_owned())
                    .with_args(vec![DiagnosticArg::new("issue", "analyzed_application_incomplete")]),
            );
            return Err(());
        }
    };

    let fields = application.into_driver_fields();
    #[cfg(feature = "mir-core")]
    let graphics_source = collect_graphics_source_facts(&fields.hir, &fields.types, &interner, name, diagnostics);
    Ok(AnalyzedModule {
        analysis_stamp: fields.stamp,
        interner,
        types: fields.types,
        shape_obligations,
        kernel_requirements: fields.kernel_requirements,
        tensor_edge_policies: fields.tensor_edge_policies,
        resolver: fields.resolver,
        cli_program,
        libraries: fields.libraries,
        annotation_contracts: fields.annotation_contracts,
        function_facts: fields.function_facts,
        resolved_uses: fields.resolved_uses,
        qualified_identities: fields.qualified_identities,
        provenance: fields.provenance,
        package_import_identities: None,
        provider_units: ProviderUnitView::default(),
        radix_lanes: fields.radix_lanes,
        gpu_builtins: fields.gpu_builtins,
        #[cfg(feature = "mir-core")]
        graphics_source,
        hir: fields.hir,
        presentation: semantic_result.presentation,
        embedded_inputs: Vec::new(),
        display_tokens,
        diagnostics: std::mem::take(diagnostics),
    })
}

/// A presentation for a unit with no trivia data.
///
/// Used by `from_loaded` before the artifact loader reconstructs the sidecar
/// (schema-v1 artifacts carry no presentation; U-4 switches the loader to
/// `from_loaded_with_presentation`). No real program-end boundary exists here,
/// so the sidecar is deliberately empty.
fn empty_presentation() -> HirPresentation {
    HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: HirModuleEnd { id: HirSourceAnchorId(0), span: crate::lexer::Span::default() },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        template_holes: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    }
}

#[cfg(feature = "mir-core")]
fn collect_graphics_source_facts(
    hir: &HirModule,
    types: &semantic::TypeTable,
    interner: &Interner,
    name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> GraphicsSourceFacts {
    let contract = shader_contract::collect_shader_contract_facts(hir, types, interner, name, diagnostics);
    GraphicsSourceFacts {
        vertex_entries: collect_vertex_entries(hir, types, interner),
        fragment_entries: collect_fragment_entries(hir, types, interner),
        shader_vertex_attributes: contract.vertex_attributes,
        shader_varyings: contract.varyings,
        shader_fragment_outputs: contract.fragment_outputs,
        shader_pipeline_states: contract.pipeline_states,
        shader_resource_bindings: contract.resource_bindings,
        bound_resource_bindings: contract.bound_resource_bindings,
    }
}

fn collect_vertex_entries(
    hir: &HirModule,
    types: &semantic::TypeTable,
    interner: &Interner,
) -> Vec<GraphicsVertexEntry> {
    let mut entries = Vec::new();
    for item in &hir.items {
        let crate::hir::HirItemKind::Function(function) = &item.kind else {
            continue;
        };
        let vertex_payload_count: usize = item
            .annotations
            .iter()
            .filter(|annotation| interner.resolve(annotation.family) == "vertex")
            .map(|annotation| annotation.fields.len() + annotation.raw_args.len())
            .sum();
        if item
            .annotations
            .iter()
            .any(|annotation| interner.resolve(annotation.family) == "vertex")
        {
            entries.push(GraphicsVertexEntry {
                def_id: item.def_id,
                entry_name: interner.resolve(function.name).to_owned(),
                annotation_payload_count: vertex_payload_count,
                param_count: function.params.len() + usize::from(function.cli_args.is_some()),
                params: stage_params(function, types, interner),
                returns_vacuum: function_returns_vacuum(function, types),
                has_error_type: function.err_ty.is_some(),
                body_shape: vertex_body_shape(function.body.as_ref()),
            });
        }
    }
    entries
}

fn stage_params(function: &HirFunction, types: &semantic::TypeTable, interner: &Interner) -> Vec<GraphicsStageParam> {
    function
        .params
        .iter()
        .map(|param| GraphicsStageParam {
            name: interner.resolve(param.name).to_owned(),
            type_def: match types.get(param.ty) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
        })
        .collect()
}

fn function_returns_vacuum(function: &HirFunction, types: &semantic::TypeTable) -> bool {
    matches!(
        function.ret_ty.map(|ty| types.get(ty)),
        Some(Type::Primitive(Primitive::Vacuum))
    )
}

fn collect_fragment_entries(
    hir: &HirModule,
    types: &semantic::TypeTable,
    interner: &Interner,
) -> Vec<GraphicsFragmentEntry> {
    let mut entries = Vec::new();
    for item in &hir.items {
        let crate::hir::HirItemKind::Function(function) = &item.kind else {
            continue;
        };
        if !item
            .annotations
            .iter()
            .any(|annotation| interner.resolve(annotation.family) == "fragment")
        {
            continue;
        }
        let fragment_payload_count: usize = item
            .annotations
            .iter()
            .filter(|annotation| interner.resolve(annotation.family) == "fragment")
            .map(|annotation| annotation.fields.len() + annotation.raw_args.len())
            .sum();
        entries.push(GraphicsFragmentEntry {
            def_id: item.def_id,
            entry_name: interner.resolve(function.name).to_owned(),
            annotation_payload_count: fragment_payload_count,
            param_count: function.params.len() + usize::from(function.cli_args.is_some()),
            params: stage_params(function, types, interner),
            returns_vacuum: function_returns_vacuum(function, types),
            has_error_type: function.err_ty.is_some(),
            body_shape: vertex_body_shape(function.body.as_ref()),
        });
    }
    entries
}

fn vertex_body_shape(body: Option<&HirBlock>) -> GraphicsVertexBodyShape {
    let Some(body) = body else {
        return GraphicsVertexBodyShape::Missing;
    };
    // Ignore compiler-injected synthetic locals (runtime_provided) when
    // deciding whether a source body is empty. HIR lowering injects shader
    // implicit inputs as synthetic locals; they do not make an empty source
    // body "authored."
    let has_user_statements = body
        .statements
        .iter()
        .any(|stmt| !matches!(&stmt.kind, crate::hir::HirStatementKind::Local(l) if l.runtime_provided));
    if has_user_statements || body.expr.is_some() || body.breakable.is_some() {
        return GraphicsVertexBodyShape::Unsupported;
    }
    GraphicsVertexBodyShape::EmptyBlock
}

fn has_diagnostic_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(Diagnostic::is_error)
}

/// Whether any *non-parse* error diagnostic is present.
///
/// The recovered-parse continuation (em-u3) lets parse diagnostics co-report
/// with the recovered items' semantic diagnostics, so the pre-semantic early
/// exits must not trip on parse-phase errors. Non-parse errors still stop the
/// pipeline as before.
fn has_non_parse_diagnostic_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diag| diag.is_error() && diag.phase != DiagnosticPhase::Parse)
}

fn collect_radix_lane_target_errors(lanes: &RadixLaneMetadata, target: Target, file: &str) -> Vec<Diagnostic> {
    lanes
        .iter()
        .filter_map(|(_, lane)| radix_lane_target_error(lane, target, file))
        .collect()
}

fn radix_lane_target_error(lane: RadixLane, target: Target, file: &str) -> Option<Diagnostic> {
    let target_name = target.as_str();
    let (message, issue, lane_name) = match (lane, target) {
        (
            RadixLane::Air,
            Target::HirRust | Target::HirFaber | Target::HirTypeScript | Target::HirGo | Target::HirSwift,
        ) => Some((
            format!(
                "@ radix lane \"air\" requires a MIR-backed target; target '{target_name}' is currently HIR-direct"
            ),
            "lane_requires_mir_backed_target",
            "air",
        )),
        (
            RadixLane::Mir,
            Target::HirRust | Target::HirFaber | Target::HirTypeScript | Target::HirGo | Target::HirSwift,
        ) => Some((
            format!(
                "@ radix lane \"mir\" requires a MIR-backed target; target '{target_name}' is currently HIR-direct"
            ),
            "lane_requires_mir_backed_target",
            "mir",
        )),
        _ => None,
    }?;

    Some(
        Diagnostic::error(message)
            .with_phase(DiagnosticPhase::Analysis)
            .with_code("TARGETLANE001")
            .with_args(vec![
                DiagnosticArg::new("issue", issue),
                DiagnosticArg::new("lane", lane_name),
                DiagnosticArg::new("target", target_name),
            ])
            .with_file(file.to_owned()),
    )
}

fn retarget_cli_program_symbols(
    mut cli_program: crate::cli::CliProgram,
    interner: &mut Interner,
) -> crate::cli::CliProgram {
    for option in cli_program
        .global_options
        .iter_mut()
        .chain(cli_program.options.iter_mut())
        .chain(
            cli_program
                .commands
                .iter_mut()
                .flat_map(|command| command.options.iter_mut()),
        )
    {
        option.binding_symbol = interner.intern(&option.binding);
    }
    for operand in cli_program
        .global_operands
        .iter_mut()
        .chain(cli_program.operands.iter_mut())
        .chain(
            cli_program
                .commands
                .iter_mut()
                .flat_map(|command| command.operands.iter_mut()),
        )
    {
        operand.binding_symbol = interner.intern(&operand.binding);
    }
    for command in &mut cli_program.commands {
        command.function_symbol = interner.intern(&command.function);
    }
    cli_program
}

#[cfg(test)]
#[cfg(feature = "mir-core")]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "cli_capability_test.rs"]
mod cli_capability_tests;

#[cfg(test)]
#[cfg(feature = "mir-core")]
#[path = "device_entry_gate_test.rs"]
mod device_entry_gate_tests;

#[cfg(test)]
#[cfg(feature = "mir-core")]
#[path = "device_orphan_admission_test.rs"]
mod device_orphan_admission_tests;

#[cfg(test)]
#[cfg(feature = "mir-llvm")]
#[path = "nvvm_device_emit_test.rs"]
mod nvvm_device_emit_tests;

#[cfg(test)]
#[cfg(feature = "mir-llvm")]
#[path = "rope_nvvm_reachability_test.rs"]
mod rope_nvvm_reachability_tests;

#[cfg(test)]
#[cfg(feature = "hir-faber")]
#[path = "locale_fanout_test.rs"]
mod locale_fanout_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "backward_integration_test.rs"]
mod backward_integration_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "generic_backward_integration_test.rs"]
mod generic_backward_integration_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "bias_broadcast_test.rs"]
mod bias_broadcast_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "gather_intrinsic_test.rs"]
mod gather_intrinsic_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "gt19_ad_acceptance_test.rs"]
mod gt19_ad_acceptance_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "rms_norm_intrinsic_test.rs"]
mod rms_norm_intrinsic_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "rope_norm_intrinsic_test.rs"]
mod rope_norm_intrinsic_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "rope_half_intrinsic_test.rs"]
mod rope_half_intrinsic_tests;

#[cfg(test)]
#[cfg(feature = "mir-metal")]
#[path = "rope_half_metal_reachability_test.rs"]
mod rope_half_metal_reachability_tests;

#[cfg(test)]
#[cfg(feature = "device")]
#[path = "causal_softmax_intrinsic_test.rs"]
mod causal_softmax_intrinsic_tests;

#[cfg(test)]
#[cfg(all(feature = "device", feature = "mir-metal"))]
#[path = "causal_softmax_metal_reachability_test.rs"]
mod causal_softmax_metal_reachability_tests;

#[cfg(test)]
#[cfg(feature = "mir-llvm")]
#[path = "causal_softmax_nvvm_reachability_test.rs"]
mod causal_softmax_nvvm_reachability_tests;

#[cfg(test)]
#[cfg(all(feature = "device", feature = "mir-metal"))]
#[path = "batched_matmul_metal_reachability_test.rs"]
mod batched_matmul_metal_reachability_tests;

#[cfg(test)]
#[cfg(feature = "mir-llvm")]
#[path = "batched_matmul_nvvm_reachability_test.rs"]
mod batched_matmul_nvvm_reachability_tests;

#[cfg(test)]
#[path = "default_code_locale_test.rs"]
mod default_code_locale_tests;

#[cfg(test)]
#[path = "type_hole_union_deny_test.rs"]
mod type_hole_union_deny_tests;

#[cfg(test)]
#[path = "member_visibility_deny_test.rs"]
mod member_visibility_deny_tests;

#[cfg(all(test, feature = "mir-amd"))]
#[path = "amd_target_test.rs"]
mod amd_target_tests;

#[cfg(test)]
#[path = "closure_shared_capture_test.rs"]
mod closure_shared_capture_tests;

#[cfg(test)]
#[path = "enum_variant_slot_test.rs"]
mod enum_variant_slot_tests;

#[cfg(test)]
#[path = "print_text_kind_test.rs"]
mod print_text_kind_tests;

#[cfg(test)]
#[path = "compile_analyzed_test.rs"]
mod compile_analyzed_tests;

#[cfg(test)]
#[cfg(feature = "mir-wgsl")]
#[path = "wgsl_bound_call_test.rs"]
mod wgsl_bound_call_tests;

#[cfg(test)]
#[cfg(feature = "mir-wgsl")]
#[path = "wgsl_bound_params_test.rs"]
mod wgsl_bound_params_tests;

#[cfg(test)]
#[cfg(feature = "mir-wgsl")]
#[path = "wgsl_bound_bindings_test.rs"]
mod wgsl_bound_bindings_tests;

#[cfg(test)]
#[cfg(feature = "mir-wgsl")]
#[path = "wgsl_stage_annotation_test.rs"]
mod wgsl_stage_annotation_tests;

#[cfg(test)]
#[path = "codegen_analysis_test.rs"]
mod codegen_analysis_tests;

#[cfg(test)]
#[path = "driver_behavior_test.rs"]
mod driver_behavior_tests;
