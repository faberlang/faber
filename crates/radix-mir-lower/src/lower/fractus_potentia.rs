//! Fractus `potentia` lowering: IEEE 754 `pow` composed from existing MIR ops.
//!
//! Faber float math follows IEEE 754 results (operator ruling 2026-09-24),
//! so `x.potentia(y)` must match C99 Annex F `pow` — not the retired
//! `base ≤ 0 → 0` guard. MIR carries no native `pow` op, so the lowering
//! spells the special cases as branches and computes the finite core from
//! `Mul`/`Div`/`Mod` plus `MirUnOp::{Log, Exp}`:
//!
//! - `y = 0 → 1` and `x = 1 → 1`, even for NaN; otherwise NaN propagates.
//! - `y = ±∞`: `(-1)^±∞ = 1`; else `∞` or `0` by whether `|x| > 1` agrees
//!   with the exponent sign.
//! - `x = ±0` / `x = ±∞`: pole and magnitude rules, signed for odd `y`.
//! - `x < 0`: NaN for non-integral `y`; odd `y` negates `|x|^y`.
//! - finite `x > 0`: integral `|y| ≤ 64` uses exact repeated squaring (so
//!   `2^3 = 8` exactly), `y = ½` uses `sqrt`, everything else is
//!   `exp(y · ln x)` (within a few ulps of libm `pow`, not correctly rounded).
//!
//! WHY the extra guards around `exp`: the MIR runner rejects `Log` of a
//! non-positive operand and `Exp` with a non-finite result, so the CFG only
//! reaches `Log` with a finite positive base and splits large exponents as
//! `exp(t/2)²`, which overflows to `∞` in the multiply instead of the `Exp`.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use radix_hir::HirCallArg;

/// Largest integral `|y|` the exact repeated-squaring path accepts. Squaring
/// error grows with the exponent; beyond this the `exp · ln` path is as good.
const EXACT_EXPONENT_BOUND: f64 = 64.0;

/// Above this `t = y · ln x`, `exp(t)` may overflow; compute `exp(t/2)²`.
const EXP_SPLIT_THRESHOLD: f64 = 709.0;

/// Above this `t`, `exp(t/2)` itself would overflow; the result is `∞`.
const EXP_OVERFLOW_THRESHOLD: f64 = 1418.0;

/// Shared frame for one `pow` CFG: result temp, join block, and types.
#[derive(Clone, Copy)]
struct PowFrame {
    ty: MirType,
    bool_ty: MirType,
    span: Span,
    result: MirTempId,
    join: MirBlockId,
}

fn float(value: f64) -> MirOperand {
    MirOperand::Constant(MirConstant::Float(value))
}

impl FunctionBuilder<'_> {
    /// Lower `potentia` for a fractus-family receiver as IEEE 754 `pow`.
    pub(super) fn lower_fractus_potentia(
        &mut self,
        receiver: &HirExpression,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let [arg] = args else {
            return None;
        };
        let x = self.lower_expr_value(receiver)?;
        let y = self.lower_expr_value(&arg.expr)?;
        let span = expr.span;
        let frame = PowFrame {
            ty,
            bool_ty: MirType::semantic(self.types.primitive(Primitive::Bivalens)),
            span,
            result: self.push_temp(ty, span),
            join: self.fresh_block(span),
        };

        // x^0 = 1 and 1^y = 1, even for NaN operands.
        let y_zero = self.pow_bool(MirBinOp::Eq, y.clone(), float(0.0), frame);
        self.pow_when(y_zero, frame, |this, _| this.pow_return(float(1.0), frame));
        let x_one = self.pow_bool(MirBinOp::Eq, x.clone(), float(1.0), frame);
        self.pow_when(x_one, frame, |this, _| this.pow_return(float(1.0), frame));

        // NaN in → NaN out. `v == v` is false exactly for NaN on every lane.
        let x_ordered = self.pow_bool(MirBinOp::Eq, x.clone(), x.clone(), frame);
        let y_ordered = self.pow_bool(MirBinOp::Eq, y.clone(), y.clone(), frame);
        let ordered = self.pow_bool(MirBinOp::And, x_ordered, y_ordered, frame);
        self.pow_unless(ordered, frame, |this, _| this.pow_return(float(f64::NAN), frame));

        // y = ±∞: (-1)^±∞ = 1; otherwise ∞ or 0 by |x| > 1 against the sign of y.
        let y_pos_inf = self.pow_bool(MirBinOp::Eq, y.clone(), float(f64::INFINITY), frame);
        let y_neg_inf = self.pow_bool(MirBinOp::Eq, y.clone(), float(f64::NEG_INFINITY), frame);
        let y_inf = self.pow_bool(MirBinOp::Or, y_pos_inf.clone(), y_neg_inf, frame);
        let x_minus_one = self.pow_bool(MirBinOp::Eq, x.clone(), float(-1.0), frame);
        let unit_inf = self.pow_bool(MirBinOp::And, y_inf, x_minus_one, frame);
        self.pow_when(unit_inf, frame, |this, _| this.pow_return(float(1.0), frame));
        let x_above = self.pow_bool(MirBinOp::Gt, x.clone(), float(1.0), frame);
        let x_below = self.pow_bool(MirBinOp::Lt, x.clone(), float(-1.0), frame);
        let x_big = self.pow_bool(MirBinOp::Or, x_above, x_below, frame);
        self.pow_when(y_pos_inf, frame, |this, _| {
            this.pow_select(x_big.clone(), float(f64::INFINITY), float(0.0), frame);
        });
        let y_neg_inf = self.pow_bool(MirBinOp::Eq, y.clone(), float(f64::NEG_INFINITY), frame);
        self.pow_when(y_neg_inf, frame, |this, _| {
            this.pow_select(x_big, float(0.0), float(f64::INFINITY), frame);
        });

        // y is finite and nonzero from here, so `fmod` is exact and never NaN.
        let whole_rem = self.binary_temp(MirBinOp::Mod, y.clone(), float(1.0), ty, span);
        let y_integral = self.pow_bool(MirBinOp::Eq, whole_rem.clone(), float(0.0), frame);
        let parity_rem = self.binary_temp(MirBinOp::Mod, y.clone(), float(2.0), ty, span);
        let parity_set = self.pow_bool(MirBinOp::NotEq, parity_rem, float(0.0), frame);
        let y_odd = self.pow_bool(MirBinOp::And, y_integral.clone(), parity_set, frame);
        let y_negative = self.pow_bool(MirBinOp::Lt, y.clone(), float(0.0), frame);
        // 1/x carries the sign of a zero or infinite base into its pole or zero.
        let reciprocal = self.binary_temp(MirBinOp::Div, float(1.0), x.clone(), ty, span);

        // x = ±0: y < 0 → odd ? 1/x (±∞) : ∞;  y > 0 → odd ? x (±0) : 0.
        let x_zero = self.pow_bool(MirBinOp::Eq, x.clone(), float(0.0), frame);
        self.pow_when(x_zero, frame, |this, _| {
            this.pow_when(y_negative.clone(), frame, |this, _| {
                this.pow_select(y_odd.clone(), reciprocal.clone(), float(f64::INFINITY), frame);
            });
            this.pow_select(y_odd.clone(), x.clone(), float(0.0), frame);
        });

        // x = ±∞: y < 0 → odd ? 1/x (±0) : 0;  y > 0 → odd ? x (±∞) : ∞.
        let x_pos_inf = self.pow_bool(MirBinOp::Eq, x.clone(), float(f64::INFINITY), frame);
        let x_neg_inf = self.pow_bool(MirBinOp::Eq, x.clone(), float(f64::NEG_INFINITY), frame);
        let x_inf = self.pow_bool(MirBinOp::Or, x_pos_inf, x_neg_inf, frame);
        self.pow_when(x_inf, frame, |this, _| {
            this.pow_when(y_negative.clone(), frame, |this, _| {
                this.pow_select(y_odd.clone(), reciprocal.clone(), float(0.0), frame);
            });
            this.pow_select(y_odd.clone(), x.clone(), float(f64::INFINITY), frame);
        });

        // Finite x < 0: NaN for non-integral y; odd y negates |x|^y.
        let x_negative = self.pow_bool(MirBinOp::Lt, x.clone(), float(0.0), frame);
        self.pow_when(x_negative, frame, |this, _| {
            let y_fractional = this.pow_bool(MirBinOp::NotEq, whole_rem.clone(), float(0.0), frame);
            this.pow_when(y_fractional, frame, |this, _| this.pow_return(float(f64::NAN), frame));
            let magnitude_base =
                this.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: x.clone() }, ty, span);
            let magnitude =
                this.lower_positive_pow(magnitude_base, y.clone(), y_integral.clone(), y_negative.clone(), frame);
            let negated =
                this.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: magnitude.clone() }, ty, span);
            this.pow_select(y_odd.clone(), negated, magnitude, frame);
        });

        let magnitude = self.lower_positive_pow(x, y, y_integral, y_negative, frame);
        self.pow_return(magnitude, frame);

        self.switch_to(frame.join);
        Some(MirOperand::Temp(frame.result))
    }

    /// `a^y` for finite `a > 0` and finite nonzero `y`; leaves the CFG open in
    /// a block holding the returned value.
    fn lower_positive_pow(
        &mut self,
        a: MirOperand,
        y: MirOperand,
        y_integral: MirOperand,
        y_negative: MirOperand,
        frame: PowFrame,
    ) -> MirOperand {
        let PowFrame { ty, span, .. } = frame;
        let out = self.push_temp(ty, span);
        let out_place = MirPlace::temp(out);
        let done = self.fresh_block(span);
        let exact = self.fresh_block(span);
        let transcendental = self.fresh_block(span);

        let not_above = self.pow_bool(MirBinOp::LtEq, y.clone(), float(EXACT_EXPONENT_BOUND), frame);
        let not_below = self.pow_bool(MirBinOp::GtEq, y.clone(), float(-EXACT_EXPONENT_BOUND), frame);
        let bounded = self.pow_bool(MirBinOp::And, not_above, not_below, frame);
        let small = self.pow_bool(MirBinOp::And, y_integral, bounded, frame);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: small, then_block: exact, else_block: transcendental },
            span,
        );

        // Exact path: repeated squaring over n = |y| (an integer ≤ 64).
        self.switch_to(exact);
        let accumulator = self.pow_local(float(1.0), frame);
        let base = self.pow_local(a.clone(), frame);
        let count = self.pow_local(y.clone(), frame);
        self.pow_when(y_negative.clone(), frame, |this, next| {
            let flipped = this.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: y.clone() }, ty, span);
            this.assign(count.clone(), flipped, ty, span);
            this.terminate_current(MirTerminatorKind::Goto(next), span);
        });
        let loop_check = self.fresh_block(span);
        let body = self.fresh_block(span);
        let square = self.fresh_block(span);
        let finish = self.fresh_block(span);
        self.terminate_current(MirTerminatorKind::Goto(loop_check), span);

        self.switch_to(loop_check);
        let remaining = self.pow_bool(MirBinOp::Gt, MirOperand::Place(count.clone()), float(0.0), frame);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: remaining, then_block: body, else_block: finish },
            span,
        );

        self.switch_to(body);
        let bit = self.binary_temp(MirBinOp::Mod, MirOperand::Place(count.clone()), float(2.0), ty, span);
        let odd = self.pow_bool(MirBinOp::NotEq, bit.clone(), float(0.0), frame);
        self.pow_when(odd, frame, |this, next| {
            let product = this.binary_temp(
                MirBinOp::Mul,
                MirOperand::Place(accumulator.clone()),
                MirOperand::Place(base.clone()),
                ty,
                span,
            );
            this.assign(accumulator.clone(), product, ty, span);
            this.terminate_current(MirTerminatorKind::Goto(next), span);
        });
        let even = self.binary_temp(MirBinOp::Sub, MirOperand::Place(count.clone()), bit, ty, span);
        let half = self.binary_temp(MirBinOp::Div, even, float(2.0), ty, span);
        self.assign(count.clone(), half, ty, span);
        let more = self.pow_bool(MirBinOp::Gt, MirOperand::Place(count.clone()), float(0.0), frame);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: more, then_block: square, else_block: loop_check },
            span,
        );

        self.switch_to(square);
        let squared = self.binary_temp(
            MirBinOp::Mul,
            MirOperand::Place(base.clone()),
            MirOperand::Place(base.clone()),
            ty,
            span,
        );
        self.assign(base, squared, ty, span);
        self.terminate_current(MirTerminatorKind::Goto(loop_check), span);

        // Negative y takes the reciprocal; an overflowed |x|^n whose reciprocal
        // is subnormal falls through to the transcendental path instead of 0.
        self.switch_to(finish);
        let raised = MirOperand::Place(accumulator);
        let positive = self.fresh_block(span);
        let negative = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: y_negative, then_block: negative, else_block: positive },
            span,
        );
        self.switch_to(positive);
        self.assign(out_place.clone(), raised.clone(), ty, span);
        self.terminate_current(MirTerminatorKind::Goto(done), span);
        self.switch_to(negative);
        let overflowed = self.pow_bool(MirBinOp::Eq, raised.clone(), float(f64::INFINITY), frame);
        let invert = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: overflowed, then_block: transcendental, else_block: invert },
            span,
        );
        self.switch_to(invert);
        let inverse = self.binary_temp(MirBinOp::Div, float(1.0), raised, ty, span);
        self.assign(out_place.clone(), inverse, ty, span);
        self.terminate_current(MirTerminatorKind::Goto(done), span);

        // Transcendental path: y = ½ is the correctly rounded `sqrt`; otherwise
        // exp(t) with t = y · ln a, split for large t.
        self.switch_to(transcendental);
        let y_half = self.pow_bool(MirBinOp::Eq, y.clone(), float(0.5), frame);
        self.pow_when(y_half, frame, |this, _| {
            let root = this.assign_temp(MirValueKind::Unary { op: MirUnOp::Sqrt, operand: a.clone() }, ty, span);
            this.assign(out_place.clone(), root, ty, span);
            this.terminate_current(MirTerminatorKind::Goto(done), span);
        });
        let log = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Log, operand: a }, ty, span);
        let t = self.binary_temp(MirBinOp::Mul, y, log, ty, span);
        let direct = self.fresh_block(span);
        let large = self.fresh_block(span);
        let split = self.fresh_block(span);
        let overflow = self.fresh_block(span);
        let may_overflow = self.pow_bool(MirBinOp::Gt, t.clone(), float(EXP_SPLIT_THRESHOLD), frame);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: may_overflow, then_block: large, else_block: direct },
            span,
        );
        self.switch_to(direct);
        let raised = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Exp, operand: t.clone() }, ty, span);
        self.assign(out_place.clone(), raised, ty, span);
        self.terminate_current(MirTerminatorKind::Goto(done), span);
        self.switch_to(large);
        let overflows = self.pow_bool(MirBinOp::Gt, t.clone(), float(EXP_OVERFLOW_THRESHOLD), frame);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: overflows, then_block: overflow, else_block: split },
            span,
        );
        self.switch_to(overflow);
        self.assign(out_place.clone(), float(f64::INFINITY), ty, span);
        self.terminate_current(MirTerminatorKind::Goto(done), span);
        self.switch_to(split);
        let half_t = self.binary_temp(MirBinOp::Mul, t, float(0.5), ty, span);
        let root = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Exp, operand: half_t }, ty, span);
        let squared = self.binary_temp(MirBinOp::Mul, root.clone(), root, ty, span);
        self.assign(out_place, squared, ty, span);
        self.terminate_current(MirTerminatorKind::Goto(done), span);

        self.switch_to(done);
        MirOperand::Temp(out)
    }

    fn pow_bool(&mut self, op: MirBinOp, lhs: MirOperand, rhs: MirOperand, frame: PowFrame) -> MirOperand {
        self.binary_temp(op, lhs, rhs, frame.bool_ty, frame.span)
    }

    fn pow_local(&mut self, initial: MirOperand, frame: PowFrame) -> MirPlace {
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: None, ty: frame.ty, mutable: true, span: frame.span });
        let place = MirPlace::local(id);
        self.assign(place.clone(), initial, frame.ty, frame.span);
        place
    }

    /// Store `value` as the `pow` result and jump to the join block.
    fn pow_return(&mut self, value: MirOperand, frame: PowFrame) {
        self.assign(MirPlace::temp(frame.result), value, frame.ty, frame.span);
        self.terminate_current(MirTerminatorKind::Goto(frame.join), frame.span);
    }

    /// Return `when_true` or `when_false` by `condition`.
    fn pow_select(&mut self, condition: MirOperand, when_true: MirOperand, when_false: MirOperand, frame: PowFrame) {
        let then_block = self.fresh_block(frame.span);
        let else_block = self.fresh_block(frame.span);
        self.terminate_current(MirTerminatorKind::Branch { condition, then_block, else_block }, frame.span);
        self.switch_to(then_block);
        self.pow_return(when_true, frame);
        self.switch_to(else_block);
        self.pow_return(when_false, frame);
    }

    /// Run `body` when `condition` holds, then continue lowering on the
    /// fall-through edge. `body` must terminate its block — by returning a
    /// result or by jumping to the fall-through block it is handed.
    fn pow_when(&mut self, condition: MirOperand, frame: PowFrame, body: impl FnOnce(&mut Self, MirBlockId)) {
        self.pow_branch(condition, true, frame, body);
    }

    /// Run `body` (same contract as [`Self::pow_when`]) when `condition` fails.
    fn pow_unless(&mut self, condition: MirOperand, frame: PowFrame, body: impl FnOnce(&mut Self, MirBlockId)) {
        self.pow_branch(condition, false, frame, body);
    }

    fn pow_branch(
        &mut self,
        condition: MirOperand,
        on: bool,
        frame: PowFrame,
        body: impl FnOnce(&mut Self, MirBlockId),
    ) {
        let hit = self.fresh_block(frame.span);
        let next = self.fresh_block(frame.span);
        let (then_block, else_block) = if on { (hit, next) } else { (next, hit) };
        self.terminate_current(MirTerminatorKind::Branch { condition, then_block, else_block }, frame.span);
        self.switch_to(hit);
        body(self, next);
        self.switch_to(next);
    }
}
