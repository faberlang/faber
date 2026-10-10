//! `↦` rows into and out of the unbounded integer `inf` (inf-delivery-spec
//! ruling 40): every row is an existing integer row with the width bound
//! removed, so the conversion to `inf` is exact and the failable ones are only
//! the ones the source can fail (NaN/±∞, malformed text).
//!
//! The rows *out of* `inf` (narrowing, `wrapping`/`saturating`, floats, `d64`,
//! `textus`, `ascii<N> via Hex/Bin/Oct`) sit beside their bounded-integer
//! siblings in `scalar.rs`, reading a canonical `Value::Magnus` (see
//! `crate::inf`); the rows *into* `inf` and the `octeti via Be/Le` pair live
//! here.

use super::rounding::RoundingMode;
use super::{Runner, RunnerError, Value};
use crate::inf::{from_magnus, to_magnus};
use handroll::{Magnus, Radix};
use radix_codegen_shared::hints::EndianHint;
use radix_lexer::{Span, Symbol};
use radix_mir::MirOperand;
use radix_types::{NumericWidth, Primitive, Type, TypeId, effective_width};

impl<H: super::super::Host + ?Sized> Runner<'_, H> {
    /// Any source `↦ inf` (ruling 40). Bounded integers and policy words widen
    /// unchanged; a float truncates toward zero and fails on NaN and `±∞`; a
    /// `d64` reduces as it does to `int`; text parses any length of digits
    /// (`via Hex/Bin/Oct`); `octeti via Be/Le` decodes minimal two's
    /// complement. A nullable `valor ∪ none` source reads its payload by the
    /// `valor ↦ inf` law, not by the payload's own shape.
    pub(super) fn convert_to_inf(
        &mut self,
        value: Value,
        source_ty: TypeId,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // A nullable `valor ∪ none` carries a dynamic payload, exactly as the
        // non-nullable `valor` does; the union/option source type is not the
        // `Primitive::Valor` that routes `convert_to_target` through the
        // dynamic extraction. Read only the integer carriers here, so a float
        // fails with the non-nullable path's text instead of truncating on the
        // payload's own shape (LANES.md inf-U8b residual (e), inf-nul).
        if let Some(inner) = self.types.nullable_inner(source_ty)
            && matches!(
                self.types.get(inner),
                Type::Primitive(Primitive::Valor | Primitive::Ignotum)
            )
        {
            return match to_magnus(&value) {
                Some(_) => Ok(value),
                None => self.recover_or_fail(recovery, span, "valor to numerus conversion failed"),
            };
        }
        if let Some(source_width) =
            effective_width(self.types, source_ty).filter(|width| width.is_decimal())
        {
            let mode = self.rounding_hint(params).unwrap_or(RoundingMode::HalfEven);
            let rounded = self.decimal_integer_value(value, source_width, mode, span)?;
            return Ok(from_magnus(Magnus::from_i128(rounded)));
        }
        match value {
            Value::Int(_) | Value::UInt(_) | Value::Magnus(_) => Ok(value),
            Value::Bool(v) => Ok(Value::Int(i64::from(v))),
            Value::Float(v) => match Magnus::from_f64(
                self.rounding_hint(params)
                    .unwrap_or(RoundingMode::Trunc)
                    .round_float(v),
            ) {
                Ok(exact) => Ok(from_magnus(exact)),
                Err(_) => self.recover_or_fail(
                    recovery,
                    span,
                    "fractus to numerus conversion out of range",
                ),
            },
            Value::Str(text) => {
                let radix = match self.numerus_radix(params) {
                    Some(16) => Radix::Hex,
                    Some(2) => Radix::Bin,
                    Some(8) => Radix::Oct,
                    _ => Radix::Dec,
                };
                match Magnus::parse_radix(&text, radix) {
                    Ok(exact) => Ok(from_magnus(exact)),
                    Err(_) => {
                        self.recover_or_fail(recovery, span, "textus to numerus conversion failed")
                    }
                }
            }
            Value::Octeti(bytes) => match self.inf_endian(params) {
                Some(EndianHint::Be) => Ok(from_magnus(Magnus::from_bytes_be(&bytes))),
                Some(EndianHint::Le) => Ok(from_magnus(Magnus::from_bytes_le(&bytes))),
                None => Err(RunnerError::internal(
                    span,
                    "unsupported conversio to numerus from Octeti",
                )),
            },
            Value::Sermo(sermo) => {
                let first = self.sermo_first_item(&sermo, span)?;
                self.convert_to_inf(first, source_ty, params, recovery, span)
            }
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to numerus from {other:?}"),
            )),
        }
    }

    /// `inf ↦ octeti via Be/Le` (ruling 40): the minimal two's-complement
    /// encoding (`0` is `00`; a positive value with its top bit set gets a
    /// leading `00`). Any other source keeps the ordinary octeti rows.
    pub(crate) fn convert_to_octeti_from(
        &mut self,
        value: Value,
        source_ty: TypeId,
        params: &[Symbol],
        span: Span,
    ) -> Result<Value, RunnerError> {
        // A conversation reply (`ad 'solum:hauri' (via) ↦ octeti`) materializes
        // its first content item, as `↦ textus` and `↦ numerus` do.
        if let Value::Sermo(sermo) = value {
            let item = self.sermo_first_item(&sermo, span)?;
            return self.convert_to_octeti_from(item, source_ty, params, span);
        }
        let inf_source = effective_width(self.types, source_ty) == Some(NumericWidth::Inf);
        if inf_source && let Some(endian) = self.inf_endian(params) {
            let Some(exact) = to_magnus(&value) else {
                return Err(RunnerError::internal(
                    span,
                    "inf to octeti conversio carried a non-integer value",
                ));
            };
            return Ok(Value::Octeti(match endian {
                EndianHint::Be => exact.to_bytes_be(),
                EndianHint::Le => exact.to_bytes_le(),
            }));
        }
        // `tensor<E, [shape]> ↦ octeti via Le|Be`: the infallible inverse of
        // the bulk `octeti ↦ tensor` reinterpretation.
        if let Value::Tensor(tensor) = &value {
            return self.convert_tensor_to_octeti(&tensor.borrow(), source_ty, params, span);
        }
        self.convert_to_octeti(value, span)
    }

    fn inf_endian(&self, params: &[Symbol]) -> Option<EndianHint> {
        radix_codegen_shared::hints::endian_hint(params, |param| {
            radix_codegen_shared::hints::classify(self.interner.resolve(*param))
        })
    }
}
