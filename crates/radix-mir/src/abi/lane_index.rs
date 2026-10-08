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
//! function: the emitter asks [`LaneIndexProof::is_proven`] for each `Index`
//! place it writes.

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

/// The proof state for one lane-form function.
pub struct LaneIndexProof<'a> {
    function: &'a MirFunction,
    validation: &'a MirValidationContext<'a>,
    defs: Defs<'a>,
    lane: MirLocalId,
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

/// Collects every written local or temp.
struct Writes<'a> {
    single: BTreeMap<BaseKey, &'a crate::MirValue>,
    multi: std::collections::BTreeSet<BaseKey>,
}

impl<'a> Writes<'a> {
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

fn collect_writes(function: &MirFunction) -> Defs<'_> {
    let mut writes = Writes {
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
    Defs {
        single: writes.single,
        multi: writes.multi,
    }
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
            lane,
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
            None if key == BaseKey::Local(self.lane.0) => Sym::Lane,
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
        let Some((tensor, index)) = self.addressed_tensor(place) else {
            return true;
        };
        if self.eval_operand(index, 0) != Sym::Lane {
            return false;
        }
        if store {
            return true;
        }
        self.tensor_dims(tensor)
            .is_some_and(|dims| factor_product(&dims) == self.guard_factors)
    }

    /// The tensor parameter and the index operand of a one-`Index` place on a
    /// tensor parameter; `None` for any other place (not a bounds-checked
    /// device storage index).
    fn addressed_tensor<'p>(&self, place: &'p MirPlace) -> Option<(MirLocalId, &'p MirOperand)> {
        let [MirProjection::Index(index)] = place.projections.as_slice() else {
            return None;
        };
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
