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
//!   `TensorMatMul` / `TensorTranspose`), an elementwise transform
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
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock,
    MirCollectionOp, MirFunction, MirIntrinsic, MirLocalId, MirOperand, MirParam, MirParamMode,
    MirPlace, MirPlaceBase, MirRuntimeCall, MirStatement, MirStatementKind, MirTemp, MirTempId,
    MirTerminator, MirTerminatorKind, MirType, MirValidationContext, MirValue, MirValueKind,
};
use radix_types::{TypeId, TypeTable};
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
        let Some(op) = statement_collection_op(statement) else {
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
        let produced = indices
            .iter()
            .rev()
            .find_map(|&index| statement_destination(&block.statements[index]));
        let written: BTreeSet<MirLocalId> = indices
            .iter()
            .filter_map(|&index| statement_destination(&block.statements[index]))
            .collect();
        let inputs: BTreeSet<MirLocalId> =
            synthetic.params.iter().map(|param| param.local).collect();
        // The recipe that opened this subchain (the plan's result local).
        let recipe_result = indices.iter().find_map(|&index| {
            let statement = &block.statements[index];
            if matches!(statement_collection_op(statement), Some(op) if is_recipe_op(op)) {
                statement_destination(statement)
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
    // subchain's output set is the function's return local (unchanged).
    // Walk backwards accumulating each later subchain's external inputs
    // (its synthetic params) — exactly the locals a later subchain reads
    // from outside itself.
    let mut later_inputs: BTreeSet<MirLocalId> = BTreeSet::new();
    for position in (0..subchains.len()).rev() {
        let written = &written_by[position];
        let mut outputs: Vec<MirLocalId> = Vec::new();
        if position + 1 == subchains.len() {
            if let Some(return_local) = terminator_return_local(&block.terminator.kind) {
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
    /// typed facts. The last subchain keeps the source return terminator
    /// (the scalar-return reduction shape survives the slice). An
    /// intermediate subchain is a buffer-producing kernel: a single-output
    /// subchain returns nothing, and a subchain with additional consumed
    /// outputs (D-2b-1) returns a TUPLE of its output set
    /// (`[primary, extra_outputs...]`) so the emitters' multi-output
    /// tuple-tail paths can materialize them as extra kernel outputs.
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
    /// derivation uses before building any synthetic.
    pub fn intern_output_tuple_types(&self, source: &MirFunction, types: &mut TypeTable) {
        for (position, subchain) in self.subchains.iter().enumerate() {
            let is_last = position + 1 == self.subchains.len();
            if is_last || subchain.outputs.len() <= 1 {
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
        if let Some(destination) = statement_destination(statement) {
            written.insert(destination);
        }
        for local in statement_read_locals(statement) {
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

    // D-2b-1: the return carries the subchain's consumed-output set. The
    // last subchain keeps the source return; a single-output intermediate
    // subchain is a buffer-producing kernel that returns nothing. An
    // intermediate subchain with ADDITIONAL consumed outputs returns a
    // TUPLE of the output locals (`[primary, extra_outputs...]` in the
    // subchain's `outputs` order) — the multi-output contract the ABI
    // recognizes via `result_tuple_count` (`abi.rs`). The tuple element
    // types come from the source function's locals (the synthetic keeps the
    // source local tables); the tuple TYPE must already be interned in
    // `types` (mirroring the reverse-AD `find_tuple` pattern, where the
    // gradient-tuple walk holds only `&TypeTable`).
    let terminator;
    let return_ty;
    let mut temps = source.temps.clone();
    if is_last {
        // The last subchain keeps the source return terminator: the return
        // local keeps its original id, so the scalar-return reduction shape
        // survives the slice.
        terminator = block.terminator.clone();
        return_ty = source.return_ty;
    } else if outputs.len() > 1 {
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
fn source_local_ty(source: &MirFunction, local: MirLocalId) -> MirType {
    source
        .params
        .iter()
        .find(|param| param.local == local)
        .map(|param| param.ty)
        .or_else(|| {
            source
                .locals
                .iter()
                .find(|entry| entry.id == local)
                .map(|entry| entry.ty)
        })
        .unwrap_or(source.return_ty)
}

/// The collection op a statement carries, when it is a collection runtime
/// call.
fn statement_collection_op(statement: &MirStatement) -> Option<MirCollectionOp> {
    let MirStatementKind::RuntimeCall {
        call:
            MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(op),
                ..
            },
        ..
    } = &statement.kind
    else {
        return None;
    };
    Some(*op)
}

/// The destination local a statement writes, when it has one.
fn statement_destination(statement: &MirStatement) -> Option<MirLocalId> {
    let destination = match &statement.kind {
        MirStatementKind::Assign { place, .. }
        | MirStatementKind::Construct {
            destination: place, ..
        } => Some(place),
        MirStatementKind::RuntimeCall { destination, .. }
        | MirStatementKind::Call {
            destination,
            callee: _,
            args: _,
        } => destination.as_ref(),
    };
    destination.and_then(|place| match place.base {
        MirPlaceBase::Local(local) => Some(local),
        // Temps are not plain local destinations (the decomposition's
        // read/write analysis tracks [`MirPlaceBase::Local`] only).
        MirPlaceBase::Temp(_) => None,
    })
}

/// Whether a statement's destination is never read anywhere in the source
/// function (the S3-A5 dead-op filter). A destination that IS a function
/// param (a side-effect write, e.g. a matmul writing its output-buffer
/// param) is always live.
fn statement_destination_is_dead(function: &MirFunction, statement: &MirStatement) -> bool {
    let Some(destination) = statement_destination(statement) else {
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
            if statement_reads_local(other, destination) {
                return false;
            }
        }
        if terminator_reads_local(&block.terminator.kind, destination) {
            return false;
        }
    }
    true
}

/// The locals a statement reads (the statement's own destination is a
/// write, never a read).
fn statement_read_locals(statement: &MirStatement) -> Vec<MirLocalId> {
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => value_read_locals(value),
        MirStatementKind::RuntimeCall { call, .. } => {
            call.args.iter().filter_map(operand_local).collect()
        }
        MirStatementKind::Call { args, .. } => args.iter().filter_map(operand_local).collect(),
        MirStatementKind::Construct { aggregate, .. } => match &aggregate.fields {
            MirAggregateFields::Ordered(fields) => fields
                .iter()
                .filter_map(|field| match field {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        operand_local(operand)
                    }
                })
                .collect(),
            MirAggregateFields::Named(fields) => fields
                .iter()
                .filter_map(|field| operand_local(&field.value))
                .collect(),
            MirAggregateFields::Keyed(fields) => fields
                .iter()
                .filter_map(|field| operand_local(&field.value))
                .collect(),
        },
    }
}

/// The locals a value reads.
fn value_read_locals(value: &MirValue) -> Vec<MirLocalId> {
    match &value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            operand_local(operand).into_iter().collect()
        }
        MirValueKind::Binary { lhs, rhs, .. } => operand_local(lhs)
            .into_iter()
            .chain(operand_local(rhs))
            .collect(),
        _ => Vec::new(),
    }
}

/// The local a plain place operand references, if any.
fn operand_local(operand: &MirOperand) -> Option<MirLocalId> {
    let MirOperand::Place(place) = operand else {
        return None;
    };
    let MirPlaceBase::Local(local) = place.base else {
        return None;
    };
    Some(local)
}

/// Whether a statement reads the given local in any operand or value
/// position (the statement's own destination is a write, never a read).
fn statement_reads_local(statement: &MirStatement, local: MirLocalId) -> bool {
    statement_read_locals(statement).contains(&local)
}

/// Whether a terminator reads the given local.
fn terminator_reads_local(terminator: &MirTerminatorKind, local: MirLocalId) -> bool {
    match terminator {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            operand_local(operand) == Some(local)
        }
        MirTerminatorKind::Branch { condition, .. }
        | MirTerminatorKind::Switch {
            value: condition, ..
        } => operand_local(condition) == Some(local),
        _ => false,
    }
}

/// The plain-local operand a function return terminator returns, when it
/// returns one (the D-2a last-subchain output set). Temps and constants are
/// not subchain outputs.
fn terminator_return_local(terminator: &MirTerminatorKind) -> Option<MirLocalId> {
    match terminator {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            operand_local(operand)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "kernel_decomposition_test.rs"]
mod tests;
