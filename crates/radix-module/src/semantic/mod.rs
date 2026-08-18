//! Semantic orchestration for resolved, typed compiler state.
//!
//! This module is the boundary between parsed syntax and the HIR consumed by
//! backend-independent analysis and code generation. It owns the semantic pass
//! order, the shared resolver and type table, and the policy that decides when a
//! partially analyzed program is too compromised to lower further.
//!
//! PASS ORDER
//! ==========
//! The canonical pass registry and execution order live in [`passes`]. In
//! outline: collect → resolve → HIR lowering → typecheck → borrow →
//! definite-assignment → exhaustiveness → return-path → lint.
//!
//! ERROR HANDLING
//! ==============
//! Collection, resolution, and HIR lowering errors stop the pipeline before
//! later passes would have to interpret invalid inputs. Typecheck and later
//! analyses collect diagnostics into one [`SemanticResult`]. Warnings flow
//! through the same diagnostics vector, but [`SemanticResult::success`] treats
//! [`SemanticErrorKind::Warning`] as non-fatal.
//!
//! INVARIANTS
//! ==========
//! - HIR lowering sits after AST resolve and before HIR typecheck.
//! - The resolver is the definition identity source for names created before
//!   lowering.
//! - The type table is shared across passes so `TypeId` values remain valid for
//!   the whole semantic result.
//! - Target-specific policy is expressed through [`PassConfig`]; this module
//!   only runs the passes that configuration enables.

mod analyzed_application;
mod annotation_contract;
mod broadcast_reduction;
mod def_id_closure;
mod error;
mod figura;
mod format_type;
mod function_facts;
#[cfg(test)]
#[path = "g2_policy_test.rs"]
mod g2_policy_test;
mod index;
mod index_infer;
mod index_unify;
pub mod init_state;
pub mod instans_precision;
pub mod lista_tensor_conversio;
mod matrix_shape;
pub mod numeric_width;
pub mod passes;
mod qualified_identity;
mod radix_lane;
mod resolved_use;
mod scope;
mod target_qualification;
pub mod tensor_bracket;
pub mod tensor_element_lattice;
#[doc(hidden)]
pub mod tensor_goal_gate;
pub mod tensor_type_directed;
mod types;
pub mod vector_width;

use crate::codegen::Target;
use crate::file_interface::{FileExportKind, FileInterface};
use crate::hir::{HirModule, HirPresentation};
use crate::lexer::Interner;
use crate::locale::{LocalePack, TypeBindingKind};
use crate::syntax::Program;
pub use analyzed_application::{
    qualify_for_target, AnalysisStamp, AnalyzedApplication, AnalyzedApplicationDriverFields, AnalyzedApplicationError,
    AnalyzedApplicationParts, QualificationStatus, TargetQualification,
};
pub use annotation_contract::{
    analyze as analyze_annotation_contracts, collect_local_contracts, register_imported_contract,
    validate_applications as validate_annotation_contract_applications, AnnotationAttachmentTarget, AnnotationContract,
    AnnotationContractField, AnnotationContractMetadata, AnnotationContractRegistry, ContractFieldType,
    ContractPayloadValue, ResolvedContractApplication, ResolvedContractField,
};
pub use broadcast_reduction::compute_cotangent_reduction_axes;
pub use def_id_closure::{
    check_def_id_closure, def_id_closure_gaps, emit_def_table_name, missing_emit_def_diagnostic,
    resolved_reference_defs, DEF_ID_CLOSURE_CODE, DEF_ID_CLOSURE_ISSUE,
};
pub use error::{SemanticError, SemanticErrorKind, WarningKind};
pub use figura::{
    format_figura, literal_index_i64s, lower_figura_expr, FIGURA_NOT_INDEX_PARAM_MSG, FIGURA_TYPE_PARAM_IN_INDEX_MSG,
    FIGURA_UNKNOWN_INDEX_MSG, SPARSA_INCOMPLETE_MSG, SPARSA_MISSING_FIGURA_MSG, TENSOR_INCOMPLETE_MSG,
    TENSOR_MISSING_FIGURA_MSG,
};
pub use format_type::format_faber_type;
pub use function_facts::{analyze_function_facts, CaptureFact, CaptureMode, FunctionFact, FunctionFactTable};
pub use index::{IndexExpr, IndexId, IndexVar};
pub use index_infer::{
    IndexConstraint, IndexInferError, IndexInferState, INCOMPATIBLE_INDEX_MSG, MISSING_TENSOR_SHAPE_MSG,
    SCALAR_TO_TENSOR_CONVERSIO_MSG, TENSOR_ELEMENT_CONVERSIO_MSG, TENSOR_SHAPE_CONVERSIO_MSG,
    TENSOR_TO_SCALAR_CONVERSIO_MSG,
};
pub use index_unify::{IndexUnifier, IndexUnifyError};
pub use init_state::InitState;
pub use instans_precision::InstansPrecision;
pub use matrix_shape::{
    literal_matrix_shape, lower_matrix_shape_expr, MATRIX_INCOMPLETE_MSG, MATRIX_MISSING_FIGURA_MSG,
    MATRIX_SHAPE_TWO_DIM_MSG,
};
pub use numeric_width::{NumericWidth, TENSOR_ELEMENT_NUMERIC_MSG, TENSOR_INDEX_VECTOR_MSG};
use passes::companion_predeclare::CompanionPredeclarations;
pub use qualified_identity::{
    build_qualified_identities, QualifiedDeclKind, QualifiedIdentity, QualifiedIdentityTable,
};
pub use radix_lane::{RadixBackward, RadixLane, RadixLaneMetadata};
pub use resolved_use::{analyze_resolved_uses, ResolvedUse, ResolvedUseIndex, ResolvedUseKey, ResolvedUseKind};
pub use scope::{
    BareVariantLookup, QualifiedVariantPathLookup, Resolver, Scope, ScopeId, ScopeKind, Symbol, SymbolKind,
};
use std::collections::BTreeMap;
pub use types::{
    collection_type_specs, is_atomic_element_type, is_intervallum_bound_type, lookup_collection_type_spec,
    lookup_primitive_type_spec, lookup_tensor_family_type_spec, primitive_type_specs, tensor_family_type_specs,
    CollectionKind, CollectionTypeSpec, FuncSig, InferVar, ParamType, Primitive, PrimitiveTypeSpec, SchemaColumnDef,
    SchemaDef, SemanticMutability, SemanticParamMode, TensorFamilyKind, TensorFamilyTypeSpec, Type, TypeId,
    TypeParamConstraint, TypeTable, TypeTableSnapshot, ATOMIC_ELEMENT_MSG, INTERVALLUM_BOUND_MSG,
};
pub use vector_width::{
    is_vector_width_shape, literal_vector_width, lower_vector_width_expr, VectorWidthError, VECTOR_INCOMPLETE_MSG,
    VECTOR_MISSING_WIDTH_MSG, VECTOR_WIDTH_SINGLE_DIM_ISSUE, VECTOR_WIDTH_SINGLE_DIM_MSG,
};

/// Final semantic product for a parsed program.
///
/// `hir` is absent when an earlier hard diagnostic made downstream phases
/// unreliable. `types` is still returned so diagnostic consumers can inspect
/// whatever type state earlier passes established, and `diagnostics` contains
/// both fatal diagnostics and non-fatal warnings.
pub struct SemanticResult {
    pub hir: Option<HirModule>,

    /// Presentation sidecar produced by the same lowering pass that produced
    /// `hir`. Travels with the HIR value into the driver's `AnalyzedModule`; the
    /// semantic passes themselves stay trivia-blind.
    pub presentation: HirPresentation,
    pub types: TypeTable,
    pub resolver: Resolver,
    pub radix_lanes: RadixLaneMetadata,
    pub annotation_contracts: AnnotationContractMetadata,
    pub gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)>,
    pub diagnostics: Vec<SemanticError>,
}

impl SemanticResult {
    /// Return whether this result is usable by later compiler stages.
    ///
    /// Warnings are intentionally non-fatal here; callers that want stricter
    /// policy must inspect `diagnostics` themselves.
    pub fn success(&self) -> bool {
        self.hir.is_some() && !self.diagnostics.iter().any(SemanticError::is_error)
    }
}

/// Target and caller policy for optional semantic analyses.
///
/// Collection, name resolution, lowering, and typechecking are always part of
/// semantic analysis. These flags cover later HIR analyses whose value depends
/// on the target or caller: Rust enables borrow analysis, while other current
/// targets keep exhaustiveness and lint checks without enforcing Rust ownership
/// rules.
pub struct PassConfig {
    pub target: Target,
    pub borrow_analysis: bool,
    pub exhaustiveness: bool,
    pub lint: bool,
    /// When true, reject legacy `norma:*` imports superseded by `faber:*` kernel paths.
    pub script_kernel_mode: bool,
    /// Namespace member names exported through imported file bindings.
    pub namespace_exports:
        BTreeMap<String, BTreeMap<String, crate::semantic::passes::visibility_annotation::VisibilityTier>>,
    /// Per-binding same-package flag supplied by the program/discovery caller.
    /// Missing bindings are treated as cross-package (fail closed).
    pub namespace_seams: BTreeMap<String, bool>,
    /// All target top-level names (including module-private) keyed by binding.
    pub namespace_declarations:
        BTreeMap<String, BTreeMap<String, crate::semantic::passes::visibility_annotation::VisibilityTier>>,
    /// Portable typed export interfaces keyed by import binding name.
    pub file_interfaces: BTreeMap<String, FileInterface>,
    /// Optional reader-locale pack for source type surface bindings.
    pub locale_pack: Option<LocalePack>,
    /// Import path templates (`§name` → path mapping) from `[paths.templates]`.
    pub import_path_templates: Option<BTreeMap<String, String>>,
}

impl PassConfig {
    /// Target-neutral base analysis configuration.
    ///
    /// Runs shared semantic passes without Rust-only borrow/lifetime checks.
    /// Target selection and qualification happen after
    /// [`AnalyzedApplication`] construction.
    #[must_use]
    pub fn base() -> Self {
        Self {
            target: Target::HirFaber,
            borrow_analysis: false,
            exhaustiveness: true,
            lint: true,
            script_kernel_mode: false,
            namespace_exports: BTreeMap::new(),
            namespace_seams: BTreeMap::new(),
            namespace_declarations: BTreeMap::new(),
            file_interfaces: BTreeMap::new(),
            locale_pack: None,
            import_path_templates: None,
        }
    }

    /// Historical target-shaped configuration.
    ///
    /// Prefer [`PassConfig::base`] plus explicit target qualification for new
    /// application-lane work. Rust still enables borrow analysis here so legacy
    /// single-shot callers keep their prior behavior until fully migrated.
    /// Borrow analysis is Rust-only; every other target keeps base analysis.
    #[must_use]
    pub fn for_target(target: Target) -> Self {
        let mut base = Self::base();
        base.target = target;
        base.borrow_analysis = matches!(target, Target::HirRust);
        base
    }
}

/// Run Rust borrow/lifetime analysis as an explicit target qualification step.
///
/// Base analysis already ran target-neutral mode checks via
/// [`passes::borrow::analyze_modes`]. This filters to Rust-only conflict issues
/// so mode diagnostics are not duplicated.
///
/// # Errors
///
/// Returns Rust borrow/lifetime diagnostics as a list of [`SemanticError`]s
/// when the program violates Rust's ownership rules.
pub fn qualify_rust_borrow(hir: &HirModule, resolver: &Resolver, types: &TypeTable) -> Result<(), Vec<SemanticError>> {
    passes::borrow::analyze_rust_lifetime(hir, resolver, types)
}

/// Whether a diagnostic is a use-site-only name-resolution miss that may let
/// the pre-lowering gate proceed.
///
/// The `:389` gate ("stop before converting syntax") exists because HIR
/// lowering assumes definition identities are coherent. Errors that break
/// that contract — collect-phase/definition failures (duplicates, broken
/// imports, alias cycles, unknown definitions) and any pre-lowering shape,
/// context, or lowering-kind errors — must keep the gate firing. Missing-symbol
/// misses on *uses* (an `UndefinedVariable` / `UndefinedFunction` /
/// `UndefinedMember` on an expression or pattern) do not corrupt the
/// definition table: the lowerer records and continues past them, so the gate
/// lets lowering run and the lowerer's diagnostics fold in before the
/// complete-set gate (`:413`) stops the pipeline. Classification is
/// deliberately CONSERVATIVE — anything not in this family keeps the gate
/// firing; the `:413` complete-set gate is the backstop.
fn is_use_site_only_error(diag: &SemanticError) -> bool {
    matches!(
        diag.kind,
        SemanticErrorKind::UndefinedVariable
            | SemanticErrorKind::UndefinedFunction
            | SemanticErrorKind::UndefinedMember
    )
}

/// Run semantic analysis on a parsed program without CLI mount context.
pub fn analyze(program: &Program, config: &PassConfig, interner: &mut Interner) -> SemanticResult {
    analyze_with_cli(program, config, interner, None)
}

/// Run the semantic pipeline, optionally validating mounted CLI declarations.
///
/// CLI metadata participates during HIR lowering because command mounts depend
/// on resolved declarations but become HIR-level entrypoint metadata for later
/// code generation.
pub fn analyze_with_cli(
    program: &Program,
    config: &PassConfig,
    interner: &mut Interner,
    cli_program: Option<&crate::cli::CliProgram>,
) -> SemanticResult {
    let mut diagnostics = Vec::new();
    let mut types = TypeTable::new();
    let mut resolver = Resolver::new();
    let mut radix_lanes = RadixLaneMetadata::default();

    crate::builtins::register(&mut resolver, &mut types, interner);
    crate::builtins::register_frame_types_if_needed(program, &mut resolver, &mut types, interner);
    if let Some(pack) = config.locale_pack.as_ref() {
        register_locale_pack_type_spellings(pack, &mut resolver, &mut types, interner);
    }
    resolver.load_scribe_family_surfaces(config.locale_pack.as_ref());
    resolver.set_namespace_exports_from_strings(&config.namespace_exports, interner);
    resolver.set_namespace_seams_from_strings(&config.namespace_seams, interner);
    resolver.set_namespace_declarations_from_strings(&config.namespace_declarations, interner);

    // Establish the AST-level symbol graph before any pass tries to bind names.
    if let Err(e) = passes::collect::collect(program, &mut resolver, &mut types) {
        diagnostics.extend(e);
    }

    // Register backward companion names before resolve so user code
    // can call them by identifier without SEM001. DefIds use the stable
    // 0x8000_0000 high-bit scheme shared with RadixLaneMetadata so
    // MIR/driver identity stays consistent.
    let companion_predeclarations = match passes::companion_predeclare::predeclare(program, &mut resolver, interner) {
        Ok(predecls) => predecls,
        Err(e) => {
            diagnostics.extend(e);
            CompanionPredeclarations::default()
        }
    };

    match resolver.set_file_interfaces_from_snapshots_with_locale(
        &config.file_interfaces,
        interner,
        &mut types,
        config.locale_pack.as_ref(),
    ) {
        Ok(skipped) => {
            for skip in skipped {
                diagnostics.push(
                    SemanticError::structured(
                        SemanticErrorKind::Warning(WarningKind::FileInterfaceExportSkipped),
                        crate::lexer::Span::default(),
                    )
                    .with_arg("issue", "file_interface_export_skipped")
                    .with_arg("namespace", skip.namespace)
                    .with_arg("export", skip.export)
                    .with_arg("reason", skip.reason)
                    .with_arg("provider", skip.provider)
                    .with_arg("package", skip.package)
                    .with_arg("module", skip.module)
                    .with_arg("owner", skip.owner)
                    .with_arg("kind", skip.kind)
                    .with_arg("canonical_name", skip.canonical_name)
                    .with_arg(
                        "projected_surfaces",
                        serde_json::to_string(&skip.projected_surfaces).unwrap_or_else(|_| "[]".to_owned()),
                    )
                    .with_arg("remap_reason", skip.remap_reason),
                );
            }
        }
        Err(err) => {
            // Rare: install is skip-unsupported by default. Keep a diagnosable path
            // if a future hard failure returns Err; fold error detail into issue so
            // plain `radix check` surfaces the variant (not only Debug via --diagnostics).
            let detail = format!("{err:?}")
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>();
            diagnostics.push(
                SemanticError::structured(SemanticErrorKind::UndefinedType, crate::lexer::Span::default())
                    .with_arg("issue", format!("file_interface_import_failed_{detail}"))
                    .with_arg("error", format!("{err:?}")),
            );
        }
    }

    if let Err(e) = passes::kernel_namespace::validate_kernel_imports(program, interner) {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::import_path::validate_import_paths(program, interner, config.import_path_templates.as_ref())
    {
        diagnostics.extend(e);
    }

    if config.script_kernel_mode {
        if let Err(e) = passes::kernel_namespace::validate_script_kernel_library_imports(program, interner) {
            diagnostics.extend(e);
        }
    }

    // Resolve still runs after collection errors so users see as many name
    // diagnostics as possible before the hard stop at the AST/HIR boundary.
    if let Err(e) = passes::resolve::resolve(program, &mut resolver, interner, &mut types) {
        diagnostics.extend(e);
    }

    match radix_lane::collect(program, &resolver, interner, &companion_predeclarations) {
        Ok(metadata) => radix_lanes = metadata,
        Err(e) => diagnostics.extend(e),
    }

    // Local `@ annotatio` contracts, then imported schemas, then applications.
    let annotation_contracts =
        collect_annotation_contract_metadata(program, &resolver, interner, &config.file_interfaces, &mut diagnostics);

    // HIR lowering assumes definition identities are coherent; stop before
    // converting syntax when the AST-level contract is already broken.
    // Use-site-only name-resolution misses (missing symbols on uses) do not
    // break that contract — let lowering run so its diagnostics fold in before
    // the complete-set gate below. Warnings (e.g. skipped file-interface
    // exports) must not block lowering.
    if diagnostics
        .iter()
        .any(|diag| diag.is_error() && !is_use_site_only_error(diag))
    {
        return early_semantic_result(types, resolver, radix_lanes, annotation_contracts, diagnostics);
    }

    let (lowered, lower_errors) = crate::hir::lower_with_cli_locale_pack_and_contracts(
        program,
        &mut resolver,
        &mut types,
        interner,
        cli_program,
        config.locale_pack.as_ref(),
        Some(&annotation_contracts),
    );
    let mut hir = lowered.hir;

    for err in lower_errors {
        let mut semantic_error = SemanticError::structured(SemanticErrorKind::LoweringError, err.span);
        semantic_error.args.extend(err.args);
        diagnostics.push(semantic_error);
    }

    // Typecheck and later analyses assume HIR nodes carry the required semantic
    // attachments; lowering diagnostics are therefore fatal for this pipeline.
    // Pre-existing warnings (file-interface skips, etc.) must not abort analysis.
    if diagnostics.iter().any(SemanticError::is_error) {
        return early_semantic_result(types, resolver, radix_lanes, annotation_contracts, diagnostics);
    }

    // Attach typed companion signatures so typecheck's collect_items registers
    // companion functions for call-site type validation. Must run after HIR
    // lowering (primal functions exist) and before typecheck.
    // Companion signatures are injected directly into the typecheck function table
    // rather than via HIR stubs -- bodyless HIR items with aggregate return types
    // poison MIR lowering.
    let companion_signatures =
        passes::companion_signature::attach_signatures(&hir, &radix_lanes, &mut resolver, &mut types, interner);

    if let Err(e) = passes::typecheck::typecheck_with_config(
        &mut hir,
        &resolver,
        &mut types,
        Some(interner),
        config,
        Some(&companion_signatures),
    ) {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::air_purity::check(&hir, &radix_lanes, interner, &types) {
        diagnostics.extend(e);
    }

    // G2: target-neutral de/in/ex mode analysis always runs on base analysis.
    // When borrow_analysis is already on (legacy for_target(Rust)), full
    // `borrow::analyze` covers modes + Rust lifetime — skip the dual pass.
    if config.borrow_analysis {
        if let Err(e) = passes::borrow::analyze(&hir, &resolver, &types) {
            diagnostics.extend(e);
        }
    } else if let Err(e) = passes::borrow::analyze_modes(&hir, &resolver, &types) {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::definite_assignment::analyze(&hir, &resolver, &types) {
        diagnostics.extend(e);
    }

    if config.exhaustiveness {
        if let Err(e) = passes::exhaustive::check(&hir, &types) {
            diagnostics.extend(e);
        }
    }

    if let Err(e) = passes::return_path::analyze(&hir, &types) {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::live_tuus_cursor_escape::analyze(&hir, &resolver, &types, interner) {
        diagnostics.extend(e);
    }

    if config.lint {
        if let Err(e) = passes::visibility_annotation::check(program, interner) {
            diagnostics.extend(e);
        }

        // lint findings include non-fatal warnings (Warning-kind entries);
        // fatality is decided downstream via SemanticError::is_error.
        diagnostics.extend(passes::lint::lint(&hir, &resolver, &types, interner));
    }

    SemanticResult {
        hir: Some(hir),
        presentation: lowered.presentation,
        types,
        radix_lanes,
        annotation_contracts,
        gpu_builtins: crate::builtins::gpu::builtin_calls(&resolver),
        resolver,
        diagnostics,
    }
}

// constructor: moves all parameters into result
fn early_semantic_result(
    types: TypeTable,
    resolver: Resolver,
    radix_lanes: RadixLaneMetadata,
    annotation_contracts: AnnotationContractMetadata,
    diagnostics: Vec<SemanticError>,
) -> SemanticResult {
    SemanticResult {
        hir: None,
        presentation: empty_presentation(),
        types,
        radix_lanes,
        annotation_contracts,
        gpu_builtins: crate::builtins::gpu::builtin_calls(&resolver),
        resolver,
        diagnostics,
    }
}

/// A presentation for a result that never reached lowering.
///
/// No HIR exists here, so there is no real program-end boundary; the sidecar
/// is deliberately empty.
fn empty_presentation() -> HirPresentation {
    HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: crate::hir::HirModuleEnd {
            id: crate::hir::HirSourceAnchorId(0),
            span: crate::lexer::Span::default(),
        },
        entry_start_anchor: None,
    }
}

fn collect_annotation_contract_metadata(
    program: &Program,
    resolver: &Resolver,
    interner: &mut Interner,
    file_interfaces: &BTreeMap<String, FileInterface>,
    diagnostics: &mut Vec<SemanticError>,
) -> AnnotationContractMetadata {
    let mut registry = match annotation_contract::collect_local_contracts(program, resolver, interner) {
        Ok(registry) => registry,
        Err(e) => {
            diagnostics.extend(e);
            AnnotationContractRegistry::new()
        }
    };
    install_imported_annotation_contracts(file_interfaces, resolver, interner, &mut registry);
    let applications = match annotation_contract::validate_applications(program, &registry, resolver, interner) {
        Ok(applications) => applications,
        Err(e) => {
            diagnostics.extend(e);
            rustc_hash::FxHashMap::default()
        }
    };
    AnnotationContractMetadata { registry, applications }
}

fn install_imported_annotation_contracts(
    file_interfaces: &BTreeMap<String, FileInterface>,
    resolver: &Resolver,
    interner: &mut Interner,
    registry: &mut AnnotationContractRegistry,
) {
    // BO-D-4: build name → DefId once (imported structs are stable for this pass).
    let mut imported_struct_by_name = rustc_hash::FxHashMap::default();
    for (def_id, _fields, _methods) in resolver.imported_file_structs() {
        if let Some(sym) = resolver.get_symbol(def_id) {
            let name = interner.resolve(sym.name).to_owned();
            imported_struct_by_name.entry(name).or_insert(def_id);
        }
    }
    for interface in file_interfaces.values() {
        for (member_name, export) in &interface.exports {
            let FileExportKind::Struct(strukt) = &export.kind else {
                continue;
            };
            let Some(meta) = &strukt.annotation_contract else {
                continue;
            };
            let Some(def_id) = imported_struct_def_id(resolver, interner, member_name, &imported_struct_by_name) else {
                continue;
            };
            let Some(target) = AnnotationAttachmentTarget::parse(meta.target.as_str()) else {
                continue;
            };
            let fields = meta
                .fields
                .iter()
                .filter_map(|field| {
                    let ty = ContractFieldType::from_type_name(field.ty.as_str())?;
                    Some(AnnotationContractField {
                        name: interner.intern(&field.name),
                        ty,
                        optional: field.optional,
                        span: crate::lexer::Span::default(),
                    })
                })
                .collect();
            register_imported_contract(
                registry,
                def_id,
                interner.intern(member_name),
                target,
                fields,
                crate::lexer::Span::default(),
            );
        }
    }
}

fn imported_struct_def_id(
    resolver: &Resolver,
    interner: &mut Interner,
    member_name: &str,
    imported_struct_by_name: &rustc_hash::FxHashMap<String, crate::hir::DefId>,
) -> Option<crate::hir::DefId> {
    let member_sym = interner.intern(member_name);
    if let Some(def_id) = resolver.lookup(member_sym) {
        return Some(def_id);
    }
    imported_struct_by_name.get(member_name).copied()
}

fn register_locale_pack_type_spellings(
    pack: &LocalePack,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) {
    // Intrinsic-locale (L3): pre-intern canonical intrinsic names so the
    // typecheck seam can rewrite a localized surface method spelling to its
    // canonical symbol on the HIR (lint and MIR lowering then match canonical
    // registry names unchanged). No-op for packs without an `[intrinsics]`
    // section (la: identity-by-absence).
    for canonical in pack.intrinsics.keys() {
        interner.intern(canonical);
    }
    for binding in pack.types.values() {
        let spellings = std::iter::once(binding.canonical.as_str()).chain(binding.aliases.iter().map(String::as_str));
        for spelling in spellings {
            let name = interner.intern(spelling);
            if resolver.lookup_type(name).is_some() {
                continue;
            }
            let ty = match binding.kind {
                TypeBindingKind::Primitive(prim) => Some(types.primitive(prim)),
                // Collections and the tensor family are parameterized shapes
                // (no bare `TypeId`); their spellings register as name-only
                // aliases and lower through dedicated type syntax.
                TypeBindingKind::Collection(_) => None,
                TypeBindingKind::TensorFamily(_) => None,
                TypeBindingKind::ModularWord => None,
                // Census relational names are reader-surface rows only in S2;
                // no bare `TypeId` until the S3/S4 type surface lands.
                TypeBindingKind::Relational(_) => None,
            };
            let symbol = scope::Symbol {
                def_id: resolver.fresh_builtin_def_id(),
                name,
                kind: SymbolKind::TypeAlias,
                ty,
                mutable: false,
                span: crate::lexer::Span::default(),
            };
            if let Ok(def_id) = resolver.define(symbol) {
                resolver.record_locale_pack_type_alias(def_id, name)
            }
        }
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
