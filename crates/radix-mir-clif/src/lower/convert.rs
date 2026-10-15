//! Explicit numeric conversions (`↦`) between integers, floats, booleans,
//! text and raw bits.
//!
//! The oracle's rules (`radix-mir-runner` `conversio/scalar.rs`): a bare
//! narrowing conversion checks the value and fails on the `⇥` channel instead
//! of wrapping or relabelling; `wrapping<W>` and `saturating<W>` targets reduce
//! or clamp and cannot fail; a float source takes its `via` rounding (toward
//! zero when none is named) and a NaN or out-of-range result fails. A failure
//! with a `⊥` default yields the default; otherwise a failable function
//! returns the failure text on its error channel and any other function ends
//! the run with `error: <text>`.

use super::*;

/// How an integer cell treats an out-of-range value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntPolicy {
    Trap,
    Wrap,
    Saturate,
}

/// An integer cell: its policy, width and the range its `i64` carrier holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntKind {
    policy: IntPolicy,
    width: NumericWidth,
    low: i64,
    high: i64,
}

/// The cell kinds an explicit numeric conversion moves between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Num {
    Int(IntKind),
    Float(NumericWidth),
    Bool,
    Text,
    Bits(BitsWidth),
}

fn int_kind(table: &TypeTable, id: TypeId) -> Option<IntKind> {
    let (policy, width) = match resolve(table, id) {
        Type::Primitive(Primitive::Numerus) => (IntPolicy::Trap, NumericWidth::I64),
        Type::SizedNumeric(Primitive::Numerus, width) => (IntPolicy::Trap, *width),
        Type::ModularWord(width) => (IntPolicy::Wrap, *width),
        Type::SaturatingWord(width) => (IntPolicy::Saturate, *width),
        _ => return None,
    };
    let (low, high) = width.integer_range()?;
    Some(IntKind {
        policy,
        width,
        low: i64::try_from(low).ok()?,
        // A u64 cell's upper bound is beyond the i64 carrier.
        high: i64::try_from(high).unwrap_or(i64::MAX),
    })
}

/// The cell kind of `id` when it takes part in a numeric conversion.
fn num_of(table: &TypeTable, id: TypeId) -> Option<Num> {
    if let Some(kind) = int_kind(table, id) {
        return Some(Num::Int(kind));
    }
    match resolve(table, id) {
        Type::Primitive(Primitive::Fractus) => Some(Num::Float(NumericWidth::F64)),
        Type::SizedNumeric(Primitive::Fractus, width) if width.is_float() => {
            Some(Num::Float(*width))
        }
        Type::Primitive(Primitive::Bivalens) => Some(Num::Bool),
        Type::Primitive(Primitive::Textus | Primitive::Ascii) => Some(Num::Text),
        Type::Bits(width) => Some(Num::Bits(*width)),
        _ => None,
    }
}

/// What lowering a source/target pair needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Plan {
    source: Num,
    target: Num,
}

impl Plan {
    #[cfg(test)]
    pub(crate) fn target(self) -> Num {
        self.target
    }
}

fn signed_width(width: NumericWidth) -> bool {
    matches!(
        width,
        NumericWidth::I8 | NumericWidth::I16 | NumericWidth::I32 | NumericWidth::I64
    )
}

/// Whether a trapping integer cell of `width` can hold a text or float
/// result: a u64 cell reaches beyond the `i64` carrier.
fn carrier_exact(kind: IntKind) -> bool {
    kind.width != NumericWidth::U64
}

/// The plan for an explicit conversion, or `None` when the pair is outside
/// the lowered set.
pub(crate) fn numeric_conversion(
    table: &TypeTable,
    source: TypeId,
    target: TypeId,
) -> Option<Plan> {
    let source = num_of(table, source)?;
    let target = num_of(table, target)?;
    let supported = match (source, target) {
        (Num::Int(_), Num::Int(to)) => match to.policy {
            IntPolicy::Trap | IntPolicy::Saturate => true,
            IntPolicy::Wrap => to.width != NumericWidth::U64,
        },
        (Num::Int(_) | Num::Bool, Num::Float(_)) | (Num::Float(_), Num::Float(_)) => true,
        (Num::Float(_) | Num::Text, Num::Int(to)) => {
            to.policy == IntPolicy::Trap && carrier_exact(to)
        }
        (Num::Text | Num::Float(_) | Num::Int(_), Num::Bool) => true,
        (Num::Bool, Num::Int(to)) => to.policy == IntPolicy::Trap,
        (Num::Text, Num::Float(_)) => true,
        // A signed integer or an unsigned one below 64 bits reads and writes
        // a same-width pattern; a u64 pattern would leave the carrier.
        (Num::Int(from), Num::Bits(width)) => {
            from.policy == IntPolicy::Trap
                && width.bits() == int_bits(from.width)
                && (signed_width(from.width) || width.bits() < 64)
        }
        (Num::Bits(width), Num::Int(to)) => {
            to.policy == IntPolicy::Trap
                && width.bits() == int_bits(to.width)
                && (signed_width(to.width) || width.bits() < 64)
        }
        (Num::Float(float), Num::Bits(width)) | (Num::Bits(width), Num::Float(float)) => {
            matches!(
                (float, width),
                (NumericWidth::F32, BitsWidth::B32) | (NumericWidth::F64, BitsWidth::B64)
            )
        }
        _ => false,
    };
    supported.then_some(Plan { source, target })
}

fn int_bits(width: NumericWidth) -> u32 {
    match width {
        NumericWidth::I8 | NumericWidth::U8 => 8,
        NumericWidth::I16 | NumericWidth::U16 => 16,
        NumericWidth::I32 | NumericWidth::U32 => 32,
        _ => 64,
    }
}

/// The failure text a conversion raises.
#[derive(Clone, Copy)]
enum Failure {
    Static(&'static str),
    /// A runtime status code (see `abi::CONVERT_MESSAGE`).
    Code(Value),
}

impl FunctionLower<'_, '_> {
    /// Lower an explicit numeric conversion into `variable`.
    pub(super) fn numeric_convert(
        &mut self,
        plan: Plan,
        conversion: &MirConversion,
        value: Value,
        variable: Variable,
    ) -> Lowered<()> {
        let recovery = match &conversion.recovery {
            Some(operand) => Some(self.scalar(operand)?),
            None => None,
        };
        let (converted, failed) = self.numeric_value(plan, conversion, value)?;
        let converted = match (failed, recovery) {
            (Some(failed), Some((default, rep))) => {
                let want = match plan.target {
                    Num::Int(_) => Rep::Int,
                    Num::Float(_) => Rep::Float,
                    Num::Bool => Rep::Bool,
                    Num::Bits(width) => Rep::Bits(width),
                    Num::Text => return Err(Decline::new("text conversion target")),
                };
                let default = self.coerce(default, rep, want)?;
                self.builder.ins().select(failed.0, default, converted)
            }
            (Some(failed), None) => {
                self.raise(failed.0, failed.1)?;
                converted
            }
            // `⊥` on a truthiness conversion supplies the default for a falsy source.
            (None, Some((default, rep)))
                if plan.target == Num::Bool && plan.source != Num::Bool =>
            {
                let default = self.coerce(default, rep, Rep::Bool)?;
                let truthy = self.builder.ins().iconst(types::I8, 1);
                self.builder.ins().select(converted, truthy, default)
            }
            (None, _) => converted,
        };
        self.builder.def_var(variable, converted);
        Ok(())
    }

    /// The converted value plus, when the conversion can fail, the failure
    /// flag and its text. A failing source still produces a defined value.
    fn numeric_value(
        &mut self,
        plan: Plan,
        conversion: &MirConversion,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok(match (plan.source, plan.target) {
            (Num::Int(from), Num::Int(to)) => self.int_to_int(from, to, value),
            (Num::Int(_), Num::Float(width)) => {
                let widened = if width == NumericWidth::F32 {
                    // Rounded once, nearest-even, straight from the integer.
                    let narrow = self.builder.ins().fcvt_from_sint(types::F32, value);
                    self.builder.ins().fpromote(types::F64, narrow)
                } else {
                    self.builder.ins().fcvt_from_sint(types::F64, value)
                };
                (widened, None)
            }
            (Num::Bool, Num::Float(_)) => {
                let wide = self.builder.ins().uextend(types::I64, value);
                (self.builder.ins().fcvt_from_sint(types::F64, wide), None)
            }
            (Num::Bool, Num::Int(_)) => (self.builder.ins().uextend(types::I64, value), None),
            (Num::Float(_), Num::Float(_)) => (value, None),
            (Num::Float(_), Num::Int(to)) => self.float_to_int(conversion, to, value)?,
            (Num::Text, Num::Int(to)) => self.text_to_int(conversion, to, value)?,
            (Num::Text, Num::Float(_)) => {
                let parsed = self
                    .call_runtime(&abi::PARSE_F64, &[value])?
                    .ok_or_else(|| Decline::new("parse result"))?;
                let status = self.parse_status()?;
                let zero = self.builder.ins().iconst(types::I32, 0);
                let failed = self.builder.ins().icmp(IntCC::NotEqual, status, zero);
                (parsed, Some((failed, Failure::Code(status))))
            }
            (Num::Int(_), Num::Bool) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                (self.builder.ins().icmp(IntCC::NotEqual, value, zero), None)
            }
            (Num::Float(_), Num::Bool) => {
                let zero = self.builder.ins().f64const(0.0);
                (
                    self.builder.ins().fcmp(FloatCC::NotEqual, value, zero),
                    None,
                )
            }
            (Num::Text, Num::Bool) => {
                let length = self
                    .call_runtime(&abi::TEXT_LEN, &[value])?
                    .ok_or_else(|| Decline::new("text length result"))?;
                let zero = self.builder.ins().iconst(types::I64, 0);
                (self.builder.ins().icmp(IntCC::NotEqual, length, zero), None)
            }
            (Num::Int(_), Num::Bits(width)) => (self.mask_pattern(value, width), None),
            (Num::Bits(width), Num::Int(to)) => (
                self.extend_pattern(value, width, signed_width(to.width)),
                None,
            ),
            (Num::Float(_), Num::Bits(width)) => {
                let pattern = if width == BitsWidth::B32 {
                    let narrow = self.builder.ins().fdemote(types::F32, value);
                    let bits = self
                        .builder
                        .ins()
                        .bitcast(types::I32, MemFlagsData::new(), narrow);
                    self.builder.ins().uextend(types::I64, bits)
                } else {
                    self.builder
                        .ins()
                        .bitcast(types::I64, MemFlagsData::new(), value)
                };
                (pattern, None)
            }
            (Num::Bits(width), Num::Float(_)) => {
                let float = if width == BitsWidth::B32 {
                    let bits = self.builder.ins().ireduce(types::I32, value);
                    let narrow = self
                        .builder
                        .ins()
                        .bitcast(types::F32, MemFlagsData::new(), bits);
                    self.builder.ins().fpromote(types::F64, narrow)
                } else {
                    self.builder
                        .ins()
                        .bitcast(types::F64, MemFlagsData::new(), value)
                };
                (float, None)
            }
            _ => return Err(Decline::new("explicit numeric conversion")),
        })
    }

    fn parse_status(&mut self) -> Lowered<Value> {
        self.call_runtime(&abi::PARSE_STATUS, &[])?
            .ok_or_else(|| Decline::new("parse status result"))
    }

    /// The low `width` bits of `value`, zero-extended.
    fn mask_pattern(&mut self, value: Value, width: BitsWidth) -> Value {
        if width.bits() == 64 {
            return value;
        }
        let mask = self
            .builder
            .ins()
            .iconst(types::I64, (1i64 << width.bits()) - 1);
        self.builder.ins().band(value, mask)
    }

    /// A `width`-bit pattern read as an integer: sign-extended when signed.
    fn extend_pattern(&mut self, pattern: Value, width: BitsWidth, signed: bool) -> Value {
        if width.bits() == 64 || !signed {
            return pattern;
        }
        let shift = i64::from(64 - width.bits());
        let raised = self.builder.ins().ishl_imm_u(pattern, shift);
        self.builder.ins().sshr_imm_u(raised, shift)
    }

    fn int_to_int(
        &mut self,
        from: IntKind,
        to: IntKind,
        value: Value,
    ) -> (Value, Option<(Value, Failure)>) {
        match to.policy {
            IntPolicy::Trap => {
                if from.low >= to.low && from.high <= to.high {
                    return (value, None);
                }
                let below = if to.low == i64::MIN {
                    None
                } else {
                    let bound = self.builder.ins().iconst(types::I64, to.low);
                    Some(self.builder.ins().icmp(IntCC::SignedLessThan, value, bound))
                };
                let above = if to.high == i64::MAX {
                    None
                } else {
                    let bound = self.builder.ins().iconst(types::I64, to.high);
                    Some(
                        self.builder
                            .ins()
                            .icmp(IntCC::SignedGreaterThan, value, bound),
                    )
                };
                let failed = match (below, above) {
                    (Some(below), Some(above)) => self.builder.ins().bor(below, above),
                    (Some(flag), None) | (None, Some(flag)) => flag,
                    (None, None) => return (value, None),
                };
                (
                    value,
                    Some((
                        failed,
                        Failure::Static("numerus to numerus conversion out of range"),
                    )),
                )
            }
            IntPolicy::Saturate => {
                if from.low >= to.low && from.high <= to.high {
                    return (value, None);
                }
                let low = self.builder.ins().iconst(types::I64, to.low);
                let high = self.builder.ins().iconst(types::I64, to.high);
                let raised = self.builder.ins().smax(value, low);
                (self.builder.ins().smin(raised, high), None)
            }
            IntPolicy::Wrap => {
                if from.low >= to.low && from.high <= to.high {
                    return (value, None);
                }
                let bits = int_bits(to.width);
                if bits == 64 {
                    return (value, None);
                }
                if signed_width(to.width) {
                    let shift = i64::from(64 - bits);
                    let raised = self.builder.ins().ishl_imm_u(value, shift);
                    (self.builder.ins().sshr_imm_u(raised, shift), None)
                } else {
                    let mask = self.builder.ins().iconst(types::I64, to.high);
                    (self.builder.ins().band(value, mask), None)
                }
            }
        }
    }

    /// `x ↦ int via <rounding>`: round, then check the result against the
    /// target's range; NaN and infinities fail.
    fn float_to_int(
        &mut self,
        conversion: &MirConversion,
        to: IntKind,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        let mode = self.rounding_mode(&conversion.params);
        let rounded = match mode {
            Rounding::Floor => self.builder.ins().floor(value),
            Rounding::Ceil => self.builder.ins().ceil(value),
            Rounding::HalfEven => self.builder.ins().nearest(value),
            Rounding::Trunc => self.builder.ins().trunc(value),
        };
        // The bounds are exact in f64: powers of two and small integers.
        let low = self.builder.ins().f64const(to.low as f64);
        let above = self.builder.ins().f64const((to.high as f64) + 1.0);
        let at_least = self
            .builder
            .ins()
            .fcmp(FloatCC::GreaterThanOrEqual, rounded, low);
        let below = self.builder.ins().fcmp(FloatCC::LessThan, rounded, above);
        let fits = self.builder.ins().band(at_least, below);
        let one = self.builder.ins().iconst(types::I8, 1);
        let failed = self.builder.ins().bxor(fits, one);
        let converted = self.builder.ins().fcvt_to_sint_sat(types::I64, rounded);
        Ok((
            converted,
            Some((
                failed,
                Failure::Static("fractus to numerus conversion out of range"),
            )),
        ))
    }

    fn rounding_mode(&self, params: &[radix_lexer::Symbol]) -> Rounding {
        params
            .iter()
            .find_map(|param| match self.state.input.interner.resolve(*param) {
                "floor" | "Floor" => Some(Rounding::Floor),
                "ceil" | "Ceil" => Some(Rounding::Ceil),
                "half_even" | "HalfEven" => Some(Rounding::HalfEven),
                "trunc" | "Trunc" => Some(Rounding::Trunc),
                _ => None,
            })
            .unwrap_or(Rounding::Trunc)
    }

    /// `textus ↦ int`: a parse by the `hex`/`bin`/`oct` radix hint (decimal
    /// otherwise), range-checked against the target.
    fn text_to_int(
        &mut self,
        conversion: &MirConversion,
        to: IntKind,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        let radix = conversion
            .params
            .iter()
            .find_map(|param| match self.state.input.interner.resolve(*param) {
                "hex" | "Hex" => Some(16),
                "bin" | "Bin" => Some(2),
                "oct" | "Oct" => Some(8),
                _ => None,
            })
            .unwrap_or(10);
        let radix = self.builder.ins().iconst(types::I32, radix);
        let low = self.builder.ins().iconst(types::I64, to.low);
        let high = self.builder.ins().iconst(types::I64, to.high);
        let parsed = self
            .call_runtime(&abi::PARSE_INT, &[value, radix, low, high])?
            .ok_or_else(|| Decline::new("parse result"))?;
        let status = self.parse_status()?;
        let zero = self.builder.ins().iconst(types::I32, 0);
        let failed = self.builder.ins().icmp(IntCC::NotEqual, status, zero);
        Ok((parsed, Some((failed, Failure::Code(status)))))
    }

    /// Raise a conversion failure when `failed` is set; continue otherwise.
    fn raise(&mut self, failed: Value, failure: Failure) -> Lowered<()> {
        let raise = self.builder.create_block();
        self.builder.set_cold_block(raise);
        let next = self.builder.create_block();
        self.builder.ins().brif(failed, raise, &[], next, &[]);
        self.builder.switch_to_block(raise);
        let message = match failure {
            Failure::Static(text) => self.text_literal(text.as_bytes())?,
            Failure::Code(code) => self
                .call_runtime(&abi::CONVERT_MESSAGE, &[code])?
                .ok_or_else(|| Decline::new("conversion message result"))?,
        };
        match self.error_rep {
            Some(Rep::Text) => {
                let mut results = vec![self.builder.ins().iconst(types::I8, 1)];
                let ret_rep = self.ret_rep;
                if let Some(zero) = self.zero(ret_rep) {
                    results.push(zero);
                }
                results.push(message);
                self.builder.ins().return_(&results);
            }
            None => {
                let length = self
                    .call_runtime(&abi::TEXT_LEN, &[message])?
                    .ok_or_else(|| Decline::new("text length result"))?;
                let bytes = self
                    .builder
                    .ins()
                    .iadd_imm_s(message, abi::TEXT_HEADER as i64);
                let reference = self.runtime_ref(&abi::TRAP)?;
                self.builder.ins().call(reference, &[bytes, length]);
                self.builder.ins().trap(TrapCode::unwrap_user(1));
            }
            Some(_) => return Err(Decline::new("conversion failure channel type")),
        }
        self.builder.switch_to_block(next);
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Rounding {
    Floor,
    Ceil,
    HalfEven,
    Trunc,
}

#[cfg(test)]
#[path = "convert_test.rs"]
mod convert_test;
