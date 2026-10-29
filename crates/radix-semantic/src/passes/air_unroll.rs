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
//! - Route M: `itera ex … apud [i, j, …]` over statically shaped tensors —
//!   one statement is one unroll unit, lanes are the product of dims in
//!   first-bound=outer order, guard-free, each coordinate bound per lane as
//!   a `numerus` constant;
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

use super::air_tensor_intrinsics::is_allowed_tensor_intrinsic;
use crate::tensor_type_directed::is_tensor_type;
use crate::{
    IndexExpr, RadixLane, RadixLaneMetadata, SemanticError, SemanticErrorKind, Type, TypeId,
    TypeTable,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{
    HirVisitor, HirVisitorMut, walk_block, walk_expr, walk_expr_mut, walk_item, walk_program,
    walk_stmt, walk_stmt_mut,
};
use radix_hir::{
    DefId, HirArrayElement, HirAuthoredTypeUse, HirBinOp, HirBlock, HirConversioTarget,
    HirExpression, HirExpressionKind, HirFunction, HirId, HirItemKind, HirIteraMode, HirLiteral,
    HirLocal, HirModule, HirOwnerRef, HirParam, HirPresentation, HirRangeKind, HirStatement,
    HirStatementKind, HirTypeSite, HirTypeSlot, HirUnOp,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_types::HIR_SYNTHETIC_DEF_ID_BASE;

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
    presentation: &mut HirPresentation,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    let mut ids = IdGen::from_module(hir);
    let mut errors = Vec::new();
    let before = LiveNodes::of(hir);

    for item in &mut hir.items {
        if lanes.lane_for(item.def_id) != Some(RadixLane::Air) {
            continue;
        }
        let HirItemKind::Function(function) = &mut item.kind else {
            continue;
        };
        // Generic templates are retained for source/FHIR and are not an
        // executable AIR body. Reachable concrete clones have already been
        // appended by `air_specialize` and carry no type parameters.
        if !function.type_params.is_empty() {
            continue;
        }
        unroll_function(
            function,
            presentation,
            lanes,
            interner,
            types,
            &mut ids,
            &mut errors,
        );
    }

    // An unrolled loop's statements, its binder and its body block leave the
    // HIR; the presentation records they owned would dangle on the finalized
    // unit (and fail FHIR validation at load).
    let after = LiveNodes::of(hir);
    forget_removed_nodes(presentation, &before.removed_by(&after));

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The ids a module's HIR holds: node ids, definitions and block-end anchors.
#[derive(Default)]
struct LiveNodes {
    hirs: FxHashSet<HirId>,
    defs: FxHashSet<DefId>,
    block_ends: FxHashSet<radix_hir::HirSourceAnchorId>,
}

impl LiveNodes {
    fn of(hir: &HirModule) -> Self {
        let mut nodes = Self::default();
        walk_program(&mut nodes, hir);
        nodes
    }

    /// The ids `self` holds that `after` no longer does.
    fn removed_by(&self, after: &Self) -> Self {
        Self {
            hirs: self.hirs.difference(&after.hirs).copied().collect(),
            defs: self.defs.difference(&after.defs).copied().collect(),
            block_ends: self
                .block_ends
                .difference(&after.block_ends)
                .cloned()
                .collect(),
        }
    }
}

impl HirVisitor for LiveNodes {
    fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
        self.defs.insert(def_id);
    }

    fn visit_block(&mut self, block: &HirBlock) {
        if let Some(end) = &block.end {
            self.block_ends.insert(end.id.clone());
        }
        walk_block(self, block);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        self.hirs.insert(stmt.id);
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        self.hirs.insert(expr.id);
        walk_expr(self, expr);
    }
}

/// Drop every presentation record keyed by a node the pass removed.
///
/// A trivia owner whose statement was unrolled away goes with its attachment:
/// the lanes that replace the loop are synthesized, not authored.
fn forget_removed_nodes(presentation: &mut HirPresentation, removed: &LiveNodes) {
    let gone = |id: &HirId| removed.hirs.contains(id);
    presentation.guard_si_ids.retain(|id| !gone(id));
    presentation.elige_expr_ids.retain(|id| !gone(id));
    presentation.glyph_ids.retain(|id| !gone(id));
    presentation.radical_quarta_ids.retain(|id| !gone(id));
    presentation.numeric_twin_ids.retain(|id| !gone(id));
    presentation.empty_dense_ids.retain(|id| !gone(id));
    presentation.inferred_constructor_ids.retain(|id| !gone(id));
    presentation.axis_sugar.retain(|record| !gone(&record.id));
    presentation
        .discard_binding_stmts
        .retain(|record| !gone(&record.expr_id));
    presentation
        .tuple_label_accesses
        .retain(|record| !gone(&record.expr_id));
    presentation
        .insere_exprs
        .retain(|record| !gone(&record.expr));
    presentation
        .template_holes
        .retain(|record| !gone(&record.expr));
    presentation
        .destructure_bindings
        .retain(|record| !record.stmt_ids.iter().any(gone));
    presentation
        .await_assignment_bindings
        .retain(|def| !removed.defs.contains(def));
    presentation
        .function_modifier_bindings
        .retain(|record| !removed.defs.contains(&record.local));
    presentation
        .compile_time_locals
        .retain(|def| !removed.defs.contains(def));
    presentation.owners.retain(|owner| match &owner.semantic {
        Some(HirOwnerRef::Hir(id)) => !gone(id),
        Some(HirOwnerRef::Def(id)) => !removed.defs.contains(id),
        None => {
            owner.kind != radix_hir::HirTriviaOwnerKind::BlockEnd
                || !removed.block_ends.contains(&owner.id)
        }
    });
    let live_anchors: FxHashSet<_> = presentation
        .owners
        .iter()
        .map(|owner| owner.id.clone())
        .collect();
    presentation
        .attachments
        .retain(|attachment| live_anchors.contains(&attachment.anchor));
    presentation
        .block_ends
        .retain(|end| !removed.block_ends.contains(&end.id));
    presentation
        .authored_types
        .retain(|record| match &record.site.owner {
            HirOwnerRef::Hir(id) => !gone(id),
            HirOwnerRef::Def(id) => !removed.defs.contains(id),
        });
}

fn unroll_function(
    function: &mut HirFunction,
    presentation: &mut HirPresentation,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    types: &TypeTable,
    ids: &mut IdGen,
    errors: &mut Vec<SemanticError>,
) {
    let Some(body) = &mut function.body else {
        return;
    };
    let root_locals = block_local_defs(body);
    let mask = MaskAnalysis::analyze(&function.params, body, types, interner);
    let fullness = fullness_facts(body, interner, types);
    let mut emitted_lanes = 0;
    rewrite_block(
        presentation,
        body,
        &root_locals,
        lanes,
        interner,
        ids,
        errors,
        &mask,
        types,
        &fullness,
        &mut emitted_lanes,
        1,
    );
}

fn rewrite_block(
    presentation: &mut HirPresentation,
    block: &mut HirBlock,
    root_locals: &FxHashSet<DefId>,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    ids: &mut IdGen,
    errors: &mut Vec<SemanticError>,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
    multiplicity: usize,
) {
    let mut index = 0;
    while index < block.statements.len() {
        // Classify `itera` before walking into it so a nested range loop is
        // still visible to the outer decline check. The counter snapshot
        // backs the whole-nest rollback when a nested unit fails.
        let lanes_before = *emitted_lanes;
        let plan = classify_itera_stmt(
            &block.statements[index],
            block,
            root_locals,
            lanes,
            interner,
            mask,
            types,
            fullness,
            emitted_lanes,
            multiplicity,
        );
        let Some(plan) = plan else {
            rewrite_nested_stmt(
                presentation,
                &mut block.statements[index],
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
            index += 1;
            continue;
        };
        match plan {
            UnrollPlan::Decline { span, causa } => {
                errors.push(decline(span, causa));
                index += 1;
            }
            UnrollPlan::Ready(mut ready) => {
                // Nested units in the body charge multiplicatively (each
                // body copy is emitted once per lane). A nested failure
                // rolls the whole nest back to a decline; the source stays
                // intact (3E/3F).
                let body_multiplicity = ready.lanes.len().saturating_mul(multiplicity);
                let mut nested_errors = Vec::new();
                let mut nested_presentation = presentation.clone();
                rewrite_block(
                    &mut nested_presentation,
                    &mut ready.body,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    &mut nested_errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    body_multiplicity,
                );
                if let Some(causa) = first_decline_causa(&nested_errors) {
                    *emitted_lanes = lanes_before;
                    errors.push(decline(ready.span, causa));
                    index += 1;
                    continue;
                }
                *presentation = nested_presentation;
                let (unrolled, acc_final) =
                    emit_unrolled_lanes(presentation, &ready, interner, ids);
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
            presentation,
            &mut expr,
            root_locals,
            lanes,
            interner,
            ids,
            errors,
            mask,
            types,
            fullness,
            emitted_lanes,
            multiplicity,
        );
        let lanes_before = *emitted_lanes;
        match classify_itera_expr(
            &expr,
            block,
            root_locals,
            lanes,
            interner,
            mask,
            types,
            fullness,
            emitted_lanes,
            multiplicity,
        ) {
            Some(UnrollPlan::Decline { span, causa }) => {
                errors.push(decline(span, causa));
                block.expr = Some(expr);
            }
            Some(UnrollPlan::Ready(mut ready)) => {
                let span = expr.span;
                let ty = expr.ty;
                let body_multiplicity = ready.lanes.len().saturating_mul(multiplicity);
                let mut nested_errors = Vec::new();
                let mut nested_presentation = presentation.clone();
                rewrite_block(
                    &mut nested_presentation,
                    &mut ready.body,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    &mut nested_errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    body_multiplicity,
                );
                if let Some(causa) = first_decline_causa(&nested_errors) {
                    *emitted_lanes = lanes_before;
                    block.expr = Some(expr);
                    errors.push(decline(ready.span, causa));
                } else {
                    *presentation = nested_presentation;
                    let (unrolled, acc_final) =
                        emit_unrolled_lanes(presentation, &ready, interner, ids);
                    // WHY: the loop is the tail, so every statement precedes it
                    // and keeps reading the accumulator's own definition; the
                    // lane-final defs exist only inside the unrolled block.
                    freeze_accumulators(block, &acc_final);
                    block.expr = Some(Box::new(HirExpression {
                        id: ids.next_hir(),
                        kind: HirExpressionKind::Block(HirBlock::simple(unrolled, None, span)),
                        ty,
                        span,
                    }));
                }
            }
            None => block.expr = Some(expr),
        }
    }
}

fn first_decline_causa(errors: &[SemanticError]) -> Option<&str> {
    errors.iter().find_map(|error| {
        error
            .args
            .iter()
            .find(|arg| arg.name == "causa")
            .map(|arg| arg.value.as_str())
    })
}

fn rewrite_nested_stmt(
    presentation: &mut HirPresentation,
    stmt: &mut HirStatement,
    root_locals: &FxHashSet<DefId>,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    ids: &mut IdGen,
    errors: &mut Vec<SemanticError>,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
    multiplicity: usize,
) {
    match &mut stmt.kind {
        HirStatementKind::Local(local) => {
            if let Some(init) = &mut local.init {
                rewrite_nested_expr(
                    presentation,
                    init,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
                );
            }
        }
        HirStatementKind::Expr(expr)
        | HirStatementKind::Redde(Some(expr))
        | HirStatementKind::ReddeFilum(Some(expr)) => {
            rewrite_nested_expr(
                presentation,
                expr,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
        }
        HirStatementKind::Custodi(custodi) => {
            for clause in &mut custodi.clauses {
                rewrite_nested_expr(
                    presentation,
                    &mut clause.cond,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
                );
                rewrite_block(
                    presentation,
                    &mut clause.body,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
                );
            }
        }
        HirStatementKind::Redde(None)
        | HirStatementKind::ReddeFilum(None)
        | HirStatementKind::Rumpe
        | HirStatementKind::Perge
        | HirStatementKind::Tacet
        | HirStatementKind::IncDec(_) => {}
    }
}

fn rewrite_nested_expr(
    presentation: &mut HirPresentation,
    expr: &mut HirExpression,
    root_locals: &FxHashSet<DefId>,
    lanes: &RadixLaneMetadata,
    interner: &mut Interner,
    ids: &mut IdGen,
    errors: &mut Vec<SemanticError>,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
    multiplicity: usize,
) {
    match &mut expr.kind {
        HirExpressionKind::Block(block) => rewrite_block(
            presentation,
            block,
            root_locals,
            lanes,
            interner,
            ids,
            errors,
            mask,
            types,
            fullness,
            emitted_lanes,
            multiplicity,
        ),
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            rewrite_nested_expr(
                presentation,
                cond,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
            rewrite_block(
                presentation,
                then_block,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
            if let Some(catch) = then_catch {
                rewrite_block(
                    presentation,
                    &mut catch.body,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
                );
            }
            if let Some(else_block) = else_block {
                rewrite_block(
                    presentation,
                    else_block,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
                );
            }
        }
        HirExpressionKind::Discerne {
            scrutinees, arms, ..
        } => {
            for scrutinee in scrutinees {
                rewrite_nested_expr(
                    presentation,
                    scrutinee,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
                );
            }
            for arm in arms {
                rewrite_nested_expr(
                    presentation,
                    &mut arm.body,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
                );
            }
        }
        HirExpressionKind::Loop(block) | HirExpressionKind::Itera { body: block, .. } => {
            rewrite_block(
                presentation,
                block,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
        }
        HirExpressionKind::Dum(cond, block) => {
            rewrite_nested_expr(
                presentation,
                cond,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
            rewrite_block(
                presentation,
                block,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
        }
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            rewrite_nested_expr(
                presentation,
                lhs,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
            rewrite_nested_expr(
                presentation,
                rhs,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
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
                presentation,
                operand,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
        }
        HirExpressionKind::Index(object, index) => {
            rewrite_nested_expr(
                presentation,
                object,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
            rewrite_nested_expr(
                presentation,
                index,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
        }
        HirExpressionKind::Call(callee, _, args, _) => {
            rewrite_nested_expr(
                presentation,
                callee,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
            for arg in args {
                rewrite_nested_expr(
                    presentation,
                    &mut arg.expr,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
                );
            }
        }
        HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
            rewrite_nested_expr(
                presentation,
                receiver,
                root_locals,
                lanes,
                interner,
                ids,
                errors,
                mask,
                types,
                fullness,
                emitted_lanes,
                multiplicity,
            );
            for arg in args {
                rewrite_nested_expr(
                    presentation,
                    &mut arg.expr,
                    root_locals,
                    lanes,
                    interner,
                    ids,
                    errors,
                    mask,
                    types,
                    fullness,
                    emitted_lanes,
                    multiplicity,
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
fn fullness_facts(
    block: &HirBlock,
    interner: &Interner,
    types: &TypeTable,
) -> FxHashMap<DefId, usize> {
    let mut facts = FxHashMap::default();
    let mut collector = Collector {
        facts: &mut facts,
        interner,
        types,
    };
    collector.visit_block(block);
    facts
}

struct Collector<'a> {
    facts: &'a mut FxHashMap<DefId, usize>,
    interner: &'a Interner,
    types: &'a TypeTable,
}

impl HirVisitor for Collector<'_> {
    fn visit_local(&mut self, local: &HirLocal) {
        if local.mutable {
            self.facts.remove(&local.def_id);
        } else if let Some(init) = &local.init
            && let Some(length) = static_length(init, self.interner, self.facts, self.types)
        {
            self.facts.insert(local.def_id, length);
        }
        radix_hir::visit::walk_local(self, local);
    }
}

fn static_length(
    expr: &HirExpression,
    interner: &Interner,
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
        // AEU-7: whole-literal scalar buffers prove their own fullness —
        // `"abcdefgh"` is 8 UTF-8 bytes, `'alpha'` 5, `|61 62|` 2 decoded
        // bytes. The conversio capacity check above then requires len == N.
        HirExpressionKind::Literal(HirLiteral::String(sym) | HirLiteral::Ascii(sym)) => {
            Some(interner.resolve(*sym).len())
        }
        HirExpressionKind::Literal(HirLiteral::Octeti(sym)) => {
            radix_lexer::octeti_literal::decode_octeti_hex(interner.resolve(*sym))
                .ok()
                .map(|bytes| bytes.len())
        }
        HirExpressionKind::Path(def_id) => facts.get(def_id).copied(),
        HirExpressionKind::Conversio {
            source,
            target: HirConversioTarget::Type(target),
            ..
        } => {
            let length = static_length(source, interner, facts, types)?;
            if let Some(capacity) = bounded_capacity(*target, types) {
                (capacity == length).then_some(length)
            } else {
                Some(length)
            }
        }
        HirExpressionKind::Verte { source, .. } => static_length(source, interner, facts, types),
        _ => None,
    }
}

fn bounded_capacity(ty: TypeId, types: &TypeTable) -> Option<usize> {
    let (_, ty) = types.peel_access_marker(ty);
    let capacity = match types.get(ty) {
        Type::BoundedArray { capacity, .. }
        | Type::BoundedTextus { capacity }
        | Type::BoundedAscii { capacity }
        | Type::BoundedOcteti { capacity } => *capacity,
        _ => return None,
    };
    let IndexExpr::Literal(value) = types.get_index(capacity) else {
        return None;
    };
    usize::try_from(*value).ok()
}

/// AEU-7: the scalar buffer element row — `octeti<N>` lanes bind
/// `octetus` (`numerus<u8>`), `ascii<N>` / `textus<N>` lanes bind `littera`
/// (D10.1: text iteration yields one Unicode scalar). The typecheck `itera`
/// arm interns the byte row for the very loop being classified, so the
/// shared table already holds it when this AIR pass runs; a miss fails closed
/// into the caller's `not_concrete` decline.
fn scalar_bounded_element(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    match types.get(ty) {
        Type::BoundedOcteti { .. } => types.find_sized_numeric(
            radix_types::Primitive::Numerus,
            radix_types::NumericWidth::U8,
        ),
        Type::BoundedAscii { .. } | Type::BoundedTextus { .. } => {
            Some(types.primitive(radix_types::Primitive::Littera))
        }
        _ => None,
    }
}

enum UnrollPlan {
    Ready(Box<ReadyUnroll>),
    Decline { span: Span, causa: &'static str },
}

struct ReadyUnroll {
    /// Span of the loop statement; used when a nested unit failure rolls
    /// the whole nest back to a decline.
    span: Span,
    index_def: DefId,
    lanes: Vec<i64>,
    body: HirBlock,
    accumulators: FxHashMap<DefId, AccInfo>,
    tensor_source: Option<HirExpression>,
    tensor_binding_ty: Option<TypeId>,
    /// Route M: `apud` coordinate defs bound per lane as `numerus` constants.
    coordinate_defs: Vec<DefId>,
    /// Route M: per-lane coordinate tuples, first bound varying slowest.
    /// Empty lanes tuples for the coordinate-free routes.
    lane_coordinates: Vec<Vec<i64>>,
}

#[derive(Clone, Copy)]
struct AccInfo {
    ty: Option<crate::TypeId>,
}

fn classify_itera_stmt(
    stmt: &HirStatement,
    block: &HirBlock,
    root_locals: &FxHashSet<DefId>,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
    multiplicity: usize,
) -> Option<UnrollPlan> {
    let HirStatementKind::Expr(expr) = &stmt.kind else {
        return None;
    };
    classify_itera_expr(
        expr,
        block,
        root_locals,
        lanes,
        interner,
        mask,
        types,
        fullness,
        emitted_lanes,
        multiplicity,
    )
}

fn classify_itera_expr(
    expr: &HirExpression,
    block: &HirBlock,
    root_locals: &FxHashSet<DefId>,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
    multiplicity: usize,
) -> Option<UnrollPlan> {
    // Only the single-source `Itera` admits an unroll plan. The Cartesian
    // `IteraRange` product (like `IteraLockstep`) has no AIR recipe: it
    // returns `None` here and stays fail-closed — the product never
    // unrolls, whatever its body shape.
    let HirExpressionKind::Itera {
        mode,
        coordinates,
        binding: index_def,
        iter: source,
        body,
        ..
    } = &expr.kind
    else {
        return None;
    };
    match mode {
        HirIteraMode::Ab => Some(classify_ab_loop(
            expr.span,
            *index_def,
            source,
            body,
            block,
            root_locals,
            lanes,
            interner,
            mask,
            emitted_lanes,
            multiplicity,
        )),
        HirIteraMode::Ex => Some(classify_ex_loop(
            expr.span,
            *index_def,
            coordinates,
            source,
            body,
            block,
            root_locals,
            lanes,
            interner,
            mask,
            types,
            fullness,
            emitted_lanes,
            multiplicity,
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
    root_locals: &FxHashSet<DefId>,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    emitted_lanes: &mut usize,
    multiplicity: usize,
) -> UnrollPlan {
    let Some(values) = concrete_range_values(source) else {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_NOT_CONCRETE,
        };
    };
    let lane_count = values.len();
    let charge = lane_count.saturating_mul(multiplicity);
    if charge > UNROLL_BUDGET.saturating_sub(*emitted_lanes) {
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
    let block_locals = block_local_defs(block);
    let decline_locals = if multiplicity > 1 {
        root_locals
    } else {
        &block_locals
    };
    if let Some(causa) = body_decline(body, decline_locals, index_def, lanes, interner) {
        return UnrollPlan::Decline { span, causa };
    }
    let accumulators = if multiplicity > 1 {
        FxHashMap::default()
    } else {
        block_accumulators(block, body)
    };
    *emitted_lanes += lane_count.saturating_mul(multiplicity);
    UnrollPlan::Ready(Box::new(ReadyUnroll {
        span,
        index_def,
        lanes: values,
        body: body.clone(),
        accumulators,
        tensor_source: None,
        tensor_binding_ty: None,
        coordinate_defs: Vec::new(),
        lane_coordinates: Vec::new(),
    }))
}

fn classify_ex_loop(
    span: Span,
    index_def: DefId,
    coordinates: &[(DefId, Symbol)],
    source: &HirExpression,
    body: &HirBlock,
    block: &HirBlock,
    root_locals: &FxHashSet<DefId>,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    types: &TypeTable,
    fullness: &FxHashMap<DefId, usize>,
    emitted_lanes: &mut usize,
    multiplicity: usize,
) -> UnrollPlan {
    let Some(source_ty) = source.ty else {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_NOT_CONCRETE,
        };
    };
    let (_, source_ty) = types.peel_access_marker(source_ty);
    // Route A bounded element sources: `lista<T, magnitudo N>` (AEU-3) and the scalar
    // buffer forms `octeti<N>` / `ascii<N>` / `textus<N>` (AEU-7).
    let bounded_element = match types.get(source_ty) {
        Type::BoundedArray { element, .. } => Some(*element),
        Type::BoundedOcteti { .. } | Type::BoundedAscii { .. } | Type::BoundedTextus { .. } => {
            scalar_bounded_element(source_ty, types)
        }
        _ => None,
    };
    if let Some(element) = bounded_element {
        let Some(lane_count) = bounded_capacity(source_ty, types) else {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_NOT_CONCRETE,
            };
        };
        if static_length(source, interner, fullness, types) != Some(lane_count) {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_NOT_CONCRETE,
            };
        }
        let charge = lane_count.saturating_mul(multiplicity);
        if charge > UNROLL_BUDGET.saturating_sub(*emitted_lanes) {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_BUDGET,
            };
        }
        if let Some(causa) = mask.decline() {
            return UnrollPlan::Decline { span, causa };
        }
        let block_locals = block_local_defs(block);
        let decline_locals = if multiplicity > 1 {
            root_locals
        } else {
            &block_locals
        };
        if let Some(causa) = body_decline(body, decline_locals, index_def, lanes, interner) {
            return UnrollPlan::Decline { span, causa };
        }

        let accumulators = if multiplicity > 1 {
            FxHashMap::default()
        } else {
            block_accumulators(block, body)
        };
        *emitted_lanes += charge;
        #[allow(clippy::cast_possible_wrap)]
        let values = (0..lane_count).map(|lane| lane as i64).collect();
        return UnrollPlan::Ready(Box::new(ReadyUnroll {
            span,
            index_def,
            lanes: values,
            body: body.clone(),
            accumulators,
            tensor_source: Some(source.clone()),
            tensor_binding_ty: Some(element),
            coordinate_defs: Vec::new(),
            lane_coordinates: Vec::new(),
        }));
    }
    let Type::Tensor(element_ty, shape) = types.get(source_ty) else {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_NOT_CONCRETE,
        };
    };

    // Route M (AC-7): `apud` coordinates over a statically shaped tensor
    // unroll as the product of dims with per-lane coordinate constants.
    // Coordinate-free iteration still admits only rank-1 (Route T).
    if coordinates.is_empty() && !matches!(types.get_index(*shape), IndexExpr::Literal(_)) {
        let IndexExpr::Tuple(dims) = types.get_index(*shape) else {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_NOT_CONCRETE,
            };
        };
        if dims.len() > 1 {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_DEFER_COORDINATES,
            };
        }
    }

    let concrete_dims = |types: &TypeTable| -> Option<Vec<usize>> {
        let dims: Vec<crate::IndexId> = match types.get_index(*shape) {
            IndexExpr::Literal(_) => vec![*shape],
            IndexExpr::Tuple(dims) => dims.clone(),
            _ => return None,
        };
        dims.iter()
            .map(|dim| match types.get_index(*dim) {
                IndexExpr::Literal(value) => usize::try_from(*value).ok(),
                _ => None,
            })
            .collect()
    };

    let dims = if coordinates.is_empty() {
        // Route T: a rank-1 literal shape, exactly as before Route M.
        let IndexExpr::Tuple(dims) = types.get_index(*shape) else {
            let IndexExpr::Literal(value) = types.get_index(*shape) else {
                return UnrollPlan::Decline {
                    span,
                    causa: CAUSA_NOT_CONCRETE,
                };
            };
            return match usize::try_from(*value) {
                Ok(count) => classify_ex_tensor_ready(
                    span,
                    index_def,
                    source,
                    body,
                    block,
                    root_locals,
                    lanes,
                    interner,
                    mask,
                    emitted_lanes,
                    multiplicity,
                    count,
                    Vec::new(),
                    Vec::new(),
                    *element_ty,
                ),
                Err(_) => UnrollPlan::Decline {
                    span,
                    causa: CAUSA_BUDGET,
                },
            };
        };
        if dims.len() != 1 || !matches!(types.get_index(dims[0]), IndexExpr::Literal(_)) {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_NOT_CONCRETE,
            };
        }
        match concrete_dims(types) {
            Some(dims) => dims,
            None => {
                return UnrollPlan::Decline {
                    span,
                    causa: CAUSA_BUDGET,
                };
            }
        }
    } else {
        // Route M: arity == rank is typechecked (AC-5); fail closed anyway.
        if coordinates.len() != rank_of(types.get_index(*shape)) {
            return UnrollPlan::Decline {
                span,
                causa: CAUSA_NOT_CONCRETE,
            };
        }
        match concrete_dims(types) {
            Some(dims) => dims,
            None => {
                return UnrollPlan::Decline {
                    span,
                    causa: CAUSA_NOT_CONCRETE,
                };
            }
        }
    };

    let Some(lane_count) = dims
        .iter()
        .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))
    else {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_BUDGET,
        };
    };

    if coordinates.is_empty() {
        return classify_ex_tensor_ready(
            span,
            index_def,
            source,
            body,
            block,
            root_locals,
            lanes,
            interner,
            mask,
            emitted_lanes,
            multiplicity,
            lane_count,
            Vec::new(),
            Vec::new(),
            *element_ty,
        );
    }

    // First-bound = outer axis: the first coordinate varies slowest across
    // lanes (row-major lane order).
    let coordinate_defs = coordinates.iter().map(|(def, _)| *def).collect();
    let mut lane_coordinates = Vec::with_capacity(lane_count);
    for lane in 0..lane_count {
        let mut rest = lane;
        let mut tuple = vec![0i64; dims.len()];
        for (slot, dim) in dims.iter().enumerate().rev() {
            tuple[slot] = i64::try_from(rest % dim).unwrap_or(i64::MAX);
            rest /= dim;
        }
        lane_coordinates.push(tuple);
    }
    classify_ex_tensor_ready(
        span,
        index_def,
        source,
        body,
        block,
        root_locals,
        lanes,
        interner,
        mask,
        emitted_lanes,
        multiplicity,
        lane_count,
        coordinate_defs,
        lane_coordinates,
        *element_ty,
    )
}

fn rank_of(shape: &IndexExpr) -> usize {
    match shape {
        IndexExpr::Literal(_) => 1,
        IndexExpr::Tuple(dims) => dims.len(),
        _ => 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_ex_tensor_ready(
    span: Span,
    index_def: DefId,
    source: &HirExpression,
    body: &HirBlock,
    block: &HirBlock,
    root_locals: &FxHashSet<DefId>,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
    mask: &MaskAnalysis,
    emitted_lanes: &mut usize,
    multiplicity: usize,
    lane_count: usize,
    coordinate_defs: Vec<DefId>,
    lane_coordinates: Vec<Vec<i64>>,
    element_ty: TypeId,
) -> UnrollPlan {
    let charge = lane_count.saturating_mul(multiplicity);
    if charge > UNROLL_BUDGET.saturating_sub(*emitted_lanes) {
        return UnrollPlan::Decline {
            span,
            causa: CAUSA_BUDGET,
        };
    }
    if let Some(causa) = mask.decline() {
        return UnrollPlan::Decline { span, causa };
    }
    let block_locals = block_local_defs(block);
    let decline_locals = if multiplicity > 1 {
        root_locals
    } else {
        &block_locals
    };
    if let Some(causa) = body_decline(body, decline_locals, index_def, lanes, interner) {
        return UnrollPlan::Decline { span, causa };
    }

    let accumulators = if multiplicity > 1 {
        FxHashMap::default()
    } else {
        block_accumulators(block, body)
    };
    *emitted_lanes += charge;
    #[allow(clippy::cast_possible_wrap)]
    let values = (0..lane_count).map(|lane| lane as i64).collect();
    UnrollPlan::Ready(Box::new(ReadyUnroll {
        span,
        index_def,
        lanes: values,
        body: body.clone(),
        accumulators,
        tensor_source: Some(source.clone()),
        tensor_binding_ty: Some(element_ty),
        coordinate_defs,
        lane_coordinates,
    }))
}

// ── First-reduction bounded-source admission ──────────────────────────

/// Whether a `reducta via prima` source has a statically known extent.
///
/// The first reduction admits on the AIR lane only over statically bounded
/// sources; the MIR dataflow lowering traverses the full extent with total
/// predicate evaluation, so admission needs the bound, not an unroll. The
/// source is a tensor or matrix, as for every reduction. Incomplete tensor
/// shapes fail closed, and a statically zero-extent shape is a bounded route.
pub(crate) fn first_source_is_bounded(source: &HirExpression, types: &TypeTable) -> bool {
    let Some(ty) = source.ty else {
        return false;
    };
    let (_, peeled) = types.peel_access_marker(ty);
    match types.get(peeled) {
        Type::Tensor(_, _) => {
            crate::tensor_type_directed::complete_tensor_type(ty, types).is_some()
        }
        Type::Matrix(_, shape) => {
            crate::i64s_from_literal_index(*shape, types).is_some_and(|dims| !dims.is_empty())
        }
        _ => false,
    }
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
    let stride = match step {
        Some(step) => as_i64(step)?,
        None => 1,
    };
    // The step is a positive stride; the bounds pick the direction.
    if stride <= 0 {
        return None;
    }
    let step = if start <= end { stride } else { -stride };
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
    same_block_locals: &FxHashSet<DefId>,
    index_def: DefId,
    lanes: &RadixLaneMetadata,
    interner: &Interner,
) -> Option<&'static str> {
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
            if let HirExpressionKind::Assign(lhs, _) = &expr.kind
                && let HirExpressionKind::Path(def) = lhs.kind
            {
                assigned.insert(def);
            }
            // A nested unroll unit splices straight-line lanes into this
            // block, so its body's assigns are accumulator updates here too.
            if let HirExpressionKind::Itera { body, .. } = &expr.kind {
                collect_straight_assigns(body, assigned);
            }
        }
    }
}

struct BodyDecline<'a> {
    index_def: DefId,
    same_block_locals: &'a FxHashSet<DefId>,
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
            | HirStatementKind::ReddeFilum(_)
            | HirStatementKind::Rumpe
            | HirStatementKind::Perge
            | HirStatementKind::Custodi(_) => self.reject(CAUSA_UNSUPPORTED_CONTROL),
            HirStatementKind::Tacet | HirStatementKind::IncDec(_) => self.reject(CAUSA_IMPURE),
            HirStatementKind::Expr(expr) => self.scan_expr(expr, true),
        }
    }

    fn scan_expr(&mut self, expr: &HirExpression, as_statement: bool) {
        match &expr.kind {
            HirExpressionKind::Assign(lhs, rhs) => self.scan_assign(lhs, rhs),
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
            // A nested itera directly at the loop body root is its own
            // unroll unit: the body rewrite validates it recursively
            // (multiplicative budget, fail-closed decline) instead of this
            // purity scan. Nested iteras under control flow stay rejected:
            // conditional lanes cannot chain a straight-line accumulator.
            // The series forms (`IteraRange`, `IteraLockstep`) have no unroll
            // recipe, so they never reach this root exemption and walk
            // through the generic arm below like any other nested
            // expression.
            HirExpressionKind::Itera { .. } if self.at_loop_body_root && as_statement => {}
            HirExpressionKind::Loop(_)
            | HirExpressionKind::Dum(_, _)
            | HirExpressionKind::Discerne { .. }
            | HirExpressionKind::Itera { .. } => self.reject(CAUSA_UNSUPPORTED_CONTROL),
            HirExpressionKind::Call(callee, _, args, _) => {
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
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
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
            | HirExpressionKind::Clausura(..)
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

    fn scan_assign(&mut self, lhs: &HirExpression, rhs: &HirExpression) {
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
    presentation: &mut HirPresentation,
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
    for (lane_index, value) in ready.lanes.iter().enumerate() {
        let coordinate_bindings =
            emit_coordinate_bindings(ready, lane_index, interner, ids, &mut out);
        let mut remap = Remap {
            index_def: ready.index_def,
            index_value: *value,
            binding_value: None,
            coordinate_bindings,
            acc_current: acc_current.clone(),
            local_map: FxHashMap::default(),
        };
        if let Some(source) = &ready.tensor_source {
            let mut source = source.clone();
            remap_fresh_expr(&mut source, &remap, ids);
            let binding_def = ids.next_def();
            let index_kind = if ready.coordinate_defs.is_empty() {
                int_literal(*value, ids)
            } else {
                HirExpressionKind::Array(
                    ready.lane_coordinates[lane_index]
                        .iter()
                        .map(|coord| {
                            HirArrayElement::Expr(HirExpression {
                                id: ids.next_hir(),
                                kind: int_literal(*coord, ids),
                                ty: None,
                                span: source.span,
                            })
                        })
                        .collect(),
                )
            };
            let index = HirExpression {
                id: ids.next_hir(),
                kind: index_kind,
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
                await_store: None,
                size_decl: false,
                size_static: false,
            };
            out.push(HirStatement {
                id: ids.next_hir(),
                kind: HirStatementKind::Local(binding),
                span: ready.body.span,
            });
            remap.binding_value = Some(binding_def);
        }
        let stmts = emit_iteration(
            presentation,
            &ready.body,
            &ready.accumulators,
            &mut remap,
            interner,
            ids,
        );
        acc_current = remap.acc_current;
        out.extend(stmts);
    }
    (out, acc_current)
}

fn emit_coordinate_bindings(
    ready: &ReadyUnroll,
    lane_index: usize,
    interner: &mut Interner,
    ids: &mut IdGen,
    out: &mut Vec<HirStatement>,
) -> FxHashMap<DefId, DefId> {
    let mut coordinate_bindings = FxHashMap::default();
    if !ready.coordinate_defs.is_empty() {
        let tuple = &ready.lane_coordinates[lane_index];
        for (def, coord) in ready.coordinate_defs.iter().zip(tuple) {
            let binding_def = ids.next_def();
            let binding = HirLocal {
                def_id: binding_def,
                name: interner.intern(&format!("__unroll_{}", binding_def.0)),
                ty: None,
                init: Some(HirExpression {
                    id: ids.next_hir(),
                    kind: int_literal(*coord, ids),
                    ty: None,
                    span: ready.body.span,
                }),
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            };
            out.push(HirStatement {
                id: ids.next_hir(),
                kind: HirStatementKind::Local(binding),
                span: ready.body.span,
            });
            coordinate_bindings.insert(*def, binding_def);
        }
    }
    coordinate_bindings
}

fn emit_iteration(
    presentation: &mut HirPresentation,
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
                if let HirExpressionKind::Assign(lhs, rhs) = &expr.kind
                    && let HirExpressionKind::Path(acc) = lhs.kind
                    && let Some(info) = accumulators.get(&acc)
                {
                    let mut rhs = rhs.as_ref().clone();
                    remap_fresh_expr(&mut rhs, remap, ids);
                    let new_def = ids.next_def();
                    out.push(ssa_local(new_def, *info, rhs, stmt.span, interner, ids));
                    remap.acc_current.insert(acc, new_def);
                    continue;
                }
                let mut stmt = stmt.clone();
                remap_fresh_stmt(&mut stmt, remap, ids);
                out.push(stmt);
            }
            HirStatementKind::Local(local) => {
                let new_def = ids.next_def();
                remap.local_map.insert(local.def_id, new_def);
                let authored_form = presentation
                    .authored_type_at(&HirTypeSite {
                        owner: HirOwnerRef::Def(local.def_id),
                        slot: HirTypeSlot::Declared,
                    })
                    .cloned();
                let mut local = local.clone();
                local.def_id = new_def;
                local.name = interner.intern(&format!("__unroll_{}", new_def.0));
                local.mutable = false;
                if let Some(init) = &mut local.init {
                    remap_fresh_expr(init, remap, ids);
                }
                if let Some(form) = authored_form {
                    // Narrow rewrite-carrier exception: presentation is read
                    // here only to carry an authored local form onto the fresh
                    // unrolled binding; it never participates in type or AIR
                    // decisions.
                    presentation.authored_types.push(HirAuthoredTypeUse {
                        site: HirTypeSite {
                            owner: HirOwnerRef::Def(new_def),
                            slot: HirTypeSlot::Declared,
                        },
                        form,
                    });
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
        if let HirExpressionKind::Assign(lhs, rhs) = &expr.kind
            && let HirExpressionKind::Path(acc) = lhs.kind
            && let Some(info) = accumulators.get(&acc)
        {
            let mut rhs = rhs.as_ref().clone();
            remap_fresh_expr(&mut rhs, remap, ids);
            let new_def = ids.next_def();
            out.push(ssa_local(new_def, *info, rhs, expr.span, interner, ids));
            remap.acc_current.insert(acc, new_def);
            return out;
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
            await_store: None,
            size_decl: false,
            size_static: false,
        }),
        span,
    }
}

struct Remap {
    index_def: DefId,
    index_value: i64,
    binding_value: Option<DefId>,
    coordinate_bindings: FxHashMap<DefId, DefId>,
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
            if let Some(binding) = self.remap.coordinate_bindings.get(&def) {
                expr.kind = HirExpressionKind::Path(*binding);
                return;
            }
            if def == self.remap.index_def {
                if let Some(binding) = self.remap.binding_value {
                    expr.kind = HirExpressionKind::Path(binding);
                } else {
                    expr.kind = int_literal(self.remap.index_value, self.ids);
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
        if let HirExpressionKind::Path(def) = &mut expr.kind
            && let Some(mapped) = self.acc_final.get(def)
        {
            *def = *mapped;
            return;
        }
        walk_expr_mut(self, expr);
    }
}

fn freeze_accumulators(block: &mut HirBlock, acc_final: &FxHashMap<DefId, DefId>) {
    for stmt in &mut block.statements {
        if let HirStatementKind::Local(local) = &mut stmt.kind
            && acc_final.contains_key(&local.def_id)
        {
            local.mutable = false;
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
        HirExpressionKind::Literal(HirLiteral::Int(value.unsigned_abs()))
    }
}

fn decline(span: Span, causa: &str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::AirPurityViolation, span)
        .with_arg("issue", "itera_loop")
        .with_arg("lane", "air")
        .with_arg("causa", causa)
}

/// Function-level facts for the guard-as-mask admit (unit-3F).
///
/// A tensor parameter that is never used as data (only as a `multiplicata`
/// operand, or unused beside a data tensor) is a mask operand. Mask mode
/// then fail-closes on in-region construction, dead-lane-unsafe `divisa`,
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
            HirExpressionKind::MethodCall(recv, method, _, _, _) => {
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
                    if let Some(def) = path_def(init) {
                        let resolved = self.resolve(def);
                        self.aliases.insert(local.def_id, resolved);
                        if self.fill_defs.contains(&def) || self.is_fill_built(init) {
                            self.fill_defs.insert(local.def_id);
                        }
                        if self.tensor_params.contains(&resolved) {
                            self.data_use.insert(resolved);
                        }
                    } else if self.is_fill_built(init) {
                        self.fill_defs.insert(local.def_id);
                    }
                }
            }
            HirStatementKind::Expr(expr) | HirStatementKind::Redde(Some(expr)) => {
                self.scan_expr(expr);
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
            HirExpressionKind::MethodCall(recv, method, _, args, _) => {
                let name = self.interner.resolve(*method);
                self.scan_expr(recv);
                for arg in args {
                    self.scan_expr(&arg.expr);
                }
                if is_fill_method(name) {
                    self.in_region_mask = true;
                }
                if name == "multiplicata" {
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
                } else if name == "divisa" {
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
            HirExpressionKind::Binary(op, lhs, rhs) => self.scan_binary(*op, lhs, rhs),
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
            HirExpressionKind::Loop(block) | HirExpressionKind::Itera { body: block, .. } => {
                self.scan_block(block);
            }
            HirExpressionKind::Dum(cond, block) => {
                self.scan_expr(cond);
                self.scan_block(block);
            }
            HirExpressionKind::Call(callee, _, args, _) => {
                self.scan_expr(callee);
                for arg in args {
                    self.scan_expr(&arg.expr);
                    self.note_data(&arg.expr);
                }
            }
            HirExpressionKind::Unary(_, operand)
            | HirExpressionKind::Field(operand, _)
            | HirExpressionKind::Praefixum(operand) => self.scan_expr(operand),
            HirExpressionKind::Index(operand, index) => {
                self.scan_expr(operand);
                self.scan_expr(index);
            }
            // WHY: `air_purity` descends through casts and conversions, so a
            // `divisa` or fill hidden under one still reaches the lane.
            HirExpressionKind::Verte { source, .. } => self.scan_expr(source),
            HirExpressionKind::Conversio {
                source, recovery, ..
            } => {
                self.scan_expr(source);
                if let Some(recovery) = recovery {
                    self.scan_expr(recovery);
                }
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                self.scan_expr(call);
                for argument in selected_arguments {
                    self.scan_expr(argument);
                }
            }
            HirExpressionKind::Ad {
                opener: Some(opener),
                ..
            } => self.scan_expr(opener),
            _ => {}
        }
    }

    fn scan_binary(&mut self, op: HirBinOp, lhs: &HirExpression, rhs: &HirExpression) {
        self.scan_expr(lhs);
        self.scan_expr(rhs);
        if matches!(
            op,
            HirBinOp::Mul | HirBinOp::Hadamard | HirBinOp::HadamardDiv
        ) {
            if self.is_fill_built(lhs)
                || self.is_fill_built(rhs)
                || is_comparison(lhs)
                || is_comparison(rhs)
            {
                self.in_region_mask = true;
            }
        } else if op == HirBinOp::Div {
            if !is_nonzero_literal(rhs) {
                self.nan_unsafe = true;
            }
            self.note_data(lhs);
            self.note_data(rhs);
        } else if !is_cmp(op) {
            self.note_data(lhs);
            self.note_data(rhs);
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
        HirExpressionKind::MethodCall(recv, method, _, args, _)
            if interner.resolve(*method) == "multiplicata" =>
        {
            is_mask_ref(recv, mask_defs, local_inits)
                || args
                    .first()
                    .is_some_and(|arg| is_mask_ref(&arg.expr, mask_defs, local_inits))
        }
        HirExpressionKind::Binary(op, lhs, rhs)
            if matches!(
                *op,
                HirBinOp::Mul | HirBinOp::Hadamard | HirBinOp::HadamardDiv
            ) =>
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
        HirExpressionKind::Literal(
            HirLiteral::Float(value) | HirLiteral::FloatText { value, .. },
        ) => *value != 0.0,
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
            HirExpressionKind::MethodCall(recv, _, _, args, _) => {
                self.scan_expr(recv);
                for arg in args {
                    self.scan_expr(&arg.expr);
                }
            }
            HirExpressionKind::Unary(_, operand) | HirExpressionKind::Field(operand, _) => {
                self.scan_expr(operand);
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
            next_def: census
                .max_def
                .saturating_add(1)
                .max(HIR_SYNTHETIC_DEF_ID_BASE),
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
        if !def_id.is_companion() {
            self.max_def = self.max_def.max(def_id.0);
        }
    }

    fn visit_block(&mut self, block: &HirBlock) {
        walk_block(self, block);
    }
}
