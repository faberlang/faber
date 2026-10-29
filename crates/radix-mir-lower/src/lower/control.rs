//! Control-flow, iteration, reduction, and alternate-exit lowering for MIR functions.
//!
//! This module lowers HIR control constructs, iteration, reductions, and
//! alternate-exit paths into explicit MIR blocks and terminators, or reports an
//! unsupported-lowering diagnostic. Value-producing control flow writes into a
//! destination place so branch arms join through storage rather than through an
//! implicit expression stack.
//!
//! ERROR HANDLING
//! ==============
//! Local `cape` handlers are modeled as a stack of handler contexts. `iace` and
//! failable calls assign the error value into the active handler place and jump
//! to its handler block. Without a local handler, `iace` can only lower for
//! functions with a declared alternate-exit type; otherwise it fails closed.
//!
//! CFG INVARIANTS
//! ==============
//! - Branch and loop helpers create all successor blocks before terminating the
//!   current block.
//! - Reachable arms jump to an explicit join block.
//! - Unreachable joins are sealed with `Unreachable` so MIR validation sees a
//!   terminator on every block.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use radix_hir::visit::HirVisitor;
use radix_mir::kernel_plan::GRID_REDUCE_WORKGROUP;
use radix_types::i64s_from_literal_index;

/// The valued `redde` type of a `filum` term block.
fn filum_term_type(body: &HirBlock) -> Option<MirType> {
    struct TermType(Option<TypeId>);
    impl HirVisitor for TermType {
        fn visit_stmt(&mut self, stmt: &HirStatement) {
            if let HirStatementKind::Redde(Some(value)) = &stmt.kind {
                self.0 = value.ty;
            }
            radix_hir::visit::walk_stmt(self, stmt);
        }
    }
    let mut term = TermType(None);
    term.visit_block(body);
    Some(MirType::semantic(term.0?))
}

/// Whether `body` holds a bracket store `place[k] ← v` whose `place` is the
/// local or parameter `def` (need, ruling O1(3)). A bracket store can
/// add a key to a map but never remove one, so a length comparison at each loop
/// step catches every insert; the method forms are rejected at check time.
fn body_bracket_stores_into(body: &HirBlock, def: DefId) -> bool {
    struct Stores {
        def: DefId,
        found: bool,
    }
    impl Stores {
        fn is_target(&self, target: &HirExpression) -> bool {
            let HirExpressionKind::Index(object, _) = &target.kind else {
                return false;
            };
            let object = match &object.kind {
                HirExpressionKind::NonNull(inner, radix_hir::HirNonNullKind::Value) => inner,
                _ => object,
            };
            matches!(&object.kind, HirExpressionKind::Path(def) if *def == self.def)
        }
    }
    impl HirVisitor for Stores {
        fn visit_expr(&mut self, expr: &HirExpression) {
            match &expr.kind {
                HirExpressionKind::Assign(target, _) | HirExpressionKind::ConversioAssign { target, .. }
                    if self.is_target(target) =>
                {
                    self.found = true;
                }
                _ => {}
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    let mut stores = Stores { def, found: false };
    stores.visit_block(body);
    stores.found
}

/// Read a coordinate binder's checked type from its body uses. HIR keeps
/// coordinate declarations as ids; the path carries the semantic type.
pub(super) fn coordinate_type(body: &HirBlock, def: DefId) -> Option<TypeId> {
    reduction_coordinate_type(Some(body), None, def)
}

pub(super) fn reduction_coordinate_type(
    body: Option<&HirBlock>,
    group: Option<&HirExpression>,
    def: DefId,
) -> Option<TypeId> {
    struct Find {
        def: DefId,
        ty: Option<TypeId>,
    }
    impl HirVisitor for Find {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if matches!(expr.kind, HirExpressionKind::Path(id) if id == self.def) {
                self.ty = self.ty.or(expr.ty);
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    let mut find = Find { def, ty: None };
    if let Some(body) = body {
        find.visit_block(body);
    }
    if let Some(group) = group {
        find.visit_expr(group);
    }
    find.ty
}

fn group_index_coordinates(index: &HirExpression) -> Option<Vec<DefId>> {
    match &index.kind {
        HirExpressionKind::Array(elements) => elements
            .iter()
            .map(|element| match element {
                radix_hir::HirArrayElement::Expr(expr) => match &expr.kind {
                    HirExpressionKind::Path(def_id) => Some(*def_id),
                    _ => None,
                },
                radix_hir::HirArrayElement::Spread(_) | radix_hir::HirArrayElement::Infer(_) => None,
            })
            .collect(),
        HirExpressionKind::Path(def_id) => Some(vec![*def_id]),
        _ => None,
    }
}

fn shape_axes(types: &TypeTable, shape: IndexId) -> Vec<IndexExpr> {
    match types.get_index(shape) {
        IndexExpr::Tuple(axes) => axes
            .iter()
            .map(|axis| types.get_index(*axis).clone())
            .collect(),
        other => vec![other.clone()],
    }
}

pub(super) fn grouped_key_tensor_reads_are_proven(
    types: &TypeTable,
    source: &HirExpression,
    coordinates: &[(DefId, Symbol)],
    group: &HirExpression,
) -> bool {
    struct Reads<'a> {
        types: &'a TypeTable,
        coordinates: &'a [(DefId, Symbol)],
        source_axes: &'a [IndexExpr],
        safe: bool,
    }
    impl HirVisitor for Reads<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Index(receiver, index) = &expr.kind
                && let Some(receiver_ty) = receiver.ty.map(|ty| self.types.peel_access_marker(ty).1)
                && let Type::Tensor(_, shape) = self.types.get(receiver_ty)
            {
                let target_axes = shape_axes(self.types, *shape);
                let Some(index_ids) = group_index_coordinates(index) else {
                    self.safe = false;
                    radix_hir::visit::walk_expr(self, expr);
                    return;
                };
                if target_axes.len() == index_ids.len() {
                    for (axis, coordinate) in target_axes.iter().zip(index_ids) {
                        let Some(source_axis) = self
                            .coordinates
                            .iter()
                            .position(|(id, _)| *id == coordinate)
                        else {
                            self.safe = false;
                            continue;
                        };
                        if self.source_axes.get(source_axis) != Some(axis) {
                            self.safe = false;
                        }
                    }
                } else {
                    self.safe = false;
                }
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    let Some(source_ty) = source.ty.map(|ty| types.peel_access_marker(ty).1) else {
        return false;
    };
    let Type::Tensor(_, source_shape) = types.get(source_ty) else {
        return false;
    };
    let source_axes = shape_axes(types, *source_shape);
    let mut reads = Reads { types, coordinates, source_axes: &source_axes, safe: true };
    reads.visit_expr(group);
    reads.safe
}

/// The device reduce recipe for an operator, or `None` for an operator no
/// recipe carries (`first`).
fn mir_reduce_op(op: radix_hir::HirReductionOp) -> Option<MirReduceOp> {
    Some(match op {
        radix_hir::HirReductionOp::Sum => MirReduceOp::Sum,
        radix_hir::HirReductionOp::Product => MirReduceOp::Product,
        radix_hir::HirReductionOp::Max => MirReduceOp::Max,
        radix_hir::HirReductionOp::Min => MirReduceOp::Min,
        radix_hir::HirReductionOp::ArgMax => MirReduceOp::ArgMax,
        radix_hir::HirReductionOp::ArgMin => MirReduceOp::ArgMin,
        radix_hir::HirReductionOp::All => MirReduceOp::All,
        radix_hir::HirReductionOp::Any => MirReduceOp::Any,
        radix_hir::HirReductionOp::Count => MirReduceOp::Count,
        radix_hir::HirReductionOp::First => return None,
    })
}

/// A grid-form `filum` reduction whose term body lowers to its own function.
struct FilumTermFunction<'a> {
    op: MirReduceOp,
    coordinates: &'a [(DefId, Symbol)],
    filum: &'a (DefId, Symbol),
    binding: Option<&'a (DefId, Symbol)>,
    body: &'a HirBlock,
    element_ty: TypeId,
    /// The source, extents, and optional identity operands so far.
    args: Vec<MirOperand>,
    group: Option<MirFilumGroupOp>,
    group_args: Vec<MirOperand>,
    result_mir_ty: MirType,
    expr: &'a HirExpression,
}

fn arm_body_returns(arm: &HirCasuArm) -> bool {
    matches!(
        &arm.body.kind,
        HirExpressionKind::Block(block)
            if block
                .statements
                .iter()
                .any(|statement| matches!(statement.kind, HirStatementKind::Redde(_)))
    )
}

fn is_literal_alternatives(alternatives: &[HirPattern]) -> bool {
    !alternatives.is_empty()
        && alternatives
            .iter()
            .all(|alternative| matches!(alternative, HirPattern::Literal(_) | HirPattern::NegatedLiteral(_)))
}

/// Variant alternatives (`casu A aut B`): every alternative is a variant
/// pattern (optionally aliased), so one arm body serves them all.
fn is_variant_alternatives(alternatives: &[HirPattern]) -> bool {
    !alternatives.is_empty()
        && alternatives.iter().all(|alternative| match alternative {
            HirPattern::Variant(_, _) => true,
            HirPattern::Alias(_, _, inner) => matches!(inner.as_ref(), HirPattern::Variant(_, _)),
            _ => false,
        })
}

fn collect_pattern_binding_defs(pattern: &HirPattern, defs: &mut Vec<DefId>) {
    match pattern {
        HirPattern::Binding(def_id, _) => defs.push(*def_id),
        HirPattern::Alias(def_id, _, inner) => {
            defs.push(*def_id);
            collect_pattern_binding_defs(inner, defs);
        }
        HirPattern::Variant(_, fields) => {
            for field in fields {
                collect_pattern_binding_defs(field, defs);
            }
        }
        _ => {}
    }
}

struct RangeIteraLowering<'a> {
    binding: DefId,
    name: Symbol,
    /// Source declared the binder `varia`: the body gets a fresh
    /// per-iteration copy, so a write to it never steers the loop.
    mutable: bool,
    start: &'a HirExpression,
    end: &'a HirExpression,
    step: Option<&'a HirExpression>,
    kind: HirRangeKind,
    body: &'a HirBlock,
    span: Span,
}

/// One already-materialized axis of the Cartesian range product (NR-3):
/// the binder local plus its loop-invariant start/bound/step locals and
/// range kind. The axis CFG itself is built by
/// [`FunctionBuilder::lower_range_product_level`].
struct RangeProductAxis {
    index: MirLocalId,
    /// The body-visible binder: the counter itself for a `fixum` binder, a
    /// per-iteration copy of it for a `varia` binder.
    binder: MirLocalId,
    start: MirLocalId,
    bound: MirLocalId,
    step: MirLocalId,
    kind: HirRangeKind,
    /// The index/bound/step type (ruling 43): `inf` when a bound is, else `numerus`.
    index_ty: MirType,
}

#[derive(Clone)]
pub(super) enum CollectionIteraKind {
    Array(TypeId),
    /// Byte value iteration uses the opaque carrier's length and u8 index row.
    Bytes(TypeId),
    /// Queue value iteration preserves insertion order (FIFO).
    Queue(TypeId),
    /// Stack value iteration walks removal order (top-first LIFO).
    Stack(TypeId),
    MapKeys(TypeId),
    MapValues(TypeId),
    /// `itera ex s` over a `copia`: walk a snapshot of the elements.
    /// Order is unspecified (D19.2).
    SetElements(TypeId),
    /// `itera ex m apud [k] fixum v` (D3.1): walk a key snapshot and read
    /// each value back by key; an `iuncta` key binds one name per part
    /// (D3.3). Order is unspecified (D19.2).
    MapEntries {
        key: TypeId,
        value: TypeId,
    },
    Cursor(TypeId),
    AsyncCursor(TypeId),
    /// Coordinate-free rank-1 tensor walk. The literal lane count is the
    /// folded case of the carrier's run-time extent.
    Tensor(TypeId, Option<i64>),
    /// Unified shaped lane: tensors and matrices share rank-N loop lowering.
    /// The matrix flag selects static cell projections for the register carrier;
    /// tensors use the runtime indexed-access ABI for rank-N coordinates.
    /// `literal` carries the extents when they are all compile-time facts;
    /// otherwise each axis bound is read from the carrier at run time.
    Shaped {
        element: TypeId,
        rank: usize,
        literal: Option<Vec<i64>>,
        matrix: bool,
    },
    // Route B (AEU-4): runtime-len `lista<T, magnitudo N>` splits on the carrier `len`
    // into N+1 guard-free unrolled arms; the literal capacity is carried so
    // the split stays base-lane MIR (never AIR).
    BoundedArray(TypeId, i64),
    // Text iteration (need, D10.1): `for from s const c` over
    // `textus` / `ascii` walks one `littera` per Unicode scalar. The text is
    // snapshot once through the cord cursor (`s.cursor()`), drained into a
    // `lista<littera>`, and read by the generic index loop — never the
    // `s[i]` projection, whose `textus` row still traps on non-ASCII (F5).
    Text(TypeId),
    Unsupported,
}

/// One `filum` reduction handed to [`FunctionBuilder::lower_filum_reduce_expr`].
pub(super) struct FilumReduceLowering<'a> {
    pub(super) op: radix_hir::HirReductionOp,
    pub(super) form: MirFilumForm,
    pub(super) source: &'a HirExpression,
    pub(super) group: Option<&'a HirExpression>,
    pub(super) coordinates: &'a [(DefId, Symbol)],
    pub(super) filum: &'a (DefId, Symbol),
    pub(super) binding: Option<&'a (DefId, Symbol)>,
    pub(super) body: Option<&'a HirBlock>,
    pub(super) identity: Option<&'a HirExpression>,
}

struct ArrayIteraLowering<'a> {
    mode: HirIteraMode,
    coordinates: &'a [(DefId, Symbol)],
    binding: DefId,
    name: Symbol,
    iter: &'a HirExpression,
    body: &'a HirBlock,
    span: Span,
    /// A source already materialized into a `lista<T>` temp (an interval
    /// value in `itera ab`, need): the loop walks this operand
    /// instead of re-lowering `iter`; the `TypeId` is the element type.
    materialized: Option<(MirOperand, TypeId)>,
}

impl FunctionBuilder<'_> {
    /// Lower `iace` either to the innermost active `cape` handler or to a
    /// function-level alternate return.
    pub(super) fn lower_iace(&mut self, value: &HirExpression, span: Span) -> Option<MirOperand> {
        // UT: inside a `capta` boundary the thrown payload wraps into the trap
        // carrier — it must never reach `ReturnError`.
        if let Some(trap) = self.take_innermost_trap() {
            let error_ty = self.expr_ty(value)?;
            let operand = self.lower_transfer_expr(value)?;
            self.assign(trap.destination, operand, error_ty, span);
            self.terminate_current(MirTerminatorKind::Goto(trap.join_block), span);
            return None;
        }

        if let Some(handler) = self.handlers.last().cloned() {
            let error_ty = self.expr_ty(value)?;
            let value = self.lower_transfer_expr(value)?;
            let place = handler.error_place;
            self.assign(place, value, error_ty, span);
            self.terminate_current(MirTerminatorKind::Goto(handler.error_block), span);
            return None;
        }

        if self.error_ty.is_none() {
            self.errors
                .push(MirError::unsupported(span, "iace without a declared alternate-exit type"));
            return None;
        }

        let value = self.lower_transfer_expr(value)?;
        self.terminate_current(MirTerminatorKind::ReturnError(value), span);
        None
    }

    /// Lower a handled expression when the handler is statement-like.
    ///
    /// Expression-valued handlers are intentionally rejected until MIR has a
    /// value-join representation for handler results.
    pub(super) fn lower_handled_expr(
        &mut self,
        body: &HirBlock,
        catch: &HirCape,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "expression-valued cape handler before value-join MIR lowering",
            ));
            return None;
        }

        self.lower_handled_block(body, catch, expr.span);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    fn lower_handled_block(&mut self, body: &HirBlock, catch: &HirCape, span: Span) {
        let Some(error_ty) = catch.binding_ty.map(MirType::semantic) else {
            self.errors
                .push(MirError::missing_type(catch.span, "cape handler binding"));
            return;
        };

        let handler_id = self.fresh_block(catch.body.span);
        let after_id = self.fresh_block(span);
        let handler_local = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: handler_local,
            name: Some(catch.binding_name),
            ty: error_ty,
            mutable: false,
            span: catch.span,
        });
        let handler_binding = LocalBinding { local: handler_local, ty: error_ty };
        self.bindings.insert(catch.binding_def_id, handler_binding);

        self.capture_depth += 1;
        self.handlers.push(HandlerContext {
            error_place: MirPlace::local(handler_local),
            error_ty,
            error_block: handler_id,
            depth: self.capture_depth,
        });
        let omnia = handled_body_is_omnia(body);
        self.open_device_block(omnia, handler_id, MirPlace::local(handler_local), span);
        self.visit_block(body);
        self.handlers.pop();
        self.close_device_block(span);
        let body_reaches = self.terminate_open_current(MirTerminatorKind::Goto(after_id), span);

        // A diverging tail (`mori`) can end the body with no error edge into
        // the handler. When nothing targets the handler *and* the body itself
        // diverged, the catch is dead: sealing it keeps `after_id` free of
        // orphaned successor targets. A body that terminates any other way
        // with no error edge is a lowering defect, not a dead catch, so it
        // keeps the old shape and is reported by post-lowering validation.
        let handler_entered = self.blocks.iter().any(|block| {
            block
                .terminator
                .as_ref()
                .is_some_and(|terminator| terminator_successors(&terminator.kind).contains(&handler_id))
        });
        let body_diverges = self.last_terminated.is_some_and(|block| {
            self.block(block)
                .terminator
                .as_ref()
                .is_some_and(|terminator| matches!(terminator.kind, MirTerminatorKind::Unreachable))
        });
        let handler_reaches = if handler_entered || !body_diverges {
            self.switch_to(handler_id);
            self.emit_device_handler_exit(catch.span);
            self.visit_block(&catch.body);
            self.terminate_open_current(MirTerminatorKind::Goto(after_id), catch.span)
        } else {
            self.seal_unreachable(handler_id, catch.span);
            false
        };

        if body_reaches || handler_reaches {
            self.switch_to(after_id);
        } else {
            self.seal_unreachable(after_id, span);
        }
    }

    /// Lower a `capta { ... }` capture boundary (UT).
    ///
    /// The trap result is the union-typed destination temp. The body lowers
    /// into that destination (its trailing expression is the success member);
    /// every failable call and `iace` inside wraps its payload into the same
    /// destination through the existing `TryCall` error block and joins — a
    /// trapped error never reaches `ReturnError`.
    pub(super) fn lower_trap_expr(&mut self, body: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let span = expr.span;
        let destination_temp = self.push_temp(ty, span);
        let destination = MirPlace::temp(destination_temp);
        let join = self.fresh_block(span);

        self.capture_depth += 1;
        self.traps.push(TrapContext {
            destination: destination.clone(),
            join_block: join,
            error_paths: false,
            depth: self.capture_depth,
        });
        self.lower_block_to_destination(body, destination.clone(), ty, span);
        let Some(trap) = self.traps.pop() else {
            self.errors.push(MirError::validation(
                span,
                "capture trap context was popped before its body finished",
            ));
            return None;
        };

        let body_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join), span);
        if body_reaches || trap.error_paths {
            self.switch_to(join);
        } else {
            self.seal_unreachable(join, span);
        }
        Some(MirOperand::Temp(destination_temp))
    }

    /// Lower `call ⊥ default` (D17.7) through the trap machinery.
    ///
    /// A trap context whose destination is a discarded error temp and whose
    /// join is a fail block: every error path of the call (the existing
    /// `TryCall` error block) stores its payload there and jumps to the fail
    /// block, which alone lowers the default into the result. The default is
    /// therefore evaluated only on failure, and panics never reach it.
    pub(super) fn lower_call_default_expr(
        &mut self,
        body: &HirBlock,
        default: &HirExpression,
        err_ty: Option<TypeId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if self.device_role.is_some() {
            return self.lower_kernel_call_default(body, default, expr);
        }
        let ty = self.expr_ty(expr)?;
        let span = expr.span;
        let Some(err_ty) = err_ty else {
            self.errors
                .push(MirError::missing_type(span, "call default error channel"));
            return None;
        };
        let destination_temp = self.push_temp(ty, span);
        let destination = MirPlace::temp(destination_temp);
        let error_temp = self.push_temp(MirType::semantic(err_ty), span);
        let fail_block = self.fresh_block(span);
        let join = self.fresh_block(span);

        self.capture_depth += 1;
        self.traps.push(TrapContext {
            destination: MirPlace::temp(error_temp),
            join_block: fail_block,
            error_paths: false,
            depth: self.capture_depth,
        });
        self.lower_block_to_destination(body, destination.clone(), ty, span);
        let error_paths = self.traps.pop().is_some_and(|trap| trap.error_paths);
        let body_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join), span);

        let mut default_reaches = false;
        if error_paths {
            self.switch_to(fail_block);
            self.lower_expr_to_destination(default, destination, ty)?;
            default_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join), default.span);
        } else {
            self.seal_unreachable(fail_block, span);
        }

        if body_reaches || default_reaches {
            self.switch_to(join);
        } else {
            self.seal_unreachable(join, span);
        }
        Some(MirOperand::Temp(destination_temp))
    }

    /// `call ⊥ default` inside a kernel body (operator ruling 2026-10-08): a
    /// value select that never unwinds. The one admitted call is the checked
    /// tensor read `t.get([i, ..]) ⊥ d`. The result is
    ///
    /// ```text
    /// in = (c_0 >= 0) & (c_0 < E_0) & ... & (c_n >= 0) & (c_n < E_n)
    /// if in { dest = <the read, bare> } else { dest = d }
    /// ```
    ///
    /// with `E_k` a constant for a literal axis and a `TensorExtent` otherwise.
    /// The same form serves the host (sequential) and device kernel lowerings,
    /// so the oracle and the device agree. No `TryCall`, no error channel.
    fn lower_kernel_call_default(
        &mut self,
        body: &HirBlock,
        default: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let span = expr.span;
        let ty = self.expr_ty(expr)?;
        let Some((receiver, index)) = self.kernel_default_tensor_read(body) else {
            self.errors.push(MirError {
                message: "device lowering of `⊥` is pending for this call: a kernel admits only the checked \
                          tensor read `t.get([i, ..]) ⊥ d`"
                    .to_owned(),
                issue: super::lane_form::KERNEL_CALL_DEFAULT_PENDING_ISSUE.to_owned(),
                span,
                definition_span: None,
            });
            return None;
        };
        let receiver_ty = self.method_receiver_payload_id(receiver.ty?);
        let rank = index.len();
        let CollectionIteraKind::Shaped { rank: tensor_rank, literal, matrix: false, .. } =
            self.classify_collection_itera(receiver_ty, HirIteraMode::Ex, rank)
        else {
            self.errors
                .push(MirError::unsupported(span, "`⊥` read of a source that is not a ranked tensor"));
            return None;
        };
        if tensor_rank != rank {
            self.errors
                .push(MirError::unsupported(span, "`⊥` read coordinate count differs from rank"));
            return None;
        }
        let lowered = self.lower_expr_value(receiver)?;
        let tensor = self.materialize_operand_place(lowered, MirType::semantic(receiver_ty), receiver.span);
        let extents = self.tensor_axis_extents(&tensor, literal.as_deref(), rank, span)?;
        let mut values = Vec::with_capacity(rank);
        for coordinate in &index {
            values.push(self.lower_expr_value(coordinate)?);
        }
        let condition = self.tensor_in_range_condition(&values, &extents, span)?;

        let destination_temp = self.push_temp(ty, span);
        let destination = MirPlace::temp(destination_temp);
        let read_block = self.fresh_block(span);
        let default_block = self.fresh_block(span);
        let join = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: read_block, else_block: default_block },
            span,
        );

        self.switch_to(read_block);
        if self.is_device_lowering() {
            // The in-range read is the row-major linear `Index` place a
            // bracket read uses; the bare `TensorGet` takes a host list.
            let linear = if rank == 1 {
                self.lower_expr_value(index[0])?
            } else {
                let extents: Vec<Option<i64>> = match literal.as_deref() {
                    Some(dimensions) => dimensions.iter().copied().map(Some).collect(),
                    None => vec![None; rank],
                };
                self.lower_row_major_linear_index(&tensor, &index, &extents, span)?
            };
            let mut read = tensor;
            read.projections.push(MirProjection::Index(linear));
            self.assign(destination.clone(), MirOperand::Place(read), ty, span);
        } else {
            // The branch above is the range check: the bare read below must
            // not add the kernel's checked-access fault site of its own.
            let channel = self.error_ty.take();
            self.lower_block_to_destination(body, destination.clone(), ty, span);
            self.error_ty = channel;
        }
        self.terminate_open_current(MirTerminatorKind::Goto(join), span);

        self.switch_to(default_block);
        self.lower_expr_to_destination(default, destination, ty)?;
        self.terminate_open_current(MirTerminatorKind::Goto(join), default.span);

        self.switch_to(join);
        Some(MirOperand::Temp(destination_temp))
    }

    /// The extent of every axis of `tensor`: a constant for a literal axis, a
    /// `TensorExtent` read otherwise.
    fn tensor_axis_extents(
        &mut self,
        tensor: &MirPlace,
        literal: Option<&[i64]>,
        rank: usize,
        span: Span,
    ) -> Option<Vec<MirOperand>> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        (0..rank)
            .map(|axis| match literal {
                Some(dimensions) => Some(MirOperand::Constant(MirConstant::Int(dimensions[axis]))),
                None => Some(self.assign_temp(
                    MirValueKind::TensorExtent {
                        tensor: MirOperand::Place(tensor.clone()),
                        axis: u32::try_from(axis).ok()?,
                    },
                    numerus,
                    span,
                )),
            })
            .collect()
    }

    /// `(c_0 >= 0) & (c_0 < E_0) & ... & (c_n >= 0) & (c_n < E_n)`.
    fn tensor_in_range_condition(
        &mut self,
        values: &[MirOperand],
        extents: &[MirOperand],
        span: Span,
    ) -> Option<MirOperand> {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let mut within: Option<MirOperand> = None;
        for (value, extent) in values.iter().zip(extents) {
            let low = self.binary_temp(
                MirBinOp::GtEq,
                value.clone(),
                MirOperand::Constant(MirConstant::Int(0)),
                bivalens,
                span,
            );
            let high = self.binary_temp(MirBinOp::Lt, value.clone(), extent.clone(), bivalens, span);
            let axis_in = self.binary_temp(MirBinOp::And, low, high, bivalens, span);
            within = Some(match within {
                Some(previous) => self.binary_temp(MirBinOp::And, previous, axis_in, bivalens, span),
                None => axis_in,
            });
        }
        within
    }

    /// An unhandled failable tensor `get` / `set` inside a kernel that declares
    /// a `⇥ text` channel (gpu-reset U9-7). `None` when the call is not that
    /// shape (the caller lowers it as before). Otherwise
    ///
    /// ```text
    /// in = (c_0 >= 0) & (c_0 < E_0) & ... & (c_n >= 0) & (c_n < E_n)
    /// if in { <the read or the store, bare> } else { ReturnError("tensor accipe|ponde invalid index") }
    /// ```
    ///
    /// with `E_k` a constant for a literal axis and a `TensorExtent` otherwise,
    /// and the text the one the checked host helper delivers
    /// (`push_checked_tensor_helper`). The failing block is a constant-text
    /// `ReturnError`, which the device gate reads as a recoverable throw site
    /// (`abi/throw_sites.rs`); no `TryCall`. The host kernel lowering emits the
    /// same compare, so `--device cpu` is the oracle for the device text.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_kernel_checked_tensor_access(
        &mut self,
        op: MirCollectionOp,
        lowered_receiver: &MirOperand,
        receiver: &HirExpression,
        method_name: &str,
        args: &[radix_hir::HirCallArg],
        expr_ty: MirType,
        span: Span,
    ) -> Option<Option<MirOperand>> {
        if self.device_role.is_none() || !self.handlers.is_empty() || !self.traps.is_empty() {
            return None;
        }
        let channel = self.error_ty?;
        if !matches!(self.types.get(channel.semantic_id()), Type::Primitive(Primitive::Textus)) {
            return None;
        }
        let is_set = op == MirCollectionOp::TensorSet;
        if args.len() != if is_set { 2 } else { 1 } || !self.is_failable_tensor_method(receiver, method_name, args) {
            return None;
        }
        let HirExpressionKind::Array(items) = &args[0].expr.kind else {
            return None;
        };
        let coordinates: Vec<&HirExpression> = items
            .iter()
            .map(|item| match item {
                HirArrayElement::Expr(expr) => Some(expr),
                HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => None,
            })
            .collect::<Option<_>>()?;
        let rank = coordinates.len();
        let receiver_ty = self.method_receiver_payload_id(receiver.ty?);
        let CollectionIteraKind::Shaped { rank: tensor_rank, literal, matrix: false, .. } =
            self.classify_collection_itera(receiver_ty, HirIteraMode::Ex, rank)
        else {
            return None;
        };
        if rank == 0 || tensor_rank != rank {
            return None;
        }
        Some(self.emit_kernel_checked_tensor_access(
            is_set,
            lowered_receiver,
            MirType::semantic(receiver_ty),
            args,
            &coordinates,
            literal.as_deref(),
            expr_ty,
            span,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_kernel_checked_tensor_access(
        &mut self,
        is_set: bool,
        lowered_receiver: &MirOperand,
        receiver_ty: MirType,
        args: &[radix_hir::HirCallArg],
        coordinates: &[&HirExpression],
        literal: Option<&[i64]>,
        expr_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let rank = coordinates.len();
        let tensor = self.materialize_operand_place(lowered_receiver.clone(), receiver_ty, span);
        let extents = self.tensor_axis_extents(&tensor, literal, rank, span)?;
        let mut values = Vec::with_capacity(rank);
        for coordinate in coordinates {
            values.push(self.lower_expr_value(coordinate)?);
        }
        let stored = if is_set {
            Some(self.lower_call_arg_value(&args[1].expr)?)
        } else {
            None
        };
        let condition = self.tensor_in_range_condition(&values, &extents, span)?;

        let ok_block = self.fresh_block(span);
        let fail_block = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: ok_block, else_block: fail_block },
            span,
        );
        self.switch_to(fail_block);
        let text = if is_set {
            self.context.diagnostic_ponde_invalid_index
        } else {
            self.context.diagnostic_accipe_invalid_index
        };
        self.terminate_current(
            MirTerminatorKind::ReturnError(MirOperand::Constant(MirConstant::String(text))),
            span,
        );
        self.switch_to(ok_block);

        if self.is_device_lowering() {
            // The in-range access is the row-major linear `Index` place a
            // bracket access uses; the bare `TensorGet` / `TensorSet` take a
            // host list.
            let mut linear = values[0].clone();
            for (value, extent) in values.iter().zip(&extents).skip(1) {
                let scaled = self.assign_temp(
                    MirValueKind::Binary { op: MirBinOp::Mul, lhs: linear, rhs: extent.clone() },
                    numerus,
                    span,
                );
                linear = self.assign_temp(
                    MirValueKind::Binary { op: MirBinOp::Add, lhs: scaled, rhs: value.clone() },
                    numerus,
                    span,
                );
            }
            let mut place = tensor;
            place.projections.push(MirProjection::Index(linear));
            return Some(if let Some(value) = stored {
                let value_ty = MirType::semantic(args[1].expr.ty?);
                self.assign(place, value, value_ty, span);
                MirOperand::Constant(MirConstant::Unit)
            } else {
                let destination = self.push_temp(expr_ty, span);
                self.assign(MirPlace::temp(destination), MirOperand::Place(place), expr_ty, span);
                MirOperand::Temp(destination)
            });
        }

        let list_ty = MirType::semantic(args[0].expr.ty?);
        let items = values.into_iter().map(MirAggregateItem::Operand).collect();
        let list = self.construct_temp(MirAggregateKind::Array, MirAggregateFields::Ordered(items), list_ty, span);
        let receiver = MirOperand::Place(tensor);
        Some(match stored {
            Some(value) => {
                self.runtime_call_to_place(
                    None,
                    MirIntrinsic::Collection(MirCollectionOp::TensorSet),
                    vec![receiver, list, value],
                    expr_ty,
                    span,
                );
                MirOperand::Constant(MirConstant::Unit)
            }
            None => self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::TensorGet),
                vec![receiver, list],
                expr_ty,
                span,
            ),
        })
    }

    /// The receiver and coordinate expressions of a block that is exactly one
    /// checked tensor read `t.get([c0, .., cn])`.
    fn kernel_default_tensor_read<'h>(
        &self,
        body: &'h HirBlock,
    ) -> Option<(&'h HirExpression, Vec<&'h HirExpression>)> {
        let call = match (body.statements.as_slice(), body.expr.as_deref()) {
            ([], Some(call)) => call,
            ([statement], None) => match &statement.kind {
                HirStatementKind::Expr(call) => call,
                _ => return None,
            },
            _ => return None,
        };
        let HirExpressionKind::MethodCall(receiver, method, _, args, _) = &call.kind else {
            return None;
        };
        let method_name = self.resolve_method_name(*method)?;
        let payload = self
            .types
            .get(self.method_receiver_payload_id(receiver.ty?));
        let entry = radix_intrinsics::lookup_method_for_type(payload, method_name, args.len())?;
        if !entry.failable || args.len() != 1 || !matches!(payload, Type::Tensor(..)) {
            return None;
        }
        let HirExpressionKind::Array(items) = &args[0].expr.kind else {
            return None;
        };
        let coordinates: Vec<&HirExpression> = items
            .iter()
            .map(|item| match item {
                HirArrayElement::Expr(expr) => Some(expr),
                HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => None,
            })
            .collect::<Option<_>>()?;
        (!coordinates.is_empty()).then_some((receiver.as_ref(), coordinates))
    }

    /// Lower any expression into an already-chosen destination place.
    ///
    /// This is the join-point contract for expression-valued blocks and `si`:
    /// complex control forms assign their eventual value into `destination`,
    /// while ordinary expressions lower to an operand and are assigned once.
    pub fn lower_expr_to_destination(
        &mut self,
        expr: &HirExpression,
        destination: MirPlace,
        ty: MirType,
    ) -> Option<()> {
        match &expr.kind {
            HirExpressionKind::Block(block) => self.lower_block_to_destination(block, destination, ty, expr.span),
            HirExpressionKind::Si { cond, then_block, then_catch, else_block } => {
                if then_catch.is_some() {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "expression-valued si with cape before handler value MIR lowering",
                    ));
                    return None;
                }
                self.lower_si_to_destination(cond, then_block, else_block.as_ref(), destination, ty, expr.span)
            }
            _ => {
                let value = self.lower_expr_value(expr)?;
                self.record_closure_destination(&destination, &value);
                if self.needs_vec3_to_vec4_widen(ty, expr) {
                    self.emit_vec3_to_vec4_construct(destination, value, ty, expr.span);
                } else {
                    self.assign(destination, value, ty, expr.span);
                }
                Some(())
            }
        }
    }

    /// Detect when a `@vertex` body assigns a vec3 value into a vec4
    /// destination — the lowering must emit a Construct(Vector) to widen
    /// the value with a `1.0` w-component instead of a plain Assign.
    fn needs_vec3_to_vec4_widen(&self, dest_ty: MirType, expr: &HirExpression) -> bool {
        if !matches!(
            self.shader_stage,
            Some(MirKernelShaderStage::Vertex | MirKernelShaderStage::Fragment)
        ) {
            return false;
        }
        let Some(f32_id) = self
            .types
            .find_sized_numeric(Primitive::Fractus, NumericWidth::F32)
        else {
            return false;
        };
        let is_vec4 = matches!(
            self.types.get(dest_ty.semantic_id()),
            Type::Vector(elem, width) if *elem == f32_id && matches!(self.types.get_index(*width), IndexExpr::Literal(4))
        );
        if !is_vec4 {
            return false;
        }
        let Some(value_ty) = expr.ty else {
            return false;
        };
        matches!(
            self.types.get(value_ty),
            Type::Vector(elem, width) if *elem == f32_id && matches!(self.types.get_index(*width), IndexExpr::Literal(3))
        )
    }

    /// Emit a `Construct(Vector)` statement that widens a vec3 operand to
    /// vec4 by projecting x, y, z as scalar lanes and appending a `1.0`
    /// w-component.  Each field is a scalar lane so MIR scalar-per-lane
    /// vector validation passes.
    fn emit_vec3_to_vec4_construct(&mut self, destination: MirPlace, value: MirOperand, ty: MirType, span: Span) {
        let f32_id = self
            .types
            .find_sized_numeric(Primitive::Fractus, NumericWidth::F32)
            .unwrap_or_else(|| self.types.primitive(Primitive::Fractus));
        let elem_ty = MirType::semantic(f32_id);

        // Materialize the vec3 source as a place so we can project lanes.
        let src_place = match value {
            MirOperand::Place(place) => place,
            MirOperand::Temp(temp) => MirPlace::temp(temp),
            other => {
                // Push a temp and assign the value to it first.
                // We know the source type is vec3<f32> from needs_vec3_to_vec4_widen.
                let temp = self.push_temp(elem_ty, span);
                self.assign(MirPlace::temp(temp), other, elem_ty, span);
                MirPlace::temp(temp)
            }
        };
        let one = self.assign_temp(
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(1.0))),
            elem_ty,
            span,
        );
        let aggregate = MirAggregate {
            kind: MirAggregateKind::Vector,
            ty,
            fields: MirAggregateFields::Ordered(vec![
                // Scalar-per-lane: four scalar elements, not mixed [vec3, scalar].
                Self::vec3_lane_operand(&src_place, 0),
                Self::vec3_lane_operand(&src_place, 1),
                Self::vec3_lane_operand(&src_place, 2),
                MirAggregateItem::Operand(one),
            ]),
        };
        self.append_stmt(MirStatement { kind: MirStatementKind::Construct { destination, aggregate }, span });
    }

    fn vec3_lane_operand(src: &MirPlace, lane: u8) -> MirAggregateItem {
        let mut projections = src.projections.clone();
        projections.push(MirProjection::VectorLane(lane));
        MirAggregateItem::Operand(MirOperand::Place(MirPlace { base: src.base.clone(), projections }))
    }

    /// Lower a HIR block used as an expression.
    ///
    /// Non-`vacuum` blocks allocate a temp and route the tail expression into
    /// it. Blocks with no tail expression are only valid when the expected MIR
    /// type is `vacuum`.
    pub(super) fn lower_block_expr(&mut self, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if self.is_vacuum(ty) {
            self.visit_block(block);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        let temp = self.push_temp(ty, expr.span);
        self.lower_block_to_destination(block, MirPlace::temp(temp), ty, expr.span)?;
        Some(MirOperand::Temp(temp))
    }

    /// Lower `si` in expression position.
    ///
    /// `vacuum` branches are statement-like. Value-producing branches require
    /// a `secus` block and share a temp destination so the two arms join through
    /// ordinary MIR storage.
    pub(super) fn lower_si_expr(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if self.is_vacuum(ty) {
            self.lower_si_stmt(cond, then_block, then_catch, else_block, expr.span);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        if then_catch.is_some() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "expression-valued si with cape before handler value MIR lowering",
            ));
            return None;
        }

        let temp = self.push_temp(ty, expr.span);
        self.lower_si_to_destination(cond, then_block, else_block, MirPlace::temp(temp), ty, expr.span)?;
        Some(MirOperand::Temp(temp))
    }

    /// Lower `dum` in expression position.
    ///
    /// Loops currently have a statement-like MIR contract: they may produce
    /// `vacuum`, but a non-`vacuum` loop result is rejected until the language
    /// has a defined value-yielding loop model.
    pub(super) fn lower_dum_expr(
        &mut self,
        cond: &HirExpression,
        block: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "dum expression with non-vacuum result"));
            return None;
        }

        self.lower_dum(cond, block, expr.span);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Lower supported `itera` subsets into target-neutral MIR.
    ///
    /// Numeric ranges and array-backed collection loops can use ordinary locals,
    /// branches, gotos, runtime length calls, and index projections. Map, set,
    /// text, and cursor iteration still need iterator/runtime ABI policy.
    pub fn lower_itera_expr(
        &mut self,
        mode: HirIteraMode,
        coordinates: &[(DefId, Symbol)],
        binding: DefId,
        name: Symbol,
        mutable: bool,
        iter: &HirExpression,
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if self.is_lane_region(expr) {
            return self.lower_lane_form_itera(coordinates, binding, name, iter, body, expr);
        }
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "itera expression with non-vacuum result"));
            return None;
        }
        match mode {
            HirIteraMode::Ab => {
                if let HirExpressionKind::Intervallum { start, end, step, kind } = &iter.kind {
                    self.lower_range_itera(RangeIteraLowering {
                        binding,
                        name,
                        mutable,
                        start,
                        end,
                        step: step.as_deref(),
                        kind: *kind,
                        body,
                        span: expr.span,
                    })?;
                } else {
                    // `itera ab` over a VALUE of `intervallum` type (not a
                    // range literal) materializes into a `lista<T>` temp
                    // through the runtime `interval_materialize` call, then
                    // the ordinary list-iteration machinery walks it in order
                    //. Any other source keeps the decline.
                    let Some(iter_ty) = iter.ty else {
                        self.errors
                            .push(MirError::missing_type(iter.span, "itera ab source"));
                        return None;
                    };
                    let interval_element = match self.normalized_type(iter_ty) {
                        Type::Intervallum(element, _) => Some(*element),
                        _ => None,
                    };
                    let Some(element) = interval_element else {
                        self.errors
                            .push(MirError::unsupported(iter.span, "itera ab source before range MIR lowering"));
                        return None;
                    };
                    let array_ty = self.require_array(element, iter.span, "itera ab interval source list")?;
                    let source = self.lower_expr_value(iter)?;
                    let materialized = self.runtime_call_value(
                        MirIntrinsic::Collection(MirCollectionOp::IntervalMaterialize),
                        vec![source],
                        MirType::semantic(array_ty),
                        iter.span,
                    );
                    self.lower_array_itera(ArrayIteraLowering {
                        mode,
                        coordinates,
                        binding,
                        name,
                        iter,
                        body,
                        span: expr.span,
                        materialized: Some((materialized, element)),
                    })?;
                }
            }
            HirIteraMode::Ex | HirIteraMode::De => {
                self.lower_array_itera(ArrayIteraLowering {
                    mode,
                    coordinates,
                    binding,
                    name,
                    iter,
                    body,
                    span: expr.span,
                    materialized: None,
                })?;
            }
        }
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Lower a sequential `reducta via summa` expression to a MIR fold over the shared
    /// apud tensor-iteration arm (SFR-4).
    ///
    /// The accumulator starts at the `+` identity; a valued `redde` in the
    /// term body adds its value into the accumulator (through the reduction fold
    /// context, never as a function return) and continues the iteration loop.
    /// The ungrouped distributed `filum` sum lowers to the declared kernel-
    /// plan entry (SFR-5): a `FilumSum` runtime call carrying the filum
    /// metadata, the captured term-body MIR, and the coordinate facts —
    /// the plan pass resolves it to `DistributedSum`, never inline MIR.
    /// Other operators retain the same sequential fold; grouped sum/count
    /// lane reductions carry their keys as typed helpers in the grid node.
    /// Sequential row-major reduction; each bucket owns exact running temps.
    pub(super) fn lower_reduction_expr(
        &mut self,
        op: radix_hir::HirReductionOp,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        group: Option<&HirExpression>,
        filum: Option<&(DefId, Symbol)>,
        binding: Option<&(DefId, Symbol)>,
        body: Option<&HirBlock>,
        identity: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        use radix_hir::HirReductionOp as Op;
        // The ungrouped `sum` is the one reduction with a declared device recipe;
        // every other `filum` reduction is the plain fold below, because `filum`
        // only permits parallel evaluation and carries no sequential meaning.
        // Host kernel lowering has no device recipe at all: the `sum` is the
        // same plain fold, the sequential meaning of the `filum` clause.
        if let Some(filum) = filum
            && self.is_device_lowering()
            && (group.is_none() || matches!(op, Op::Sum | Op::Count))
        {
            if self.is_lane_region(expr) {
                return self.lower_filum_reduce_expr(
                    FilumReduceLowering {
                        op,
                        form: MirFilumForm::Grid,
                        source,
                        group,
                        coordinates,
                        filum,
                        binding,
                        body,
                        identity,
                    },
                    expr,
                );
            }
            if self.lane_region.is_some() {
                self.errors.push(MirError {
                    message: "lane kernel: nested reduce is not lowered yet (C4-8 nested)".to_owned(),
                    issue: super::lane_form::LANE_FORM_NESTED_REDUCE_ISSUE.to_owned(),
                    span: expr.span,
                    definition_span: self.lane_region.map(|region| region.span),
                });
                return None;
            }
        }
        if self.is_device_lowering() && filum.is_some() && group.is_some() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "grouped grid reduction must be the top-level lane region",
            ));
            return None;
        }
        if let Some(filum) = filum
            && op == Op::Sum
            && group.is_none()
            && !self.is_host_kernel_lowering()
        {
            return self.lower_filum_reduce_expr(
                FilumReduceLowering {
                    op,
                    form: MirFilumForm::Declared,
                    source,
                    group: None,
                    coordinates,
                    filum,
                    binding,
                    body,
                    identity: None,
                },
                expr,
            );
        }
        // A `mut`/`own`/`ref` parameter carries its access marker on the
        // source type; the reduction reads the tensor carrier under it.
        let source_ty = self.types.peel_access_marker(source.ty?).1;
        let (element, shape, matrix) = match self.normalized_type(source_ty) {
            Type::Tensor(element, shape) => (*element, *shape, false),
            Type::Matrix(element, shape) => (*element, *shape, true),
            _ => {
                self.errors.push(MirError::unsupported(
                    source.span,
                    "reduction source before tensor or matrix MIR lowering",
                ));
                return None;
            }
        };
        let dimensions = i64s_from_literal_index(shape, self.types);
        let rank = match self.types.get_index(shape) {
            radix_types::IndexExpr::Tuple(dims) => dims.len(),
            radix_types::IndexExpr::Literal(_)
            | radix_types::IndexExpr::Param(_)
            | radix_types::IndexExpr::Runtime(_)
            | radix_types::IndexExpr::Affine { .. }
            | radix_types::IndexExpr::Divide { .. } => 1,
            _ => {
                self.errors
                    .push(MirError::unsupported(expr.span, "reducta_source_rank_unknown"));
                return None;
            }
        };
        if group.is_none() && body.is_none() {
            let collection = match op {
                Op::Max => Some(MirCollectionOp::TensorMax),
                Op::Min => Some(MirCollectionOp::TensorMin),
                _ => None,
            };
            if let Some(collection) = collection {
                let mut args = vec![self.lower_expr_value(source)?];
                if let Some(identity) = identity {
                    args.push(self.lower_expr_value(identity)?);
                }
                return Some(self.runtime_call_value(
                    MirIntrinsic::Collection(collection),
                    args,
                    MirType::semantic(expr.ty?),
                    expr.span,
                ));
            }
        }
        let result_ty = MirType::semantic(expr.ty?);
        let term = if let Some(body) = body {
            struct TermType(Option<TypeId>);
            impl HirVisitor for TermType {
                fn visit_stmt(&mut self, stmt: &HirStatement) {
                    if let HirStatementKind::Redde(Some(value)) = &stmt.kind {
                        self.0 = value.ty;
                    }
                    radix_hir::visit::walk_stmt(self, stmt);
                }
            }
            let mut term = TermType(None);
            term.visit_block(body);
            term.0?
        } else {
            element
        };
        let term_ty = MirType::semantic(term);
        // Count and arg-extrema carry the checked result's integer width;
        // other operators still bind coordinates at their checked body type.
        let mut payload = expr.ty?;
        loop {
            payload = match self.types.get(payload) {
                Type::Option(inner) | Type::Tensor(inner, _) => *inner,
                Type::Tuple(elements) if !elements.is_empty() => elements[0],
                _ => break,
            };
        }
        let result_integer = matches!(op, Op::Count | Op::ArgMax | Op::ArgMin).then_some(payload);
        let integer = MirType::semantic(
            result_integer
                .or_else(|| {
                    coordinates
                        .iter()
                        .find_map(|(id, _)| reduction_coordinate_type(body, group, *id))
                })
                .unwrap_or_else(|| self.types.primitive(Primitive::Numerus)),
        );
        let boolean = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let accumulator_ty = match op {
            Op::Sum | Op::Product if group.is_some() => match self.normalized_type(expr.ty?) {
                Type::Tensor(element, _) => MirType::semantic(*element),
                _ => return None,
            },
            Op::Sum | Op::Product => result_ty,
            Op::Count => integer,
            Op::All | Op::Any => boolean,
            // The accumulator holds the selected element; the term is the predicate.
            Op::First => MirType::semantic(element),
            _ => term_ty,
        };
        let source_value = self.lower_expr_value(source)?;
        let source_place = self.materialize_operand_place(source_value, MirType::semantic(source_ty), expr.span);
        let identity = match identity {
            Some(identity) => Some(self.lower_expr_value(identity)?),
            None => None,
        };
        let group_dims: Vec<usize> = if group.is_some() {
            let Type::Tensor(_, shape) = self.normalized_type(expr.ty?) else {
                return None;
            };
            i64s_from_literal_index(*shape, self.types)?
                .into_iter()
                .map(|dim| usize::try_from(dim).ok())
                .collect::<Option<_>>()?
        } else {
            Vec::new()
        };
        let bucket_count = if group.is_some() {
            group_dims.iter().product()
        } else {
            1
        };
        let zero = match self.types.get(accumulator_ty.semantic_id()) {
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => MirConstant::Float(0.0),
            Type::Primitive(Primitive::Bivalens) => MirConstant::Bool(false),
            _ => MirConstant::Int(0),
        };
        let initial = match op {
            Op::Product => match zero {
                MirConstant::Float(_) => MirConstant::Float(1.0),
                _ => MirConstant::Int(1),
            },
            Op::All => MirConstant::Bool(true),
            _ => zero.clone(),
        };
        let mut buckets = Vec::with_capacity(bucket_count);
        for _ in 0..bucket_count {
            let accumulator = self.push_temp(accumulator_ty, expr.span);
            self.assign(
                MirPlace::temp(accumulator),
                MirOperand::Constant(initial.clone()),
                accumulator_ty,
                expr.span,
            );
            let score = self.push_temp(term_ty, expr.span);
            let score_zero = match self.types.get(term) {
                Type::Primitive(Primitive::Bivalens) => MirConstant::Bool(false),
                Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                    MirConstant::Float(0.0)
                }
                _ => MirConstant::Int(0),
            };
            self.assign(MirPlace::temp(score), MirOperand::Constant(score_zero), term_ty, expr.span);
            let seen = self.push_temp(boolean, expr.span);
            self.assign(
                MirPlace::temp(seen),
                MirOperand::Constant(MirConstant::Bool(false)),
                boolean,
                expr.span,
            );
            let mut winner = Vec::new();
            if matches!(op, Op::ArgMax | Op::ArgMin) {
                for _ in 0..rank {
                    let temp = self.push_temp(integer, expr.span);
                    self.assign(
                        MirPlace::temp(temp),
                        MirOperand::Constant(MirConstant::Int(0)),
                        integer,
                        expr.span,
                    );
                    winner.push(temp);
                }
            }
            buckets.push(ReductionBucket { accumulator, score, seen, coordinates: winner });
        }
        let error_tensor = if group.is_some() {
            Some(self.reduction_tensor(&buckets, result_ty, accumulator_ty, &group_dims, expr.span)?)
        } else {
            None
        };
        let mut coordinate_locals = Vec::new();
        let saved_bindings = self.bindings.clone();
        for axis in 0..rank {
            let local = self.next_local_id();
            self.locals.push(MirLocalDecl {
                id: local,
                name: coordinates.get(axis).map(|(_, name)| *name),
                ty: integer,
                mutable: true,
                span: expr.span,
            });
            if let Some((id, _)) = coordinates.get(axis) {
                self.bindings
                    .insert(*id, LocalBinding { local, ty: integer });
            }
            coordinate_locals.push(local);
        }
        let element_local = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: element_local,
            name: binding.map(|(_, name)| *name),
            ty: MirType::semantic(element),
            mutable: true,
            span: expr.span,
        });
        if let Some((id, _)) = binding {
            self.bindings
                .insert(*id, LocalBinding { local: element_local, ty: MirType::semantic(element) });
        }
        let bounds = self.tensor_axis_bounds(&source_place, dimensions.as_deref(), rank, expr.span)?;
        let fold = ReductionFoldContext {
            op,
            term_ty,
            accumulator_ty,
            buckets: buckets.clone(),
            coordinates: coordinate_locals.clone(),
            coordinate_ty: integer,
            group: group.cloned(),
            group_dims: group_dims.clone(),
            error_tensor,
            element: Some(element_local),
        };
        let previous = self.reduction_fold.replace(fold);
        let after = self.fresh_block(expr.span);
        self.lower_reduction_axis(
            &source_place,
            &bounds,
            &coordinate_locals,
            element_local,
            element,
            matrix,
            dimensions.as_deref(),
            body,
            0,
            after,
            expr.span,
        )?;
        self.switch_to(after);
        self.reduction_fold = previous;
        self.bindings = saved_bindings;
        if group.is_some() {
            if matches!(op, Op::Max | Op::Min) {
                for bucket in &buckets {
                    self.reduction_empty_identity(bucket, identity.clone()?, accumulator_ty, expr.span);
                }
            }
            return self.reduction_tensor(&buckets, result_ty, accumulator_ty, &group_dims, expr.span);
        }
        let bucket = &buckets[0];
        let result = if matches!(op, Op::ArgMax | Op::ArgMin) {
            if rank == 1 {
                MirOperand::Temp(bucket.coordinates[0])
            } else {
                let inner = match self.types.get(result_ty.semantic_id()) {
                    Type::Option(inner) => *inner,
                    _ => result_ty.semantic_id(),
                };
                self.construct_temp(
                    MirAggregateKind::Tuple,
                    MirAggregateFields::Ordered(
                        bucket
                            .coordinates
                            .iter()
                            .map(|t| MirAggregateItem::Operand(MirOperand::Temp(*t)))
                            .collect(),
                    ),
                    MirType::semantic(inner),
                    expr.span,
                )
            }
        } else {
            MirOperand::Temp(bucket.accumulator)
        };
        if matches!(op, Op::Max | Op::Min | Op::ArgMax | Op::ArgMin | Op::First) {
            // A first reduction is optional even over a statically non-empty
            // source: no element need satisfy the predicate.
            if op != Op::First
                && identity.is_none()
                && dimensions
                    .as_ref()
                    .is_some_and(|d| !d.is_empty() && d.iter().all(|n| *n > 0))
            {
                return Some(result);
            }
            let destination = self.push_temp(result_ty, expr.span);
            let nonempty = self.fresh_block(expr.span);
            let empty = self.fresh_block(expr.span);
            let join = self.fresh_block(expr.span);
            self.terminate_current(
                MirTerminatorKind::Branch {
                    condition: MirOperand::Temp(bucket.seen),
                    then_block: nonempty,
                    else_block: empty,
                },
                expr.span,
            );
            self.switch_to(nonempty);
            let result = if matches!(self.types.get(result_ty.semantic_id()), Type::Option(_)) {
                self.assign_temp(MirValueKind::Option(MirOptionOp::Some(result)), result_ty, expr.span)
            } else {
                result
            };
            self.assign(MirPlace::temp(destination), result, result_ty, expr.span);
            self.terminate_current(MirTerminatorKind::Goto(join), expr.span);
            self.switch_to(empty);
            let empty_result = identity.unwrap_or(MirOperand::Constant(MirConstant::Nil));
            self.assign(MirPlace::temp(destination), empty_result, result_ty, expr.span);
            self.terminate_current(MirTerminatorKind::Goto(join), expr.span);
            self.switch_to(join);
            return Some(MirOperand::Temp(destination));
        }
        Some(result)
    }

    fn reduction_tensor(
        &mut self,
        buckets: &[ReductionBucket],
        result_ty: MirType,
        element_ty: MirType,
        dims: &[usize],
        span: Span,
    ) -> Option<MirOperand> {
        let flat_ty =
            MirType::semantic(self.require_array(element_ty.semantic_id(), span, "reduction tensor element list")?);
        let integer = self.types.primitive(Primitive::Numerus);
        let dimensions_ty = MirType::semantic(self.require_array(integer, span, "reduction tensor dimension list")?);
        let flat = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(
                buckets
                    .iter()
                    .map(|b| MirAggregateItem::Operand(MirOperand::Temp(b.accumulator)))
                    .collect(),
            ),
            flat_ty,
            span,
        );
        let dimensions = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(
                dims.iter()
                    .map(|dim| MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(*dim as i64))))
                    .collect(),
            ),
            dimensions_ty,
            span,
        );
        let seed = self.construct_temp(
            MirAggregateKind::Tensor,
            MirAggregateFields::Ordered(Vec::new()),
            result_ty,
            span,
        );
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
            vec![seed, flat, dimensions],
            result_ty,
            span,
        ))
    }

    fn reduction_empty_identity(&mut self, bucket: &ReductionBucket, identity: MirOperand, ty: MirType, span: Span) {
        let empty = self.fresh_block(span);
        let join = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: MirOperand::Temp(bucket.seen), then_block: join, else_block: empty },
            span,
        );
        self.switch_to(empty);
        self.assign(MirPlace::temp(bucket.accumulator), identity, ty, span);
        self.terminate_current(MirTerminatorKind::Goto(join), span);
        self.switch_to(join);
    }

    /// Nested loops visit every coordinate once in row-major order.
    fn lower_reduction_axis(
        &mut self,
        source: &MirPlace,
        bounds: &[MirOperand],
        coordinates: &[MirLocalId],
        element_local: MirLocalId,
        element: TypeId,
        matrix: bool,
        dimensions: Option<&[i64]>,
        body: Option<&HirBlock>,
        axis: usize,
        continuation: MirBlockId,
        span: Span,
    ) -> Option<()> {
        if axis >= coordinates.len() {
            self.errors
                .push(MirError::unsupported(span, "reducta_rank_zero_not_lowered"));
            return None;
        }
        let coordinate = coordinates[axis];
        let integer = self.locals.iter().find(|local| local.id == coordinate)?.ty;
        let boolean = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        self.assign(
            MirPlace::local(coordinate),
            MirOperand::Constant(MirConstant::Int(0)),
            integer,
            span,
        );
        let condition_block = self.fresh_block(span);
        let body_block = self.fresh_block(span);
        let increment = self.fresh_block(span);
        let after = self.fresh_block(span);
        self.terminate_current(MirTerminatorKind::Goto(condition_block), span);
        self.switch_to(condition_block);
        let condition = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(coordinate)),
            bounds[axis].clone(),
            boolean,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: body_block, else_block: after },
            span,
        );
        self.switch_to(body_block);
        if axis + 1 < coordinates.len() {
            self.lower_reduction_axis(
                source,
                bounds,
                coordinates,
                element_local,
                element,
                matrix,
                dimensions,
                body,
                axis + 1,
                increment,
                span,
            )?;
        } else {
            let element_ty = MirType::semantic(element);
            if matrix {
                let dimensions = dimensions?;
                let columns = u8::try_from(dimensions[1]).ok()?;
                let rows = u8::try_from(dimensions[0]).ok()?;
                let offset = self.binary_temp(
                    MirBinOp::Mul,
                    MirOperand::Place(MirPlace::local(coordinates[0])),
                    MirOperand::Constant(MirConstant::Int(i64::from(columns))),
                    integer,
                    span,
                );
                let offset = self.binary_temp(
                    MirBinOp::Add,
                    offset,
                    MirOperand::Place(MirPlace::local(coordinates[1])),
                    integer,
                    span,
                );
                let ready = self.fresh_block(span);
                let cases = (0..u16::from(rows) * u16::from(columns))
                    .map(|offset| MirSwitchCase {
                        value: MirConstant::Int(i64::from(offset)),
                        target: self.fresh_block(span),
                    })
                    .collect::<Vec<_>>();
                let blocks = cases.iter().map(|c| c.target).collect::<Vec<_>>();
                self.terminate_current(MirTerminatorKind::Switch { value: offset, cases, default: increment }, span);
                for (offset, block) in blocks.into_iter().enumerate() {
                    self.switch_to(block);
                    let mut cell = source.clone();
                    cell.projections.push(MirProjection::MatrixCell {
                        row: (offset / usize::from(columns)) as u8,
                        column: (offset % usize::from(columns)) as u8,
                    });
                    self.assign(MirPlace::local(element_local), MirOperand::Place(cell), element_ty, span);
                    self.terminate_current(MirTerminatorKind::Goto(ready), span);
                }
                self.switch_to(ready);
            } else {
                let indices = self.reduction_indices(
                    coordinates
                        .iter()
                        .map(|c| MirOperand::Place(MirPlace::local(*c)))
                        .collect(),
                    integer.semantic_id(),
                    span,
                )?;
                let option_ty = MirType::semantic(self.types.find_equal(&Type::Option(element))?);
                let value = self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::TensorGet),
                    vec![MirOperand::Place(source.clone()), indices],
                    option_ty,
                    span,
                );
                let value = self.assign_temp(
                    MirValueKind::Option(MirOptionOp::Unwrap { value, mode: MirOptionUnwrapMode::Assume }),
                    element_ty,
                    span,
                );
                self.assign(MirPlace::local(element_local), value, element_ty, span);
            }
            self.breakables.push(BreakableContext {
                perge_target: Some(increment),
                rumpe_target: after,
                rumpe_used: false,
            });
            if let Some(body) = body {
                self.visit_block(body);
            } else {
                self.fold_reduction_value(MirOperand::Place(MirPlace::local(element_local)), span);
            }
            self.breakables.pop();
            self.terminate_open_current(MirTerminatorKind::Goto(increment), span);
        }
        self.switch_to(increment);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(coordinate)),
            MirOperand::Constant(MirConstant::Int(1)),
            integer,
            span,
        );
        self.assign(MirPlace::local(coordinate), next, integer, span);
        self.terminate_current(MirTerminatorKind::Goto(condition_block), span);
        self.switch_to(after);
        self.terminate_current(MirTerminatorKind::Goto(continuation), span);
        Some(())
    }

    /// The interned `lista<element>` row, or a named lowering error. A bare
    /// `?` on `find_array` returns `None` with no diagnostic and the
    /// enclosing statement silently vanishes from MIR.
    pub fn require_array(&mut self, element: TypeId, span: Span, what: &str) -> Option<TypeId> {
        let found = self.types.find_array(element);
        if found.is_none() {
            self.errors.push(MirError::missing_type(span, what));
        }
        found
    }

    fn reduction_indices(&mut self, indices: Vec<MirOperand>, integer: TypeId, span: Span) -> Option<MirOperand> {
        let ty = MirType::semantic(self.require_array(integer, span, "reduction index list")?);
        Some(self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(indices.into_iter().map(MirAggregateItem::Operand).collect()),
            ty,
            span,
        ))
    }

    pub(super) fn fold_reduction_value(&mut self, value: MirOperand, span: Span) {
        let Some(fold) = self.reduction_fold.clone() else {
            return;
        };
        if let Some(group) = &fold.group {
            // A group is one key (`pro k`) or a coordinate list (`pro [k0, k1, ..]`);
            // each key is evaluated once. A bucket is selected when every key equals
            // its row-major coordinate, so an out-of-range key never aliases another
            // bucket and falls through to the tensor write failure below.
            let mut keys = Vec::new();
            for key_expr in radix_hir::reduction_group_keys(group) {
                let Some(key) = self.lower_expr_value(key_expr) else {
                    return;
                };
                keys.push(key);
            }
            let boolean = MirType::semantic(self.types.primitive(Primitive::Bivalens));
            let join = self.fresh_block(span);
            for (index, bucket) in fold.buckets.iter().enumerate() {
                let selected = self.fresh_block(span);
                let next = self.fresh_block(span);
                let mut remainder = index;
                let mut coordinates = vec![0usize; keys.len()];
                for (axis, key_dim) in fold.group_dims.iter().enumerate().take(keys.len()).rev() {
                    coordinates[axis] = remainder % (*key_dim).max(1);
                    remainder /= (*key_dim).max(1);
                }
                let mut condition: Option<MirOperand> = None;
                for (key, coordinate) in keys.iter().zip(coordinates) {
                    let matches = self.binary_temp(
                        MirBinOp::Eq,
                        key.clone(),
                        MirOperand::Constant(MirConstant::Int(coordinate as i64)),
                        boolean,
                        span,
                    );
                    condition = Some(match condition {
                        Some(previous) => self.binary_temp(MirBinOp::And, previous, matches, boolean, span),
                        None => matches,
                    });
                }
                let Some(condition) = condition else {
                    return;
                };
                self.terminate_current(
                    MirTerminatorKind::Branch { condition, then_block: selected, else_block: next },
                    span,
                );
                self.switch_to(selected);
                self.combine_reduction_bucket(&fold, bucket, value.clone(), span);
                self.terminate_open_current(MirTerminatorKind::Goto(join), span);
                self.switch_to(next);
            }
            let Some(key_ty) = radix_hir::reduction_group_keys(group)
                .first()
                .and_then(|key| key.ty)
            else {
                return;
            };
            if let (Some(tensor), Some(indices)) =
                (fold.error_tensor.clone(), self.reduction_indices(keys, key_ty, span))
            {
                // Preserve the existing write failure, never reinterpret a bad key.
                self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::TensorSet),
                    vec![tensor, indices, MirOperand::Constant(MirConstant::Int(0))],
                    MirType::semantic(self.types.primitive(Primitive::Vacuum)),
                    span,
                );
            }
            self.terminate_open_current(MirTerminatorKind::Unreachable, span);
            self.switch_to(join);
        } else if let Some(bucket) = fold.buckets.first() {
            self.combine_reduction_bucket(&fold, bucket, value, span);
        }
    }

    pub(super) fn combine_reduction_bucket(
        &mut self,
        fold: &ReductionFoldContext,
        bucket: &ReductionBucket,
        value: MirOperand,
        span: Span,
    ) {
        use radix_hir::HirReductionOp as Op;
        let boolean = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        if matches!(fold.op, Op::Max | Op::Min | Op::ArgMax | Op::ArgMin) {
            let absent = self.binary_temp(
                MirBinOp::Eq,
                MirOperand::Temp(bucket.seen),
                MirOperand::Constant(MirConstant::Bool(false)),
                boolean,
                span,
            );
            let comparison = if matches!(fold.op, Op::Max | Op::ArgMax) {
                MirBinOp::Gt
            } else {
                MirBinOp::Lt
            };
            let better = self.binary_temp(comparison, value.clone(), MirOperand::Temp(bucket.score), boolean, span);
            let score_nan = self.binary_temp(
                MirBinOp::NotEq,
                MirOperand::Temp(bucket.score),
                MirOperand::Temp(bucket.score),
                boolean,
                span,
            );
            let value_number = self.binary_temp(MirBinOp::Eq, value.clone(), value.clone(), boolean, span);
            let replaces_nan = self.binary_temp(MirBinOp::And, score_nan, value_number, boolean, span);
            let better = self.binary_temp(MirBinOp::Or, better, replaces_nan, boolean, span);
            let take = self.binary_temp(MirBinOp::Or, absent, better, boolean, span);
            let selected = self.fresh_block(span);
            let join = self.fresh_block(span);
            self.terminate_current(
                MirTerminatorKind::Branch { condition: take, then_block: selected, else_block: join },
                span,
            );
            self.switch_to(selected);
            self.assign(MirPlace::temp(bucket.score), value.clone(), fold.term_ty, span);
            self.assign(MirPlace::temp(bucket.accumulator), value, fold.accumulator_ty, span);
            for (winner, coordinate) in bucket.coordinates.iter().zip(&fold.coordinates) {
                let integer = fold.coordinate_ty;
                self.assign(
                    MirPlace::temp(*winner),
                    MirOperand::Place(MirPlace::local(*coordinate)),
                    integer,
                    span,
                );
            }
            self.assign(
                MirPlace::temp(bucket.seen),
                MirOperand::Constant(MirConstant::Bool(true)),
                boolean,
                span,
            );
            self.terminate_current(MirTerminatorKind::Goto(join), span);
            self.switch_to(join);
            return;
        }
        if fold.op == Op::First {
            // First live element: once `seen`, later live elements never store.
            // The predicate already ran for this element (total evaluation).
            let unseen = self.binary_temp(
                MirBinOp::Eq,
                MirOperand::Temp(bucket.seen),
                MirOperand::Constant(MirConstant::Bool(false)),
                boolean,
                span,
            );
            let take = self.binary_temp(MirBinOp::And, value, unseen, boolean, span);
            let selected = self.fresh_block(span);
            let join = self.fresh_block(span);
            self.terminate_current(
                MirTerminatorKind::Branch { condition: take, then_block: selected, else_block: join },
                span,
            );
            self.switch_to(selected);
            if let Some(element) = fold.element {
                self.assign(
                    MirPlace::temp(bucket.accumulator),
                    MirOperand::Place(MirPlace::local(element)),
                    fold.accumulator_ty,
                    span,
                );
            }
            self.assign(
                MirPlace::temp(bucket.seen),
                MirOperand::Constant(MirConstant::Bool(true)),
                boolean,
                span,
            );
            self.terminate_current(MirTerminatorKind::Goto(join), span);
            self.switch_to(join);
            return;
        }
        if fold.op == Op::Count {
            let selected = self.fresh_block(span);
            let join = self.fresh_block(span);
            self.terminate_current(
                MirTerminatorKind::Branch { condition: value, then_block: selected, else_block: join },
                span,
            );
            self.switch_to(selected);
            let next = self.new_value(
                MirValueKind::Binary {
                    op: MirBinOp::Add,
                    lhs: MirOperand::Temp(bucket.accumulator),
                    rhs: MirOperand::Constant(MirConstant::Int(1)),
                },
                fold.accumulator_ty,
                span,
            );
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(bucket.accumulator), value: next },
                span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join), span);
            self.switch_to(join);
            return;
        }
        let op = match fold.op {
            Op::Sum => MirBinOp::Add,
            Op::Product => MirBinOp::Mul,
            Op::All => MirBinOp::And,
            Op::Any => MirBinOp::Or,
            _ => return,
        };
        let next = self.new_value(
            MirValueKind::Binary { op, lhs: MirOperand::Temp(bucket.accumulator), rhs: value },
            fold.accumulator_ty,
            span,
        );
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Assign { place: MirPlace::temp(bucket.accumulator), value: next },
            span,
        });
    }

    /// Frozen cyclic `filum` width for the distributed sum kernel-plan entry.
    const FILUM_LANE_WIDTH: u32 = 32;

    pub(super) fn lower_filum_reduce_expr(
        &mut self,
        request: FilumReduceLowering<'_>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        use radix_hir::HirReductionOp as Op;
        let FilumReduceLowering { op, form, source, group, coordinates, filum, binding, body, identity } = request;
        let Some(reduce_op) = mir_reduce_op(op) else {
            // The selected element rides a `T ∪ none` result and an earliest-lane
            // selection no device reduce recipe carries yet: a named decline,
            // never a quiet route through another reducer.
            self.errors.push(MirError {
                message: "a `thread` first reduction has no device lowering yet".to_owned(),
                issue: super::lane_form::FIRST_REDUCE_DEVICE_ISSUE.to_owned(),
                span: expr.span,
                definition_span: None,
            });
            return None;
        };
        let grid = form == MirFilumForm::Grid;
        let Some(result_ty) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "filum reduce result type"));
            return None;
        };
        let result_mir_ty = MirType::semantic(result_ty);
        let (group_payload, group_args) = match group {
            Some(group) => {
                let (group_payload, group_args) =
                    self.lower_filum_group_keys(group, source, coordinates, result_ty, expr.span)?;
                (Some(group_payload), group_args)
            }
            None => (None, Vec::new()),
        };
        match self.types.get(result_ty) {
            Type::Primitive(Primitive::Fractus | Primitive::Numerus)
            | Type::SizedNumeric(Primitive::Fractus | Primitive::Numerus, _) => {}
            Type::Primitive(Primitive::Bivalens) if grid => {}
            Type::Tensor(_, _) if grid && group.is_some() && matches!(op, Op::Sum | Op::Count) => {}
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "filum reduce before numeric scalar MIR lowering",
                ));
                return None;
            }
        }
        let Some(source_ty) = source.ty.map(|ty| self.types.peel_access_marker(ty).1) else {
            self.errors
                .push(MirError::missing_type(source.span, "filum reduce source type"));
            return None;
        };
        let element_ty = if let Type::Tensor(element, _) = self.normalized_type(source_ty) {
            *element
        } else {
            self.errors.push(MirError::unsupported(
                source.span,
                "filum reduce source before tensor MIR lowering",
            ));
            return None;
        };
        // The source operand lowers into the real flow (ahead of the
        // capture) so the declaration's argument is a live kernel value.
        let source_operand = self.lower_expr_value(source)?;
        let source_operand = self.materialize_operand_place(source_operand, MirType::semantic(source_ty), source.span);
        let mut args = vec![MirOperand::Place(source_operand.clone())];
        if grid {
            args.extend(self.lane_form_extents(&source_operand, source_ty, coordinates.len(), source.span)?);
            if let Some(identity) = identity {
                args.push(self.lower_expr_value(identity)?);
            }
            if let Some(body) = body {
                return self.lower_filum_term_function(FilumTermFunction {
                    op: reduce_op,
                    coordinates,
                    filum,
                    binding,
                    body,
                    element_ty,
                    args,
                    group: group_payload,
                    group_args,
                    result_mir_ty,
                    expr,
                });
            }
        }

        // Per-element binders: coordinates (numerus, one per axis) and the
        // term binder (the element type). They live as real locals so the
        // captured term-body statements resolve against them.
        let numerus_ty = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let mut saved_bindings: Vec<(DefId, Option<LocalBinding>)> = Vec::new();
        let mut coordinate_facts = Vec::with_capacity(coordinates.len());
        for (def_id, name) in coordinates {
            let numerus_ty =
                MirType::semantic(reduction_coordinate_type(body, None, *def_id).unwrap_or(numerus_ty.semantic_id()));
            let local = self.next_local_id();
            self.locals.push(MirLocalDecl {
                id: local,
                name: Some(*name),
                ty: numerus_ty,
                mutable: false,
                span: expr.span,
            });
            saved_bindings.push((
                *def_id,
                self.bindings
                    .insert(*def_id, LocalBinding { local, ty: numerus_ty }),
            ));
            coordinate_facts.push(MirFilumCoordinate { local, name: Some(*name) });
        }
        let binder_local = self.next_local_id();
        let binder_ty = MirType::semantic(element_ty);
        let binder_name = binding.map(|(_, name)| *name);
        self.locals.push(MirLocalDecl {
            id: binder_local,
            name: binder_name,
            ty: binder_ty,
            mutable: false,
            span: expr.span,
        });
        if let Some((def_id, _)) = binding {
            saved_bindings.push((
                *def_id,
                self.bindings
                    .insert(*def_id, LocalBinding { local: binder_local, ty: binder_ty }),
            ));
        }

        // The term type: the sum's own result for the declared form; the
        // valued `redde` type (or the element when there is no body) for the
        // grid form.
        let term_mir_ty = if grid {
            match body {
                Some(body) => filum_term_type(body)?,
                None => binder_ty,
            }
        } else {
            result_mir_ty
        };
        let (term_body, term_result) = match body {
            Some(body) => {
                let term_result = self.next_local_id();
                self.locals.push(MirLocalDecl {
                    id: term_result,
                    name: None,
                    ty: term_mir_ty,
                    mutable: true,
                    span: expr.span,
                });
                (
                    self.capture_filum_term_body(body, term_result, term_mir_ty, expr.span),
                    term_result,
                )
            }
            None => (Vec::new(), binder_local),
        };
        for (def_id, binding) in saved_bindings {
            match binding {
                Some(previous) => {
                    self.bindings.insert(def_id, previous);
                }
                None => {
                    self.bindings.remove(&def_id);
                }
            }
        }

        let declaration = MirFilumReduceOp {
            op: reduce_op,
            form,
            filum_binder: Some(filum.1),
            term_binder: binder_local,
            term_binder_name: binder_name,
            coordinates: coordinate_facts,
            lane_width: if grid {
                GRID_REDUCE_WORKGROUP
            } else {
                Self::FILUM_LANE_WIDTH
            },
            term_body,
            term_captures: None,
            term_result,
            term_ty: term_mir_ty,
            group: group_payload,
        };
        args.extend(group_args);
        let call = self.runtime_call_value(MirIntrinsic::FilumReduce(declaration), args, result_mir_ty, expr.span);
        let result = self.materialize_operand_place(call, result_mir_ty, expr.span);
        Some(MirOperand::Place(result))
    }

    /// Lower each grouped key as a typed scalar helper. The key expressions
    /// run once per source element and keep each result axis distinct until
    /// the device checks its own bound.
    fn lower_filum_group_keys(
        &mut self,
        group: &HirExpression,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        result_ty: TypeId,
        span: Span,
    ) -> Option<(MirFilumGroupOp, Vec<MirOperand>)> {
        let (result_element_ty, shape) = if let Type::Tensor(element, shape) = self.normalized_type(result_ty) {
            (*element, *shape)
        } else {
            self.errors.push(MirError::unsupported(
                span,
                "grouped grid reduction result before tensor lowering",
            ));
            return None;
        };
        let Some(literal_dimensions) = i64s_from_literal_index(shape, self.types) else {
            self.errors.push(MirError::unsupported(
                span,
                "grouped grid reduction requires literal result extents",
            ));
            return None;
        };
        let Ok(dimensions) = literal_dimensions
            .into_iter()
            .map(u64::try_from)
            .collect::<Result<Vec<_>, _>>()
        else {
            self.errors.push(MirError::unsupported(
                span,
                "grouped grid reduction result extent is outside u64",
            ));
            return None;
        };
        let keys = radix_hir::reduction_group_keys(group);
        if dimensions.len() != keys.len() {
            self.errors
                .push(MirError::unsupported(span, "grouped grid reduction shape/key rank mismatch"));
            return None;
        }
        if !grouped_key_tensor_reads_are_proven(self.types, source, coordinates, group) {
            self.errors.push(MirError {
                message: "a grouped reduction key tensor read is not proven in range for every source coordinate"
                    .to_owned(),
                issue: "grouped_grid_reduction_key_index_unproven".to_owned(),
                span,
                definition_span: None,
            });
            return None;
        }
        let excluded = coordinates.iter().map(|(def_id, _)| *def_id).collect();
        let captures = self.collect_expression_captures(excluded, group);
        if captures
            .iter()
            .any(|capture| capture.mode == MirCaptureMode::Shared)
        {
            self.errors
                .push(MirError::unsupported(span, "grouped grid reduction key captures shared state"));
            return None;
        }
        let mut args = Vec::new();
        let mut key_plans = Vec::with_capacity(keys.len());
        for key in keys {
            let key_ty = MirType::semantic(key.ty?);
            let function = self.lower_filum_group_key_function(key, group, coordinates, &captures, key_ty, span)?;
            args.extend(
                captures
                    .iter()
                    .map(|capture| MirOperand::Place(capture.source_place.clone())),
            );
            args.push(MirOperand::Constant(MirConstant::Function(function)));
            key_plans.push(MirFilumGroupKey { captures: u32::try_from(captures.len()).ok()?, key_ty });
        }
        Some((
            MirFilumGroupOp { dimensions, result_element_ty: MirType::semantic(result_element_ty), keys: key_plans },
            args,
        ))
    }

    fn lower_filum_group_key_function(
        &mut self,
        key: &HirExpression,
        group: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        captures: &[MirClosureCapture],
        key_ty: MirType,
        span: Span,
    ) -> Option<MirFunctionId> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            None,
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        // Admission above proved each tensor read against the source
        // coordinates. Carry that fact on the existing checked-index carrier;
        // the helper itself has no lane from which to rediscover it.
        struct ProvenReads<'a> {
            types: &'a TypeTable,
            ids: Vec<radix_hir::HirId>,
        }
        impl HirVisitor for ProvenReads<'_> {
            fn visit_expr(&mut self, expr: &HirExpression) {
                if let HirExpressionKind::Index(receiver, _) = &expr.kind
                    && receiver.ty.is_some_and(|ty| {
                        matches!(self.types.get(self.types.peel_access_marker(ty).1), Type::Tensor(..))
                    })
                {
                    self.ids.push(expr.id);
                }
                radix_hir::visit::walk_expr(self, expr);
            }
        }
        let mut reads = ProvenReads { types: self.types, ids: Vec::new() };
        reads.visit_expr(key);
        Arc::make_mut(&mut nested.context.proven_indexes).extend(reads.ids);
        for capture in captures {
            nested.add_param(
                capture.source,
                capture.name,
                capture.ty.semantic_id(),
                HirParamMode::Owned,
                span,
            );
        }
        for (def_id, name) in coordinates {
            let ty = reduction_coordinate_type(None, Some(group), *def_id)
                .unwrap_or_else(|| self.types.primitive(Primitive::Numerus));
            nested.add_param(*def_id, *name, ty, HirParamMode::Owned, span);
        }
        let entry = nested.fresh_block(span);
        nested.switch_to(entry);
        nested.emit_copy_param_entries();
        let Some(value) = nested.lower_expr_value(key) else {
            self.errors.extend(nested.errors);
            return None;
        };
        nested.terminate_current(MirTerminatorKind::Return(Some(value)), span);
        let blocks = nested.finish_blocks();
        let nested_synthetics = nested.take_synthetic_functions();
        let nested_environments = nested.take_closure_environments();
        let nested_metadata = nested.take_device_function_metadata();
        let nested_error_count = nested.errors.len();
        self.errors.extend(nested.errors);
        if nested_error_count > 0 {
            return None;
        }
        self.synthetic_functions.push(MirFunction {
            id,
            source: None,
            name: None,
            params: nested.params,
            locals: nested.locals,
            temps: nested.temps,
            blocks,
            return_ty: key_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        self.synthetic_functions.extend(nested_synthetics);
        self.closure_environments.extend(nested_environments);
        self.device_function_metadata.extend(nested_metadata);
        self.device_function_metadata.insert(
            id,
            MirDeviceFunctionMetadata {
                identity: MirSyntheticFunctionIdentity {
                    parent: self.parent_function_id,
                    ordinal: id.0 - self.parent_function_id.0,
                },
                role: MirDeviceRole::Kernel,
                shader_stage: MirKernelShaderStage::Compute,
                visibility: MirFunctionVisibility::Private,
                origin: MirFunctionOrigin::SyntheticKernelClosure,
                lane_domain_local: None,
                refused_backends: Default::default(),
                tier: radix_mir::device::KernelTier::Default,
            },
        );
        Some(id)
    }

    /// Lower a grid-form term body into its own MIR function (C4-8 U5): the
    /// parameters are the values the term captures from before the node, the
    /// coordinate binders, and the element binder; it returns the term value.
    /// The reduction node names the function through its last argument.
    fn lower_filum_term_function(&mut self, request: FilumTermFunction<'_>) -> Option<MirOperand> {
        let FilumTermFunction {
            op,
            coordinates,
            filum,
            binding,
            body,
            element_ty,
            mut args,
            group,
            group_args,
            result_mir_ty,
            expr,
        } = request;
        let span = expr.span;
        let term_ty = filum_term_type(body)?;
        let mut excluded = FxHashSet::default();
        excluded.extend(coordinates.iter().map(|(def_id, _)| *def_id));
        excluded.extend(binding.map(|(def_id, _)| *def_id));
        let captures = self.collect_block_captures(excluded, body);
        if captures
            .iter()
            .any(|capture| capture.mode == MirCaptureMode::Shared)
        {
            self.errors
                .push(MirError::unsupported(span, "filum reduce term capturing a shared binding"));
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            None,
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        for capture in &captures {
            nested.add_param(
                capture.source,
                capture.name,
                capture.ty.semantic_id(),
                HirParamMode::Owned,
                span,
            );
        }
        let numerus = self.types.primitive(Primitive::Numerus);
        let coordinate_start = nested.params.len();
        for (def_id, name) in coordinates {
            nested.add_param(*def_id, *name, numerus, HirParamMode::Owned, span);
        }
        let coordinate_facts: Vec<MirFilumCoordinate> = coordinates
            .iter()
            .enumerate()
            .map(|(axis, (_, name))| MirFilumCoordinate {
                local: nested.params[coordinate_start + axis].local,
                name: Some(*name),
            })
            .collect();
        let binder_name = binding.map(|(_, name)| *name);
        if let Some((def_id, name)) = binding {
            nested.add_param(*def_id, *name, element_ty, HirParamMode::Owned, span);
        } else {
            let local = nested.next_local_id();
            let ty = MirType::semantic(element_ty);
            nested
                .params
                .push(MirParam { local, name: None, ty, mode: MirParamMode::Owned, span });
            nested
                .locals
                .push(MirLocalDecl { id: local, name: None, ty, mutable: false, span });
        }
        let binder_local = nested.params.last()?.local;
        let blocks = nested.lower_body(body);
        let nested_synthetics = nested.take_synthetic_functions();
        let nested_environments = nested.take_closure_environments();
        let nested_metadata = nested.take_device_function_metadata();
        let nested_error_count = nested.errors.len();
        self.errors.extend(nested.errors);
        if nested_error_count > 0 {
            return None;
        }
        // Mutable-borrow note: the captures' source places are read from the
        // enclosing flow as the node's operands.
        args.extend(
            captures
                .iter()
                .map(|capture| MirOperand::Place(capture.source_place.clone())),
        );
        args.push(MirOperand::Constant(MirConstant::Function(id)));
        args.extend(group_args);
        self.synthetic_functions.push(MirFunction {
            id,
            source: None,
            name: None,
            params: nested.params,
            locals: nested.locals,
            temps: nested.temps,
            blocks,
            return_ty: term_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        self.synthetic_functions.extend(nested_synthetics);
        self.closure_environments.extend(nested_environments);
        self.device_function_metadata.extend(nested_metadata);
        // A device-side private helper by carrier fact: composition inlines
        // the helper calls inside the term, and the emitter owns it as the
        // reduction's term function. Never a launch entry (no shader stage).
        self.device_function_metadata.insert(
            id,
            MirDeviceFunctionMetadata {
                identity: MirSyntheticFunctionIdentity {
                    parent: self.parent_function_id,
                    ordinal: id.0 - self.parent_function_id.0,
                },
                role: MirDeviceRole::Kernel,
                shader_stage: MirKernelShaderStage::Compute,
                visibility: MirFunctionVisibility::Private,
                origin: MirFunctionOrigin::SyntheticKernelClosure,
                lane_domain_local: None,
                refused_backends: Default::default(),
                tier: radix_mir::device::KernelTier::Default,
            },
        );
        let declaration = MirFilumReduceOp {
            op,
            form: MirFilumForm::Grid,
            filum_binder: Some(filum.1),
            term_binder: binder_local,
            term_binder_name: binder_name,
            coordinates: coordinate_facts,
            lane_width: GRID_REDUCE_WORKGROUP,
            term_body: Vec::new(),
            term_captures: Some(u32::try_from(captures.len()).ok()?),
            term_result: binder_local,
            term_ty,
            group,
        };
        let call = self.runtime_call_value(MirIntrinsic::FilumReduce(declaration), args, result_mir_ty, span);
        let result = self.materialize_operand_place(call, result_mir_ty, span);
        Some(MirOperand::Place(result))
    }

    /// Capture a filum term body into scratch blocks: the `redde` arm stores
    /// the term value into `term_result` and jumps to the capture exit. The
    /// captured statements are returned; the scratch blocks are dropped.
    fn capture_filum_term_body(
        &mut self,
        body: &HirBlock,
        term_result: MirLocalId,
        term_mir_ty: MirType,
        span: Span,
    ) -> Vec<MirStatement> {
        let blocks_mark = self.blocks.len();
        let saved_current = self.current;
        let capture_entry = self.fresh_block(span);
        let capture_exit = self.fresh_block(span);
        self.current = Some(capture_entry);
        self.breakables.push(BreakableContext {
            perge_target: Some(capture_exit),
            rumpe_target: capture_exit,
            rumpe_used: false,
        });
        let previous_capture = self
            .filum_capture
            .replace(FilumCapture { term_result, ty: term_mir_ty });
        self.visit_block(body);
        self.filum_capture = previous_capture;
        self.breakables.pop();
        if !self.blocks[blocks_mark..]
            .iter()
            .any(|block| block.terminator.is_some())
        {
            self.errors
                .push(MirError::unsupported(span, "filum reduce term body without a valued redde"));
        }
        let term_body: Vec<MirStatement> = self.blocks[blocks_mark..]
            .iter()
            .flat_map(|block| block.statements.iter().cloned())
            .collect();
        self.blocks.truncate(blocks_mark);
        self.current = saved_current;
        term_body
    }

    /// Lower the literal `elige` subset into a MIR switch.
    ///
    /// HIR represents `elige` as `Discerne` with one scrutinee and literal
    /// patterns. Destructuring, enum variants, guards, multi-subject matching,
    /// and value-producing matches remain explicit later-phase work.
    pub(super) fn lower_discerne_expr(
        &mut self,
        scrutinees: &[HirExpression],
        arms: &[HirCasuArm],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if arms
            .iter()
            .flat_map(|arm| arm.patterns.iter())
            .any(|pattern| matches!(pattern, HirPattern::TypeTest { .. }))
        {
            let [scrutinee] = scrutinees else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "multi-subject type-test discerne before MIR lowering",
                ));
                return None;
            };
            return self.lower_type_test_discerne_expr(scrutinee, arms, expr);
        }
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "value-producing discerne before switch MIR lowering",
            ));
            return None;
        }
        let [scrutinee] = scrutinees else {
            return self.lower_multi_subject_discerne(scrutinees, arms, expr);
        };

        let value = self.lower_expr_value(scrutinee)?;
        let join_id = self.fresh_block(expr.span);
        let mut cases = Vec::new();
        let mut bodies = Vec::new();
        let mut default_id = join_id;
        let mut default_body = None;

        for arm in arms {
            let pattern = self.discerne_switch_arm_pattern(arm)?;
            match pattern {
                HirPattern::Literal(_) | HirPattern::NegatedLiteral(_) => {
                    let scrutinee_ty = self.expr_ty(scrutinee).map(radix_mir::MirType::semantic_id);
                    let constant = self.literal_pattern_switch_constant(pattern, scrutinee_ty, arm.span)?;
                    let target = self.fresh_block(arm.span);
                    cases.push(MirSwitchCase { value: constant, target });
                    bodies.push((target, &arm.body, arm.span));
                }
                // `casu 1 aut 2`: every alternative is a case value of the same switch
                // target, so one arm body serves them all.
                HirPattern::Or(alternatives) if is_literal_alternatives(alternatives) => {
                    let scrutinee_ty = self.expr_ty(scrutinee).map(radix_mir::MirType::semantic_id);
                    let target = self.fresh_block(arm.span);
                    for alternative in alternatives {
                        let constant = self.literal_pattern_switch_constant(alternative, scrutinee_ty, arm.span)?;
                        cases.push(MirSwitchCase { value: constant, target });
                    }
                    bodies.push((target, &arm.body, arm.span));
                }
                HirPattern::Wildcard if default_body.is_none() => {
                    default_id = self.fresh_block(arm.span);
                    default_body = Some((&arm.body, arm.span));
                }
                HirPattern::Wildcard => {
                    self.errors.push(MirError::unsupported(
                        arm.span,
                        "multiple discerne defaults before switch MIR lowering",
                    ));
                    return None;
                }
                _ => {
                    return self.lower_unit_variant_discerne(value, scrutinee, arms, expr);
                }
            }
        }

        // A discerne whose arms are all wildcards produces a switch with no
        // literal cases; MIR validation rejects empty switches. Fold it into a
        // direct jump to the default (the only arm body), skipping the switch.
        if cases.is_empty() {
            let (body, span) = default_body?;
            self.terminate_current(MirTerminatorKind::Goto(default_id), expr.span);
            self.switch_to(default_id);
            self.lower_expr_value(body);
            let reaches_join = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
            if reaches_join {
                self.switch_to(join_id);
            } else {
                self.seal_unreachable(join_id, expr.span);
            }
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        // A closed-set switch with no default (`casu verum` / `casu falsum`)
        // has no value that misses every case: its default edge is dead, so
        // it must not fall through to the join as a no-value return.
        let default_is_dead = default_body.is_none() && self.discerne_is_total(scrutinees, arms);
        if default_is_dead {
            default_id = self.fresh_block(expr.span);
            self.seal_unreachable(default_id, expr.span);
        }

        self.terminate_current(MirTerminatorKind::Switch { value, cases, default: default_id }, expr.span);

        let mut reaches_join = default_body.is_none() && !default_is_dead;

        for (target, body, span) in bodies {
            self.switch_to(target);
            self.lower_expr_value(body);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
        }

        if let Some((body, span)) = default_body {
            self.switch_to(default_id);
            self.lower_expr_value(body);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
        }

        if reaches_join {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, expr.span);
        }
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Whether semantic coverage proves every scrutinee value matches an arm,
    /// so a match with no default arm has no fallthrough edge.
    fn discerne_is_total(&self, scrutinees: &[HirExpression], arms: &[HirCasuArm]) -> bool {
        radix_semantic::passes::exhaustive::discerne_definitely_covered(
            scrutinees,
            arms,
            self.types,
            &self.context.enum_variants,
        )
    }

    fn lower_type_test_discerne_expr(
        &mut self,
        scrutinee: &HirExpression,
        arms: &[HirCasuArm],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let scrutinee_ty = scrutinee.ty?;
        let value = self.lower_expr_value(scrutinee)?;
        let scrutinee_place =
            self.materialize_operand_place(value.clone(), MirType::semantic(scrutinee_ty), scrutinee.span);
        let result_ty = expr.ty?;
        let all_arms_return = arms.iter().all(arm_body_returns);
        let destination = if all_arms_return || self.is_vacuum(MirType::semantic(result_ty)) {
            None
        } else {
            Some(MirPlace::temp(self.push_temp(MirType::semantic(result_ty), expr.span)))
        };
        let join_id = self.fresh_block(expr.span);
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let mut reaches_join = false;
        let mut has_default = false;

        for arm in arms {
            let [pattern] = arm.patterns.as_slice() else {
                self.errors.push(MirError::unsupported(
                    arm.span,
                    "multi-pattern type-test discerne arm before MIR lowering",
                ));
                return None;
            };
            let HirPattern::TypeTest { ty, binding, .. } = pattern else {
                if matches!(pattern, HirPattern::Wildcard | HirPattern::Binding(_, _)) {
                    has_default = true;
                    self.lower_type_test_arm_body(arm, destination.clone(), result_ty)?;
                    reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), arm.span);
                    break;
                }
                self.errors
                    .push(MirError::unsupported(arm.span, "non-type-test pattern in type-test discerne"));
                return None;
            };

            if self.types.equals(*ty, scrutinee_ty) {
                has_default = true;
                if let Some((def_id, name)) = binding {
                    self.bind_type_test_family(*def_id, *name, &scrutinee_place, scrutinee_ty, arm.span)?;
                }
                self.lower_type_test_arm_body(arm, destination.clone(), result_ty)?;
                reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), arm.span);
                break;
            }

            let condition = self.runtime_call_value(
                MirIntrinsic::TypeCheck(MirTypeCheckOp { positive: true, ty: MirType::semantic(*ty) }),
                vec![value.clone()],
                bool_ty,
                arm.span,
            );
            let then_id = self.fresh_block(arm.span);
            let else_id = self.fresh_block(arm.span);
            self.terminate_current(
                MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
                arm.span,
            );

            self.switch_to(then_id);
            if let Some((def_id, name)) = binding {
                self.bind_type_test_family(*def_id, *name, &scrutinee_place, *ty, arm.span)?;
            }
            self.lower_type_test_arm_body(arm, destination.clone(), result_ty)?;
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), arm.span);
            self.switch_to(else_id);
        }

        if !has_default {
            // Semantic exhaustiveness owns the missing-member diagnostic. Keep
            // the MIR CFG total for the runner by routing the statically
            // unreachable fallthrough to the join rather than emitting a
            // runner-unsupported `Unreachable` terminator.
            if self.discerne_is_total(std::slice::from_ref(scrutinee), arms) {
                // Every member is matched by an arm above, so the last probe's
                // else edge is dead; it must not reach the join.
                if let Some(current) = self.current {
                    self.seal_unreachable(current, expr.span);
                }
            } else {
                reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);
            }
        }
        if reaches_join {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, expr.span);
        }
        destination
            .map(MirOperand::Place)
            .or(Some(MirOperand::Constant(MirConstant::Unit)))
    }

    fn bind_type_test_family(
        &mut self,
        def_id: DefId,
        name: Symbol,
        scrutinee_place: &MirPlace,
        bound_ty: TypeId,
        span: Span,
    ) -> Option<()> {
        let binding_ty = MirType::semantic(bound_ty);
        let local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: local, name: Some(name), ty: binding_ty, mutable: false, span });
        self.bindings
            .insert(def_id, LocalBinding { local, ty: binding_ty });
        self.assign(
            MirPlace::local(local),
            MirOperand::Place(scrutinee_place.clone()),
            binding_ty,
            span,
        );
        Some(())
    }

    fn lower_type_test_arm_body(
        &mut self,
        arm: &HirCasuArm,
        destination: Option<MirPlace>,
        result_ty: TypeId,
    ) -> Option<()> {
        if let Some(destination) = destination {
            let HirExpressionKind::Block(block) = &arm.body.kind else {
                self.errors
                    .push(MirError::unsupported(arm.span, "type-test value arm is not a block"));
                return None;
            };
            self.lower_block_to_destination(block, destination, MirType::semantic(result_ty), arm.span)?;
        } else {
            self.lower_expr_value(&arm.body)?;
        }
        Some(())
    }

    fn lower_unit_variant_discerne(
        &mut self,
        value: MirOperand,
        scrutinee: &HirExpression,
        arms: &[HirCasuArm],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let scrutinee_ty = self.expr_ty(scrutinee)?;
        let scrutinee_place = self.materialize_operand_place(value.clone(), scrutinee_ty, scrutinee.span);
        let join_id = self.fresh_block(expr.span);
        let mut default_body = None;
        let mut reaches_join = false;

        for arm in arms {
            let pattern = self.discerne_switch_arm_pattern(arm)?;
            match pattern {
                HirPattern::Variant(variant, fields) => {
                    self.lower_unit_variant_arm(
                        *variant,
                        fields,
                        pattern,
                        &value,
                        &scrutinee_place,
                        scrutinee_ty,
                        join_id,
                        arm,
                        &mut reaches_join,
                    )?;
                }
                HirPattern::Alias(_, _, inner) if let HirPattern::Variant(variant, fields) = inner.as_ref() => {
                    self.lower_unit_variant_arm(
                        *variant,
                        fields,
                        pattern,
                        &value,
                        &scrutinee_place,
                        scrutinee_ty,
                        join_id,
                        arm,
                        &mut reaches_join,
                    )?;
                }
                HirPattern::Or(alternatives) if is_variant_alternatives(alternatives) => {
                    self.lower_variant_alternatives_arm(
                        alternatives,
                        &value,
                        &scrutinee_place,
                        scrutinee_ty,
                        join_id,
                        arm,
                        &mut reaches_join,
                    )?;
                }
                HirPattern::Wildcard if default_body.is_none() => {
                    default_body = Some((&arm.body, arm.span));
                    break;
                }
                HirPattern::Wildcard => {
                    self.errors.push(MirError::unsupported(
                        arm.span,
                        "multiple discerne defaults before switch MIR lowering",
                    ));
                    return None;
                }
                _ => {
                    self.errors.push(MirError::unsupported(
                        arm.span,
                        "non-literal discerne pattern before switch MIR lowering",
                    ));
                    return None;
                }
            }
        }

        if let Some((body, span)) = default_body {
            self.lower_expr_value(body);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
        } else if let Some(current) = self.current {
            self.seal_unreachable(current, expr.span);
        }

        if reaches_join {
            self.switch_to(join_id);
            Some(MirOperand::Constant(MirConstant::Unit))
        } else {
            self.seal_unreachable(join_id, expr.span);
            None
        }
    }

    /// Lower a multi-subject variant `discerne` (`match a and b { case X and Y }`)
    /// to a decision chain over the tuple of tags.
    ///
    /// Every subject is a closed set (semantic step M2), evaluated once and left
    /// in a place. Each arm probes its columns left to right: a miss falls to the
    /// next arm, a full hit binds the payloads and runs the body. Arm order and
    /// the all-wildcard default arm keep their source meaning.
    fn lower_multi_subject_discerne(
        &mut self,
        scrutinees: &[HirExpression],
        arms: &[HirCasuArm],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let mut subjects = Vec::with_capacity(scrutinees.len());
        for scrutinee in scrutinees {
            let ty = self.expr_ty(scrutinee)?;
            let value = self.lower_expr_value(scrutinee)?;
            let place = self.materialize_operand_place(value.clone(), ty, scrutinee.span);
            subjects.push((value, place, ty));
        }
        let join_id = self.fresh_block(expr.span);
        let mut default_body = None;
        let mut reaches_join = false;

        for arm in arms {
            if arm.patterns.len() != subjects.len() {
                self.errors.push(MirError::unsupported(
                    arm.span,
                    "multi-subject discerne arm pattern count differs from subject count",
                ));
                return None;
            }
            if arm
                .patterns
                .iter()
                .all(|pattern| matches!(pattern, HirPattern::Wildcard))
            {
                default_body = Some((&arm.body, arm.span));
                break;
            }
            let else_id = self.fresh_block(arm.span);
            let mut hits = Vec::new();
            for (pattern, (value, place, ty)) in arm.patterns.iter().zip(&subjects) {
                let variant_pattern = match pattern {
                    HirPattern::Alias(_, _, inner) => inner.as_ref(),
                    other => other,
                };
                match variant_pattern {
                    HirPattern::Wildcard => {
                        if matches!(pattern, HirPattern::Alias(..)) {
                            hits.push((pattern, place, *ty));
                        }
                    }
                    HirPattern::Variant(variant, fields) => {
                        let then_id = self.fresh_block(arm.span);
                        let condition = self.variant_probe_condition(*variant, fields, value, place, *ty, arm.span)?;
                        self.terminate_current(
                            MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
                            arm.span,
                        );
                        self.switch_to(then_id);
                        hits.push((pattern, place, *ty));
                    }
                    _ => {
                        self.errors.push(MirError::unsupported(
                            arm.span,
                            "non-variant pattern in multi-subject discerne before MIR lowering",
                        ));
                        return None;
                    }
                }
            }
            for (pattern, place, ty) in hits {
                if let HirPattern::Alias(alias_def, alias_name, inner) = pattern
                    && matches!(inner.as_ref(), HirPattern::Wildcard)
                {
                    let alias = self.pattern_binding_local(*alias_def, *alias_name, ty, arm.span);
                    self.assign(MirPlace::local(alias), MirOperand::Place(place.clone()), ty, arm.span);
                } else {
                    self.bind_variant_payload(pattern, place, ty, arm.span)?;
                }
            }
            self.lower_expr_value(&arm.body);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), arm.span);
            self.switch_to(else_id);
        }

        if let Some((body, span)) = default_body {
            self.lower_expr_value(body);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
        } else if let Some(current) = self.current {
            if self.discerne_is_total(scrutinees, arms) {
                self.seal_unreachable(current, expr.span);
            } else {
                reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);
            }
        }

        if reaches_join {
            self.switch_to(join_id);
            Some(MirOperand::Constant(MirConstant::Unit))
        } else {
            self.seal_unreachable(join_id, expr.span);
            None
        }
    }

    /// Shared prelude for the discerne switch loops: reject multi-pattern arms
    /// with their standing diagnostic, or yield the arm's single pattern.
    fn discerne_switch_arm_pattern<'a>(&mut self, arm: &'a HirCasuArm) -> Option<&'a HirPattern> {
        let [pattern] = arm.patterns.as_slice() else {
            self.errors.push(MirError::unsupported(
                arm.span,
                "multi-pattern discerne arm before switch MIR lowering",
            ));
            return None;
        };
        Some(pattern)
    }

    /// Lower one unit-variant arm: probe the scrutinee, branch on equality with
    /// the constructed variant, bind the payload, and route to the join block.
    ///
    /// `payload_pattern` is the whole arm pattern (a `Variant` or an `Alias`
    /// wrapping one) so an alias binding layers on before the variant payload.
    fn lower_unit_variant_arm(
        &mut self,
        variant: DefId,
        fields: &[HirPattern],
        payload_pattern: &HirPattern,
        value: &MirOperand,
        scrutinee_place: &MirPlace,
        scrutinee_ty: MirType,
        join_id: MirBlockId,
        arm: &HirCasuArm,
        reaches_join: &mut bool,
    ) -> Option<()> {
        let then_id = self.fresh_block(arm.span);
        let else_id = self.fresh_block(arm.span);
        let condition =
            self.variant_probe_condition(variant, fields, value, scrutinee_place, scrutinee_ty, arm.span)?;
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
            arm.span,
        );

        self.switch_to(then_id);
        self.bind_variant_payload(payload_pattern, scrutinee_place, scrutinee_ty, arm.span)?;
        self.lower_expr_value(&arm.body);
        *reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), arm.span);

        self.switch_to(else_id);
        Some(())
    }

    /// Lower an `aut` arm of unit-variant alternatives: each alternative
    /// probes the scrutinee in turn and a hit jumps to the one shared body.
    fn lower_variant_alternatives_arm(
        &mut self,
        alternatives: &[HirPattern],
        value: &MirOperand,
        scrutinee_place: &MirPlace,
        scrutinee_ty: MirType,
        join_id: MirBlockId,
        arm: &HirCasuArm,
        reaches_join: &mut bool,
    ) -> Option<()> {
        let body_id = self.fresh_block(arm.span);
        let else_id = self.fresh_block(arm.span);
        let mut stale = Vec::new();
        for alternative in alternatives {
            collect_pattern_binding_defs(alternative, &mut stale);
        }
        for def_id in stale {
            self.bindings.remove(&def_id);
        }
        let previous_shared = std::mem::replace(&mut self.shared_pattern_bindings, true);
        for (index, alternative) in alternatives.iter().enumerate() {
            let variant_pattern = match alternative {
                HirPattern::Alias(_, _, inner) => inner.as_ref(),
                other => other,
            };
            let HirPattern::Variant(variant, fields) = variant_pattern else {
                self.errors.push(MirError::unsupported(
                    arm.span,
                    "variant alternative that is not a variant pattern",
                ));
                return None;
            };
            let hit_id = self.fresh_block(arm.span);
            let miss_id = if index + 1 == alternatives.len() {
                else_id
            } else {
                self.fresh_block(arm.span)
            };
            let condition =
                self.variant_probe_condition(*variant, fields, value, scrutinee_place, scrutinee_ty, arm.span)?;
            self.terminate_current(
                MirTerminatorKind::Branch { condition, then_block: hit_id, else_block: miss_id },
                arm.span,
            );
            self.switch_to(hit_id);
            self.bind_variant_payload(alternative, scrutinee_place, scrutinee_ty, arm.span)?;
            self.terminate_current(MirTerminatorKind::Goto(body_id), arm.span);
            self.switch_to(miss_id);
        }
        self.shared_pattern_bindings = previous_shared;

        self.switch_to(body_id);
        self.lower_expr_value(&arm.body);
        *reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), arm.span);

        self.switch_to(else_id);
        Some(())
    }

    /// Branch condition for "the scrutinee is this variant".
    fn variant_probe_condition(
        &mut self,
        variant: DefId,
        fields: &[HirPattern],
        value: &MirOperand,
        scrutinee_place: &MirPlace,
        scrutinee_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let fields = if fields.is_empty()
            && self
                .context
                .variant_fields
                .get(&variant)
                .is_none_or(Vec::is_empty)
        {
            // A unit variant probes as the empty ordered aggregate: its
            // payload map is empty, so equality is variant identity.
            MirAggregateFields::Ordered(Vec::new())
        } else {
            // A bare arm on a payload variant (`casu Moveo ut totum`) probes
            // by re-reading every declared field from the scrutinee — the
            // same aggregate a wildcard-destructuring arm builds. Runner
            // equality on `EnumVariant` compares the payload map too, so an
            // empty aggregate would never match a populated scrutinee.
            self.variant_pattern_probe_fields(variant, scrutinee_place, span)?
        };
        let variant = self.construct_temp(MirAggregateKind::EnumVariant(variant), fields, scrutinee_ty, span);
        Some(self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Eq, lhs: value.clone(), rhs: variant },
            bool_ty,
            span,
        ))
    }

    fn variant_pattern_probe_fields(
        &mut self,
        variant: DefId,
        scrutinee: &MirPlace,
        span: Span,
    ) -> Option<MirAggregateFields> {
        let Some(field_names) = self.context.variant_fields.get(&variant).cloned() else {
            self.errors
                .push(MirError::unsupported(span, "variant payload pattern is missing field metadata"));
            return None;
        };

        Some(MirAggregateFields::Named(
            field_names
                .into_iter()
                .map(|field| {
                    let mut place = scrutinee.clone();
                    place
                        .projections
                        .push(MirProjection::VariantField { variant, field });
                    MirNamedOperand { name: field, value: MirOperand::Place(place) }
                })
                .collect(),
        ))
    }

    fn bind_variant_payload(
        &mut self,
        pattern: &HirPattern,
        scrutinee: &MirPlace,
        scrutinee_ty: MirType,
        span: Span,
    ) -> Option<()> {
        match pattern {
            HirPattern::Variant(variant, fields) => {
                self.bind_variant_fields(*variant, fields, scrutinee, scrutinee_ty, span)
            }
            HirPattern::Alias(alias_def, alias_name, inner) => {
                let alias = self.pattern_binding_local(*alias_def, *alias_name, scrutinee_ty, span);
                self.assign(MirPlace::local(alias), MirOperand::Place(scrutinee.clone()), scrutinee_ty, span);
                self.bind_variant_payload(inner, scrutinee, scrutinee_ty, span)
            }
            _ => {
                self.errors
                    .push(MirError::unsupported(span, "non-variant discerne payload binding"));
                None
            }
        }
    }

    fn bind_variant_fields(
        &mut self,
        variant: DefId,
        fields: &[HirPattern],
        scrutinee: &MirPlace,
        scrutinee_ty: MirType,
        span: Span,
    ) -> Option<()> {
        let Some(field_names) = self.context.variant_fields.get(&variant).cloned() else {
            self.errors
                .push(MirError::unsupported(span, "variant payload pattern is missing field metadata"));
            return None;
        };
        let Some(field_tys) = self.context.variant_field_tys.get(&variant).cloned() else {
            self.errors
                .push(MirError::missing_type(span, "variant payload pattern fields"));
            return None;
        };
        // A bare arm (`casu Moveo ut totum`) names the variant and binds none
        // of its payload fields; the probe condition already routed on the
        // variant identity, so there is nothing to bind. Semantic analysis
        // admits this shape only under a whole-value alias.
        if fields.is_empty() {
            return Some(());
        }
        if fields.len() != field_names.len() {
            self.errors
                .push(MirError::unsupported(span, "variant payload pattern arity mismatch"));
            return None;
        }

        for (field, subpattern) in field_names.into_iter().zip(fields) {
            let field_ty = field_tys
                .get(&field)
                .copied()
                .and_then(|ty| self.instantiate_variant_field_ty(variant, ty, scrutinee_ty, span));
            match subpattern {
                HirPattern::Binding(def_id, name) | HirPattern::Alias(def_id, name, _) => {
                    self.bind_variant_field_payload(*def_id, *name, variant, field, scrutinee, field_ty, span)?;
                }
                HirPattern::Wildcard => {}
                _ => {
                    self.errors.push(MirError::unsupported(
                        span,
                        "nested variant payload patterns before MIR lowering",
                    ));
                    return None;
                }
            }
        }
        Some(())
    }

    fn instantiate_variant_field_ty(
        &mut self,
        variant: DefId,
        field_ty: MirType,
        scrutinee_ty: MirType,
        span: Span,
    ) -> Option<MirType> {
        if !type_contains_param(self.types, field_ty.semantic_id()) {
            return Some(field_ty);
        }
        let parent = *self.context.variant_parents.get(&variant)?;
        let params = self.context.enum_type_params.get(&parent)?;
        let Type::Applied(base, args) = self.types.get(scrutinee_ty.semantic_id()) else {
            return Some(field_ty);
        };
        if !matches!(self.types.get(*base), Type::Enum(def_id) if *def_id == parent) || params.len() != args.len() {
            return Some(field_ty);
        }
        let type_args = args
            .iter()
            .map(|arg| match arg {
                radix_types::AppliedArg::Type(id) => Some(*id),
                radix_types::AppliedArg::Size(_) => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let bindings = params
            .iter()
            .copied()
            .zip(type_args.into_iter().map(MirType::semantic))
            .collect::<MirGenericBindings>();
        instantiate_existing_generic_type(self.types, field_ty, &bindings, span)
            .map_err(|error| {
                self.errors
                    .push(MirError::validation(error.span, error.message));
            })
            .ok()
    }

    /// The local a pattern binding lives in: a fresh one, or (inside
    /// `casu A aut B`) the one the first alternative already declared.
    fn pattern_binding_local(&mut self, def_id: DefId, name: Symbol, ty: MirType, span: Span) -> MirLocalId {
        if self.shared_pattern_bindings
            && let Some(existing) = self.bindings.get(&def_id)
        {
            return existing.local;
        }
        let local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: local, name: Some(name), ty, mutable: false, span });
        self.bindings.insert(def_id, LocalBinding { local, ty });
        local
    }

    fn bind_variant_field_payload(
        &mut self,
        def_id: DefId,
        name: Symbol,
        variant: DefId,
        field: Symbol,
        scrutinee: &MirPlace,
        ty: Option<MirType>,
        span: Span,
    ) -> Option<()> {
        let Some(ty) = ty else {
            self.errors
                .push(MirError::missing_type(span, "variant payload pattern field"));
            return None;
        };
        let local = self.pattern_binding_local(def_id, name, ty, span);
        let mut place = scrutinee.clone();
        place
            .projections
            .push(MirProjection::VariantField { variant, field });
        self.assign(MirPlace::local(local), MirOperand::Place(place), ty, span);
        Some(())
    }

    /// The switch constant for one literal or negated-literal pattern.
    fn literal_pattern_switch_constant(
        &mut self,
        pattern: &HirPattern,
        scrutinee_ty: Option<TypeId>,
        span: Span,
    ) -> Option<MirConstant> {
        let (HirPattern::Literal(literal) | HirPattern::NegatedLiteral(literal)) = pattern else {
            self.errors
                .push(MirError::unsupported(span, "literal alternative that is not a literal pattern"));
            return None;
        };
        let constant = self.literal_switch_constant(literal, scrutinee_ty, span)?;
        if matches!(pattern, HirPattern::NegatedLiteral(_)) {
            self.negated_switch_constant(constant, span)
        } else {
            Some(constant)
        }
    }

    fn literal_switch_constant(
        &mut self,
        literal: &HirLiteral,
        scrutinee_ty: Option<TypeId>,
        span: Span,
    ) -> Option<MirConstant> {
        match literal {
            HirLiteral::Int(value) => match i64::try_from(*value) {
                Ok(value) => Some(MirConstant::Int(value)),
                // Switch patterns on unsigned scrutinees may legitimately reach
                // the upper half of u64; carry them exactly as `UInt`.
                Err(_) => Some(MirConstant::UInt(*value)),
            },
            // decimal-widths D1 successor: a decimal-typed
            // text pattern carries the exact scaled carrier, so the case
            // compares against the same Int carrier the scrutinee stores.
            HirLiteral::FloatText { text, .. } if let Some(width) = self.decimal_literal_width(scrutinee_ty) => {
                let carrier = self.context.interner.and_then(|interner| {
                    radix_codegen_shared::decimal::decimal_text_to_carrier(interner.resolve(*text), width)
                });
                let Some(carrier) = carrier else {
                    self.errors.push(MirError::unsupported(
                        span,
                        "decimal literal pattern carrier is outside the declared width",
                    ));
                    return None;
                };
                Some(MirConstant::Int(carrier))
            }
            HirLiteral::Float(value) | HirLiteral::FloatText { value, .. } => Some(MirConstant::Float(*value)),
            HirLiteral::String(symbol) => Some(MirConstant::String(*symbol)),
            HirLiteral::Ascii(symbol) => Some(MirConstant::Ascii(*symbol)),
            HirLiteral::Bool(value) => Some(MirConstant::Bool(*value)),
            HirLiteral::Nil => Some(MirConstant::Nil),
            // A case on an `inf` subject beyond `u64` (ruling 43).
            HirLiteral::BigInt(digits) => Some(super::inf_literal_constant(digits)),
            HirLiteral::Octeti(_) | HirLiteral::JsonValor(_) | HirLiteral::Regex(_, _) => {
                self.errors
                    .push(MirError::unsupported(span, "literal pattern before switch MIR lowering"));
                None
            }
        }
    }

    /// Negate a number switch constant for a `casu -N` pattern (D22.2).
    ///
    /// Integer magnitudes negate from their unsigned carrier so `i64::MIN`
    /// (whose magnitude arrives as `UInt`) stays representable.
    fn negated_switch_constant(&mut self, constant: MirConstant, span: Span) -> Option<MirConstant> {
        let negated = match constant {
            MirConstant::Int(value) => value.checked_neg().map(MirConstant::Int),
            MirConstant::UInt(magnitude) => 0i64.checked_sub_unsigned(magnitude).map(MirConstant::Int),
            MirConstant::Float(value) => Some(MirConstant::Float(-value)),
            // `casu -N` on an `inf` subject: negate exactly and re-canonicalize.
            MirConstant::Magnus(digits) => handroll::Magnus::parse_decimal(&digits)
                .ok()
                .map(|value| super::inf_literal_constant(&(-value).to_string())),
            _ => None,
        };
        if negated.is_none() {
            self.errors.push(MirError::unsupported(
                span,
                "negated literal pattern outside the signed carrier",
            ));
        }
        negated
    }

    /// Lower a block into a destination place, preserving early terminators.
    ///
    /// Statement prefixes are emitted first. If they leave the current block
    /// sealed, no tail assignment is generated; otherwise the tail expression
    /// supplies the destination value.
    pub(super) fn lower_block_to_destination(
        &mut self,
        block: &HirBlock,
        destination: MirPlace,
        ty: MirType,
        span: Span,
    ) -> Option<()> {
        if block.breakable.is_some() {
            self.lower_breakable_block(block, block.span);
            if !self.current_is_open() {
                return Some(());
            }
            if self.is_vacuum(ty) {
                self.assign(destination, MirOperand::Constant(MirConstant::Unit), ty, span);
                return Some(());
            }
            self.errors.push(MirError::unsupported(
                block.span,
                "breakable block with value destination requires redde or a tail expression after epilogue",
            ));
            return None;
        }

        for stmt in &block.statements {
            if !self.current_is_open() {
                return Some(());
            }
            self.visit_stmt(stmt);
        }

        if !self.current_is_open() {
            return Some(());
        }

        let Some(expr) = &block.expr else {
            if self.is_vacuum(ty) {
                self.assign(destination, MirOperand::Constant(MirConstant::Unit), ty, span);
                return Some(());
            }
            self.errors.push(MirError::unsupported(
                block.span,
                "expression-valued block without a tail expression",
            ));
            return None;
        };

        self.lower_expr_to_destination(expr, destination, ty)
    }

    /// Lower `si` when its result is used only for control effects.
    ///
    /// The helper creates an explicit branch and an explicit join even when no
    /// `secus` block exists; the missing else arm simply reaches the join.
    pub(super) fn lower_si_stmt(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
        span: Span,
    ) {
        if let Some(catch) = then_catch {
            self.lower_handled_si_stmt(cond, then_block, catch, else_block, span);
            return;
        }

        let Some(condition) = self.lower_expr_value(cond) else {
            return;
        };

        let then_id = self.fresh_block(then_block.span);
        let (else_id, join_id) = if let Some(block) = else_block {
            let else_id = self.fresh_block(block.span);
            let join_id = self.fresh_block(span);
            (else_id, join_id)
        } else {
            let join_id = self.fresh_block(span);
            (join_id, join_id)
        };

        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
            span,
        );

        self.switch_to(then_id);
        self.visit_block(then_block);
        let then_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        let else_reaches = if let Some(block) = else_block {
            self.switch_to(else_id);
            self.visit_block(block);
            self.terminate_open_current(MirTerminatorKind::Goto(join_id), span)
        } else {
            true
        };

        if then_reaches || else_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }
    }

    fn lower_handled_si_stmt(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        catch: &HirCape,
        else_block: Option<&HirBlock>,
        span: Span,
    ) {
        let Some(error_ty) = catch.binding_ty.map(MirType::semantic) else {
            self.errors
                .push(MirError::missing_type(catch.span, "cape handler binding"));
            return;
        };

        let then_id = self.fresh_block(then_block.span);
        let (else_id, join_id) = if let Some(block) = else_block {
            let else_id = self.fresh_block(block.span);
            let join_id = self.fresh_block(span);
            (else_id, join_id)
        } else {
            let join_id = self.fresh_block(span);
            (join_id, join_id)
        };
        let handler_id = self.fresh_block(catch.body.span);

        let handler_local = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: handler_local,
            name: Some(catch.binding_name),
            ty: error_ty,
            mutable: false,
            span: catch.span,
        });
        self.bindings
            .insert(catch.binding_def_id, LocalBinding { local: handler_local, ty: error_ty });

        self.capture_depth += 1;
        self.handlers.push(HandlerContext {
            error_place: MirPlace::local(handler_local),
            error_ty,
            error_block: handler_id,
            depth: self.capture_depth,
        });
        self.open_device_block(false, handler_id, MirPlace::local(handler_local), span);
        let Some(condition) = self.lower_expr_value(cond) else {
            self.handlers.pop();
            self.close_device_block(span);
            self.seal_unreachable(handler_id, catch.span);
            self.switch_to(join_id);
            return;
        };

        // The else edge leaves the handled region without running the then
        // block, so it closes the device block on its own edge.
        let else_edge = if self.context.fault_delivery {
            self.fresh_block(span)
        } else {
            else_id
        };
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_edge },
            span,
        );
        if else_edge != else_id {
            self.switch_to(else_edge);
            self.emit_device_marker(radix_mir::MirDeviceBlockOp::Exit { error_edge: false }, Vec::new(), span);
            self.terminate_current(MirTerminatorKind::Goto(else_id), span);
        }

        self.switch_to(then_id);
        self.visit_block(then_block);
        self.handlers.pop();
        self.close_device_block(span);
        let then_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        self.switch_to(handler_id);
        self.emit_device_handler_exit(catch.span);
        self.visit_block(&catch.body);
        let handler_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), catch.span);

        let else_reaches = if let Some(block) = else_block {
            self.switch_to(else_id);
            self.visit_block(block);
            self.terminate_open_current(MirTerminatorKind::Goto(join_id), span)
        } else {
            true
        };

        if then_reaches || handler_reaches || else_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }
    }

    /// Lower expression-valued `si` into an existing destination.
    ///
    /// A missing `secus` branch is rejected because MIR has no implicit default
    /// value for an expression result.
    pub(super) fn lower_si_to_destination(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        else_block: Option<&HirBlock>,
        destination: MirPlace,
        ty: MirType,
        span: Span,
    ) -> Option<()> {
        let Some(else_block) = else_block else {
            self.errors
                .push(MirError::unsupported(span, "expression-valued si without secus destination"));
            return None;
        };

        // K4: a tensor mask makes this the `✓ ✗` select, not a branch.
        if self.is_tensor_select(cond) {
            return self.lower_tensor_select(cond, then_block, else_block, destination, ty, span);
        }
        if self.is_register_select(cond) {
            return self.lower_register_select(cond, (then_block, else_block), destination, ty, span);
        }
        let condition = self.lower_expr_value(cond)?;
        let then_id = self.fresh_block(then_block.span);
        let else_id = self.fresh_block(else_block.span);
        let join_id = self.fresh_block(span);

        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
            span,
        );

        self.switch_to(then_id);
        self.lower_block_to_destination(then_block, destination.clone(), ty, then_block.span)?;
        let then_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        self.switch_to(else_id);
        self.lower_block_to_destination(else_block, destination, ty, else_block.span)?;
        let else_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        if then_reaches || else_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }

        Some(())
    }

    /// Lower a `Loop` block (`fac { } dum cond`) into body, test, and after
    /// blocks.
    ///
    /// `rumpe` jumps to the after block and `perge` to the test block through
    /// the loop context pushed while lowering the body; the test jumps back
    /// to the body head.
    pub(super) fn lower_loop_expr(&mut self, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "loop expression with non-vacuum result"));
            return None;
        }

        let body_id = self.fresh_block(block.span);
        let after_id = self.fresh_block(expr.span);

        self.terminate_current(MirTerminatorKind::Goto(body_id), expr.span);
        self.switch_to(body_id);
        let Some((body, test, cond)) = block.fac_dum_parts() else {
            self.breakables.push(BreakableContext {
                perge_target: Some(body_id),
                rumpe_target: after_id,
                rumpe_used: false,
            });
            self.visit_block(block);
            self.breakables.pop();
            self.terminate_open_current(MirTerminatorKind::Goto(body_id), expr.span);
            self.switch_to(after_id);
            return Some(MirOperand::Constant(MirConstant::Unit));
        };

        // Ruling R3: `perge` lands at the end of the body, so the `dum` test
        // runs next. The test gets its own block; body fallthrough and every
        // `perge` goto it, and its `rumpe` leaves the loop.
        let test_id = self.fresh_block(test.span);
        self.breakables.push(BreakableContext {
            perge_target: Some(test_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        for stmt in body {
            if !self.current_is_open() {
                break;
            }
            self.visit_stmt(stmt);
        }
        // The synthesized `si ¬cond { rumpe }` test is lowered while this
        // breakable context is still on the stack, so snapshot the body's
        // `rumpe` flag first: the test's own `rumpe` must not count as a user
        // break out of the loop.
        let body_rumpe_used = self
            .breakables
            .last()
            .is_some_and(|context| context.rumpe_used);
        self.terminate_open_current(MirTerminatorKind::Goto(test_id), test.span);
        self.switch_to(test_id);
        if block
            .breakable
            .is_some_and(radix_hir::HirBreakableKind::is_loop_marker)
            && self.context.fault_delivery
        {
            // `fac omnia … dum` delivers per iteration at the back edge.
            self.emit_device_marker(radix_mir::MirDeviceBlockOp::Deliver { sync: true }, Vec::new(), test.span);
        }
        self.visit_stmt(test);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(body_id), expr.span);
        // `fac { … } dum verum` is the do-while spelling of `while true`: the
        // synthesized `si ¬verum { rumpe }` is unreachable, so with no user
        // `rumpe` the exit block must not carry the implicit function return,
        // and the dead rumpe edge still reaches the sealed block keeping the
        // loop a valid terminator path.
        if hir_cond_is_literal_true(cond) && !body_rumpe_used {
            self.seal_unreachable(after_id, expr.span);
        } else {
            self.switch_to(after_id);
        }
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Lower `dum` into condition, body, and after blocks.
    ///
    /// `perge` jumps back to the condition block and `rumpe` jumps to the after
    /// block through the loop context pushed while lowering the body.
    pub(super) fn lower_dum(&mut self, cond: &HirExpression, body: &HirBlock, span: Span) {
        let cond_id = self.fresh_block(cond.span);
        let body_id = self.fresh_block(body.span);
        let after_id = self.fresh_block(span);

        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(cond_id);
        let Some(condition) = self.lower_expr_value(cond) else {
            self.seal_unreachable(cond_id, cond.span);
            self.switch_to(after_id);
            return;
        };
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: body_id, else_block: after_id },
            cond.span,
        );

        self.breakables.push(BreakableContext {
            perge_target: Some(cond_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        self.switch_to(body_id);
        self.visit_block(body);
        let loop_rumpe_used = self.breakables.pop().is_some_and(|ctx| ctx.rumpe_used);
        self.terminate_open_current(MirTerminatorKind::Goto(cond_id), span);

        // `while true` never exits through its condition, so with no `rumpe` the
        // after-block is dead; sealing it keeps the implicit function return out
        // of a block the loop cannot reach. The `Branch` terminator above stays:
        // its else edge is what keeps an exit-less `while true { }` loop a valid
        // terminator path.
        if hir_cond_is_literal_true(cond) && !loop_rumpe_used {
            self.seal_unreachable(after_id, span);
        } else {
            self.switch_to(after_id);
        }
    }

    fn lower_range_itera(&mut self, range: RangeIteraLowering<'_>) -> Option<()> {
        let index_ty = coordinate_type(range.body, range.binding).map_or_else(
            || self.range_index_ty(&[Some(range.start), Some(range.end), range.step]),
            MirType::semantic,
        );
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let index = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: index,
            name: (!range.mutable).then_some(range.name),
            ty: index_ty,
            mutable: true,
            span: range.span,
        });
        let binder = self.range_binder_local(index, range.mutable, range.name, index_ty, range.span);
        self.bindings
            .insert(range.binding, LocalBinding { local: binder, ty: index_ty });

        let end_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: end_local, name: None, ty: index_ty, mutable: false, span: range.span });
        let step_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: step_local, name: None, ty: index_ty, mutable: false, span: range.span });

        self.lower_expr_to_destination(range.start, MirPlace::local(index), index_ty)?;
        self.lower_expr_to_destination(range.end, MirPlace::local(end_local), index_ty)?;
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(range.span);
        match range.step {
            Some(step) => {
                self.lower_expr_to_destination(step, MirPlace::local(step_local), index_ty)?;
                if !self.current_is_open() {
                    return None;
                }
                self.lower_range_stride(index, end_local, step_local, cond_id, bivalens, index_ty, range.span);
            }
            None => {
                self.lower_default_range_step(index, end_local, step_local, cond_id, bivalens, index_ty, range.span);
            }
        }

        let positive_check_id = self.fresh_block(range.span);
        let negative_or_zero_id = self.fresh_block(range.span);
        let negative_check_id = self.fresh_block(range.span);
        let body_id = self.fresh_block(range.body.span);
        let increment_id = self.fresh_block(range.span);
        let after_id = self.fresh_block(range.span);

        self.switch_to(cond_id);
        let step_is_positive = self.binary_temp(
            MirBinOp::Gt,
            MirOperand::Place(MirPlace::local(step_local)),
            MirOperand::Constant(MirConstant::Int(0)),
            bivalens,
            range.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: step_is_positive,
                then_block: positive_check_id,
                else_block: negative_or_zero_id,
            },
            range.span,
        );

        self.switch_to(positive_check_id);
        let positive_op = match range.kind {
            HirRangeKind::Exclusive => MirBinOp::Lt,
            HirRangeKind::Inclusive => MirBinOp::LtEq,
        };
        let positive_continues = self.binary_temp(
            positive_op,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Place(MirPlace::local(end_local)),
            bivalens,
            range.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: positive_continues, then_block: body_id, else_block: after_id },
            range.span,
        );

        self.switch_to(negative_or_zero_id);
        let step_is_negative = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(step_local)),
            MirOperand::Constant(MirConstant::Int(0)),
            bivalens,
            range.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: step_is_negative,
                then_block: negative_check_id,
                else_block: after_id,
            },
            range.span,
        );

        self.switch_to(negative_check_id);
        let negative_op = match range.kind {
            HirRangeKind::Exclusive => MirBinOp::Gt,
            HirRangeKind::Inclusive => MirBinOp::GtEq,
        };
        let negative_continues = self.binary_temp(
            negative_op,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Place(MirPlace::local(end_local)),
            bivalens,
            range.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: negative_continues, then_block: body_id, else_block: after_id },
            range.span,
        );

        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        self.switch_to(body_id);
        self.copy_range_binder(binder, index, index_ty, range.span);
        self.visit_block(range.body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), range.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Place(MirPlace::local(step_local)),
            index_ty,
            range.span,
        );
        self.assign(MirPlace::local(index), next, index_ty, range.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), range.span);

        self.switch_to(after_id);
        Some(())
    }

    /// Lower the Cartesian `itera ab` range product (NR-3, nary-range-walk).
    ///
    /// One ordered Cartesian construct, never a zip: every range's bounds
    /// are evaluated in source order before the walk starts (the extents
    /// live outside the binder scope, so no extent can observe a binder),
    /// then one range CFG per axis nests first-range-outermost and later
    /// ranges inward. Each axis keeps the single-range walk's sign-split
    /// admission and inclusive/exclusive/step behavior; no equal-length
    /// guard, shared index, or truncation is installed. `perge` continues
    /// the innermost axis; `rumpe` exits the innermost axis into the
    /// enclosing axis's increment — hand-nested loop reading.
    pub(super) fn lower_itera_range_expr(
        &mut self,
        ranges: &[HirExpression],
        bindings: &[(DefId, Symbol)],
        mutable: bool,
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "itera expression with non-vacuum result"));
            return None;
        }
        if ranges.len() != bindings.len() || ranges.is_empty() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "itera range product with unequal range and binder counts",
            ));
            return None;
        }
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let span = expr.span;

        // Only `Intervallum` members reach MIR (the mixed-source law declines
        // earlier at check); anything else is a fail-closed decline, never a
        // guessed walk.
        let mut members = Vec::with_capacity(ranges.len());
        for range in ranges {
            let HirExpressionKind::Intervallum { start, end, step, kind } = &range.kind else {
                self.errors.push(MirError::unsupported(
                    range.span,
                    "itera range product member before range MIR lowering",
                ));
                return None;
            };
            members.push((start, end, step.as_deref(), *kind));
        }

        // Pre-allocate each axis's extent-entry block plus the nest entry
        // (the outermost axis's per-pass entry) so every extent chain can
        // name its exit before the nest is filled.
        let axis_count = members.len();
        let nest_entry = self.fresh_block(span);
        let mut extent_entries = Vec::with_capacity(axis_count);
        for _ in 0..axis_count {
            extent_entries.push(self.fresh_block(span));
        }

        let mut axes = Vec::with_capacity(axis_count);
        for (position, ((start, end, step, kind), (binding, name))) in members.iter().zip(bindings).enumerate() {
            let index_ty = coordinate_type(body, *binding)
                .map_or_else(|| self.range_index_ty(&[Some(*start), Some(*end), *step]), MirType::semantic);
            let index = self.next_local_id();
            self.locals.push(MirLocalDecl {
                id: index,
                name: (!mutable).then_some(*name),
                ty: index_ty,
                mutable: true,
                span,
            });
            let binder = self.range_binder_local(index, mutable, *name, index_ty, span);
            self.bindings
                .insert(*binding, LocalBinding { local: binder, ty: index_ty });
            // The extent values are loop-invariant (no extent can reference
            // a binder), so each pass re-assigns the index from `start`
            // instead of re-evaluating the extent expressions.
            let start_local = self.next_local_id();
            self.locals
                .push(MirLocalDecl { id: start_local, name: None, ty: index_ty, mutable: false, span });
            let bound = self.next_local_id();
            self.locals
                .push(MirLocalDecl { id: bound, name: None, ty: index_ty, mutable: false, span });
            let step_local = self.next_local_id();
            self.locals
                .push(MirLocalDecl { id: step_local, name: None, ty: index_ty, mutable: false, span });

            if position > 0 {
                self.switch_to(extent_entries[position]);
            }
            self.lower_expr_to_destination(start, MirPlace::local(start_local), index_ty)?;
            self.lower_expr_to_destination(end, MirPlace::local(bound), index_ty)?;
            if !self.current_is_open() {
                return None;
            }
            let exit = if position + 1 < axis_count {
                extent_entries[position + 1]
            } else {
                nest_entry
            };
            match step {
                Some(step) => {
                    self.lower_expr_to_destination(step, MirPlace::local(step_local), index_ty)?;
                    if !self.current_is_open() {
                        return None;
                    }
                    self.lower_range_stride(start_local, bound, step_local, exit, bivalens, index_ty, span);
                }
                None => self.lower_default_range_step(start_local, bound, step_local, exit, bivalens, index_ty, span),
            }
            axes.push(RangeProductAxis {
                index,
                binder,
                start: start_local,
                bound,
                step: step_local,
                kind: *kind,
                index_ty,
            });
        }

        let after_id = self.fresh_block(span);
        self.lower_range_product_level(&axes, body, 0, nest_entry, after_id, span)?;
        self.switch_to(after_id);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Build one axis's range CFG and nest inward (NR-3).
    ///
    /// Mirrors the single-range walk's condition blocks: each pass re-assigns
    /// the axis index from its loop-invariant start, the step sign splits
    /// positive/negative admissions, an inclusive axis compares with
    /// `<=`/`>=`, and a failing axis exits into `continuation` (the
    /// enclosing axis's increment; the outermost exit joins `after`).
    fn lower_range_product_level(
        &mut self,
        axes: &[RangeProductAxis],
        body: &HirBlock,
        level: usize,
        entry: MirBlockId,
        continuation: MirBlockId,
        span: Span,
    ) -> Option<()> {
        let range_axis = &axes[level];
        let innermost = level + 1 == axes.len();
        let numerus = range_axis.index_ty;
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let cond_id = self.fresh_block(span);
        let positive_check_id = self.fresh_block(span);
        let negative_or_zero_id = self.fresh_block(span);
        let negative_check_id = self.fresh_block(span);
        let then_target = if innermost {
            self.fresh_block(body.span)
        } else {
            self.fresh_block(span)
        };
        let increment_id = self.fresh_block(span);
        let loop_after_id = self.fresh_block(span);

        self.switch_to(entry);
        self.assign(
            MirPlace::local(range_axis.index),
            MirOperand::Place(MirPlace::local(range_axis.start)),
            numerus,
            span,
        );
        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(cond_id);
        let step_is_positive = self.binary_temp(
            MirBinOp::Gt,
            MirOperand::Place(MirPlace::local(range_axis.step)),
            MirOperand::Constant(MirConstant::Int(0)),
            bivalens,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: step_is_positive,
                then_block: positive_check_id,
                else_block: negative_or_zero_id,
            },
            span,
        );

        self.switch_to(positive_check_id);
        let positive_op = match range_axis.kind {
            HirRangeKind::Exclusive => MirBinOp::Lt,
            HirRangeKind::Inclusive => MirBinOp::LtEq,
        };
        let positive_continues = self.binary_temp(
            positive_op,
            MirOperand::Place(MirPlace::local(range_axis.index)),
            MirOperand::Place(MirPlace::local(range_axis.bound)),
            bivalens,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: positive_continues,
                then_block: then_target,
                else_block: loop_after_id,
            },
            span,
        );

        self.switch_to(negative_or_zero_id);
        let step_is_negative = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(range_axis.step)),
            MirOperand::Constant(MirConstant::Int(0)),
            bivalens,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: step_is_negative,
                then_block: negative_check_id,
                else_block: loop_after_id,
            },
            span,
        );

        self.switch_to(negative_check_id);
        let negative_op = match range_axis.kind {
            HirRangeKind::Exclusive => MirBinOp::Gt,
            HirRangeKind::Inclusive => MirBinOp::GtEq,
        };
        let negative_continues = self.binary_temp(
            negative_op,
            MirOperand::Place(MirPlace::local(range_axis.index)),
            MirOperand::Place(MirPlace::local(range_axis.bound)),
            bivalens,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: negative_continues,
                then_block: then_target,
                else_block: loop_after_id,
            },
            span,
        );

        self.switch_to(then_target);
        self.copy_range_binder(range_axis.binder, range_axis.index, numerus, span);
        if innermost {
            self.breakables.push(BreakableContext {
                perge_target: Some(increment_id),
                rumpe_target: loop_after_id,
                rumpe_used: false,
            });
            self.visit_block(body);
            self.breakables.pop();
            self.terminate_open_current(MirTerminatorKind::Goto(increment_id), span);
        } else {
            let next_entry = self.fresh_block(span);
            self.terminate_current(MirTerminatorKind::Goto(next_entry), span);
            self.lower_range_product_level(axes, body, level + 1, next_entry, increment_id, span)?;
        }

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(range_axis.index)),
            MirOperand::Place(MirPlace::local(range_axis.step)),
            numerus,
            span,
        );
        self.assign(MirPlace::local(range_axis.index), next, numerus, span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(loop_after_id);
        self.terminate_open_current(MirTerminatorKind::Goto(continuation), span);
        Some(())
    }

    /// Mark the `cursor_stream` call that produced `collection` as stepped
    /// (D6.6) when it is the direct iterable of an `itera ex` walk.
    ///
    /// The call must be the last statement of the open block and write the
    /// operand's temp, so a generator call nested in arguments or bound to a
    /// name first keeps its materializing read. An async cursor is retyped to
    /// the `lista<T>` read the walk indexes, which replaces the drain snapshot.
    /// Returns the marked call's generator callee when the stream was marked,
    /// and `None` otherwise.
    fn mark_stepped_cursor_stream(
        &mut self,
        collection: &MirOperand,
        kind: &CollectionIteraKind,
        mode: HirIteraMode,
    ) -> Option<DefId> {
        let element = match kind {
            CollectionIteraKind::Array(element)
            | CollectionIteraKind::Cursor(element)
            | CollectionIteraKind::AsyncCursor(element) => *element,
            _ => return None,
        };
        let (HirIteraMode::Ex, MirOperand::Temp(temp), Some(current)) = (mode, collection, self.current) else {
            return None;
        };
        let array = self.types.find_array(element)?;
        let array_ty = MirType::semantic(array);
        let Some(MirStatement {
            kind:
                MirStatementKind::RuntimeCall {
                    destination: Some(destination),
                    call: MirRuntimeCall { intrinsic: MirIntrinsic::CursorStream(stream), return_ty, .. },
                },
            ..
        }) = self.block_mut(current).statements.last_mut()
        else {
            return None;
        };
        if *destination != MirPlace::temp(*temp) {
            return None;
        }
        let callee = stream.callee;
        stream.stepped = true;
        *return_ty = array_ty;
        self.temps[temp.0 as usize].ty = array_ty;
        Some(callee)
    }

    /// Whether the walk reads a conversation's content frames
    /// (`cursor<scrinium<T>>`), whose length grows while the loop runs.
    fn is_frame_cursor_walk(&self, kind: &CollectionIteraKind) -> bool {
        let (CollectionIteraKind::Cursor(element) | CollectionIteraKind::AsyncCursor(element)) = kind else {
            return false;
        };
        let frame = match self.normalized_type(*element) {
            Type::Struct(def_id) => Some(*def_id),
            Type::Applied(base, _) => match self.normalized_type(*base) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
            _ => None,
        };
        frame.is_some() && frame == self.context.scrinium_def
    }

    /// Classify an itera receiver into the lane that walks it. A `Ref` marker
    /// is peeled and its pointee classified by this same function.
    pub(super) fn classify_collection_itera(
        &self,
        ty: TypeId,
        mode: HirIteraMode,
        coordinate_count: usize,
    ) -> CollectionIteraKind {
        let has_coordinates = coordinate_count > 0;
        let normalized = self.normalized_type(ty);
        match normalized {
            // The extent of each axis is read from the carrier at run time
            // (`tensor_axis_bounds`); a literal axis is the folded case of
            // that same read, so the rank is the only static requirement.
            Type::Tensor(elem, shape) => {
                let literal = i64s_from_literal_index(*shape, self.types);
                let rank = literal
                    .as_ref()
                    .map(Vec::len)
                    .or_else(|| self.tensor_shape_rank(*shape));
                match rank {
                    Some(rank) if matches!(mode, HirIteraMode::Ex) && has_coordinates && rank == coordinate_count => {
                        CollectionIteraKind::Shaped { element: *elem, rank, literal, matrix: false }
                    }
                    // The coordinate-free route walks a rank-1 tensor.
                    Some(1) if matches!(mode, HirIteraMode::Ex) && !has_coordinates => {
                        CollectionIteraKind::Tensor(*elem, literal.and_then(|dims| dims.first().copied()))
                    }
                    _ => CollectionIteraKind::Unsupported,
                }
            }
            Type::Matrix(elem, shape) => {
                let dimensions = i64s_from_literal_index(*shape, self.types);
                let literal_matrix = dimensions
                    .as_ref()
                    .is_some_and(|dims| dims.len() == 2 && dims.iter().all(|dim| u8::try_from(*dim).is_ok()));
                if matches!(mode, HirIteraMode::Ex) && has_coordinates && coordinate_count == 2 && literal_matrix {
                    CollectionIteraKind::Shaped { element: *elem, rank: 2, literal: dimensions, matrix: true }
                } else {
                    CollectionIteraKind::Unsupported
                }
            }
            // Route B (AEU-4): runtime-len `lista<T, magnitudo N>` lowers in the base
            // lane to a static split on the carrier `len` with N+1
            // guard-free arms (total N(N+1)/2 lanes). The N <= 10
            // code-size cap keeps the split bounded; non-literal capacities
            // (shape-generics) and N > 10 fall through to Unsupported.
            // `de` binds the index, not a lane, so it is a plain index loop
            // over the runtime length: the same shape as an unbounded lista,
            // independent of any literal capacity.
            Type::BoundedArray { element, .. } | Type::RangedArray { element, .. }
                if matches!(mode, HirIteraMode::De) =>
            {
                CollectionIteraKind::Array(*element)
            }
            Type::BoundedArray { element, capacity } => {
                let literal_n = if matches!(mode, HirIteraMode::Ex) {
                    i64s_from_literal_index(*capacity, self.types)
                        .and_then(|dims| dims.first().copied())
                        .filter(|n| (0..=10).contains(n))
                } else {
                    None
                };
                match literal_n {
                    Some(n) => CollectionIteraKind::BoundedArray(*element, n),
                    None => CollectionIteraKind::Unsupported,
                }
            }
            // Route B (AEU-7): the scalar buffer forms ride the same
            // static split — `octeti<N>` lanes bind `numerus<u8>`,
            // `ascii<N>` / `textus<N>` lanes bind `ascii<1>` (the D14/D17
            // scalar index rows). Arm k reads only lanes 0..k ⊆ 0..len,
            // so the non-ASCII trap row can never fire from a dead slot.
            Type::BoundedOcteti { capacity } | Type::BoundedAscii { capacity } | Type::BoundedTextus { capacity } => {
                let literal_n = if matches!(mode, HirIteraMode::Ex) {
                    i64s_from_literal_index(*capacity, self.types)
                        .and_then(|dims| dims.first().copied())
                        .filter(|n| (0..=10).contains(n))
                } else {
                    None
                };
                match literal_n {
                    Some(n) => match self.scalar_buffer_element(normalized) {
                        Some(element) => CollectionIteraKind::BoundedArray(element, n),
                        None => CollectionIteraKind::Unsupported,
                    },
                    None => CollectionIteraKind::Unsupported,
                }
            }
            Type::Array(item) | Type::RangedArray { element: item, .. } => CollectionIteraKind::Array(*item),
            // D10.4/D19.4: byte iteration binds the same u8 as octeti[i].
            // Retain the byte carrier rather than fabricating a list type.
            Type::Primitive(Primitive::Octeti) if matches!(mode, HirIteraMode::Ex) => {
                match self.scalar_buffer_element(normalized) {
                    Some(element) => CollectionIteraKind::Bytes(element),
                    None => CollectionIteraKind::Unsupported,
                }
            }
            // Text value iteration: only `ex` admits it,
            // mirroring the queue/stack value-surface rule. The binding
            // is `littera` (D10.1), one Unicode scalar, with no trap.
            Type::Primitive(Primitive::Textus | Primitive::Ascii) if matches!(mode, HirIteraMode::Ex) => {
                CollectionIteraKind::Text(self.types.primitive(Primitive::Littera))
            }
            // Queue and stack share the array-backed carrier but not its
            // access order. Only `ex` admits the value iteration surface;
            // `de`/`ab` stay unsupported rather than inventing an index law.
            Type::Queue(item) | Type::BoundedQueue { element: item, .. } if matches!(mode, HirIteraMode::Ex) => {
                CollectionIteraKind::Queue(*item)
            }
            Type::Stack(item) | Type::BoundedStack { element: item, .. } if matches!(mode, HirIteraMode::Ex) => {
                CollectionIteraKind::Stack(*item)
            }
            Type::Map(key, value) | Type::Ledger(key, value) if matches!(mode, HirIteraMode::Ex) && has_coordinates => {
                CollectionIteraKind::MapEntries { key: *key, value: *value }
            }
            Type::Set(element) if matches!(mode, HirIteraMode::Ex) => CollectionIteraKind::SetElements(*element),
            Type::Map(key, _) | Type::Ledger(key, _) if matches!(mode, HirIteraMode::De) => {
                CollectionIteraKind::MapKeys(*key)
            }
            Type::Map(_, value) | Type::Ledger(_, value) if matches!(mode, HirIteraMode::Ex) => {
                CollectionIteraKind::MapValues(*value)
            }
            // Borrowed receiver (`de`/`mut` param, or a field read through
            // a reference): peel the `Ref` marker and classify the pointee
            // with this same classifier, so a borrowed receiver and its
            // by-value twin lower through the same lane.
            Type::Ref(_, inner) => self.classify_collection_itera(*inner, mode, coordinate_count),
            Type::Cursor(value) if matches!(mode, HirIteraMode::Ex) => CollectionIteraKind::Cursor(*value),
            Type::AsyncCursor(value, _) if matches!(mode, HirIteraMode::Ex) => CollectionIteraKind::AsyncCursor(*value),
            _ => CollectionIteraKind::Unsupported,
        }
    }

    /// Rank of a tensor shape that carries no literal extents, or `None` when
    /// the shape does not state one. A bare size parameter may bind a whole
    /// shape of any rank, so it states no rank here.
    fn tensor_shape_rank(&self, shape: IndexId) -> Option<usize> {
        match self.types.get_index(shape) {
            radix_types::IndexExpr::Tuple(dims) => Some(dims.len()),
            radix_types::IndexExpr::Literal(_)
            | radix_types::IndexExpr::Runtime(_)
            | radix_types::IndexExpr::Affine { .. }
            | radix_types::IndexExpr::Divide { .. } => Some(1),
            _ => None,
        }
    }

    /// The loop bound of every axis of a tensor carrier. Literal extents fold
    /// to constants; otherwise each axis is read from the carrier's shape at
    /// run time, so a loop over a tensor never depends on the extent being a
    /// compile-time fact.
    pub(super) fn tensor_axis_bounds(
        &mut self,
        tensor: &MirPlace,
        literal: Option<&[i64]>,
        rank: usize,
        span: Span,
    ) -> Option<Vec<MirOperand>> {
        if let Some(dimensions) = literal {
            return Some(
                dimensions
                    .iter()
                    .map(|n| MirOperand::Constant(MirConstant::Int(*n)))
                    .collect(),
            );
        }
        let integer = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let Some(array) = self.types.find_array(integer.semantic_id()) else {
            self.errors
                .push(MirError::missing_type(span, "tensor shape extent list"));
            return None;
        };
        let Some(option) = self.types.find_equal(&Type::Option(integer.semantic_id())) else {
            self.errors
                .push(MirError::missing_type(span, "tensor shape extent option"));
            return None;
        };
        let (array_ty, option_ty) = (MirType::semantic(array), MirType::semantic(option));
        let shape_value = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorShape),
            vec![MirOperand::Place(tensor.clone())],
            array_ty,
            span,
        );
        let mut bounds = Vec::with_capacity(rank);
        for axis in 0..rank {
            let bound = self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Index),
                vec![shape_value.clone(), MirOperand::Constant(MirConstant::Int(axis as i64))],
                option_ty,
                span,
            );
            bounds.push(self.assign_temp(
                MirValueKind::Option(MirOptionOp::Unwrap { value: bound, mode: MirOptionUnwrapMode::Assume }),
                integer,
                span,
            ));
        }
        Some(bounds)
    }

    /// The declared member series of the enum named by a bare `itera ex`
    /// source, or `None` when the source is not an enum path.
    fn enum_member_variants(&self, source: &HirExpression, mode: HirIteraMode) -> Option<Vec<DefId>> {
        if !matches!(mode, HirIteraMode::Ex) {
            return None;
        }
        let HirExpressionKind::Path(def_id) = &source.kind else {
            return None;
        };
        self.context.enum_variants.get(def_id).cloned()
    }

    /// Materialize an enum's declared member series as a constant
    /// `lista<Enum>` temp: one unit-variant operand per member,
    /// in declaration order, so the ordinary list-iteration machinery walks
    /// the members in that same order.
    fn construct_enum_member_list(
        &mut self,
        enum_ty: TypeId,
        variants: &[DefId],
        span: Span,
    ) -> Option<(MirOperand, TypeId)> {
        let array_ty = self.require_array(enum_ty, span, "itera enum member list")?;
        let mir_enum_ty = MirType::semantic(enum_ty);
        let fields = variants
            .iter()
            .map(|variant| {
                MirAggregateItem::Operand(self.construct_temp(
                    MirAggregateKind::EnumVariant(*variant),
                    MirAggregateFields::Ordered(Vec::new()),
                    mir_enum_ty,
                    span,
                ))
            })
            .collect();
        let operand = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(fields),
            MirType::semantic(array_ty),
            span,
        );
        Some((operand, array_ty))
    }

    fn lower_array_itera(&mut self, iter: ArrayIteraLowering<'_>) -> Option<()> {
        let Some(iter_ty) = iter.iter.ty else {
            self.errors
                .push(MirError::missing_type(iter.iter.span, "itera collection source"));
            return None;
        };
        // Enum-member iteration: a bare enum name as the
        // source of an `itera ex` names the enum's member series, not a
        // value. Materialize the declared unit variants as a `lista<Enum>`
        // temp so the ordinary list-iteration machinery below walks them in
        // declaration order.
        let enum_member_array = match self.enum_member_variants(iter.iter, iter.mode) {
            Some(variants) => Some(self.construct_enum_member_list(iter_ty, &variants, iter.iter.span)?),
            None => None,
        };
        // `itera ab` over a value of `intervallum` type is materialized by the
        // `Ab` arm into a `lista<T>` operand; the loop walks that array in
        // order instead of classifying the interval carrier.
        let interval_element = iter.materialized.as_ref().map(|(_, element)| *element);
        let collection_kind = match &enum_member_array {
            Some(_) => CollectionIteraKind::Array(iter_ty),
            None => match interval_element {
                Some(element) => CollectionIteraKind::Array(element),
                None => self.classify_collection_itera(iter_ty, iter.mode, iter.coordinates.len()),
            },
        };
        if matches!(&collection_kind, CollectionIteraKind::Unsupported) {
            // Only map-value iteration carries the tabula decline; tensor and
            // bounded-lista declines keep the generic iterator diagnostic.
            let is_map = matches!(self.normalized_type(iter_ty), Type::Map(_, _) | Type::Ledger(_, _));
            self.errors.push(MirError::unsupported(
                iter.iter.span,
                if matches!(iter.mode, HirIteraMode::Ex) && is_map {
                    "itera ex over tabula before map value iteration MIR lowering"
                } else {
                    "itera collection before iterator MIR lowering"
                },
            ));
            return None;
        }

        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        // A borrowed receiver types its loop-local collection by the payload
        // under the `Ref` marker, so a borrowed receiver and its by-value twin
        // lower byte-identically.
        // An enum-member source lowers to a materialized `lista<Enum>` temp,
        // so the loop-local collection is typed by that array; every other
        // source keeps the receiver's own (peeled) type.
        let collection_semantic = match &enum_member_array {
            Some((_, array_ty)) => *array_ty,
            None => match interval_element {
                Some(element) => self.require_array(element, iter.iter.span, "itera ab interval source list")?,
                None => match self.normalized_type(iter_ty) {
                    Type::Ref(_, inner) => *inner,
                    _ => iter_ty,
                },
            },
        };
        let collection = match enum_member_array {
            Some((operand, _)) => operand,
            None => match &iter.materialized {
                Some((operand, _)) => operand.clone(),
                None => self.lower_expr_value(iter.iter)?,
            },
        };
        // D6.6: a direct generator call feeding `itera ex` is walked stepwise,
        // so a suspending backend resumes the producer once per step.
        let stepped_callee = self.mark_stepped_cursor_stream(&collection, &collection_kind, iter.mode);
        let stepped = stepped_callee.is_some();
        let (collection, collection_ty) = match &collection_kind {
            CollectionIteraKind::Cursor(element) | CollectionIteraKind::AsyncCursor(element) if stepped => (
                collection,
                MirType::semantic(self.require_array(*element, iter.iter.span, "itera cursor element list")?),
            ),
            CollectionIteraKind::Text(element) => {
                let snapshot = self.text_scalar_snapshot(collection, *element, iter.iter.span)?;
                let array_ty =
                    MirType::semantic(self.require_array(*element, iter.iter.span, "itera text scalar list")?);
                (MirOperand::Place(snapshot), array_ty)
            }
            CollectionIteraKind::Cursor(element) | CollectionIteraKind::AsyncCursor(element) => {
                let array = self.require_array(*element, iter.iter.span, "itera cursor snapshot list")?;
                let array_ty = MirType::semantic(array);
                let snapshot = self.runtime_call_value(
                    MirIntrinsic::Sermo(radix_mir::MirSermoOp::DrainCursor),
                    vec![collection],
                    array_ty,
                    iter.iter.span,
                );
                (snapshot, array_ty)
            }
            _ => (collection, MirType::semantic(collection_semantic)),
        };
        // D6.6: a failable generator's stepped walk still owns an error edge
        // after the loop; the settle call reuses the ordinary failable-call
        // routing instead of hand-rolling an error block.
        let settle = stepped_callee.and_then(|callee| {
            self.context
                .function_errors
                .get(&callee)
                .copied()
                .map(|err_ty| (collection.clone(), err_ty))
        });
        let collection = self.materialize_operand_place(collection, collection_ty, iter.iter.span);
        if let CollectionIteraKind::Shaped { element, rank, literal, matrix } = &collection_kind {
            return self.lower_shaped_itera(&iter, collection, *element, *rank, literal.as_deref(), *matrix);
        }

        // The map itself stays addressable for the per-key value read.
        let entries_map =
            matches!(&collection_kind, CollectionIteraKind::MapEntries { .. }).then(|| collection.clone());
        // O1(3): a loop over a map whose body bracket-stores into that same
        // map remembers the size at entry and compares it at every step.
        let size_guard_map = (matches!(
            &collection_kind,
            CollectionIteraKind::MapKeys(_)
                | CollectionIteraKind::MapValues(_)
                | CollectionIteraKind::MapEntries { .. }
        ) && {
            let source = match &iter.iter.kind {
                HirExpressionKind::NonNull(inner, radix_hir::HirNonNullKind::Value) => inner,
                _ => iter.iter,
            };
            matches!(&source.kind, HirExpressionKind::Path(def) if body_bracket_stores_into(iter.body, *def))
        })
        .then(|| collection.clone());
        let (iteration_place, binding_ty, array_de) = match &collection_kind {
            CollectionIteraKind::Array(item) => {
                let item_ty = MirType::semantic(*item);
                let binding_ty = if matches!(iter.mode, HirIteraMode::De) {
                    numerus
                } else {
                    item_ty
                };
                (collection, binding_ty, matches!(iter.mode, HirIteraMode::De))
            }
            CollectionIteraKind::Queue(item) | CollectionIteraKind::Stack(item) => {
                (collection, MirType::semantic(*item), false)
            }
            CollectionIteraKind::MapKeys(key) => {
                let (snapshot_place, binding_ty) =
                    self.lower_map_itera_snapshot(collection, *key, MirCollectionOp::Keys, iter.iter.span)?;
                (snapshot_place, binding_ty, false)
            }
            CollectionIteraKind::SetElements(element) => {
                let (snapshot_place, binding_ty) =
                    self.lower_set_itera_snapshot(collection, collection_semantic, *element, iter.iter.span)?;
                (snapshot_place, binding_ty, false)
            }
            CollectionIteraKind::MapValues(value) => {
                let (snapshot_place, binding_ty) =
                    self.lower_map_itera_snapshot(collection, *value, MirCollectionOp::Values, iter.iter.span)?;
                (snapshot_place, binding_ty, false)
            }
            CollectionIteraKind::MapEntries { key, value } => {
                let (snapshot_place, _) =
                    self.lower_map_itera_snapshot(collection, *key, MirCollectionOp::Keys, iter.iter.span)?;
                (snapshot_place, MirType::semantic(*value), false)
            }
            CollectionIteraKind::Cursor(value) | CollectionIteraKind::AsyncCursor(value) => {
                (collection, MirType::semantic(*value), false)
            }
            CollectionIteraKind::Tensor(elem, _) => {
                // Element gather rides the generic Index projection over the
                // tensor place (every lane is defined by construction).
                (collection, MirType::semantic(*elem), false)
            }
            CollectionIteraKind::BoundedArray(elem, _) => (collection, MirType::semantic(*elem), false),
            CollectionIteraKind::Bytes(element) | CollectionIteraKind::Text(element) => {
                (collection, MirType::semantic(*element), false)
            }
            // Shaped itera returned through nested lowering above and
            // unsupported kinds failed closed before the collection lowered.
            CollectionIteraKind::Shaped { .. } | CollectionIteraKind::Unsupported => {
                self.errors.push(MirError::unsupported(
                    iter.iter.span,
                    "itera collection before iterator MIR lowering",
                ));
                return None;
            }
        };

        let size_guard = size_guard_map.map(|map_place| {
            let start = self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Length),
                vec![MirOperand::Place(map_place.clone())],
                numerus,
                iter.span,
            );
            (map_place, start)
        });
        if let CollectionIteraKind::BoundedArray(_, capacity) = &collection_kind {
            let text = matches!(
                self.normalized_type(collection_semantic),
                Type::BoundedAscii { .. } | Type::BoundedTextus { .. }
            );
            return self.lower_bounded_array_itera_split(&iter, iteration_place, binding_ty, *capacity, text);
        }

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: iter.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            iter.span,
        );

        // A stepped cursor re-reads its length at every condition check, so
        // the bound is lowered inside the condition block instead of here.
        // A conversation cursor (`tuus.cursor()`, A4/U9) is walked the same
        // way: its frames arrive while the loop runs, so the runner can pull
        // one frame per step from a cursor handler.
        let hoisted_length = if stepped || self.is_frame_cursor_walk(&collection_kind) {
            None
        } else if let CollectionIteraKind::Tensor(_, literal_lanes) = &collection_kind {
            let literal = literal_lanes.map(|lanes| [lanes]);
            let bounds =
                self.tensor_axis_bounds(&iteration_place, literal.as_ref().map(<[i64; 1]>::as_slice), 1, iter.span)?;
            bounds.into_iter().next()
        } else {
            Some(self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Length),
                vec![MirOperand::Place(iteration_place.clone())],
                numerus,
                iter.span,
            ))
        };
        if !self.current_is_open() {
            return None;
        }

        let binding = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: binding,
            name: Some(iter.name),
            ty: binding_ty,
            mutable: true,
            span: iter.span,
        });
        self.bindings
            .insert(iter.binding, LocalBinding { local: binding, ty: binding_ty });
        // `itera ex xs apud [i]` (D3.1): the position is the loop counter,
        // exactly the index `xs[i]` reads back.
        if let (CollectionIteraKind::Array(_), [(coordinate, _)]) = (&collection_kind, iter.coordinates) {
            self.bindings
                .insert(*coordinate, LocalBinding { local: index, ty: numerus });
        }

        let cond_id = self.fresh_block(iter.span);
        let body_id = self.fresh_block(iter.body.span);
        let increment_id = self.fresh_block(iter.span);
        let after_id = self.fresh_block(iter.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), iter.span);

        self.switch_to(cond_id);
        if let Some((map_place, start)) = size_guard {
            let current = self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Length),
                vec![MirOperand::Place(map_place)],
                numerus,
                iter.span,
            );
            let changed = self.binary_temp(MirBinOp::NotEq, current, start, bivalens, iter.span);
            let trap_id = self.fresh_block(iter.span);
            let step_id = self.fresh_block(iter.span);
            self.terminate_current(
                MirTerminatorKind::Branch { condition: changed, then_block: trap_id, else_block: step_id },
                iter.span,
            );
            self.switch_to(trap_id);
            let message = MirOperand::Constant(MirConstant::String(self.context.diagnostic_map_changed_size));
            let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
            self.append_stmt(MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall { intrinsic: MirIntrinsic::Panic, args: vec![message], return_ty: numquam },
                },
                span: iter.span,
            });
            self.terminate_current(MirTerminatorKind::Unreachable, iter.span);
            self.switch_to(step_id);
        }
        let length = match hoisted_length {
            Some(length) => length,
            None => self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Length),
                vec![MirOperand::Place(iteration_place.clone())],
                numerus,
                iter.span,
            ),
        };
        let condition = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length.clone(),
            bivalens,
            iter.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: body_id, else_block: after_id },
            iter.span,
        );

        self.switch_to(body_id);
        let stack_reverse = matches!(&collection_kind, CollectionIteraKind::Stack(_));
        let source_index = if stack_reverse {
            // The loop counter remains a zero-based visit count. Convert it to
            // the source index only for stacks, so the first visit reads
            // `length - 1` (the top) and subsequent visits walk toward zero.
            let from_end = self.binary_temp(
                MirBinOp::Sub,
                length.clone(),
                MirOperand::Place(MirPlace::local(index)),
                numerus,
                iter.span,
            );
            self.binary_temp(
                MirBinOp::Sub,
                from_end,
                MirOperand::Constant(MirConstant::Int(1)),
                numerus,
                iter.span,
            )
        } else {
            MirOperand::Place(MirPlace::local(index))
        };
        let value = if let (CollectionIteraKind::MapEntries { key, .. }, Some(map_place)) =
            (&collection_kind, entries_map.clone())
        {
            let mut key_place = iteration_place;
            key_place
                .projections
                .push(MirProjection::Index(source_index));
            self.lower_map_entry_key_binding(&iter, *key, key_place, map_place)
        } else if matches!(&collection_kind, CollectionIteraKind::Queue(_) | CollectionIteraKind::Stack(_)) {
            // Queue/stack carriers intentionally remain opaque MIR handles for
            // method access. Read their array-backed runtime value through the
            // collection index intrinsic rather than an index projection, whose
            // representation contract is reserved for physical arrays.
            self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Index),
                vec![MirOperand::Place(iteration_place.clone()), source_index],
                binding_ty,
                iter.span,
            )
        } else if array_de {
            source_index
        } else {
            let mut indexed = iteration_place;
            indexed.projections.push(MirProjection::Index(source_index));
            MirOperand::Place(indexed)
        };
        self.assign(MirPlace::local(binding), value, binding_ty, iter.span);
        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        self.visit_block(iter.body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), iter.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            iter.span,
        );
        self.assign(MirPlace::local(index), next, numerus, iter.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), iter.span);

        self.switch_to(after_id);
        if let Some((operand, err_ty)) = settle {
            let unit = MirType::semantic(self.types.primitive(Primitive::Vacuum));
            self.lower_failable_call(MirCallee::Value(operand), Vec::new(), unit, err_ty, None, iter.span)?;
        }
        Some(())
    }

    /// Lower the N-ary lockstep `itera ex` walk.
    ///
    /// One aligned index walk: every source lowers to a place, the shared
    /// length comes from the first source, and each further source's length
    /// must equal it before the loop runs — the admission fails closed (a
    /// panic terminator, mirroring `mori`) and the walk never truncates to
    /// the minimum. Statically known mismatches already failed at check.
    /// The optional `apud [i]` coordinate names the shared index local.
    pub(super) fn lower_itera_lockstep_expr(
        &mut self,
        sources: &[HirExpression],
        coordinates: &[(DefId, Symbol)],
        bindings: &[(DefId, Symbol)],
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "itera expression with non-vacuum result"));
            return None;
        }
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        // Materialize each source place with its element type; a non-lista
        // source is impossible after typecheck, so decline fail-closed rather
        // than inventing an element row here.
        let mut collections = Vec::with_capacity(sources.len());
        for source in sources {
            let Some(source_ty) = source.ty else {
                self.errors
                    .push(MirError::missing_type(source.span, "itera lockstep collection source"));
                return None;
            };
            let semantic = match self.normalized_type(source_ty) {
                Type::Ref(_, inner) => *inner,
                _ => source_ty,
            };
            let element = match self.types.get(semantic) {
                Type::Array(inner)
                | Type::BoundedArray { element: inner, .. }
                | Type::RangedArray { element: inner, .. } => *inner,
                _ => {
                    self.errors.push(MirError::unsupported(
                        source.span,
                        "itera lockstep source before lista MIR lowering",
                    ));
                    return None;
                }
            };
            let collection = self.lower_expr_value(source)?;
            let collection_ty = MirType::semantic(semantic);
            let place = self.materialize_operand_place(collection, collection_ty, source.span);
            collections.push((place, MirType::semantic(element)));
        }

        // Fail-closed equal-length admission: guard every further source
        // against the first length before any loop block exists.
        let first_length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collections[0].0.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }
        let mismatch_message =
            MirOperand::Constant(MirConstant::String(self.context.diagnostic_lockstep_length_mismatch));
        for (place, _) in &collections[1..] {
            let length = self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Length),
                vec![MirOperand::Place(place.clone())],
                numerus,
                expr.span,
            );
            let equal = self.binary_temp(MirBinOp::Eq, first_length.clone(), length, bivalens, expr.span);
            let ok_id = self.fresh_block(expr.span);
            let mismatch_id = self.fresh_block(expr.span);
            self.terminate_current(
                MirTerminatorKind::Branch { condition: equal, then_block: ok_id, else_block: mismatch_id },
                expr.span,
            );
            self.switch_to(mismatch_id);
            let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
            self.append_stmt(MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Panic,
                        args: vec![mismatch_message.clone()],
                        return_ty: numquam,
                    },
                },
                span: expr.span,
            });
            self.terminate_current(MirTerminatorKind::Unreachable, expr.span);
            self.switch_to(ok_id);
        }

        // The shared index local doubles as the `apud [i]` coordinate when
        // one is present; without a clause it stays a synthetic counter.
        let index = self.next_local_id();
        let coordinate_name = coordinates.first().map(|(_, name)| *name);
        self.locals.push(MirLocalDecl {
            id: index,
            name: coordinate_name,
            ty: numerus,
            mutable: true,
            span: expr.span,
        });
        if let Some((coordinate_def, _)) = coordinates.first() {
            self.bindings
                .insert(*coordinate_def, LocalBinding { local: index, ty: numerus });
        }
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let mut binder_locals = Vec::with_capacity(bindings.len());
        for ((binding_def, binding_name), (_, element_ty)) in bindings.iter().zip(collections.iter()) {
            let binding = self.next_local_id();
            self.locals.push(MirLocalDecl {
                id: binding,
                name: Some(*binding_name),
                ty: *element_ty,
                mutable: true,
                span: expr.span,
            });
            self.bindings
                .insert(*binding_def, LocalBinding { local: binding, ty: *element_ty });
            binder_locals.push((binding, *element_ty));
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(body.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let condition = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            first_length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        for ((place, _), (binder_local, binder_ty)) in collections.iter().zip(binder_locals.iter()) {
            let mut indexed = place.clone();
            indexed
                .projections
                .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
            self.assign(
                MirPlace::local(*binder_local),
                MirOperand::Place(indexed),
                *binder_ty,
                expr.span,
            );
        }
        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        self.visit_block(body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Lower one shaped `apud` loop as rank-N nested CFG loops.
    ///
    /// Tensor element reads use the runtime `TensorGet` ABI with the current
    /// coordinate locals as its index list. Matrix values are register-class
    /// aggregates, so their dynamic coordinate pair is switched to the
    /// corresponding static `MatrixCell` projection before the shared body.
    fn lower_shaped_itera(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        element: TypeId,
        rank: usize,
        literal: Option<&[i64]>,
        matrix: bool,
    ) -> Option<()> {
        if rank == 0 || rank != iter.coordinates.len() || (matrix && literal.is_none()) {
            self.errors.push(MirError::unsupported(
                iter.iter.span,
                "itera shaped collection dimensions before nested loop MIR lowering",
            ));
            return None;
        }

        let element_ty = MirType::semantic(element);
        let binding = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: binding,
            name: Some(iter.name),
            ty: element_ty,
            mutable: true,
            span: iter.span,
        });
        self.bindings
            .insert(iter.binding, LocalBinding { local: binding, ty: element_ty });

        // Bounds are read once, before the outermost loop: a tensor's shape
        // cannot change while it is walked.
        let bounds = self.tensor_axis_bounds(&collection, literal, rank, iter.span)?;
        let matrix_dimensions = if matrix { literal } else { None };
        let after_id = self.fresh_block(iter.span);
        self.lower_shaped_level(iter, collection, element, binding, &bounds, matrix_dimensions, 0, after_id)?;
        self.switch_to(after_id);
        Some(())
    }

    fn lower_shaped_level(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        element: TypeId,
        binding: MirLocalId,
        bounds: &[MirOperand],
        matrix: Option<&[i64]>,
        level: usize,
        continuation: MirBlockId,
    ) -> Option<()> {
        let (coordinate_def, coordinate_name) = iter.coordinates[level];
        let numerus = MirType::semantic(
            coordinate_type(iter.body, coordinate_def)
                .or_else(|| {
                    iter.coordinates
                        .iter()
                        .find_map(|(def, _)| coordinate_type(iter.body, *def))
                })
                .unwrap_or_else(|| self.types.primitive(Primitive::Numerus)),
        );
        let coordinate = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: coordinate,
            name: Some(coordinate_name),
            ty: numerus,
            mutable: true,
            span: iter.span,
        });
        self.bindings
            .insert(coordinate_def, LocalBinding { local: coordinate, ty: numerus });
        self.assign(
            MirPlace::local(coordinate),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            iter.span,
        );

        let condition_id = self.fresh_block(iter.span);
        let body_id = self.fresh_block(iter.body.span);
        let increment_id = self.fresh_block(iter.span);
        let loop_after_id = self.fresh_block(iter.span);
        self.terminate_current(MirTerminatorKind::Goto(condition_id), iter.span);

        self.switch_to(condition_id);
        let condition = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(coordinate)),
            bounds[level].clone(),
            MirType::semantic(self.types.primitive(Primitive::Bivalens)),
            iter.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: body_id, else_block: loop_after_id },
            iter.span,
        );

        self.switch_to(body_id);
        if level + 1 < bounds.len() {
            self.lower_shaped_level(iter, collection, element, binding, bounds, matrix, level + 1, increment_id)?;
        } else if let Some(dimensions) = matrix {
            self.lower_shaped_matrix_body(
                iter,
                collection,
                element,
                binding,
                coordinate,
                iter.coordinates[level - 1].0,
                dimensions,
                increment_id,
                loop_after_id,
            )?;
        } else {
            self.lower_shaped_tensor_body(iter, collection, element, binding, increment_id, loop_after_id)?;
        }

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(coordinate)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            iter.span,
        );
        self.assign(MirPlace::local(coordinate), next, numerus, iter.span);
        self.terminate_current(MirTerminatorKind::Goto(condition_id), iter.span);

        self.switch_to(loop_after_id);
        self.terminate_open_current(MirTerminatorKind::Goto(continuation), iter.span);
        Some(())
    }

    fn lower_shaped_tensor_body(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        element: TypeId,
        binding: MirLocalId,
        increment_id: MirBlockId,
        loop_after_id: MirBlockId,
    ) -> Option<()> {
        let numerus = self.bindings.get(&iter.coordinates.first()?.0)?.ty;
        let index_array = self.require_array(numerus.semantic_id(), iter.iter.span, "shaped itera coordinate list")?;
        let mut coordinate_operands = Vec::with_capacity(iter.coordinates.len());
        for (def_id, _) in iter.coordinates {
            let Some(binding) = self.bindings.get(def_id) else {
                self.errors
                    .push(MirError::validation(iter.iter.span, "shaped itera coordinate has no binding"));
                return None;
            };
            coordinate_operands.push(MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(binding.local))));
        }
        let index_list = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(coordinate_operands),
            MirType::semantic(index_array),
            iter.span,
        );
        let Some(option_ty) = self.types.find_equal(&Type::Option(element)) else {
            self.errors
                .push(MirError::missing_type(iter.iter.span, "shaped itera element option"));
            return None;
        };
        let option = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorGet),
            vec![MirOperand::Place(collection), index_list],
            MirType::semantic(option_ty),
            iter.iter.span,
        );
        if !self.current_is_open() {
            return None;
        }
        let value = self.assign_temp(
            MirValueKind::Option(MirOptionOp::Unwrap { value: option, mode: MirOptionUnwrapMode::Assume }),
            MirType::semantic(element),
            iter.span,
        );
        self.assign(MirPlace::local(binding), value, MirType::semantic(element), iter.span);
        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: loop_after_id,
            rumpe_used: false,
        });
        self.visit_block(iter.body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), iter.span);
        Some(())
    }

    fn lower_shaped_matrix_body(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        element: TypeId,
        binding: MirLocalId,
        column: MirLocalId,
        row_def: DefId,
        dimensions: &[i64],
        increment_id: MirBlockId,
        loop_after_id: MirBlockId,
    ) -> Option<()> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let Some(row) = self.bindings.get(&row_def).map(|binding| binding.local) else {
            self.errors
                .push(MirError::validation(iter.iter.span, "matrix row coordinate has no binding"));
            return None;
        };
        let rows = u8::try_from(dimensions[0]).ok()?;
        let columns = u8::try_from(dimensions[1]).ok()?;
        let row_stride = self.binary_temp(
            MirBinOp::Mul,
            MirOperand::Place(MirPlace::local(row)),
            MirOperand::Constant(MirConstant::Int(i64::from(columns))),
            numerus,
            iter.span,
        );
        let linear = self.binary_temp(
            MirBinOp::Add,
            row_stride,
            MirOperand::Place(MirPlace::local(column)),
            numerus,
            iter.span,
        );
        let body_id = self.fresh_block(iter.body.span);
        let cases = (0..u16::from(rows) * u16::from(columns))
            .map(|offset| MirSwitchCase {
                value: MirConstant::Int(i64::from(offset)),
                target: self.fresh_block(iter.span),
            })
            .collect::<Vec<_>>();
        let cell_blocks = cases.iter().map(|case| case.target).collect::<Vec<_>>();
        self.terminate_current(
            MirTerminatorKind::Switch { value: linear, cases, default: increment_id },
            iter.span,
        );
        for (offset, cell_id) in cell_blocks.into_iter().enumerate() {
            let matrix_row = u8::try_from(offset / usize::from(columns)).ok()?;
            let matrix_column = u8::try_from(offset % usize::from(columns)).ok()?;
            self.switch_to(cell_id);
            let mut cell = collection.clone();
            cell.projections
                .push(MirProjection::MatrixCell { row: matrix_row, column: matrix_column });
            self.assign(
                MirPlace::local(binding),
                MirOperand::Place(cell),
                MirType::semantic(element),
                iter.span,
            );
            self.terminate_current(MirTerminatorKind::Goto(body_id), iter.span);
        }
        self.switch_to(body_id);
        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: loop_after_id,
            rumpe_used: false,
        });
        self.visit_block(iter.body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), iter.span);
        Some(())
    }

    /// Route B (AEU-4): lower `itera ex` over a runtime-len `lista<T, magnitudo N>` to
    /// a base-lane static split — a switch on the carrier `len` with N+1
    /// literal arms, arm k being a guard-free unroll of k lanes over 0..k.
    ///
    /// Each arm touches only `0..k ⊆ 0..len`, so no dead slot (i ≥ len) is
    /// ever read. The branch is base-lane MIR; the AIR lane never admits the
    /// loop (`air_unroll` declines runtime-len values `not_concrete`), so
    /// reverse AD never sees a non-literal scrutinee.
    /// The scalar buffer lane element — bare and bounded `octeti` bind
    /// `octetus` (`numerus<u8>`), `ascii<N>` / `textus<N>` bind `littera`
    /// (D10.1). The typecheck `itera` arm interns the byte row for the loop
    /// being lowered; a miss fails closed into Unsupported.
    fn scalar_buffer_element(&self, ty: &Type) -> Option<TypeId> {
        match ty {
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => self
                .types
                .find_sized_numeric(Primitive::Numerus, NumericWidth::U8),
            Type::BoundedAscii { .. } | Type::BoundedTextus { .. } => Some(self.types.primitive(Primitive::Littera)),
            _ => None,
        }
    }

    /// D10.1 text walk: snapshot `text` once through the cord cursor into a
    /// `lista<littera>` (one Unicode scalar per cell, no non-ASCII trap). The
    /// checker interned `cursor<littera>` / `lista<littera>` for the loop
    /// (`itera_element`).
    fn text_scalar_snapshot(&mut self, text: MirOperand, element: TypeId, span: Span) -> Option<MirPlace> {
        let Some(cursor) = self.types.find_equal(&Type::Cursor(element)) else {
            self.errors
                .push(MirError::missing_type(span, "itera text scalar cursor"));
            return None;
        };
        let cursor_ty = MirType::semantic(cursor);
        let array_ty = MirType::semantic(self.require_array(element, span, "itera text scalar list")?);
        let cursor = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::CordCursor),
            vec![text],
            cursor_ty,
            span,
        );
        let snapshot = self.runtime_call_value(
            MirIntrinsic::Sermo(radix_mir::MirSermoOp::DrainCursor),
            vec![cursor],
            array_ty,
            span,
        );
        Some(self.materialize_operand_place(snapshot, array_ty, span))
    }

    fn lower_bounded_array_itera_split(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        binding_ty: MirType,
        capacity: i64,
        text: bool,
    ) -> Option<()> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        // Runtime `len` read from the ListaN carrier — the split scrutinee.
        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            iter.iter.span,
        );
        if !self.current_is_open() {
            return None;
        }
        // D10.1: `ascii<N>` / `textus<N>` lanes bind `littera`. The live
        // prefix is snapshot once through the cord cursor into a
        // `lista<littera>`, so a lane reads one Unicode scalar with no
        // non-ASCII trap (the `textus<N>[i]` row keeps its trap under F5).
        let reads = if text {
            self.text_scalar_snapshot(MirOperand::Place(collection), binding_ty.semantic_id(), iter.iter.span)?
        } else {
            collection
        };

        let binding = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: binding,
            name: Some(iter.name),
            ty: binding_ty,
            mutable: true,
            span: iter.span,
        });
        self.bindings
            .insert(iter.binding, LocalBinding { local: binding, ty: binding_ty });

        let after_id = self.fresh_block(iter.span);
        let mut cases = Vec::new();
        for k in 0..=capacity {
            cases.push(MirSwitchCase { value: MirConstant::Int(k), target: self.fresh_block(iter.span) });
        }
        let arm_blocks = cases.iter().map(|case| case.target).collect::<Vec<_>>();
        self.terminate_current(
            MirTerminatorKind::Switch { value: length, cases, default: after_id },
            iter.iter.span,
        );

        let mut reaches_after = false;
        for (k, arm_id) in arm_blocks.into_iter().enumerate() {
            self.switch_to(arm_id);
            for lane in 0..k as i64 {
                // Arm k reads only lanes 0..k ⊆ 0..len — never a dead slot.
                let mut indexed = reads.clone();
                indexed
                    .projections
                    .push(MirProjection::Index(MirOperand::Constant(MirConstant::Int(lane))));
                self.assign(MirPlace::local(binding), MirOperand::Place(indexed), binding_ty, iter.span);
                let lane_end = self.fresh_block(iter.span);
                self.breakables.push(BreakableContext {
                    perge_target: Some(lane_end),
                    rumpe_target: after_id,
                    rumpe_used: false,
                });
                self.visit_block(iter.body);
                let Some(ctx) = self.breakables.pop() else {
                    self.errors.push(MirError::validation(
                        iter.span,
                        "breakable context popped before its body finished",
                    ));
                    return None;
                };
                reaches_after |= ctx.rumpe_used;
                self.terminate_open_current(MirTerminatorKind::Goto(lane_end), iter.span);
                self.switch_to(lane_end);
            }
            reaches_after |= self.terminate_open_current(MirTerminatorKind::Goto(after_id), iter.span);
        }
        if reaches_after {
            self.switch_to(after_id);
        } else {
            self.seal_unreachable(after_id, iter.span);
        }
        Some(())
    }

    /// Bind one `apud` map entry (D3.1/D3.3): the key read from the key
    /// snapshot lands in the single `apud` name, or — for an `iuncta` key
    /// with one name per part — in a hidden key local whose parts project
    /// into the names. Returns the value read back as `map[key]`.
    fn lower_map_entry_key_binding(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        key: TypeId,
        key_read: MirPlace,
        map_place: MirPlace,
    ) -> MirOperand {
        let key_ty = MirType::semantic(key);
        let key_local = self.next_local_id();
        // WHY: typecheck types each `apud` name as one key part whenever the
        // key is a tuple, so a one-part `iuncta` key must destructure like the
        // N-part arm instead of binding the whole tuple to a part-typed name.
        let key_is_tuple = matches!(self.normalized_type(key), Type::Tuple(_));
        let single = match iter.coordinates {
            [(coordinate, name)] if !key_is_tuple => Some((*coordinate, *name)),
            _ => None,
        };
        self.locals.push(MirLocalDecl {
            id: key_local,
            name: single.map(|(_, name)| name),
            ty: key_ty,
            mutable: true,
            span: iter.span,
        });
        self.assign(MirPlace::local(key_local), MirOperand::Place(key_read), key_ty, iter.span);
        if let Some((coordinate, _)) = single {
            self.bindings
                .insert(coordinate, LocalBinding { local: key_local, ty: key_ty });
        } else {
            let parts = match self.normalized_type(key) {
                Type::Tuple(parts) => parts.clone(),
                _ => Vec::new(),
            };
            for (position, ((coordinate, name), part)) in iter.coordinates.iter().zip(parts).enumerate() {
                let part_ty = MirType::semantic(part);
                let part_local = self.next_local_id();
                self.locals.push(MirLocalDecl {
                    id: part_local,
                    name: Some(*name),
                    ty: part_ty,
                    mutable: true,
                    span: iter.span,
                });
                let mut part_read = MirPlace::local(key_local);
                part_read
                    .projections
                    .push(MirProjection::Index(MirOperand::Constant(MirConstant::Int(
                        i64::try_from(position).unwrap_or(i64::MAX),
                    ))));
                self.assign(MirPlace::local(part_local), MirOperand::Place(part_read), part_ty, iter.span);
                self.bindings
                    .insert(*coordinate, LocalBinding { local: part_local, ty: part_ty });
            }
        }
        let mut value_read = map_place;
        value_read
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(key_local))));
        MirOperand::Place(value_read)
    }

    /// A `copia` walk reads a `copia ↦ lista<T>` snapshot of the elements, the
    /// same conversion a written `↦ lista<T>` lowers to, so the loop shares
    /// the array lane and the runner's unordered element order (D19.2).
    fn lower_set_itera_snapshot(
        &mut self,
        collection: MirPlace,
        set_ty: TypeId,
        element: TypeId,
        span: Span,
    ) -> Option<(MirPlace, MirType)> {
        let element_ty = MirType::semantic(element);
        let Some(array) = self.types.find_array(element) else {
            self.errors
                .push(MirError::missing_type(span, "copia element list snapshot"));
            return None;
        };
        let array_ty = MirType::semantic(array);
        let snapshot = self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty: MirType::semantic(set_ty),
                target_ty: array_ty,
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![MirOperand::Place(collection)],
            array_ty,
            span,
        );
        if !self.current_is_open() {
            return None;
        }
        let snapshot_place = self.materialize_operand_place(snapshot, array_ty, span);
        Some((snapshot_place, element_ty))
    }

    fn lower_map_itera_snapshot(
        &mut self,
        collection: MirPlace,
        element: TypeId,
        op: MirCollectionOp,
        span: Span,
    ) -> Option<(MirPlace, MirType)> {
        debug_assert!(matches!(op, MirCollectionOp::Keys | MirCollectionOp::Values));
        let element_ty = MirType::semantic(element);
        let array_id = self.require_array(element, span, "itera map snapshot list")?;
        let array_ty = MirType::semantic(array_id);
        let snapshot = self.runtime_call_value(
            MirIntrinsic::Collection(op),
            vec![MirOperand::Place(collection)],
            array_ty,
            span,
        );
        if !self.current_is_open() {
            return None;
        }
        let snapshot_place = self.materialize_operand_place(snapshot, array_ty, span);
        Some((snapshot_place, element_ty))
    }

    pub(super) fn materialize_operand_place(&mut self, operand: MirOperand, ty: MirType, span: Span) -> MirPlace {
        match operand {
            MirOperand::Place(place) => place,
            MirOperand::Temp(temp) => MirPlace::temp(temp),
            other => {
                let temp = self.push_temp(ty, span);
                self.assign(MirPlace::temp(temp), other, ty, span);
                MirPlace::temp(temp)
            }
        }
    }

    /// The index/bound/step type of a range walk (ruling 43): the binder is
    /// typed by the join of the bounds, so an `inf` bound gives an `inf`
    /// index (its `+ step` is exact, never the 64-bit trap) and bounded
    /// bounds keep today's `numerus`.
    fn range_index_ty(&self, bounds: &[Option<&HirExpression>]) -> MirType {
        bounds
            .iter()
            .flatten()
            .filter_map(|bound| bound.ty)
            .map(|ty| self.types.peel_access_marker(ty).1)
            .find(|&ty| radix_types::numeric_width::is_unbounded_integer_type(self.types, ty))
            .map_or_else(
                || MirType::semantic(self.types.primitive(Primitive::Numerus)),
                MirType::semantic,
            )
    }

    /// The body-visible local of a range binder: the counter itself for a
    /// `fixum` binder; for a `varia` binder a fresh local that every
    /// iteration copies the counter into (ruling 2026-10-02), so a write to
    /// the binder never steers the loop.
    fn range_binder_local(
        &mut self,
        counter: MirLocalId,
        mutable: bool,
        name: Symbol,
        ty: MirType,
        span: Span,
    ) -> MirLocalId {
        if !mutable {
            return counter;
        }
        let binder = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: binder, name: Some(name), ty, mutable: true, span });
        binder
    }

    /// Refresh a `varia` range binder from the loop counter at the top of an
    /// iteration; a `fixum` binder is the counter and needs no copy.
    fn copy_range_binder(&mut self, binder: MirLocalId, counter: MirLocalId, ty: MirType, span: Span) {
        if binder != counter {
            self.assign(MirPlace::local(binder), MirOperand::Place(MirPlace::local(counter)), ty, span);
        }
    }

    /// Turn an explicit range step into the signed stride the walk adds.
    ///
    /// The written step is a positive stride (ruling 2026-10-02): zero or
    /// negative traps at run time (a literal one is a compile error), and
    /// the direction comes from the bounds alone, so a descending range
    /// negates the stride once, before the walk. Ends by jumping to `exit`.
    fn lower_range_stride(
        &mut self,
        start: MirLocalId,
        end: MirLocalId,
        step: MirLocalId,
        exit: MirBlockId,
        bivalens: MirType,
        index_ty: MirType,
        span: Span,
    ) {
        let positive = self.binary_temp(
            MirBinOp::Gt,
            MirOperand::Place(MirPlace::local(step)),
            MirOperand::Constant(MirConstant::Int(0)),
            bivalens,
            span,
        );
        let direction_id = self.fresh_block(span);
        let trap_id = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: positive, then_block: direction_id, else_block: trap_id },
            span,
        );

        self.switch_to(trap_id);
        let message = MirOperand::Constant(MirConstant::String(self.context.diagnostic_range_step_not_positive));
        let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall { intrinsic: MirIntrinsic::Panic, args: vec![message], return_ty: numquam },
            },
            span,
        });
        self.terminate_current(MirTerminatorKind::Unreachable, span);

        self.switch_to(direction_id);
        let ascends = self.binary_temp(
            MirBinOp::LtEq,
            MirOperand::Place(MirPlace::local(start)),
            MirOperand::Place(MirPlace::local(end)),
            bivalens,
            span,
        );
        let descend_id = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: ascends, then_block: exit, else_block: descend_id },
            span,
        );

        self.switch_to(descend_id);
        let negated = self.assign_temp(
            MirValueKind::Unary { op: MirUnOp::Neg, operand: MirOperand::Place(MirPlace::local(step)) },
            index_ty,
            span,
        );
        self.assign(MirPlace::local(step), negated, index_ty, span);
        self.terminate_current(MirTerminatorKind::Goto(exit), span);
    }

    fn lower_default_range_step(
        &mut self,
        index: MirLocalId,
        end: MirLocalId,
        step: MirLocalId,
        cond_id: MirBlockId,
        bivalens: MirType,
        index_ty: MirType,
        span: Span,
    ) {
        let positive_step_id = self.fresh_block(span);
        let negative_step_id = self.fresh_block(span);
        let ascends = self.binary_temp(
            MirBinOp::LtEq,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Place(MirPlace::local(end)),
            bivalens,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: ascends,
                then_block: positive_step_id,
                else_block: negative_step_id,
            },
            span,
        );

        self.switch_to(positive_step_id);
        self.assign(MirPlace::local(step), MirOperand::Constant(MirConstant::Int(1)), index_ty, span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(negative_step_id);
        self.assign(
            MirPlace::local(step),
            MirOperand::Constant(MirConstant::Int(-1)),
            index_ty,
            span,
        );
        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);
    }

    pub(super) fn binary_temp(
        &mut self,
        op: MirBinOp,
        lhs: MirOperand,
        rhs: MirOperand,
        ty: MirType,
        span: Span,
    ) -> MirOperand {
        self.assign_temp(MirValueKind::Binary { op, lhs, rhs }, ty, span)
    }

    /// Emit one device fault-delivery marker (a value-less runtime call).
    pub(super) fn emit_device_marker(&mut self, op: radix_mir::MirDeviceBlockOp, args: Vec<MirOperand>, span: Span) {
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));
        self.runtime_call_to_place(None, MirIntrinsic::DeviceBlock(op), args, vacuum, span);
    }

    /// Open a handled block: its launches are tagged with it at run time.
    /// No-op unless the unit declares a `⇥` kernel.
    fn open_device_block(&mut self, omnia: bool, handler: MirBlockId, error_place: MirPlace, span: Span) {
        if !self.context.fault_delivery {
            return;
        }
        self.emit_device_marker(
            radix_mir::MirDeviceBlockOp::Enter { omnia, handler },
            vec![MirOperand::Place(error_place)],
            span,
        );
        self.device_blocks
            .push(DeviceBlockContext { breakables_len: self.breakables.len(), omnia });
    }

    /// Close the innermost handled block on its fall-through edge.
    fn close_device_block(&mut self, span: Span) {
        if self.device_blocks.pop().is_some() && self.current_is_open() {
            self.emit_device_marker(radix_mir::MirDeviceBlockOp::Exit { error_edge: false }, Vec::new(), span);
        }
    }

    /// Mark the handler-entry edge: an error already chose the handler.
    fn emit_device_handler_exit(&mut self, span: Span) {
        if self.context.fault_delivery {
            self.emit_device_marker(radix_mir::MirDeviceBlockOp::Exit { error_edge: true }, Vec::new(), span);
        }
    }

    /// Close every open block that a jump to a breakable pushed at depth
    /// `breakables_len` leaves, innermost first (`0`: all of them, for
    /// `redde`).
    pub(super) fn emit_device_exits_to(&mut self, breakables_len: usize, span: Span) {
        let leaving = self
            .device_blocks
            .iter()
            .rev()
            .take_while(|block| block.breakables_len >= breakables_len)
            .count();
        for _ in 0..leaving {
            self.emit_device_marker(radix_mir::MirDeviceBlockOp::Exit { error_edge: false }, Vec::new(), span);
        }
    }

    /// Cheap delivery check after a statement of an omnia body.
    pub(super) fn deliver_after_statement(&mut self, span: Span) {
        if self.device_blocks.last().is_some_and(|block| block.omnia) && self.current_is_open() {
            self.emit_device_marker(radix_mir::MirDeviceBlockOp::Deliver { sync: false }, Vec::new(), span);
        }
    }

    /// Lower `rumpe` to the innermost breakable scope exit edge.
    pub(super) fn lower_rumpe(&mut self, span: Span) {
        let Some(context) = self.breakables.last_mut() else {
            self.errors
                .push(MirError::unsupported(span, "rumpe without an enclosing breakable block"));
            return;
        };
        context.rumpe_used = true;
        let target = context.rumpe_target;
        let target_len = self.breakables.len();
        self.emit_device_exits_to(target_len, span);
        self.terminate_current(MirTerminatorKind::Goto(target), span);
    }

    /// Lower `perge` to the active loop's head edge.
    pub(super) fn lower_perge(&mut self, span: Span) {
        let Some(context) = self.breakables.last().copied() else {
            self.errors
                .push(MirError::unsupported(span, "perge without an active loop"));
            return;
        };
        let Some(perge_target) = context.perge_target else {
            self.errors
                .push(MirError::unsupported(span, "perge outside a loop"));
            return;
        };
        // A breakable `fac` block shares its loop's `perge` target; the loop is
        // the outermost context with that target.
        let mut loop_index = self.breakables.len() - 1;
        while loop_index > 0 && self.breakables[loop_index - 1].perge_target == Some(perge_target) {
            loop_index -= 1;
        }
        self.emit_device_exits_to(loop_index + 1, span);
        self.terminate_current(MirTerminatorKind::Goto(perge_target), span);
    }

    /// Lower a HIR block marked [`HirBlock::breakable`].
    pub(super) fn lower_breakable_block(&mut self, block: &HirBlock, span: Span) {
        if block.breakable.is_none() {
            self.visit_block_statements(block);
            return;
        }

        let after_id = self.fresh_block(span);
        let exit_id = after_id;

        // Lower the body into the current (predecessor) block. Body fall-through
        // and `rumpe` both goto `exit_id`. This must run before the epilogue is
        // built so the predecessor stays the active cursor and body statements
        // are not emitted into a sealed block.
        // Ruling R3: a breakable block is not a loop. `rumpe` stops here, but
        // `perge` passes through to the enclosing loop's continue edge (or
        // stays unset when no loop encloses the block in this function).
        let perge_target = self.breakables.last().and_then(|outer| outer.perge_target);
        let context = BreakableContext { perge_target, rumpe_target: exit_id, rumpe_used: false };
        self.breakables.push(context);
        self.visit_block_statements(block);
        // WHY: read back the context so `rumpe_used` reflects any `rumpe` that
        // fired while lowering the body. The body may have pushed and popped
        // nested breakable scopes, so a copy taken upfront would go stale.
        let Some(context) = self.breakables.pop() else {
            return;
        };
        let body_reaches = self.terminate_open_current(MirTerminatorKind::Goto(exit_id), span);

        // The after-block is reachable if the body's straight-line fall-through
        // reached it, or if any `rumpe` targeted this scope's exit edge. Keep it
        // live only then; otherwise leave current sealed so the enclosing
        // builder does not lower subsequent code (or an implicit function
        // return) into a block with no predecessors. A truly dead after-block
        // is sealed by the finish pass.
        if body_reaches || context.rumpe_used {
            self.switch_to(after_id);
        }
    }

    fn visit_block_statements(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            if !self.current_is_open() {
                break;
            }
            self.visit_stmt(stmt);
        }
        if let Some(expr) = &block.expr
            && self.current_is_open()
        {
            self.visit_expr(expr);
        }
    }
}

/// Whether `cond` is the literal boolean `true`.
fn hir_cond_is_literal_true(expr: &HirExpression) -> bool {
    matches!(&expr.kind, HirExpressionKind::Literal(radix_hir::HirLiteral::Bool(true)))
}

/// Whether a handled body is a `fac omnia` or `fac machina` block or loop.
/// V0 lowers `machina` exactly as `omnia`.
fn handled_body_is_omnia(body: &HirBlock) -> bool {
    let omnia_block = |block: &HirBlock| {
        block
            .breakable
            .is_some_and(radix_hir::HirBreakableKind::defers_faults)
    };
    omnia_block(body)
        || matches!(
            body.expr.as_deref().map(|expr| &expr.kind),
            Some(HirExpressionKind::Block(inner) | HirExpressionKind::Loop(inner)) if omnia_block(inner)
        )
}
