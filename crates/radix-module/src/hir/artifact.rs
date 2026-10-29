//! HIR artifact loader for deserializing pre-analyzed compilation units.
//!
//! This module owns the public entry point for loading a
//! [`radix_hir_fhir::HirArtifact`] from a `postcard` binary and converting it
//! into an [`AnalyzedModule`]. The schema version gate (F3) and
//! referential-integrity validation (U2.3) run inside the `radix-hir-fhir`
//! format crate; this module only reconstructs arenas from the validated
//! artifact (U2.2).

use crate::driver::AnalyzedModule;
use crate::driver::GraphicsSourceFacts;
use crate::semantic::{
    AnnotationContractMetadata, CaptureFact, CaptureMode, FunctionFact, FunctionFactTable, FunctionProvenanceTable,
    QualifiedIdentityTable, ResolvedUse, ResolvedUseIndex, ResolvedUseKey, ResolvedUseKind, Resolver,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir_fhir::{
    CaptureFactWire, CaptureModeWire, CliProgramWire, FunctionFactTableWire, FunctionFactWire, GpuBuiltinWire,
    HirArtifactError, RadixLaneWire, ResolvedUseKeyWire, ResolvedUseKindWire, ResolvedUseWire,
};
use radix_lexer::Interner;
use radix_types::TypeTable;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Wire → live type mappings (F18, F20)
// ---------------------------------------------------------------------------

/// Map a wire GPU kernel builtin to the live `MirKernelBuiltin` (F13, F20).
fn map_gpu_builtin(wire: GpuBuiltinWire) -> radix_types::MirKernelBuiltin {
    match wire {
        GpuBuiltinWire::GlobalInvocationX => radix_types::MirKernelBuiltin::GlobalInvocationX,
        GpuBuiltinWire::GlobalInvocationY => radix_types::MirKernelBuiltin::GlobalInvocationY,
        GpuBuiltinWire::GlobalInvocationZ => radix_types::MirKernelBuiltin::GlobalInvocationZ,
        GpuBuiltinWire::GlobalInvocationId => radix_types::MirKernelBuiltin::GlobalInvocationId,
        GpuBuiltinWire::WorkgroupIdX => radix_types::MirKernelBuiltin::WorkgroupIdX,
        GpuBuiltinWire::WorkgroupIdY => radix_types::MirKernelBuiltin::WorkgroupIdY,
        GpuBuiltinWire::WorkgroupIdZ => radix_types::MirKernelBuiltin::WorkgroupIdZ,
        GpuBuiltinWire::WorkgroupId => radix_types::MirKernelBuiltin::WorkgroupId,
        GpuBuiltinWire::LocalInvocationIdX => radix_types::MirKernelBuiltin::LocalInvocationIdX,
        GpuBuiltinWire::LocalInvocationIdY => radix_types::MirKernelBuiltin::LocalInvocationIdY,
        GpuBuiltinWire::LocalInvocationIdZ => radix_types::MirKernelBuiltin::LocalInvocationIdZ,
        GpuBuiltinWire::LocalInvocationId => radix_types::MirKernelBuiltin::LocalInvocationId,
        GpuBuiltinWire::WorkgroupSizeX => radix_types::MirKernelBuiltin::WorkgroupSizeX,
        GpuBuiltinWire::WorkgroupSizeY => radix_types::MirKernelBuiltin::WorkgroupSizeY,
        GpuBuiltinWire::WorkgroupSizeZ => radix_types::MirKernelBuiltin::WorkgroupSizeZ,
    }
}

fn map_tensor_edge_policy(wire: radix_hir_fhir::ResolvedTensorEdgePolicyWire) -> radix_hir::ResolvedTensorEdgePolicy {
    match wire {
        radix_hir_fhir::ResolvedTensorEdgePolicyWire::Builtin(policy) => {
            radix_hir::ResolvedTensorEdgePolicy::Builtin(policy)
        }
        radix_hir_fhir::ResolvedTensorEdgePolicyWire::Custom { policy_def, remap, transform } => {
            radix_hir::ResolvedTensorEdgePolicy::Custom { policy_def, remap, transform }
        }
        radix_hir_fhir::ResolvedTensorEdgePolicyWire::Deferred { policy_type } => {
            radix_hir::ResolvedTensorEdgePolicy::Deferred { policy_type }
        }
    }
}

fn map_tensor_shift_context(wire: radix_hir_fhir::TensorShiftContextWire) -> radix_hir::TensorShiftContext {
    radix_hir::TensorShiftContext { source_shape: wire.source_shape, shift_expression: wire.shift_expression }
}

fn map_tensor_edge_policy_table(wire: radix_hir_fhir::TensorEdgePolicyTableWire) -> radix_hir::TensorEdgePolicyTable {
    radix_hir::TensorEdgePolicyTable {
        calls: wire
            .calls
            .into_iter()
            .map(|row| (row.call, map_tensor_edge_policy(row.policy)))
            .collect(),
        shifted_expressions: wire.shifted_expressions.into_iter().collect(),
        shifted_view_expressions: wire
            .shifted_view_expressions
            .into_iter()
            .map(|(id, context)| (id, map_tensor_shift_context(context)))
            .collect(),
        shifted_bindings: wire.shifted_bindings.into_iter().collect(),
        shifted_view_bindings: wire
            .shifted_view_bindings
            .into_iter()
            .map(|(id, context)| (id, map_tensor_shift_context(context)))
            .collect(),
        limes_contexts: wire
            .limes_contexts
            .into_iter()
            .map(|(id, context)| (id, map_tensor_shift_context(context)))
            .collect(),
        function_requirements: wire
            .function_requirements
            .into_iter()
            .map(|row| {
                (
                    row.function,
                    row.requirements
                        .into_iter()
                        .map(|requirement| radix_hir::TensorEdgePolicyRequirement {
                            policy_type: requirement.policy_type,
                            element_type: requirement.element_type,
                            rank: requirement.rank,
                        })
                        .collect(),
                )
            })
            .collect(),
        provider_impls: wire
            .provider_impls
            .into_iter()
            .map(|provider| radix_hir::TensorEdgePolicyProviderImpl {
                policy_type: provider.policy_type,
                element_type: provider.element_type,
                rank: provider.rank,
                policy: map_tensor_edge_policy(provider.policy),
            })
            .collect(),
        ordo_order: radix_hir::OrdoOrderTable {
            comparisons: wire
                .ordo_order
                .into_iter()
                .map(|row| (row.lhs, row.rows))
                .collect(),
        },
    }
}

/// Map a wire resolved-use key to the live type (F20).
fn map_resolved_use_key(wire: ResolvedUseKeyWire) -> ResolvedUseKey {
    match wire {
        ResolvedUseKeyWire::Local(def_id) => ResolvedUseKey::Local(def_id),
        ResolvedUseKeyWire::Portable(key) => ResolvedUseKey::Portable(key),
    }
}

/// Map a wire resolved-use kind to the live type (F20).
fn map_resolved_use_kind(wire: ResolvedUseKindWire) -> ResolvedUseKind {
    match wire {
        ResolvedUseKindWire::Path => ResolvedUseKind::Path,
        ResolvedUseKindWire::Call => ResolvedUseKind::Call,
        ResolvedUseKindWire::MethodCall => ResolvedUseKind::MethodCall,
        ResolvedUseKindWire::TypeRef => ResolvedUseKind::TypeRef,
        ResolvedUseKindWire::ImportBinding => ResolvedUseKind::ImportBinding,
    }
}

/// Map a wire resolved use to the live type (F20).
fn map_resolved_use(wire: ResolvedUseWire) -> ResolvedUse {
    ResolvedUse {
        key: map_resolved_use_key(wire.key),
        kind: map_resolved_use_kind(wire.kind),
        enclosing: wire.enclosing,
        local: wire.local,
    }
}

/// Map a wire capture mode to the live type (F20).
fn map_capture_mode(wire: CaptureModeWire) -> CaptureMode {
    match wire {
        CaptureModeWire::Read => CaptureMode::Read,
        CaptureModeWire::Mutate => CaptureMode::Mutate,
        CaptureModeWire::Consume => CaptureMode::Consume,
    }
}

/// Map a wire capture fact to the live type (F20).
fn map_capture_fact(wire: CaptureFactWire) -> CaptureFact {
    CaptureFact { def_id: wire.def_id, mode: map_capture_mode(wire.mode) }
}

/// Map a wire function fact to the live type (F20).
fn map_function_fact(wire: FunctionFactWire) -> FunctionFact {
    FunctionFact {
        direct_failure: wire.has_direct_failure,
        may_fail: wire.may_fail,
        is_async: wire.is_async,
        is_generator: wire.is_generator,
        requires_await: wire.requires_await,
        call_edges: wire.call_edges.into_iter().collect::<FxHashSet<_>>(),
        captures: wire.captures.into_iter().map(map_capture_fact).collect(),
        param_modes: wire.param_modes.into_iter().collect::<FxHashMap<_, _>>(),
        err_ty: wire.err_ty,
    }
}

/// Map a wire function-fact table to the live type (F20).
fn map_function_fact_table(wire: FunctionFactTableWire) -> FunctionFactTable {
    FunctionFactTable::from_raw(
        wire.entries
            .into_iter()
            .map(|(def_id, fact)| (def_id, map_function_fact(fact)))
            .collect(),
        wire.entry.map(map_function_fact),
    )
}

/// Map a wire provenance table to the live type (SCR-04).
fn map_provenance_table(wire: radix_hir_fhir::FunctionProvenanceTableWire) -> FunctionProvenanceTable {
    use radix_hir_fhir::{FunctionProvenanceWire, LastUseWire, RegionRelationshipWire, ReturnedBorrowWire};
    use radix_types::{RegionRelationship, ReturnedBorrowProvenance};

    fn map_row(row: FunctionProvenanceWire) -> crate::semantic::FunctionProvenanceSummary {
        crate::semantic::FunctionProvenanceSummary {
            function: row.function,
            returned_borrow: match row.returned_borrow {
                ReturnedBorrowWire::None => ReturnedBorrowProvenance::None,
                ReturnedBorrowWire::Parameter(def_id) => ReturnedBorrowProvenance::Parameter(def_id),
                ReturnedBorrowWire::Definition(def_id) => ReturnedBorrowProvenance::Definition(def_id),
            },
            required_regions: row
                .required_regions
                .into_iter()
                .map(|rel: RegionRelationshipWire| RegionRelationship { from: rel.from, to: rel.to })
                .collect(),
            last_uses: row
                .last_uses
                .into_iter()
                .map(|fact: LastUseWire| crate::semantic::LastUseFact { borrower: fact.borrower, site: fact.site })
                .collect(),
        }
    }

    FunctionProvenanceTable::from_rows(wire.functions.into_iter().map(map_row).collect())
}

/// Map a wire CLI program to the live `CliProgram` (F18).
fn map_cli_program(wire: CliProgramWire) -> crate::cli::CliProgram {
    use radix_hir_fhir::{
        CliCommandWire, CliDefaultWire, CliExitWire, CliModeWire, CliOperandWire, CliOptionWire, CliTypeWire,
    };

    fn map_cli_mode(w: CliModeWire) -> crate::cli::CliMode {
        match w {
            CliModeWire::NotCli => crate::cli::CliMode::NotCli,
            CliModeWire::SingleCommand => crate::cli::CliMode::SingleCommand,
            CliModeWire::Subcommand => crate::cli::CliMode::Subcommand,
        }
    }

    fn map_cli_type(w: CliTypeWire) -> crate::cli::CliType {
        match w {
            CliTypeWire::Textus => crate::cli::CliType::Textus,
            CliTypeWire::Numerus => crate::cli::CliType::Numerus,
            CliTypeWire::Fractus => crate::cli::CliType::Fractus,
            CliTypeWire::Bivalens => crate::cli::CliType::Bivalens,
            CliTypeWire::Octeti => crate::cli::CliType::Octeti,
            CliTypeWire::Ignotum => crate::cli::CliType::Ignotum,
            CliTypeWire::ListaTextus => crate::cli::CliType::ListaTextus,
            CliTypeWire::ListaNumerus => crate::cli::CliType::ListaNumerus,
        }
    }

    fn map_cli_default(w: CliDefaultWire) -> crate::cli::CliDefault {
        match w {
            CliDefaultWire::Text(s) => crate::cli::CliDefault::Text(s),
            CliDefaultWire::Integer(i) => crate::cli::CliDefault::Integer(i),
            CliDefaultWire::Float(f) => crate::cli::CliDefault::Float(f),
            CliDefaultWire::Bool(b) => crate::cli::CliDefault::Bool(b),
            CliDefaultWire::Nil => crate::cli::CliDefault::Nil,
            CliDefaultWire::Expr(s) => crate::cli::CliDefault::Expr(s),
        }
    }

    fn map_cli_exit(w: CliExitWire) -> crate::cli::CliExit {
        match w {
            CliExitWire::Fixed(i) => crate::cli::CliExit::Fixed(i),
            CliExitWire::Binding(s) => crate::cli::CliExit::Binding(s),
            CliExitWire::Field { object, field } => crate::cli::CliExit::Field { object, field },
            CliExitWire::Unsupported => crate::cli::CliExit::Unsupported,
        }
    }

    fn map_cli_option(w: CliOptionWire) -> crate::cli::CliOption {
        crate::cli::CliOption {
            binding: w.binding,
            binding_symbol: w.binding_symbol,
            ty: map_cli_type(w.ty),
            short: w.short,
            long: w.long,
            description: w.description,
            global: w.global,
            default: w.default.map(map_cli_default),
            flag: w.is_flag,
            span: radix_lexer::Span::default(),
        }
    }

    fn map_cli_operand(w: CliOperandWire) -> crate::cli::CliOperand {
        crate::cli::CliOperand {
            binding: w.binding,
            binding_symbol: w.binding_symbol,
            ty: map_cli_type(w.ty),
            rest: w.is_rest,
            description: w.description,
            global: w.global,
            default: w.default.map(map_cli_default),
            span: radix_lexer::Span::default(),
        }
    }

    fn map_cli_command(w: CliCommandWire) -> crate::cli::CliCommand {
        crate::cli::CliCommand {
            path: w.path,
            module_path: w.module_path,
            function: w.function,
            function_symbol: w.function_symbol,
            args_binding: w.args_binding,
            aliases: w.aliases,
            description: w.description,
            options: w.options.into_iter().map(map_cli_option).collect(),
            operands: w.operands.into_iter().map(map_cli_operand).collect(),
            span: radix_lexer::Span::default(),
        }
    }

    crate::cli::CliProgram {
        name: wire.name,
        entry_args: wire.entry_args,
        mode: map_cli_mode(wire.mode),
        version: wire.version,
        description: wire.description,
        global_options: wire
            .global_options
            .into_iter()
            .map(map_cli_option)
            .collect(),
        global_operands: wire
            .global_operands
            .into_iter()
            .map(map_cli_operand)
            .collect(),
        options: wire.options.into_iter().map(map_cli_option).collect(),
        operands: wire.operands.into_iter().map(map_cli_operand).collect(),
        commands: wire.commands.into_iter().map(map_cli_command).collect(),
        exit: wire.exit.map(map_cli_exit),
    }
}

// ---------------------------------------------------------------------------
// load_fhir
// ---------------------------------------------------------------------------

/// Deserialize a [`radix_hir_fhir::HirArtifact`] from a `postcard` byte slice
/// and return a fully reconstructed [`AnalyzedModule`].
///
/// # Schema versioning (F3)
///
/// `schema_version` is the first wire field, so the loader reads the version
/// prefix before interpreting the payload and rejects any mismatch — forward
/// or backward — with [`HirArtifactError::UnsupportedSchemaVersion`]. The
/// schema version ratchets on every breaking HIR change; there is no
/// compatibility decoder for earlier schemas.
///
/// # Arena reconstruction (U2.2)
///
/// Reconstructs all arenas from their snapshot forms:
/// - [`TypeTable`] from [`radix_types::TypeTableSnapshot`] (caches rebuilt per CTM E.2)
/// - [`Interner`] from ordered `Vec<String>` (stable `Symbol(u32)` indices)
/// - [`Resolver`] from [`radix_hir_fhir::ResolverSnapshot`] (builtin `DefId`
///   values re-derived as compiler constants, not deserialized)
/// - [`ResolvedUseIndex`] from `Vec<ResolvedUse>` (lookup maps rebuilt)
/// - Wire DTOs mapped to live types (F18, F20)
///
/// # Errors
///
/// Returns [`HirArtifactError`] for any deserialization, truncation,
/// schema-mismatch, or dangling-index problem. Never panics on the load
/// path (F7).
pub fn load_fhir(bytes: &[u8]) -> Result<AnalyzedModule, HirArtifactError> {
    // F3 schema-version gate and U2.3 referential-integrity validation run
    // inside the format crate's decode; only arena reconstruction remains
    // here (U2.2).
    let artifact = radix_hir_fhir::decode(bytes)?;
    reconstruct_unit(artifact)
}

/// Reconstruct an [`AnalyzedModule`] from a validated
/// [`radix_hir_fhir::HirArtifact`] (U2.2).
///
/// Shared by the single-unit loader and the FHIR package loader
/// (`crate::hir::package`), which reconstructs every embedded unit through
/// this same path so package units and standalone units rebuild identically.
pub(crate) fn reconstruct_unit(artifact: radix_hir_fhir::HirArtifact) -> Result<AnalyzedModule, HirArtifactError> {
    // TypeTable: rebuild from snapshot (CTM E.2), then restore the typecheck
    // binder's resolved generic call vectors (schema v10) so specialization
    // reads the original explicit/inferred/defaulted facts instead of
    // re-running inference.
    let mut types = TypeTable::from_snapshot(artifact.types)
        .map_err(|msg| HirArtifactError::MalformedSnapshot { kind: "types", detail: msg })?;
    types.resolved_generic_calls = artifact.resolved_generic_calls;
    types.ordering_contract = artifact.contract_roles.ordering;
    types.ordering_contract_bindings = artifact
        .contract_roles
        .ordering_bindings
        .into_iter()
        .collect();
    types.closing_contract = artifact.contract_roles.closing;
    types.sinking_contract = artifact.contract_roles.sinking;
    types.sinking_contract_bindings = artifact
        .contract_roles
        .sinking_bindings
        .into_iter()
        .collect();

    // Interner: rebuild from ordered string table.
    let interner = Interner::from_string_table(artifact.interner);

    // Resolver: rebuild from snapshot (F5, F19). Builtins are compiler constants.
    let resolver = Resolver::from_snapshot(artifact.resolver);

    // ResolvedUseIndex: rebuild lookup maps from uses vector (F20).
    let resolved_uses: Vec<ResolvedUse> = artifact
        .resolved_uses
        .into_iter()
        .map(map_resolved_use)
        .collect();
    let resolved_uses = ResolvedUseIndex::from_snapshot(resolved_uses);

    // FunctionFactTable: map wire → live (F20).
    let function_facts = map_function_fact_table(artifact.function_facts);

    // CliProgram: map wire → live (F18).
    let cli_program = artifact.cli_program.map(map_cli_program);

    // GPU builtins: map wire → live (F13, F20).
    let gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)> = artifact
        .gpu_builtins
        .into_iter()
        .map(|(def_id, builtin)| (def_id, map_gpu_builtin(builtin)))
        .collect();

    // Capability provenance: map wire → live (SCR-04).
    let provenance = map_provenance_table(artifact.capability_provenance);

    let analysis_stamp = crate::semantic::AnalysisStamp::from_persisted(artifact.analysis_stamp);

    let radix_lanes =
        crate::semantic::RadixLaneMetadata::from_snapshot(artifact.radix_lanes.into_iter().map(|entry| {
            let lane = match entry.lane {
                RadixLaneWire::Air => crate::semantic::RadixLane::Air,
                RadixLaneWire::Mir => crate::semantic::RadixLane::Mir,
                RadixLaneWire::HirDirect => crate::semantic::RadixLane::HirDirect,
            };
            let backward = entry
                .backward
                .map(|backward| crate::semantic::RadixBackward {
                    companion_name: backward.companion_name,
                    companion_def_id: backward.companion_def_id,
                    selected_params: backward.selected_params,
                });
            (entry.def_id, lane, backward)
        }));

    // The kernel requirement wire carries no tier; the tier lives on the HIR
    // annotations, so read it back from them before the HIR moves.
    let kernel_tiers = crate::semantic::passes::kernel_requirements::declared_kernel_tiers(&artifact.hir, &interner);

    let provider_units = provider_units_live(artifact.provider_units)?;
    let tensor_edge_policies = map_tensor_edge_policy_table(artifact.tensor_edge_policies);

    let mut unit = AnalyzedModule::from_loaded_with_presentation(
        analysis_stamp,
        interner,
        types,
        resolver,
        artifact.hir,
        artifact.presentation,
        cli_program,
        artifact.libraries,
        AnnotationContractMetadata::default(),
        function_facts,
        resolved_uses,
        QualifiedIdentityTable::default(),
        provenance,
        radix_lanes,
        gpu_builtins,
        tensor_edge_policies,
        GraphicsSourceFacts::default(),
        Vec::new(), // diagnostics
    );
    // Reinstall the provider carriage (schema v14, ARCH-U4): the loaded
    // surface must emit provider imports from carried units exactly as the
    // direct surface does, with no disk re-derivation.
    unit.provider_units = provider_units;
    unit.shape_obligations = map_shape_obligations(artifact.shape_obligations);
    unit.kernel_requirements = map_kernel_requirements(artifact.kernel_requirements);
    unit.kernel_requirements.apply_tiers(&kernel_tiers);
    // Program display tokens follow the unit's code locale (schema v44):
    // restore the words the author's locale prints so a loaded unit does not
    // fall back to the Latin defaults.
    unit.display_tokens = crate::runtime_contract::display_tokens::DisplayTokens {
        true_: artifact.display_tokens.true_,
        false_: artifact.display_tokens.false_,
        none: artifact.display_tokens.none,
        tuple: artifact.display_tokens.tuple,
    };
    Ok(unit)
}

fn map_kernel_requirements(
    wire: radix_hir_fhir::KernelRequirementsWire,
) -> crate::semantic::passes::kernel_requirements::KernelRequirementsReport {
    use crate::semantic::passes::kernel_requirements as live;

    live::KernelRequirementsReport {
        kernels: wire
            .kernels
            .into_iter()
            .map(|kernel| live::KernelRequirement {
                def_id: kernel.def_id,
                name: kernel.name,
                tier: radix_hir::KernelTier::Default,
                target_reports: kernel
                    .target_reports
                    .into_iter()
                    .map(|report| live::KernelTargetRequirement {
                        target: match report.target {
                            radix_hir_fhir::KernelTargetWire::MirAmd => crate::codegen::Target::MirAmd,
                            radix_hir_fhir::KernelTargetWire::MirMetal => crate::codegen::Target::MirMetal,
                            radix_hir_fhir::KernelTargetWire::MirWgsl => crate::codegen::Target::MirWgsl,
                        },
                        admitted: report.admitted,
                        recursive: report.recursive,
                        callees: report.callees,
                        widths: report
                            .widths
                            .into_iter()
                            .map(|width| live::KernelWidthRequirement {
                                span: width.span,
                                primitive: match width.primitive {
                                    radix_hir_fhir::KernelPrimitiveWire::Numerus => crate::semantic::Primitive::Numerus,
                                    radix_hir_fhir::KernelPrimitiveWire::Fractus => crate::semantic::Primitive::Fractus,
                                },
                                width: width.width,
                                policy: match width.policy {
                                    radix_hir_fhir::KernelNumericPolicyWire::Exact => live::KernelNumericPolicy::Exact,
                                    radix_hir_fhir::KernelNumericPolicyWire::Wrapping => {
                                        live::KernelNumericPolicy::Wrapping
                                    }
                                    radix_hir_fhir::KernelNumericPolicyWire::Saturating => {
                                        live::KernelNumericPolicy::Saturating
                                    }
                                    radix_hir_fhir::KernelNumericPolicyWire::PolicyFloat => {
                                        live::KernelNumericPolicy::PolicyFloat
                                    }
                                },
                            })
                            .collect(),
                        extent_conditions: report
                            .extent_conditions
                            .into_iter()
                            .map(|condition| live::KernelExtentCondition {
                                span: condition.span,
                                range: map_kernel_value_range(condition.range),
                                minimum: condition.minimum,
                                maximum: condition.maximum,
                            })
                            .collect(),
                        trapping_stores: report
                            .trapping_stores
                            .into_iter()
                            .map(|store| live::KernelTrappingStore { span: store.span, width: store.width })
                            .collect(),
                        failures: report
                            .failures
                            .into_iter()
                            .map(map_kernel_requirement_failure)
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
        regions: Vec::new(),
    }
}

fn map_kernel_value_range(
    wire: radix_hir_fhir::KernelValueRangeWire,
) -> crate::semantic::passes::kernel_requirements::KernelValueRange {
    crate::semantic::passes::kernel_requirements::KernelValueRange {
        minimum: map_kernel_range_expr(wire.minimum),
        maximum: map_kernel_range_expr(wire.maximum),
    }
}

fn map_kernel_range_expr(
    wire: radix_hir_fhir::KernelRangeExprWire,
) -> crate::semantic::passes::kernel_requirements::KernelRangeExpr {
    use crate::semantic::passes::kernel_requirements::KernelRangeExpr as Live;
    match wire {
        radix_hir_fhir::KernelRangeExprWire::Constant(value) => Live::Constant(value),
        radix_hir_fhir::KernelRangeExprWire::Shape(index) => Live::Shape(index),
        radix_hir_fhir::KernelRangeExprWire::Add(lhs, rhs) => {
            Live::Add(Box::new(map_kernel_range_expr(*lhs)), Box::new(map_kernel_range_expr(*rhs)))
        }
        radix_hir_fhir::KernelRangeExprWire::Subtract(lhs, rhs) => {
            Live::Subtract(Box::new(map_kernel_range_expr(*lhs)), Box::new(map_kernel_range_expr(*rhs)))
        }
        radix_hir_fhir::KernelRangeExprWire::Multiply(lhs, rhs) => {
            Live::Multiply(Box::new(map_kernel_range_expr(*lhs)), Box::new(map_kernel_range_expr(*rhs)))
        }
        radix_hir_fhir::KernelRangeExprWire::FloorDivide(lhs, rhs) => {
            Live::FloorDivide(Box::new(map_kernel_range_expr(*lhs)), Box::new(map_kernel_range_expr(*rhs)))
        }
        radix_hir_fhir::KernelRangeExprWire::Minimum(lhs, rhs) => {
            Live::Minimum(Box::new(map_kernel_range_expr(*lhs)), Box::new(map_kernel_range_expr(*rhs)))
        }
        radix_hir_fhir::KernelRangeExprWire::Maximum(lhs, rhs) => {
            Live::Maximum(Box::new(map_kernel_range_expr(*lhs)), Box::new(map_kernel_range_expr(*rhs)))
        }
        radix_hir_fhir::KernelRangeExprWire::Parameter(def_id) => Live::Parameter(def_id),
    }
}

fn map_kernel_requirement_failure(
    wire: radix_hir_fhir::KernelRequirementFailureWire,
) -> crate::semantic::passes::kernel_requirements::KernelRequirementFailure {
    use crate::semantic::passes::kernel_requirements::KernelRequirementFailure as Live;
    match wire {
        radix_hir_fhir::KernelRequirementFailureWire::RecursiveCall => Live::RecursiveCall,
        radix_hir_fhir::KernelRequirementFailureWire::CalleeNotAdmitted { callee } => {
            Live::CalleeNotAdmitted { callee }
        }
        radix_hir_fhir::KernelRequirementFailureWire::UnsupportedIntegerWidth { span, width } => {
            Live::UnsupportedIntegerWidth { span, width }
        }
        radix_hir_fhir::KernelRequirementFailureWire::UnsupportedFloatWidth { span, width } => {
            Live::UnsupportedFloatWidth { span, width }
        }
        radix_hir_fhir::KernelRequirementFailureWire::IntermediateOutOfRange { span, range } => {
            Live::IntermediateOutOfRange { span, range: map_kernel_value_range(range) }
        }
        radix_hir_fhir::KernelRequirementFailureWire::StoreOutOfRange { span, range } => {
            Live::StoreOutOfRange { span, range: map_kernel_value_range(range) }
        }
        radix_hir_fhir::KernelRequirementFailureWire::UnsupportedIntegerRange { span } => {
            Live::UnsupportedIntegerRange { span }
        }
        radix_hir_fhir::KernelRequirementFailureWire::DivideByZeroPossible { span } => {
            Live::DivideByZeroPossible { span }
        }
        radix_hir_fhir::KernelRequirementFailureWire::NegativeShiftCountPossible { span } => {
            Live::NegativeShiftCountPossible { span }
        }
        radix_hir_fhir::KernelRequirementFailureWire::TensorEdgePolicyDeferred { span } => {
            Live::TensorEdgePolicyDeferred { span }
        }
        radix_hir_fhir::KernelRequirementFailureWire::SaturatingLoweringDeferred { span, width } => {
            Live::SaturatingLoweringDeferred { span, width }
        }
        radix_hir_fhir::KernelRequirementFailureWire::OptionFRequired { span, width } => {
            Live::OptionFRequired { span, width }
        }
    }
}

fn map_shape_obligations(wire: radix_hir_fhir::ShapeObligationTableWire) -> crate::semantic::ShapeObligationTable {
    let map_relations = |relations: Vec<radix_hir_fhir::ShapeRelationWire>| {
        relations
            .into_iter()
            .map(|relation| crate::semantic::ShapeRelation {
                left: relation.left,
                right: relation.right,
                span: relation.span,
                boundary: match relation.boundary {
                    radix_hir_fhir::ShapeRelationBoundaryWire::Call => crate::semantic::ShapeRelationBoundary::Call,
                    radix_hir_fhir::ShapeRelationBoundaryWire::FunctionEntry => {
                        crate::semantic::ShapeRelationBoundary::FunctionEntry
                    }
                    radix_hir_fhir::ShapeRelationBoundaryWire::FunctionReturn => {
                        crate::semantic::ShapeRelationBoundary::FunctionReturn
                    }
                    radix_hir_fhir::ShapeRelationBoundaryWire::Store => crate::semantic::ShapeRelationBoundary::Store,
                },
            })
            .collect()
    };
    let map_bindings = |bindings: Vec<radix_hir_fhir::RuntimeSizeBindingWire>| {
        bindings
            .into_iter()
            .map(|binding| crate::semantic::RuntimeSizeBinding {
                parameter: binding.parameter,
                witness: binding.witness,
                span: binding.span,
            })
            .collect()
    };
    crate::semantic::ShapeObligationTable {
        calls: wire
            .calls
            .into_iter()
            .map(|entry| (entry.call, map_relations(entry.relations)))
            .collect(),
        call_size_bindings: wire
            .call_size_bindings
            .into_iter()
            .map(|entry| (entry.call, map_bindings(entry.bindings)))
            .collect(),
        functions: wire
            .functions
            .into_iter()
            .map(|entry| (entry.function, map_relations(entry.relations)))
            .collect(),
        stores: wire
            .stores
            .into_iter()
            .map(|entry| (entry.initializer, map_relations(entry.relations)))
            .collect(),
        function_size_bindings: wire
            .function_size_bindings
            .into_iter()
            .map(|entry| (entry.function, map_bindings(entry.bindings)))
            .collect(),
        proven_indexes: wire.proven_indexes.into_iter().collect(),
    }
}

/// Rebuild a live [`crate::driver::ProviderUnitView`] from its wire form,
/// recursing through each fragment's nested view. Diagnostics are not wire
/// facts: carriage units carry warnings-only diagnostics by construction and
/// the emitter's fail-closed error check reads them only for `Failed`-free
/// entries, so loaded carriage units reconstruct with empty diagnostics — the
/// same treatment the loader gives the consumer unit's own diagnostics.
fn provider_units_live(
    wire: BTreeMap<String, radix_hir_fhir::ProviderCarriageEntryWire>,
) -> Result<crate::driver::ProviderUnitView, HirArtifactError> {
    use crate::driver::{CarriedProviderUnit, ProviderUnit};

    let mut units = BTreeMap::new();
    for (import_path, entry) in wire {
        let carried = match entry {
            radix_hir_fhir::ProviderCarriageEntryWire::Failed { message } => CarriedProviderUnit::Failed { message },
            radix_hir_fhir::ProviderCarriageEntryWire::Unit(fragment) => {
                let fragment = *fragment;
                let mut types = TypeTable::from_snapshot(fragment.types)
                    .map_err(|msg| HirArtifactError::MalformedSnapshot { kind: "types", detail: msg })?;
                types.resolved_generic_calls = fragment.resolved_generic_calls;
                let nested = provider_units_live(fragment.nested)?;
                CarriedProviderUnit::Unit(std::sync::Arc::new(std::sync::Mutex::new(ProviderUnit {
                    hir: fragment.hir,
                    types,
                    interner: Interner::from_string_table(fragment.interner),
                    resolver: Resolver::from_snapshot(fragment.resolver),
                    tensor_edge_policies: map_tensor_edge_policy_table(fragment.tensor_edge_policies),
                    shape_obligations: map_shape_obligations(fragment.shape_obligations),
                    diagnostics: Vec::new(),
                    nested,
                })))
            }
        };
        units.insert(import_path, carried);
    }
    Ok(crate::driver::ProviderUnitView { units })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "artifact_test.rs"]
mod tests;
