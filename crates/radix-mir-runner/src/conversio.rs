//! Target-directed `↦` conversion for the MIR runner.
//!
//! WHY: MIR validation proves `MirConversion.target_ty`; guessing from the
//! source `Value` shape disagrees with typed contracts (e.g. `textus ↦ bivalens`).

use super::regex_program::RegexProgram;
use super::value::{
    MapKey, SermoValue, Value, decimal_scale_factor, float_to_numerus_width,
    float_truncation_fits_width, int_width_range, modular_word_float_result, modular_word_result,
    saturating_word_result, trapping_float_store_fails,
};
use super::{Runner, RunnerError};
use radix_lexer::{Span, Symbol};
use radix_mir::lista_tensor_conversio::{ListaToTensorShape, resolve_lista_to_tensor_shape};
use radix_mir::{MirConversion, MirNamedOperand, MirOperand, MirStructFieldDefaults};
use radix_runtime_contract::instans::{Instans, InstansPraecisio};
use radix_types::{
    BitsWidth, FloatPolicy, InstansPrecision, NumericWidth, Primitive, Type, TypeId, TypeTable,
    effective_width,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::rc::Rc;

mod aggregate;
mod bits_list;
mod inf;
mod littera;
#[cfg(test)]
mod littera_test;
mod octeti_tensor;
mod rounding;
#[cfg(test)]
mod rounding_test;
mod scalar;
mod scalar_octeti;

#[derive(Clone, Copy)]
pub(crate) enum MissingFieldPolicy {
    Valor,
    Json,
}

impl<H: super::Host + ?Sized> Runner<'_, H> {
    /// Normalize a typed decimal MIR value into the scaled `Value::Int`
    /// carrier. Decimal literals lower to exact carriers at the MIR literal
    /// seam; this boundary re-derives from the f64 field only
    /// for legacy float-constant values that still reach a decimal slot.
    pub(crate) fn normalize_decimal_value(
        &mut self,
        value: Value,
        ty: TypeId,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Some(width) = effective_width(self.types, ty).filter(|width| width.is_decimal()) else {
            return Ok(value);
        };
        // An exact intermediate stays exact until a slot stores it (D11.7).
        if matches!(value, Value::Dec(..)) {
            return Ok(value);
        }
        self.convert_to_numerus_from(value, ty, &[], width, None, span)
    }

    pub(crate) fn eval_runtime_convert(
        &mut self,
        conversion: &MirConversion,
        value: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        self.convert_to_target(
            value,
            conversion.source_ty.semantic_id(),
            conversion.target_ty.semantic_id(),
            &conversion.params,
            conversion.recovery.as_ref(),
            &conversion.struct_defaults,
            span,
        )
    }

    /// `octeti ↦ tensor<E, [.., size n]>`: the conversion carries the target's
    /// evaluated `shape` as a second operand, because a run-time extent is a
    /// value and cannot be read from the target type the way a literal is.
    pub(crate) fn eval_runtime_convert_shaped(
        &mut self,
        conversion: &MirConversion,
        value: Value,
        shape: Vec<i64>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let bytes = match value {
            Value::Octeti(bytes) => bytes,
            Value::Array(cell) if self.is_bytes_list_type(conversion.source_ty.semantic_id()) => {
                bytes_of_list(&cell.borrow(), span)?
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("conversion shape operand on a non-byte source: {other:?}"),
                ));
            }
        };
        let target_ty = self.resolve_conversion_target(conversion.target_ty.semantic_id());
        self.convert_octeti_to_tensor(
            bytes,
            target_ty,
            Some(shape),
            &conversion.params,
            conversion.recovery.as_ref(),
            span,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn convert_to_target(
        &mut self,
        value: Value,
        source_ty: TypeId,
        target_ty: TypeId,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let target_ty = self.resolve_conversion_target(target_ty);
        let target = self.types.get(target_ty).clone();
        if self.is_dynamic_source(source_ty) && !matches!(target, Type::Primitive(Primitive::Valor))
        {
            let missing_policy = if matches!(target, Type::Primitive(Primitive::Json)) {
                MissingFieldPolicy::Json
            } else {
                MissingFieldPolicy::Valor
            };
            return match self.extract_dynamic_value_for_type(
                value,
                target_ty,
                struct_defaults,
                span,
                missing_policy,
            ) {
                Ok(value) => Ok(value),
                Err(_) => self.recover_or_fail(recovery, span, &dynamic_conversion_error(&target)),
            };
        }
        // Same-width bN <-> iN and bN <-> fN (need, C2 and C3): the
        // pattern is kept and read as the other type.
        if let Some(reinterpreted) = self.reinterpret_bits(&value, source_ty, &target) {
            return Ok(reinterpreted);
        }
        // `lista<bN> ↔ lista<number | bits>` across a width change takes a
        // `via Le|Be` clause (need, B5).
        if let Some(converted) =
            self.convert_bits_list_width(&value, source_ty, &target, params, recovery, span)
        {
            return converted;
        }
        // `lista<bN> ↦ textus` is a UTF-8/16/32 decode and `textus ↦
        // lista<bN>` the encode (need, B4).
        if matches!(target, Type::Primitive(Primitive::Textus))
            && let Some(width) = self.bits_text_width(source_ty)
        {
            return self.convert_bits_list_to_textus(&value, width, recovery, span);
        }
        if let (Type::Array(elem), Value::Str(text)) = (&target, &value)
            && let Some(width) = self.bits_text_element_width(*elem)
        {
            return Ok(Self::convert_textus_to_bits_list(text, width));
        }
        // A `lista<b8>` value read as a number or as ascii (`bytes ↦ u16 via
        // Le`, `bytes ↦ inf via Be`) takes the byte rows: gather the patterns
        // into the byte carrier those rows decode.
        let value = match &value {
            Value::Array(cell)
                if matches!(
                    target,
                    Type::Primitive(Primitive::Numerus | Primitive::Fractus | Primitive::Ascii)
                        | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
                ) && self.is_bytes_list_type(source_ty) =>
            {
                cell.borrow()
                    .iter()
                    .map(|item| bits_pattern(item).and_then(|p| u8::try_from(p).ok()))
                    .collect::<Option<Vec<u8>>>()
                    .map_or_else(|| value.clone(), Value::Octeti)
            }
            _ => value,
        };
        // Need: `octeti` is `lista<b8>`. A byte source that is not
        // already a list (a conversation reply, an `inf` via `Le|Be`, a
        // tensor, a text) takes the byte conversion and lands as a list of
        // byte patterns.
        if let Type::Array(elem) = &target
            && !matches!(value, Value::Array(_))
            && matches!(
                self.types.get(self.resolve_conversion_target(*elem)),
                Type::Bits(BitsWidth::B8)
            )
        {
            let bytes = self.convert_to_octeti_from(value, source_ty, params, span)?;
            return Ok(octeti_into_bits_list(bytes));
        }
        match &target {
            Type::Primitive(Primitive::Textus) => {
                self.convert_to_textus(value, source_ty, recovery, span)
            }
            Type::Primitive(Primitive::Vacuum) => self.convert_to_vacuum(value, span),
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
                // WHY: `textus ↦ numerus<W>` must honor `W` exactly as Rust codegen's
                // `.parse::<W>()` does — a value outside `W`'s signed range is a
                // conversion failure (SA-002). Unsized numerus defaults to i64.
                // D10.6: a `littera` source parses its digit (or, with
                // `Code`, reads its code point) — never a text parse.
                if matches!(
                    self.types.get(source_ty),
                    Type::Primitive(Primitive::Littera)
                ) && let Some(ch) = littera::littera_value(&value)
                {
                    return self.convert_littera_to_numerus(ch, params, recovery, span);
                }
                let width = match &target {
                    Type::SizedNumeric(_, width) => *width,
                    _ => NumericWidth::I64,
                };
                self.convert_to_numerus_from(value, source_ty, params, width, recovery, span)
            }
            Type::ModularWord(width) => {
                self.convert_to_modular_word(value, source_ty, *width, params, recovery, span)
            }
            Type::SaturatingWord(width) => {
                self.convert_to_saturating_word(value, source_ty, *width, params, recovery, span)
            }
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                let width = match &target {
                    Type::SizedNumeric(_, width) => *width,
                    _ => NumericWidth::F64,
                };
                self.convert_to_fractus_from(value, source_ty, params, width, recovery, span)
            }
            Type::Primitive(Primitive::Bivalens) => self.convert_to_bivalens(value, recovery, span),
            Type::Primitive(Primitive::Octeti) => {
                self.convert_to_octeti_from(value, source_ty, params, span)
            }
            // `uN ↦ bN` and `bN ↦ bN` at one width keep the pattern (the
            // checker admits no other source); bits live as unsigned cells.
            Type::Bits(_) => match value {
                Value::UInt(_) => Ok(value),
                Value::Int(v) => Ok(Value::UInt(v.cast_unsigned())),
                other => Err(RunnerError::unsupported(
                    span,
                    &format!("runtime conversio to bits from {other:?}"),
                )),
            },
            Type::Primitive(Primitive::Ascii) => self.convert_to_ascii(value, recovery, span),
            Type::Primitive(Primitive::Littera) => {
                self.convert_to_littera(value, params, recovery, span)
            }
            Type::BoundedAscii { capacity } => {
                if self.numerus_radix(params).is_some() {
                    self.convert_to_ascii_format_pack(value, *capacity, params, recovery, span)
                } else {
                    Err(RunnerError::unsupported(
                        span,
                        &format!("runtime conversio to {target:?}"),
                    ))
                }
            }
            Type::BoundedTextus { capacity } => {
                self.convert_to_bounded_textus(value, *capacity, recovery, span)
            }
            Type::BoundedOcteti { capacity } => {
                if matches!(value, Value::Int(_) | Value::UInt(_) | Value::Float(_)) {
                    self.convert_scalar_to_bounded_octeti(value, source_ty, *capacity, params, span)
                } else {
                    self.convert_to_bounded_octeti(value, *capacity, recovery, span)
                }
            }
            Type::Primitive(Primitive::Regex) => self.convert_to_regex(value, recovery, span),
            Type::Primitive(Primitive::Json) => self.convert_to_json(value, recovery, span),
            Type::Primitive(Primitive::Valor) => self.convert_to_valor(value, source_ty, span),
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => {
                self.convert_to_instans(value, &target, recovery, span)
            }
            Type::Array(element) => {
                self.convert_to_array(value, source_ty, *element, recovery, struct_defaults, span)
            }
            Type::Map(key, value_ty) => self.convert_to_map(
                value,
                source_ty,
                *key,
                *value_ty,
                recovery,
                struct_defaults,
                span,
            ),
            Type::Set(_) => self.convert_to_set(value, span),
            Type::Cursor(_) | Type::Tensor(_, _) | Type::Sparsa(_, _) => match value {
                // `octeti ↦ tensor<E, [shape]> via Le|Be`: faithful bulk
                // reinterpretation; the `via` hint rides in `params`.
                Value::Octeti(bytes) if matches!(target, Type::Tensor(_, _)) => {
                    self.convert_octeti_to_tensor(bytes, target_ty, None, params, recovery, span)
                }
                // The same row over a `lista<b8>` value held as a list of
                // byte patterns (need: `octeti` is `lista<b8>`).
                Value::Array(cell)
                    if matches!(target, Type::Tensor(_, _))
                        && self.is_bytes_list_type(source_ty) =>
                {
                    let bytes = bytes_of_list(&cell.borrow(), span)?;
                    self.convert_octeti_to_tensor(bytes, target_ty, None, params, recovery, span)
                }
                value => self.convert_to_eager_sequence(value, target_ty, struct_defaults, span),
            },
            Type::Struct(def_id) => {
                self.convert_to_struct(value, source_ty, *def_id, recovery, struct_defaults, span)
            }
            // N7E-3 (D1, D2, D3): a conversion into a policy-float cell is the
            // conversion to the bare float of the cell's width; a `trapping`
            // cell then checks the result, as the end of the conversion row.
            Type::PolicyFloat(policy, width) => self
                .convert_to_policy_float(value, source_ty, *policy, *width, params, recovery, span),
            other => Err(RunnerError::unsupported(
                span,
                &format!("runtime conversio to {other:?}"),
            )),
        }
    }

    /// `bN ↦ iN`, `iN ↦ bN`, `bN ↦ fN` and `fN ↦ bN` at one width. The
    /// checker admits only exact width matches. `None` when neither side is a
    /// `bN` paired with a signed integer or a float, so the ordinary rows run.
    fn reinterpret_bits(&self, value: &Value, source_ty: TypeId, target: &Type) -> Option<Value> {
        let source = self.types.get(self.resolve_conversion_target(source_ty));
        match (source, target) {
            (Type::Bits(width), Type::SizedNumeric(Primitive::Numerus, signed))
                if is_signed_width(*signed) =>
            {
                let pattern = bits_pattern(value)?;
                let shift = 64 - width.bits();
                Some(Value::Int((pattern << shift).cast_signed() >> shift))
            }
            (Type::Bits(width), Type::SizedNumeric(Primitive::Fractus, float)) => {
                float_from_pattern(bits_pattern(value)?, *width, *float)
            }
            (Type::Bits(width), Type::Primitive(Primitive::Fractus)) => {
                float_from_pattern(bits_pattern(value)?, *width, NumericWidth::F64)
            }
            (Type::SizedNumeric(Primitive::Numerus, signed), Type::Bits(width))
                if is_signed_width(*signed) =>
            {
                let Value::Int(v) = value else { return None };
                let mask = if width.bits() == 64 {
                    u64::MAX
                } else {
                    (1u64 << width.bits()) - 1
                };
                Some(Value::UInt(v.cast_unsigned() & mask))
            }
            (Type::SizedNumeric(Primitive::Fractus, float), Type::Bits(width)) => {
                pattern_from_float(value, *float, *width)
            }
            (Type::Primitive(Primitive::Fractus), Type::Bits(width)) => {
                pattern_from_float(value, NumericWidth::F64, *width)
            }
            _ => None,
        }
    }

    /// Whether `ty` is `lista<b8>` (aliases looked through).
    fn is_bytes_list_type(&self, ty: TypeId) -> bool {
        let Type::Array(element) = self.types.get(self.resolve_conversion_target(ty)) else {
            return false;
        };
        matches!(
            self.types.get(self.resolve_conversion_target(*element)),
            Type::Bits(radix_types::BitsWidth::B8)
        )
    }

    fn is_dynamic_source(&self, source_ty: TypeId) -> bool {
        matches!(
            self.types.get(source_ty),
            Type::Primitive(Primitive::Valor | Primitive::Ignotum)
        )
    }

    fn resolve_conversion_target(&self, mut target_ty: TypeId) -> TypeId {
        while let Type::Alias(_, inner) = self.types.get(target_ty) {
            target_ty = *inner;
        }
        target_ty
    }
}

/// The bytes of a `lista<b8>` value held as a list of byte patterns.
fn bytes_of_list(items: &[Value], span: Span) -> Result<Vec<u8>, RunnerError> {
    items
        .iter()
        .map(|item| bits_pattern(item).and_then(|pattern| u8::try_from(pattern).ok()))
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(|| {
            RunnerError::internal(
                span,
                "bytes tensor conversio met an element that is not a byte",
            )
        })
}

fn is_signed_width(width: NumericWidth) -> bool {
    matches!(
        width,
        NumericWidth::I8 | NumericWidth::I16 | NumericWidth::I32 | NumericWidth::I64
    )
}

/// A byte value as the `lista<b8>` the language sees: one unsigned pattern
/// per byte. Any other value passes through unchanged.
pub(crate) fn octeti_into_bits_list(value: Value) -> Value {
    match value {
        Value::Octeti(bytes) => Value::Array(Rc::new(RefCell::new(
            bytes
                .into_iter()
                .map(|byte| Value::UInt(u64::from(byte)))
                .collect(),
        ))),
        other => other,
    }
}

/// The unsigned pattern a `bN` cell holds.
fn bits_pattern(value: &Value) -> Option<u64> {
    match value {
        Value::UInt(v) => Some(*v),
        Value::Int(v) => Some(v.cast_unsigned()),
        _ => None,
    }
}

/// Read a `bN` pattern as the float of the same width (`f16` and `bf16` pair
/// with `b16`, `f32` with `b32`, `f64` with `b64`).
fn float_from_pattern(pattern: u64, width: BitsWidth, float: NumericWidth) -> Option<Value> {
    let value = match (width, float) {
        (BitsWidth::B16, NumericWidth::F16) => {
            let bits = u16::try_from(pattern & 0xffff).ok()?;
            // A NaN keeps its payload: the half seam canonicalizes it.
            if bits & 0x7c00 == 0x7c00 && bits & 0x03ff != 0 {
                let sign = u32::from(bits >> 15) << 31;
                let payload = u32::from(bits & 0x03ff) << 13;
                f64::from(f32::from_bits(sign | 0x7f80_0000 | payload))
            } else {
                f64::from(super::half_to_f32(bits))
            }
        }
        (BitsWidth::B16, NumericWidth::BF16) => f64::from(f32::from_bits(
            u32::from(u16::try_from(pattern & 0xffff).ok()?) << 16,
        )),
        (BitsWidth::B32, NumericWidth::F32) => {
            f64::from(f32::from_bits(u32::try_from(pattern & 0xffff_ffff).ok()?))
        }
        (BitsWidth::B64, NumericWidth::F64) => f64::from_bits(pattern),
        _ => return None,
    };
    Some(Value::Float(value))
}

/// Write a float as the `bN` pattern of the same width.
fn pattern_from_float(value: &Value, float: NumericWidth, width: BitsWidth) -> Option<Value> {
    let Value::Float(v) = value else { return None };
    let pattern = match (float, width) {
        (NumericWidth::F16, BitsWidth::B16) => {
            let wide = (*v as f32).to_bits();
            if v.is_nan() {
                // Keep the sign and the top payload bits; never collapse to infinity.
                let payload = ((wide >> 13) & 0x03ff).max(0x0200);
                u64::from(u16::try_from(((wide >> 16) & 0x8000) | 0x7c00 | payload).ok()?)
            } else {
                u64::from(super::f32_to_f16_rn(*v as f32))
            }
        }
        (NumericWidth::BF16, BitsWidth::B16) => {
            let wide = (*v as f32).to_bits();
            if v.is_nan() {
                u64::from(u16::try_from((wide >> 16) | 0x0040).ok()?)
            } else {
                let rounded = wide.wrapping_add(0x7fff + ((wide >> 16) & 1));
                u64::from((rounded >> 16) as u16)
            }
        }
        (NumericWidth::F32, BitsWidth::B32) => u64::from((*v as f32).to_bits()),
        (NumericWidth::F64, BitsWidth::B64) => v.to_bits(),
        _ => return None,
    };
    Some(Value::UInt(pattern))
}

fn dynamic_conversion_error(target: &Type) -> String {
    let label = match target {
        Type::Primitive(Primitive::Textus) => "textus",
        Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
            "numerus"
        }
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
            "fractus"
        }
        Type::Primitive(Primitive::Bivalens) => "bivalens",
        Type::Primitive(Primitive::Ascii) => "ascii",
        Type::Primitive(Primitive::Octeti) => "octeti",
        Type::Primitive(Primitive::Json) => "json",
        Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => "instans",
        Type::Array(_) => "lista",
        Type::Map(_, _) => "tabula",
        Type::Struct(_) => "genus",
        Type::Tensor(_, _) => "tensor",
        _ => "target",
    };
    format!("valor to {label} conversion failed")
}

fn instans_precision(target: &Type) -> InstansPraecisio {
    match target {
        Type::SizedInstans(InstansPrecision::Millis) => InstansPraecisio::Millisecunda,
        Type::SizedInstans(InstansPrecision::Micros) => InstansPraecisio::Microsecunda,
        Type::SizedInstans(InstansPrecision::Nanos) => InstansPraecisio::Nanosecunda,
        _ => InstansPraecisio::Secunda,
    }
}
