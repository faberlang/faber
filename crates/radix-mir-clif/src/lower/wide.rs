//! Exact integer intermediates (ruling 17a, CTO ruling 2026-10-15).
//!
//! An integer expression computes the exact value, and every operand and
//! intermediate lies in `[-2^63, 2^64 - 1]`; a value outside it traps. A
//! variable or slot holds an `i64`, so an expression whose type is the cap
//! (`int`, `i64`, `u64`) computes in an `i128` carrier instead: `a + 1` of
//! the largest `int` is a value the next operation can still use, and only the
//! store into a bounded slot decides whether it fits.
//!
//! The carrier is a property of the Cranelift value, not of [`Rep`]: a value
//! of type `I128` is a wide integer. Reading an operand for any consumer that
//! needs an `i64` ([`FunctionLower::scalar`]) narrows it with a range check;
//! arithmetic, comparison, display and the implicit store read it wide
//! ([`FunctionLower::scalar_wide`]). A temporary that only arithmetic defines
//! is declared `I128` ([`wide_temps`]); every other slot stays `I64`.
//!
//! Multiplication of two operands known to come from `i64` is one `imul`; any
//! other product and every wide division go through the runtime, because
//! Cranelift lowers neither an `i128` overflow check nor `i128` division.

use super::{
    Decline, FunctionLower, InstBuilder, IntCC, Lowered, MirBinOp, MirFunction, MirOperand,
    MirPlace, MirPlaceBase, MirStatementKind, MirTempId, MirTerminatorKind, MirType, MirUnOp,
    MirValueKind, NumericWidth, Primitive, Rep, RuntimeSymbol, Type, TypeTable, Value, abi, rep_of,
    resolve, trap_text, types,
};

/// The lowest exact integer.
const EXACT_MIN: i128 = i64::MIN as i128;
/// The highest exact integer.
const EXACT_MAX: i128 = u64::MAX as i128;

/// Whether `ty` is the cap of the integer range, where an exact intermediate
/// can leave the `i64` carrier.
pub(super) fn cap_int(table: &TypeTable, ty: MirType) -> bool {
    matches!(
        resolve(table, ty.semantic_id()),
        Type::Primitive(Primitive::Numerus)
            | Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64 | NumericWidth::U64)
    )
}

/// Whether `op` is an integer operation that produces a number.
pub(super) fn arithmetic(op: MirBinOp) -> bool {
    matches!(
        op,
        MirBinOp::Add
            | MirBinOp::Sub
            | MirBinOp::Mul
            | MirBinOp::Div
            | MirBinOp::Mod
            | MirBinOp::BitAnd
            | MirBinOp::BitOr
            | MirBinOp::BitXor
            | MirBinOp::Shl
            | MirBinOp::Shr
    )
}

/// The temporaries that only integer arithmetic (or a copy of one such
/// temporary) defines: they hold an `i128`.
pub(super) fn wide_temps(
    table: &TypeTable,
    function: &MirFunction,
) -> handroll::FxHashSet<MirTempId> {
    use handroll::FxHashSet;
    let mut candidates: FxHashSet<MirTempId> = function
        .temps
        .iter()
        .filter(|temp| cap_int(table, temp.ty) && rep_of(table, temp.ty) == Ok(Rep::Int))
        .map(|temp| temp.id)
        .collect();
    let temp_of = |place: &MirPlace| match place.base {
        MirPlaceBase::Temp(id) => Some(id),
        MirPlaceBase::Local(_) => None,
    };
    loop {
        let mut removed = Vec::new();
        for block in &function.blocks {
            for statement in &block.statements {
                match &statement.kind {
                    MirStatementKind::Assign { place, value } => {
                        let Some(id) = temp_of(place) else { continue };
                        let arithmetic = place.projections.is_empty()
                            && match &value.kind {
                                MirValueKind::Binary { op, .. } => arithmetic(*op),
                                MirValueKind::Unary { op, .. } => {
                                    matches!(op, MirUnOp::Neg | MirUnOp::BitNot)
                                }
                                MirValueKind::Operand(MirOperand::Temp(source)) => {
                                    candidates.contains(source)
                                }
                                MirValueKind::Operand(MirOperand::Place(source)) => {
                                    source.projections.is_empty()
                                        && temp_of(source).is_some_and(|s| candidates.contains(&s))
                                }
                                _ => false,
                            };
                        if !arithmetic {
                            removed.push(id);
                        }
                    }
                    MirStatementKind::Call { destination, .. }
                    | MirStatementKind::RuntimeCall { destination, .. } => {
                        removed.extend(destination.as_ref().and_then(temp_of));
                    }
                    MirStatementKind::Construct { destination, .. } => {
                        removed.extend(temp_of(destination));
                    }
                }
            }
            if let MirTerminatorKind::TryCall {
                destination,
                error_place,
                ..
            } = &block.terminator.kind
            {
                removed.extend(destination.as_ref().and_then(temp_of));
                removed.extend(temp_of(error_place));
            }
        }
        let before = candidates.len();
        for id in removed {
            candidates.remove(&id);
        }
        if candidates.len() == before {
            return candidates;
        }
    }
}

impl FunctionLower<'_, '_> {
    pub(super) fn is_wide(&self, value: Value) -> bool {
        self.builder.func.dfg.value_type(value) == types::I128
    }

    /// An `i64` value as the wide carrier; a wide value stays.
    pub(super) fn widen(&mut self, value: Value) -> Value {
        if self.is_wide(value) {
            return value;
        }
        let wide = self.builder.ins().sextend(types::I128, value);
        self.narrow.insert(wide);
        wide
    }

    pub(super) fn wide_const(&mut self, value: i128) -> Value {
        let bytes = value.to_le_bytes();
        let low = i64::from_le_bytes(std::array::from_fn(|index| bytes[index]));
        let low = self.builder.ins().iconst(types::I64, low);
        let high = self.builder.ins().iconst(types::I64, (value >> 64) as i64);
        self.builder.ins().iconcat(low, high)
    }

    pub(super) fn wide_zero(&mut self) -> Value {
        self.wide_const(0)
    }

    /// Traps unless the wide value lies in the exact range.
    fn range_guard(&mut self, value: Value) -> Lowered<()> {
        let low = self.wide_const(EXACT_MIN);
        let high = self.wide_const(EXACT_MAX);
        let below = self.builder.ins().icmp(IntCC::SignedLessThan, value, low);
        let above = self
            .builder
            .ins()
            .icmp(IntCC::SignedGreaterThan, value, high);
        let outside = self.builder.ins().bor(below, above);
        self.trap_if(outside, trap_text::NUMERUS_OVERFLOW)
    }

    /// A wide value as the `i64` carrier, trapping when it does not fit; an
    /// `i64` value stays.
    pub(super) fn narrow(&mut self, value: Value) -> Lowered<Value> {
        if !self.is_wide(value) {
            return Ok(value);
        }
        if self.huge.contains(&value) {
            return Err(Decline::new("integer constant beyond i64"));
        }
        let low = self.builder.ins().ireduce(types::I64, value);
        let back = self.builder.ins().sextend(types::I128, low);
        let differs = self.builder.ins().icmp(IntCC::NotEqual, back, value);
        self.trap_if(differs, trap_text::NUMERUS_OVERFLOW)?;
        Ok(low)
    }

    /// An integer value in the carrier its destination slot holds.
    pub(super) fn fit_int(&mut self, value: Value, wide: bool) -> Lowered<Value> {
        if wide {
            Ok(self.widen(value))
        } else {
            self.narrow(value)
        }
    }

    /// The two halves of a wide value.
    pub(super) fn split_wide(&mut self, value: Value) -> (Value, Value) {
        self.builder.ins().isplit(value)
    }

    fn runtime_wide(&mut self, symbol: &RuntimeSymbol, args: &[Value]) -> Lowered<Value> {
        let low = self
            .call_runtime(symbol, args)?
            .ok_or_else(|| Decline::new("wide arithmetic result"))?;
        let high = self
            .call_runtime(&abi::WIDE_HI, &[])?
            .ok_or_else(|| Decline::new("wide arithmetic result"))?;
        Ok(self.builder.ins().iconcat(low, high))
    }

    /// One integer operation over exact operands; `None` for a comparison.
    pub(super) fn wide_arithmetic(&mut self, op: MirBinOp, a: Value, b: Value) -> Lowered<Value> {
        let (a, b) = (self.widen(a), self.widen(b));
        let both_narrow = self.narrow.contains(&a) && self.narrow.contains(&b);
        let result = match op {
            MirBinOp::Add => self.builder.ins().iadd(a, b),
            MirBinOp::Sub => self.builder.ins().isub(a, b),
            MirBinOp::Mul if both_narrow => self.builder.ins().imul(a, b),
            MirBinOp::Mul => {
                let (alo, ahi) = self.split_wide(a);
                let (blo, bhi) = self.split_wide(b);
                return self.runtime_wide(&abi::WIDE_MUL, &[alo, ahi, blo, bhi]);
            }
            MirBinOp::Div | MirBinOp::Mod => {
                let remainder = op == MirBinOp::Mod;
                if both_narrow {
                    let a64 = self.builder.ins().ireduce(types::I64, a);
                    let b64 = self.builder.ins().ireduce(types::I64, b);
                    return self.floor_div_exact(a64, b64, remainder);
                }
                let (alo, ahi) = self.split_wide(a);
                let (blo, bhi) = self.split_wide(b);
                let flag = self.builder.ins().iconst(types::I32, i64::from(remainder));
                return self.runtime_wide(&abi::WIDE_DIV, &[alo, ahi, blo, bhi, flag]);
            }
            MirBinOp::BitAnd => self.builder.ins().band(a, b),
            MirBinOp::BitOr => self.builder.ins().bor(a, b),
            MirBinOp::BitXor => self.builder.ins().bxor(a, b),
            MirBinOp::Shl => return self.wide_shift_left(a, b),
            MirBinOp::Shr => return self.wide_shift_right(a, b),
            _ => return Err(Decline::new("integer operation")),
        };
        self.range_guard(result)?;
        Ok(result)
    }

    /// Exact floor division or remainder of two `i64` operands. `i64::MIN / -1`
    /// is `2^63`, a value of the exact range, so a divisor of `-1` negates in
    /// the wide carrier instead of going through the hardware divide.
    fn floor_div_exact(&mut self, a: Value, b: Value, remainder: bool) -> Lowered<Value> {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let is_zero = self.builder.ins().icmp(IntCC::Equal, b, zero);
        self.trap_if(is_zero, "numerus division failed")?;
        let minus_one = self.builder.ins().iconst(types::I64, -1);
        let is_minus_one = self.builder.ins().icmp(IntCC::Equal, b, minus_one);
        let one = self.builder.ins().iconst(types::I64, 1);
        let safe = self.builder.ins().select(is_minus_one, one, b);
        let quotient = self.builder.ins().sdiv(a, safe);
        let rest = self.builder.ins().srem(a, safe);
        let rest_nonzero = self.builder.ins().icmp(IntCC::NotEqual, rest, zero);
        let signs = self.builder.ins().bxor(rest, safe);
        let signs_differ = self.builder.ins().icmp(IntCC::SignedLessThan, signs, zero);
        let adjust = self.builder.ins().band(rest_nonzero, signs_differ);
        let adjust_wide = self.builder.ins().uextend(types::I64, adjust);
        let floored = self.builder.ins().isub(quotient, adjust_wide);
        let floored_rest = {
            let corrected = self.builder.ins().iadd(rest, safe);
            self.builder.ins().select(adjust, corrected, rest)
        };
        if remainder {
            let chosen = self.builder.ins().select(is_minus_one, zero, floored_rest);
            return Ok(self.builder.ins().sextend(types::I128, chosen));
        }
        let wide_a = self.builder.ins().sextend(types::I128, a);
        let negated = self.builder.ins().ineg(wide_a);
        let wide_floored = self.builder.ins().sextend(types::I128, floored);
        Ok(self
            .builder
            .ins()
            .select(is_minus_one, negated, wide_floored))
    }

    /// `x ⇐ n` is `x · 2ⁿ`; a negative count is an error, and a result past
    /// the exact range traps.
    fn wide_shift_left(&mut self, a: Value, count: Value) -> Lowered<Value> {
        let zero = self.wide_const(0);
        let negative = self.builder.ins().icmp(IntCC::SignedLessThan, count, zero);
        self.trap_if(negative, "negative shift count")?;
        let limit = self.wide_const(64);
        let large = self
            .builder
            .ins()
            .icmp(IntCC::SignedGreaterThan, count, limit);
        let nonzero = self.builder.ins().icmp(IntCC::NotEqual, a, zero);
        let too_far = self.builder.ins().band(large, nonzero);
        self.trap_if(too_far, trap_text::NUMERUS_OVERFLOW)?;
        let bounded = self.builder.ins().select(large, zero, count);
        let amount = self.builder.ins().ireduce(types::I64, bounded);
        let moved = self.builder.ins().ishl(a, amount);
        let back = self.builder.ins().sshr(moved, amount);
        let lost = self.builder.ins().icmp(IntCC::NotEqual, back, a);
        self.trap_if(lost, trap_text::NUMERUS_OVERFLOW)?;
        self.range_guard(moved)?;
        Ok(moved)
    }

    /// `x ⇒ n` is `⌊x / 2ⁿ⌋`; a count of 127 or more leaves only the sign.
    fn wide_shift_right(&mut self, a: Value, count: Value) -> Lowered<Value> {
        let zero = self.wide_const(0);
        let negative = self.builder.ins().icmp(IntCC::SignedLessThan, count, zero);
        self.trap_if(negative, "negative shift count")?;
        let limit = self.wide_const(127);
        let large = self
            .builder
            .ins()
            .icmp(IntCC::SignedGreaterThan, count, limit);
        let bounded = self.builder.ins().select(large, limit, count);
        let amount = self.builder.ins().ireduce(types::I64, bounded);
        Ok(self.builder.ins().sshr(a, amount))
    }

    /// An integer comparison; the operands are widened when either is wide.
    pub(super) fn wide_compare(&mut self, cc: IntCC, a: Value, b: Value) -> Value {
        let (a, b) = if self.is_wide(a) || self.is_wide(b) {
            (self.widen(a), self.widen(b))
        } else {
            (a, b)
        };
        self.builder.ins().icmp(cc, a, b)
    }

    pub(super) fn wide_negate(&mut self, value: Value) -> Lowered<Value> {
        let value = self.widen(value);
        let negated = self.builder.ins().ineg(value);
        self.range_guard(negated)?;
        Ok(negated)
    }

    /// `¬x` is `-x - 1`.
    pub(super) fn wide_bit_not(&mut self, value: Value) -> Lowered<Value> {
        let value = self.widen(value);
        let flipped = self.builder.ins().bnot(value);
        self.range_guard(flipped)?;
        Ok(flipped)
    }

    /// Print a wide integer line.
    pub(super) fn print_wide(&mut self, stream: Value, value: Value) -> Lowered<()> {
        let (low, high) = self.split_wide(value);
        let reference = self.runtime_ref(&abi::PRINT_WIDE)?;
        self.builder.ins().call(
            reference,
            &[
                self.arena.expect("arena entry parameter"),
                stream,
                low,
                high,
            ],
        );
        Ok(())
    }

    /// Render a wide integer as text.
    pub(super) fn text_from_wide(&mut self, value: Value) -> Lowered<Option<Value>> {
        let (low, high) = self.split_wide(value);
        self.call_runtime(&abi::TEXT_FROM_WIDE, &[low, high])
    }
}
