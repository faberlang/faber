//! Semantic analysis for Faber: collection, resolution, HIR lowering,
//! typechecking, and post-typecheck analyses.
//!
//! `radix-semantic` owns the boundary between parsed syntax and the typed HIR
//! consumed by backend-independent analysis and code generation, plus its
//! coupled satellites: the `@ cli` source model, reader-locale packs,
//! portable file interfaces, builtins/kernel registries, and the HIR
//! lowering entry. It runs the semantic pass order, owns the shared resolver
//! and type table, and decides when a partially analyzed program is too
//! compromised to lower further.
//!
//! PASS ORDER
//! ==========
//! The canonical pass registry and execution order live in [`passes`]. In
//! outline: collect → resolve → HIR lowering → typecheck → air-unroll →
//! air-purity → borrow → definite-assignment → exhaustiveness → return-path →
//! lint.
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

pub mod access_facts;
pub mod analyzed_application;
pub mod annotation_contract;
pub mod broadcast_reduction;
pub mod capability;
pub mod def_id_closure;
pub mod error;
pub mod figura;
pub mod format_type;
pub mod function_facts;
pub mod index;
pub mod index_infer;
pub mod index_unify;
pub mod init_state;
pub mod instans_precision;
pub mod lista_tensor_conversio;
pub mod matrix_shape;
pub mod numeric_width;
pub mod passes;
pub mod qualified_identity;
pub mod radix_lane;
pub mod resolved_use;
pub mod scope;
pub mod target_qualification;
pub mod template_holes;
pub mod tensor_bracket;
pub mod tensor_element_lattice;
pub mod tensor_type_directed;
pub mod tuple_labels;
pub mod types;
pub mod vector_width;

pub mod builtins;
pub mod cli;
pub mod coverage;
pub mod file_interface;
pub mod kernel;
pub mod live_tuus_cursor;
pub mod locale;
pub mod lower;
#[cfg(test)]
#[path = "test_support.rs"]
pub mod test_support;
pub use lower::LoweredProgram;
pub use lower::annotation::try_parse_nondum_annotation;
pub use numeric_width::is_tensor_element_type;
pub use numeric_width::tensor_element_conversio_allowed;
pub use radix_hir::{DefId, HirAnnotation, HirAnnotationField, HirAnnotationValue};
pub use rustc_hash::FxHashMap;
pub use rustc_hash::FxHashSet;

pub mod catalog;
pub mod catalog_semantic;
pub mod catalog_syntax;
pub use catalog::{arg_value, quote_arg};
pub use catalog_semantic::semantic_spec;
pub use catalog_syntax::{lex_spec, parse_message, parse_spec};

use radix_diagnostics::DiagnosticSpec;

/// Every diagnostic spec across the syntax and semantic catalogs plus the
/// foundation unresolved/locale specs. Reader-locale packs validate their
/// templates against this list.
pub fn all_diagnostic_specs() -> Vec<DiagnosticSpec> {
    let mut specs = vec![
        radix_diagnostics::unresolved_keyword_spec(),
        radix_diagnostics::unresolved_intrinsic_spec(),
        radix_diagnostics::unresolved_library_member_spec(),
        radix_diagnostics::locale_suggestion_spec(),
        radix_diagnostics::locale_unknown_spec(),
        radix_diagnostics::mixed_script_identifier_spec(),
    ];
    specs.extend(
        catalog_syntax::LEX_ERROR_KINDS
            .iter()
            .copied()
            .map(catalog_syntax::lex_spec),
    );
    specs.extend(
        catalog_syntax::PARSE_ERROR_KINDS
            .iter()
            .copied()
            .map(catalog_syntax::parse_spec),
    );
    specs.extend(
        catalog_semantic::SEMANTIC_ERROR_KINDS
            .iter()
            .copied()
            .map(catalog_semantic::semantic_spec),
    );
    specs.extend(catalog_semantic::WARNING_KINDS.iter().copied().map(|kind| {
        catalog_semantic::semantic_spec(crate::error::SemanticErrorKind::Warning(kind))
    }));
    specs
}

use crate::file_interface::{FileExportKind, FileInterface};
use crate::locale::{LocalePack, TypeBindingKind};
pub use access_facts::{
    AccessFact, AccessFactTable, DeadSourceFact, DeadSourceReason, analyze_access_facts,
    analyze_access_facts_with_types,
};
pub use analyzed_application::{
    AnalysisStamp, AnalyzedApplication, AnalyzedApplicationDriverFields, AnalyzedApplicationError,
    AnalyzedApplicationParts, QualificationStatus, TargetQualification, qualify_for_target,
};
pub use annotation_contract::{
    AnnotationAttachmentTarget, AnnotationContract, AnnotationContractField,
    AnnotationContractMetadata, AnnotationContractRegistry, ContractFieldType,
    ContractPayloadValue, ResolvedContractApplication, ResolvedContractField,
    analyze as analyze_annotation_contracts, collect_local_contracts, register_imported_contract,
    validate_applications as validate_annotation_contract_applications,
};
pub use broadcast_reduction::compute_cotangent_reduction_axes;
pub use capability::{
    CAPABILITY_VOCABULARY_VERSION, CapabilityAnswer, CapabilityFamily, CapabilityKind,
    CapabilityQueries, CapabilityQuery, CapabilityReason, CapturePosture, CaptureRequirement,
    DerivedCapabilityQueries, EffectPosture, ExportVisibility, FailClosedCapabilityQueries,
    FunctionCapabilityAnswer, FunctionCapabilityKind, FunctionCapabilityProof,
    FunctionCapabilityQueries, FunctionCapabilitySummary, FunctionProof, FunctionProvenanceSummary,
    FunctionProvenanceTable, FunctionSummary, IndexCapabilityAnswer, IndexCapabilityKind,
    IndexCapabilityProof, IndexCapabilityQueries, IndexCapabilitySummary, IndexProof, IndexSummary,
    LastUseFact, Liveness, ParameterAccess, ParameterAccessPosture, PlaceCapabilityAnswer,
    PlaceCapabilityKind, PlaceCapabilityProof, PlaceCapabilityQueries, PlaceCapabilitySummary,
    PlaceProof, RecursionOutcome, RegionRelationship, RequiredRegionRelationships,
    ReturnedBorrowProvenance, SolvedIndexedIdentity, TypeCapabilityAnswer, TypeCapabilityKind,
    TypeCapabilityProof, TypeCapabilityQueries, TypeCapabilitySummary, TypeProof, TypeSummary,
};
pub use def_id_closure::{
    DEF_ID_CLOSURE_CODE, DEF_ID_CLOSURE_ISSUE, check_def_id_closure, def_id_closure_gaps,
    emit_def_table_name, missing_emit_def_diagnostic, resolved_reference_defs,
};
pub use error::{SemanticError, SemanticErrorKind, WarningKind};
pub use figura::{
    FIGURA_NOT_INDEX_PARAM_MSG, FIGURA_TYPE_PARAM_IN_INDEX_MSG, FIGURA_UNKNOWN_INDEX_MSG,
    SPARSA_INCOMPLETE_MSG, SPARSA_MISSING_FIGURA_MSG, TENSOR_INCOMPLETE_MSG,
    TENSOR_MISSING_FIGURA_MSG, format_figura, i64s_from_literal_index, lower_figura_expr,
};
pub use format_type::format_faber_type;
pub use function_facts::{
    CaptureFact, CaptureMode, FunctionFact, FunctionFactTable, analyze_function_facts,
};
pub use index::{IndexExpr, IndexId, IndexVar};
pub use index_infer::{
    INCOMPATIBLE_INDEX_MSG, IndexConstraint, IndexInferError, IndexInferState,
    MISSING_TENSOR_SHAPE_MSG, SCALAR_TO_TENSOR_CONVERSIO_MSG, TENSOR_ELEMENT_CONVERSIO_MSG,
    TENSOR_SHAPE_CONVERSIO_MSG, TENSOR_TO_SCALAR_CONVERSIO_MSG,
};
pub use index_unify::{IndexUnifier, IndexUnifyError};
pub use init_state::InitState;
pub use instans_precision::InstansPrecision;
pub use matrix_shape::{
    MATRIX_INCOMPLETE_MSG, MATRIX_MISSING_FIGURA_MSG, MATRIX_SHAPE_TWO_DIM_MSG,
    literal_matrix_shape, lower_matrix_shape_expr,
};
pub use numeric_width::{
    DECIMAL_INDEX_SCALAR_MSG, DECIMAL_TENSOR_MEDIA_MSG, NumericWidth, TENSOR_ELEMENT_NUMERIC_MSG,
    TENSOR_INDEX_VECTOR_MSG,
};
use passes::companion_predeclare::CompanionPredeclarations;
pub use qualified_identity::{
    QualifiedDeclKind, QualifiedIdentity, QualifiedIdentityTable, build_qualified_identities,
};
use radix_hir::{HirModule, HirPresentation};
pub use radix_lane::{RadixBackward, RadixLane, RadixLaneMetadata};
use radix_lexer::Interner;
use radix_syntax::Program;
use radix_target::Target;
pub use resolved_use::{
    ResolvedUse, ResolvedUseIndex, ResolvedUseKey, ResolvedUseKind, analyze_resolved_uses,
};
pub use scope::{
    BareVariantLookup, CommuneField, ConversionPair, ConversionRow, ConversionRowConflict,
    QualifiedVariantPathLookup, Resolver, Scope, ScopeId, ScopeKind, StructHeritageFieldRow,
    Symbol, SymbolKind,
};
use std::collections::BTreeMap;
pub use types::{
    ATOMIC_ELEMENT_MSG, AccessMarker, CollectionKind, CollectionTypeSpec, FuncSig,
    GenericBindingSource, GenericParamDescriptor, GenericParamSort, GenericWitness,
    INTERVALLUM_BOUND_MSG, ISSUE_ASSIGN_THROUGH_REF, ISSUE_DEREF_INTO_OWN, ISSUE_FIELD_INTO_OWN,
    ISSUE_INDEX_INTO_OWN, ISSUE_MUT_INTO_OWN, ISSUE_MUT_LOCAL, ISSUE_MUTATING_RECEIVER_THROUGH_REF,
    ISSUE_PROJECTION_INTO_OWN, ISSUE_REF_INTO_OWN, ISSUE_REF_TO_MUT_OR_OWN_POSITION,
    ISSUE_SCALAR_ACCESS_MARKER, ISSUE_SHARED_INTO_OWN, ISSUE_UNMARKED_INTO_OWN, InferVar,
    MarkerFamily, ParamType, Primitive, PrimitiveTypeSpec, ResolvedGenericBinding,
    ResolvedGenericCall, SUGGESTION_COPY, SchemaColumnDef, SchemaDef, SemanticGenericParamDefault,
    SemanticMutability, SemanticParamMode, TensorFamilyKind, TensorFamilyTypeSpec, Type, TypeId,
    TypeParamConstraint, TypeTable, TypeTableSnapshot, access_marker_from_type_mode,
    collection_type_specs, is_atomic_element_type, is_intervallum_bound_type,
    lookup_collection_type_spec, lookup_primitive_type_spec, lookup_tensor_family_type_spec,
    primitive_type_specs, tensor_family_type_specs,
};
pub use vector_width::{
    VECTOR_INCOMPLETE_MSG, VECTOR_MISSING_WIDTH_MSG, VECTOR_WIDTH_SINGLE_DIM_ISSUE,
    VECTOR_WIDTH_SINGLE_DIM_MSG, VectorWidthError, is_vector_width_shape, literal_vector_width,
    lower_vector_width_expr,
};

/// Final semantic product for a parsed program.
///
/// `hir` is absent when an earlier hard diagnostic made downstream phases
/// unreliable. `types` is still returned so diagnostic consumers can inspect
/// whatever type state earlier passes established, and `diagnostics` contains
/// both fatal diagnostics and non-fatal warnings.
pub struct SemanticResult {
    pub hir: Option<HirModule>,

    /// The one typed-HIR access-fact table constructed after AIR passes.  A
    /// failed pre-HIR result carries `None`; every successful result carries
    /// the table consumed by the region handoff.
    pub access_facts: Option<AccessFactTable>,

    /// Region facts from the same run's single region-engine handoff. A
    /// failed pre-HIR result carries `None`; every successful result carries
    /// the facts the borrow passes already consumed (`None` only when the
    /// engine rejected an unsupported control-flow shape). The analyzed
    /// application consumes these carried facts instead of re-running the
    /// engine.
    pub region_facts: Option<crate::passes::borrow_region::RegionFacts>,

    /// Presentation sidecar produced by the same lowering pass that produced
    /// `hir`. Travels with the HIR value into the driver's `AnalyzedModule`; the
    /// semantic passes themselves stay trivia-blind.
    pub presentation: HirPresentation,
    pub types: TypeTable,
    pub resolver: Resolver,
    pub radix_lanes: RadixLaneMetadata,
    pub annotation_contracts: AnnotationContractMetadata,
    pub gpu_builtins: Vec<(radix_hir::DefId, radix_types::MirKernelBuiltin)>,
    pub diagnostics: Vec<SemanticError>,
}

impl SemanticResult {
    /// Return whether this result is usable by later compiler stages.
    ///
    /// Warnings are intentionally non-fatal here; callers that want stricter
    /// policy must inspect `diagnostics` themselves.
    pub fn success(&self) -> bool {
        self.hir.is_some()
            && self.access_facts.is_some()
            && !self.diagnostics.iter().any(SemanticError::is_error)
    }

    /// Result-shaped alias of [`Self::success`] for in-crate tests.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_ok(&self) -> bool {
        self.success()
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
    /// Namespace member names exported through imported file bindings.
    pub namespace_exports:
        BTreeMap<String, BTreeMap<String, crate::passes::visibility_annotation::VisibilityTier>>,
    /// Per-binding same-package flag supplied by the program/discovery caller.
    /// Missing bindings are treated as cross-package (fail closed).
    pub namespace_seams: BTreeMap<String, bool>,
    /// All target top-level names (including module-private) keyed by binding.
    pub namespace_declarations:
        BTreeMap<String, BTreeMap<String, crate::passes::visibility_annotation::VisibilityTier>>,
    /// Portable typed export interfaces keyed by import binding name.
    pub file_interfaces: BTreeMap<String, FileInterface>,
    /// Optional reader-locale pack for source type surface bindings.
    pub locale_pack: Option<LocalePack>,
    /// Import path templates (`§name` → path mapping) from `[paths.templates]`.
    pub import_path_templates: Option<BTreeMap<String, String>>,
    /// Per-function cyclomatic-complexity warning budget; `None` uses the
    /// baked default ([`passes::complexity::DEFAULT_COMPLEXITY_BUDGET`]).
    pub complexity_budget: Option<u32>,
    /// Advisory per-function complexity budget for `@ nucleum` kernel
    /// entries; `None` uses the baked kernel default
    /// ([`passes::complexity::DEFAULT_KERNEL_COMPLEXITY_BUDGET`]). Warns
    /// only — it never gates kernel admission.
    pub kernel_complexity_budget: Option<u32>,
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
            namespace_exports: BTreeMap::new(),
            namespace_seams: BTreeMap::new(),
            namespace_declarations: BTreeMap::new(),
            file_interfaces: BTreeMap::new(),
            locale_pack: None,
            import_path_templates: None,
            complexity_budget: None,
            kernel_complexity_budget: None,
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
pub fn qualify_rust_borrow(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
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
#[must_use]
pub fn is_use_site_only_error(diag: &SemanticError) -> bool {
    matches!(
        diag.kind,
        SemanticErrorKind::VariableUndefined
            | SemanticErrorKind::FunctionUndefined
            | SemanticErrorKind::MemberUndefined
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

    // Analysis setup: builtin genera plus the compiler-shipped conversion
    // rows (CTR-04) seed before user declaration collection so enrollment can
    // never shadow them.
    crate::builtins::register(&mut resolver, &mut types, interner);
    crate::builtins::register_frame_types_if_needed(
        program,
        &mut resolver,
        &mut types,
        interner,
        config.locale_pack.as_ref(),
    );
    intern_catalog_generic_names(interner);
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
    let companion_predeclarations =
        match passes::companion_predeclare::predeclare(program, &mut resolver, interner) {
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
                        radix_lexer::Span::default(),
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
                        serde_json::to_string(&skip.projected_surfaces)
                            .unwrap_or_else(|_| "[]".to_owned()),
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
                SemanticError::structured(
                    SemanticErrorKind::TypeUndefined,
                    radix_lexer::Span::default(),
                )
                .with_arg("issue", format!("file_interface_import_failed_{detail}"))
                .with_arg("error", format!("{err:?}")),
            );
        }
    }

    if let Err(e) = passes::kernel_namespace::validate_kernel_imports(program, interner) {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::import_path::validate_import_paths(
        program,
        interner,
        config.import_path_templates.as_ref(),
    ) {
        diagnostics.extend(e);
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
    let annotation_contracts = collect_annotation_contract_metadata(
        program,
        &resolver,
        interner,
        &config.file_interfaces,
        &mut diagnostics,
    );

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
        return early_semantic_result(
            types,
            resolver,
            radix_lanes,
            annotation_contracts,
            // Region inference has not run at this failure point.
            None,
            diagnostics,
        );
    }

    let (mut lowered, lower_errors) = crate::lower::lower(crate::lower::LowerRequest {
        program,
        resolver: &mut resolver,
        types: &mut types,
        interner,
        cli_program,
        locale_pack: config.locale_pack.as_ref(),
        annotation_contracts: Some(&annotation_contracts),
    });
    let mut hir = lowered.hir;

    for err in lower_errors {
        let mut semantic_error =
            SemanticError::structured(SemanticErrorKind::LoweringError, err.span);
        semantic_error.args.extend(err.args);
        diagnostics.push(semantic_error);
    }

    // Typecheck and later analyses assume HIR nodes carry the required semantic
    // attachments; lowering diagnostics are therefore fatal for this pipeline.
    // Pre-existing warnings (file-interface skips, etc.) must not abort analysis.
    if diagnostics.iter().any(SemanticError::is_error) {
        return early_semantic_result(
            types,
            resolver,
            radix_lanes,
            annotation_contracts,
            // Region inference has not run at this failure point.
            None,
            diagnostics,
        );
    }

    // Attach typed companion signatures so typecheck's collect_items registers
    // companion functions for call-site type validation. Must run after HIR
    // lowering (primal functions exist) and before typecheck.
    // Companion signatures are injected directly into the typecheck function table
    // rather than via HIR stubs -- bodyless HIR items with aggregate return types
    // poison MIR lowering.
    let companion_signatures = passes::companion_signature::attach_signatures(
        &hir,
        &radix_lanes,
        &mut resolver,
        &mut types,
        interner,
    );

    match passes::typecheck::typecheck_with_config(
        &mut hir,
        &resolver,
        &mut types,
        Some(interner),
        config,
        Some(&companion_signatures),
        Some(&mut lowered.presentation),
    ) {
        // The merged heritage view is the checker's admission authority for
        // genus literals and field access; publish it so MIR aggregate
        // lowering/validation reuse the one view instead of re-deriving
        // parent-flattening.
        Ok(merged_heritage_fields) => {
            resolver.register_struct_heritage_fields(merged_heritage_fields);
        }
        Err(e) => diagnostics.extend(e),
    }
    // Concrete generic AIR instances must exist before named unroll: symbolic
    // templates remain source/FHIR facts, while their reachable concrete clones
    // expose literal shapes and bounds to the existing transform.
    if !diagnostics.iter().any(SemanticError::is_error)
        && matches!(
            config.target,
            Target::MirStepper
                | Target::MirWasm
                | Target::MirLlvm
                | Target::MirLlvmHost
                | Target::MirAmd
                | Target::MirMetal
                | Target::MirWgsl
                | Target::MirSexp
                | Target::MirScena
        )
    {
        passes::air_specialize::specialize(&mut hir, &mut radix_lanes, &mut types, interner);
    }

    // Named unroll: concrete-N `itera ab` → straight line, before air_purity.
    // Skip when typecheck already failed — types on the rewritten nodes would
    // be unreliable.
    if !diagnostics.iter().any(SemanticError::is_error)
        && let Err(e) = passes::air_unroll::unroll(
            &mut hir,
            &mut lowered.presentation,
            &radix_lanes,
            interner,
            &types,
        )
    {
        diagnostics.extend(e);
    }

    // Skip when typecheck already failed — admission here is decided from
    // those same (unreliable) types, so a decline would describe a purity
    // problem the program does not have.
    if !diagnostics.iter().any(SemanticError::is_error)
        && let Err(e) = passes::air_purity::check(&mut hir, &mut radix_lanes, interner, &types)
    {
        diagnostics.extend(e);
    }

    // BRI-2: construct the sole typed-HIR fact table only after both AIR
    // passes, hand the same borrowed table to region inference, then carry it
    // forward for the analyzed-application snapshot. Unsupported HIR shapes
    // leave region facts unavailable rather than receiving an approximation.
    let access_facts = analyze_access_facts_with_types(&hir, &types);
    let region_facts = passes::borrow_region::analyze(&hir, &types, &access_facts).ok();

    diagnostics
        .extend(passes::fragment_definition::check(&hir, &resolver, &types, interner).errors);

    // G2: target-neutral de/in/ex mode analysis always runs on base analysis.
    // When borrow_analysis is already on (legacy for_target(Rust)), full
    // `borrow::analyze` covers modes + Rust lifetime — skip the dual pass.
    if config.borrow_analysis {
        if let Err(e) =
            passes::borrow::analyze_with_regions(&hir, &resolver, &types, region_facts.as_ref())
        {
            diagnostics.extend(e);
        }
    } else if let Err(e) =
        passes::borrow::analyze_modes_with_regions(&hir, &resolver, &types, region_facts.as_ref())
    {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::definite_assignment::analyze(&hir, &resolver, &types) {
        diagnostics.extend(e);
    }

    if config.exhaustiveness
        && let Err(e) = passes::exhaustive::check(&hir, &types)
    {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::return_path::analyze(&hir, &types) {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::live_tuus_cursor_escape::analyze(&hir, &resolver, &types, interner) {
        diagnostics.extend(e);
    }

    if let Err(e) = passes::libera::analyze(&hir, &resolver, interner) {
        diagnostics.extend(e);
    }

    if config.lint {
        if let Err(e) = passes::visibility_annotation::check(program, interner) {
            diagnostics.extend(e);
        }

        // lint findings include non-fatal warnings (Warning-kind entries);
        // fatality is decided downstream via SemanticError::is_error.
        diagnostics.extend(passes::lint::lint(
            &hir,
            &resolver,
            &types,
            interner,
            config.locale_pack.as_ref(),
        ));

        // Complexity is a structural per-function budget analysis with its own
        // options; warn-only, same gate as lint. Kernel entries measure
        // against their own advisory budget (never admission).
        let complexity_budget = config
            .complexity_budget
            .unwrap_or(passes::complexity::DEFAULT_COMPLEXITY_BUDGET);
        let kernel_complexity_budget = config
            .kernel_complexity_budget
            .unwrap_or(passes::complexity::DEFAULT_KERNEL_COMPLEXITY_BUDGET);
        diagnostics.extend(passes::complexity::complexity(
            &hir,
            interner,
            complexity_budget,
            kernel_complexity_budget,
        ));
    }

    SemanticResult {
        hir: Some(hir),
        access_facts: Some(access_facts),
        region_facts,
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
pub(crate) fn early_semantic_result(
    types: TypeTable,
    resolver: Resolver,
    radix_lanes: RadixLaneMetadata,
    annotation_contracts: AnnotationContractMetadata,
    // Region inference runs after typecheck, so every early-failure caller
    // has no region facts to carry and passes `None` here.
    region_facts: Option<crate::passes::borrow_region::RegionFacts>,
    diagnostics: Vec<SemanticError>,
) -> SemanticResult {
    SemanticResult {
        hir: None,
        access_facts: None,
        region_facts,
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
pub(crate) fn empty_presentation() -> HirPresentation {
    HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
    }
}

fn collect_annotation_contract_metadata(
    program: &Program,
    resolver: &Resolver,
    interner: &mut Interner,
    file_interfaces: &BTreeMap<String, FileInterface>,
    diagnostics: &mut Vec<SemanticError>,
) -> AnnotationContractMetadata {
    let mut registry =
        match annotation_contract::collect_local_contracts(program, resolver, interner) {
            Ok(registry) => registry,
            Err(e) => {
                diagnostics.extend(e);
                AnnotationContractRegistry::new()
            }
        };
    install_imported_annotation_contracts(file_interfaces, resolver, interner, &mut registry);
    let applications =
        match annotation_contract::validate_applications(program, &registry, resolver, interner) {
            Ok(applications) => applications,
            Err(e) => {
                diagnostics.extend(e);
                rustc_hash::FxHashMap::default()
            }
        };
    AnnotationContractMetadata {
        registry,
        applications,
    }
}

fn install_imported_annotation_contracts(
    file_interfaces: &BTreeMap<String, FileInterface>,
    resolver: &Resolver,
    interner: &mut Interner,
    registry: &mut AnnotationContractRegistry,
) {
    // BO-D-4: build name → DefId once (imported structs are stable for this pass).
    let mut imported_struct_by_name = rustc_hash::FxHashMap::default();
    for (def_id, _fields, _methods, _type_params) in resolver.imported_file_structs() {
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
            let Some(def_id) =
                imported_struct_def_id(resolver, interner, member_name, &imported_struct_by_name)
            else {
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
                        span: radix_lexer::Span::default(),
                    })
                })
                .collect();
            register_imported_contract(
                registry,
                def_id,
                interner.intern(member_name),
                target,
                fields,
                radix_lexer::Span::default(),
            );
        }
    }
}

fn imported_struct_def_id(
    resolver: &Resolver,
    interner: &mut Interner,
    member_name: &str,
    imported_struct_by_name: &rustc_hash::FxHashMap<String, radix_hir::DefId>,
) -> Option<radix_hir::DefId> {
    let member_sym = interner.intern(member_name);
    if let Some(def_id) = resolver.lookup(member_sym) {
        return Some(def_id);
    }
    imported_struct_by_name.get(member_name).copied()
}

fn intern_catalog_generic_names(interner: &mut Interner) {
    for entry in radix_intrinsics::INTRINSICS {
        for param in entry.generic {
            interner.intern(param.name);
        }
    }
    for entry in radix_intrinsics::NAMESPACE_INTRINSICS {
        for param in entry.generic {
            interner.intern(param.name);
        }
    }
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
        // Conversation-family spellings (`channel`, `frame`, `send`, `recv`)
        // resolve only through the parser's canonical type ident. They are
        // never symbols: the words stay ordinary identifiers (a user `fn send`
        // must not collide with a type alias).
        if matches!(binding.kind, TypeBindingKind::Frame(_)) {
            continue;
        }
        let spellings = std::iter::once(binding.canonical.as_str())
            .chain(binding.aliases.iter().map(String::as_str));
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
                TypeBindingKind::ModularWord | TypeBindingKind::ByteWord => None,
                // Census relational names are reader-surface rows only in S2;
                // no bare `TypeId` until the S3/S4 type surface lands.
                TypeBindingKind::Relational(_) => None,
                TypeBindingKind::Ratio | TypeBindingKind::Frame(_) => None,
            };
            let symbol = scope::Symbol {
                def_id: resolver.fresh_builtin_def_id(),
                name,
                kind: SymbolKind::TypeAlias,
                ty,
                mutable: false,
                span: radix_lexer::Span::default(),
            };
            if let Ok(def_id) = resolver.define(symbol) {
                resolver.record_locale_pack_type_alias(def_id, name)
            }
        }
    }
}

#[cfg(test)]
#[path = "access_facts_test.rs"]
mod access_facts_test;

#[cfg(test)]
#[path = "capability_test.rs"]
mod capability_test;

#[cfg(test)]
#[path = "analyzed_application_test.rs"]
mod analyzed_application_test;

#[cfg(test)]
#[path = "annotation_contract_test.rs"]
mod annotation_contract_test;

#[cfg(test)]
#[path = "broadcast_reduction_test.rs"]
mod broadcast_reduction_test;

#[cfg(test)]
#[path = "def_id_closure_test.rs"]
mod def_id_closure_test;

#[cfg(test)]
#[path = "error_test.rs"]
mod error_test;

#[cfg(test)]
#[path = "figura_literal_test.rs"]
mod figura_literal_test;

#[cfg(test)]
#[path = "figura_test.rs"]
mod figura_test;

#[cfg(test)]
#[path = "file_interface_test.rs"]
mod file_interface_test;

#[cfg(test)]
#[path = "format_type_test.rs"]
mod format_type_test;

#[cfg(test)]
#[path = "function_facts_propagate_worklist_test.rs"]
mod function_facts_propagate_worklist_test;

#[cfg(test)]
#[path = "function_facts_test.rs"]
mod function_facts_test;

#[cfg(test)]
#[path = "index_infer_test.rs"]
mod index_infer_test;

#[cfg(test)]
#[path = "index_test.rs"]
mod index_test;

#[cfg(test)]
#[path = "index_unify_test.rs"]
mod index_unify_test;

#[cfg(test)]
#[path = "init_state_test.rs"]
mod init_state_test;

#[cfg(test)]
#[path = "instans_precision_test.rs"]
mod instans_precision_test;

#[cfg(test)]
#[path = "lista_tensor_conversio_test.rs"]
mod lista_tensor_conversio_test;

#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_test;

#[cfg(test)]
#[path = "numeric_width_test.rs"]
mod numeric_width_test;

#[cfg(test)]
#[path = "nucleum_lane_test.rs"]
mod nucleum_lane_test;

#[cfg(test)]
#[path = "radix_lane_test.rs"]
mod radix_lane_test;

#[cfg(test)]
#[path = "scope_test.rs"]
mod scope_test;

#[cfg(test)]
#[path = "tensor_bracket_test.rs"]
mod tensor_bracket_test;

#[cfg(test)]
#[path = "tensor_element_lattice_test.rs"]
mod tensor_element_lattice_test;

#[cfg(test)]
#[path = "template_holes_test.rs"]
mod template_holes_test;

#[cfg(test)]
#[path = "types_test.rs"]
mod types_test;

#[cfg(test)]
#[path = "vector_width_test.rs"]
mod vector_width_test;
