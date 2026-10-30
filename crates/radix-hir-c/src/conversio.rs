//! Explicit conversions (`x ↦ T [via hint] [⊥ recovery]`) between the scalar
//! classes.
//!
//! The ones that cannot fail are one C expression. The ones that can (text
//! that is not a number, a value outside the target's range, a float that is
//! not a finite integer after rounding) go through a `faber_try_*` call that
//! says why it failed: with `⊥` the recovery value takes its place; without,
//! the program ends with the runner's text for that failure. `list<T> ↦
//! list<T, size N>` copies into a fixed-capacity list and fails when the source is
//! longer, the runner's rule.

use crate::Emitter;
use crate::error::{UNSUPPORTED_ISSUE, unsupported};
use crate::expr::{Operand, Value};
use crate::helpers::{Helper, ListOp};
use crate::ty::{Elem, IntWidth, Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::hints::{self, EndianHint};
use radix_diagnostics::DiagnosticArg;
use radix_hir::HirExpression;
use radix_lexer::Symbol;
use radix_types::{BitsWidth, NumericWidth, Primitive, Type, TypeId};

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

fn int_width_for_class(class: Ty) -> Option<u8> {
    match class {
        Ty::Sized(IntWidth::I8 | IntWidth::U8) => Some(8),
        Ty::Sized(IntWidth::I16 | IntWidth::U16) => Some(16),
        Ty::Sized(IntWidth::I32 | IntWidth::U32) => Some(32),
        Ty::Int | Ty::Sized(IntWidth::U64) => Some(64),
        _ => None,
    }
}

fn bits_width(width: radix_types::BitsWidth) -> u8 {
    match width {
        radix_types::BitsWidth::B8 => 8,
        radix_types::BitsWidth::B16 => 16,
        radix_types::BitsWidth::B32 => 32,
        radix_types::BitsWidth::B64 => 64,
    }
}

/// Return the target refusal name for a cross-width scalar bits conversion.
pub(crate) fn bits_conversion_width_mismatch(
    source: Option<Ty>,
    target: Ty,
) -> Option<&'static str> {
    let source = source?;
    match (source, target) {
        (Ty::Bits(from), Ty::Bits(to)) if from != to => {
            Some("raw bits conversion at a different width")
        }
        (Ty::Bits(from), Ty::Int | Ty::Sized(_))
            if int_width_for_class(target) != Some(bits_width(from)) =>
        {
            Some("raw bits to integer conversion at a different width")
        }
        (Ty::Int | Ty::Sized(_), Ty::Bits(to))
            if int_width_for_class(source) != Some(bits_width(to)) =>
        {
            Some("integer to raw bits conversion at a different width")
        }
        (Ty::Bits(from), Ty::Float32) if bits_width(from) != 32 => {
            Some("raw bits to float conversion at a different width")
        }
        (Ty::Bits(from), Ty::Float) if bits_width(from) != 64 => {
            Some("raw bits to float conversion at a different width")
        }
        (Ty::Float32, Ty::Bits(to)) if bits_width(to) != 32 => {
            Some("float to raw bits conversion at a different width")
        }
        (Ty::Float, Ty::Bits(to)) if bits_width(to) != 64 => {
            Some("float to raw bits conversion at a different width")
        }
        _ => None,
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

fn list_width_refusal(from: &str, to: &str) -> CodegenError {
    let construct = format!("width-changing list conversion from {from} to {to}");
    CodegenError {
        message: format!("unsupported on target c: {construct}"),
        args: vec![
            DiagnosticArg::new("issue", UNSUPPORTED_ISSUE),
            DiagnosticArg::new("construct", construct),
            DiagnosticArg::new("target", "c"),
        ],
    }
}

/// The element type of an unbounded list, peeling an alias or access marker.
fn list_element_type(types: &radix_types::TypeTable, mut ty: TypeId) -> Option<TypeId> {
    loop {
        ty = types.peel_access_marker(ty).1;
        match types.get(ty) {
            Type::Alias(_, inner) => ty = *inner,
            Type::Array(element) => return Some(*element),
            _ => return None,
        }
    }
}

/// The C element family and its byte width for the supported list byte view.
fn bytes_list_element(types: &radix_types::TypeTable, mut ty: TypeId) -> Option<(Elem, usize)> {
    loop {
        ty = types.peel_access_marker(ty).1;
        match types.get(ty) {
            Type::Alias(_, inner) => ty = *inner,
            Type::Bits(width) => {
                let elem = Elem::Bits(*width);
                return Some((elem, usize::from(bits_width(*width) / 8)));
            }
            Type::SizedNumeric(Primitive::Numerus, width) => {
                let (width, bytes) = match width {
                    NumericWidth::U16 => (IntWidth::U16, 2),
                    NumericWidth::U32 => (IntWidth::U32, 4),
                    NumericWidth::U64 => (IntWidth::U64, 8),
                    _ => return None,
                };
                return Some((Elem::Modular(width), bytes));
            }
            _ => return None,
        }
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
        if let Some((element, capacity)) = self.bounded_list_target(target) {
            let value = self.expr(source)?;
            let Value::List(elem) = value.value else {
                return Err(unsupported("conversion to a bounded list from a non-list"));
            };
            if classify(self.types, element)? != elem.ty() {
                return Err(unsupported(
                    "conversion to a bounded list of another element class",
                ));
            }
            let operand = self.bounded_list(&value, elem, capacity, recovery)?;
            return Ok(operand.with_effect(value.effect));
        }
        // `ascii<N>` targets: a `via Hex|Bin|Oct` pack from an integer, or the
        // D7 resize from a text (the same counted text, capacity-checked). A
        // capacity that is still an inference hole (the `ascii<_>` form of a
        // non-constant source) has no literal to work from, so it is
        // rejected by name rather than emitted as a plain integer.
        if self.is_bounded_ascii(target) {
            let Some(capacity) = self.ascii_capacity(target) else {
                return Err(unsupported(
                    "bounded ascii with a capacity that is not a literal",
                ));
            };
            let value = self.expr(source)?;
            if matches!(value.value, Value::Int | Value::Exact) {
                return self.ascii_pack(&value, capacity, params, recovery);
            }
            if value.value == Value::Text {
                return self.ascii_resize(&value, capacity, recovery);
            }
            return Err(unsupported("conversion to bounded ascii from this value"));
        }
        // The text pairs of a list of raw bits: `lista<b8> ↦ string` is the
        // UTF-8 decode and `string ↦ lista<b8>` the encode, the pair the
        // `octeti` (en `bytes`) leaf needs. The 16- and 32-bit pairs stay
        // refused by name: no C text carrier here is UTF-16 or UTF-32, so a
        // list show or a cast would be a wrong program.
        if let Some(source_ty) = source.ty {
            if radix_codegen_shared::is_byte_text_conversion(self.types, source_ty, target) {
                let value = self.expr(source)?;
                return if matches!(value.value, Value::List(_)) {
                    self.bytes_text_decode(&value, recovery)
                } else {
                    Ok(self.text_bytes_encode(&value))
                };
            }
            if radix_codegen_shared::list_text_conversion(self.types, source_ty, target).is_some() {
                return Err(unsupported("list of raw bits and text"));
            }
            if let Some((from, to)) =
                radix_codegen_shared::list_width_change(self.types, source_ty, target)
            {
                let Some(be) = self.endian_hint(params) else {
                    return Err(list_width_refusal(&from, &to));
                };
                return self.bits_list_width_change(
                    source,
                    target,
                    &from,
                    &to,
                    be,
                    recovery.is_some(),
                );
            }
        }
        let class = classify(self.types, target)?;
        let value = self.expr(source)?;
        let source_class = source.ty.map(|ty| classify(self.types, ty)).transpose()?;
        if let Some(construct) = bits_conversion_width_mismatch(source_class, class) {
            return Err(unsupported(construct));
        }
        let effect = value.effect;
        // Conversions that cannot fail ignore a recovery (typecheck allows
        // one only where the conversion can fail).
        let operand = self.bits_scalar_conversion(value, class, source_class, params, recovery)?;
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

    /// Emit same-width raw-bits views.
    fn bits_scalar_conversion(
        &mut self,
        value: Operand,
        class: Ty,
        source_class: Option<Ty>,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        let operand = match (value.value, class) {
            // Raw bits are a same-width view of an integer's low-width
            // pattern. C's conversion to an unsigned cell is defined modulo
            // 2^N, including negative signed sources.
            (Value::Int | Value::Exact, Ty::Bits(width))
                if source_class.and_then(int_width_for_class) == Some(bits_width(width)) =>
            {
                let c_type = crate::ty::int_width_of_bits(width).c_type();
                Operand::new(format!("(({c_type})({}))", value.text), Value::from(class))
            }
            // A bN-to-bN conversion preserves the already unsigned carrier.
            (Value::Int | Value::Exact, Ty::Bits(width))
                if source_class == Some(Ty::Bits(width)) =>
            {
                value
            }
            // Reading raw bits into an integer must copy the representation:
            // an out-of-range unsigned-to-signed C cast is implementation-defined.
            (Value::Int | Value::Exact, Ty::Int)
                if source_class == Some(Ty::Bits(radix_types::BitsWidth::B64)) =>
            {
                Operand::new(format!("faber_bits_read_i64({})", value.text), Value::Int)
            }
            (Value::Int | Value::Exact, Ty::Sized(width)) if matches!(source_class, Some(Ty::Bits(bits)) if int_width_for_class(Ty::Sized(width)) == Some(bits_width(bits))) => {
                Operand::new(
                    format!("faber_bits_read_{}({})", width.marker(), value.text),
                    Value::from(class),
                )
            }
            (Value::Float32, Ty::Bits(radix_types::BitsWidth::B32))
                if source_class == Some(Ty::Float32) =>
            {
                Operand::new(
                    format!("faber_bits_of_f32({})", value.text),
                    Value::from(class),
                )
            }
            (Value::Float, Ty::Bits(radix_types::BitsWidth::B64))
                if source_class == Some(Ty::Float) =>
            {
                Operand::new(
                    format!("faber_bits_of_f64({})", value.text),
                    Value::from(class),
                )
            }
            (Value::Int | Value::Exact, Ty::Float32)
                if source_class == Some(Ty::Bits(radix_types::BitsWidth::B32)) =>
            {
                Operand::new(format!("faber_f32_of_bits({})", value.text), Value::Float32)
            }
            (Value::Int | Value::Exact, Ty::Float)
                if source_class == Some(Ty::Bits(radix_types::BitsWidth::B64)) =>
            {
                Operand::new(format!("faber_f64_of_bits({})", value.text), Value::Float)
            }
            _ => return self.plain_scalar_conversion(value, class, params, recovery),
        };
        Ok(operand)
    }

    /// Emit plain scalar casts and display.
    fn plain_scalar_conversion(
        &mut self,
        value: Operand,
        class: Ty,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        let operand = match (value.value, class) {
            // A conversion into a float class is the C cast: `int ↦ float`
            // widens, `float ↦ f32` narrows, and `f32 ↦ float` widens the
            // already-binary32 value.
            (Value::Int | Value::Exact | Value::Float32, Ty::Float) => {
                Operand::new(format!("((double){})", value.text), Value::Float)
            }
            (Value::Int | Value::Exact | Value::Float, Ty::Float32) => {
                Operand::new(format!("((float){})", value.text), Value::Float32)
            }
            (Value::Int | Value::Exact, Ty::Text) => Operand::new(self.show(&value)?, Value::Text),
            (Value::Float, Ty::Text) => {
                self.support.need(Helper::FloatText);
                Operand::new(
                    format!("faber_float_display(faber_a, {})", value.text),
                    Value::Text,
                )
            }
            // `f32 ↦ textus` keeps the integral `.0` (the runner's
            // `display_float_at_width`), unlike the binary64 form.
            (Value::Float32, Ty::Text) => {
                self.support.need(Helper::FloatText);
                Operand::new(
                    format!("faber_float_text_f32(faber_a, {})", value.text),
                    Value::Text,
                )
            }
            (Value::Truth, Ty::Text) => {
                let tokens = self.names.display_tokens();
                let (yes, no) = (
                    self.text_literal(&tokens.true_)?,
                    self.text_literal(&tokens.false_)?,
                );
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
            _ => return self.decimal_word_conversion(value, class, params, recovery),
        };
        Ok(operand)
    }

    /// Emit decimal and word conversions.
    fn decimal_word_conversion(
        &mut self,
        value: Operand,
        class: Ty,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        let operand = match (value.value, class) {
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
                Operand::new(
                    format!("faber_dec_parse({}, 8)", value.text),
                    Value::Decimal,
                )
            }
            // `↦ wrapping<W>` reduces modulo 2^W; `↦ saturating<W>` clamps.
            // Both are total, so a recovery is never taken.
            (Value::Int | Value::Exact, Ty::Modular(width)) => {
                self.store_word(&value, Ty::Modular(width), "wrapping", width)
            }
            (Value::Int | Value::Exact, Ty::Saturating(width)) => {
                self.store_word(&value, Ty::Saturating(width), "saturating", width)
            }
            _ => return self.collection_conversion(value, class, params, recovery),
        };
        Ok(operand)
    }

    /// Emit identity and collection conversions.
    fn collection_conversion(
        &mut self,
        value: Operand,
        class: Ty,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        let operand = match (value.value, class) {
            (Value::Text, Ty::Text)
            | (Value::Float, Ty::Float)
            | (Value::Float32, Ty::Float32)
            | (Value::Truth, Ty::Bool)
            | (Value::Decimal, Ty::Decimal)
            | (Value::Int, Ty::Int) => value,
            // `copia ↦ lista` materializes a new list; `lista ↦ copia`
            // deduplicates into a new set. Both read their source (`↦`).
            (Value::Set(elem), Ty::List(slot)) if slot == elem => {
                self.support.need(Helper::List(elem, ListOp::New));
                Operand::new(
                    format!(
                        "faber_list_{}_copy(faber_a, {})",
                        self.elem_suffix(elem)?,
                        value.text
                    ),
                    Value::List(elem),
                )
            }
            (Value::List(elem), Ty::Set(slot)) if slot == elem => {
                self.support.need(Helper::List(elem, ListOp::New));
                Operand::new(
                    format!(
                        "faber_set_{}_of(faber_a, {})",
                        self.elem_suffix(elem)?,
                        value.text
                    ),
                    Value::Set(elem),
                )
            }
            (Value::Set(elem), Ty::Set(slot)) if slot == elem => value,
            // `tabula ↦ lista<T>`: the target element class picks the keys
            // (`T` is the key class) or the values (`T` is the value class).
            (Value::Map(k, v), Ty::List(slot)) if slot == k => {
                self.support.need(Helper::List(k, ListOp::New));
                Operand::new(
                    format!(
                        "faber_map_{}_keys(faber_a, {})",
                        self.map_suffix(k, v)?,
                        value.text
                    ),
                    Value::List(k),
                )
            }
            (Value::Map(k, v), Ty::List(slot)) if slot == v => {
                self.support.need(Helper::List(v, ListOp::New));
                Operand::new(
                    format!(
                        "faber_map_{}_values(faber_a, {})",
                        self.map_suffix(k, v)?,
                        value.text
                    ),
                    Value::List(v),
                )
            }
            _ => return self.fallible_scalar_conversion(&value, class, params, recovery),
        };
        Ok(operand)
    }

    /// Emit fallible numeric and truth conversions.
    fn fallible_scalar_conversion(
        &mut self,
        value: &Operand,
        class: Ty,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        let integer = matches!(value.value, Value::Int | Value::Exact);
        let operand = match (value.value, class) {
            (_, Ty::Int | Ty::Sized(_)) if integer => {
                self.fallible_int(value, class, Kind::IntInt, None, params, recovery)?
            }
            (Value::Text, Ty::Int | Ty::Sized(_)) => {
                self.fallible_int(value, class, Kind::TextInt, None, params, recovery)?
            }
            (Value::Float | Value::Float32, Ty::Int | Ty::Sized(_)) => {
                let mode = self.rounding_mode(params)?;
                self.fallible_int(value, class, Kind::FloatInt, Some(mode), params, recovery)?
            }
            (Value::Text, Ty::Float | Ty::Float32) => {
                let float_value = if class == Ty::Float32 {
                    Value::Float32
                } else {
                    Value::Float
                };
                self.text_to_float(value, recovery, float_value)?
            }
            // `text ↦ bool` parses the code locale's own `true`/`false`
            // words and fails on any other text (the runner's ruling);
            // `fractus ↦ bool` is non-zero (`-0.0` is false, NaN is true).
            // `bool ↦ fractus` is 1.0 / 0.0.
            (Value::Text, Ty::Bool) => self.text_to_bool(value, recovery)?,
            (Value::Float, Ty::Bool) => {
                Operand::new(format!("(({}) != 0.0)", value.text), Value::Truth)
            }
            (Value::Float32, Ty::Bool) => {
                Operand::new(format!("(({}) != 0.0f)", value.text), Value::Truth)
            }
            (Value::Truth, Ty::Float) => {
                Operand::new(format!("(({}) ? 1.0 : 0.0)", value.text), Value::Float)
            }
            (Value::Truth, Ty::Float32) => {
                Operand::new(format!("(({}) ? 1.0f : 0.0f)", value.text), Value::Float32)
            }
            _ => return Err(unsupported("conversion between these value classes")),
        };
        Ok(operand)
    }

    /// Whether the target (peeling aliases) is a bounded `ascii<N>`.
    pub(crate) fn is_bounded_ascii(&self, ty: TypeId) -> bool {
        match self.types.get(ty) {
            Type::Alias(_, inner) => self.is_bounded_ascii(*inner),
            Type::BoundedAscii { .. } => true,
            _ => false,
        }
    }

    /// The capacity of an `ascii<N>` target, peeling aliases; `None` for any
    /// other type (or a non-literal capacity).
    pub(crate) fn ascii_capacity(&self, ty: TypeId) -> Option<u64> {
        match self.types.get(ty) {
            Type::Alias(_, inner) => self.ascii_capacity(*inner),
            Type::BoundedAscii { capacity } => match self.types.get_index(*capacity) {
                radix_types::IndexExpr::Literal(value) => Some(*value),
                _ => None,
            },
            _ => None,
        }
    }

    /// `numerus ↦ ascii<N> via Hex|Bin|Oct`: zero-padded fixed-width digits.
    fn ascii_pack(
        &mut self,
        value: &Operand,
        capacity: u64,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Conv);
        self.support.need(Helper::AsciiPack);
        let mut radix = 10;
        for param in params {
            match self.names.resolve_symbol(*param) {
                "Hex" => radix = 16,
                "Bin" => radix = 2,
                "Oct" => radix = 8,
                _ => {}
            }
        }
        if radix == 10 {
            return Err(unsupported("ascii pack without a Hex, Bin, or Oct hint"));
        }
        let n = self.temps;
        self.temps += 1;
        let (v, s) = (format!("faber_v{n}"), format!("faber_s{n}"));
        let call = format!(
            "faber_try_ascii_pack(faber_a, {}, {radix}, {capacity}, &{s})",
            value.text
        );
        let tail = match recovery {
            Some(recovery) => {
                let fallback = self.expr(recovery)?;
                if fallback.value != Value::Text {
                    return Err(unsupported("recovery value of a different class"));
                }
                format!("{s} == 0 ? {v} : {};", fallback.text)
            }
            None => {
                format!("if ({s} != 0) {{ faber_conv_fail(FABER_CONV_ASCII_PACK, {s}); }} {v};")
            }
        };
        let text = format!("(__extension__ ({{ int {s} = 0; faber_text {v} = {call}; {tail} }}))");
        Ok(Operand::new(text, Value::Text).with_effect(value.effect))
    }

    /// `ascii<N> ↦ ascii<M>` (D7 resize): the counted text when it fits.
    fn ascii_resize(
        &mut self,
        value: &Operand,
        capacity: u64,
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::TextRead);
        let n = self.temps;
        self.temps += 1;
        let (v, s) = (format!("faber_v{n}"), format!("faber_s{n}"));
        let tail = match recovery {
            Some(recovery) => {
                let fallback = self.expr(recovery)?;
                if fallback.value != Value::Text {
                    return Err(unsupported("recovery value of a different class"));
                }
                format!("{s} ? {v} : {};", fallback.text)
            }
            None => format!(
                "if (!{s}) {{ faber_trap(\"ascii to bounded ascii conversion failed\"); }} {v};"
            ),
        };
        let text = format!(
            "(__extension__ ({{ faber_text {v} = {}; bool {s} = faber_text_length({v}) <= {capacity}; {tail} }}))",
            value.text
        );
        Ok(Operand::new(text, Value::Text).with_effect(value.effect))
    }

    /// `lista<b8> ↦ string`: the UTF-8 decode. The bytes are valid UTF-8 and
    /// carry no `0x00` byte, which the NUL-terminated carrier has no text
    /// value for, so `faber_try_bytes_text` says why it failed and the `⊥`
    /// recovery takes its place.
    fn bytes_text_decode(
        &mut self,
        value: &Operand,
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Conv);
        // The decode allocates the text it seals, like a list build.
        self.support.need(Helper::List(
            crate::ty::Elem::Bits(radix_types::BitsWidth::B8),
            ListOp::New,
        ));
        let n = self.temps;
        self.temps += 1;
        let (v, s) = (format!("faber_v{n}"), format!("faber_s{n}"));
        let mut effect = value.effect;
        let tail = match recovery {
            Some(recovery) => {
                let fallback = self.expr(recovery)?;
                if fallback.value != Value::Text {
                    return Err(unsupported("recovery value of a different class"));
                }
                effect |= fallback.effect;
                format!("{s} == 0 ? {v} : {};", fallback.text)
            }
            None => {
                format!("if ({s} != 0) {{ faber_conv_fail(FABER_CONV_BYTES_TEXT, {s}); }} {v};")
            }
        };
        let text = format!(
            "(__extension__ ({{ int {s} = 0; faber_text {v} = faber_try_bytes_text(faber_a, {}, &{s}); {tail} }}))",
            value.text
        );
        Ok(Operand::new(text, Value::Text).with_effect(effect))
    }

    /// `string ↦ lista<b8>`: the UTF-8 encode. The header's byte count is
    /// copied into a fresh byte list, and it cannot fail: a text never carries
    /// a NUL byte.
    fn text_bytes_encode(&mut self, value: &Operand) -> Operand {
        self.support.need(Helper::List(
            crate::ty::Elem::Bits(radix_types::BitsWidth::B8),
            ListOp::New,
        ));
        Operand::new(
            format!("faber_text_bytes(faber_a, {})", value.text),
            Value::List(crate::ty::Elem::Bits(radix_types::BitsWidth::B8)),
        )
        .with_effect(value.effect)
    }

    /// A bounded-list target (`list<T, size N>`), peeling aliases: its element type
    /// and its literal capacity. `None` for any other target.
    fn bounded_list_target(&self, ty: TypeId) -> Option<(TypeId, u64)> {
        match self.types.get(ty) {
            Type::Alias(_, inner) => self.bounded_list_target(*inner),
            Type::BoundedArray { element, capacity } => match self.types.get_index(*capacity) {
                radix_types::IndexExpr::Literal(value) => Some((*element, *value)),
                _ => None,
            },
            _ => None,
        }
    }

    /// `list<T> ↦ list<T, size N>`: a fixed copy of capacity N, or a failure the
    /// `⊥` recovery takes (`lista to bounded lista conversion failed`).
    fn bounded_list(
        &mut self,
        value: &Operand,
        elem: crate::ty::Elem,
        capacity: u64,
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::List(elem, ListOp::New));
        let n = self.temps;
        self.temps += 1;
        let (v, s) = (format!("faber_v{n}"), format!("faber_s{n}"));
        let c_type = self.elem_list_c_pointer(elem)?;
        let call = format!(
            "faber_list_{}_bounded(faber_a, {}, {capacity}, &{v})",
            self.elem_suffix(elem)?,
            value.text
        );
        let tail = match recovery {
            Some(recovery) => {
                let fallback = self.expr(recovery)?;
                if fallback.value != Value::List(elem) {
                    return Err(unsupported("recovery value of a different class"));
                }
                format!("{s} ? {v} : {};", fallback.text)
            }
            None => format!(
                "if (!{s}) {{ faber_trap(\"lista to bounded lista conversion failed\"); }} {v};"
            ),
        };
        let text =
            format!("(__extension__ ({{ {c_type} {v} = NULL; bool {s} = {call}; {tail} }}))");
        let mut operand = Operand::new(text, Value::List(elem));
        operand.stored = Some(Ty::List(elem));
        Ok(operand)
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

    /// The endian hint used by a width-changing list conversion.
    fn endian_hint(&self, params: &[Symbol]) -> Option<bool> {
        hints::endian_hint(params, |param| {
            hints::classify(self.names.resolve_symbol(*param))
        })
        .map(|hint| hint == EndianHint::Be)
    }

    /// Lower a width-changing list pair through the runtime byte view. The
    /// shared predicate admits the pair; this C lane carries b8 and the u16,
    /// u32, u64, b16, b32 and b64 families only.
    fn bits_list_width_change(
        &mut self,
        source: &HirExpression,
        target_ty: TypeId,
        from: &str,
        to: &str,
        be: bool,
        has_recovery: bool,
    ) -> Result<Operand, CodegenError> {
        if has_recovery {
            return Err(list_width_refusal(from, to));
        }
        let Some(source_ty) = source.ty else {
            return Err(list_width_refusal(from, to));
        };
        let Some(from_type) = list_element_type(self.types, source_ty) else {
            return Err(list_width_refusal(from, to));
        };
        let Some(to_type) = list_element_type(self.types, target_ty) else {
            return Err(list_width_refusal(from, to));
        };
        let Some((from_elem, from_bytes)) = bytes_list_element(self.types, from_type) else {
            return Err(list_width_refusal(from, to));
        };
        let Some((to_elem, to_bytes)) = bytes_list_element(self.types, to_type) else {
            return Err(list_width_refusal(from, to));
        };
        let decode = from_elem == Elem::Bits(BitsWidth::B8) && to_bytes > 1;
        let encode = to_elem == Elem::Bits(BitsWidth::B8) && from_bytes > 1;
        if !decode && !encode {
            return Err(list_width_refusal(from, to));
        }

        let mut value = self.expr(source)?;
        let Value::List(actual_elem) = value.value else {
            return Err(list_width_refusal(from, to));
        };
        if actual_elem != from_elem {
            return Err(list_width_refusal(from, to));
        }
        let prelude = self.bind(&mut value)?;
        let be = if be { "true" } else { "false" };
        let call = if decode {
            self.support
                .need(Helper::List(Elem::Bits(BitsWidth::B8), ListOp::New));
            self.support.need(Helper::List(to_elem, ListOp::New));
            let bytes = format!(
                "faber_bits_to_bytes(faber_a, {items}->items, {items}->len, sizeof *{items}->items)",
                items = value.text,
            );
            format!(
                "faber_bytes_to_{}(faber_a, {bytes}, {be})",
                self.elem_suffix(to_elem)?
            )
        } else {
            self.support.need(Helper::List(to_elem, ListOp::New));
            format!(
                "faber_bits_from_bytes(faber_a, {items}->items, {items}->len, sizeof *{items}->items, {be})",
                items = value.text,
            )
        };
        Ok(
            Operand::new(crate::expr::wrap(&prelude, &call), Value::List(to_elem))
                .with_effect(true),
        )
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

    /// `text ↦ bool`: the code locale's own `true`/`false` words, and any
    /// other text fails (`text to bivalens conversion failed`), the runner's
    /// ruling. The source is evaluated once.
    fn text_to_bool(
        &mut self,
        value: &Operand,
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Conv);
        let tokens = self.names.display_tokens();
        let (yes, no) = (
            self.text_literal(&tokens.true_)?,
            self.text_literal(&tokens.false_)?,
        );
        let n = self.temps;
        self.temps += 1;
        let (t, v) = (format!("faber_t{n}"), format!("faber_v{n}"));
        let tail = match recovery {
            Some(recovery) => {
                let fallback = self.expr(recovery)?;
                if fallback.value != Value::Truth {
                    return Err(unsupported("recovery value of a different class"));
                }
                format!("else {{ {v} = {}; }}", fallback.text)
            }
            None => "else { faber_conv_fail(FABER_CONV_TEXT_BOOL, FABER_CONV_SYNTAX); }".to_owned(),
        };
        let text = format!(
            "(__extension__ ({{ const char *{t} = {}; bool {v}; \
             if (strcmp({t}, {yes}) == 0) {{ {v} = true; }} \
             else if (strcmp({t}, {no}) == 0) {{ {v} = false; }} {tail} {v}; }}))",
            value.text
        );
        Ok(Operand::new(text, Value::Truth))
    }

    /// `text ↦ float`, with or without `⊥`.
    fn text_to_float(
        &mut self,
        value: &Operand,
        recovery: Option<&HirExpression>,
        float_value: Value,
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Conv);
        let n = self.temps;
        self.temps += 1;
        let (v, s) = (format!("faber_v{n}"), format!("faber_s{n}"));
        // The parse is binary64 (the runner parses `f64` then rounds at the
        // declared width); an `f32` target narrows the parsed value.
        let narrowed = if float_value == Value::Float32 {
            format!("(float){v}")
        } else {
            v.clone()
        };
        let tail = match recovery {
            Some(recovery) => {
                let fallback = self.expr(recovery)?;
                if fallback.value != float_value {
                    return Err(unsupported("recovery value of a different class"));
                }
                format!("{s} == 0 ? {narrowed} : {};", fallback.text)
            }
            None => format!(
                "if ({s} != 0) {{ faber_conv_fail({}, {s}); }} {narrowed};",
                Kind::TextFloat.constant()
            ),
        };
        let text = format!(
            "(__extension__ ({{ double {v} = 0.0; int {s} = faber_try_text_float({}, &{v}); {tail} }}))",
            value.text
        );
        Ok(Operand::new(text, float_value))
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
