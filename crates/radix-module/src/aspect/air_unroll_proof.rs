//! Lean proof source and route recognizer for AIR unroll equivalence.
//!
//! The proof is deliberately structural.  It models the lane sequence that
//! the landed AEU pass (`radix-semantic` `air_unroll.rs`) admits: one lane
//! value per iteration in source order, one straight-line binding block per
//! lane spliced in source order, the first failing lane aborting the whole
//! form.  It proves only that structural correspondence.  No floating-point
//! type, numeric-body semantics, or execution claim belongs here.
//!
//! The AEU pass itself is strictly read-only for this line: the recognizer
//! consumes its settled route facts (`itera ab` concrete literal ranges and
//! `itera ex` statically shaped sources) and never re-decides its budget,
//! mask, or purity policy beyond the shape facts named here.

use crate::hir::{HirExpression, HirExpressionKind, HirIteraMode, HirLiteral, HirRangeKind, HirUnOp};
use crate::semantic::passes::air_unroll::UNROLL_BUDGET;
use crate::semantic::{IndexExpr, Type, TypeTable};

/// Admitted AEU route whose lane sequence this proof line discharges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnrollRoute {
    /// Route A: `itera ab` over a concrete literal interval.  Lanes are the
    /// enumerated range values in source order.
    Ab,
    /// Route E: `itera ex` over a statically shaped source (bounded array or
    /// tensor).  Lanes are the flat lane indices `0..count-1` in source
    /// order; Route M coordinate tuples enumerate the same order.
    Ex,
}

/// Settled route facts consumed by the Line-1 recognizer and emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnrollRouteFacts {
    /// Which admitted AEU route produced the lane sequence.
    pub route: UnrollRoute,
    /// Lane values in source order, exactly as the pass enumerates them.
    pub lanes: Vec<i64>,
}

/// Recognize an admitted AEU lane sequence from a loop head's checked HIR.
///
/// Returns the settled route facts when the loop's source shape matches an
/// admitted AEU route within the pass's unroll budget, and `None` for every
/// decline shape (`itera de`, non-literal bounds, unknown shapes, rank
/// mismatches, over-budget charges).  Declines carry no proof obligation.
#[must_use]
pub fn unroll_route_facts(
    mode: HirIteraMode,
    coordinates: usize,
    source: &HirExpression,
    types: &TypeTable,
) -> Option<UnrollRouteFacts> {
    match mode {
        HirIteraMode::Ab => {
            let lanes = concrete_interval_values(source)?;
            within_budget(lanes.len()).then_some(UnrollRouteFacts { route: UnrollRoute::Ab, lanes })
        }
        HirIteraMode::Ex => {
            let count = i64::try_from(static_lane_count(coordinates, source, types)?).ok()?;
            let lanes: Vec<i64> = (0..count).collect();
            within_budget(lanes.len()).then_some(UnrollRouteFacts { route: UnrollRoute::Ex, lanes })
        }
        HirIteraMode::De => None,
    }
}

fn within_budget(lane_count: usize) -> bool {
    lane_count <= UNROLL_BUDGET
}

/// Concrete range values for an `itera ab` literal interval, mirroring the
/// pass's `concrete_range_values` enumeration order (start, step, kind).
fn concrete_interval_values(source: &HirExpression) -> Option<Vec<i64>> {
    let HirExpressionKind::Intervallum { start, end, step, kind } = &source.kind else {
        return None;
    };
    let start = literal_i64(start)?;
    let end = literal_i64(end)?;
    let stride = match step {
        Some(step) => literal_i64(step)?,
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
            return None;
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

fn literal_i64(expr: &HirExpression) -> Option<i64> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => i64::try_from(*value).ok(),
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => match &inner.kind {
            HirExpressionKind::Literal(HirLiteral::Int(value)) => i64::try_from(*value).ok().map(|n| -n),
            _ => None,
        },
        _ => None,
    }
}

/// Static lane count for an `itera ex` source, mirroring the pass's E-route
/// shapes: a bounded array of proven static length, or a tensor whose static
/// shape fixes the lane count (rank-1 when no `apud` coordinates are bound,
/// coordinate arity == rank for Route M).
fn static_lane_count(coordinates: usize, source: &HirExpression, types: &TypeTable) -> Option<usize> {
    let ty = source.ty?;
    let (_, ty) = types.peel_access_marker(ty);
    match types.get(ty) {
        Type::BoundedArray { capacity, .. } => {
            let IndexExpr::Literal(capacity) = types.get_index(*capacity) else {
                return None;
            };
            let capacity = usize::try_from(*capacity).ok()?;
            // The pass requires a proven exact length; the recognizer admits
            // the immutable array-literal construction site directly.
            let length = literal_array_length(source)?;
            (length == capacity).then_some(capacity)
        }
        Type::Tensor(_, shape) => {
            let dims: Vec<usize> = match types.get_index(*shape) {
                IndexExpr::Literal(count) => vec![usize::try_from(*count).ok()?],
                IndexExpr::Tuple(dims) => dims
                    .iter()
                    .map(|dim| match types.get_index(*dim) {
                        IndexExpr::Literal(value) => usize::try_from(*value).ok(),
                        _ => None,
                    })
                    .collect::<Option<_>>()?,
                _ => return None,
            };
            if coordinates == 0 {
                // Coordinate-free iteration admits only rank-1 (Route T).
                (dims.len() == 1).then_some(dims[0])
            } else {
                // Route M: coordinate arity == rank; lanes are the product
                // of dims in first-bound=outer order.
                (coordinates == dims.len()).then_some(())?;
                dims.iter()
                    .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))
            }
        }
        _ => None,
    }
}

fn literal_array_length(source: &HirExpression) -> Option<usize> {
    match &source.kind {
        HirExpressionKind::Array(elements)
            if elements
                .iter()
                .all(|element| matches!(element, crate::hir::HirArrayElement::Expr(_))) =>
        {
            Some(elements.len())
        }
        _ => None,
    }
}

/// Shared declarations used by every generated Line-1 theorem.
///
/// The model carries lane values as `Int` (the pass enumerates `i64` lanes),
/// a per-lane binding block, and an aborting step function.  Evaluation is
/// structural: it consumes the lane sequence and never computes numerics.
pub const UNROLL_PROOF_PRELUDE: &str = r"-- Machine-checked structural evidence for the AIR unroll route.
-- Structural only: lane sequences, splice order, and failure order. No
-- float type, no numeric-body semantics, no execution claim.

/-- Failure token: a lane aborts the whole form. -/
inductive LaneFailure where
  | abort : Int → LaneFailure

/-- One emitted straight-line statement: the lane's bound constants plus the
specialized body, collapsed to the lane value it carries. -/
inductive UnrollStmt where
  | lane : Int → UnrollStmt

def UnrollStmt.laneValue : UnrollStmt → Int
  | .lane value => value

/-- One lane step: consume the accumulator, continue or abort. -/
abbrev UnrollStep (α : Type u) := Int → α → Except LaneFailure α

/-- Source loop: consume the lane sequence in source order; the first
failing lane aborts the whole loop. -/
def sourceLaneLoop (step : UnrollStep α) : List Int → α → Except LaneFailure α
  | [], acc => .ok acc
  | lane :: rest, acc =>
      match step lane acc with
      | .ok acc' => sourceLaneLoop step rest acc'
      | .error e => .error e

/-- The binding block the unroll emits for one lane. -/
def laneBinding (lane : Int) : List UnrollStmt := [UnrollStmt.lane lane]

/-- The emitted form: one binding block per lane, spliced in source order. -/
def unrolledStraightLine : List Int → List UnrollStmt
  | [] => []
  | lane :: rest => laneBinding lane ++ unrolledStraightLine rest

/-- Sequential evaluation of the spliced straight-line bindings. -/
def evalUnrolled (step : UnrollStep α) : List UnrollStmt → α → Except LaneFailure α
  | [], acc => .ok acc
  | .lane lane :: rest, acc =>
      match step lane acc with
      | .ok acc' => evalUnrolled step rest acc'
      | .error e => .error e

def isAbort (result : Except LaneFailure α) : Bool :=
  match result with
  | .ok _ => false
  | .error _ => true

theorem evalUnrolled_append (step : UnrollStep α) (xs ys : List UnrollStmt) (acc : α) :
    evalUnrolled step (xs ++ ys) acc =
      (evalUnrolled step xs acc).bind (evalUnrolled step ys) := by
  induction xs generalizing acc with
  | nil => simp [evalUnrolled, Except.bind]
  | cons x xs ih =>
      cases x with
      | lane l =>
        simp only [List.cons_append, evalUnrolled]
        split
        · next acc' h => rw [ih acc']
        · next e h => simp [Except.bind]

/-- Lane-sequence equivalence: the lane constants carried by the emitted
straight-line bindings enumerate exactly the source lane sequence, in
source order. -/
theorem unrolledLanesMatchSource (lanes : List Int) :
    (unrolledStraightLine lanes).map UnrollStmt.laneValue = lanes := by
  induction lanes with
  | nil => rfl
  | cons lane rest ih =>
      simp [unrolledStraightLine, laneBinding, UnrollStmt.laneValue, ih]

/-- Source-order preservation: evaluating the spliced straight-line bindings
consumes the same lane sequence, in the same order, with the same
accumulator chain as the source loop. -/
theorem unrollEvaluationPreservesSourceOrder (step : UnrollStep α) (lanes : List Int) (acc : α) :
    evalUnrolled step (unrolledStraightLine lanes) acc = sourceLaneLoop step lanes acc := by
  induction lanes generalizing acc with
  | nil => rfl
  | cons lane rest ih =>
      simp only [unrolledStraightLine, sourceLaneLoop]
      rw [evalUnrolled_append]
      simp only [evalUnrolled, laneBinding]
      split
      · next acc' h => exact ih acc'
      · next e h => rfl

/-- Failure-behavior preservation: the emitted straight-line form aborts
exactly when the source loop aborts, with the identical failure value
(the first failing lane). -/
theorem unrollFailureBehaviorPreserved (step : UnrollStep α) (lanes : List Int) (acc : α) :
    isAbort (evalUnrolled step (unrolledStraightLine lanes) acc) =
      isAbort (sourceLaneLoop step lanes acc) := by
  rw [unrollEvaluationPreservesSourceOrder]
";

/// Emit one real theorem for a recognized unroll-equivalence obligation.
///
/// The theorem instantiates the lane-sequence, source-order, and
/// failure-behavior master theorems at the route's concrete lane sequence.
#[must_use]
pub fn emit_unroll_equivalence_theorem(name: &str, lanes: &[i64]) -> String {
    let lane_list = lanes
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "theorem {name} (step : Int → Nat → Except LaneFailure Nat) :\n\
    evalUnrolled step (unrolledStraightLine [{lane_list}]) 0 =\n\
      sourceLaneLoop step [{lane_list}] 0 ∧\n\
    isAbort (evalUnrolled step (unrolledStraightLine [{lane_list}]) 0) =\n\
      isAbort (sourceLaneLoop step [{lane_list}] 0) :=\n\
  ⟨unrollEvaluationPreservesSourceOrder step [{lane_list}] 0,\n\
   by rw [unrollEvaluationPreservesSourceOrder step [{lane_list}] 0]⟩\n"
    )
}

#[cfg(test)]
#[path = "air_unroll_proof_test.rs"]
mod tests;
