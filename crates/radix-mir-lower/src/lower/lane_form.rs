//! Lane-form lowering of a top-level `thread lane` loop (gpu-reset C4-0).
//!
//! Under `KernelLowering::Device`, a compute body's one top-level
//! `for from t at [i, j] thread lane const v { ... }` loop is not a CFG loop:
//! the loop *is* the launch grid. The function takes one compiler-internal
//! `Lane` builtin parameter (the 1-D global invocation id) and the loop
//! lowers to the lane form:
//!
//! ```text
//! E_k   = TensorExtent(src, k)            (constants for literal axes)
//! if lane >= E_0 * E_1 * ... { return }   (the grid guard)
//! j = lane % E_1 ; i = lane / E_1          (row-major binders)
//! v = src[lane]                            (Index place, no TensorGet)
//! <body, inline>
//! ```
//!
//! The other admitted region is a top-level whole-grid reduction (gpu-reset
//! C4-8): `dest ← reduce via Op from t at [i, j] thread lane const v { ... }
//! coalesce d`. It lowers to the same `Lane` parameter and extent temps, then
//! ONE `FilumReduce` node (form `Grid`) and no grid guard: every lane must
//! reach the node's barriers, and an out-of-range lane contributes the
//! identity. The destination store and anything after the node stay ordinary
//! MIR.
//!
//! Host kernel lowering never reaches this module: the host form stays the
//! untouched sequential oracle. A kernel with a second top-level lane loop,
//! or any statement after the loop, depends on a whole-grid barrier that has
//! no launch split yet and fails closed with a named issue.

use super::control::CollectionIteraKind;
use super::{
    BreakableContext, DefId, FunctionBuilder, HirBinOp, HirBlock, HirExpression, HirExpressionKind,
    HirExpressionLoweringVisitor, HirIteraMode, HirStatement, HirStatementKind, HirVisitor, LocalBinding, MirBinOp,
    MirCollectionOp, MirConstant, MirError, MirFilumForm, MirIntrinsic, MirLocalDecl, MirLocalId, MirOperand, MirPlace,
    MirPlaceBase, MirProjection, MirStatement, MirStatementKind, MirTerminatorKind, MirType, MirValueKind,
    NumericWidth, OpenBlock, Primitive, Span, Symbol, TypeId, TypeTable,
};

/// Issue name: a second top-level `thread lane` loop in one kernel.
pub const LANE_FORM_SECOND_REGION_ISSUE: &str = "lane_form_second_region";
/// Issue name: a statement after the top-level `thread lane` loop.
pub const LANE_FORM_POST_LOOP_ISSUE: &str = "lane_form_post_loop_statement";
/// Issue name: a kernel `⊥` whose call is not the checked tensor read (the
/// device lowering of other failable calls is pending).
pub(crate) const KERNEL_CALL_DEFAULT_PENDING_ISSUE: &str = "kernel_call_default_pending";
/// Issue name: a `thread lane` reduce inside a lane region (C4-8 nested).
pub const LANE_FORM_NESTED_REDUCE_ISSUE: &str = "lane_form_nested_reduce";

/// A `thread` first reduction on a device lowering: declined by name until a
/// device recipe carries the earliest-lane selection and the optional result.
pub const FIRST_REDUCE_DEVICE_ISSUE: &str = "first_reduce_device_unsupported";

/// Intern the device word (`u32`) the lane-form arithmetic runs in. Lowering
/// holds the type table immutably, so the arena gets it before function
/// lowering starts.
pub(crate) fn ensure_lane_word_type(types: &mut TypeTable) {
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
}

/// The device word the lane id and the grid arithmetic run in.
fn lane_word(types: &TypeTable) -> Option<TypeId> {
    types.find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
}

/// The one lane region a device kernel body lowers in lane form.
#[derive(Debug, Clone, Copy)]
pub(super) struct LaneRegion {
    /// Span of the `Itera` expression that is the region.
    pub span: Span,
    /// The compiler-internal lane id parameter.
    pub lane: MirLocalId,
    /// Half-open local-id range of the program-visible coordinate binders.
    pub coordinate_locals: Option<(MirLocalId, MirLocalId)>,
    /// The exact materialized tensor local that defines this region's lane
    /// domain. Resolved to its ABI resource through parameter aliases later.
    pub source_local: Option<MirLocalId>,
}

/// The region expression of a top-level statement: a `filum` loop, or a
/// `filum` reduction (bare or the right side of an assignment).
fn lane_loop(statement: &HirStatement) -> Option<&HirExpression> {
    match &statement.kind {
        HirStatementKind::Expr(expr) => tail_lane_loop(expr),
        HirStatementKind::Local(local) => local
            .init
            .as_ref()
            .and_then(|expr| grouped_grid_reduction(expr).map(|_| expr)),
        // A grouped grid reduction can be the returned tensor itself. The
        // function result buffer is its destination, so the return is part of
        // the lane region and is emitted by stage 2.
        HirStatementKind::Redde(Some(expr)) if grouped_grid_reduction(expr).is_some() => Some(expr),
        _ => None,
    }
}

fn tail_lane_loop(expr: &HirExpression) -> Option<&HirExpression> {
    if matches!(
        &expr.kind,
        HirExpressionKind::Itera { filum: Some(_), mode: HirIteraMode::Ex, .. }
    ) {
        return Some(expr);
    }
    let candidate = match &expr.kind {
        HirExpressionKind::Assign(_, value) => value.as_ref(),
        _ => expr,
    };
    grid_reduction(candidate).is_some().then_some(candidate)
}

/// The `filum` reduction of a region expression: the reduction itself, or
/// the left side of its `coalesce` (the source spells the identity as
/// `reduce ... coalesce d`, which the front end keeps as a coalesce).
fn grid_reduction(expr: &HirExpression) -> Option<&HirExpression> {
    let expr = peel_implicit_narrowing(expr);
    let reduction = match &expr.kind {
        HirExpressionKind::Binary(HirBinOp::Coalesce, lhs, _) => lhs.as_ref(),
        _ => expr,
    };
    matches!(
        &reduction.kind,
        HirExpressionKind::Reduction { filum: Some(_), group: None, .. }
    )
    .then_some(reduction)
    .or_else(|| grouped_grid_reduction(reduction))
}

fn grouped_grid_reduction(expr: &HirExpression) -> Option<&HirExpression> {
    matches!(
        &expr.kind,
        HirExpressionKind::Reduction {
            op: radix_hir::HirReductionOp::Sum | radix_hir::HirReductionOp::Count,
            filum: Some(_),
            group: Some(_),
            ..
        }
    )
    .then_some(expr)
}

/// The expression under the implicit narrowing the type checker wraps around a
/// store into a sized integer cell (`out[0] ← reduce ...` into `i32`). The
/// narrowing is the store's width boundary (F9 ruling 27), not part of the
/// reduction region.
fn peel_implicit_narrowing(expr: &HirExpression) -> &HirExpression {
    match &expr.kind {
        HirExpressionKind::Conversio { source, implicit: Some(_), .. } => source,
        _ => expr,
    }
}

/// Whether a region expression is a reduction (statements after it are
/// allowed: they run after the node).
fn is_reduce_region(expr: &HirExpression) -> bool {
    grid_reduction(expr).is_some()
}

impl FunctionBuilder<'_> {
    /// Pre-scan a device compute body for its lane region. Adds the `Lane`
    /// builtin parameter when there is exactly one admissible region and
    /// records it; fails closed on a second region or a statement after it.
    pub(super) fn prepare_lane_region(&mut self, body: &HirBlock) {
        if !self.is_device_lowering() {
            return;
        }
        let mut regions: Vec<&HirExpression> = body.statements.iter().filter_map(lane_loop).collect();
        if let Some(tail) = body.expr.as_deref().and_then(tail_lane_loop) {
            regions.push(tail);
        }
        let Some(first) = regions.first().copied() else {
            return;
        };
        if let Some(second) = regions.get(1) {
            self.errors.push(MirError {
                message: "a kernel with a second top-level `thread lane` loop needs a whole-grid launch split, \
                          which does not exist yet"
                    .to_owned(),
                issue: LANE_FORM_SECOND_REGION_ISSUE.to_owned(),
                span: second.span,
                definition_span: Some(first.span),
            });
            return;
        }
        // Everything after the loop would read results of the whole grid. A
        // reduction region is exempt: its node is a launch split point.
        let returned_alias = body.statements.iter().find_map(|statement| {
            let HirStatementKind::Local(local) = &statement.kind else {
                return None;
            };
            local
                .init
                .as_ref()
                .filter(|init| init.span == first.span)
                .map(|_| local.def_id)
        });
        let after_loop = body
            .statements
            .iter()
            .skip_while(|statement| lane_loop(statement).is_none_or(|expr| expr.span != first.span))
            .skip(1)
            .find(|statement| {
                let returns_alias = returned_alias.is_some_and(|alias| {
                    matches!(
                        &statement.kind,
                        HirStatementKind::Redde(Some(expr))
                            if matches!(&expr.kind, HirExpressionKind::Path(def_id) if *def_id == alias)
                    )
                });
                !returns_alias && !matches!(statement.kind, HirStatementKind::Tacet | HirStatementKind::Redde(None))
            });
        let trailing = body.expr.as_deref().filter(|expr| expr.span != first.span);
        let post_region = if is_reduce_region(first) {
            None
        } else {
            after_loop
                .map(|statement| statement.span)
                .or(trailing.map(|expr| expr.span))
        };
        if let Some(span) = post_region {
            self.errors.push(MirError {
                message: "a statement after the top-level `thread lane` loop depends on the whole grid, which \
                          needs a launch split that does not exist yet"
                    .to_owned(),
                issue: LANE_FORM_POST_LOOP_ISSUE.to_owned(),
                span,
                definition_span: Some(first.span),
            });
            return;
        }
        let region_kind = grid_reduction(first).map_or(&first.kind, |reduction| &reduction.kind);
        let (HirExpressionKind::Itera { filum: Some((def_id, name)), .. }
        | HirExpressionKind::Reduction { filum: Some((def_id, name)), .. }) = region_kind
        else {
            return;
        };
        // The lane id is the 32-bit global invocation id the device launches.
        let Some(lane_ty) = lane_word(self.types) else {
            self.errors
                .push(MirError::missing_type(first.span, "device lane word (u32 numerus)"));
            return;
        };
        let lane = self.next_local_id();
        self.add_lane_param(*def_id, *name, lane_ty, first.span);
        self.lane_region = Some(LaneRegion { span: first.span, lane, coordinate_locals: None, source_local: None });
    }

    /// Whether `expr` is the pre-scanned lane region of this body.
    pub(super) fn is_lane_region(&self, expr: &HirExpression) -> bool {
        self.lane_region
            .is_some_and(|region| region.span == expr.span)
    }

    /// Lower a grid reduction region stored through the implicit narrowing the
    /// type checker inserts for a sized integer cell (`out[0] ← reduce ...`
    /// into `i32`). The kernel-requirements pass has already proven the
    /// reduction's result range fits the cell (rule 12.1: the proof is the
    /// narrowing), so the reduction node is typed as the cell and no 64-bit
    /// value or checked conversion exists on the device. `None` when `source`
    /// is not a grid reduction region.
    pub(super) fn lower_grid_reduce_store(
        &mut self,
        source: &HirExpression,
        narrowing: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        let reduction = grid_reduction(source)?;
        let HirExpressionKind::Reduction {
            op,
            source: tensor,
            coordinates,
            filum: Some(filum),
            binding,
            body,
            identity,
            ..
        } = &reduction.kind
        else {
            return None;
        };
        let identity = match &source.kind {
            HirExpressionKind::Binary(HirBinOp::Coalesce, _, identity) => Some(identity.as_ref()),
            _ => identity.as_deref(),
        };
        Some(self.lower_filum_reduce_expr(
            &super::control::FilumReduceLowering {
                op: *op,
                form: MirFilumForm::Grid,
                source: tensor,
                group: None,
                coordinates,
                filum,
                binding: binding.as_ref(),
                body: body.as_ref(),
                identity,
            },
            narrowing,
        ))
    }

    /// Lower a coalesced grid reduction region: `reduce ... coalesce d`
    /// arrives as `Coalesce(Reduction, d)`; the reduction is the node and `d`
    /// its identity (out-of-range lanes contribute it).
    pub(super) fn lower_grid_reduce_coalesce(
        &mut self,
        reduction: &HirExpression,
        identity: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let HirExpressionKind::Reduction { op, source, coordinates, filum: Some(filum), binding, body, .. } =
            &reduction.kind
        else {
            return None;
        };
        self.lower_filum_reduce_expr(
            &super::control::FilumReduceLowering {
                op: *op,
                form: MirFilumForm::Grid,
                source,
                group: None,
                coordinates,
                filum,
                binding: binding.as_ref(),
                body: body.as_ref(),
                identity: Some(identity),
            },
            expr,
        )
    }

    /// The per-axis extent operands of a ranked tensor source: constants for
    /// literal axes, one `TensorExtent` temp otherwise. The grid reduction
    /// node carries them so emitters compute the element count in-kernel.
    pub(super) fn lane_form_extents(
        &mut self,
        tensor: &MirPlace,
        source_ty: TypeId,
        coordinate_count: usize,
        span: Span,
    ) -> Option<Vec<MirOperand>> {
        let CollectionIteraKind::Shaped { rank, literal, matrix: false, .. } =
            self.classify_collection_itera(source_ty, HirIteraMode::Ex, coordinate_count)
        else {
            self.errors.push(MirError::unsupported(
                span,
                "thread lane reduce over a source that is not a ranked tensor",
            ));
            return None;
        };
        if rank == 0 || rank != coordinate_count {
            self.errors.push(MirError::unsupported(
                span,
                "thread lane reduce coordinate count differs from rank",
            ));
            return None;
        }
        let word = MirType::semantic(lane_word(self.types)?);
        let mut extents = Vec::with_capacity(rank);
        for axis in 0..rank {
            extents.push(match literal.as_deref() {
                Some(dimensions) => MirOperand::Constant(MirConstant::Int(dimensions[axis])),
                None => self.assign_temp(
                    MirValueKind::TensorExtent {
                        tensor: MirOperand::Place(tensor.clone()),
                        axis: u32::try_from(axis).ok()?,
                    },
                    word,
                    span,
                ),
            });
        }
        Some(extents)
    }

    /// Lower the lane region in lane form (see the module docs).
    pub(super) fn lower_lane_form_itera(
        &mut self,
        coordinates: &[(DefId, Symbol)],
        binding: DefId,
        name: Symbol,
        iter: &HirExpression,
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let region = self.lane_region?;
        let span = expr.span;
        let iter_ty = iter.ty?;
        let CollectionIteraKind::Shaped { element, rank, literal, matrix: false } =
            self.classify_collection_itera(iter_ty, HirIteraMode::Ex, coordinates.len())
        else {
            self.errors.push(MirError::unsupported(
                iter.span,
                "thread lane loop over a source that is not a ranked tensor",
            ));
            return None;
        };
        if rank == 0 || rank != coordinates.len() {
            self.errors.push(MirError::unsupported(
                iter.span,
                "thread lane loop coordinate count differs from rank",
            ));
            return None;
        }
        // The grid uses u32, while named binders retain the program's int
        // carrier. Checked i32 uses are materialized at the coordinate seam.
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let word = MirType::semantic(lane_word(self.types)?);
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let element_ty = MirType::semantic(element);

        let source = self.lower_expr_value(iter)?;
        let tensor = self.materialize_operand_place(source, MirType::semantic(iter_ty), iter.span);
        // A shift view (`src.shift(s).limes<P>()`) has its base tensor's shape
        // and no storage of its own, so iterating one has the base parameter
        // for its domain — the same reading the device shift-view fold takes
        // when it retargets the view's extents and reads onto the base.
        let source_local = match &tensor.base {
            MirPlaceBase::Local(local) => Some(*local),
            MirPlaceBase::Temp(_) => view_base_local(&self.blocks, &tensor.base),
        };
        self.lane_region.as_mut()?.source_local = source_local;

        // Extents: constants for literal axes, `TensorExtent` otherwise.
        let mut extents = Vec::with_capacity(rank);
        for axis in 0..rank {
            let extent = match literal.as_deref() {
                Some(dimensions) => MirOperand::Constant(MirConstant::Int(dimensions[axis])),
                None => self.assign_temp(
                    MirValueKind::TensorExtent {
                        tensor: MirOperand::Place(tensor.clone()),
                        axis: u32::try_from(axis).ok()?,
                    },
                    word,
                    span,
                ),
            };
            extents.push(extent);
        }
        let lane = MirOperand::Place(MirPlace::local(region.lane));
        let mut total = extents[0].clone();
        for extent in &extents[1..] {
            total = self.binary_temp(MirBinOp::Mul, total, extent.clone(), word, span);
        }

        // Guard: a lane past the grid writes nothing.
        let guard = self.binary_temp(MirBinOp::GtEq, lane.clone(), total, bivalens, span);
        let exit_id = self.fresh_block(span);
        let body_id = self.fresh_block(body.span);
        let done_id = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: guard, then_block: exit_id, else_block: body_id },
            span,
        );
        self.switch_to(exit_id);
        self.terminate_current(MirTerminatorKind::Return(None), span);
        self.switch_to(body_id);

        // Row-major binders: the last axis varies fastest.
        let mut remainder = lane.clone();
        let mut values: Vec<Option<MirOperand>> = vec![None; rank];
        for axis in (1..rank).rev() {
            values[axis] = Some(self.binary_temp(MirBinOp::Mod, remainder.clone(), extents[axis].clone(), word, span));
            remainder = self.binary_temp(MirBinOp::Div, remainder, extents[axis].clone(), word, span);
        }
        values[0] = Some(remainder);
        let coordinate_start = self.next_local_id();
        for (axis, (def_id, coordinate_name)) in coordinates.iter().enumerate() {
            let local = self.next_local_id();
            self.locals.push(MirLocalDecl {
                id: local,
                name: Some(*coordinate_name),
                ty: numerus,
                mutable: true,
                span,
            });
            self.bindings
                .insert(*def_id, LocalBinding { local, ty: numerus });
            let value = values[axis].take()?;
            self.assign(MirPlace::local(local), value, numerus, span);
        }

        let coordinate_end = self.next_local_id();
        self.lane_region.as_mut()?.coordinate_locals = Some((coordinate_start, coordinate_end));

        // Element binder: `v = src[lane]` as an Index place.
        let element_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: element_local, name: Some(name), ty: element_ty, mutable: true, span });
        self.bindings
            .insert(binding, LocalBinding { local: element_local, ty: element_ty });
        let mut read = tensor;
        read.projections.push(MirProjection::Index(lane));
        self.assign(MirPlace::local(element_local), MirOperand::Place(read), element_ty, span);

        self.breakables.push(BreakableContext {
            perge_target: Some(done_id),
            rumpe_target: done_id,
            rumpe_used: false,
        });
        self.visit_block(body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(done_id), span);
        self.switch_to(done_id);
        Some(MirOperand::Constant(MirConstant::Unit))
    }
}

/// The parameter local a shift view chains to, when `base` names the view
/// (`src.shift(s)` then `…limes<P>()`, optionally through whole-value copies).
/// `None` when the chain does not end at a local place.
fn view_base_local(blocks: &[OpenBlock], base: &MirPlaceBase) -> Option<MirLocalId> {
    view_base_chain(blocks, base, 0)
}

fn view_base_chain(blocks: &[OpenBlock], base: &MirPlaceBase, depth: u32) -> Option<MirLocalId> {
    if depth > 8 {
        return None;
    }
    let definition = blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .find(|statement| defines(statement, base))?;
    match &definition.kind {
        MirStatementKind::RuntimeCall { call, .. } => match &call.intrinsic {
            MirIntrinsic::TensorLimes(_) => view_base_operand(blocks, call.args.first()?, depth),
            // The shift's base is the tensor parameter itself.
            MirIntrinsic::Collection(MirCollectionOp::TensorShift) => match call.args.first()? {
                MirOperand::Place(place) if place.projections.is_empty() => match &place.base {
                    MirPlaceBase::Local(local) => Some(*local),
                    MirPlaceBase::Temp(_) => None,
                },
                _ => None,
            },
            _ => None,
        },
        MirStatementKind::Assign { value, .. } => match &value.kind {
            MirValueKind::Operand(operand) => view_base_operand(blocks, operand, depth),
            _ => None,
        },
        _ => None,
    }
}

fn view_base_operand(blocks: &[OpenBlock], operand: &MirOperand, depth: u32) -> Option<MirLocalId> {
    match operand {
        MirOperand::Place(place) if place.projections.is_empty() => view_base_chain(blocks, &place.base, depth + 1),
        MirOperand::Temp(temp) => view_base_chain(blocks, &MirPlaceBase::Temp(*temp), depth + 1),
        _ => None,
    }
}

/// Whether `statement` writes the whole storage `base` names.
fn defines(statement: &MirStatement, base: &MirPlaceBase) -> bool {
    let place = match &statement.kind {
        MirStatementKind::Assign { place, .. } | MirStatementKind::Construct { destination: place, .. } => place,
        MirStatementKind::RuntimeCall { destination, .. } => match destination {
            Some(place) => place,
            None => return false,
        },
        MirStatementKind::Call { .. } => return false,
    };
    place.projections.is_empty() && place.base == *base
}
