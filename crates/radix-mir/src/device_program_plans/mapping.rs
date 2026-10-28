//! Typed `MirCollectionOp -> CollectionKernelPlan` mapping.

use super::{
    PlanMappingError, is_contract_recipe_op, is_elementwise_collection_op, is_recipe_op,
    is_transformer_recipe_op, kernel_matmul_shapes, scan_plan,
};
use crate::abi::{
    CollectionOpContract, DEFAULT_STORAGE_BUFFER_LIMIT, MatMulDispatch, MirKernelResourceKind,
    MirKernelSignature, StorageBufferLimitRecord, collection_op_contract,
    storage_buffer_limit_error,
};
use crate::kernel_plan::{CollectionKernelPlan, MatMulShapes};
use crate::{
    MirCollectionOp, MirFunction, MirIntrinsic, MirLocalId, MirOperand, MirPlaceBase,
    MirRuntimeCall, MirStatementKind, MirValidationContext,
};

// ---------------------------------------------------------------------------
// PLAN MAPPING
// ---------------------------------------------------------------------------

/// The typed `MirCollectionOp → CollectionKernelPlan` mapping (council TB5).
///
/// Maps a collection intrinsic plus the resolved dispatch contract onto the
/// closed recipe set through the shared plan pass
/// ([`CollectionKernelPlan::from_contract`]).
///
/// **Fail-closed (N3.2):** an op that has no recipe under the resolved
/// contract is a [`PlanMappingError`], never a silent
/// [`CollectionKernelPlan::Elementwise`] floor. Elementwise kernels are
/// decided by the function-level scan ([`kernel_plan_for_function`]) from
/// the body's complete op set; this op-level mapping admits recipe ops only
/// — an elementwise op under a recipe contract is a dispatch/op mismatch and
/// fails closed here too.
///
/// The caller must already hold the resolved [`CollectionOpContract`] (the
/// sibling of the plan pass; `abi::collection_op_contract`) and, for a
/// `TensorMatMul` op, the exact operand shapes `(M, K, N)`; the contract
/// stops at the tile grid, the recipe needs the exact dims.
///
/// # Errors
/// Returns [`PlanMappingError`] when the op has no recipe under the resolved
/// contract (unplannable collection op, or an elementwise op under a recipe
/// contract).
pub fn plan_for_collection_op(
    signature: &MirKernelSignature,
    contract: &CollectionOpContract,
    op: MirCollectionOp,
    matmul_shapes: Option<MatMulShapes>,
) -> Result<CollectionKernelPlan, PlanMappingError> {
    CollectionKernelPlan::from_contract(signature, contract, op, matmul_shapes).map_or_else(
        || {
            Err(PlanMappingError::mapping(format!(
                "collection op {op:?} has no kernel plan under the resolved {contract:?} contract \
                     (no recipe; elementwise kernels are decided by the function-level scan, \
                     never by a silent Elementwise fallback)"
            )))
        },
        Ok,
    )
}

/// Re-derive the collection-op plan from a kernel function's typed facts.
///
/// Runs the same typed inputs the shared plan pass consumes — the **full**
/// statement scan's collection-op set (never the first matching statement),
/// the ABI dispatch contract, and (for matmul) the operand dims resolved
/// from the operands' tensor types — and returns the plan the function
/// carries, or `None` for a kernel whose op set is elementwise-only (no
/// recipe).
///
/// The emitters use this to cross-check a [`DeviceProgram`] kernel's **carried**
/// plan against the MIR function it was built from: a carried recipe that
/// disagrees with the function's typed op facts fails closed (A3) instead of
/// emitting a body that contradicts the declared plan.
///
/// # Errors
/// Returns [`PlanMappingError`] when the contract resolver cannot resolve the
/// operand shapes (rank, static dims, inner-dimension agreement), when a
/// recipe op has no resolved contract or no plan under it, when a body mixes
/// collection ops with conflicting recipes, or when the body carries a
/// collection op that is neither recipe-plannable nor a known elementwise
/// transform: **never a silent `Elementwise`.**
///
/// `TensorTranspose` is a recipe op (S5-U1); the S6-P1 transformer recipes
/// (`TensorSumAxis` / `TensorSoftmax` / `TensorLayerNorm`) resolve through
/// [`CollectionKernelPlan::from_transformer_op`] (no `CollectionOpContract`
/// variant exists for them). A body whose complete op set MIXES distinct
/// recipes cannot carry one plan and fails here with a typed diagnostic —
/// the decomposition pass
/// ([`crate::kernel_decomposition::decompose_kernel_function`]) is the path
/// that splits such bodies into single-recipe subchain kernels.
pub fn kernel_plan_for_function(
    function: &MirFunction,
    signature: &MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionKernelPlan>, PlanMappingError> {
    kernel_plan_for_function_with_limit(
        function,
        signature,
        validation,
        DEFAULT_STORAGE_BUFFER_LIMIT,
    )
}

/// Re-derive a kernel plan against an explicit lane-owned storage-buffer
/// capacity. The default wrapper keeps the conservative shared minimum.
///
/// # Errors
/// Returns [`PlanMappingError`] when the signature exceeds the selected lane
/// limit or its typed recipe facts do not resolve.
pub fn kernel_plan_for_function_with_limit(
    function: &MirFunction,
    signature: &MirKernelSignature,
    validation: &MirValidationContext<'_>,
    limit: StorageBufferLimitRecord,
) -> Result<Option<CollectionKernelPlan>, PlanMappingError> {
    for statement in function.blocks.iter().flat_map(|block| &block.statements) {
        if let MirStatementKind::RuntimeCall { call, .. } = &statement.kind
            && call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::TensorExpand)
            && crate::tensor_expansion::TensorExpansion::for_call(function, call, validation)
                .is_none()
        {
            return Err(PlanMappingError::mapping(
                "tensor expansion has no exact compatible static tensor witness",
            ));
        }
    }
    // A kernel whose storage-buffer binding count exceeds the per-lane
    // `StorageBufferLimitRecord` capacity cannot be emitted as ONE kernel
    // (the shared default is the minimum across admitted lanes). The whole-function plan is refused
    // with a typed diagnostic so the callers (faber, both emitters) fall
    // through to the decomposition path, which splits the body into
    // budget-fitting kernels — never a silent Elementwise floor.
    let buffer_count = signature
        .resources()
        .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
        .count();
    if !limit.admits(buffer_count) {
        return Err(PlanMappingError::mapping(
            storage_buffer_limit_error(buffer_count, limit).message,
        ));
    }
    if let Some(plan) =
        scan_plan(function, signature, validation).map_err(PlanMappingError::mapping)?
    {
        return Ok(Some(plan));
    }
    // A lane-form kernel (it carries the compiler-internal `Lane` builtin) is
    // one launch grid of independent scalar bodies; its `abs`/`maximum`/`sqrt`
    // collection-op statements are scalar math, not tensor recipes, so the
    // recipe scan does not apply. The caller derives the `LaneGrid` plan.
    if function
        .params
        .iter()
        .any(|param| param.mode == crate::MirParamMode::Lane)
    {
        return Ok(None);
    }
    // SFR-5 declared-recipe scan: a `FilumSum` declaration is the author-
    // stated recipe. The collection-op statement scan never sees it (the
    // declaration is not a collection op), so it resolves ahead of the op
    // scan; the source length `K` comes from the kernel signature's source
    // buffer — never guessed from the body. A second declared or recognized
    // recipe in the same body fails closed (one kernel, one recipe).
    //
    // The declared recipe resolves BEFORE the single-lane classification:
    // `FilumReduce` is not a `MirCollectionOp`, so `is_single_lane_form`
    // cannot see it, and a sequential single-lane early return here would
    // swallow the recipe (SFR-5).
    let filum_declarations = filum_sum_declarations(function);
    if !filum_declarations.is_empty() {
        if filum_declarations.len() > 1 {
            return Err(PlanMappingError::mapping(
                "kernel body carries more than one filum summa declaration (one recipe per kernel)",
            ));
        }
        let (filum, source_local) = &filum_declarations[0];
        let length = signature
            .resources()
            .find(|resource| resource.source_local == Some(*source_local))
            .map(|resource| resource.element_count)
            .ok_or_else(|| {
                PlanMappingError::mapping(format!(
                    "filum summa source local {source_local:?} has no storage-buffer resource in the kernel signature"
                ))
            })?;
        let plan = CollectionKernelPlan::from_filum_summa(filum, length).ok_or_else(|| {
            PlanMappingError::mapping(format!(
                "filum summa declaration has no kernel plan under its typed facts \
                 (lane width {}, source length {length})",
                filum.lane_width
            ))
        })?;
        for op in crate::collection_ops::collection_ops_for_function(function) {
            if is_contract_recipe_op(op) || is_transformer_recipe_op(op) {
                return Err(PlanMappingError::mapping(format!(
                    "filum summa kernel body mixes a second recipe ({op:?}) with the declared distributed sum"
                )));
            }
        }
        return Ok(Some(plan));
    }
    // A single-lane kernel (no lane loop, one `mut` tensor) is a sequential
    // body: its collection-op statements are scalar math too. The caller
    // selects the `SingleLane` plan.
    if MirKernelSignature::is_single_lane_form(function, validation) {
        return Ok(None);
    }
    // Live-recipe-statement census: the op scan below is deduplicated by op
    // kind, so two LIVE statements of the SAME recipe op would adopt ONE
    // plan by plan-equality — silently folding a multi-recipe body into a
    // single kernel whose matmul shapes read only the first statement. Fail
    // closed instead: one kernel, one recipe statement. Distinct-op mixes
    // stay with the scan below, whose `adopt_recipe` owns the pinned
    // "conflicting recipes" diagnostic.
    let stability_max_dead = crate::abi::stability_max_is_dead(function);
    let live_recipes = live_recipe_statements(function, stability_max_dead);
    if live_recipes.len() > 1 && live_recipes.iter().all(|op| *op == live_recipes[0]) {
        return Err(PlanMappingError::mapping(format!(
            "kernel body carries {} live {:?} recipe statements \
             (one recipe statement per kernel)",
            live_recipes.len(),
            live_recipes[0],
        )));
    }
    // Scan ALL statements: the kernel body's plan is decided by its complete
    // collection-op set, never by the first matching statement (N3.2).
    let ops = crate::collection_ops::collection_ops_for_function(function);
    if ops.is_empty() {
        return Ok(None);
    }
    // The ABI dispatch contract (reduction / matmul), when the body carries a
    // recipe op; elementwise-only bodies resolve no contract. Matmul shapes
    // are resolved once from the body's matmul statement scan.
    let contract = collection_op_contract(function, validation)?;
    // Run-time extents (`MatMulDispatch::Runtime`) carry no literal shapes:
    // the plan is built from the signature alone and the dims are read at
    // launch.
    let matmul_shapes = if matches!(
        contract,
        Some(CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime
        })
    ) {
        None
    } else {
        kernel_matmul_shapes(function, validation)?
    };
    resolve_recipe_for_ops(
        ops,
        signature,
        function,
        validation,
        contract,
        matmul_shapes,
        stability_max_dead,
    )
}

fn resolve_recipe_for_ops(
    ops: Vec<MirCollectionOp>,
    signature: &MirKernelSignature,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    contract: Option<CollectionOpContract>,
    matmul_shapes: Option<MatMulShapes>,
    stability_max_dead: bool,
) -> Result<Option<CollectionKernelPlan>, PlanMappingError> {
    let mut recipe: Option<CollectionKernelPlan> = None;
    for op in ops {
        // PGC-R5: a DEAD stability-max reduction is not a recipe — the
        // same dead skip the contract resolver applies (the `_stability_max`
        // idiom of the causal-softmax entries; the emitters' dead skips are
        // the precedent).
        if stability_max_dead
            && matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin)
        {
            continue;
        }
        if is_transformer_recipe_op(op) {
            // S6-P1: the transformer recipes have no `CollectionOpContract`
            // variant; the plan pass resolves them from the function's typed
            // facts (axis/epsilon from the trailing const operands, dims
            // from the operand tensor types) and cross-checks the signature's
            // element counts. A second distinct recipe must agree exactly
            // (never guessed).
            let plan =
                CollectionKernelPlan::from_transformer_op(signature, function, validation, op)
                    .ok_or_else(|| {
                        PlanMappingError::mapping(format!(
                            "collection op {op:?} has no kernel plan under its typed facts \
                             (axis/epsilon from the const operands, shapes from the tensor types)"
                        ))
                    })?;
            adopt_recipe(&mut recipe, op, plan)?;
        } else if is_contract_recipe_op(op) {
            // Contract recipes resolve through the shared closed-set
            // predicate — the same boundary the decomposition slice uses; a
            // second distinct recipe must agree exactly (never guessed).
            let Some(contract) = &contract else {
                return Err(PlanMappingError::mapping(format!(
                    "collection op {op:?} has a recipe but no resolved dispatch contract"
                )));
            };
            let shapes = if op == MirCollectionOp::TensorMatMul {
                matmul_shapes
            } else {
                None
            };
            let plan = CollectionKernelPlan::from_contract(signature, contract, op, shapes)
                .ok_or_else(|| {
                    PlanMappingError::mapping(format!(
                        "collection op {op:?} has no kernel plan under the resolved {contract:?} contract"
                    ))
                })?;
            adopt_recipe(&mut recipe, op, plan)?;
        } else if is_elementwise_collection_op(op) {
            // Elementwise transforms contribute no recipe; the function-level
            // scan decides them elementwise.
        } else {
            return Err(PlanMappingError::mapping(format!(
                "collection op {op:?} has no kernel plan \
                 (no recipe and not an elementwise transform)"
            )));
        }
    }
    Ok(recipe)
}

/// The body's LIVE recipe statements in scan order (the F2 census) —
/// statement level, never deduplicated by op kind the way the op scan is.
/// A recipe statement whose destination is never read is dead (the same
/// dead filter `collection_ops_with_read_context` applies), and a PGC-R5
/// dead stability-max `TensorMax`/`TensorMin` is not a recipe (the same
/// skip the op scan applies).
fn live_recipe_statements(
    function: &MirFunction,
    stability_max_dead: bool,
) -> Vec<MirCollectionOp> {
    let mut recipes = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            let MirStatementKind::RuntimeCall { destination, .. } = &statement.kind else {
                continue;
            };
            if stability_max_dead
                && matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin)
            {
                continue;
            }
            if !is_recipe_op(op) {
                continue;
            }
            if destination
                .as_ref()
                .is_some_and(|dest| !crate::read_analysis::place_is_read(function, dest))
            {
                continue;
            }
            recipes.push(op);
        }
    }
    recipes
}

/// The kernel body's `FilumSum` declarations (SFR-5): each entry is the
/// declaration facts plus the source local its lone tensor argument reads.
/// The caller admits exactly one per kernel (one recipe per kernel).
fn filum_sum_declarations(function: &MirFunction) -> Vec<(crate::MirFilumReduceOp, MirLocalId)> {
    let aliases = crate::abi::ParameterAliases::new(function);
    let mut declarations = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::FilumReduce(filum),
                        args,
                        ..
                    },
                ..
            } = &statement.kind
            else {
                continue;
            };
            if filum.form != crate::MirFilumForm::Declared {
                continue;
            }
            let Some(MirOperand::Place(place)) = args.first() else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let MirPlaceBase::Local(source_local) = place.base else {
                continue;
            };
            // Composition binds a callee's tensor parameter by a whole copy of
            // the caller's: the declaration reads the parameter's buffer
            // through the copy.
            let source_local = aliases.resolve(source_local);
            declarations.push((filum.clone(), source_local));
        }
    }
    declarations
}

fn adopt_recipe(
    recipe: &mut Option<CollectionKernelPlan>,
    op: MirCollectionOp,
    plan: CollectionKernelPlan,
) -> Result<(), PlanMappingError> {
    if let Some(existing) = recipe.as_ref()
        && existing != &plan
    {
        return Err(PlanMappingError::mapping(format!(
            "kernel body mixes collection ops with conflicting recipes \
                 ({op:?} resolves to {plan:?}, but the body already carries {existing:?})"
        )));
    }
    *recipe = Some(plan);
    Ok(())
}
