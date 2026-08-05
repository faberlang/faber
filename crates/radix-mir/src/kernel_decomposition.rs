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
//!   like the others), an elementwise transform
//!   ([`is_elementwise_collection_op`](crate::device_program_plans::is_elementwise_collection_op)),
//!   or elementwise value math (Assign / Construct / Call);
//! - every **live** recipe op resolves its own dispatch contract and plan.
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

use crate::device_program_plans::{
    is_elementwise_collection_op, is_recipe_op, resolve_subchain_plan, PlanMappingError,
};
use crate::kernel_plan::CollectionKernelPlan;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirFunction,
    MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirStatement,
    MirStatementKind, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext,
};
// The decomposition's own scan helpers moved to `crate::collection_ops`; the
// collection-op types remain in scope for the `#[path]` test module.
#[cfg(test)]
use crate::{MirCollectionOp, MirIntrinsic, MirRuntimeCall};
use radix_types::{Type, TypeId, TypeTable};
use rustc_hash::FxHashSet;
use std::collections::BTreeSet;

/// One decomposed kernel subchain: a contiguous statement slice of the
/// source function carrying exactly one recipe (or an elementwise-only
/// body), plus the data-flow edges that connect it to the rest of the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelSubchain {
    /// Statement indices into the source function's single block, in order
    /// (the slice this kernel lowers from).
    pub statements: Vec<usize>,
    /// The single plan this subchain carries (elementwise-only subchains
    /// carry [`CollectionKernelPlan::Elementwise`]).
    pub plan: CollectionKernelPlan,
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
/// plan pass, and records the explicit data-flow edges (which earlier
/// subchain's produced local each subchain consumes).
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
    if function.blocks.len() != 1 {
        return Err(PlanMappingError::mapping(
            "device function body has control flow and cannot decompose into subchain kernels \
             (subchains are straight-line statement slices)",
        ));
    }
    let block = &function.blocks[0];

    // Slice the statement stream at recipe boundaries. `current` holds the
    // source statement indices of the open subchain; `current_has_recipe`
    // tracks whether the open subchain already adopted a recipe (a second
    // live recipe op closes it and starts a new slice).
    let mut slices: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_has_recipe = false;
    for (index, statement) in block.statements.iter().enumerate() {
        let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
            // Elementwise value math (Assign / Construct / Call).
            current.push(index);
            continue;
        };
        if is_recipe_op(op) {
            // S3-A5: a recipe op whose result is never read is dead — not
            // part of the kernel semantics, and it must not force a recipe
            // boundary or a contract the live plan does not carry.
            if statement_destination_is_dead(function, statement) {
                continue;
            }
            if current_has_recipe {
                slices.push(std::mem::take(&mut current));
            }
            current_has_recipe = true;
            current.push(index);
            continue;
        }
        if is_elementwise_collection_op(op) {
            current.push(index);
            continue;
        }
        return Err(PlanMappingError::mapping(format!(
            "collection op {op:?} has no kernel plan \
             (no recipe and not an elementwise transform)"
        )));
    }
    if !current.is_empty() {
        slices.push(current);
    }

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
        let synthetic = build_subchain_function(function, indices, &[], is_last, validation.types);
        // The S3-A5 dead filter resolves reads against the SOURCE function
        // (the subchain's produced local is consumed by a later subchain,
        // invisible inside the slice itself).
        let plan = resolve_subchain_plan(&synthetic, function, validation)?;
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
            consumes,
            produced,
            outputs: Vec::new(),
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
                    if written.contains(&field) && !outputs.contains(&field) {
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
        build_subchain_function(
            source,
            &self.subchains[index].statements,
            &self.subchains[index].outputs,
            is_last,
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
#[must_use]
pub fn multi_output_result_aggregation_slots(
    function: &MirFunction,
    types: &TypeTable,
) -> ResultAggregationSlots {
    let mut slots = ResultAggregationSlots::default();

    // Rule 1: the function's OWN result aggregation — the local/temp base of
    // the terminator's return operand when the return type is a non-empty
    // tuple.
    if is_non_empty_tuple_type(types, function.return_ty) {
        if let MirTerminatorKind::Return(Some(operand)) = &function.blocks[0].terminator.kind {
            match operand {
                MirOperand::Place(place) => match place.base {
                    MirPlaceBase::Local(local)
                        if is_non_empty_tuple_type(
                            types,
                            crate::local_ty::local_ty(function, local)
                                .unwrap_or(function.return_ty),
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
    }

    // Rule 2: the KEPT source result-tuple local (D-6). The kept local is
    // never read (the fragment's return assembly is the synthetic's
    // tuple-temp construct, or nothing), and the fragment is one of the two
    // shapes a decomposed multi-output source produces: a non-empty-tuple
    // return (the last subchain / a multi-output intermediate) or nothing (a
    // single-output intermediate subchain).
    let read = crate::read_analysis::function_read_locals(function);
    let returns_tuple_or_nothing = is_non_empty_tuple_type(types, function.return_ty)
        || function
            .blocks
            .iter()
            .any(|block| matches!(block.terminator.kind, MirTerminatorKind::Return(None)));
    if returns_tuple_or_nothing {
        let mut written: FxHashSet<MirLocalId> = FxHashSet::default();
        let mut tuple_construct_destinations: FxHashSet<MirLocalId> = FxHashSet::default();
        for block in &function.blocks {
            for statement in &block.statements {
                if let MirStatementKind::Construct {
                    destination,
                    aggregate,
                } = &statement.kind
                {
                    if aggregate.kind == MirAggregateKind::Tuple {
                        if let Some(local) = crate::read_analysis::place_local_base(destination) {
                            tuple_construct_destinations.insert(local);
                        }
                    }
                }
                if let Some(local) = crate::read_analysis::statement_destination(statement) {
                    written.insert(local);
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
    }

    slots
}

/// Whether a type is a non-empty tuple (the S3-A1 result-tuple contract).
fn is_non_empty_tuple_type(types: &TypeTable, ty: MirType) -> bool {
    matches!(
        types.get(ty.semantic_id()),
        Type::Tuple(elements) if !elements.is_empty()
    )
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
    let return_local = crate::read_analysis::terminator_return_local(&block.terminator.kind)?;
    block.statements.iter().find_map(|statement| {
        let MirStatementKind::Construct {
            destination,
            aggregate,
        } = &statement.kind
        else {
            return None;
        };
        if crate::read_analysis::place_local_base(destination) != Some(return_local) {
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
/// additional consumed outputs (D-2b-1).
///
/// # Panics
/// Panics when an intermediate subchain carries additional outputs but the
/// caller has not interned the tuple return type in `types`.
fn build_subchain_function(
    source: &MirFunction,
    indices: &[usize],
    outputs: &[MirLocalId],
    is_last: bool,
    types: &TypeTable,
) -> MirFunction {
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
    let params = read_order
        .into_iter()
        .map(|local| MirParam {
            local,
            name: None,
            ty: source_local_ty(source, local),
            mode: MirParamMode::Owned,
            span,
        })
        .collect();

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
    let terminator;
    let return_ty;
    let mut temps = source.temps.clone();
    if needs_synthetic_tuple_return(outputs, is_last, source) {
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
        terminator = MirTerminator {
            kind: MirTerminatorKind::Return(Some(MirOperand::Temp(tuple_temp))),
            span,
        };
        return_ty = tuple_ty;
    } else if is_last {
        // The last subchain keeps the source return terminator: the return
        // local keeps its original id, so the scalar-return reduction shape
        // survives the slice.
        terminator = block.terminator.clone();
        return_ty = source.return_ty;
    } else {
        terminator = MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span,
        };
        return_ty = source.return_ty;
    }

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
