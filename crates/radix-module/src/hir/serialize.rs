//! HIR artifact serialization from analyzed units.
//!
//! Public constructor for creating a [`radix_hir_fhir::HirArtifact`]
//! from an analyzed unit. Serialization into `postcard` bytes and writing
//! to disk happen at the call site.

use crate::driver::AnalyzedModule;
use crate::hir::presentation_witness::normalize_presentation_witnesses;
use radix_hir_fhir::{
    CaptureFactWire, CaptureModeWire, CliCommandWire, CliDefaultWire, CliExitWire, CliModeWire, CliOperandWire,
    CliOptionWire, CliProgramWire, CliTypeWire, DisplayTokensWire, FunctionFactTableWire, FunctionFactWire,
    FunctionProvenanceTableWire, FunctionProvenanceWire, GpuBuiltinWire, HirArtifact, KernelExtentConditionWire,
    KernelNumericPolicyWire, KernelPrimitiveWire, KernelRangeExprWire, KernelRequirementFailureWire,
    KernelRequirementWire, KernelRequirementsWire, KernelTargetRequirementWire, KernelTargetWire,
    KernelTrappingStoreWire, KernelValueRangeWire, KernelWidthRequirementWire, LastUseWire, OrdoOrderComparisonWire,
    RadixBackwardWire, RadixLaneEntryWire, RadixLaneWire, RegionRelationshipWire, ResolvedTensorEdgePolicyWire,
    ResolvedUseKeyWire, ResolvedUseKindWire, ResolvedUseWire, ReturnedBorrowWire, SCHEMA_VERSION, SourceIdentity,
    TensorEdgePolicyCallWire, TensorEdgePolicyFunctionRequirementsWire, TensorEdgePolicyProviderImplWire,
    TensorEdgePolicyRequirementWire, TensorEdgePolicyTableWire, TensorShiftContextWire,
};
use std::collections::BTreeMap;

// ── Entry point ───────────────────────────────────────────────────────────

/// Build a [`HirArtifact`] from a fully analyzed unit.
///
/// Populates all serializable fields: HIR program, type table snapshot,
/// interner table, resolver snapshot, CLI program (F18), function facts,
/// resolved uses, analysis stamp, and GPU builtins.
///
/// The `content_hash` should be a hex-encoded SHA-256 of the source bytes.
/// The `relative_path` should be a path relative to the project root.
pub fn from_analyzed(unit: &AnalyzedModule, content_hash: &str, relative_path: &str) -> HirArtifact {
    // HIR finalization may canonicalize a compound witness or add an access
    // marker to a local without rewriting the presentation sidecar. Reconcile
    // the sidecar at the artifact boundary so schema-v9 validation sees the
    // same finalized witnesses as HIR while retaining authored source shape.
    let mut presentation = unit.presentation.clone();
    normalize_presentation_witnesses(&unit.hir, &mut presentation, &unit.types);
    record_named_size_args(&mut presentation, &unit.types);

    HirArtifact {
        schema_version: SCHEMA_VERSION,
        shape_hash: radix_hir_fhir::wire_shape_hash(),
        source_identity: SourceIdentity {
            content_hash: content_hash.to_string(),
            relative_path: relative_path.to_string(),
        },
        hir: unit.hir.clone(),
        presentation,
        cli_program: unit.cli_program.as_ref().map(cli_program_wire),
        types: unit.types.snapshot(),
        resolved_generic_calls: unit.types.resolved_generic_calls.clone(),
        radix_lanes: unit
            .radix_lanes
            .snapshot()
            .into_iter()
            .map(|(def_id, lane, backward)| RadixLaneEntryWire {
                def_id,
                lane: match lane {
                    crate::semantic::RadixLane::Air => RadixLaneWire::Air,
                    crate::semantic::RadixLane::Mir => RadixLaneWire::Mir,
                    crate::semantic::RadixLane::HirDirect => RadixLaneWire::HirDirect,
                },
                backward: backward.map(|backward| RadixBackwardWire {
                    companion_name: backward.companion_name,
                    companion_def_id: backward.companion_def_id,
                    selected_params: backward.selected_params,
                }),
            })
            .collect(),
        interner: unit.interner.to_string_table(),
        resolver: unit.resolver.to_snapshot(),
        libraries: unit.libraries.clone(),
        function_facts: function_fact_table_to_wire(&unit.function_facts),
        resolved_uses: unit
            .resolved_uses
            .to_snapshot()
            .into_iter()
            .map(|u| resolved_use_to_wire(&u))
            .collect(),
        analysis_stamp: unit.analysis_stamp.get(),
        gpu_builtins: unit
            .gpu_builtins
            .iter()
            .map(|(def_id, builtin)| (*def_id, gpu_builtin_to_wire(*builtin)))
            .collect(),
        capability_provenance: provenance_table_to_wire(&unit.provenance),
        provider_units: provider_units_wire(&unit.provider_units),
        shape_obligations: shape_obligations_wire(&unit.shape_obligations),
        contract_roles: contract_roles_wire(&unit.types),
        kernel_requirements: kernel_requirements_wire(&unit.kernel_requirements),
        tensor_edge_policies: tensor_edge_policies_wire(&unit.tensor_edge_policies),
        display_tokens: DisplayTokensWire {
            true_: unit.display_tokens.true_.clone(),
            false_: unit.display_tokens.false_.clone(),
            none: unit.display_tokens.none.clone(),
            tuple: unit.display_tokens.tuple.clone(),
        },
        // The conversation builtin is compiler-owned: record its definition so
        // the artifact validator can pair an authored `sermo<O, R>` with the
        // bare carrier witness analysis finalizes it to.
        conversation_constructor: unit.resolver.builtin_sermo_def_id(),
    }
}

/// Record the size arguments a size-generic genus application binds
/// (`Box<3>`) in the authored form.
///
/// Lowering captures an authored child form for every *type* argument but not
/// for the magnitude argument a `size` parameter binds, so a `Named` form
/// whose witness is `Type::Applied(_, args)` can reach the artifact missing
/// its size children. The validator pairs each authored `Size` with its
/// witness argument and the Faber re-emitter renders the authored argument
/// list, so re-align the authored list with the witness before serializing.
/// The witness index is authoritative — lowering already resolved the authored
/// figura, exactly as `normalize_size_arg` does for tensor shape arguments.
fn record_named_size_args(presentation: &mut radix_hir::HirPresentation, types: &radix_types::TypeTable) {
    for authored in &mut presentation.authored_types {
        record_named_size_args_in_form(&mut authored.form, types);
    }
}

/// Align one form and every authored type child beneath it, so a size-generic
/// application nested under another form (the `Box<3>` of `Box<3> ∪ none`)
/// carries its size child like a root one does.
fn record_named_size_args_in_form(form: &mut radix_hir::HirTypeForm, types: &radix_types::TypeTable) {
    if matches!(form.head, radix_hir::HirTypeFormHead::Named { .. })
        && let radix_types::Type::Applied(_, applied) = types.get(form.ty)
    {
        let applied = applied.clone();
        align_named_form_args(form, &applied);
    }
    for arg in &mut form.args {
        if let radix_hir::HirTypeFormArg::Type(child) = arg {
            record_named_size_args_in_form(child, types);
        }
    }
}

/// Insert the witness's size arguments into a `Named` form's authored argument
/// list at their declaration positions, leaving the authored type children in
/// order. A size argument the producer already recorded is kept as authored, so
/// repeated serialization is idempotent.
fn align_named_form_args(form: &mut radix_hir::HirTypeForm, applied: &[radix_types::AppliedArg]) {
    let mut authored = std::mem::take(&mut form.args).into_iter().peekable();
    let mut rebuilt = Vec::with_capacity(applied.len());
    for expected in applied {
        match expected {
            radix_types::AppliedArg::Type(_) => {
                if let Some(arg) = authored.next() {
                    rebuilt.push(arg);
                }
            }
            radix_types::AppliedArg::Size(index) => {
                if matches!(authored.peek(), Some(radix_hir::HirTypeFormArg::Size(_))) {
                    if let Some(arg) = authored.next() {
                        rebuilt.push(arg);
                    }
                } else {
                    rebuilt.push(radix_hir::HirTypeFormArg::Size(*index));
                }
            }
        }
    }
    rebuilt.extend(authored);
    form.args = rebuilt;
}

fn resolved_tensor_edge_policy_wire(policy: radix_hir::ResolvedTensorEdgePolicy) -> ResolvedTensorEdgePolicyWire {
    match policy {
        radix_hir::ResolvedTensorEdgePolicy::Builtin(policy) => ResolvedTensorEdgePolicyWire::Builtin(policy),
        radix_hir::ResolvedTensorEdgePolicy::Custom { policy_def, remap, transform } => {
            ResolvedTensorEdgePolicyWire::Custom { policy_def, remap, transform }
        }
        radix_hir::ResolvedTensorEdgePolicy::Deferred { policy_type } => {
            ResolvedTensorEdgePolicyWire::Deferred { policy_type }
        }
    }
}

fn tensor_shift_context_wire(context: radix_hir::TensorShiftContext) -> TensorShiftContextWire {
    TensorShiftContextWire { source_shape: context.source_shape, shift_expression: context.shift_expression }
}

fn tensor_edge_policies_wire(table: &radix_hir::TensorEdgePolicyTable) -> TensorEdgePolicyTableWire {
    let mut calls: Vec<_> = table
        .calls
        .iter()
        .map(|(call, policy)| TensorEdgePolicyCallWire {
            call: *call,
            policy: resolved_tensor_edge_policy_wire(*policy),
        })
        .collect();
    calls.sort_by_key(|row| row.call.0);

    let mut shifted_expressions: Vec<_> = table.shifted_expressions.iter().copied().collect();
    shifted_expressions.sort_by_key(|id| id.0);
    let mut shifted_view_expressions: Vec<_> = table
        .shifted_view_expressions
        .iter()
        .map(|(id, context)| (*id, tensor_shift_context_wire(*context)))
        .collect();
    shifted_view_expressions.sort_by_key(|(id, _)| id.0);
    let mut shifted_bindings: Vec<_> = table.shifted_bindings.iter().copied().collect();
    shifted_bindings.sort_by_key(|id| id.0);
    let mut shifted_view_bindings: Vec<_> = table
        .shifted_view_bindings
        .iter()
        .map(|(id, context)| (*id, tensor_shift_context_wire(*context)))
        .collect();
    shifted_view_bindings.sort_by_key(|(id, _)| id.0);
    let mut limes_contexts: Vec<_> = table
        .limes_contexts
        .iter()
        .map(|(id, context)| (*id, tensor_shift_context_wire(*context)))
        .collect();
    limes_contexts.sort_by_key(|(id, _)| id.0);
    let mut function_requirements: Vec<_> = table
        .function_requirements
        .iter()
        .map(|(function, requirements)| TensorEdgePolicyFunctionRequirementsWire {
            function: *function,
            requirements: requirements
                .iter()
                .map(|requirement| TensorEdgePolicyRequirementWire {
                    policy_type: requirement.policy_type,
                    element_type: requirement.element_type,
                    rank: requirement.rank,
                })
                .collect(),
        })
        .collect();
    function_requirements.sort_by_key(|row| row.function.0);
    let mut provider_impls: Vec<_> = table
        .provider_impls
        .iter()
        .map(|provider| TensorEdgePolicyProviderImplWire {
            policy_type: provider.policy_type,
            element_type: provider.element_type,
            rank: provider.rank,
            policy: resolved_tensor_edge_policy_wire(provider.policy),
        })
        .collect();
    provider_impls.sort_by_key(|row| (row.policy_type.0, row.element_type.0, row.rank));
    let mut ordo_order: Vec<_> = table
        .ordo_order
        .comparisons
        .iter()
        .map(|(lhs, rows)| OrdoOrderComparisonWire { lhs: *lhs, rows: rows.clone() })
        .collect();
    ordo_order.sort_by_key(|row| row.lhs.0);

    TensorEdgePolicyTableWire {
        calls,
        shifted_expressions,
        shifted_view_expressions,
        shifted_bindings,
        shifted_view_bindings,
        limes_contexts,
        function_requirements,
        provider_impls,
        ordo_order,
    }
}

fn kernel_requirements_wire(
    requirements: &crate::semantic::passes::kernel_requirements::KernelRequirementsReport,
) -> KernelRequirementsWire {
    use crate::semantic::passes::kernel_requirements as live;

    KernelRequirementsWire {
        kernels: requirements
            .kernels
            .iter()
            .map(|kernel| KernelRequirementWire {
                def_id: kernel.def_id,
                name: kernel.name.clone(),
                target_reports: kernel
                    .target_reports
                    .iter()
                    .filter_map(|report| Some((kernel_target_wire(report.target)?, report)))
                    .map(|(target, report)| KernelTargetRequirementWire {
                        target,
                        admitted: report.admitted,
                        recursive: report.recursive,
                        callees: report.callees.clone(),
                        widths: report
                            .widths
                            .iter()
                            .filter_map(|width| Some((kernel_primitive_wire(width.primitive)?, width)))
                            .map(|(primitive, width)| KernelWidthRequirementWire {
                                span: width.span,
                                primitive,
                                width: width.width,
                                policy: match width.policy {
                                    live::KernelNumericPolicy::Exact => KernelNumericPolicyWire::Exact,
                                    live::KernelNumericPolicy::Wrapping => KernelNumericPolicyWire::Wrapping,
                                    live::KernelNumericPolicy::Saturating => KernelNumericPolicyWire::Saturating,
                                    live::KernelNumericPolicy::PolicyFloat => KernelNumericPolicyWire::PolicyFloat,
                                },
                            })
                            .collect(),
                        extent_conditions: report
                            .extent_conditions
                            .iter()
                            .map(|condition| KernelExtentConditionWire {
                                span: condition.span,
                                range: kernel_value_range_wire(&condition.range),
                                minimum: condition.minimum,
                                maximum: condition.maximum,
                            })
                            .collect(),
                        trapping_stores: report
                            .trapping_stores
                            .iter()
                            .map(|store| KernelTrappingStoreWire { span: store.span, width: store.width })
                            .collect(),
                        failures: report
                            .failures
                            .iter()
                            .map(kernel_requirement_failure_wire)
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Wire encoding of a kernel report target. Only device targets have one: the
/// kernel-requirements analysis records device targets alone, so any other
/// target has no row on the wire.
fn kernel_target_wire(target: crate::codegen::Target) -> Option<KernelTargetWire> {
    match target {
        crate::codegen::Target::MirAmd => Some(KernelTargetWire::MirAmd),
        crate::codegen::Target::MirMetal => Some(KernelTargetWire::MirMetal),
        crate::codegen::Target::MirWgsl => Some(KernelTargetWire::MirWgsl),
        _ => None,
    }
}

/// Wire encoding of a kernel width primitive. Only the numeric primitives have
/// one; the analysis reports no width for any other primitive.
fn kernel_primitive_wire(primitive: crate::semantic::Primitive) -> Option<KernelPrimitiveWire> {
    match primitive {
        crate::semantic::Primitive::Numerus => Some(KernelPrimitiveWire::Numerus),
        crate::semantic::Primitive::Fractus => Some(KernelPrimitiveWire::Fractus),
        _ => None,
    }
}

fn kernel_value_range_wire(
    range: &crate::semantic::passes::kernel_requirements::KernelValueRange,
) -> KernelValueRangeWire {
    KernelValueRangeWire {
        minimum: kernel_range_expr_wire(&range.minimum),
        maximum: kernel_range_expr_wire(&range.maximum),
    }
}

fn kernel_range_expr_wire(
    expression: &crate::semantic::passes::kernel_requirements::KernelRangeExpr,
) -> KernelRangeExprWire {
    use crate::semantic::passes::kernel_requirements::KernelRangeExpr as Live;
    match expression {
        Live::Constant(value) => KernelRangeExprWire::Constant(*value),
        Live::Shape(index) => KernelRangeExprWire::Shape(*index),
        Live::Add(lhs, rhs) => {
            KernelRangeExprWire::Add(Box::new(kernel_range_expr_wire(lhs)), Box::new(kernel_range_expr_wire(rhs)))
        }
        Live::Subtract(lhs, rhs) => {
            KernelRangeExprWire::Subtract(Box::new(kernel_range_expr_wire(lhs)), Box::new(kernel_range_expr_wire(rhs)))
        }
        Live::Multiply(lhs, rhs) => {
            KernelRangeExprWire::Multiply(Box::new(kernel_range_expr_wire(lhs)), Box::new(kernel_range_expr_wire(rhs)))
        }
        Live::FloorDivide(lhs, rhs) => KernelRangeExprWire::FloorDivide(
            Box::new(kernel_range_expr_wire(lhs)),
            Box::new(kernel_range_expr_wire(rhs)),
        ),
        Live::Minimum(lhs, rhs) => {
            KernelRangeExprWire::Minimum(Box::new(kernel_range_expr_wire(lhs)), Box::new(kernel_range_expr_wire(rhs)))
        }
        Live::Maximum(lhs, rhs) => {
            KernelRangeExprWire::Maximum(Box::new(kernel_range_expr_wire(lhs)), Box::new(kernel_range_expr_wire(rhs)))
        }
        Live::Parameter(def_id) => KernelRangeExprWire::Parameter(*def_id),
    }
}

fn kernel_requirement_failure_wire(
    failure: &crate::semantic::passes::kernel_requirements::KernelRequirementFailure,
) -> KernelRequirementFailureWire {
    use crate::semantic::passes::kernel_requirements::KernelRequirementFailure as Live;
    match failure {
        Live::RecursiveCall => KernelRequirementFailureWire::RecursiveCall,
        Live::CalleeNotAdmitted { callee } => KernelRequirementFailureWire::CalleeNotAdmitted { callee: *callee },
        Live::UnsupportedIntegerWidth { span, width } => {
            KernelRequirementFailureWire::UnsupportedIntegerWidth { span: *span, width: *width }
        }
        Live::UnsupportedFloatWidth { span, width } => {
            KernelRequirementFailureWire::UnsupportedFloatWidth { span: *span, width: *width }
        }
        Live::IntermediateOutOfRange { span, range } => {
            KernelRequirementFailureWire::IntermediateOutOfRange { span: *span, range: kernel_value_range_wire(range) }
        }
        Live::StoreOutOfRange { span, range } => {
            KernelRequirementFailureWire::StoreOutOfRange { span: *span, range: kernel_value_range_wire(range) }
        }
        Live::UnsupportedIntegerRange { span } => KernelRequirementFailureWire::UnsupportedIntegerRange { span: *span },
        Live::DivideByZeroPossible { span } => KernelRequirementFailureWire::DivideByZeroPossible { span: *span },
        Live::NegativeShiftCountPossible { span } => {
            KernelRequirementFailureWire::NegativeShiftCountPossible { span: *span }
        }
        Live::TensorEdgePolicyDeferred { span } => {
            KernelRequirementFailureWire::TensorEdgePolicyDeferred { span: *span }
        }
        Live::SaturatingLoweringDeferred { span, width } => {
            KernelRequirementFailureWire::SaturatingLoweringDeferred { span: *span, width: *width }
        }
        Live::OptionFRequired { span, width } => {
            KernelRequirementFailureWire::OptionFRequired { span: *span, width: *width }
        }
    }
}

fn contract_roles_wire(types: &radix_types::TypeTable) -> radix_hir_fhir::ContractRolesWire {
    let sorted = |set: &handroll::FxHashSet<crate::hir::DefId>| {
        let mut defs: Vec<_> = set.iter().copied().collect();
        defs.sort_by_key(|def_id| def_id.0);
        defs
    };
    radix_hir_fhir::ContractRolesWire {
        ordering: types.ordering_contract,
        ordering_bindings: sorted(&types.ordering_contract_bindings),
        closing: types.closing_contract,
        sinking: types.sinking_contract,
        sinking_bindings: sorted(&types.sinking_contract_bindings),
    }
}

fn shape_obligations_wire(
    obligations: &crate::semantic::ShapeObligationTable,
) -> radix_hir_fhir::ShapeObligationTableWire {
    use radix_hir_fhir::{
        CallSizeBindingsWire, FunctionSizeBindingsWire, RuntimeSizeBindingWire, ShapeCallObligationsWire,
        ShapeFunctionObligationsWire, ShapeObligationTableWire, ShapeRelationBoundaryWire, ShapeRelationWire,
        ShapeStoreObligationsWire,
    };

    let map_relations = |relations: &[crate::semantic::ShapeRelation]| {
        relations
            .iter()
            .map(|relation| ShapeRelationWire {
                left: relation.left,
                right: relation.right,
                span: relation.span,
                boundary: match relation.boundary {
                    crate::semantic::ShapeRelationBoundary::Call => ShapeRelationBoundaryWire::Call,
                    crate::semantic::ShapeRelationBoundary::FunctionEntry => ShapeRelationBoundaryWire::FunctionEntry,
                    crate::semantic::ShapeRelationBoundary::FunctionReturn => ShapeRelationBoundaryWire::FunctionReturn,
                    crate::semantic::ShapeRelationBoundary::Store => ShapeRelationBoundaryWire::Store,
                },
            })
            .collect()
    };
    let mut calls: Vec<_> = obligations
        .calls
        .iter()
        .map(|(call, relations)| ShapeCallObligationsWire { call: *call, relations: map_relations(relations) })
        .collect();
    calls.sort_by_key(|entry| entry.call.0);
    let mut functions: Vec<_> = obligations
        .functions
        .iter()
        .map(|(function, relations)| ShapeFunctionObligationsWire {
            function: *function,
            relations: map_relations(relations),
        })
        .collect();
    functions.sort_by_key(|entry| entry.function.0);
    let mut stores: Vec<_> = obligations
        .stores
        .iter()
        .map(|(initializer, relations)| ShapeStoreObligationsWire {
            initializer: *initializer,
            relations: map_relations(relations),
        })
        .collect();
    stores.sort_by_key(|entry| entry.initializer.0);
    let mut call_size_bindings: Vec<_> = obligations
        .call_size_bindings
        .iter()
        .map(|(call, bindings)| CallSizeBindingsWire {
            call: *call,
            bindings: bindings
                .iter()
                .map(|binding| RuntimeSizeBindingWire {
                    parameter: binding.parameter,
                    witness: binding.witness,
                    span: binding.span,
                })
                .collect(),
        })
        .collect();
    call_size_bindings.sort_by_key(|entry| entry.call.0);
    let mut function_size_bindings: Vec<_> = obligations
        .function_size_bindings
        .iter()
        .map(|(function, bindings)| FunctionSizeBindingsWire {
            function: *function,
            bindings: bindings
                .iter()
                .map(|binding| RuntimeSizeBindingWire {
                    parameter: binding.parameter,
                    witness: binding.witness,
                    span: binding.span,
                })
                .collect(),
        })
        .collect();
    function_size_bindings.sort_by_key(|entry| entry.function.0);
    let mut proven_indexes: Vec<_> = obligations.proven_indexes.iter().copied().collect();
    proven_indexes.sort_by_key(|access| access.0);
    ShapeObligationTableWire { calls, call_size_bindings, functions, stores, function_size_bindings, proven_indexes }
}

// ── Provider carriage (ARCH-U4 / schema v14) ───────────────────────────────

/// Map one analyzed unit's carried provider view onto the wire, recursing
/// through each provider's nested view so the loaded surface can re-emit
/// nested namespace imports as sibling modules exactly as the direct surface
/// does. Diagnostics are not wire facts: a carried unit's diagnostics are
/// warnings-only by construction (analysis errors travel as `Failed`
/// markers), so the wire omits them.
fn provider_units_wire(
    view: &crate::driver::ProviderUnitView,
) -> BTreeMap<String, radix_hir_fhir::ProviderCarriageEntryWire> {
    view.units
        .iter()
        .map(|(import_path, carried)| {
            let entry = match carried {
                crate::driver::CarriedProviderUnit::Failed { message } => {
                    radix_hir_fhir::ProviderCarriageEntryWire::Failed { message: message.clone() }
                }
                crate::driver::CarriedProviderUnit::Unit(unit) => {
                    let provider = unit
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    radix_hir_fhir::ProviderCarriageEntryWire::Unit(Box::new(radix_hir_fhir::ProviderUnitWire {
                        hir: provider.hir.clone(),
                        types: provider.types.snapshot(),
                        resolved_generic_calls: provider.types.resolved_generic_calls.clone(),
                        interner: provider.interner.to_string_table(),
                        resolver: provider.resolver.to_snapshot(),
                        tensor_edge_policies: tensor_edge_policies_wire(&provider.tensor_edge_policies),
                        shape_obligations: shape_obligations_wire(&provider.shape_obligations),
                        nested: provider_units_wire(&provider.nested),
                    }))
                }
            };
            (import_path.clone(), entry)
        })
        .collect()
}

// ── Capability provenance wire conversions (SCR-04) ───────────────────────

fn returned_borrow_to_wire(p: crate::semantic::ReturnedBorrowProvenance) -> ReturnedBorrowWire {
    match p {
        crate::semantic::ReturnedBorrowProvenance::None => ReturnedBorrowWire::None,
        crate::semantic::ReturnedBorrowProvenance::Parameter(def_id) => ReturnedBorrowWire::Parameter(def_id),
        crate::semantic::ReturnedBorrowProvenance::Definition(def_id) => ReturnedBorrowWire::Definition(def_id),
    }
}

fn provenance_table_to_wire(table: &crate::semantic::FunctionProvenanceTable) -> FunctionProvenanceTableWire {
    FunctionProvenanceTableWire {
        functions: table
            .iter()
            .map(|row| FunctionProvenanceWire {
                function: row.function,
                returned_borrow: returned_borrow_to_wire(row.returned_borrow),
                required_regions: row
                    .required_regions
                    .iter()
                    .map(|rel| RegionRelationshipWire { from: rel.from, to: rel.to })
                    .collect(),
                last_uses: row
                    .last_uses
                    .iter()
                    .map(|fact| LastUseWire { borrower: fact.borrower, site: fact.site })
                    .collect(),
            })
            .collect(),
    }
}

// ── Wire-conversion helpers ───────────────────────────────────────────────

fn capture_fact_to_wire(fact: &crate::semantic::CaptureFact) -> CaptureFactWire {
    CaptureFactWire {
        def_id: fact.def_id,
        mode: match fact.mode {
            crate::semantic::CaptureMode::Read => CaptureModeWire::Read,
            crate::semantic::CaptureMode::Mutate => CaptureModeWire::Mutate,
            crate::semantic::CaptureMode::Consume => CaptureModeWire::Consume,
        },
    }
}

fn function_fact_to_wire(fact: &crate::semantic::FunctionFact) -> FunctionFactWire {
    FunctionFactWire {
        has_direct_failure: fact.direct_failure,
        may_fail: fact.may_fail,
        is_async: fact.is_async,
        is_generator: fact.is_generator,
        requires_await: fact.requires_await,
        call_edges: fact.call_edges.iter().copied().collect(),
        captures: fact.captures.iter().map(capture_fact_to_wire).collect(),
        param_modes: fact.param_modes.iter().map(|(k, v)| (*k, *v)).collect(),
        err_ty: fact.err_ty,
    }
}

fn function_fact_table_to_wire(ft: &crate::semantic::FunctionFactTable) -> FunctionFactTableWire {
    FunctionFactTableWire {
        entries: ft
            .iter()
            .map(|(def_id, fact)| (def_id, function_fact_to_wire(fact)))
            .collect(),
        entry: ft.entry.as_ref().map(function_fact_to_wire),
    }
}

fn resolved_use_to_wire(u: &crate::semantic::ResolvedUse) -> ResolvedUseWire {
    ResolvedUseWire {
        key: match &u.key {
            crate::semantic::ResolvedUseKey::Local(def_id) => ResolvedUseKeyWire::Local(*def_id),
            crate::semantic::ResolvedUseKey::Portable(s) => ResolvedUseKeyWire::Portable(s.clone()),
        },
        kind: match u.kind {
            crate::semantic::ResolvedUseKind::Path => ResolvedUseKindWire::Path,
            crate::semantic::ResolvedUseKind::Call => ResolvedUseKindWire::Call,
            crate::semantic::ResolvedUseKind::MethodCall => ResolvedUseKindWire::MethodCall,
            crate::semantic::ResolvedUseKind::TypeRef => ResolvedUseKindWire::TypeRef,
            crate::semantic::ResolvedUseKind::ImportBinding => ResolvedUseKindWire::ImportBinding,
        },
        enclosing: u.enclosing,
        local: u.local,
    }
}

fn gpu_builtin_to_wire(b: radix_types::MirKernelBuiltin) -> GpuBuiltinWire {
    match b {
        radix_types::MirKernelBuiltin::GlobalInvocationX => GpuBuiltinWire::GlobalInvocationX,
        radix_types::MirKernelBuiltin::GlobalInvocationY => GpuBuiltinWire::GlobalInvocationY,
        radix_types::MirKernelBuiltin::GlobalInvocationZ => GpuBuiltinWire::GlobalInvocationZ,
        radix_types::MirKernelBuiltin::GlobalInvocationId => GpuBuiltinWire::GlobalInvocationId,
        radix_types::MirKernelBuiltin::WorkgroupIdX => GpuBuiltinWire::WorkgroupIdX,
        radix_types::MirKernelBuiltin::WorkgroupIdY => GpuBuiltinWire::WorkgroupIdY,
        radix_types::MirKernelBuiltin::WorkgroupIdZ => GpuBuiltinWire::WorkgroupIdZ,
        radix_types::MirKernelBuiltin::WorkgroupId => GpuBuiltinWire::WorkgroupId,
        radix_types::MirKernelBuiltin::LocalInvocationIdX => GpuBuiltinWire::LocalInvocationIdX,
        radix_types::MirKernelBuiltin::LocalInvocationIdY => GpuBuiltinWire::LocalInvocationIdY,
        radix_types::MirKernelBuiltin::LocalInvocationIdZ => GpuBuiltinWire::LocalInvocationIdZ,
        radix_types::MirKernelBuiltin::LocalInvocationId => GpuBuiltinWire::LocalInvocationId,
        radix_types::MirKernelBuiltin::WorkgroupSizeX => GpuBuiltinWire::WorkgroupSizeX,
        radix_types::MirKernelBuiltin::WorkgroupSizeY => GpuBuiltinWire::WorkgroupSizeY,
        radix_types::MirKernelBuiltin::WorkgroupSizeZ => GpuBuiltinWire::WorkgroupSizeZ,
    }
}

// ── CLI wire conversions (F18 Shape B — forward mapping) ─────────────────

fn cli_type_wire(ty: &crate::cli::CliType) -> CliTypeWire {
    match ty {
        crate::cli::CliType::Textus => CliTypeWire::Textus,
        crate::cli::CliType::Numerus => CliTypeWire::Numerus,
        crate::cli::CliType::Fractus => CliTypeWire::Fractus,
        crate::cli::CliType::Bivalens => CliTypeWire::Bivalens,
        crate::cli::CliType::Octeti => CliTypeWire::Octeti,
        crate::cli::CliType::Ignotum => CliTypeWire::Ignotum,
        crate::cli::CliType::ListaTextus => CliTypeWire::ListaTextus,
        crate::cli::CliType::ListaNumerus => CliTypeWire::ListaNumerus,
    }
}

fn cli_default_wire(d: &crate::cli::CliDefault) -> CliDefaultWire {
    match d {
        crate::cli::CliDefault::Text(s) => CliDefaultWire::Text(s.clone()),
        crate::cli::CliDefault::Integer(i) => CliDefaultWire::Integer(*i),
        crate::cli::CliDefault::Float(f) => CliDefaultWire::Float(*f),
        crate::cli::CliDefault::Bool(b) => CliDefaultWire::Bool(*b),
        crate::cli::CliDefault::Nil => CliDefaultWire::Nil,
        crate::cli::CliDefault::Expr(s) => CliDefaultWire::Expr(s.clone()),
    }
}

fn cli_exit_wire(e: &crate::cli::CliExit) -> CliExitWire {
    match e {
        crate::cli::CliExit::Fixed(v) => CliExitWire::Fixed(*v),
        crate::cli::CliExit::Binding(s) => CliExitWire::Binding(s.clone()),
        crate::cli::CliExit::Field { object, field } => {
            CliExitWire::Field { object: object.clone(), field: field.clone() }
        }
        crate::cli::CliExit::Unsupported => CliExitWire::Unsupported,
    }
}

fn cli_option_wire(o: &crate::cli::CliOption) -> CliOptionWire {
    CliOptionWire {
        binding: o.binding.clone(),
        binding_symbol: o.binding_symbol,
        ty: cli_type_wire(&o.ty),
        short: o.short.clone(),
        long: o.long.clone(),
        description: o.description.clone(),
        global: o.global,
        default: o.default.as_ref().map(cli_default_wire),
        is_flag: o.flag,
    }
}

fn cli_operand_wire(o: &crate::cli::CliOperand) -> CliOperandWire {
    CliOperandWire {
        binding: o.binding.clone(),
        binding_symbol: o.binding_symbol,
        ty: cli_type_wire(&o.ty),
        is_rest: o.rest,
        description: o.description.clone(),
        global: o.global,
        default: o.default.as_ref().map(cli_default_wire),
    }
}

fn cli_command_wire(c: &crate::cli::CliCommand) -> CliCommandWire {
    CliCommandWire {
        path: c.path.clone(),
        module_path: c.module_path.clone(),
        function: c.function.clone(),
        function_symbol: c.function_symbol,
        args_binding: c.args_binding.clone(),
        aliases: c.aliases.clone(),
        description: c.description.clone(),
        options: c.options.iter().map(cli_option_wire).collect(),
        operands: c.operands.iter().map(cli_operand_wire).collect(),
    }
}

fn cli_mode_wire(m: &crate::cli::CliMode) -> CliModeWire {
    match m {
        crate::cli::CliMode::NotCli => CliModeWire::NotCli,
        crate::cli::CliMode::SingleCommand => CliModeWire::SingleCommand,
        crate::cli::CliMode::Subcommand => CliModeWire::Subcommand,
    }
}

fn cli_program_wire(p: &crate::cli::CliProgram) -> CliProgramWire {
    CliProgramWire {
        name: p.name.clone(),
        entry_args: p.entry_args.clone(),
        mode: cli_mode_wire(&p.mode),
        version: p.version.clone(),
        description: p.description.clone(),
        global_options: p.global_options.iter().map(cli_option_wire).collect(),
        global_operands: p.global_operands.iter().map(cli_operand_wire).collect(),
        options: p.options.iter().map(cli_option_wire).collect(),
        operands: p.operands.iter().map(cli_operand_wire).collect(),
        commands: p.commands.iter().map(cli_command_wire).collect(),
        exit: p.exit.as_ref().map(cli_exit_wire),
    }
}

#[cfg(test)]
#[path = "serialize_test.rs"]
mod tests;
