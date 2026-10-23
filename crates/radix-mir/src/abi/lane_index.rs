//! Proven-index analysis of a lane-form kernel (gpu-reset U9-2).
//!
//! A lane-form kernel (`MirParamMode::Lane` parameter) starts with the grid
//! guard `if lane >= E_0 * E_1 * ... { return }`, so every other block runs
//! with `lane < E_0 * E_1 * ...`. An `Index` projection on a tensor parameter
//! is PROVEN in range when its operand is, after row-major recombination, the
//! lane itself:
//!
//! - a load `t[lane]` (or `t[i, j]` over the lane binders, whose recombination
//!   `((i * e1) + j)` simplifies back to `lane`) on a tensor whose axis
//!   extents are the guard's factors, so `lane < element count of t`;
//! - a store whose index simplifies to `lane`: the dispatcher's lane-domain
//!   cover check refuses a written argument smaller than the lane domain.
//!
//! Everything else (offsets such as `i + 1`, parameters, gathered values) is
//! UNPROVEN and needs a device bounds compare plus a fault site. The analysis
//! is purely structural over the lowered MIR, so no carrier is stored on the
//! function: the emitter asks [`LaneIndexProof::guards`] for each `Index`
//! place it writes.
//!
//! # Per axis (F-07)
//!
//! A rank-N (N >= 2) index list lowers to the row-major recombination
//! `((c0 * e1 + c1) * e2 + c2) ...`. A flat compare against the element count
//! lets a coordinate past the end of a row that is still inside the tensor
//! read the next row, and a negative coordinate offset by a larger one slip
//! through, which the host runner (per-axis) never does. So the analysis
//! recovers the coordinates from the MIR definitions of that recombination and
//! guards each unproven axis `k >= 1` against its own extent. Axis 0 is
//! covered by the flat compare once every other axis holds (`0 <= c0 < e0`
//! iff `0 <= linear < count` when `0 <= c_k < e_k` for `k >= 1`), so it needs
//! no extent of its own. A coordinate proven by the lane binders stays
//! unguarded. A form whose recombination cannot be recovered keeps the flat
//! compare.

use std::collections::BTreeMap;

use radix_types::{IndexExpr, Type};

use crate::{
    MirBinOp, MirConstant, MirFunction, MirLocalId, MirOperand, MirParamMode, MirPlace,
    MirPlaceBase, MirProjection, MirStatement, MirStatementKind, MirTerminatorKind, MirType,
    MirValidationContext, MirValueKind, visit::MirVisitor,
};

use super::signature::resolve_parameter_alias;

/// One axis extent of a tensor type: a literal, or a symbolic dimension named
/// by its interned index expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Sym {
    Lane,
    Const(i64),
    Dim(u32),
    Add(Box<Sym>, Box<Sym>),
    Mul(Box<Sym>, Box<Sym>),
    Div(Box<Sym>, Box<Sym>),
    Mod(Box<Sym>, Box<Sym>),
    Opaque,
}

/// Which `Index` form needs a fault site: a tensor parameter and whether the
/// index is stored through or loaded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnprovenIndex {
    /// The tensor parameter local the index addresses.
    pub tensor: MirLocalId,
    /// `true` for a store destination, `false` for a load.
    pub store: bool,
}

/// One device bounds compare an unproven `Index` place needs.
#[derive(Debug, Clone, PartialEq)]
pub enum IndexGuard {
    /// The flat index against the tensor's element count (through unsigned,
    /// so a negative index trips too).
    Flat(MirOperand),
    /// The coordinate of an axis `>= 1` against that axis's extent.
    Axis {
        /// The coordinate operand.
        coordinate: MirOperand,
        /// The extent operand the recombination multiplies by.
        extent: MirOperand,
    },
}

/// A hashable identity of a place base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum BaseKey {
    Local(u32),
    Temp(u32),
}

impl BaseKey {
    pub(super) fn of(base: &MirPlaceBase) -> Self {
        match base {
            MirPlaceBase::Local(local) => Self::Local(local.0),
            MirPlaceBase::Temp(temp) => Self::Temp(temp.0),
        }
    }
}

/// The base a projection-free operand names, if it names one.
pub(super) fn operand_key(operand: &MirOperand) -> Option<BaseKey> {
    match operand {
        MirOperand::Place(place) if place.projections.is_empty() => Some(BaseKey::of(&place.base)),
        MirOperand::Temp(temp) => Some(BaseKey::Temp(temp.0)),
        _ => None,
    }
}

/// How many times each local or temp is written, and the single defining value.
struct Defs<'a> {
    single: BTreeMap<BaseKey, &'a crate::MirValue>,
    multi: std::collections::BTreeSet<BaseKey>,
}

impl<'a> Defs<'a> {
    fn note(&mut self, place: &MirPlace, value: Option<&'a crate::MirValue>) {
        let base = BaseKey::of(&place.base);
        match value {
            Some(value) if place.projections.is_empty() && !self.single.contains_key(&base) => {
                self.single.insert(base, value);
            }
            _ => {
                self.multi.insert(base);
            }
        }
    }
}

/// The proof state for one lane-form function.
pub struct LaneIndexProof<'a> {
    function: &'a MirFunction,
    validation: &'a MirValidationContext<'a>,
    defs: Defs<'a>,
    /// The lane-form kernel's lane id, or `None` for a whole-grid reduction
    /// term (which has no grid guard).
    lane: Option<MirLocalId>,
    /// Factors of the grid guard total, sorted.
    guard_factors: Vec<Sym>,
}

/// The `Index`-bearing places of a function, with whether each is a store.
#[derive(Default)]
struct IndexPlaces {
    found: Vec<(MirPlace, bool)>,
}

impl MirVisitor for IndexPlaces {
    fn visit_stmt(&mut self, stmt: &MirStatement) {
        if let MirStatementKind::Assign { place, value } = &stmt.kind {
            if place
                .projections
                .iter()
                .any(|projection| matches!(projection, MirProjection::Index(_)))
            {
                self.found.push((place.clone(), true));
            }
            for projection in &place.projections {
                if let MirProjection::Index(index) = projection {
                    self.visit_operand(index);
                }
            }
            self.visit_value(value);
        } else {
            crate::visit::walk_stmt(self, stmt);
        }
    }

    fn visit_place(&mut self, place: &MirPlace) {
        if place
            .projections
            .iter()
            .any(|projection| matches!(projection, MirProjection::Index(_)))
        {
            self.found.push((place.clone(), false));
        }
        crate::visit::walk_place(self, place);
    }
}

fn collect_writes(function: &MirFunction) -> Defs<'_> {
    let mut writes = Defs {
        single: BTreeMap::new(),
        multi: std::collections::BTreeSet::new(),
    };
    for block in &function.blocks {
        for statement in &block.statements {
            match &statement.kind {
                MirStatementKind::Assign { place, value } => writes.note(place, Some(value)),
                MirStatementKind::Call { destination, .. }
                | MirStatementKind::RuntimeCall { destination, .. } => {
                    if let Some(place) = destination {
                        writes.note(place, None);
                    }
                }
                MirStatementKind::Construct { destination, .. } => writes.note(destination, None),
            }
        }
        if let MirTerminatorKind::TryCall {
            destination,
            error_place,
            ..
        } = &block.terminator.kind
        {
            if let Some(place) = destination {
                writes.note(place, None);
            }
            writes.note(error_place, None);
        }
    }
    writes
}

impl<'a> LaneIndexProof<'a> {
    /// The proof state of `function`, or `None` when it is not a guarded
    /// lane-form kernel (no `Lane` parameter, a whole-grid reduction, or a
    /// grid guard that is not the entry block's branch): such a kernel carries
    /// no fault sites.
    #[must_use]
    pub fn new(
        function: &'a MirFunction,
        validation: &'a MirValidationContext<'a>,
    ) -> Option<Self> {
        let lane = function
            .params
            .iter()
            .find(|param| param.mode == MirParamMode::Lane)?
            .local;
        let entry = function.blocks.first()?;
        let MirTerminatorKind::Branch {
            condition,
            then_block,
            ..
        } = &entry.terminator.kind
        else {
            return None;
        };
        let exit = function.block_by_id(*then_block)?;
        if !exit.statements.is_empty()
            || !matches!(exit.terminator.kind, MirTerminatorKind::Return(None))
        {
            return None;
        }
        let mut proof = Self {
            function,
            validation,
            defs: collect_writes(function),
            lane: Some(lane),
            guard_factors: Vec::new(),
        };
        let total = proof.guard_total(condition)?;
        proof.guard_factors = factors(&total);
        Some(proof)
    }

    /// `total` of the entry guard `lane >= total`.
    fn guard_total(&self, condition: &MirOperand) -> Option<Sym> {
        let key = operand_key(condition)?;
        if self.defs.multi.contains(&key) {
            return None;
        }
        let value = self.defs.single.get(&key)?;
        let MirValueKind::Binary {
            op: MirBinOp::GtEq,
            lhs,
            rhs,
        } = &value.kind
        else {
            return None;
        };
        (self.eval_operand(lhs, 0) == Sym::Lane).then(|| self.eval_operand(rhs, 0))
    }

    fn eval_operand(&self, operand: &MirOperand, depth: u32) -> Sym {
        if depth > 64 {
            return Sym::Opaque;
        }
        if let MirOperand::Constant(MirConstant::Int(n)) = operand {
            return Sym::Const(*n);
        }
        let Some(key) = operand_key(operand) else {
            return Sym::Opaque;
        };
        if self.defs.multi.contains(&key) {
            return Sym::Opaque;
        }
        match self.defs.single.get(&key) {
            Some(value) => self.eval_value(&value.kind, depth + 1),
            None if self.lane.is_some_and(|lane| key == BaseKey::Local(lane.0)) => Sym::Lane,
            None => Sym::Opaque,
        }
    }

    fn eval_value(&self, kind: &MirValueKind, depth: u32) -> Sym {
        match kind {
            MirValueKind::Operand(operand) => self.eval_operand(operand, depth),
            MirValueKind::Binary { op, lhs, rhs } => {
                let lhs = self.eval_operand(lhs, depth);
                let rhs = self.eval_operand(rhs, depth);
                if lhs == Sym::Opaque || rhs == Sym::Opaque {
                    return Sym::Opaque;
                }
                match op {
                    MirBinOp::Add => simplify_add(lhs, rhs),
                    MirBinOp::Mul => Sym::Mul(Box::new(lhs), Box::new(rhs)),
                    MirBinOp::Div => Sym::Div(Box::new(lhs), Box::new(rhs)),
                    MirBinOp::Mod => Sym::Mod(Box::new(lhs), Box::new(rhs)),
                    _ => Sym::Opaque,
                }
            }
            MirValueKind::TensorExtent { tensor, axis } => {
                let MirOperand::Place(place) = tensor else {
                    return Sym::Opaque;
                };
                let (MirPlaceBase::Local(local), true) =
                    (&place.base, place.projections.is_empty())
                else {
                    return Sym::Opaque;
                };
                let local = resolve_parameter_alias(self.function, *local);
                self.tensor_dims(local)
                    .and_then(|dims| dims.get(*axis as usize).cloned())
                    .unwrap_or(Sym::Opaque)
            }
            _ => Sym::Opaque,
        }
    }

    /// The per-axis extents of tensor parameter `local`, from its type.
    fn tensor_dims(&self, local: MirLocalId) -> Option<Vec<Sym>> {
        let param = self
            .function
            .params
            .iter()
            .find(|param| param.local == local)?;
        tensor_type_dims(param.ty, self.validation)
    }

    /// Whether the `Index` place `place` (a store when `store`) is in range
    /// for every lane that passed the grid guard.
    #[must_use]
    pub fn is_proven(&self, place: &MirPlace, store: bool) -> bool {
        self.guards(place, store).is_empty()
    }

    /// The bounds compares `place` needs; empty when it is proven.
    #[must_use]
    pub fn guards(&self, place: &MirPlace, store: bool) -> Vec<IndexGuard> {
        let Some((tensor, index)) = self.addressed_tensor(place) else {
            return Vec::new();
        };
        let Some(dims) = self.tensor_dims(tensor) else {
            return Vec::new();
        };
        let covers_grid = factor_product(&dims) == self.guard_factors;
        if self.eval_operand(index, 0) == Sym::Lane {
            if store || covers_grid {
                return Vec::new();
            }
            return vec![IndexGuard::Flat(index.clone())];
        }
        let Some(axes) = self.coordinates(index, &dims) else {
            return vec![IndexGuard::Flat(index.clone())];
        };
        let mut guards = Vec::new();
        for (axis, (coordinate, extent)) in axes.iter().enumerate().skip(1) {
            if let Some(extent) = extent
                && !self.axis_proven(axis, coordinate, &dims, covers_grid)
            {
                guards.push(IndexGuard::Axis {
                    coordinate: coordinate.clone(),
                    extent: extent.clone(),
                });
            }
        }
        if !self.axis_proven(0, &axes[0].0, &dims, covers_grid) {
            guards.push(IndexGuard::Flat(index.clone()));
        }
        guards
    }

    /// The per-axis coordinates (and, for axes `>= 1`, the extent operand) of
    /// the row-major recombination `index` is, for a tensor of axis extents
    /// `dims`; `None` for rank 1 or when `index` is not that recombination.
    fn coordinates(
        &self,
        index: &MirOperand,
        dims: &[Sym],
    ) -> Option<Vec<(MirOperand, Option<MirOperand>)>> {
        if dims.len() < 2 {
            return None;
        }
        let mut axes = Vec::with_capacity(dims.len());
        let mut current = index.clone();
        for axis in (1..dims.len()).rev() {
            let (scaled, coordinate) = self.binary_parts(&current, |op| op == MirBinOp::Add)?;
            let (prefix, extent) = self.binary_parts(&scaled, |op| op == MirBinOp::Mul)?;
            if self.eval_operand(&extent, 0) != dims[axis] {
                return None;
            }
            axes.push((coordinate, Some(extent)));
            current = prefix;
        }
        axes.push((current, None));
        axes.reverse();
        Some(axes)
    }

    /// The operands of the single `lhs op rhs` definition of `operand`.
    fn binary_parts(
        &self,
        operand: &MirOperand,
        wanted: impl Fn(MirBinOp) -> bool,
    ) -> Option<(MirOperand, MirOperand)> {
        let key = operand_key(operand)?;
        if self.defs.multi.contains(&key) {
            return None;
        }
        let MirValueKind::Binary { op, lhs, rhs } = &self.defs.single.get(&key)?.kind else {
            return None;
        };
        wanted(*op).then(|| (lhs.clone(), rhs.clone()))
    }

    /// Whether the lane binders bound coordinate `coordinate` of axis `axis`
    /// in `[0, extent)`: axis `k` is `(lane / e_(k+1) / ... / e_(N-1)) % e_k`
    /// (axis 0 without the `%`, in range only when the tensor covers the grid).
    fn axis_proven(
        &self,
        axis: usize,
        coordinate: &MirOperand,
        dims: &[Sym],
        covers_grid: bool,
    ) -> bool {
        if axis == 0 && !covers_grid {
            return false;
        }
        let mut expected = Sym::Lane;
        for later in (axis + 1..dims.len()).rev() {
            expected = Sym::Div(Box::new(expected), Box::new(dims[later].clone()));
        }
        if axis > 0 {
            expected = Sym::Mod(Box::new(expected), Box::new(dims[axis].clone()));
        }
        self.eval_operand(coordinate, 0) == expected
    }

    /// The tensor parameter and the index operand of a one-`Index` place on a
    /// tensor parameter (a vector-element tensor's lane component follows the
    /// `Index`, and does not change what is bounds-checked); `None` for any
    /// other place (not a bounds-checked device storage index).
    fn addressed_tensor<'p>(&self, place: &'p MirPlace) -> Option<(MirLocalId, &'p MirOperand)> {
        let (MirProjection::Index(index), rest) = place.projections.split_first()? else {
            return None;
        };
        if !rest
            .iter()
            .all(|projection| matches!(projection, MirProjection::VectorLane(_)))
        {
            return None;
        }
        let MirPlaceBase::Local(local) = &place.base else {
            return None;
        };
        let local = resolve_parameter_alias(self.function, *local);
        self.tensor_dims(local)?;
        Some((local, index))
    }

    /// Every distinct unproven index form of the kernel, in tensor-parameter
    /// order, loads before stores.
    #[must_use]
    pub fn unproven(&self) -> Vec<UnprovenIndex> {
        let mut places = IndexPlaces::default();
        places.visit_function(self.function);
        let mut forms = std::collections::BTreeSet::new();
        for (place, store) in &places.found {
            if let Some((tensor, _)) = self.addressed_tensor(place)
                && !self.is_proven(place, *store)
            {
                forms.insert(UnprovenIndex {
                    tensor,
                    store: *store,
                });
            }
        }
        let mut forms: Vec<UnprovenIndex> = forms.into_iter().collect();
        let order = |local: MirLocalId| {
            self.function
                .params
                .iter()
                .position(|param| param.local == local)
                .unwrap_or(usize::MAX)
        };
        forms.sort_by_key(|form| (order(form.tensor), form.store));
        forms
    }
}

/// The unproven index forms of `function`; empty for a kernel that is not
/// guarded lane form.
#[must_use]
pub fn unproven_indexes(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Vec<UnprovenIndex> {
    LaneIndexProof::new(function, validation).map_or_else(Vec::new, |proof| proof.unproven())
}

/// `Add(Mul(Div(x, e), e), Mod(x, e))` is `x` (row-major recombination).
fn simplify_add(lhs: Sym, rhs: Sym) -> Sym {
    if let (Sym::Mul(scaled, e_mul), Sym::Mod(x_mod, e_mod)) = (&lhs, &rhs)
        && let Sym::Div(x_div, e_div) = scaled.as_ref()
        && x_div == x_mod
        && e_div == e_mod
        && e_mul == e_mod
    {
        return x_mod.as_ref().clone();
    }
    Sym::Add(Box::new(lhs), Box::new(rhs))
}

/// The multiplicative factors of `sym`, constants folded into one, sorted.
fn factors(sym: &Sym) -> Vec<Sym> {
    fn collect(sym: &Sym, out: &mut Vec<Sym>, constant: &mut i64) {
        match sym {
            Sym::Mul(a, b) => {
                collect(a, out, constant);
                collect(b, out, constant);
            }
            Sym::Const(n) => *constant = constant.saturating_mul(*n),
            other => out.push(other.clone()),
        }
    }
    let mut out = Vec::new();
    let mut constant = 1;
    collect(sym, &mut out, &mut constant);
    if constant != 1 {
        out.push(Sym::Const(constant));
    }
    out.sort();
    out
}

fn factor_product(dims: &[Sym]) -> Vec<Sym> {
    let mut out = Vec::new();
    let mut constant = 1_i64;
    for dim in dims {
        match dim {
            Sym::Const(n) => constant = constant.saturating_mul(*n),
            other => out.push(other.clone()),
        }
    }
    if constant != 1 {
        out.push(Sym::Const(constant));
    }
    out.sort();
    out
}

/// The axis extents of a tensor type: literals as constants, anything else as
/// a symbolic dimension. `None` when `ty` is not a ranked tensor.
fn tensor_type_dims(ty: MirType, validation: &MirValidationContext<'_>) -> Option<Vec<Sym>> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(_, shape) => match validation.types.get_index(*shape) {
            IndexExpr::Literal(count) => Some(vec![Sym::Const(i64::try_from(*count).ok()?)]),
            IndexExpr::Tuple(dims) => dims
                .iter()
                .map(|dim| match validation.types.get_index(*dim) {
                    IndexExpr::Literal(count) => Some(Sym::Const(i64::try_from(*count).ok()?)),
                    _ => Some(Sym::Dim(dim.0)),
                })
                .collect(),
            _ => None,
        },
        Type::Alias(_, target) => tensor_type_dims(MirType::semantic(*target), validation),
        _ => None,
    }
}
