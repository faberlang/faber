//! Fusion-group decomposition (S5-U1): multi-recipe device-resident bodies →
//! subchain kernels with explicit data-flow.
//!
//! The one-contract-per-kernel limit (`device_program_plans.rs`): a kernel
//! carries exactly ONE [`CollectionKernelPlan`] under its dispatch contract,
//! and [`kernel_plan_for_function`](crate::device_program_plans::kernel_plan_for_function)
//! fails on a body whose complete op set mixes distinct recipes. But the MLP
//! device-resident bodies mix recipes by construction: the forward lane
//! `mlp_loss` carries 2×`MatMul` + `Mean`, and the matmul-VJP companion carries
//! `Transpose` + `MatMul` + elementwise chains. This module is the MIR-level
//! decomposition pass that splits such bodies at recipe boundaries into
//! **subchain kernels** — each carrying exactly one recipe (or an
//! elementwise-only body) — with explicit carried data-flow edges between
//! them (the campaign risk row: "model subchains/multiple resources
//! explicitly").
//!
//! # The decomposition contract
//!
//! A device-resident function body decomposes when:
//! - it is **single-block** (straight-line statements; control flow cannot
//!   be sliced into an ordered kernel chain);
//! - every statement is either a recipe op (`Sum` / `TensorMean` /
//!   `TensorMatMul` / `TensorTranspose` / `TensorSumAxis` / `TensorSoftmax`
//!   / `TensorLayerNorm` — the S6-P1 transformer ops are recipe boundaries
//!   like the others — plus the GI3-1 inference recipes
//!   `TensorRmsNorm` / `TensorRope` / `TensorGather` /
//!   `TensorCausalMaskedSoftmax`, which follow the same
//!   `from_transformer_op`-sibling admission path), an elementwise transform
//!   ([`is_elementwise_collection_op`](crate::device_program_plans::is_elementwise_collection_op)),
//!   or elementwise value math (Assign / Construct / Call);
//! - every **live** recipe op resolves its own dispatch contract and plan.
//!
//! A body that stores into an explicit output parameter (the indexed-view
//! kernel shape `y ⇇ d`) is carried by its FINAL subchain: that subchain is
//! built as the indexed-view kernel the source is, so its output IS the
//! parameter and the fused tail behind its recipe is emitted
//! ([`KernelSubchain::output_param`]). A store to an output parameter in any
//! earlier subchain, a second output parameter, or a read of the output
//! parameter fails closed with a typed [`PlanMappingError`]; no path drops the
//! store.
//!
//! The pass slices the statement stream at recipe-op boundaries: consecutive
//! elementwise statements fold into the adjacent recipe subchain (the
//! S3-B1 mul-mean forward folds its mul into the reduction load), and a new
//! recipe op after a recipe-bearing slice starts a new subchain. Dead recipe
//! statements (S3-A5: the destination is never read) are dropped — they are
//! not part of the kernel's semantics and must not force a recipe boundary.
//!
//! Each subchain resolves its plan through the shared plan pass
//! ([`resolve_subchain_plan`](crate::device_program_plans::resolve_subchain_plan))
//! — the single source of recipe truth — and records which earlier
//! subchains it consumes (its data-flow inputs). A body that cannot
//! decompose (control flow, an unplannable op, a recipe subchain that
//! cannot resolve) fails with a **typed** [`PlanMappingError`] — never a
//! silent `Elementwise`.
//!
//! AIR remains the differentiation authority (A1): this pass only slices
//! and re-labels existing MIR statements; it never introduces a GPU-specific
//! AIR node or relowers semantics.

use crate::abi::{DEFAULT_STORAGE_BUFFER_LIMIT, StorageBufferLimitRecord};
use crate::device_program_plans::{
    PlanMappingError, data_flow_element_ty_and_count, is_elementwise_collection_op, is_recipe_op,
    resolve_subchain_plan,
};
use crate::elementwise_plan::{ElementwisePlan, plan_elementwise};
use crate::kernel_plan::CollectionKernelPlan;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirFunction,
    MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirStatement,
    MirStatementKind, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, MirValueKind,
};
// The decomposition's own scan helpers moved to `crate::collection_ops`; the
// collection-op types remain in scope for the `#[path]` test module.
#[cfg(test)]
use crate::{MirCollectionOp, MirIntrinsic, MirRuntimeCall};
use handroll::FxHashSet;
use radix_types::{Type, TypeId, TypeTable};
use std::collections::{BTreeMap, BTreeSet};

const PD18_MAX_CANDIDATES: usize = 25;

/// One decomposed kernel subchain: a contiguous statement slice of the
/// source function carrying exactly one recipe (or an elementwise-only
/// body), plus the data-flow edges that connect it to the rest of the chain.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelSubchain {
    /// Statement indices into the source function's single block, in order
    /// (the slice this kernel lowers from).
    pub statements: Vec<usize>,
    /// The single plan this subchain carries (elementwise-only subchains
    /// carry [`CollectionKernelPlan::Elementwise`]).
    pub plan: CollectionKernelPlan,
    /// Typed elementwise expression plan for a legal elementwise-only
    /// subchain. `None` when [`Self::plan`] is a recipe or when the slice
    /// hits a typed [`crate::elementwise_plan::ElementwiseBarrierError`]. The
    /// collection-plan classification is unchanged: this is additive
    /// payload, not a replacement for [`CollectionKernelPlan::Elementwise`].
    pub elementwise_plan: Option<ElementwisePlan>,
    /// Indices into [`KernelDecomposition::subchains`] whose produced local
    /// this subchain reads — its explicit carried data-flow inputs.
    pub consumes: Vec<usize>,
    /// The local this subchain produces — the destination of the last
    /// destination-writing statement in the slice — when a later subchain
    /// consumes it.
    pub produced: Option<MirLocalId>,
    /// The locals this subchain must expose as kernel outputs (D-2a): the
    /// recipe result (the subchain's primary/current output) plus every
    /// local it writes that a LATER subchain of the same function reads.
    /// The last subchain's output set is the function's return local
    /// (unchanged). Deterministic order: primary first, then the additional
    /// consumed locals by ascending local id.
    ///
    /// D-2b-1 materializes this set on the synthetic subchain function: an
    /// intermediate subchain with additional outputs returns a TUPLE of
    /// `outputs` (`[primary, extra_outputs...]`), which the emission
    /// signature carries as `extra_outputs` and the emitters' multi-output
    /// tuple-tail paths lower.
    pub outputs: Vec<MirLocalId>,
    /// The source function's explicit output parameter (the `varia` tensor
    /// parameter of an indexed-view kernel, `y ⇇ d`) this subchain stores
    /// into. Only the LAST subchain can carry it: the final store is the
    /// kernel's output, so that subchain is built as a whole-function
    /// indexed-view kernel whose output IS this parameter (its synthetic
    /// declares the parameter and the source's trailing index parameters)
    /// instead of a contract-shaped kernel that binds the recipe result as
    /// its output and would write a dead local.
    pub output_param: Option<MirLocalId>,
}

impl Eq for KernelSubchain {}

impl KernelSubchain {
    /// The typed elementwise expression plan when this subchain is a legal
    /// elementwise-only candidate.
    #[must_use]
    pub fn expression_plan(&self) -> Option<&ElementwisePlan> {
        self.elementwise_plan.as_ref()
    }
}

/// The ordered subchain kernels of one decomposed device-resident function.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KernelDecomposition {
    /// Ordered subchain kernels (each carries exactly one recipe or an
    /// elementwise-only body; a subchain at index `i` may consume the
    /// produced local of any earlier subchain).
    pub subchains: Vec<KernelSubchain>,
}

/// Decompose a device-resident function body into fusion-group subchain
/// kernels (S5-U1).
///
/// The pass slices the statement stream at recipe-op boundaries, folds
/// elementwise statements into the adjacent subchain, drops dead recipe
/// statements (S3-A5), resolves each subchain's plan through the shared
/// plan pass, attaches a typed [`ElementwisePlan`] to a legal
/// elementwise-only subchain, and records the explicit data-flow edges
/// (which earlier subchain's produced local each subchain consumes).
///
/// Fail-closed: a multi-block (control-flow) body, a statement with a
/// collection op that is neither recipe nor elementwise, or a recipe
/// subchain whose contract/plan cannot resolve is a typed
/// [`PlanMappingError`] — never a silent `Elementwise`.
///
/// # Errors
/// Returns [`PlanMappingError`] on any of the fail-closed conditions above.
pub fn decompose_kernel_function(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<KernelDecomposition, PlanMappingError> {
    decompose_kernel_function_with_limit(function, validation, DEFAULT_STORAGE_BUFFER_LIMIT)
}

/// Decompose a device-resident function body against an explicit lane-owned
/// storage-buffer capacity. The default wrapper keeps the shared conservative
/// minimum across admitted lanes.
///
/// # Errors
/// Returns [`PlanMappingError`] on any fail-closed decomposition condition.
pub fn decompose_kernel_function_with_limit(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    limit: StorageBufferLimitRecord,
) -> Result<KernelDecomposition, PlanMappingError> {
    // The closed lineages a statement's reads resolve to (params and the
    // open slice's own writes contribute nothing; temps resolve to the
    // slice containing their definition).
    fn cross_reads(
        statement: &MirStatement,
        current_writes: &BTreeSet<MirLocalId>,
        current_temp_defs: &BTreeSet<MirTempId>,
        lineage_of_local: &BTreeMap<MirLocalId, usize>,
        lineage_of_temp: &BTreeMap<MirTempId, usize>,
    ) -> BTreeSet<usize> {
        let mut cross = BTreeSet::new();
        for local in crate::read_analysis::statement_read_locals(statement) {
            if current_writes.contains(&local) {
                continue;
            }
            if let Some(lineage) = lineage_of_local.get(&local) {
                cross.insert(*lineage);
            }
        }
        let (_, referenced) = statement_temp_sets(statement);
        for temp in referenced {
            if current_temp_defs.contains(&temp) {
                continue;
            }
            if let Some(lineage) = lineage_of_temp.get(&temp) {
                cross.insert(*lineage);
            }
        }
        cross
    }

    // Close the open slice: its writes and temp definitions take the
    // closed slice's lineage for the rest of the walk.
    // Foreign-arity note (kcm Unit 1A): allowed as-is; flagged by stage 2
    // while authored-form closeout ran — not part of that goal.
    #[allow(clippy::too_many_arguments)]
    fn close_current(
        slices: &mut Vec<Vec<usize>>,
        current: &mut Vec<usize>,
        current_writes: &mut BTreeSet<MirLocalId>,
        current_temp_defs: &mut BTreeSet<MirTempId>,
        current_lineage: &mut BTreeSet<usize>,
        current_has_recipe: &mut bool,
        lineage_of_local: &mut BTreeMap<MirLocalId, usize>,
        lineage_of_temp: &mut BTreeMap<MirTempId, usize>,
    ) {
        let closed = slices.len();
        for local in current_writes.iter() {
            lineage_of_local.insert(*local, closed);
        }
        for temp in current_temp_defs.iter() {
            lineage_of_temp.insert(*temp, closed);
        }
        slices.push(std::mem::take(current));
        current_lineage.clear();
        current_writes.clear();
        current_temp_defs.clear();
        *current_has_recipe = false;
    }

    // Record one statement's writes into the open slice's sets.
    fn record_writes(
        statement: &MirStatement,
        current_writes: &mut BTreeSet<MirLocalId>,
        current_temp_defs: &mut BTreeSet<MirTempId>,
    ) {
        if let Some(local) = crate::read_analysis::statement_destination(statement) {
            current_writes.insert(local);
        }
        let (defined, _) = statement_temp_sets(statement);
        current_temp_defs.extend(defined);
    }

    if function.blocks.len() != 1 {
        return Err(PlanMappingError::mapping(
            "device function body has control flow and cannot decompose into subchain kernels \
             (subchains are straight-line statement slices)",
        ));
    }
    let block = &function.blocks[0];

    // Slice the statement stream at recipe boundaries AND along producer
    // lineages. `current` holds the source statement indices of the open
    // subchain; `current_has_recipe` tracks whether it adopted a recipe;
    // `current_lineage` tracks the closed subchains whose written locals the
    // open slice reads. Source adjacency is not a dependency: an elementwise
    // statement folds into the open slice only when it introduces no NEW
    // lineage — an elementwise join of an independent producer lineage
    // becomes its own elementwise subchain (fork/join truth, so the plan
    // never manufactures an edge between independent roots). A recipe
    // arriving at a lineage-carrying slice starts a new subchain instead of
    // adopting the completed join; the lineage-free elementwise prefix
    // still folds into the recipe it feeds (S3-B1). The returned tuple
    // assembly folds by ABI contract (D-6), not by adjacency.
    let mut slices: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_has_recipe = false;
    let mut current_lineage: BTreeSet<usize> = BTreeSet::new();
    let mut current_writes: BTreeSet<MirLocalId> = BTreeSet::new();
    let mut current_temp_defs: BTreeSet<MirTempId> = BTreeSet::new();
    let mut lineage_of_local: BTreeMap<MirLocalId, usize> = BTreeMap::new();
    let mut lineage_of_temp: BTreeMap<MirTempId, usize> = BTreeMap::new();

    // The returned place base: the tuple-return assembly is the ABI's
    // output boundary, not a schedulable join.
    let returned_base: Option<MirPlaceBase> = match &block.terminator.kind {
        MirTerminatorKind::Return(Some(MirOperand::Place(place))) => Some(place.base.clone()),
        MirTerminatorKind::Return(Some(MirOperand::Temp(temp))) => Some(MirPlaceBase::Temp(*temp)),
        _ => None,
    };
    let is_return_assembly = |statement: &MirStatement| -> bool {
        matches!(
            statement.kind,
            MirStatementKind::Construct {
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Tuple,
                    ..
                },
                ..
            }
        ) && matches!(
            statement.kind,
            MirStatementKind::Construct { ref destination, .. }
            if Some(&destination.base) == returned_base.as_ref()
        )
    };

    // Walk the statement stream. An elementwise statement (value math or
    // an elementwise collection op) folds into the open slice only when its
    // cross-reads stay within the slice's lineage; the first statement that
    // reads a NEW producer lineage closes the open slice and starts an
    // elementwise JOIN slice whose lineage is the union of everything it
    // reads — later elementwise statements accumulate into that join (a
    // fan-in of any arity is one join child). A recipe closes a join slice
    // and opens a fresh recipe slice; the lineage-free elementwise prefix
    // still folds into the recipe it feeds (S3-B1). Source adjacency never
    // manufactures an edge between independent roots.
    let mut index = 0usize;
    while index < block.statements.len() {
        let statement = &block.statements[index];
        if let Some(op) = crate::collection_ops::statement_collection_op(statement) {
            if is_recipe_op(op) {
                // S3-A5: a recipe op whose result is never read is dead — not
                // part of the kernel semantics, and it must not force a recipe
                // boundary or a contract the live plan does not carry.
                if statement_destination_is_dead(function, statement) {
                    index += 1;
                    continue;
                }
                if current_has_recipe || !current_lineage.is_empty() {
                    close_current(
                        &mut slices,
                        &mut current,
                        &mut current_writes,
                        &mut current_temp_defs,
                        &mut current_lineage,
                        &mut current_has_recipe,
                        &mut lineage_of_local,
                        &mut lineage_of_temp,
                    );
                }
                current_has_recipe = true;
                current.push(index);
                record_writes(statement, &mut current_writes, &mut current_temp_defs);
                index += 1;
                continue;
            }
            if !is_elementwise_collection_op(op) {
                return Err(PlanMappingError::mapping(format!(
                    "collection op {op:?} has no kernel plan \
                     (no recipe and not an elementwise transform)"
                )));
            }
        } else if is_return_assembly(statement) {
            // D-6: the returned tuple assembly folds into the open slice
            // by ABI contract — it is the output boundary, not a join.
            current.push(index);
            record_writes(statement, &mut current_writes, &mut current_temp_defs);
            index += 1;
            continue;
        }
        // The statement is elementwise (a collection transform or plain value
        // math): fold it by its cross-reads, or open a join with its lineage.
        let cross = cross_reads(
            statement,
            &current_writes,
            &current_temp_defs,
            &lineage_of_local,
            &lineage_of_temp,
        );
        if cross.is_subset(&current_lineage) {
            // Plain fold into the open slice.
        } else if !current_lineage.is_empty() && !current_has_recipe {
            // An open JOIN slice absorbs a further producer lineage —
            // fan-in of any arity is one join child.
            current_lineage.extend(&cross);
        } else {
            // Close the open slice and start a join with this lineage.
            if !current.is_empty() {
                close_current(
                    &mut slices,
                    &mut current,
                    &mut current_writes,
                    &mut current_temp_defs,
                    &mut current_lineage,
                    &mut current_has_recipe,
                    &mut lineage_of_local,
                    &mut lineage_of_temp,
                );
            }
            current_has_recipe = false;
            current_lineage = cross;
        }
        current.push(index);
        record_writes(statement, &mut current_writes, &mut current_temp_defs);
        index += 1;
    }
    if !current.is_empty() {
        close_current(
            &mut slices,
            &mut current,
            &mut current_writes,
            &mut current_temp_defs,
            &mut current_lineage,
            &mut current_has_recipe,
            &mut lineage_of_local,
            &mut lineage_of_temp,
        );
    }

    // The per-kernel storage-buffer budget: a whole body whose emission
    // signature exceeds it (train_step_bert_linear: 24 tensor params + the
    // lr scalar + a 12-tuple return = 37 buffers; Metal's `[[buffer(N)]]`
    // maximum is 31) cannot be emitted as ONE kernel. An elementwise-only
    // body splits into consecutive elementwise sub-slices so every subchain
    // kernel fits the budget, preserving the plan semantics (the carried
    // data-flow edges between the subchains carry the same values).
    let slices = sink_constant_statements(function, block, slices);
    let slices = split_oversized_elementwise_body(function, block, slices, validation, limit)?;

    // The explicit output parameter the final subchain stores into (fails
    // closed when a store to an output parameter has no kernel to carry it).
    let output_param = explicit_output_param(function, block, &slices)?;

    // Build the subchains. `written` records each subchain's produced
    // locals so the data-flow edges can be derived. The subchains preserve
    // the source local tables, so `inputs` (each subchain's external reads,
    // its synthetic params) and `recipe_results` (each subchain's recipe
    // destination) can be derived for the consumed-output pass below.
    let mut subchains: Vec<KernelSubchain> = Vec::new();
    let mut written_by: Vec<BTreeSet<MirLocalId>> = Vec::new();
    let mut inputs_by: Vec<BTreeSet<MirLocalId>> = Vec::new();
    let mut recipe_results: Vec<Option<MirLocalId>> = Vec::new();
    for (position, indices) in slices.iter().enumerate() {
        let is_last = position + 1 == slices.len();
        // The final subchain needs the indexed-view reading unless its
        // existing emission surface already binds the output parameter: a
        // parameter the function RETURNS is the last subchain's output (the
        // consumed-output pass), and a recipe result forwarded straight into
        // the parameter (`out ⇇ a.matmul(b)`) is the contract output. Any other
        // store — a fused tail behind a recipe, an elementwise body, a recipe
        // result copied in through a stage local — is a statement the
        // contract-shaped surface cannot bind: it names the recipe result
        // local as the output and leaves the parameter unwritten.
        let initial_subchain = build_subchain_function(
            function,
            indices,
            &[],
            is_last,
            false,
            None,
            validation.types.as_table(),
        );
        let subchain_output_param = output_param.filter(|&output| {
            is_last
                && crate::read_analysis::terminator_return_local(&block.terminator.kind)
                    != Some(output)
                && !crate::device_program_plans::recipe_result_is_param(&initial_subchain, output)
        });
        let mut synthetic = initial_subchain;
        if subchain_output_param.is_some() {
            synthetic = build_subchain_function(
                function,
                indices,
                &[],
                is_last,
                false,
                subchain_output_param,
                validation.types.as_table(),
            );
        }
        // The S3-A5 dead filter resolves reads against the SOURCE function
        // (the subchain's produced local is consumed by a later subchain,
        // invisible inside the slice itself).
        let plan = resolve_subchain_plan(&synthetic, function, validation)?;
        crate::kernel_plan::confirm_packed_admission(&synthetic, &plan)
            .map_err(|error| PlanMappingError::mapping(error.to_string()))?;
        // Additive payload: a legal elementwise-only subchain carries the
        // OF-1 expression plan. Recipe classification stays
        // `resolve_subchain_plan` / `kernel_plan/` — this never re-derives
        // it, and a typed `ElementwiseBarrierError` leaves the collection plan
        // as payload-less `Elementwise` (existing gates stay closed). The
        // elementwise planning synthetic is rebuilt with the join child's
        // product-return (Unit 1A) so the planner's liveness sees the
        // cross-kernel read.
        let elementwise_plan = match &plan {
            CollectionKernelPlan::Elementwise => {
                synthetic = build_subchain_function(
                    function,
                    indices,
                    &[],
                    is_last,
                    true,
                    subchain_output_param,
                    validation.types.as_table(),
                );
                plan_elementwise(&synthetic, validation).ok()
            }
            _ => None,
        };
        let produced = indices.iter().rev().find_map(|&index| {
            crate::read_analysis::statement_destination(&block.statements[index])
        });
        let written: BTreeSet<MirLocalId> = indices
            .iter()
            .filter_map(|&index| {
                crate::read_analysis::statement_destination(&block.statements[index])
            })
            .collect();
        let inputs: BTreeSet<MirLocalId> =
            synthetic.params.iter().map(|param| param.local).collect();
        // The recipe that opened this subchain (the plan's result local).
        let recipe_result = indices.iter().find_map(|&index| {
            let statement = &block.statements[index];
            if matches!(
                crate::collection_ops::statement_collection_op(statement),
                Some(op) if is_recipe_op(op)
            ) {
                crate::read_analysis::statement_destination(statement)
            } else {
                None
            }
        });

        // Data-flow: subchains whose produced local this subchain reads.
        let mut consumes = Vec::new();
        for (prior, prior_written) in written_by.iter().enumerate() {
            if prior_written.iter().any(|local| inputs.contains(local)) {
                consumes.push(prior);
            }
        }

        written_by.push(written);
        inputs_by.push(inputs);
        recipe_results.push(recipe_result);
        subchains.push(KernelSubchain {
            statements: indices.clone(),
            plan,
            elementwise_plan,
            consumes,
            produced,
            outputs: Vec::new(),
            output_param: subchain_output_param,
        });
    }

    // The consumed-output sets (D-2a): each subchain must expose every local
    // it writes that a LATER subchain of the same function reads (or the
    // function returns). The recipe result stays the primary output; the
    // consumed elementwise destinations are the additional outputs. The last
    // subchain's output set is the function's return local — plus, for a
    // tuple-return function (D-6), every return-tuple field local the last
    // subchain writes: the return-tuple `Construct` binds full-tuple index
    // `i` → local, and a gradient local assembled inside the last subchain
    // has no distinct kernel buffer (no gradient slot to alias) unless it is
    // exposed here as an extra output. Walk backwards accumulating each
    // later subchain's external inputs (its synthetic params) — exactly the
    // locals a later subchain reads from outside itself.
    let return_tuple_fields = source_return_tuple_fields(function);
    let mut later_inputs: BTreeSet<MirLocalId> = BTreeSet::new();
    for position in (0..subchains.len()).rev() {
        let written = &written_by[position];
        let mut outputs: Vec<MirLocalId> = Vec::new();
        if position + 1 == subchains.len() {
            if let Some(fields) = &return_tuple_fields {
                // D-6: a tuple-return LAST subchain exposes the return-tuple
                // fields it writes (in full-tuple index order) as its output
                // set. The return-tuple LOCAL itself is never a kernel output
                // — the ABI flattens tuple returns into per-field device-view
                // outputs, and a nested tuple element (the return-tuple local
                // as an output) would fail the indexed-kernel device-view law.
                // Every gradient slot's producer resolution
                // (`producer_position`) then lands on a distinct kernel output
                // buffer (the train_step reads alias the field buffers).
                for &field in fields {
                    if written.contains(&field) {
                        outputs.push(field);
                    }
                }
            } else if let Some(return_local) =
                crate::read_analysis::terminator_return_local(&block.terminator.kind)
            {
                // A scalar-return last subchain keeps the source return local.
                if written.contains(&return_local) {
                    outputs.push(return_local);
                }
            }
        } else {
            // Primary: the recipe result stays the subchain's output even
            // when a later subchain never reads it (the contract output).
            if let Some(primary) = recipe_results[position] {
                outputs.push(primary);
            }
            // Additional: written locals a later subchain reads, by
            // ascending local id (deterministic; the recipe result, when
            // also consumed later, is already the primary).
            for &local in written {
                if later_inputs.contains(&local) && !outputs.contains(&local) {
                    outputs.push(local);
                }
            }
        }
        subchains[position].outputs = outputs;
        later_inputs.extend(inputs_by[position].iter().copied());
    }

    Ok(KernelDecomposition { subchains })
}

/// Split an elementwise-only body whose whole-function storage-buffer count
/// exceeds the selected lane's storage-buffer capacity into consecutive elementwise
/// sub-slices, each with an emission signature at or under the budget.
///
/// Only the single-slice elementwise-only shape splits (the whole-function
/// plan gate in `kernel_plan_for_function` routes an oversized body here):
/// the body has no recipe op, so every sub-slice stays elementwise-only and
/// the D-2a consumed-output pass decides each subchain's output set.
///
/// # Split structure
///
/// The statement stream is partitioned at the FIRST return-tuple
/// field-writing statement (a statement whose destination is one of the
/// function's return-tuple field locals):
///
/// 1. **Pre sub-slices** — the statements before the first field-writing
///    statement (the `train_step` fills + scaled-grad muls). They produce
///    the intermediate values (the scaled grads) and never alias the
///    result-tuple output slots, so they are split greedily by the buffer
///    budget alone.
/// 2. **Field sub-slices** — the field-writing statements grouped in
///    full-tuple order, plus (in the LAST sub-slice) the source return
///    assembly (the tuple Construct + Return). Grouping the field writers
///    separately keeps each field subchain's output-set tuple indices
///    aligned with the full result-tuple indices, so the caller's
///    result-tuple output slots (the S5-U5 `update_output` aliasing) stay
///    correct: an intermediate field subchain whose outputs are fields
///    `0..=k` aliases the params `0..=k` exactly, and the last subchain
///    (which keeps the source return assembly) aliases the params whose
///    update statements it carries.
///
/// # Errors
/// Returns [`PlanMappingError`] when the body exceeds the budget but cannot
/// be split (no return-tuple fields written in the body, or a statement
/// after the first field writer that is neither a field writer nor part of
/// the return assembly).
fn split_oversized_elementwise_body(
    function: &MirFunction,
    block: &MirBlock,
    slices: Vec<Vec<usize>>,
    validation: &MirValidationContext<'_>,
    limit: StorageBufferLimitRecord,
) -> Result<Vec<Vec<usize>>, PlanMappingError> {
    // Only the single-slice elementwise-only shape splits: a recipe-bearing
    // slice resolves contract-shaped signatures (smaller counts) and the
    // whole-function budget gate only routes an oversized ELEMENTWISE body
    // to the decomposition path.
    if slices.len() != 1 {
        return Ok(slices);
    }
    let indices = slices[0].clone();
    if indices.iter().any(|&index| {
        matches!(
            crate::collection_ops::statement_collection_op(&block.statements[index]),
            Some(op) if is_recipe_op(op)
        )
    }) {
        return Ok(slices);
    }

    // The whole-body storage-buffer count — the same shape the faber /
    // emitters build (every non-vacuum param is an input; a tuple return
    // binds one output per tuple element, a scalar return one buffer) —
    // decides whether a split is needed at all. Computed from the typed
    // facts directly (the ABI synthesis applies target-entry storage
    // admission, which test fixtures with host-owned tensors do not meet).
    let input_count = function
        .params
        .iter()
        .filter(|param| !crate::abi::is_vacuum(param.ty, validation))
        .count();
    let output_count = match validation.types.get(function.return_ty.semantic_id()) {
        Type::Tuple(elements) if !elements.is_empty() => elements.len(),
        _ => 1,
    };
    let whole_count = input_count + output_count;
    let limit_phrase = limit.binding_limit_phrase();
    if limit.admits(whole_count) {
        return Ok(slices);
    }

    let n = indices.len();
    // The return-tuple field locals in full-tuple order (local- or
    // temp-return forms both resolve; `None` when the source does not
    // return an ordered plain-local tuple).
    let Some(fields) = return_tuple_field_locals(function) else {
        return Err(PlanMappingError::mapping(format!(
            "elementwise body storage-buffer count {whole_count} exceeds the \
             {limit_phrase} and the body \
             does not return an ordered plain-local tuple the split can preserve"
        )));
    };
    let field_set: BTreeSet<MirLocalId> = fields.iter().copied().collect();

    // Per-statement written local, read locals, and the temps a cut must
    // not separate from their defining call.
    let written_at: Vec<Option<MirLocalId>> = indices
        .iter()
        .map(|&index| crate::read_analysis::statement_destination(&block.statements[index]))
        .collect();
    let reads_at: Vec<Vec<MirLocalId>> = indices
        .iter()
        .map(|&index| {
            let mut read = crate::read_analysis::statement_read_locals(&block.statements[index]);
            read.sort_unstable();
            read.dedup();
            read
        })
        .collect();
    let (temps_defined_at, temps_referenced_at): (
        Vec<BTreeSet<MirTempId>>,
        Vec<BTreeSet<MirTempId>>,
    ) = indices
        .iter()
        .map(|&index| statement_temp_sets(&block.statements[index]))
        .collect();

    // Precomputed suffix aggregates keep the tail check O(1) in the number
    // of statements. `suffix_reads[k]` is the union of reads at positions
    // >= k; the other sets describe the same range's writes and params.
    let mut suffix_reads = vec![BTreeSet::new(); n + 1];
    let mut suffix_written = vec![BTreeSet::new(); n + 1];
    let mut suffix_params = vec![BTreeSet::new(); n + 1];
    let mut suffix_temp_references = vec![BTreeSet::new(); n + 1];
    // Destination slots are empty and written once here: `clone_from` cannot
    // borrow the same Vec through IndexMut and has no allocation to reuse.
    #[allow(clippy::assigning_clones)]
    for i in (0..n).rev() {
        suffix_reads[i] = suffix_reads[i + 1].clone();
        suffix_reads[i].extend(reads_at[i].iter().copied());

        suffix_written[i] = suffix_written[i + 1].clone();
        if let Some(local) = written_at[i] {
            suffix_written[i].insert(local);
        }
        suffix_params[i] = suffix_params[i + 1].clone();
        suffix_params[i].extend(reads_at[i].iter().copied());
        suffix_params[i].retain(|local| !suffix_written[i].contains(local));

        suffix_temp_references[i] = suffix_temp_references[i + 1].clone();
        suffix_temp_references[i].extend(temps_referenced_at[i].iter().copied());
    }

    // The first field-writing statement. The statements before it are the
    // "pre" range; the statements from it onward are the field range (field
    // writers grouped in order, with the return assembly at the tail).
    let Some(first_field_raw) = written_at
        .iter()
        .position(|written| written.is_some_and(|local| field_set.contains(&local)))
    else {
        return Err(PlanMappingError::mapping(format!(
            "elementwise body storage-buffer count {whole_count} exceeds the \
             {limit_phrase} and the body \
             writes none of its return-tuple fields (the split cannot preserve \
             the result-tuple output semantics)"
        )));
    };
    // Backtrack the boundary to include the temp-producing calls of the
    // field writers (the real lowering's call-then-forward pairs must stay
    // together: a temp defined in the pre range and referenced by a field
    // writer in the field range would make every pre-range cut invalid).
    let mut first_field = first_field_raw;
    for (position, defined_at) in temps_defined_at[..first_field_raw].iter().enumerate() {
        for &temp in defined_at {
            if suffix_temp_references[first_field_raw].contains(&temp) {
                first_field = first_field.min(position);
            }
        }
    }

    // The pre range must be entirely before the first field writer, and the
    // field range's LOCAL-destination statements must be return-tuple field
    // writers or the return assembly (the tuple Construct). Temp-writing
    // collection calls (the real lowering's call-then-forward pairs) write
    // no local and contribute no output ordering, so they are free.
    for (position, &index) in indices.iter().enumerate().skip(first_field) {
        let Some(written) = written_at[position] else {
            continue;
        };
        if field_set.contains(&written) {
            continue;
        }
        let is_return_assembly = matches!(
            block.statements[index].kind,
            MirStatementKind::Construct {
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Tuple,
                    ..
                },
                ..
            }
        );
        if !is_return_assembly {
            return Err(PlanMappingError::mapping(format!(
                "elementwise body storage-buffer count {whole_count} exceeds the \
                 {limit_phrase} and a \
                 statement after the first return-tuple field writer writes a \
                 non-field local outside the return assembly (the split cannot \
                 group the result-tuple output slots)"
            )));
        }
    }

    // The intermediate-slice validity predicate: the range [start..end)
    // fits the budget, exposes at least the minimum consumed-output count,
    // carries no non-kernel-surface consumed local, and never separates a
    // temp definition from its forward. The range state is maintained by
    // the descending candidate scan below.
    let intermediate_valid = |params: &BTreeSet<MirLocalId>,
                              written: &BTreeSet<MirLocalId>,
                              defined: &BTreeSet<MirTempId>,
                              referenced: &BTreeSet<MirTempId>,
                              end: usize|
     -> bool {
        if !referenced.is_subset(defined)
            || defined
                .iter()
                .any(|temp| suffix_temp_references[end].contains(temp))
        {
            return false;
        }
        let consumed: BTreeSet<MirLocalId> =
            written.intersection(&suffix_reads[end]).copied().collect();
        let consumed_count = consumed.len();
        if consumed_count == 0 {
            return false;
        }
        // A tuple-return source's intermediate subchain must expose at
        // least TWO consumed outputs: a single-output intermediate returns
        // nothing (Return(None)) with the source's tuple return type, which
        // the ABI would misread as the FULL result tuple.
        if fields.len() > 1 && consumed_count < 2 {
            return false;
        }
        if params.len() + consumed_count > limit.max_storage_buffers {
            return false;
        }
        // Every consumed local must bind as a kernel buffer (a shape list
        // or vacua seed cut across a boundary would fail the signature).
        consumed.iter().all(|&local| {
            crate::local_ty::local_ty(function, local)
                .is_some_and(|ty| data_flow_element_ty_and_count(ty, validation).is_ok())
        })
    };

    // The LAST-sub-slice validity for the FIELD range: [start..n) keeps the
    // source return assembly, so its signature carries the FULL result-tuple
    // output count and it must write at least one return-tuple field (the
    // caller's result-tuple output slots alias only the params its own
    // update statements read).
    let last_field_valid = |start: usize| -> bool {
        let fields_written = suffix_written[start].intersection(&field_set).count();
        fields_written >= 1
            && suffix_params[start].len() + fields.len() <= limit.max_storage_buffers
    };

    // Split the statement stream: the pre range (before the first field
    // writer) and the field range (from it onward) are split independently,
    // greedily closing the largest valid intermediate prefix at each step.
    // A sub-slice never crosses the pre/field boundary (a mixed subchain's
    // output-set tuple indices would misalign with the full result-tuple
    // indices the caller's output slots alias), and a cut always leaves at
    // least one return-tuple field writer for the last sub-slice (whose
    // signature keeps the FULL result-tuple output count and must write at
    // least one field for the caller's alias to fire).
    let field_positions: Vec<usize> = (first_field..n)
        .filter(|&index| written_at[index].is_some_and(|local| field_set.contains(&local)))
        .collect();
    let Some(&last_field_position) = field_positions.last() else {
        return Err(PlanMappingError::mapping(format!(
            "elementwise body storage-buffer count {whole_count} exceeds the \
             {limit_phrase} and the body \
             writes none of its return-tuple fields (the split cannot preserve \
             the result-tuple output semantics)"
        )));
    };

    let pre_window_states = split_window_states(
        first_field,
        &written_at,
        &reads_at,
        &temps_defined_at,
        &temps_referenced_at,
    );
    let field_window_states = split_window_states(
        last_field_position,
        &written_at,
        &reads_at,
        &temps_defined_at,
        &temps_referenced_at,
    );

    let mut split: Vec<Vec<usize>> = Vec::new();
    let mut start = 0;
    while start < n {
        // The field range's tail [start..n) forms the function's LAST
        // sub-slice when it fits the budget and writes at least one field.
        if start >= first_field && last_field_valid(start) {
            split.push(indices[start..n].to_vec());
            return Ok(split);
        }
        // The largest valid intermediate prefix [start..end): never crosses
        // the pre/field boundary, and always leaves at least one
        // field-writing statement for the last sub-slice.
        let max_end = if start < first_field {
            first_field
        } else {
            last_field_position
        };
        let mut end = max_end.min(n.saturating_sub(1));
        let mut best: Option<usize> = None;
        let mut candidate_count = 0;
        let mut candidate_state = SplitRangeState::default();
        if end > start {
            candidate_state = if start < first_field {
                pre_window_states[start].clone()
            } else {
                field_window_states[start].clone()
            };
        }
        while end > start {
            candidate_count += 1;
            if candidate_count > PD18_MAX_CANDIDATES {
                return Err(PlanMappingError::mapping(
                    "elementwise split candidate window exceeded (maximum 25 candidate ends)",
                ));
            }
            if intermediate_valid(
                &candidate_state.params,
                &candidate_state.written,
                &candidate_state.defined,
                &candidate_state.referenced,
                end,
            ) {
                best = Some(end);
                break;
            }
            let slot = end - 1;
            candidate_state.remove(
                written_at[slot],
                &reads_at[slot],
                &temps_defined_at[slot],
                &temps_referenced_at[slot],
            );
            end -= 1;
        }
        let Some(end) = best else {
            return Err(PlanMappingError::mapping(format!(
                "elementwise body storage-buffer count {whole_count} exceeds the \
                 {limit_phrase} and the \
                 statement stream cannot be split within the budget"
            )));
        };
        split.push(indices[start..end].to_vec());
        start = end;
    }
    Ok(split)
}

/// The temps a statement DEFINES (a temp destination) and REFERENCES (a temp
/// operand) — a cut must never separate a collection call's temp result
/// from the forward that copies it into a local (the local-based read
/// analysis cannot see the temp linkage, so the splitter enforces it
/// directly).
fn statement_temp_sets(statement: &MirStatement) -> (BTreeSet<MirTempId>, BTreeSet<MirTempId>) {
    let mut defined = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    match &statement.kind {
        MirStatementKind::RuntimeCall {
            destination, call, ..
        } => {
            if let Some(place) = destination
                && let MirPlaceBase::Temp(temp) = place.base.clone()
            {
                defined.insert(temp);
            }
            for arg in &call.args {
                if let Some(temp) = operand_temp(arg) {
                    referenced.insert(temp);
                }
            }
        }
        MirStatementKind::Assign { value, .. } => {
            if let Some(temp) = value_temp(value) {
                referenced.insert(temp);
            }
        }
        MirStatementKind::Call {
            destination, args, ..
        } => {
            if let Some(place) = destination
                && let MirPlaceBase::Temp(temp) = place.base.clone()
            {
                defined.insert(temp);
            }
            for arg in args {
                if let Some(temp) = operand_temp(arg) {
                    referenced.insert(temp);
                }
            }
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
            ..
        } => {
            if let MirPlaceBase::Temp(temp) = destination.base.clone() {
                defined.insert(temp);
            }
            let MirAggregateFields::Ordered(items) = &aggregate.fields else {
                return (defined, referenced);
            };
            for item in items {
                if let MirAggregateItem::Operand(operand) = item
                    && let Some(temp) = operand_temp(operand)
                {
                    referenced.insert(temp);
                }
            }
        }
    }
    (defined, referenced)
}

#[derive(Clone, Default)]
struct SplitRangeState {
    read_counts: BTreeMap<MirLocalId, usize>,
    written_counts: BTreeMap<MirLocalId, usize>,
    params: BTreeSet<MirLocalId>,
    written: BTreeSet<MirLocalId>,
    defined_counts: BTreeMap<MirTempId, usize>,
    referenced_counts: BTreeMap<MirTempId, usize>,
    defined: BTreeSet<MirTempId>,
    referenced: BTreeSet<MirTempId>,
}

impl SplitRangeState {
    fn add(
        &mut self,
        written: Option<MirLocalId>,
        reads: &[MirLocalId],
        defined: &BTreeSet<MirTempId>,
        referenced: &BTreeSet<MirTempId>,
    ) {
        for &local in reads {
            *self.read_counts.entry(local).or_default() += 1;
            if !self.written.contains(&local) {
                self.params.insert(local);
            }
        }
        if let Some(local) = written {
            *self.written_counts.entry(local).or_default() += 1;
            self.written.insert(local);
            self.params.remove(&local);
        }
        for &temp in defined {
            *self.defined_counts.entry(temp).or_default() += 1;
            self.defined.insert(temp);
        }
        for &temp in referenced {
            *self.referenced_counts.entry(temp).or_default() += 1;
            self.referenced.insert(temp);
        }
    }

    fn remove(
        &mut self,
        written: Option<MirLocalId>,
        reads: &[MirLocalId],
        defined: &BTreeSet<MirTempId>,
        referenced: &BTreeSet<MirTempId>,
    ) {
        for &local in reads {
            decrement_count(&mut self.read_counts, &local);
        }
        if let Some(local) = written {
            decrement_count(&mut self.written_counts, &local);
            if !self.written_counts.contains_key(&local) {
                self.written.remove(&local);
            }
        }
        for &local in reads {
            self.refresh_param(local);
        }
        if let Some(local) = written {
            self.refresh_param(local);
        }
        for &temp in defined {
            decrement_count(&mut self.defined_counts, &temp);
            if !self.defined_counts.contains_key(&temp) {
                self.defined.remove(&temp);
            }
        }
        for &temp in referenced {
            decrement_count(&mut self.referenced_counts, &temp);
            if !self.referenced_counts.contains_key(&temp) {
                self.referenced.remove(&temp);
            }
        }
    }

    fn refresh_param(&mut self, local: MirLocalId) {
        if self.read_counts.contains_key(&local) && !self.written.contains(&local) {
            self.params.insert(local);
        } else {
            self.params.remove(&local);
        }
    }
}

fn decrement_count<K: Ord>(counts: &mut BTreeMap<K, usize>, key: &K) {
    if let Some(count) = counts.get_mut(key) {
        if *count > 1 {
            *count -= 1;
        } else {
            counts.remove(key);
        }
    }
}

fn split_window_states(
    max_end: usize,
    written_at: &[Option<MirLocalId>],
    reads_at: &[Vec<MirLocalId>],
    temps_defined_at: &[BTreeSet<MirTempId>],
    temps_referenced_at: &[BTreeSet<MirTempId>],
) -> Vec<SplitRangeState> {
    let mut states = vec![SplitRangeState::default(); max_end + 1];
    let mut state = SplitRangeState::default();
    for start in (0..max_end).rev() {
        state.add(
            written_at[start],
            &reads_at[start],
            &temps_defined_at[start],
            &temps_referenced_at[start],
        );
        states[start] = state.clone();
    }
    states
}

/// The temp operand of an operand, if the operand is a plain temp.
fn operand_temp(operand: &MirOperand) -> Option<MirTempId> {
    match operand {
        MirOperand::Temp(temp) => Some(*temp),
        MirOperand::Place(place) => match place.base.clone() {
            MirPlaceBase::Temp(temp) => Some(temp),
            MirPlaceBase::Local(_) => None,
        },
        _ => None,
    }
}

/// The temp operand of a value, if the value is a plain temp forward.
fn value_temp(value: &crate::MirValue) -> Option<MirTempId> {
    match &value.kind {
        crate::MirValueKind::Operand(operand) => operand_temp(operand),
        _ => None,
    }
}

/// The return-tuple field locals in full-tuple order, for a function whose
/// return is an ordered non-spread tuple `Construct` of plain local places —
/// resolving the local-return and the temp-return (the real lowering's
/// shape: the terminator returns a tuple TEMP built by a tuple `Construct`).
/// `None` when the function does not return such a tuple.
fn return_tuple_field_locals(function: &MirFunction) -> Option<Vec<MirLocalId>> {
    if function.blocks.len() != 1 {
        return None;
    }
    let block = &function.blocks[0];
    // The returned place base (local or temp) of the terminator.
    let MirTerminatorKind::Return(Some(operand)) = &block.terminator.kind else {
        return None;
    };
    let base = match operand {
        MirOperand::Place(place) => place.base.clone(),
        MirOperand::Temp(temp) => MirPlaceBase::Temp(*temp),
        _ => return None,
    };
    let destination_matches = |place: &MirPlace| place.base == base;
    block.statements.iter().find_map(|statement| {
        let MirStatementKind::Construct {
            destination,
            aggregate,
        } = &statement.kind
        else {
            return None;
        };
        if !destination_matches(destination) {
            return None;
        }
        if aggregate.kind != MirAggregateKind::Tuple {
            return None;
        }
        let MirAggregateFields::Ordered(items) = &aggregate.fields else {
            return None;
        };
        items
            .iter()
            .map(|item| match item {
                MirAggregateItem::Operand(MirOperand::Place(place)) => {
                    crate::read_analysis::place_local_base(place)
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
    })
}

impl KernelDecomposition {
    /// Build the synthetic single-recipe function the subchain at `index`
    /// emits from (S5-U1b emitter seam).
    ///
    /// Slices the source function's statement stream to the subchain's
    /// statements and rebuilds the params from the subchain's data-flow
    /// inputs — exactly the function the pass resolved the subchain's plan
    /// from — so the emitters can lower a decomposed kernel from the same
    /// typed facts. A scalar-return last subchain keeps the source return
    /// terminator (the scalar-return reduction shape survives the slice). A
    /// buffer-producing subchain returns a TUPLE of its output set
    /// (`[primary, extra_outputs...]`) when it carries additional outputs
    /// (D-2b-1) — or, for a tuple-return LAST subchain (D-6), when its
    /// exposed return-tuple fields are its output set — so the emitters'
    /// multi-output tuple-tail paths can materialize them as extra kernel
    /// outputs; a single-output intermediate subchain returns nothing.
    ///
    /// # Panics
    /// Panics when `index` is out of range (callers validate against
    /// [`Self::subchains`]), or when a multi-output subchain's tuple return
    /// type is not interned in `types` (the caller must intern
    /// `Type::Tuple([...])` of the output element types before building the
    /// synthetic — the tuple element types come from the source function's
    /// locals, which the synthetic keeps).
    #[must_use]
    pub fn subchain_function(
        &self,
        source: &MirFunction,
        index: usize,
        types: &TypeTable,
    ) -> MirFunction {
        let is_last = index + 1 == self.subchains.len();
        let plan_is_elementwise = matches!(
            self.subchains[index].plan,
            CollectionKernelPlan::Elementwise
        );
        build_subchain_function(
            source,
            &self.subchains[index].statements,
            &self.subchains[index].outputs,
            is_last,
            plan_is_elementwise,
            self.subchains[index].output_param,
            types,
        )
    }

    /// Pre-intern the tuple return types of every multi-output intermediate
    /// subchain into `types` (D-2b-2). `subchain_function` holds only
    /// `&TypeTable` and fails closed when a multi-output subchain's tuple
    /// type is missing, so callers run this (idempotent — interning is
    /// hash-consed, and the element types come from the source function's
    /// locals, which the synthetics keep) on the mutable table the subchain
    /// derivation uses before building any synthetic. A multi-output LAST
    /// subchain (a tuple-return function whose return-tuple fields are
    /// exposed as extra outputs, D-6) is interned the same way.
    pub fn intern_output_tuple_types(&self, source: &MirFunction, types: &mut TypeTable) {
        for (position, subchain) in self.subchains.iter().enumerate() {
            let is_last = position + 1 == self.subchains.len();
            if !needs_synthetic_tuple_return(&subchain.outputs, is_last, source) {
                continue;
            }
            let element_ids: Vec<TypeId> = subchain
                .outputs
                .iter()
                .map(|&local| source_local_ty(source, local).semantic_id())
                .collect();
            types.tuple(element_ids);
        }
    }

    /// The FULL result-tuple → local field bindings of a tuple-return
    /// function (D-6): the return-tuple `Construct` binds full-tuple index
    /// `i` → the local carrying that field. The last subchain keeps the
    /// source return assembly, so these bindings describe the decomposition's
    /// result tuple exactly as the caller's full-tuple gradient slots index
    /// it — a companion's `gradient_slot` is a FULL result-tuple index (the
    /// selected input's param position), never any subchain's local output
    /// index.
    ///
    /// `None` when the function does not return a tuple built by an ordered
    /// non-spread `Construct` of plain local places (a scalar return, a bare
    /// local return, or an aggregate the caller cannot index by plain local
    /// field).
    #[must_use]
    pub fn return_tuple_fields(&self, source: &MirFunction) -> Option<Vec<MirLocalId>> {
        source_return_tuple_fields(source)
    }

    /// The producer resolution (D-6): which subchain kernel's output produces
    /// a given local, as `(subchain index, output position in that subchain's
    /// `outputs`)`. The output position doubles as the subchain kernel's
    /// output-slot tuple index (the emission signature's `output` is position
    /// 0 and each `extra_outputs` element follows), so the caller can look up
    /// the produced buffer by `(subchain index, position)`.
    ///
    /// `None` when no subchain exposes the local as an output — the local is
    /// internal to its subchain (never read by a later subchain, so the
    /// D-2a consumed-output pass did not expose it) and has no distinct
    /// kernel buffer to alias.
    #[must_use]
    pub fn producer_position(&self, local: MirLocalId) -> Option<(usize, usize)> {
        self.subchains
            .iter()
            .enumerate()
            .find_map(|(index, subchain)| {
                subchain
                    .outputs
                    .iter()
                    .position(|&output| output == local)
                    .map(|position| (index, position))
            })
    }
}

/// The output-set tuple aggregation slots of a multi-output subchain
/// (S5A-U4): the locals and temps that hold the result-tuple aggregation the
/// ABI flattens into DISTINCT output resources.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResultAggregationSlots {
    /// Tuple-typed locals that are the output-set tuple aggregation.
    pub locals: FxHashSet<MirLocalId>,
    /// Tuple-typed temps that are the output-set tuple aggregation.
    pub temps: FxHashSet<MirTempId>,
}

impl ResultAggregationSlots {
    /// Whether a place base is one of the result-aggregation slots.
    #[must_use]
    pub fn contains(&self, base: &MirPlaceBase) -> bool {
        match base {
            MirPlaceBase::Local(local) => self.locals.contains(local),
            MirPlaceBase::Temp(temp) => self.temps.contains(temp),
        }
    }
}

/// The output-set tuple aggregation slots of one function (S5A-U4) — the
/// device-safety gate's tuple allowance, replacing the program-level
/// multi-output source `DefId` set (radix `ce36c1285`).
///
/// A slot is a result-aggregation slot — the ONLY tuple-typed
/// local/temp/value the gate admits — when the decomposition records it as
/// the explicit output-set tuple a multi-output subchain aggregates:
///
/// 1. **Own result aggregation** — the function's return type is a non-empty
///    tuple and it returns that tuple through the base of the terminator's
///    return operand. This covers a plain tuple-return device function AND a
///    D-2b-1 subchain synthetic whose return IS the output-set tuple (the
///    tuple built into its fresh tuple temp by `build_subchain_function`).
///
/// 2. **Kept source result-tuple local** — a tuple-typed local the subchain
///    synthetics keep from the source's full local table
///    (`build_subchain_function` clones it): the local the source's
///    return-tuple `Construct` wrote into (D-6), which every subchain of the
///    decomposed multi-output source declares even when the fragment's own
///    return is the output-set tuple (the last subchain) or Vacuum (a
///    single-output intermediate subchain — the CUDA artifact override). The
///    local is admitted when it is never READ in the fragment (the kept
///    aggregation is never consumed — the return assembly was replaced by
///    the synthetic's tuple-temp construct) and the fragment returns a
///    non-empty tuple or nothing — exactly the two synthetic shapes a
///    decomposed multi-output source produces. An unrelated LIVE tuple
///    aggregate in a tuple-returning source is read somewhere and stays
///    fail-closed.
///
/// Within rule 2, an intermediate subchain's kept local is never written
/// either; a last subchain's kept local is written by the source's kept
/// Tuple `Construct` (which survives the slice). Both are covered by
/// admitting locals that are written only by a Tuple `Construct` (or not
/// written at all). A local written by an ordinary `Assign`/`RuntimeCall`
/// is not a kept aggregation and stays fail-closed even when dead.
///
/// 3. **Kept source result-tuple temp** — the temp analog of rule 2: a
///    tuple-typed TEMP the subchain synthetics keep from the source's full
///    temp table (`build_subchain_function` clones `source.temps`). A
///    reverse-AD companion whose result aggregation lowers into a TEMP
///    (the bert-tiny backward gradient tuple, `Return(Some(Temp(..)))`))
///    keeps that temp in every subchain, but the synthetic's own return is a
///    FRESH tuple temp (`needs_synthetic_tuple_return`), so the kept source
///    tuple temp is never READ and is written only by the source's Tuple
///    `Construct` (the last subchain) or not written at all (an intermediate
///    subchain). It is admitted under the identical structural conditions as
///    the kept local — and an unrelated LIVE tuple temp stays fail-closed.
#[must_use]
pub fn multi_output_result_aggregation_slots(
    function: &MirFunction,
    types: &TypeTable,
) -> ResultAggregationSlots {
    let mut slots = ResultAggregationSlots::default();

    // Rule 1: the function's OWN result aggregation — the local/temp base of
    // the terminator's return operand when the return type is a non-empty
    // tuple.
    if is_non_empty_tuple_type(types, function.return_ty)
        && let MirTerminatorKind::Return(Some(operand)) = &function.blocks[0].terminator.kind
    {
        match operand {
            MirOperand::Place(place) => match place.base {
                MirPlaceBase::Local(local)
                    if is_non_empty_tuple_type(
                        types,
                        crate::local_ty::local_ty(function, local).unwrap_or(function.return_ty),
                    ) =>
                {
                    slots.locals.insert(local);
                }
                MirPlaceBase::Temp(temp)
                    if is_non_empty_tuple_type(
                        types,
                        function
                            .temps
                            .iter()
                            .find(|entry| entry.id == temp)
                            .map_or(function.return_ty, |entry| entry.ty),
                    ) =>
                {
                    slots.temps.insert(temp);
                }
                _ => {}
            },
            MirOperand::Temp(temp)
                if is_non_empty_tuple_type(
                    types,
                    function
                        .temps
                        .iter()
                        .find(|entry| entry.id == *temp)
                        .map_or(function.return_ty, |entry| entry.ty),
                ) =>
            {
                slots.temps.insert(*temp);
            }
            _ => {}
        }
    }

    // Rule 2: the KEPT source result-tuple local (D-6) and its temp analog.
    collect_kept_result_aggregation_slots(function, types, &mut slots);

    slots
}

fn collect_kept_result_aggregation_slots(
    function: &MirFunction,
    types: &TypeTable,
    slots: &mut ResultAggregationSlots,
) {
    let read = crate::read_analysis::function_read_locals(function);
    let returns_tuple_or_nothing = is_non_empty_tuple_type(types, function.return_ty)
        || function
            .blocks
            .iter()
            .any(|block| matches!(block.terminator.kind, MirTerminatorKind::Return(None)));
    if returns_tuple_or_nothing {
        let mut written: FxHashSet<MirLocalId> = FxHashSet::default();
        let mut tuple_construct_destinations: FxHashSet<MirLocalId> = FxHashSet::default();
        // S5A-U4 (rule 3): the kept source result-tuple TEMP — the temp
        // analog of the kept result-tuple local. A reverse-AD companion
        // whose result aggregation lowers into a temp keeps that temp in
        // every subchain synthetic; the synthetic's own return is a FRESH
        // tuple temp, so the kept source tuple temp is admitted under the
        // identical structural conditions.
        let mut written_temps: FxHashSet<MirTempId> = FxHashSet::default();
        let mut tuple_construct_temp_destinations: FxHashSet<MirTempId> = FxHashSet::default();
        for block in &function.blocks {
            for statement in &block.statements {
                if let MirStatementKind::Construct {
                    destination,
                    aggregate,
                } = &statement.kind
                    && aggregate.kind == MirAggregateKind::Tuple
                {
                    if let Some(local) = crate::read_analysis::place_local_base(destination) {
                        tuple_construct_destinations.insert(local);
                    } else if let MirPlaceBase::Temp(temp) = destination.base {
                        tuple_construct_temp_destinations.insert(temp);
                    }
                }
                if let Some(local) = crate::read_analysis::statement_destination(statement) {
                    written.insert(local);
                }
                if let Some(temp) = statement_destination_temp(statement) {
                    written_temps.insert(temp);
                }
            }
        }
        for local in &function.locals {
            if !is_non_empty_tuple_type(types, local.ty)
                || function.params.iter().any(|param| param.local == local.id)
                || read.contains(&local.id)
            {
                continue;
            }
            if tuple_construct_destinations.contains(&local.id) || !written.contains(&local.id) {
                slots.locals.insert(local.id);
            }
        }
        for temp in &function.temps {
            if !is_non_empty_tuple_type(types, temp.ty)
                || crate::read_analysis::temp_directly_read(function, temp.id)
            {
                continue;
            }
            // A kept source tuple temp admitted only when written solely by a
            // Tuple `Construct` (the last subchain) or not written at all (an
            // intermediate subchain) — mirroring the kept-local rule. A temp
            // written by an ordinary `Assign`/`RuntimeCall` is not a kept
            // aggregation and stays fail-closed even when dead; a LIVE tuple
            // temp (read somewhere) is excluded above by `temp_directly_read`.
            if tuple_construct_temp_destinations.contains(&temp.id)
                || !written_temps.contains(&temp.id)
            {
                slots.temps.insert(temp.id);
            }
        }
    }
}

/// Whether a type is a non-empty tuple (the S3-A1 result-tuple contract).
fn is_non_empty_tuple_type(types: &TypeTable, ty: MirType) -> bool {
    matches!(
        types.get(ty.semantic_id()),
        Type::Tuple(elements) if !elements.is_empty()
    )
}

/// The destination temp of a statement, when it writes a temp place — the
/// temp analog of [`crate::read_analysis::statement_destination`] (which
/// tracks only [`MirPlaceBase::Local`]). Used by the S5A-U4 kept
/// result-tuple temp admission (rule 3 of
/// [`multi_output_result_aggregation_slots`]).
fn statement_destination_temp(statement: &MirStatement) -> Option<MirTempId> {
    let destination: Option<&MirPlace> = match &statement.kind {
        MirStatementKind::Assign { place, .. }
        | MirStatementKind::Construct {
            destination: place, ..
        } => Some(place),
        MirStatementKind::RuntimeCall { destination, .. }
        | MirStatementKind::Call { destination, .. } => destination.as_ref(),
    };
    destination.and_then(|place| match place.base {
        MirPlaceBase::Temp(temp) => Some(temp),
        MirPlaceBase::Local(_) => None,
    })
}

/// The FULL result-tuple → local field bindings of a tuple-return function
/// (D-6): the return-tuple `Construct` binds full-tuple index `i` → the
/// local carrying that field. The last subchain keeps the source return
/// assembly, so these bindings describe the decomposition's result tuple
/// exactly as the caller's full-tuple gradient slots index it. `None` when
/// the function does not return a tuple built by an ordered non-spread
/// `Construct` of plain local places.
fn source_return_tuple_fields(source: &MirFunction) -> Option<Vec<MirLocalId>> {
    if source.blocks.len() != 1 {
        return None;
    }
    let block = &source.blocks[0];
    let MirTerminatorKind::Return(Some(operand)) = &block.terminator.kind else {
        return None;
    };
    // The returned place base: the local-return assembly resolves through the
    // local; a TEMP-return assembly (the `return tuple<..> [...]` lowering
    // constructs into a result temp) resolves through the same construct.
    // Both shapes must resolve — a temp-shaped assembly the D-6 pass cannot
    // see leaves a tuple-return last subchain without its field output set,
    // and the kept live source assembly is then an aggregate the emitters
    // refuse (the same two shapes the ABI's result-tuple scan and the
    // constructor's `tuple_return_locals` resolve).
    let returned = match operand {
        MirOperand::Place(place) => place.base.clone(),
        MirOperand::Temp(temp) => MirPlaceBase::Temp(*temp),
        _ => return None,
    };
    block.statements.iter().find_map(|statement| {
        let MirStatementKind::Construct {
            destination,
            aggregate,
        } = &statement.kind
        else {
            return None;
        };
        if destination.base != returned {
            return None;
        }
        if aggregate.kind != MirAggregateKind::Tuple {
            return None;
        }
        let MirAggregateFields::Ordered(items) = &aggregate.fields else {
            return None;
        };
        items
            .iter()
            .map(|item| match item {
                MirAggregateItem::Operand(MirOperand::Place(place)) => {
                    crate::read_analysis::place_local_base(place)
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
    })
}

/// Whether a subchain's synthetic returns a TUPLE of its output locals (the
/// multi-output contract): a multi-output intermediate subchain, or — D-6 —
/// a tuple-return LAST subchain whose exposed return-tuple fields are its
/// output set (even a single exposed field returns the 1-tuple so the ABI's
/// result-tuple flattening stays consistent with the field buffers).
fn needs_synthetic_tuple_return(
    outputs: &[MirLocalId],
    is_last: bool,
    source: &MirFunction,
) -> bool {
    if outputs.len() > 1 {
        return true;
    }
    is_last && !outputs.is_empty() && source_return_tuple_fields(source).is_some()
}

/// Build the synthetic kernel function for one subchain slice.
///
/// Params = the subchain's external inputs in read order: every local the
/// slice reads that it does not write (source params plus locals produced by
/// earlier subchains — the data-flow). The synthetic function keeps the
/// SOURCE function's full local/temp tables (ids and types preserved), so
/// every typed lookup (`operand_local_ty`, the contract resolver's shape
/// scan) resolves exactly as it does in the source body. The last subchain
/// returns the source function's return local (preserving the scalar-return
/// reduction shape); an intermediate subchain returns nothing when its
/// output set is the single recipe result, and a TUPLE of its output set
/// (`[primary, extra_outputs...]` in `outputs` order) when it carries
/// additional consumed outputs (D-2b-1). An ELEMENTWISE join child (Unit 1A
/// fork/join) instead returns its last destination local: its product is the
/// cross-kernel output, and the return makes the elementwise planner's
/// liveness see that read while the full-ABI synthesis binds the join's
/// output buffer.
///
/// `output_param` (the LAST subchain of a source that stores into an explicit
/// output parameter) makes the synthetic an indexed-view kernel like the
/// source: its params are the slice's inputs, then the output parameter, then
/// the source's trailing index parameters — see
/// [`explicit_output_subchain_params`].
///
/// # Panics
/// Panics when an intermediate subchain carries additional outputs but the
/// caller has not interned the tuple return type in `types`.
fn build_subchain_function(
    source: &MirFunction,
    indices: &[usize],
    outputs: &[MirLocalId],
    is_last: bool,
    plan_is_elementwise: bool,
    output_param: Option<MirLocalId>,
    types: &TypeTable,
) -> MirFunction {
    let block = &source.blocks[0];
    let span = source.span;
    let params = match output_param {
        Some(output) => explicit_output_subchain_params(source, indices, output),
        None => subchain_params(source, indices),
    };

    let mut statements: Vec<MirStatement> = indices
        .iter()
        .map(|&index| block.statements[index].clone())
        .collect();

    // D-2b-1: the return carries the subchain's consumed-output set. A
    // multi-output subchain (an intermediate subchain with ADDITIONAL
    // consumed outputs, or — D-6 — a tuple-return LAST subchain whose
    // return-tuple fields are exposed as its output set) returns a TUPLE of
    // the output locals (`[primary, extra_outputs...]` in the subchain's
    // `outputs` order) — the multi-output contract the ABI recognizes via
    // `result_tuple_count` (`abi.rs`). The tuple element types come from the
    // source function's locals (the synthetic keeps the source local tables);
    // the tuple TYPE must already be interned in `types` (mirroring the
    // reverse-AD `find_tuple` pattern, where the gradient-tuple walk holds
    // only `&TypeTable`). A single-output last subchain keeps the source
    // return terminator — the return local keeps its original id, so the
    // scalar-return reduction shape survives the slice — and a single-output
    // intermediate subchain is a buffer-producing kernel that returns
    // nothing.
    let mut temps = source.temps.clone();
    let (terminator, return_ty) = if needs_synthetic_tuple_return(outputs, is_last, source) {
        let element_ids: Vec<TypeId> = outputs
            .iter()
            .map(|&local| source_local_ty(source, local).semantic_id())
            .collect();
        let tuple_id = types.find_tuple(&element_ids).unwrap_or_else(|| {
            panic!(
                "subchain output set {outputs:?} has no interned tuple return type for \
                 elements {element_ids:?}; the caller must intern \
                 `Type::Tuple(elements)` before building the subchain synthetic"
            )
        });
        let tuple_ty = MirType::semantic(tuple_id);
        let tuple_temp = MirTempId(u32::try_from(temps.len()).unwrap_or(u32::MAX));
        temps.push(MirTemp {
            id: tuple_temp,
            ty: tuple_ty,
            span,
        });
        statements.push(MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::temp(tuple_temp),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Tuple,
                    ty: tuple_ty,
                    fields: MirAggregateFields::Ordered(
                        outputs
                            .iter()
                            .map(|&local| {
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(local)))
                            })
                            .collect(),
                    ),
                },
            },
            span,
        });
        (
            MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Temp(tuple_temp))),
                span,
            },
            tuple_ty,
        )
    } else if is_last {
        // The last subchain keeps the source return terminator: the return
        // local keeps its original id, so the scalar-return reduction shape
        // survives the slice.
        (block.terminator.clone(), source.return_ty)
    } else if plan_is_elementwise {
        elementwise_subchain_return(source, block, indices)
    } else {
        (
            MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span,
            },
            source.return_ty,
        )
    };

    MirFunction {
        id: source.id,
        source: source.source,
        name: source.name,
        params,
        locals: source.locals.clone(),
        temps,
        blocks: vec![MirBlock {
            id: block.id,
            statements,
            terminator,
            span: block.span,
        }],
        return_ty,
        error_ty: source.error_ty,
        is_async: source.is_async,
        is_generator: source.is_generator,
        shader_stage: source.shader_stage,
        span,
    }
}

fn subchain_params(source: &MirFunction, indices: &[usize]) -> Vec<MirParam> {
    let block = &source.blocks[0];
    let mut written: BTreeSet<MirLocalId> = BTreeSet::new();
    let mut read_order: Vec<MirLocalId> = Vec::new();
    for &index in indices {
        let statement = &block.statements[index];
        if let Some(destination) = crate::read_analysis::statement_destination(statement) {
            written.insert(destination);
        }
        for local in crate::read_analysis::statement_read_locals(statement) {
            if !written.contains(&local) && !read_order.contains(&local) {
                read_order.push(local);
            }
        }
    }
    let span = source.span;
    read_order
        .into_iter()
        .map(|local| MirParam {
            local,
            name: None,
            ty: source_local_ty(source, local),
            mode: MirParamMode::Owned,
            span,
        })
        .collect()
}

/// Move a constant statement to the subchains that read it.
///
/// A constant (`_30 = const float 0.5`, typed as a tensor so it broadcasts) has
/// no operands, so the slicing walk folds it into whatever slice is open. When
/// a LATER slice reads it, it becomes an extra output of that kernel at the
/// tensor's full extent. A recipe kernel dispatches over its own result's
/// extent, so such an output can be larger than the dispatch covers (a
/// `[512,16]` matmul carrying five `[512,512]` constants). The constant needs
/// no producer: each subchain that reads one computes it, and the slice that
/// merely held it drops it.
///
/// Only a constant written once, read by a slice other than the one holding it,
/// and not returned moves; every other statement keeps its place.
fn sink_constant_statements(
    function: &MirFunction,
    block: &MirBlock,
    mut slices: Vec<Vec<usize>>,
) -> Vec<Vec<usize>> {
    let statements = &block.statements;
    let mut writes: BTreeMap<MirLocalId, usize> = BTreeMap::new();
    for statement in statements {
        if let Some(local) = crate::read_analysis::statement_destination(statement) {
            *writes.entry(local).or_default() += 1;
        }
    }
    let returned = crate::read_analysis::terminator_return_local(&block.terminator.kind);
    let constants: Vec<(usize, MirLocalId)> = statements
        .iter()
        .enumerate()
        .filter_map(|(index, statement)| {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                return None;
            };
            let MirValueKind::Operand(MirOperand::Constant(_)) = &value.kind else {
                return None;
            };
            let local = crate::read_analysis::place_local_base(place)
                .filter(|_| place.projections.is_empty())?;
            let is_param = function.params.iter().any(|param| param.local == local);
            (writes.get(&local) == Some(&1) && !is_param && returned != Some(local))
                .then_some((index, local))
        })
        .collect();
    for (index, local) in constants {
        let readers: Vec<usize> = slices
            .iter()
            .enumerate()
            .filter(|(_, slice)| {
                slice.iter().any(|&other| {
                    other != index
                        && crate::read_analysis::statement_reads_local(&statements[other], local)
                })
            })
            .map(|(position, _)| position)
            .collect();
        let holder = slices.iter().position(|slice| slice.contains(&index));
        if readers.is_empty() || (readers.len() == 1 && holder == Some(readers[0])) {
            continue;
        }
        for (position, slice) in slices.iter_mut().enumerate() {
            let reads = readers.contains(&position);
            let holds = slice.contains(&index);
            if reads && !holds {
                slice.push(index);
                slice.sort_unstable();
            } else if holds && !reads {
                slice.retain(|&other| other != index);
            }
        }
    }
    slices.retain(|slice| !slice.is_empty());
    slices
}

/// The source function's explicit output parameter, when its body stores into
/// one (the indexed-view kernel shape `y ⇇ d`, a `varia` parameter).
///
/// The final store to `y` is the kernel's output, so the subchain holding it
/// must be built as an indexed-view kernel whose output IS `y`; the
/// contract-shaped subchain signature binds the recipe result as the output
/// instead, and the store to `y` (with the fused tail behind the recipe) is
/// lost. Only the LAST subchain can be that kernel, and it must be the only
/// writer of `y`: an earlier subchain's store to `y` would leave a later
/// subchain nothing to bind it to, and a read of `y` makes it an input as well
/// as the output. Those shapes fail closed rather than drop the store.
///
/// # Errors
/// Returns [`PlanMappingError`] when a store to an output parameter sits in a
/// subchain other than the last, when two output parameters are stored to, or
/// when an output parameter is read.
fn explicit_output_param(
    function: &MirFunction,
    block: &MirBlock,
    slices: &[Vec<usize>],
) -> Result<Option<MirLocalId>, PlanMappingError> {
    let outputs: BTreeSet<MirLocalId> = function
        .params
        .iter()
        .filter(|param| param.mode == MirParamMode::MutRef)
        .map(|param| param.local)
        .collect();
    if outputs.is_empty() {
        return Ok(None);
    }
    let mut stored: Option<MirLocalId> = None;
    for (position, indices) in slices.iter().enumerate() {
        let is_last = position + 1 == slices.len();
        for &index in indices {
            let Some(destination) =
                crate::read_analysis::statement_destination(&block.statements[index])
                    .filter(|local| outputs.contains(local))
            else {
                continue;
            };
            if !is_last {
                return Err(PlanMappingError::mapping(format!(
                    "device function body stores to its output parameter local {} in subchain \
                     {position} of {}; only the final subchain can store to an output parameter",
                    destination.0,
                    slices.len()
                )));
            }
            if stored.is_some_and(|earlier| earlier != destination) {
                return Err(PlanMappingError::mapping(
                    "device function body stores to more than one output parameter and cannot \
                     decompose into subchain kernels (the final subchain has one output)",
                ));
            }
            stored = Some(destination);
        }
    }
    let Some(output) = stored else {
        return Ok(None);
    };
    let reads_output = slices.iter().flatten().any(|&index| {
        crate::read_analysis::statement_reads_local(&block.statements[index], output)
    });
    if reads_output {
        return Err(PlanMappingError::mapping(format!(
            "device function body reads its output parameter local {} and cannot decompose into \
             subchain kernels (a subchain output parameter is write-only)",
            output.0
        )));
    }
    Ok(Some(output))
}

/// The params of the final subchain that stores into the explicit output
/// parameter `output`: the slice's external inputs in read order, then the
/// output parameter, then the source's trailing parameters that its body never
/// reads (the invocation-index parameter of the indexed-view shape
/// `(inputs…, output, index)`). Names and modes are the source's, so the
/// subchain's ABI synthesis reads the same indexed-view surface as the source.
fn explicit_output_subchain_params(
    source: &MirFunction,
    indices: &[usize],
    output: MirLocalId,
) -> Vec<MirParam> {
    let mut params: Vec<MirParam> = subchain_params(source, indices)
        .into_iter()
        .filter(|param| param.local != output)
        .collect();
    let Some(output_position) = source.params.iter().position(|param| param.local == output) else {
        return params;
    };
    params.push(source.params[output_position].clone());
    let read_anywhere: BTreeSet<MirLocalId> = source
        .blocks
        .iter()
        .flat_map(|block| {
            block
                .statements
                .iter()
                .flat_map(crate::read_analysis::statement_read_locals)
        })
        .collect();
    let trailing_unread = source.params[output_position + 1..]
        .iter()
        .rev()
        .take_while(|param| !read_anywhere.contains(&param.local))
        .count();
    let trailing_start = source.params.len() - trailing_unread;
    params.extend(source.params[trailing_start..].iter().cloned());
    params
}

fn elementwise_subchain_return(
    source: &MirFunction,
    block: &MirBlock,
    indices: &[usize],
) -> (MirTerminator, MirType) {
    let span = source.span;
    // Unit 1A fork/join: an elementwise join child's product is its
    // cross-kernel output. The synthetic returns the slice's last
    // destination so the elementwise planner's liveness sees the
    // cross-kernel read (the D-2a output set exposes exactly this
    // local) and the full-ABI synthesis binds the join's output
    // buffer.
    let last_local = indices
        .iter()
        .rev()
        .find_map(|&index| crate::read_analysis::statement_destination(&block.statements[index]));
    if let Some(local) = last_local {
        (
            MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(local)))),
                span,
            },
            source_local_ty(source, local),
        )
    } else {
        (
            MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span,
            },
            source.return_ty,
        )
    }
}

/// The MIR type of a local in the source function (params, then locals) —
/// the synthetic subchain param type. Temps are not referenced by plain
/// local operands (the decomposition's read analysis only tracks
/// [`MirPlaceBase::Local`]), so they never appear as a subchain param.
///
/// Falls back to the source function's return type when the local is neither
/// a param nor a declared local (the decomposition's W5 fallback; the shared
/// lookup returns `None` and this wrapper applies the return-type fallback).
fn source_local_ty(source: &MirFunction, local: MirLocalId) -> MirType {
    crate::local_ty::local_ty(source, local).unwrap_or(source.return_ty)
}

/// Whether a statement's destination is never read anywhere in the source
/// function (the S3-A5 dead-op filter). A destination that IS a function
/// param (a side-effect write, e.g. a matmul writing its output-buffer
/// param) is always live.
fn statement_destination_is_dead(function: &MirFunction, statement: &MirStatement) -> bool {
    let Some(destination) = crate::read_analysis::statement_destination(statement) else {
        return false;
    };
    if function
        .params
        .iter()
        .any(|param| param.local == destination)
    {
        return false;
    }
    for block in &function.blocks {
        for other in &block.statements {
            if crate::read_analysis::statement_reads_local(other, destination) {
                return false;
            }
        }
        if crate::read_analysis::terminator_reads_local(&block.terminator.kind, destination) {
            return false;
        }
    }
    true
}
#[cfg(test)]
#[path = "kernel_decomposition_test.rs"]
mod tests;
