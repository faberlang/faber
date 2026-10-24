//! Shift views inside device kernels.
//!
//! `src.shift([dy, dx]).limes<P>()` is a view: element `c` of the view is the
//! base element at `c + offset`, with the edge policy `P` deciding what a
//! coordinate past the edge reads. A stencil states its neighbour radius this
//! way. The view has its base tensor's shape, so on a device it needs no
//! storage of its own: this pass resolves every element read of the view onto
//! the base tensor parameter and every extent of the view onto the base's
//! extent, then drops the view's construction.
//!
//! For a view of base `B` (extents `e_0 … e_{r-1}`) read at flat index `f`,
//! the coordinates are `c_k = (f / (e_{k+1} · … · e_{r-1})) % e_k` and the read
//! becomes `B[(n_0 · e_1 + n_1) · e_2 + …]` with, for offset `s` on an axis of
//! extent `e`:
//!
//! - `limes<clamp>`: `n = min(c + s, e - 1)` for `s > 0`, and
//!   `n = max(c, -s) + s` for `s < 0` (both stay in `[0, e)` for every `s`);
//! - `limes<wrap>`: `n = (c + s mod e) % e`, with `s mod e` built from
//!   non-negative terms only.
//!
//! The base read is then always in range, so the only index that can fault
//! is the view index itself. The pass therefore requires that index to be
//! proven in range of the view (the lane's own coordinate, as a stencil reads
//! it); any other view index is refused by name rather than read without its
//! check. Kernels allow less, never mean something different: `limes<reflect>`,
//! a custom policy, a fill (`coalesce`) and any use of the view other than an
//! element read or an extent are refused by name.

use crate::abi::{LaneIndexProof, resolve_parameter_alias};
use crate::static_shape_fold::{ShapeFoldError, ShapeFoldKind};
use crate::{
    MirAggregateFields, MirAggregateItem, MirBinOp, MirBuiltinEdgePolicy, MirCollectionOp,
    MirConstant, MirEdgePolicy, MirFunction, MirIntrinsic, MirLocalId, MirOperand, MirPlace,
    MirPlaceBase, MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirTemp,
    MirTempId, MirTerminatorKind, MirType, MirValidationContext, MirValue, MirValueId,
    MirValueKind, visit::MirVisitor,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, Type};
use std::collections::BTreeSet;

/// `limes<P>` boundary that the device lowering cannot express faithfully.
pub const ERR_SHIFT_VIEW_POLICY: &str = "kernel shift view edge policy is not lowered on devices";
/// A shift whose result is not bounded by a `limes` policy.
pub const ERR_SHIFT_VIEW_UNBOUNDED: &str =
    "kernel shift view must be bounded by `limes<clamp>` or `limes<wrap>` on devices";
/// A shift view used other than by element read or extent.
pub const ERR_SHIFT_VIEW_USE: &str =
    "kernel shift view is lowered only for element reads and extents";
/// A shift view read at an index not proven in range of the view.
pub const ERR_SHIFT_VIEW_INDEX: &str =
    "kernel shift view read must be at an index proven in range of the view";
/// A shift whose base or offsets are not a tensor parameter and constants.
pub const ERR_SHIFT_VIEW_SHAPE: &str =
    "kernel shift view needs a tensor parameter base and constant offsets of its rank";

/// The largest offset magnitude a device index carries (32-bit signed).
const DEVICE_INDEX_MAX: u64 = 0x7fff_ffff;

fn refuse(detail: &str, what: impl std::fmt::Display) -> ShapeFoldError {
    ShapeFoldError {
        kind: ShapeFoldKind::NotDeviceMaterializable,
        message: format!("{detail} ({what})"),
    }
}

/// The edge policy of a resolvable view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Clamp,
    Wrap,
}

/// One resolvable shift view of a kernel.
struct ShiftView {
    /// The tensor parameter the view reads.
    base: MirLocalId,
    /// The constant offset of each axis.
    offsets: Vec<i64>,
    edge: Edge,
    /// Every local and temp naming the view (the limes result and its copies).
    names: BTreeSet<Key>,
    /// The shift result (read only by the limes call).
    shifted: Key,
    /// The offsets list and its element temps, each dropped with the view
    /// when nothing else reads it.
    offset_storage: Vec<Key>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Local(u32),
    Temp(u32),
}

impl Key {
    fn of(base: &MirPlaceBase) -> Self {
        match base {
            MirPlaceBase::Local(local) => Self::Local(local.0),
            MirPlaceBase::Temp(temp) => Self::Temp(temp.0),
        }
    }

    /// The storage a bare operand names (no projections).
    fn of_operand(operand: &MirOperand) -> Option<Self> {
        match operand {
            MirOperand::Temp(temp) => Some(Self::Temp(temp.0)),
            MirOperand::Place(place) if place.projections.is_empty() => Some(Self::of(&place.base)),
            _ => None,
        }
    }
}

fn destination_key(statement: &MirStatement) -> Option<Key> {
    let place = match &statement.kind {
        MirStatementKind::Assign { place, .. } => place,
        MirStatementKind::RuntimeCall {
            destination: Some(place),
            ..
        }
        | MirStatementKind::Call {
            destination: Some(place),
            ..
        } => place,
        MirStatementKind::Construct { destination, .. } => destination,
        MirStatementKind::RuntimeCall { .. } | MirStatementKind::Call { .. } => return None,
    };
    place.projections.is_empty().then(|| Key::of(&place.base))
}

/// Counts every reference to a set of storage names, wherever it appears.
struct References<'a> {
    names: &'a BTreeSet<Key>,
    count: usize,
}

impl MirVisitor for References<'_> {
    fn visit_operand(&mut self, operand: &MirOperand) {
        if let MirOperand::Temp(temp) = operand
            && self.names.contains(&Key::Temp(temp.0))
        {
            self.count += 1;
        }
        crate::visit::walk_operand(self, operand);
    }

    fn visit_place(&mut self, place: &MirPlace) {
        if self.names.contains(&Key::of(&place.base)) {
            self.count += 1;
        }
        crate::visit::walk_place(self, place);
    }
}

fn reference_count(function: &MirFunction, names: &BTreeSet<Key>) -> usize {
    let mut references = References { names, count: 0 };
    references.visit_function(function);
    references.count
}

fn statements(function: &MirFunction) -> impl Iterator<Item = &MirStatement> {
    function.blocks.iter().flat_map(|block| &block.statements)
}

/// Resolve the shift views of one device kernel onto their base parameters.
///
/// A kernel without a `shift` is returned unchanged.
///
/// # Errors
/// Returns a [`ShapeFoldError`] naming the refused form: an unsupported edge
/// policy, an unbounded shift, a non-parameter base or non-constant offsets,
/// a use other than an element read or extent, or a view read at an index
/// not proven in range of the view.
pub(crate) fn resolve_kernel_shift_views(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Option<MirFunction>, ShapeFoldError> {
    let views = find_views(function, validation)?;
    if views.is_empty() {
        return Ok(None);
    }
    let mut resolved = function.clone();
    for view in &views {
        retarget_extents(&mut resolved, view);
        drop_construction(&mut resolved, view);
    }
    // The view index must be proven in range of the view before the read is
    // moved onto the always-in-range base read. The view has its base's
    // shape, so the proof asks about the base parameter at the view index.
    let proof = LaneIndexProof::new(&resolved, validation);
    for view in &views {
        for index in view_read_indexes(&resolved, view) {
            let proven = proof
                .as_ref()
                .is_some_and(|proof| proof.index_guards(view.base, &index, false).is_empty());
            if !proven {
                return Err(refuse(ERR_SHIFT_VIEW_INDEX, view_name(function, view)));
            }
        }
    }
    let mut rewriter = Rewriter {
        next_temp: resolved
            .temps
            .iter()
            .map(|temp| temp.id.0 + 1)
            .max()
            .unwrap_or(0),
        next_value: statements(&resolved)
            .filter_map(|statement| match &statement.kind {
                MirStatementKind::Assign { value, .. } => Some(value.id.0 + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0),
        new_temps: Vec::new(),
    };
    for view in &views {
        rewriter.rewrite_reads(&mut resolved, view)?;
    }
    resolved.temps.extend(rewriter.new_temps);
    for view in &views {
        let mut names = view.names.clone();
        names.insert(view.shifted);
        if reference_count(&resolved, &names) != 0 {
            return Err(refuse(ERR_SHIFT_VIEW_USE, view_name(function, view)));
        }
    }
    Ok(Some(resolved))
}

fn view_name(function: &MirFunction, view: &ShiftView) -> String {
    let base = function
        .locals
        .iter()
        .find(|local| local.id == view.base)
        .and_then(|local| local.name)
        .map_or_else(|| format!("_{}", view.base.0), |name| format!("{name:?}"));
    format!("shift {:?} of parameter {base}", view.offsets)
}

/// Every `tensor_shift` of the kernel, each with its `limes` view.
fn find_views(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<ShiftView>, ShapeFoldError> {
    let mut views = Vec::new();
    for statement in statements(function) {
        let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
            continue;
        };
        if call.intrinsic != MirIntrinsic::Collection(MirCollectionOp::TensorShift) {
            continue;
        }
        let shifted = destination_key(statement)
            .ok_or_else(|| refuse(ERR_SHIFT_VIEW_USE, "shift result is not stored"))?;
        let (base, offsets, offset_storage) = shift_operands(function, validation, call)?;
        let (edge, view) = limes_of(function, shifted, &offsets)?;
        if edge == Edge::Wrap
            && offsets
                .iter()
                .any(|offset| offset.unsigned_abs() > DEVICE_INDEX_MAX)
        {
            return Err(refuse(
                ERR_SHIFT_VIEW_SHAPE,
                "a wrap offset exceeds the device index range",
            ));
        }
        let names = view_names(function, view);
        // Each name of the view is written once: dropping its definition
        // must not drop another value stored into the same local.
        for name in &names {
            let writes = statements(function)
                .filter(|statement| destination_key(statement) == Some(*name))
                .count();
            if writes != 1 {
                return Err(refuse(
                    ERR_SHIFT_VIEW_USE,
                    "a view binding is written again",
                ));
            }
        }
        views.push(ShiftView {
            base,
            offsets,
            edge,
            names,
            shifted,
            offset_storage,
        });
    }
    Ok(views)
}

/// The base parameter, the constant offsets and the offsets list of a shift.
fn shift_operands(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    call: &MirRuntimeCall,
) -> Result<(MirLocalId, Vec<i64>, Vec<Key>), ShapeFoldError> {
    let [base, offsets] = call.args.as_slice() else {
        return Err(refuse(ERR_SHIFT_VIEW_SHAPE, "shift arity"));
    };
    let base = match Key::of_operand(base) {
        Some(Key::Local(local)) => resolve_parameter_alias(function, MirLocalId(local)),
        _ => return Err(refuse(ERR_SHIFT_VIEW_SHAPE, "base is not a parameter")),
    };
    let Some(param) = function.params.iter().find(|param| param.local == base) else {
        return Err(refuse(ERR_SHIFT_VIEW_SHAPE, "base is not a parameter"));
    };
    let list = Key::of_operand(offsets);
    let items = list
        .and_then(|key| {
            statements(function).find_map(|statement| match &statement.kind {
                MirStatementKind::Construct {
                    destination,
                    aggregate,
                } if destination.projections.is_empty() && Key::of(&destination.base) == key => {
                    Some(aggregate)
                }
                _ => None,
            })
        })
        .and_then(|aggregate| match &aggregate.fields {
            MirAggregateFields::Ordered(items) => Some(items),
            _ => None,
        })
        .ok_or_else(|| refuse(ERR_SHIFT_VIEW_SHAPE, "offsets are not a list literal"))?;
    let mut storage: Vec<Key> = list.into_iter().collect();
    let offsets = items
        .iter()
        .map(|item| match item {
            MirAggregateItem::Operand(operand) => {
                if let Some(key) = Key::of_operand(operand) {
                    storage.push(key);
                }
                constant_value(function, operand, 0)
            }
            MirAggregateItem::Spread(_) => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| refuse(ERR_SHIFT_VIEW_SHAPE, "offsets are not constants"))?;
    if tensor_rank(param.ty, validation) != Some(offsets.len()) {
        return Err(refuse(
            ERR_SHIFT_VIEW_SHAPE,
            "offsets do not match the rank",
        ));
    }
    Ok((base, offsets, storage))
}

/// The integer constant an offset operand holds: a literal, or a single
/// definition that negates or copies one (`-1` lowers to `neg const 1`).
fn constant_value(function: &MirFunction, operand: &MirOperand, depth: u32) -> Option<i64> {
    match operand {
        MirOperand::Constant(MirConstant::Int(value)) => return Some(*value),
        MirOperand::Constant(MirConstant::UInt(value)) => return i64::try_from(*value).ok(),
        _ => {}
    }
    let key = Key::of_operand(operand)?;
    if depth > 8 {
        return None;
    }
    let mut definitions =
        statements(function).filter(|statement| destination_key(statement) == Some(key));
    let (Some(definition), None) = (definitions.next(), definitions.next()) else {
        return None;
    };
    let MirStatementKind::Assign { value, .. } = &definition.kind else {
        return None;
    };
    match &value.kind {
        MirValueKind::Operand(inner) => constant_value(function, inner, depth + 1),
        MirValueKind::Unary {
            op: crate::MirUnOp::Neg,
            operand,
        } => constant_value(function, operand, depth + 1)?.checked_neg(),
        _ => None,
    }
}

/// The rank of a tensor type; `None` when `ty` is not a ranked tensor.
fn tensor_rank(ty: MirType, validation: &MirValidationContext<'_>) -> Option<usize> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(_, shape) => match validation.types.get_index(*shape) {
            IndexExpr::Literal(_) => Some(1),
            IndexExpr::Tuple(dims) => Some(dims.len()),
            _ => None,
        },
        Type::Alias(_, target) => tensor_rank(MirType::semantic(*target), validation),
        _ => None,
    }
}

/// The edge policy and result of the one `limes` call that bounds `shifted`.
fn limes_of(
    function: &MirFunction,
    shifted: Key,
    offsets: &[i64],
) -> Result<(Edge, Key), ShapeFoldError> {
    let mut found = None;
    for statement in statements(function) {
        let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
            continue;
        };
        let MirIntrinsic::TensorLimes(policy) = &call.intrinsic else {
            continue;
        };
        if call.args.first().and_then(Key::of_operand) != Some(shifted) {
            continue;
        }
        let edge = match policy {
            MirEdgePolicy::Builtin(MirBuiltinEdgePolicy::Clamp) => Edge::Clamp,
            MirEdgePolicy::Builtin(MirBuiltinEdgePolicy::Wrap) => Edge::Wrap,
            MirEdgePolicy::Builtin(MirBuiltinEdgePolicy::Reflect) => {
                return Err(refuse(ERR_SHIFT_VIEW_POLICY, "limes<reflect>"));
            }
            MirEdgePolicy::Custom { .. } => {
                return Err(refuse(ERR_SHIFT_VIEW_POLICY, "custom limes policy"));
            }
        };
        let view = destination_key(statement)
            .ok_or_else(|| refuse(ERR_SHIFT_VIEW_USE, "limes result is not stored"))?;
        found = Some((edge, view));
    }
    let found =
        found.ok_or_else(|| refuse(ERR_SHIFT_VIEW_UNBOUNDED, format!("shift {offsets:?}")))?;
    // The shift result is written once and read only by its limes call.
    if reference_count(function, &BTreeSet::from([shifted])) != 2 {
        return Err(refuse(
            ERR_SHIFT_VIEW_UNBOUNDED,
            format!("shift {offsets:?}"),
        ));
    }
    Ok(found)
}

/// The view's result and every whole-value copy of it.
fn view_names(function: &MirFunction, view: Key) -> BTreeSet<Key> {
    let mut names = BTreeSet::from([view]);
    loop {
        let before = names.len();
        for statement in statements(function) {
            if let MirStatementKind::Assign { place, value } = &statement.kind
                && place.projections.is_empty()
                && let MirValueKind::Operand(source) = &value.kind
                && Key::of_operand(source).is_some_and(|key| names.contains(&key))
            {
                names.insert(Key::of(&place.base));
            }
        }
        if names.len() == before {
            return names;
        }
    }
}

/// `extent view axis k` reads `extent base axis k`: the view has its base's
/// shape.
fn retarget_extents(function: &mut MirFunction, view: &ShiftView) {
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            if let MirStatementKind::Assign { value, .. } = &mut statement.kind
                && let MirValueKind::TensorExtent { tensor, .. } = &mut value.kind
                && Key::of_operand(tensor).is_some_and(|key| view.names.contains(&key))
            {
                *tensor = MirOperand::Place(MirPlace::local(view.base));
            }
        }
    }
}

/// Drop the shift, the limes, the view copies and (when nothing else reads
/// it) the offsets list.
fn drop_construction(function: &mut MirFunction, view: &ShiftView) {
    // An offsets list (or one of its element temps) read only by this shift
    // has exactly two references: its definition and that one read.
    let dead_storage: BTreeSet<Key> = view
        .offset_storage
        .iter()
        .copied()
        .filter(|key| reference_count(function, &BTreeSet::from([*key])) == 2)
        .collect();
    for block in &mut function.blocks {
        block.statements.retain(|statement| {
            let Some(key) = destination_key(statement) else {
                return true;
            };
            !(key == view.shifted || view.names.contains(&key) || dead_storage.contains(&key))
        });
    }
}

/// The flat index of every element read of the view.
fn view_read_indexes(function: &MirFunction, view: &ShiftView) -> Vec<MirOperand> {
    struct Reads<'a> {
        names: &'a BTreeSet<Key>,
        found: Vec<MirOperand>,
    }
    impl MirVisitor for Reads<'_> {
        fn visit_place(&mut self, place: &MirPlace) {
            if self.names.contains(&Key::of(&place.base))
                && let Some(MirProjection::Index(index)) = place.projections.first()
            {
                self.found.push(index.clone());
            }
            crate::visit::walk_place(self, place);
        }
    }
    let mut reads = Reads {
        names: &view.names,
        found: Vec::new(),
    };
    reads.visit_function(function);
    reads.found
}

/// Mints the index arithmetic that moves a view read onto its base.
struct Rewriter {
    next_temp: u32,
    next_value: u32,
    new_temps: Vec<MirTemp>,
}

impl Rewriter {
    fn temp(&mut self, ty: MirType, span: Span) -> MirTempId {
        let id = MirTempId(self.next_temp);
        self.next_temp += 1;
        self.new_temps.push(MirTemp { id, ty, span });
        id
    }

    fn assign(
        &mut self,
        out: &mut Vec<MirStatement>,
        ty: MirType,
        kind: MirValueKind,
        span: Span,
    ) -> MirOperand {
        let temp = self.temp(ty, span);
        let id = MirValueId(self.next_value);
        self.next_value += 1;
        out.push(MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::temp(temp),
                value: MirValue { id, kind, ty, span },
            },
            span,
        });
        MirOperand::Temp(temp)
    }

    fn binary(
        &mut self,
        out: &mut Vec<MirStatement>,
        ty: MirType,
        op: MirBinOp,
        lhs: MirOperand,
        rhs: MirOperand,
        span: Span,
    ) -> MirOperand {
        self.assign(out, ty, MirValueKind::Binary { op, lhs, rhs }, span)
    }

    fn extremum(
        &mut self,
        out: &mut Vec<MirStatement>,
        ty: MirType,
        op: MirCollectionOp,
        lhs: MirOperand,
        rhs: MirOperand,
        span: Span,
    ) -> MirOperand {
        let temp = self.temp(ty, span);
        out.push(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(MirPlace::temp(temp)),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Collection(op),
                    args: vec![lhs, rhs],
                    return_ty: ty,
                },
            },
            span,
        });
        MirOperand::Temp(temp)
    }

    fn constant(
        &mut self,
        out: &mut Vec<MirStatement>,
        ty: MirType,
        value: i64,
        span: Span,
    ) -> MirOperand {
        self.assign(
            out,
            ty,
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(value))),
            span,
        )
    }

    /// The base flat index of the view read at flat `index` (type `ty`).
    fn base_index(
        &mut self,
        out: &mut Vec<MirStatement>,
        view: &ShiftView,
        index: &MirOperand,
        ty: MirType,
        span: Span,
    ) -> MirOperand {
        let rank = view.offsets.len();
        let extents: Vec<MirOperand> = (0..rank)
            .map(|axis| {
                self.assign(
                    out,
                    ty,
                    MirValueKind::TensorExtent {
                        tensor: MirOperand::Place(MirPlace::local(view.base)),
                        axis: u32::try_from(axis).unwrap_or(u32::MAX),
                    },
                    span,
                )
            })
            .collect();
        // Coordinates, last axis first: c_k = rest % e_k, rest = rest / e_k.
        let mut coordinates = vec![MirOperand::Constant(MirConstant::Int(0)); rank];
        let mut rest = index.clone();
        for axis in (0..rank).rev() {
            if axis == 0 {
                coordinates[0] = rest.clone();
            } else {
                coordinates[axis] = self.binary(
                    out,
                    ty,
                    MirBinOp::Mod,
                    rest.clone(),
                    extents[axis].clone(),
                    span,
                );
                rest = self.binary(out, ty, MirBinOp::Div, rest, extents[axis].clone(), span);
            }
        }
        let mut flat: Option<MirOperand> = None;
        for axis in 0..rank {
            let moved = self.moved_coordinate(
                out,
                view.edge,
                coordinates[axis].clone(),
                &extents[axis],
                view.offsets[axis],
                ty,
                span,
            );
            flat = Some(match flat {
                None => moved,
                Some(prefix) => {
                    let scaled =
                        self.binary(out, ty, MirBinOp::Mul, prefix, extents[axis].clone(), span);
                    self.binary(out, ty, MirBinOp::Add, scaled, moved, span)
                }
            });
        }
        flat.unwrap_or_else(|| index.clone())
    }

    /// Coordinate `c` (in `[0, e)`) moved by offset `s` under the edge policy;
    /// every intermediate is non-negative.
    #[allow(clippy::too_many_arguments)]
    fn moved_coordinate(
        &mut self,
        out: &mut Vec<MirStatement>,
        edge: Edge,
        coordinate: MirOperand,
        extent: &MirOperand,
        offset: i64,
        ty: MirType,
        span: Span,
    ) -> MirOperand {
        if offset == 0 {
            return coordinate;
        }
        // Past the edge every clamped offset of magnitude `>= e` reads the same
        // element, so a clamp offset is capped to the device's 32-bit index
        // range; a wrap offset beyond it is refused where the views are found.
        let magnitude = offset.unsigned_abs().min(DEVICE_INDEX_MAX);
        let magnitude = self.constant(out, ty, i64::try_from(magnitude).unwrap_or(i64::MAX), span);
        match edge {
            Edge::Clamp if offset > 0 => {
                // `min(c + min(s, e - 1), e - 1)`: equal to `min(c + s, e - 1)`
                // and never past `2e - 2`, so the sum cannot overflow.
                let one = self.constant(out, ty, 1, span);
                let last = self.binary(out, ty, MirBinOp::Sub, extent.clone(), one, span);
                let step = self.extremum(
                    out,
                    ty,
                    MirCollectionOp::Minimum,
                    magnitude,
                    last.clone(),
                    span,
                );
                let moved = self.binary(out, ty, MirBinOp::Add, coordinate, step, span);
                self.extremum(out, ty, MirCollectionOp::Minimum, moved, last, span)
            }
            Edge::Clamp => {
                let floor = self.extremum(
                    out,
                    ty,
                    MirCollectionOp::Maximum,
                    coordinate,
                    magnitude.clone(),
                    span,
                );
                self.binary(out, ty, MirBinOp::Sub, floor, magnitude, span)
            }
            Edge::Wrap => {
                // `s mod e` from non-negative terms: `|s| % e` for `s > 0`,
                // `e - |s| % e` for `s < 0` (at most `e`, so the sum stays
                // below `2e` and one `% e` lands it in range).
                let reduced = self.binary(out, ty, MirBinOp::Mod, magnitude, extent.clone(), span);
                let step = if offset > 0 {
                    reduced
                } else {
                    self.binary(out, ty, MirBinOp::Sub, extent.clone(), reduced, span)
                };
                let moved = self.binary(out, ty, MirBinOp::Add, coordinate, step, span);
                self.binary(out, ty, MirBinOp::Mod, moved, extent.clone(), span)
            }
        }
    }

    /// Rewrite every element read of `view` onto its base parameter.
    fn rewrite_reads(
        &mut self,
        function: &mut MirFunction,
        view: &ShiftView,
    ) -> Result<(), ShapeFoldError> {
        let types = OperandTypes::new(function);
        for block in &mut function.blocks {
            let mut statements = Vec::with_capacity(block.statements.len());
            for mut statement in std::mem::take(&mut block.statements) {
                let span = statement.span;
                let mut prefix = Vec::new();
                for operand in statement_operands(&mut statement) {
                    self.rewrite_operand(&mut prefix, operand, view, &types, span)?;
                }
                statements.extend(prefix);
                statements.push(statement);
            }
            let span = block.terminator.span;
            let mut prefix = Vec::new();
            for operand in terminator_operands(&mut block.terminator.kind) {
                self.rewrite_operand(&mut prefix, operand, view, &types, span)?;
            }
            statements.extend(prefix);
            block.statements = statements;
        }
        Ok(())
    }

    fn rewrite_operand(
        &mut self,
        out: &mut Vec<MirStatement>,
        operand: &mut MirOperand,
        view: &ShiftView,
        types: &OperandTypes,
        span: Span,
    ) -> Result<(), ShapeFoldError> {
        let MirOperand::Place(place) = operand else {
            return Ok(());
        };
        if !view.names.contains(&Key::of(&place.base)) {
            return Ok(());
        }
        let Some(MirProjection::Index(index)) = place.projections.first() else {
            return Err(refuse(ERR_SHIFT_VIEW_USE, "whole-view read"));
        };
        let ty = types
            .of(index)
            .ok_or_else(|| refuse(ERR_SHIFT_VIEW_USE, "view index has no integer type"))?;
        let index = index.clone();
        let base_index = self.base_index(out, view, &index, ty, span);
        place.base = MirPlaceBase::Local(view.base);
        place.projections[0] = MirProjection::Index(base_index);
        Ok(())
    }
}

/// The declared type of a temp or local operand.
struct OperandTypes {
    temps: std::collections::BTreeMap<u32, MirType>,
    locals: std::collections::BTreeMap<u32, MirType>,
}

impl OperandTypes {
    fn new(function: &MirFunction) -> Self {
        Self {
            temps: function
                .temps
                .iter()
                .map(|temp| (temp.id.0, temp.ty))
                .collect(),
            locals: function
                .locals
                .iter()
                .map(|local| (local.id.0, local.ty))
                .collect(),
        }
    }

    fn of_key(&self, key: Key) -> Option<MirType> {
        match key {
            Key::Temp(temp) => self.temps.get(&temp).copied(),
            Key::Local(local) => self.locals.get(&local).copied(),
        }
    }

    fn of(&self, operand: &MirOperand) -> Option<MirType> {
        self.of_key(Key::of_operand(operand)?)
    }
}

/// The read operands of a statement that can hold a view element read.
fn statement_operands(statement: &mut MirStatement) -> Vec<&mut MirOperand> {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => match &mut value.kind {
            MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => vec![operand],
            MirValueKind::Binary { lhs, rhs, .. } => vec![lhs, rhs],
            _ => Vec::new(),
        },
        MirStatementKind::Call { args, .. } => args.iter_mut().collect(),
        MirStatementKind::RuntimeCall { call, .. } => call.args.iter_mut().collect(),
        MirStatementKind::Construct { aggregate, .. } => match &mut aggregate.fields {
            MirAggregateFields::Ordered(items) => items
                .iter_mut()
                .map(|item| match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        operand
                    }
                })
                .collect(),
            _ => Vec::new(),
        },
    }
}

fn terminator_operands(kind: &mut MirTerminatorKind) -> Vec<&mut MirOperand> {
    match kind {
        MirTerminatorKind::Return(Some(operand))
        | MirTerminatorKind::ReturnError(operand)
        | MirTerminatorKind::Branch {
            condition: operand, ..
        }
        | MirTerminatorKind::Switch { value: operand, .. } => vec![operand],
        MirTerminatorKind::TryCall { args, .. } => args.iter_mut().collect(),
        _ => Vec::new(),
    }
}
