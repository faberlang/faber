//! Explicit conversions (`x ↦ T [via hint] [⊥ recovery]`) between the scalar
//! classes.
//!
//! The ones that cannot fail are one C expression. The ones that can (text
//! that is not a number, a value outside the target's range, a float that is
//! not a finite integer after rounding) go through a `faber_try_*` call that
//! says why it failed: with `⊥` the recovery value takes its place; without,
//! the program ends with the runner's text for that failure. The runner does
//! not convert a list into a bounded list at run time, so neither does this.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, c_string};
use crate::helpers::Helper;
use crate::ty::{IntWidth, Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::HirExpression;
use radix_lexer::Symbol;
use radix_types::{Type, TypeId};

/// Which failure texts a fallible conversion uses (`FABER_CONV_*`).
#[derive(Clone, Copy)]
enum Kind {
    TextInt,
    TextFloat,
    IntInt,
    FloatInt,
}

impl Kind {
    fn constant(self) -> &'static str {
        match self {
            Kind::TextInt => "FABER_CONV_TEXT_INT",
            Kind::TextFloat => "FABER_CONV_TEXT_FLOAT",
            Kind::IntInt => "FABER_CONV_INT_INT",
            Kind::FloatInt => "FABER_CONV_FLOAT_INT",
        }
    }
}

/// The range an integer class holds, and its value form once converted.
fn int_target(class: Ty) -> Option<(i128, i128, bool)> {
    match class {
        Ty::Int => Some((i128::from(i64::MIN), i128::from(i64::MAX), false)),
        Ty::Sized(width) => {
            let (low, high) = width.range();
            Some((low, high, width == IntWidth::U64))
        }
        _ => None,
    }
}

impl Emitter<'_> {
    pub(crate) fn conversion(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        if matches!(self.types.get(target), Type::BoundedArray { .. }) {
            return Err(unsupported("conversion to a bounded list"));
        }
        // `↦ ascii<N>` (resize or radix formatting) is not part of the subset;
        // the runner's resize path is itself unsupported.
        if matches!(self.types.get(target), Type::BoundedAscii { .. }) {
            return Err(unsupported("conversion to bounded ascii"));
        }
        let class = classify(self.types, target)?;
        let value = self.expr(source)?;
        let integer = matches!(value.value, Value::Int | Value::Exact);
        let effect = value.effect;
        // Conversions that cannot fail ignore a recovery (typecheck allows
        // one only where the conversion can fail).
        let operand = match (value.value, class) {
            (Value::Int | Value::Exact, Ty::Float) => {
                Operand::new(format!("((double){})", value.text), Value::Float)
            }
            (Value::Int | Value::Exact, Ty::Text) => Operand::new(self.show(&value)?, Value::Text),
            (Value::Float, Ty::Text) => {
                self.support.need(Helper::FloatText);
                Operand::new(
                    format!("faber_float_display(faber_a, {})", value.text),
                    Value::Text,
                )
            }
            (Value::Truth, Ty::Text) => {
                let tokens = self.names.display_tokens();
                let (yes, no) = (c_string(&tokens.true_)?, c_string(&tokens.false_)?);
                Operand::new(format!("({} ? {yes} : {no})", value.text), Value::Text)
            }
            (Value::Truth, Ty::Int) => {
                self.support.need_ints();
                Operand::new(
                    format!("({} ? INT64_C(1) : INT64_C(0))", value.text),
                    Value::Int,
                )
            }
            (Value::Int | Value::Exact, Ty::Bool) => {
                Operand::new(format!("(({}) != 0)", value.text), Value::Truth)
            }
            // `d64` rows (D11.7): the exact carrier converts through the
            // runner's `faber_dec` helpers.
            (Value::Decimal, Ty::Int | Ty::Sized(_)) => {
                self.decimal_to_int(&value, class, params)?
            }
            (Value::Decimal, Ty::Float) => {
                self.support.need(Helper::Decimal);
                Operand::new(format!("faber_dec_f64({})", value.text), Value::Float)
            }
            (Value::Decimal, Ty::Text) => {
                self.support.need(Helper::DecimalText);
                Operand::new(
                    format!("faber_dec_fmt(faber_a, {})", value.text),
                    Value::Text,
                )
            }
            (Value::Int | Value::Exact, Ty::Decimal) => {
                self.support.need(Helper::Decimal);
                Operand::new(
                    format!("faber_dec_int((faber_i128){})", value.text),
                    Value::Decimal,
                )
            }
            (Value::Float, Ty::Decimal) => {
                self.support.need(Helper::Decimal);
                Operand::new(
                    format!(
                        "faber_dec_parse(faber_f64_text({}, (char[FABER_F64_TEXT]){{0}}), 8)",
                        value.text
                    ),
                    Value::Decimal,
                )
            }
            (Value::Text, Ty::Decimal) => {
                self.support.need(Helper::Decimal);
                Operand::new(format!("faber_dec_parse({}, 8)", value.text), Value::Decimal)
            }
            // `↦ wrapping<W>` reduces modulo 2^W; `↦ saturating<W>` clamps.
            // Both are total, so a recovery is never taken.
            (Value::Int | Value::Exact, Ty::Modular(width)) => {
                self.store_word(&value, Ty::Modular(width), "wrapping", width)
            }
            (Value::Int | Value::Exact, Ty::Saturating(width)) => {
                self.store_word(&value, Ty::Saturating(width), "saturating", width)
            }
            (Value::Text, Ty::Text)
            | (Value::Float, Ty::Float)
            | (Value::Truth, Ty::Bool)
            | (Value::Decimal, Ty::Decimal) => value,
            (Value::Int, Ty::Int) => value,
            (_, Ty::Int | Ty::Sized(_)) if integer => {
                self.fallible_int(&value, class, Kind::IntInt, None, params, recovery)?
            }
            (Value::Text, Ty::Int | Ty::Sized(_)) => {
                self.fallible_int(&value, class, Kind::TextInt, None, params, recovery)?
            }
            (Value::Float, Ty::Int | Ty::Sized(_)) => {
                let mode = self.rounding_mode(params)?;
                self.fallible_int(&value, class, Kind::FloatInt, Some(mode), params, recovery)?
            }
            (Value::Text, Ty::Float) => self.text_to_float(&value, recovery)?,
            // `text ↦ bool` is non-empty; a text value cannot hold a NUL, so
            // the first byte decides. `fractus ↦ bool` is non-zero (`-0.0` is
            // false, NaN is true). `bool ↦ fractus` is 1.0 / 0.0.
            (Value::Text, Ty::Bool) => {
                Operand::new(format!("(({})[0] != '\\0')", value.text), Value::Truth)
            }
            (Value::Float, Ty::Bool) => {
                Operand::new(format!("(({}) != 0.0)", value.text), Value::Truth)
            }
            (Value::Truth, Ty::Float) => {
                Operand::new(format!("(({}) ? 1.0 : 0.0)", value.text), Value::Float)
            }
            _ => return Err(unsupported("conversion between these value classes")),
        };
        // `textus ↦ string<N>`: the capacity check is a runtime store.
        let operand = match self.text_capacity(target) {
            Some(capacity) if operand.value == Value::Text => {
                self.support.need(Helper::TextRead);
                Operand::new(
                    format!("faber_bounded_text({}, {capacity})", operand.text),
                    Value::Text,
                )
            }
            _ => operand,
        };
        Ok(operand.with_effect(effect))
    }

    /// The capacity of a bounded text type (`string<N>`, `ascii<N>`), peeling
    /// aliases; `None` for an unbounded text or a non-literal capacity.
    fn text_capacity(&self, ty: TypeId) -> Option<u64> {
        match self.types.get(ty) {
            Type::Alias(_, inner) => self.text_capacity(*inner),
            Type::BoundedTextus { capacity } | Type::BoundedAscii { capacity } => {
                match self.types.get_index(*capacity) {
                    radix_types::IndexExpr::Literal(value) => Some(*value),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The `via` hint of a float-to-integer conversion, as `faber_try_float_int`'s
    /// `mode` argument.
    fn rounding_mode(&self, params: &[Symbol]) -> Result<u8, CodegenError> {
        for param in params {
            match self.names.resolve_symbol(*param) {
                "Floor" | "floor" => return Ok(0),
                "Ceil" | "ceil" => return Ok(1),
                "Trunc" | "trunc" => return Ok(2),
                "HalfEven" | "half_even" => return Ok(3),
                _ => {}
            }
        }
        Err(unsupported(
            "float to integer conversion without a rounding hint",
        ))
    }

    /// A `d64` rounded to an integer cell: the rounding half of the `via`
    /// hint picks the helper, then the target width checks the result.
    fn decimal_to_int(
        &mut self,
        value: &Operand,
        class: Ty,
        params: &[Symbol],
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Decimal);
        let rounding = self.rounding_mode(params)?;
        let helper = match rounding {
            0 => "faber_dec_round_floor",
            1 => "faber_dec_round_ceil",
            2 => "faber_dec_round_trunc",
            _ => "faber_dec_round",
        };
        let rounded = Operand::new(format!("{helper}({})", value.text), Value::Exact)
            .with_effect(value.effect);
        match class {
            Ty::Int => {
                self.support.need(Helper::StoreI64);
                Ok(Operand::new(
                    format!("faber_store_i64({}, \"conversion\")", rounded.text),
                    Value::Int,
                )
                .with_effect(value.effect))
            }
            Ty::Sized(width) => Ok(self.store_sized(&rounded, width, "conversion")),
            _ => Err(unsupported("decimal conversion to a non-integer class")),
        }
    }

    /// The radix of a text-to-integer conversion: decimal, or the `via` hint.
    /// Byte-order and code hints belong to bit and list conversions.
    fn radix_hint(&self, params: &[Symbol]) -> Result<u32, CodegenError> {
        let mut radix = 10;
        for param in params {
            match self.names.resolve_symbol(*param) {
                "Hex" => radix = 16,
                "Bin" => radix = 2,
                "Oct" => radix = 8,
                "Be" | "Le" | "Code" => {
                    return Err(unsupported("byte-order or code conversion hint"));
                }
                _ => {}
            }
        }
        Ok(radix)
    }

    /// An integer-class result from `faber_try_*`, narrowed to `class`.
    fn fallible_int(
        &mut self,
        value: &Operand,
        class: Ty,
        kind: Kind,
        mode: Option<u8>,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        let Some((low, high, wide)) = int_target(class) else {
            return Err(unsupported("conversion to a non-integer class"));
        };
        self.support.need(Helper::Conv);
        self.support.need(Helper::Types);
        let n = self.temps;
        self.temps += 1;
        let (v, s) = (format!("faber_v{n}"), format!("faber_s{n}"));
        let radix = self.radix_hint(params)?;
        let low_text = bound_text(low);
        let high_text = bound_text(high);
        let attempt = match kind {
            Kind::IntInt => format!(
                "faber_try_int({}, {low_text}, {high_text}, &{v})",
                value.text
            ),
            Kind::TextInt => format!(
                "faber_try_text_int({}, {radix}, {low_text}, {high_text}, &{v})",
                value.text
            ),
            Kind::FloatInt => format!(
                "faber_try_float_int({}, {}, {low_text}, {high_text}, &{v})",
                value.text,
                mode.unwrap_or(0)
            ),
            Kind::TextFloat => return Err(unsupported("conversion to a non-integer class")),
        };
        let (cast, form) = if wide {
            ("faber_i128", Value::Exact)
        } else {
            ("int64_t", Value::Int)
        };
        let converted = format!("(({cast}){v})");
        let tail = match recovery {
            Some(recovery) => {
                let fallback = self.expr(recovery)?;
                let fallback = self.settle(fallback, class)?;
                let fallback_text = match (fallback.value, form) {
                    (given, wanted) if given == wanted => fallback.text,
                    // A `u64` result is an exact value; a recovery that fits
                    // an `int64_t` widens to meet it.
                    (Value::Int, Value::Exact) => format!("((faber_i128){})", fallback.text),
                    _ => return Err(unsupported("recovery value of a different class")),
                };
                format!("{s} == 0 ? {converted} : {fallback_text};")
            }
            None => format!(
                "if ({s} != 0) {{ faber_conv_fail({}, {s}); }} {converted};",
                kind.constant()
            ),
        };
        let text = format!("(__extension__ ({{ faber_i128 {v}; int {s} = {attempt}; {tail} }}))");
        let mut operand = Operand::new(text, form);
        operand.stored = Some(class);
        Ok(operand)
    }

    /// `text ↦ float`, with or without `⊥`.
    fn text_to_float(
        &mut self,
        value: &Operand,
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Conv);
        let n = self.temps;
        self.temps += 1;
        let (v, s) = (format!("faber_v{n}"), format!("faber_s{n}"));
        let tail = match recovery {
            Some(recovery) => {
                let fallback = self.expr(recovery)?;
                if fallback.value != Value::Float {
                    return Err(unsupported("recovery value of a different class"));
                }
                format!("{s} == 0 ? {v} : {};", fallback.text)
            }
            None => format!(
                "if ({s} != 0) {{ faber_conv_fail({}, {s}); }} {v};",
                Kind::TextFloat.constant()
            ),
        };
        let text = format!(
            "(__extension__ ({{ double {v} = 0.0; int {s} = faber_try_text_float({}, &{v}); {tail} }}))",
            value.text
        );
        Ok(Operand::new(text, Value::Float))
    }
}

/// An exact bound as a C expression (`faber_i128` arguments).
fn bound_text(value: i128) -> String {
    if value == i128::from(i64::MIN) {
        "INT64_MIN".to_owned()
    } else if value < 0 {
        format!("-(faber_i128)INT64_C({})", value.unsigned_abs())
    } else if value > i128::from(i64::MAX) {
        format!("(faber_i128)UINT64_C({value})")
    } else {
        format!("(faber_i128)INT64_C({value})")
    }
}
