//! Named unroll pass for AIR-lane `itera ab` range loops.
//!
//! Post-typecheck, pre-`air_purity`. Eligible concrete-`N` `itera ab` loops in
//! `@ radix lane "air"` functions become straight-line bindings so the two
//! admissible differentiable subsets reach reverse AD:
//!
//! - full-prefix (`len == N`): guard-free straight line (unit-3E);
//! - guard-as-mask (`len ≤ N`): all `N` lanes run; every reduction exit is
//!   multiplied by a caller-built 0/1 mask operand (unit-3F).
//!
//! The pass owns:
//! - the unroll budget (64, the `MAX_MONOMORPHIZATIONS` convention);
//! - the `len`-guard preservation proof (a proven-exact iteration count emits
//!   no `If`);
//! - mask-mode admit conditions (mask-as-operand, dead-lane NaN safety, body
//!   purity, every exit masked) — fail-closed with a `causa`;
//! - evaluation-order and failure-behavior preservation (lanes splice in
//!   source order);
//! - fail-closed decline through `SEM059.itera_loop` plus a `causa` arg.
//!
//! Non-AIR-lane functions are untouched. Generic-`N` and unknown bounds
//! decline. In-region mask construction (`Fill` / runtime comparison) never
//! admits.

use crate::tensor_type_directed::is_tensor_type;
use crate::{
    IndexExpr, RadixLane, RadixLaneMetadata, SemanticError, SemanticErrorKind, Type, TypeId,
    TypeTable,
};
use radix_hir::visit::{
    walk_block, walk_expr, walk_expr_mut, walk_item, walk_program, walk_stmt, walk_stmt_mut,
    HirVisitor, HirVisitorMut,
};
use radix_hir::{
    DefId, HirArrayElement, HirBinOp, HirBlock, HirConversioTarget, HirExpression,
    HirExpressionKind, HirFunction, HirId, HirItemKind, HirIteraMode, HirLiteral, HirLocal,
    HirModule, HirParam, HirRangeKind, HirStatement, HirStatementKind, HirUnOp,
};
use radix_lexer::{Interner, Span, Symbol};
use rustc_hash::{FxHashMap, FxHashSet};

/// Unroll budget. Same constant as `MAX_MONOMORPHIZATIONS` in MIR lowering.
pub const UNROLL_BUDGET: usize = 64;

const CAUSA_BUDGET: &str = "budget";
const CAUSA_IMPURE: &str = "impure";
const CAUSA_UNSUPPORTED_CONTROL: &str = "unsupported_control";
const CAUSA_NOT_CONCRETE: &str = "not_concrete";
const CAUSA_IN_REGION_MASK: &str = "in_region_mask";
const CAUSA_NAN_UNSAFE: &str = "nan_unsafe";
const CAUSA_UNMASKED_EXIT: &str = "unmasked_exit";
const CAUSA_DEFER_COORDINATES: &str = "defer_coordinates";

/// Rewrite eligible AIR-lane `itera ab` range loops into straight-line bindings.
///
/// # Errors
///
/// Returns [`SemanticError`]s for loops the pass declines. Eligible loops are
/// rewritten in place before the error list is returned, so a mixed function
/// can admit one loop and fail closed on another.
pub fn unroll(
    hir: &mut HirModule,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    let mut ids = IdGen::from_module(hir);
    let mut errors = Vec::new();

    for item in &mut hir.items {
        if lanes.lane_for(item.def_id) != Some(RadixLane::Air) {
            continue;
        }
        let HirItemKind::Function(function) = &mut item.kind else {
            continue;
        };
        unroll_function(function, lanes, interner, types, &mut ids, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn unroll_function(
    function: &mut HirFunction,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    types: &TypeTable,
    ids: &mut IdGen,
    errors: &mut Vec<SemanticError>,
) {
    let Some(body) = &mut function.body else {
        return;
    };
    let mask = MaskAnalysis::analyze(&function.params, body, types, interner);
    let fullness = FullnessFacts::from_block(body, types);
    let mut emitted_lanes = 0;
    rewrite_block(
        body,
        lanes,
        interner,
        ids,
        errors,
        &mask,
        types,
        &fullness,
        &mut emitted_lanes,
    );
}

fn rewrite_block(
    block: &mut HirBlock,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    ids: &mut IdGen,
    errors: &mut Vec<SemanticError>,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
) {
    let mut index = 0;
    while index < block.statements.len() {
        // Classify `itera` before walking into it so a nested range loop is
        // still visible to the outer decline check.
        let plan = classify_itera_stmt(
            &block.statements[index],
            block,
            lanes,
            interner,
            mask,
            types,
            fullness,
            emitted_lanes,
        );
        if plan.is_none() {
            rewrite_nested_stmt(
                &mut block.statements[index],
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
        }
        let Some(plan) = plan else {
            index += 1;
            continue;
        };
        match plan {
            UnrollPlan::Decline { span, causa } => {
                errors.push(decline(span, causa));
                index += 1;
            }
            UnrollPlan::Ready(ready) => {
                let (unrolled, acc_final) = emit_unrolled_lanes(&ready, interner, ids);
                freeze_accumulators(block, &acc_final);
                let mut rest = block.statements.split_off(index + 1);
                block.statements.pop();
                remap_acc_paths_in_stmts(&mut rest, &acc_final);
                if let Some(tail) = &mut block.expr {
                    remap_acc_paths_in_expr(tail, &acc_final);
                }
                let inserted = unrolled.len();
                block.statements.extend(unrolled);
                block.statements.extend(rest);
                index += inserted;
            }
        }
    }

    if let Some(mut expr) = block.expr.take() {
        rewrite_nested_expr(
            &mut expr,
            lanes,
            interner,
            ids,
            errors,
            mask,
            types,
            fullness,
            emitted_lanes,
        );
        match classify_itera_expr(&expr, block, lanes, interner, mask, types, fullness, emitted_lanes) {
            Some(UnrollPlan::Decline { span, causa }) => {
                errors.push(decline(span, causa));
                block.expr = Some(expr);
            }
            Some(UnrollPlan::Ready(ready)) => {
                let span = expr.span;
                let ty = expr.ty;
                let (unrolled, acc_final) = emit_unrolled_lanes(&ready, interner, ids);
                freeze_accumulators(block, &acc_final);
                remap_acc_paths_in_stmts(&mut block.statements, &acc_final);
                block.expr = Some(Box::new(HirExpression {
                    id: ids.next_hir(),
                    kind: HirExpressionKind::Block(HirBlock::simple(unrolled, None, span)),
                    ty,
                    span,
                }));
            }
            None => block.expr = Some(expr),
        }
    }
}

fn rewrite_nested_stmt(
    stmt: &mut HirStatement,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    ids: &mut IdGen,
    errors: &mut Vec<SemanticError>,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
) {
    match &mut stmt.kind {
        HirStatementKind::Local(local) => {
            if let Some(init) = &mut local.init {
                rewrite_nested_expr(
                    init,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
            }
        }
        HirStatementKind::Expr(expr) | HirStatementKind::Redde(Some(expr)) => {
            rewrite_nested_expr(
                expr,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
        }
        HirStatementKind::Custodi(custodi) => {
            for clause in &mut custodi.clauses {
                rewrite_nested_expr(
                    &mut clause.cond,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
                rewrite_block(
                    &mut clause.body,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
            }
        }
        HirStatementKind::Redde(None)
        | HirStatementKind::Rumpe
        | HirStatementKind::Perge
        | HirStatementKind::Tacet
        | HirStatementKind::IncDec(_) => {}
    }
}

fn rewrite_nested_expr(
    expr: &mut HirExpression,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    ids: &mut IdGen,
    errors: &mut Vec<SemanticError>,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
) {
    match &mut expr.kind {
        HirExpressionKind::Block(block) => rewrite_block(
            block,
            lanes,
            interner,
            ids,
            errors,
            mask,
            types,
            fullness,
            emitted_lanes,
        ),
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            rewrite_nested_expr(
                cond,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
            rewrite_block(
                then_block,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
            if let Some(catch) = then_catch {
                rewrite_block(
                    &mut catch.body,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
            }
            if let Some(else_block) = else_block {
                rewrite_block(
                    else_block,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
            }
        }
        HirExpressionKind::Discerne {
            scrutinees, arms, ..
        } => {
            for scrutinee in scrutinees {
                rewrite_nested_expr(
                    scrutinee,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
            }
            for arm in arms {
                if let Some(guard) = &mut arm.guard {
                    rewrite_nested_expr(
                        guard,
                        lanes,
                        interner,
                        ids,
                        errors,
                        mask,
                        types,
                        fullness,
                        emitted_lanes,
                    );
                }
                rewrite_nested_expr(
                    &mut arm.body,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
            }
        }
        HirExpressionKind::Loop(block) | HirExpressionKind::Itera(_, _, _, _, block) => {
            rewrite_block(
                block,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
        }
        HirExpressionKind::Dum(cond, block) => {
            rewrite_nested_expr(
                cond,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
            rewrite_block(
                block,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
        }
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            rewrite_nested_expr(
                lhs,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
            rewrite_nested_expr(
                rhs,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
        }
        HirExpressionKind::Unary(_, operand)
        | HirExpressionKind::Field(operand, _)
        | HirExpressionKind::Praefixum(operand)
        | HirExpressionKind::Ref(_, operand)
        | HirExpressionKind::Deref(operand)
        | HirExpressionKind::Cede(operand)
        | HirExpressionKind::Reddet(operand)
        | HirExpressionKind::Tacebit(operand)
        | HirExpressionKind::Yield(operand)
        | HirExpressionKind::TypeCheck { expr: operand, .. } => {
            rewrite_nested_expr(
                operand,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
        }
        HirExpressionKind::Index(object, index) => {
            rewrite_nested_expr(
                object,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
            rewrite_nested_expr(
                index,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
        }
        HirExpressionKind::Call(callee, _, args) => {
            rewrite_nested_expr(
                callee,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
            for arg in args {
                rewrite_nested_expr(
                    &mut arg.expr,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
            }
        }
        HirExpressionKind::MethodCall(receiver, _, _, args) => {
            rewrite_nested_expr(
                receiver,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
            );
            for arg in args {
                rewrite_nested_expr(
                    &mut arg.expr,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                );
            }
        }
        _ => {}
    }
}

/// Static collection lengths proven from immutable construction sites.
///
/// A bounded carrier stores only its live prefix, so its capacity is not an
/// iteration count. This fact table records an exact length only for an
/// immutable literal/conversion chain; parameters and mutable values stay
/// unknown and therefore cannot admit Route A.
struct FullnessFacts;

impl FullnessFacts {
    fn from_block(block: &HirBlock, types: &TypeTable) -> FxHashMap<DefId, usize> {
        let mut facts = FxHashMap::default();
        let mut collector = Collector { facts: &mut facts, types };
        collector.visit_block(block);
        facts
    }
}

struct Collector<'a> {
    facts: &'a mut FxHashMap<DefId, usize>,
    types: &'a TypeTable,
}

impl HirVisitor for Collector<'_> {
    fn visit_local(&mut self, local: &HirLocal) {
        if !local.mutable {
            if let Some(init) = &local.init {
                if let Some(length) = static_length(init, self.facts, self.types) {
                    self.facts.insert(local.def_id, length);
                }
            }
        } else {
            self.facts.remove(&local.def_id);
        }
        radix_hir::visit::walk_local(self, local);
    }
}

fn static_length(
    expr: &HirExpression,
    facts: &FxHashMap<DefId, usize>,
    types: &TypeTable,
) -> Option<usize> {
    match &expr.kind {
        HirExpressionKind::Array(elements)
            if elements
                .iter()
                .all(|element| matches!(element, HirArrayElement::Expr(_))) =>
        {
            Some(elements.len())
        }
        HirExpressionKind::Path(def_id) => facts.get(def_id).copied(),
        HirExpressionKind::Conversio {
            source,
            target: HirConversioTarget::Type(target),
            ..
        } => {
            let length = static_length(source, facts, types)?;
            if let Some(capacity) = bounded_capacity(*target, types) {
                (capacity == length).then_some(length)
            } else {
                Some(length)
            }
        }
        HirExpressionKind::Verte { source, .. } => static_length(source, facts, types),
        _ => None,
    }
}

fn bounded_capacity(ty: TypeId, types: &TypeTable) -> Option<usize> {
    let (_, ty) = types.peel_access_marker(ty);
    let Type::BoundedArray { capacity, .. } = types.get(ty) else {
        return None;
    };
    let IndexExpr::Literal(value) = types.get_index(*capacity) else {
        return None;
    };
    usize::try_from(*value).ok()
}

enum UnrollPlan {
    Ready(ReadyUnroll),
    Decline { span: Span, causa: &'static str },
}

struct ReadyUnroll {
    index_def: DefId,
    lanes: Vec<i64>,
    body: HirBlock,
    accumulators: FxHashMap<DefId, AccInfo>,
    tensor_source: Option<HirExpression>,
    tensor_binding_ty: Option<TypeId>,
}

#[derive(Clone, Copy)]
struct AccInfo {
    ty: Option<crate::TypeId>,
}

fn classify_itera_stmt(
    stmt: &HirStatement,
    block: &HirBlock,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
) -> Option<UnrollPlan> {
    let HirStatementKind::Expr(expr) = &stmt.kind else {
        return None;
    };
    classify_itera_expr(expr, block, lanes, interner, mask, types, fullness, emitted_lanes)
}

fn classify_itera_expr(
    expr: &HirExpression,
    block: &HirBlock,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
) -> Option<UnrollPlan> {
    let HirExpressionKind::Itera(mode, index_def, _, source, body) = &expr.kind else {
        return None;
    };
    match mode {
        HirIteraMode::Ab => Some(classify_ab_loop(
            expr.span,
            *index_def,
            source,
            body,
            block,
            lanes,
            interner,
            mask,
            emitted_lanes,
        )),
        HirIteraMode::Ex => Some(classify_ex_loop(
            expr.span,
            *index_def,
            source,
            body,
            block,
            lanes,
            interner,
            mask,
            types,
            fullness,
            emitted_lanes,
        )),
        HirIteraMode::De => Some(UnrollPlan::Decline {
            span: expr.span,
            causa: CAUSA_NOT_CONCRETE,
        }),
    }
}

fn classify_ab_loop(
    span: Span,
    index_def: DefId,
    source: &HirExpression,
    body: &HirBlock,
    block: &HirBlock,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    emitted_lanes: &mut usize,
) -> UnrollPlan {
    let Some(values) = concrete_range_values(source) else {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_NOT_CONCRETE,
        };
    };
    if values.len() > UNROLL_BUDGET.saturating_sub(*emitted_lanes) {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_BUDGET,
        };
    }
    if has_runtime_len_guard(body, index_def) {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_IN_REGION_MASK,
        };
    }
    if let Some(causa) = mask.decline() {
        return UnrollPlan::Decline { span, causa };
    }
    if let Some(causa) = body_decline(body, block, index_def, lanes, interner) {
        return UnrollPlan::Decline { span, causa };
    }
    let accumulators = block_accumulators(block, body);
    *emitted_lanes += values.len();
    UnrollPlan::Ready(ReadyUnroll {
        index_def,
        lanes: values,
        body: body.clone(),
        accumulators,
        tensor_source: None,
        tensor_binding_ty: None,
    })
}

fn classify_ex_loop(
    span: Span,
    index_def: DefId,
    source: &HirExpression,
    body: &HirBlock,
    block: &HirBlock,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
) -> UnrollPlan {
    let Some(source_ty) = source.ty else {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_NOT_CONCRETE,
        };
    };
    let (_, source_ty) = types.peel_access_marker(source_ty);
    if let Type::BoundedArray { element, .. } = types.get(source_ty) {
        let Some(lane_count) = bounded_capacity(source_ty, types) else {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_NOT_CONCRETE,
            };
        };
        if static_length(source, fullness, types) != Some(lane_count) {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_NOT_CONCRETE,
            };
        }
        if lane_count > UNROLL_BUDGET.saturating_sub(*emitted_lanes) {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_BUDGET,
            };
        }
        if let Some(causa) = mask.decline() {
            return UnrollPlan::Decline { span, causa };
        }
        if let Some(causa) = body_decline(body, block, index_def, lanes, interner) {
            return UnrollPlan::Decline { span, causa };
        }

        let accumulators = block_accumulators(block, body);
        *emitted_lanes += lane_count;
        #[allow(clippy::cast_possible_wrap)]
        let values = (0..lane_count).map(|lane| lane as i64).collect();
        return UnrollPlan::Ready(ReadyUnroll {
            index_def,
            lanes: values,
            body: body.clone(),
            accumulators,
            tensor_source: Some(source.clone()),
            tensor_binding_ty: Some(*element),
        });
    }
    let Type::Tensor(element_ty, shape) = types.get(source_ty) else {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_NOT_CONCRETE,
        };
    };

    let lane_count = match types.get_index(*shape) {
        IndexExpr::Literal(value) => match usize::try_from(*value) {
            Ok(count) => count,
            Err(_) => {
                return UnrollPlan::Decline {
                    span,
                    causa: CAUSA_BUDGET,
                };
            }
        },
        IndexExpr::Tuple(dims) if dims.len() > 1 => {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_DEFER_COORDINATES,
            };
        }
        IndexExpr::Tuple(dims) if dims.len() == 1 => {
            if !matches!(types.get_index(dims[0]), IndexExpr::Literal(_)) {
                return UnrollPlan::Decline {
                    span,
                    causa: CAUSA_NOT_CONCRETE,
                };
            }
            match types.get_index(dims[0]) {
                IndexExpr::Literal(value) => match usize::try_from(*value) {
                    Ok(count) => count,
                    Err(_) => {
                        return UnrollPlan::Decline {
                            span,
                            causa: CAUSA_BUDGET,
                        };
                    }
                },
                _ => unreachable!("literal tensor dimension was checked above"),
            }
        }
        IndexExpr::Tuple(_)
        | IndexExpr::Param(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_NOT_CONCRETE,
            };
        }
    };

    if lane_count > UNROLL_BUDGET.saturating_sub(*emitted_lanes) {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_BUDGET,
        };
    }
    if let Some(causa) = mask.decline() {
        return UnrollPlan::Decline { span, causa };
    }
    if let Some(causa) = body_decline(body, block, index_def, lanes, interner) {
        return UnrollPlan::Decline { span, causa };
    }

    let accumulators = block_accumulators(block, body);
    *emitted_lanes += lane_count;
    #[allow(clippy::cast_possible_wrap)]
    let values = (0..lane_count).map(|lane| lane as i64).collect();
    UnrollPlan::Ready(ReadyUnroll {
        index_def,
        lanes: values,
        body: body.clone(),
        accumulators,
        tensor_source: Some(source.clone()),
        tensor_binding_ty: Some(*element_ty),
    })
}

fn concrete_range_values(source: &HirExpression) -> Option<Vec<i64>> {
    let HirExpressionKind::Intervallum {
        start,
        end,
        step,
        kind,
    } = &source.kind
    else {
        return None;
    };
    let start = as_i64(start)?;
    let end = as_i64(end)?;
    let step = match step {
        Some(step) => as_i64(step)?,
        None => 1,
    };
    if step == 0 {
        return None;
    }
    let mut values = Vec::new();
    let mut current = start;
    while range_holds(current, end, step, *kind) {
        if values.len() > UNROLL_BUDGET {
            values.push(current);
            break;
        }
        values.push(current);
        current = current.checked_add(step)?;
    }
    Some(values)
}

fn range_holds(current: i64, end: i64, step: i64, kind: HirRangeKind) -> bool {
    match kind {
        HirRangeKind::Exclusive if step > 0 => current < end,
        HirRangeKind::Exclusive => current > end,
        HirRangeKind::Inclusive if step > 0 => current <= end,
        HirRangeKind::Inclusive => current >= end,
    }
}

fn as_i64(expr: &HirExpression) -> Option<i64> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => i64::try_from(*value).ok(),
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => match &inner.kind {
            HirExpressionKind::Literal(HirLiteral::Int(value)) => {
                i64::try_from(*value).ok().map(|n| -n)
            }
            _ => None,
        },
        _ => None,
    }
}

fn body_decline(
    body: &HirBlock,
    parent: &HirBlock,
    index_def: DefId,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
) -> Option<&'static str> {
    let same_block_locals = block_local_defs(parent);
    let mut walker = BodyDecline {
        index_def,
        same_block_locals,
        lanes,
        interner,
        at_loop_body_root: false,
        causa: None,
    };
    walker.scan_loop_body(body);
    walker.causa
}

fn block_local_defs(block: &HirBlock) -> FxHashSet<DefId> {
    block
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => Some(local.def_id),
            _ => None,
        })
        .collect()
}

fn block_accumulators(block: &HirBlock, body: &HirBlock) -> FxHashMap<DefId, AccInfo> {
    let locals: FxHashMap<DefId, AccInfo> = block
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => Some((local.def_id, AccInfo { ty: local.ty })),
            _ => None,
        })
        .collect();
    let mut assigned = FxHashSet::default();
    collect_straight_assigns(body, &mut assigned);
    locals
        .into_iter()
        .filter(|(def, _)| assigned.contains(def))
        .collect()
}

fn collect_straight_assigns(block: &HirBlock, assigned: &mut FxHashSet<DefId>) {
    for stmt in &block.statements {
        if let HirStatementKind::Expr(expr) = &stmt.kind {
            if let HirExpressionKind::Assign(lhs, _) = &expr.kind {
                if let HirExpressionKind::Path(def) = lhs.kind {
                    assigned.insert(def);
                }
            }
        }
    }
}

struct BodyDecline<'a> {
    index_def: DefId,
    same_block_locals: FxHashSet<DefId>,
    lanes: &'a RadixLaneMetadata,
    interner: &'a Interner,
    at_loop_body_root: bool,
    causa: Option<&'static str>,
}

impl BodyDecline<'_> {
    fn reject(&mut self, causa: &'static str) {
        if self.causa.is_none() {
            self.causa = Some(causa);
        }
    }

    fn scan_loop_body(&mut self, body: &HirBlock) {
        self.at_loop_body_root = true;
        self.scan_block(body);
    }

    fn scan_block(&mut self, block: &HirBlock) {
        let root = self.at_loop_body_root;
        self.at_loop_body_root = false;
        if block.breakable.is_some() {
            self.reject(CAUSA_UNSUPPORTED_CONTROL);
        }
        for stmt in &block.statements {
            self.at_loop_body_root = root;
            self.scan_stmt(stmt);
        }
        self.at_loop_body_root = false;
        if let Some(expr) = &block.expr {
            self.scan_expr(expr, false);
        }
    }

    fn scan_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                if let Some(init) = &local.init {
                    self.scan_expr(init, false);
                }
            }
            HirStatementKind::Redde(_)
            | HirStatementKind::Rumpe
            | HirStatementKind::Perge
            | HirStatementKind::Custodi(_) => self.reject(CAUSA_UNSUPPORTED_CONTROL),
            HirStatementKind::Tacet | HirStatementKind::IncDec(_) => self.reject(CAUSA_IMPURE),
            HirStatementKind::Expr(expr) => self.scan_expr(expr, true),
        }
    }

    fn scan_expr(&mut self, expr: &HirExpression, as_statement: bool) {
        match &expr.kind {
            HirExpressionKind::Assign(lhs, rhs) => {
                match &lhs.kind {
                    HirExpressionKind::Path(def) => {
                        if *def == self.index_def || !self.same_block_locals.contains(def) {
                            self.reject(CAUSA_IMPURE);
                        } else if !self.at_loop_body_root {
                            self.reject(CAUSA_UNSUPPORTED_CONTROL);
                        }
                    }
                    _ => self.reject(CAUSA_IMPURE),
                }
                self.scan_expr(rhs, false);
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                if then_catch.is_some() {
                    self.reject(CAUSA_UNSUPPORTED_CONTROL);
                }
                self.scan_expr(cond, false);
                let saved = self.at_loop_body_root;
                self.at_loop_body_root = false;
                self.scan_block(then_block);
                if let Some(else_block) = else_block {
                    self.scan_block(else_block);
                }
                self.at_loop_body_root = saved;
            }
            HirExpressionKind::Block(block) => {
                let saved = self.at_loop_body_root;
                self.at_loop_body_root = false;
                self.scan_block(block);
                self.at_loop_body_root = saved;
            }
            HirExpressionKind::Loop(_)
            | HirExpressionKind::Dum(_, _)
            | HirExpressionKind::Itera(_, _, _, _, _)
            | HirExpressionKind::Discerne { .. } => self.reject(CAUSA_UNSUPPORTED_CONTROL),
            HirExpressionKind::Call(callee, _, args) => {
                if !matches!(&callee.kind, HirExpressionKind::Path(def) if self.lanes.lane_for(*def) == Some(RadixLane::Air))
                {
                    self.reject(CAUSA_IMPURE);
                }
                self.scan_expr(callee, false);
                for arg in args {
                    if arg.name.is_some() || arg.spread {
                        self.reject(CAUSA_IMPURE);
                    }
                    self.scan_expr(&arg.expr, false);
                }
            }
            HirExpressionKind::MethodCall(receiver, method, _, args) => {
                let name = self.interner.resolve(*method);
                if !is_allowed_tensor_intrinsic(name) {
                    self.reject(CAUSA_IMPURE);
                }
                self.scan_expr(receiver, false);
                for arg in args {
                    if arg.name.is_some() || arg.spread {
                        self.reject(CAUSA_IMPURE);
                    }
                    self.scan_expr(&arg.expr, false);
                }
            }
            HirExpressionKind::Scribe(_, _)
            | HirExpressionKind::ReadLine(_)
            | HirExpressionKind::Adfirma(_, _)
            | HirExpressionKind::Clausura(_, _, _, _, _)
            | HirExpressionKind::Conversio { .. }
            | HirExpressionKind::ConversioAssign { .. }
            | HirExpressionKind::Ref(_, _)
            | HirExpressionKind::Deref(_)
            | HirExpressionKind::Ad { .. }
            | HirExpressionKind::TypeCheck { .. }
            | HirExpressionKind::Cede(_)
            | HirExpressionKind::Reddet(_)
            | HirExpressionKind::Tacebit(_)
            | HirExpressionKind::Yield(_)
            | HirExpressionKind::Gradient { .. }
            | HirExpressionKind::OptionalChain(_, _)
            | HirExpressionKind::NonNull(_, _)
            | HirExpressionKind::Handled { .. } => self.reject(CAUSA_IMPURE),
            HirExpressionKind::Panic(_) | HirExpressionKind::Throw(_) => {
                walk_expr(self, expr);
            }
            _ if as_statement
                && !matches!(
                    expr.kind,
                    HirExpressionKind::Si { .. }
                        | HirExpressionKind::Block(_)
                        | HirExpressionKind::Assign(_, _)
                ) =>
            {
                self.reject(CAUSA_IMPURE);
            }
            _ => walk_expr(self, expr),
        }
    }
}

impl HirVisitor for BodyDecline<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        self.scan_expr(expr, false);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        self.scan_stmt(stmt);
    }

    fn visit_block(&mut self, block: &HirBlock) {
        self.scan_block(block);
    }
}

fn emit_unrolled_lanes(
    ready: &ReadyUnroll,
    interner: &mut Interner,
    ids: &mut IdGen,
) -> (Vec<HirStatement>, FxHashMap<DefId, DefId>) {
    let mut acc_current: FxHashMap<DefId, DefId> = ready
        .accumulators
        .keys()
        .copied()
        .map(|def| (def, def))
        .collect();
    let mut out = Vec::new();
    for value in &ready.lanes {
        let mut remap = Remap {
            index_def: ready.index_def,
            index_value: *value,
            index_ty: None,
            binding_value: None,
            acc_current: acc_current.clone(),
            local_map: FxHashMap::default(),
        };
        if let Some(source) = &ready.tensor_source {
            let mut source = source.clone();
            remap_fresh_expr(&mut source, &remap, ids);
            let binding_def = ids.next_def();
            let index = HirExpression {
                id: ids.next_hir(),
                kind: int_literal(*value, ids),
                ty: None,
                span: source.span,
            };
            let element = HirExpression {
                id: ids.next_hir(),
                kind: HirExpressionKind::Index(Box::new(source), Box::new(index)),
                ty: ready.tensor_binding_ty,
                span: ready.body.span,
            };
            let binding = HirLocal {
                def_id: binding_def,
                name: interner.intern(&format!("__unroll_{}", binding_def.0)),
                ty: ready.tensor_binding_ty,
                init: Some(element),
                mutable: false,
                is_await: false,
                runtime_provided: false,
            };
            out.push(HirStatement {
                id: ids.next_hir(),
                kind: HirStatementKind::Local(binding),
                span: ready.body.span,
            });
            remap.binding_value = Some(binding_def);
        }
        let stmts = emit_iteration(&ready.body, &ready.accumulators, &mut remap, interner, ids);
        acc_current = remap.acc_current;
        out.extend(stmts);
    }
    (out, acc_current)
}

fn emit_iteration(
    body: &HirBlock,
    accumulators: &FxHashMap<DefId, AccInfo>,
    remap: &mut Remap,
    interner: &mut Interner,
    ids: &mut IdGen,
) -> Vec<HirStatement> {
    let mut out = Vec::new();
    for stmt in &body.statements {
        match &stmt.kind {
            HirStatementKind::Expr(expr) => {
                if let HirExpressionKind::Assign(lhs, rhs) = &expr.kind {
                    if let HirExpressionKind::Path(acc) = lhs.kind {
                        if let Some(info) = accumulators.get(&acc) {
                            let mut rhs = rhs.as_ref().clone();
                            remap_fresh_expr(&mut rhs, remap, ids);
                            let new_def = ids.next_def();
                            out.push(ssa_local(new_def, *info, rhs, stmt.span, interner, ids));
                            remap.acc_current.insert(acc, new_def);
                            continue;
                        }
                    }
                }
                let mut stmt = stmt.clone();
                remap_fresh_stmt(&mut stmt, remap, ids);
                out.push(stmt);
            }
            HirStatementKind::Local(local) => {
                let new_def = ids.next_def();
                remap.local_map.insert(local.def_id, new_def);
                let mut local = local.clone();
                local.def_id = new_def;
                local.name = interner.intern(&format!("__unroll_{}", new_def.0));
                local.mutable = false;
                if let Some(init) = &mut local.init {
                    remap_fresh_expr(init, remap, ids);
                }
                out.push(HirStatement {
                    id: ids.next_hir(),
                    kind: HirStatementKind::Local(local),
                    span: stmt.span,
                });
            }
            _ => {
                let mut stmt = stmt.clone();
                remap_fresh_stmt(&mut stmt, remap, ids);
                out.push(stmt);
            }
        }
    }
    if let Some(expr) = &body.expr {
        let mut expr = expr.clone();
        if let HirExpressionKind::Assign(lhs, rhs) = &expr.kind {
            if let HirExpressionKind::Path(acc) = lhs.kind {
                if let Some(info) = accumulators.get(&acc) {
                    let mut rhs = rhs.as_ref().clone();
                    remap_fresh_expr(&mut rhs, remap, ids);
                    let new_def = ids.next_def();
                    out.push(ssa_local(new_def, *info, rhs, expr.span, interner, ids));
                    remap.acc_current.insert(acc, new_def);
                    return out;
                }
            }
        }
        remap_fresh_expr(&mut expr, remap, ids);
        out.push(HirStatement {
            id: ids.next_hir(),
            kind: HirStatementKind::Expr(*expr),
            span: body.span,
        });
    }
    out
}

fn ssa_local(
    def_id: DefId,
    info: AccInfo,
    init: HirExpression,
    span: Span,
    interner: &mut Interner,
    ids: &mut IdGen,
) -> HirStatement {
    let name = interner.intern(&format!("__unroll_{}", def_id.0));
    HirStatement {
        id: ids.next_hir(),
        kind: HirStatementKind::Local(HirLocal {
            def_id,
            name,
            ty: info.ty.or(init.ty),
            init: Some(init),
            mutable: false,
            is_await: false,
            runtime_provided: false,
        }),
        span,
    }
}

struct Remap {
    index_def: DefId,
    index_value: i64,
    index_ty: Option<crate::TypeId>,
    binding_value: Option<DefId>,
    acc_current: FxHashMap<DefId, DefId>,
    local_map: FxHashMap<DefId, DefId>,
}

impl Remap {
    fn mapped(&self, def: DefId) -> Option<DefId> {
        self.local_map
            .get(&def)
            .copied()
            .or_else(|| self.acc_current.get(&def).copied())
    }
}

fn remap_fresh_stmt(stmt: &mut HirStatement, remap: &Remap, ids: &mut IdGen) {
    stmt.id = ids.next_hir();
    let mut visitor = FreshRemap { remap, ids };
    walk_stmt_mut(&mut visitor, stmt);
}

fn remap_fresh_expr(expr: &mut HirExpression, remap: &Remap, ids: &mut IdGen) {
    let mut visitor = FreshRemap { remap, ids };
    visitor.visit_expr_mut(expr);
}

struct FreshRemap<'a> {
    remap: &'a Remap,
    ids: &'a mut IdGen,
}

impl HirVisitorMut for FreshRemap<'_> {
    fn visit_stmt_mut(&mut self, stmt: &mut HirStatement) {
        stmt.id = self.ids.next_hir();
        walk_stmt_mut(self, stmt);
    }

    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        expr.id = self.ids.next_hir();
        if let HirExpressionKind::Path(def) = expr.kind {
            if def == self.remap.index_def {
                if let Some(binding) = self.remap.binding_value {
                    expr.kind = HirExpressionKind::Path(binding);
                } else {
                    expr.kind = int_literal(self.remap.index_value, self.ids);
                    if expr.ty.is_none() {
                        expr.ty = self.remap.index_ty;
                    }
                }
                return;
            }
            if let Some(mapped) = self.remap.mapped(def) {
                expr.kind = HirExpressionKind::Path(mapped);
                return;
            }
        }
        walk_expr_mut(self, expr);
    }
}

fn remap_acc_paths_in_stmts(stmts: &mut [HirStatement], acc_final: &FxHashMap<DefId, DefId>) {
    let mut visitor = AccRemap { acc_final };
    for stmt in stmts {
        walk_stmt_mut(&mut visitor, stmt);
    }
}

fn remap_acc_paths_in_expr(expr: &mut HirExpression, acc_final: &FxHashMap<DefId, DefId>) {
    let mut visitor = AccRemap { acc_final };
    visitor.visit_expr_mut(expr);
}

struct AccRemap<'a> {
    acc_final: &'a FxHashMap<DefId, DefId>,
}

impl HirVisitorMut for AccRemap<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        if let HirExpressionKind::Path(def) = &mut expr.kind {
            if let Some(mapped) = self.acc_final.get(def) {
                *def = *mapped;
                return;
            }
        }
        walk_expr_mut(self, expr);
    }
}

fn freeze_accumulators(block: &mut HirBlock, acc_final: &FxHashMap<DefId, DefId>) {
    for stmt in &mut block.statements {
        if let HirStatementKind::Local(local) = &mut stmt.kind {
            if acc_final.contains_key(&local.def_id) {
                local.mutable = false;
            }
        }
    }
}

fn int_literal(value: i64, ids: &mut IdGen) -> HirExpressionKind {
    if value < 0 {
        HirExpressionKind::Unary(
            HirUnOp::Neg,
            Box::new(HirExpression {
                id: ids.next_hir(),
                kind: HirExpressionKind::Literal(HirLiteral::Int(value.unsigned_abs())),
                ty: None,
                span: Span::default(),
            }),
        )
    } else {
        // WHY: unroll bounds are non-negative by construction (loop indices).
        #[allow(clippy::cast_sign_loss)]
        HirExpressionKind::Literal(HirLiteral::Int(value as u64))
    }
}

fn decline(span: Span, causa: &'static str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::AirPurityViolation, span)
        .with_arg("issue", "itera_loop")
        .with_arg("lane", "air")
        .with_arg("causa", causa)
}

/// Function-level facts for the guard-as-mask admit (unit-3F).
///
/// A tensor parameter that is never used as data (only as a `multiplica`
/// operand, or unused beside a data tensor) is a mask operand. Mask mode
/// then fail-closes on in-region construction, dead-lane-unsafe `divisio`,
/// and unmasked reduction exits.
struct MaskAnalysis {
    mask_defs: FxHashSet<DefId>,
    in_region_mask: bool,
    nan_unsafe: bool,
    unmasked_exit: bool,
}

impl MaskAnalysis {
    fn analyze(
        params: &[HirParam],
        body: &HirBlock,
        types: &TypeTable,
        interner: &Interner,
    ) -> Self {
        let tensor_params: FxHashSet<DefId> = params
            .iter()
            .filter(|param| is_tensor_type(param.ty, types))
            .map(|param| param.def_id)
            .collect();
        let mut analysis = MaskCollector {
            interner,
            tensor_params: tensor_params.clone(),
            aliases: FxHashMap::default(),
            local_inits: FxHashMap::default(),
            fill_defs: FxHashSet::default(),
            data_use: FxHashSet::default(),
            reductions: Vec::new(),
            in_region_mask: false,
            nan_unsafe: false,
        };
        analysis.scan_block(body);
        let mask_defs: FxHashSet<DefId> = tensor_params
            .iter()
            .copied()
            .filter(|def| !analysis.data_use.contains(def))
            .collect();
        let unmasked_exit = !mask_defs.is_empty()
            && analysis.reductions.iter().any(|operand| {
                !is_masked_exit(operand, &mask_defs, &analysis.local_inits, interner)
            });
        Self {
            mask_defs,
            in_region_mask: analysis.in_region_mask,
            nan_unsafe: analysis.nan_unsafe,
            unmasked_exit,
        }
    }

    fn decline(&self) -> Option<&'static str> {
        if self.in_region_mask {
            return Some(CAUSA_IN_REGION_MASK);
        }
        if self.mask_defs.is_empty() {
            return None;
        }
        if self.nan_unsafe {
            return Some(CAUSA_NAN_UNSAFE);
        }
        if self.unmasked_exit {
            return Some(CAUSA_UNMASKED_EXIT);
        }
        None
    }
}

struct MaskCollector<'a> {
    interner: &'a Interner,
    tensor_params: FxHashSet<DefId>,
    aliases: FxHashMap<DefId, DefId>,
    local_inits: FxHashMap<DefId, HirExpression>,
    fill_defs: FxHashSet<DefId>,
    data_use: FxHashSet<DefId>,
    reductions: Vec<HirExpression>,
    in_region_mask: bool,
    nan_unsafe: bool,
}

impl MaskCollector<'_> {
    fn resolve(&self, def: DefId) -> DefId {
        self.aliases.get(&def).copied().unwrap_or(def)
    }

    fn note_data(&mut self, expr: &HirExpression) {
        if let Some(def) = path_def(expr) {
            let resolved = self.resolve(def);
            if self.tensor_params.contains(&resolved) {
                self.data_use.insert(resolved);
            }
        }
    }

    fn is_fill_built(&self, expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::MethodCall(recv, method, _, _) => {
                is_fill_method(self.interner.resolve(*method)) || self.is_fill_built(recv)
            }
            HirExpressionKind::Path(def) => {
                self.fill_defs.contains(def)
                    || self
                        .local_inits
                        .get(def)
                        .is_some_and(|init| self.is_fill_built(init))
            }
            _ => false,
        }
    }

    fn scan_block(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            self.scan_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.scan_expr(expr);
        }
    }

    fn scan_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                if let Some(init) = &local.init {
                    self.scan_expr(init);
                    self.local_inits.insert(local.def_id, init.clone());
                    if let Some(src) = path_def(init) {
                        let resolved = self.resolve(src);
                        self.aliases.insert(local.def_id, resolved);
                        if self.fill_defs.contains(&src) || self.is_fill_built(init) {
                            self.fill_defs.insert(local.def_id);
                        }
                    } else if self.is_fill_built(init) {
                        self.fill_defs.insert(local.def_id);
                    }
                    if let HirExpressionKind::Path(def) = init.kind {
                        let resolved = self.resolve(def);
                        if self.tensor_params.contains(&resolved) {
                            self.data_use.insert(resolved);
                        }
                    }
                }
            }
            HirStatementKind::Expr(expr) | HirStatementKind::Redde(Some(expr)) => {
                self.scan_expr(expr)
            }
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.scan_expr(&clause.cond);
                    self.scan_block(&clause.body);
                }
            }
            _ => {}
        }
    }

    fn scan_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::MethodCall(recv, method, _, args) => {
                let name = self.interner.resolve(*method);
                self.scan_expr(recv);
                for arg in args {
                    self.scan_expr(&arg.expr);
                }
                if is_fill_method(name) {
                    self.in_region_mask = true;
                }
                if name == "multiplica" {
                    if let Some(arg) = args.first() {
                        if self.is_fill_built(recv)
                            || self.is_fill_built(&arg.expr)
                            || is_comparison(&arg.expr)
                            || is_comparison(recv)
                        {
                            self.in_region_mask = true;
                        }
                    } else if self.is_fill_built(recv) || is_comparison(recv) {
                        self.in_region_mask = true;
                    }
                } else if name == "divisio" {
                    self.note_data(recv);
                    let denom_ok = args
                        .first()
                        .is_some_and(|arg| is_nonzero_literal(&arg.expr));
                    if !denom_ok {
                        self.nan_unsafe = true;
                    }
                } else if is_reduction_method(name) {
                    self.reductions.push(recv.as_ref().clone());
                    self.note_data(recv);
                } else {
                    self.note_data(recv);
                    for arg in args {
                        self.note_data(&arg.expr);
                    }
                }
            }
            HirExpressionKind::Binary(op, lhs, rhs) => {
                self.scan_expr(lhs);
                self.scan_expr(rhs);
                if matches!(*op, HirBinOp::Mul | HirBinOp::Hadamard) {
                    if self.is_fill_built(lhs)
                        || self.is_fill_built(rhs)
                        || is_comparison(lhs)
                        || is_comparison(rhs)
                    {
                        self.in_region_mask = true;
                    }
                } else if *op == HirBinOp::Div {
                    if !is_nonzero_literal(rhs) {
                        self.nan_unsafe = true;
                    }
                    self.note_data(lhs);
                    self.note_data(rhs);
                } else if !is_cmp(*op) {
                    self.note_data(lhs);
                    self.note_data(rhs);
                }
            }
            HirExpressionKind::Assign(lhs, rhs) => {
                self.scan_expr(rhs);
                self.note_data(lhs);
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                else_block,
                ..
            } => {
                self.scan_expr(cond);
                self.scan_block(then_block);
                if let Some(else_block) = else_block {
                    self.scan_block(else_block);
                }
            }
            HirExpressionKind::Block(block) => self.scan_block(block),
            HirExpressionKind::Loop(block) | HirExpressionKind::Itera(_, _, _, _, block) => {
                self.scan_block(block)
            }
            HirExpressionKind::Dum(cond, block) => {
                self.scan_expr(cond);
                self.scan_block(block);
            }
            HirExpressionKind::Call(callee, _, args) => {
                self.scan_expr(callee);
                for arg in args {
                    self.scan_expr(&arg.expr);
                    self.note_data(&arg.expr);
                }
            }
            HirExpressionKind::Unary(_, operand)
            | HirExpressionKind::Field(operand, _)
            | HirExpressionKind::Index(operand, _)
            | HirExpressionKind::Praefixum(operand) => self.scan_expr(operand),
            _ => {}
        }
    }
}

fn is_masked_exit(
    expr: &HirExpression,
    mask_defs: &FxHashSet<DefId>,
    local_inits: &FxHashMap<DefId, HirExpression>,
    interner: &Interner,
) -> bool {
    match &expr.kind {
        HirExpressionKind::Path(def) => local_inits
            .get(def)
            .is_some_and(|init| is_masked_exit(init, mask_defs, local_inits, interner)),
        HirExpressionKind::MethodCall(recv, method, _, args)
            if interner.resolve(*method) == "multiplica" =>
        {
            is_mask_ref(recv, mask_defs, local_inits)
                || args
                    .first()
                    .is_some_and(|arg| is_mask_ref(&arg.expr, mask_defs, local_inits))
        }
        HirExpressionKind::Binary(op, lhs, rhs)
            if matches!(*op, HirBinOp::Mul | HirBinOp::Hadamard) =>
        {
            is_mask_ref(lhs, mask_defs, local_inits) || is_mask_ref(rhs, mask_defs, local_inits)
        }
        _ => false,
    }
}

fn is_mask_ref(
    expr: &HirExpression,
    mask_defs: &FxHashSet<DefId>,
    local_inits: &FxHashMap<DefId, HirExpression>,
) -> bool {
    match &expr.kind {
        HirExpressionKind::Path(def) => {
            mask_defs.contains(def)
                || local_inits
                    .get(def)
                    .is_some_and(|init| is_mask_ref(init, mask_defs, local_inits))
        }
        _ => false,
    }
}

fn path_def(expr: &HirExpression) -> Option<DefId> {
    match &expr.kind {
        HirExpressionKind::Path(def) => Some(*def),
        _ => None,
    }
}

fn is_fill_method(name: &str) -> bool {
    matches!(name, "reple" | "fill")
}

fn is_reduction_method(name: &str) -> bool {
    matches!(name, "media" | "summa" | "crux_entropia" | "summa_axis")
}

fn is_cmp(op: HirBinOp) -> bool {
    matches!(
        op,
        HirBinOp::Lt
            | HirBinOp::Gt
            | HirBinOp::LtEq
            | HirBinOp::GtEq
            | HirBinOp::Eq
            | HirBinOp::NotEq
    )
}

fn is_comparison(expr: &HirExpression) -> bool {
    matches!(&expr.kind, HirExpressionKind::Binary(op, _, _) if is_cmp(*op))
}

fn is_nonzero_literal(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Float(value)) => *value != 0.0,
        HirExpressionKind::Literal(HirLiteral::Int(value)) => *value != 0,
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => is_nonzero_literal(inner),
        _ => false,
    }
}

fn has_runtime_len_guard(body: &HirBlock, index_def: DefId) -> bool {
    let mut found = RuntimeLenGuard {
        index_def,
        hit: false,
    };
    found.scan_block(body);
    found.hit
}

struct RuntimeLenGuard {
    index_def: DefId,
    hit: bool,
}

impl RuntimeLenGuard {
    fn scan_block(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            if let HirStatementKind::Expr(expr) | HirStatementKind::Redde(Some(expr)) = &stmt.kind {
                self.scan_expr(expr);
            }
        }
        if let Some(expr) = &block.expr {
            self.scan_expr(expr);
        }
    }

    fn scan_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Si {
                cond,
                then_block,
                else_block,
                ..
            } => {
                if is_runtime_len_guard(cond, self.index_def) {
                    self.hit = true;
                }
                self.scan_expr(cond);
                self.scan_block(then_block);
                if let Some(else_block) = else_block {
                    self.scan_block(else_block);
                }
            }
            HirExpressionKind::Block(block) => self.scan_block(block),
            HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
                self.scan_expr(lhs);
                self.scan_expr(rhs);
            }
            HirExpressionKind::MethodCall(recv, _, _, args) => {
                self.scan_expr(recv);
                for arg in args {
                    self.scan_expr(&arg.expr);
                }
            }
            HirExpressionKind::Unary(_, operand) | HirExpressionKind::Field(operand, _) => {
                self.scan_expr(operand)
            }
            _ => {}
        }
    }
}

fn is_runtime_len_guard(cond: &HirExpression, index_def: DefId) -> bool {
    match &cond.kind {
        HirExpressionKind::Binary(op, lhs, rhs) if is_cmp(*op) => {
            foldable_after_index(lhs, index_def) != foldable_after_index(rhs, index_def)
        }
        _ => false,
    }
}

fn foldable_after_index(expr: &HirExpression, index_def: DefId) -> bool {
    match &expr.kind {
        HirExpressionKind::Literal(_) => true,
        HirExpressionKind::Path(def) => *def == index_def,
        HirExpressionKind::Unary(_, inner) => foldable_after_index(inner, index_def),
        HirExpressionKind::Binary(_, lhs, rhs) => {
            foldable_after_index(lhs, index_def) && foldable_after_index(rhs, index_def)
        }
        _ => false,
    }
}

fn is_allowed_tensor_intrinsic(method: &str) -> bool {
    matches!(
        method,
        "addita"
            | "addita_bias"
            | "subtrahe"
            | "multiplica"
            | "divisio"
            | "nega"
            | "summa"
            | "media"
            | "matmul"
            | "transpone"
            | "forma"
            | "exponentia"
            | "logarithmus"
            | "activatio_relu"
            | "radix"
            | "gelu"
            | "activatio_tanh"
            | "activatio_softmax"
            | "laminatio"
            | "summa_axis"
            | "crux_entropia"
            | "rms_norm"
            | "rope_norm"
            | "gather"
    )
}

struct IdGen {
    next_hir: u32,
    next_def: u32,
}

impl IdGen {
    fn from_module(hir: &HirModule) -> Self {
        let mut census = IdCensus {
            max_hir: 0,
            max_def: 0,
        };
        walk_program(&mut census, hir);
        Self {
            next_hir: census.max_hir.saturating_add(1),
            next_def: census.max_def.saturating_add(1).max(1_000_000),
        }
    }

    fn next_hir(&mut self) -> HirId {
        let id = self.next_hir;
        self.next_hir = self.next_hir.saturating_add(1);
        HirId(id)
    }

    fn next_def(&mut self) -> DefId {
        let id = self.next_def;
        self.next_def = self.next_def.saturating_add(1);
        DefId(id)
    }
}

struct IdCensus {
    max_hir: u32,
    max_def: u32,
}

impl HirVisitor for IdCensus {
    fn visit_item(&mut self, item: &radix_hir::HirItem) {
        self.max_hir = self.max_hir.max(item.id.0);
        walk_item(self, item);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        self.max_hir = self.max_hir.max(stmt.id.0);
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        self.max_hir = self.max_hir.max(expr.id.0);
        walk_expr(self, expr);
    }

    fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
        if def_id.0 < 0x8000_0000 {
            self.max_def = self.max_def.max(def_id.0);
        }
    }

    fn visit_block(&mut self, block: &HirBlock) {
        walk_block(self, block);
    }
}
