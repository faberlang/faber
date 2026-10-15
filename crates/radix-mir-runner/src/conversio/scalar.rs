#![allow(
    clippy::bool_to_int_with_if,
    clippy::cast_lossless,
    clippy::format_push_string,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::only_used_in_recursion,
    clippy::map_unwrap_or,
    clippy::unused_self,
    clippy::wildcard_imports
)]
// Semantic numeric conversions between host value kinds during conversio
// lowering: ordinary fractus→numerus keeps its truncation law, while decimal
// carriers parse and reduce round-half-even; width range-checks compare in
// i128/u128, and numerus→fractus promotes to f64.
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "semantic numeric conversions between host value kinds in conversio lowering"
)]

use super::*;
use radix_codegen_shared::hints::{self, EndianHint};
use radix_types::IndexExpr;

use super::rounding::RoundingMode;

fn octeti_array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut arr = [0_u8; N];
    arr.copy_from_slice(bytes);
    arr
}

fn pack_ascii_digits(value: u128, width: usize, radix: u32) -> Option<String> {
    let unpadded = match radix {
        16 => format!("{value:x}"),
        2 => format!("{value:b}"),
        8 => format!("{value:o}"),
        _ => return None,
    };
    if unpadded.len() > width {
        return None;
    }
    Some(format!("{unpadded:0>width$}"))
}

impl<H: super::super::Host + ?Sized> Runner<'_, H> {
    pub(crate) fn convert_to_textus(
        &mut self,
        value: Value,
        source_ty: TypeId,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if let Some(width) = decimal_source_width(self.types, source_ty) {
            if let Value::Dec(carrier, scale) = value {
                return Ok(Value::Str(crate::decimal::format(carrier, scale)));
            }
            let carrier = match value {
                Value::Int(value) => decimal_carrier_value(value, width, span)?,
                Value::Float(value) => decimal_float_carrier(value, width, span)?,
                other => {
                    return Err(RunnerError::internal(
                        span,
                        format!("unsupported decimal value for textus: {other:?}"),
                    ));
                }
            };
            return Ok(Value::Str(format_decimal_carrier(carrier, width)));
        }
        if matches!(self.types.get(source_ty), Type::Primitive(Primitive::Json)) {
            let value = self.prepare_tensor_escape(value, span)?;
            // Render the JSON document directly from concrete `Value`, mirroring
            // the prior `value_to_runtime_json_valor` -> `faber::Json::to_wire`
            // composition without the `Valor`/`Json` middleman.
            if !self.check_json_document(&value, span)? {
                return Err(RunnerError::conversion(
                    span,
                    "json to textus conversion failed",
                ));
            }
            return Ok(Value::Str(self.value_to_json_wire(&value, span)?));
        }
        Ok(Value::Str(match value {
            Value::Str(text) => text,
            Value::Ascii(b) => char::from(b).to_string(),
            Value::Char(ch) => ch.to_string(),
            Value::Int(v) => v.to_string(),
            Value::UInt(v) => v.to_string(),
            // `inf ↦ textus` (ruling 40): decimal digits, `-` sign.
            Value::Magnus(v) => v.to_string(),
            Value::Float(v) => match Self::element_display_width(source_ty, self.types, 0) {
                // Declared narrower float: same repr as `print`/interpolation.
                Some(width) if width != NumericWidth::F64 => {
                    crate::value::display_float_at_width(v, Some(width))
                }
                _ => v.to_string(),
            },
            Value::Bool(v) => v.to_string(),
            Value::Instans(v) => v.to_rfc3339(),
            Value::Sermo(sermo) => {
                return self.materialize_sermo_textus(sermo, recovery, span);
            }
            Value::Octeti(bytes) => {
                return self.decode_textus_octeti(bytes, recovery, span);
            }
            Value::Array(items) => {
                // `octeti` is semantically bidirectionally assignable with
                // `lista<numerus<u8>>`. The runner keeps those carriers
                // distinct, so a list assigned into an octeti slot must be
                // normalized at this conversion boundary before UTF-8
                // decoding. Backends with a shared byte-vector carrier do
                // not need this representation repair.
                let bytes = items
                    .borrow()
                    .iter()
                    .map(|item| match item {
                        Value::Int(value) => u8::try_from(*value).ok(),
                        Value::UInt(value) => u8::try_from(*value).ok(),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(bytes) = bytes else {
                    return self.recover_or_fail(
                        recovery,
                        span,
                        "octeti to textus conversion failed",
                    );
                };
                return self.decode_textus_octeti(bytes, recovery, span);
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("unsupported conversio to textus from {other:?}"),
                ));
            }
        }))
    }

    /// UTF-8 decode shared by the `octeti` and `lista<numerus<u8>>` textus
    /// arms.
    fn decode_textus_octeti(
        &mut self,
        bytes: Vec<u8>,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match String::from_utf8(bytes) {
            Ok(text) => Ok(Value::Str(text)),
            Err(_) => self.recover_or_fail(recovery, span, "octeti to textus conversion failed"),
        }
    }

    pub(crate) fn convert_to_numerus_from(
        &mut self,
        value: Value,
        source_ty: TypeId,
        params: &[Symbol],
        width: NumericWidth,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // Ruling 40: every source into `inf` is exact; the row table lives in
        // `conversio/inf.rs`.
        if width.is_unbounded() {
            return self.convert_to_inf(value, source_ty, params, recovery, span);
        }
        if width.is_decimal() {
            return self.convert_to_decimal(value, source_ty, width, recovery, span);
        }
        if let Some(source_width) = decimal_source_width(self.types, source_ty) {
            return self.convert_decimal_to_integer(
                value,
                source_width,
                width,
                self.rounding_hint(params),
                recovery,
                span,
            );
        }
        match value {
            // `inf ↦ int<W>` (ruling 40): a `Magnus` is canonically beyond the
            // 64-bit carriers, so it fits no bounded width — the failable
            // magnitude-checked narrowing always fails.
            Value::Magnus(_) => {
                self.recover_or_fail(recovery, span, "numerus to numerus conversion out of range")
            }
            Value::Str(text) => match self.parse_textus_numerus(&text, params) {
                // SA-002: range-check against `W` so `textus ↦ numerus<u8>` of
                // "300" fails here exactly as Rust codegen's `.parse::<u8>()`.
                // The parse is exact (`i128`), so a `u64` target keeps every
                // magnitude through `u64::MAX` (u64-parse).
                Ok(v) if exact_fits_width(v, width) => Ok(exact_numerus_value(v)),
                Ok(_) => self.recover_or_fail(
                    recovery,
                    span,
                    "textus to numerus conversion out of range",
                ),
                Err(_) => {
                    self.recover_or_fail(recovery, span, "textus to numerus conversion failed")
                }
            },
            // D1.11 / D11.8: `fractus ↦ numerus<W>` is a check. The fraction
            // goes by the `via` rounding (`Trunc`, toward zero, when none is
            // named); a NaN, an infinity, or a rounded value outside `W`'s
            // range fails the conversion (never saturates).
            Value::Float(v) => {
                let rounded = self.float_rounding(params, v);
                if float_truncation_fits_width(rounded, width) {
                    Ok(float_to_numerus_width(rounded, width))
                } else {
                    self.recover_or_fail(
                        recovery,
                        span,
                        "fractus to numerus conversion out of range",
                    )
                }
            }
            Value::Bool(v) => Ok(Value::Int(if v { 1 } else { 0 })),
            Value::Int(v) => {
                // GT-3 / D1.11: conversion never wraps (DEFER-116 boundary
                // contract). The value is magnitude-checked against `W` — a
                // negative into an unsigned target and a magnitude past either
                // bound both fail closed, instead of leaving an out-of-range
                // carrier in a narrow slot (`300 ↦ numerus<u8>` printed 300).
                if !numerus_fits_width(v, width) {
                    return self.recover_or_fail(
                        recovery,
                        span,
                        "numerus to numerus conversion out of range",
                    );
                }
                Ok(Value::Int(v))
            }
            Value::UInt(v) if width.is_unsigned_int() => {
                let (_, hi) = int_width_range(width);
                if v as u128 <= hi as u128 {
                    Ok(Value::UInt(v))
                } else {
                    self.recover_or_fail(
                        recovery,
                        span,
                        "numerus to numerus conversion out of range",
                    )
                }
            }
            Value::UInt(v) => match i64::try_from(v) {
                Ok(v) if numerus_fits_width(v, width) => Ok(Value::Int(v)),
                _ => self.recover_or_fail(
                    recovery,
                    span,
                    "numerus to numerus conversion out of range",
                ),
            },
            Value::Sermo(sermo) => match self.sermo_first_item(&sermo, span) {
                Ok(value) => {
                    self.convert_to_numerus_from(value, source_ty, params, width, recovery, span)
                }
                Err(_) if recovery.is_some() => {
                    self.recover_or_fail(recovery, span, "sermo to numerus conversion failed")
                }
                Err(error) => Err(error),
            },
            Value::Octeti(bytes) => {
                self.convert_octeti_endian_numerus(bytes, params, width, recovery, span)
            }
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to numerus from {other:?}"),
            )),
        }
    }

    fn convert_to_decimal(
        &mut self,
        value: Value,
        source_ty: TypeId,
        target_width: NumericWidth,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let source_width = decimal_source_width(self.types, source_ty);
        let result = match value {
            Value::Int(value) => match source_width {
                // d64 is the only decimal width: decimal → decimal is identity.
                Some(source_width) => decimal_carrier_from_raw(i128::from(value), source_width),
                None => decimal_integer_to_carrier(i128::from(value), target_width),
            },
            Value::UInt(value) => decimal_integer_to_carrier(i128::from(value), target_width),
            // `inf ↦ d64` (ruling 40): beyond the 64-bit carriers is beyond
            // the d64 range (±92,233,720,368.54775807).
            Value::Magnus(_) => Err(DecimalTextError::OutOfRange),
            // An exact intermediate lands at scale 8: half-even, range-checked.
            Value::Dec(carrier, scale) => {
                crate::decimal::store_carrier(carrier, scale).ok_or(DecimalTextError::OutOfRange)
            }
            Value::Float(value) => decimal_text_to_carrier(&value.to_string(), target_width),
            Value::Str(text) => decimal_text_to_carrier(&text, target_width),
            Value::Sermo(sermo) => {
                let value = self.sermo_first_item(&sermo, span)?;
                return self.convert_to_decimal(value, source_ty, target_width, recovery, span);
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("unsupported conversio to decimal from {other:?}"),
                ));
            }
        };
        match result {
            Ok(value) => Ok(Value::Int(value)),
            Err(DecimalTextError::Invalid) => {
                self.recover_or_fail(recovery, span, "textus to numerus conversion failed")
            }
            Err(DecimalTextError::OutOfRange) => {
                self.recover_or_fail(recovery, span, "textus to numerus conversion out of range")
            }
        }
    }

    /// Exact integer a decimal value reduces to (round-half-even): the shared
    /// integer choice of `d64 ↦ numerus<W>` and `d64 ↦ wrapping/saturating<W>`.
    pub(super) fn decimal_integer_value(
        &mut self,
        value: Value,
        source_width: NumericWidth,
        mode: RoundingMode,
        span: Span,
    ) -> Result<i128, RunnerError> {
        if let Value::Dec(carrier, scale) = value {
            return crate::decimal::pow10(scale)
                .and_then(|factor| mode.round_quotient(carrier, factor))
                .ok_or_else(|| {
                    RunnerError::conversion(span, "decimal to numerus conversion failed")
                });
        }
        let raw = match value {
            Value::Int(value) => decimal_carrier_value(value, source_width, span),
            Value::Float(value) => decimal_float_carrier(value, source_width, span),
            Value::Sermo(sermo) => {
                let value = self.sermo_first_item(&sermo, span)?;
                return self.decimal_integer_value(value, source_width, mode, span);
            }
            other => Err(RunnerError::internal(
                span,
                format!("unsupported decimal conversion from {other:?}"),
            )),
        }?;
        let scale = decimal_scale_factor(source_width).ok_or_else(|| {
            RunnerError::internal(span, "decimal conversion requires decimal width")
        })?;
        mode.round_quotient(i128::from(raw), scale)
            .ok_or_else(|| RunnerError::conversion(span, "decimal to numerus conversion failed"))
    }

    fn convert_decimal_to_integer(
        &mut self,
        value: Value,
        source_width: NumericWidth,
        target_width: NumericWidth,
        mode: Option<RoundingMode>,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // A decimal with no `via` rounding keeps the half-even default.
        let mode = mode.unwrap_or(RoundingMode::HalfEven);
        let rounded = self.decimal_integer_value(value, source_width, mode, span)?;
        let (lo, hi) = int_width_range(target_width);
        if !(lo..=hi).contains(&rounded) {
            return self.recover_or_fail(
                recovery,
                span,
                "decimal to numerus conversion out of range",
            );
        }
        if target_width.is_unsigned_int() {
            return u64::try_from(rounded).map(Value::UInt).map_err(|_| {
                RunnerError::conversion(span, "decimal to numerus conversion out of range")
            });
        }
        i64::try_from(rounded).map(Value::Int).map_err(|_| {
            RunnerError::conversion(span, "decimal to numerus conversion out of range")
        })
    }

    fn convert_octeti_endian_numerus(
        &mut self,
        bytes: Vec<u8>,
        params: &[Symbol],
        width: NumericWidth,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Some(endian) = self.endian_hint(params) else {
            return Err(RunnerError::internal(
                span,
                "unsupported conversio to numerus from Octeti",
            ));
        };
        let need = match width {
            NumericWidth::I16 | NumericWidth::U16 => 2,
            NumericWidth::I32 | NumericWidth::U32 => 4,
            NumericWidth::I64 | NumericWidth::U64 => 8,
            _ => {
                return self.recover_or_fail(recovery, span, "octeti endian width unsupported");
            }
        };
        if bytes.len() != need {
            return self.recover_or_fail(recovery, span, "octeti endian width mismatch");
        }
        match (width, endian) {
            (NumericWidth::I16, EndianHint::Be) => Ok(Value::Int(i64::from(i16::from_be_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::I16, EndianHint::Le) => Ok(Value::Int(i64::from(i16::from_le_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::I32, EndianHint::Be) => Ok(Value::Int(i64::from(i32::from_be_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::I32, EndianHint::Le) => Ok(Value::Int(i64::from(i32::from_le_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::I64, EndianHint::Be) => {
                Ok(Value::Int(i64::from_be_bytes(octeti_array(&bytes))))
            }
            (NumericWidth::I64, EndianHint::Le) => {
                Ok(Value::Int(i64::from_le_bytes(octeti_array(&bytes))))
            }
            (NumericWidth::U16, EndianHint::Be) => Ok(Value::UInt(u64::from(u16::from_be_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::U16, EndianHint::Le) => Ok(Value::UInt(u64::from(u16::from_le_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::U32, EndianHint::Be) => Ok(Value::UInt(u64::from(u32::from_be_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::U32, EndianHint::Le) => Ok(Value::UInt(u64::from(u32::from_le_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::U64, EndianHint::Be) => {
                Ok(Value::UInt(u64::from_be_bytes(octeti_array(&bytes))))
            }
            (NumericWidth::U64, EndianHint::Le) => {
                Ok(Value::UInt(u64::from_le_bytes(octeti_array(&bytes))))
            }
            _ => self.recover_or_fail(recovery, span, "octeti endian width unsupported"),
        }
    }

    fn endian_hint(&self, params: &[Symbol]) -> Option<EndianHint> {
        hints::endian_hint(params, |param| {
            hints::classify(self.interner.resolve(*param))
        })
    }

    /// A float rounded by the `via` clause (`Trunc` when none is named).
    fn float_rounding(&self, params: &[Symbol], value: f64) -> f64 {
        self.rounding_hint(params)
            .unwrap_or(RoundingMode::Trunc)
            .round_float(value)
    }

    /// A float source whose rounded value is NaN or infinite has no integer.
    fn float_fails_rounding(&self, value: &Value, params: &[Symbol]) -> bool {
        matches!(value, Value::Float(v) if !self.float_rounding(params, *v).is_finite())
    }

    /// D11.6 ruling 24: the exact integer a `wrapping`/`saturating` conversion
    /// applies its policy to. Integer sources of any family carry it directly;
    /// a `d64` source reduces to an integer exactly as `d64 ↦ numerus<W>` does.
    fn policy_word_source_value(
        &mut self,
        value: Value,
        source_ty: TypeId,
        params: &[Symbol],
        span: Span,
    ) -> Result<i128, RunnerError> {
        if let Some(source_width) = decimal_source_width(self.types, source_ty) {
            let mode = self.rounding_hint(params).unwrap_or(RoundingMode::HalfEven);
            return self.decimal_integer_value(value, source_width, mode, span);
        }
        match value {
            Value::Int(value) => Ok(i128::from(value)),
            Value::UInt(value) => Ok(i128::from(value)),
            // `fractus ↦ wrapping<W>` / `saturating<W>`: the `via` rounding
            // makes the integer (the callers fail NaN and infinity first); a
            // magnitude beyond i128 saturates there and the policy word then
            // clamps or reduces it like any large integer.
            Value::Float(value) => Ok(self.float_rounding(params, value) as i128),
            _ => Err(RunnerError::internal(
                span,
                "wrapping/saturating conversion requires an integer value",
            )),
        }
    }

    /// `↦ wrapping<W>` reduces the exact source value mod 2^W; it cannot fail.
    pub(crate) fn convert_to_modular_word(
        &mut self,
        value: Value,
        source_ty: TypeId,
        width: NumericWidth,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // `inf ↦ wrapping<W>`: reduce the exact bignum mod 2^W (ruling 40).
        if let Value::Magnus(big) = &value {
            return Ok(crate::inf::wrap_to_width(big, width));
        }
        if self.float_fails_rounding(&value, params) {
            return self.recover_or_fail(
                recovery,
                span,
                "fractus to numerus conversion out of range",
            );
        }
        // A float source reduces by its exact integer value: narrowing it
        // through `i128` first would saturate a huge magnitude before the ring.
        if let Value::Float(float) = value {
            return Ok(modular_word_float_result(
                self.float_rounding(params, float),
                width,
            ));
        }
        let exact = self.policy_word_source_value(value, source_ty, params, span)?;
        Ok(modular_word_result(exact, width))
    }

    /// `↦ saturating<W>` clamps the exact source value to W's range; it cannot fail.
    pub(crate) fn convert_to_saturating_word(
        &mut self,
        value: Value,
        source_ty: TypeId,
        width: NumericWidth,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // `inf ↦ saturating<W>`: clamp the exact bignum to `W` (ruling 40).
        if let Value::Magnus(big) = &value {
            return Ok(crate::inf::clamp_to_width(big, width));
        }
        if self.float_fails_rounding(&value, params) {
            return self.recover_or_fail(
                recovery,
                span,
                "fractus to numerus conversion out of range",
            );
        }
        let exact = self.policy_word_source_value(value, source_ty, params, span)?;
        Ok(saturating_word_result(exact, width))
    }

    /// `↦ trapping<W>` and `↦ saturating<W>` (D1, D2, D3). The conversion is
    /// the one into the bare float of width `W`, with every row of it (`inf`
    /// beyond the finite range gives ±infinity, text parses, integers widen).
    /// A `saturating` cell keeps that result as is (bare IEEE, no clamp). A
    /// `trapping` cell checks the result at the end of the row: a non-finite
    /// value raises the trapping overflow error (`numerus overflow`), which an
    /// authored `⊥` recovers.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convert_to_policy_float(
        &mut self,
        value: Value,
        source_ty: TypeId,
        policy: FloatPolicy,
        width: NumericWidth,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let converted =
            self.convert_to_fractus_from(value, source_ty, params, width, recovery, span)?;
        if policy == FloatPolicy::Trapping
            && let Value::Float(float) = converted
            && trapping_float_store_fails(float, width)
        {
            return match recovery {
                Some(recovery) => self.eval_recovery(recovery, span),
                None => Err(RunnerError::internal(span, "numerus overflow")),
            };
        }
        Ok(converted)
    }

    pub(crate) fn convert_to_vacuum(
        &mut self,
        value: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Value::Sermo(sermo) = value else {
            return Err(RunnerError::internal(
                span,
                "unsupported conversio to vacuum",
            ));
        };
        self.sermo_items(&sermo, span)?;
        Ok(Value::Unit)
    }

    fn parse_textus_numerus(
        &self,
        text: &str,
        params: &[Symbol],
    ) -> Result<i128, std::num::ParseIntError> {
        match self.numerus_radix(params) {
            Some(radix) => i128::from_str_radix(text, radix),
            None => text.parse::<i128>(),
        }
    }

    pub(super) fn numerus_radix(&self, params: &[Symbol]) -> Option<u32> {
        params
            .iter()
            .find_map(|param| match self.interner.resolve(*param) {
                "hex" | "Hex" => Some(16),
                "bin" | "Bin" => Some(2),
                "oct" | "Oct" => Some(8),
                _ => None,
            })
    }

    #[allow(dead_code)]
    pub(crate) fn convert_to_fractus(
        &mut self,
        value: Value,
        params: &[Symbol],
        width: NumericWidth,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        self.convert_to_fractus_from(value, TypeId(0), params, width, recovery, span)
    }

    pub(crate) fn convert_to_fractus_from(
        &mut self,
        value: Value,
        source_ty: TypeId,
        params: &[Symbol],
        width: NumericWidth,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if let Value::Sermo(sermo) = value {
            let value = self.sermo_first_item(&sermo, span)?;
            return self.convert_to_fractus_from(value, source_ty, params, width, recovery, span);
        }
        if let Some(source_width) = decimal_source_width(self.types, source_ty) {
            let (carrier, scale) = match value {
                Value::Int(value) => {
                    let scale = decimal_scale_factor(source_width).ok_or_else(|| {
                        RunnerError::internal(span, "decimal conversion requires decimal width")
                    })?;
                    (
                        i128::from(decimal_carrier_value(value, source_width, span)?),
                        scale,
                    )
                }
                Value::Dec(carrier, scale) => (
                    carrier,
                    crate::decimal::pow10(scale).ok_or_else(|| {
                        RunnerError::internal(span, "decimal conversion scale out of range")
                    })?,
                ),
                Value::Float(value) => {
                    let scale = decimal_scale_factor(source_width).ok_or_else(|| {
                        RunnerError::internal(span, "decimal conversion requires decimal width")
                    })?;
                    (
                        i128::from(decimal_float_carrier(value, source_width, span)?),
                        scale,
                    )
                }
                other => {
                    return Err(RunnerError::internal(
                        span,
                        format!("unsupported decimal conversion from {other:?}"),
                    ));
                }
            };
            let value = (carrier as f64) / (scale as f64);
            let value = if matches!(width, NumericWidth::F64) {
                value
            } else {
                value as f32 as f64
            };
            return Ok(Value::Float(value));
        }
        // `inf ↦ f16/bf16/f32/f64` (ruling 40): nearest-even, ±∞ beyond the
        // float's finite range. An `inf`-typed value the 64-bit carriers
        // still hold takes the same path so `f16` overflows to ±∞ too.
        let inf_source = matches!(value, Value::Magnus(_))
            || (matches!(value, Value::Int(_) | Value::UInt(_))
                && effective_width(self.types, source_ty) == Some(NumericWidth::Inf));
        if inf_source && let Some(big) = crate::inf::to_magnus(&value) {
            return Ok(Value::Float(crate::inf::to_float(&big, width)));
        }
        let value = match value {
            Value::Octeti(bytes) => {
                if self.endian_hint(params).is_some() {
                    return self
                        .convert_octeti_endian_fractus(bytes, params, width, recovery, span);
                }
                return Err(RunnerError::internal(
                    span,
                    "unsupported conversio to fractus from Octeti",
                ));
            }
            other => other,
        };
        match value {
            // Text parses as `f64` then narrows: a double rounding for `f32`,
            // accepted for text sources.
            Value::Str(text) => match text.parse::<f64>() {
                Ok(v) if matches!(width, NumericWidth::F32) => {
                    Ok(Value::Float(f64::from(v as f32)))
                }
                Ok(v) => Ok(Value::Float(v)),
                Err(_) => {
                    self.recover_or_fail(recovery, span, "textus to fractus conversion failed")
                }
            },
            // `int ↦ f32` rounds once, nearest-even, straight from the
            // integer: going through `f64` first would round twice and can
            // land on a tie that the exact value is not on.
            Value::Int(v) if matches!(width, NumericWidth::F32) => {
                Ok(Value::Float(f64::from(v as f32)))
            }
            Value::UInt(v) if matches!(width, NumericWidth::F32) => {
                Ok(Value::Float(f64::from(v as f32)))
            }
            Value::Int(v) => Ok(Value::Float(v as f64)),
            Value::UInt(v) => Ok(Value::Float(v as f64)),
            Value::Bool(v) => Ok(Value::Float(if v { 1.0 } else { 0.0 })),
            // `f64 ↦ f32` rounds to the nearest binary32, ties to even;
            // overflow is ±inf (IEEE), nothing clamps.
            Value::Float(v) if matches!(width, NumericWidth::F32) => {
                Ok(Value::Float(f64::from(v as f32)))
            }
            Value::Float(v) => Ok(Value::Float(v)),
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to fractus from {other:?}"),
            )),
        }
    }

    fn convert_octeti_endian_fractus(
        &mut self,
        bytes: Vec<u8>,
        params: &[Symbol],
        width: NumericWidth,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Some(endian) = self.endian_hint(params) else {
            return Err(RunnerError::internal(
                span,
                "unsupported conversio to fractus from Octeti",
            ));
        };
        let need = match width {
            NumericWidth::F32 => 4,
            NumericWidth::F64 => 8,
            _ => {
                return self.recover_or_fail(recovery, span, "octeti endian width unsupported");
            }
        };
        if bytes.len() != need {
            return self.recover_or_fail(recovery, span, "octeti endian width mismatch");
        }
        match (width, endian) {
            (NumericWidth::F32, EndianHint::Be) => Ok(Value::Float(f64::from(f32::from_be_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::F32, EndianHint::Le) => Ok(Value::Float(f64::from(f32::from_le_bytes(
                octeti_array(&bytes),
            )))),
            (NumericWidth::F64, EndianHint::Be) => {
                Ok(Value::Float(f64::from_be_bytes(octeti_array(&bytes))))
            }
            (NumericWidth::F64, EndianHint::Le) => {
                Ok(Value::Float(f64::from_le_bytes(octeti_array(&bytes))))
            }
            _ => self.recover_or_fail(recovery, span, "octeti endian width unsupported"),
        }
    }

    pub(crate) fn convert_to_bivalens(
        &mut self,
        value: Value,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Str(text) => {
                self.bivalens_from_scalar_truthiness(!text.is_empty(), recovery, span)
            }
            Value::Int(v) => self.bivalens_from_scalar_truthiness(v != 0, recovery, span),
            Value::UInt(v) => self.bivalens_from_scalar_truthiness(v != 0, recovery, span),
            // Canonical `Magnus` is never zero.
            Value::Magnus(_) => self.bivalens_from_scalar_truthiness(true, recovery, span),
            Value::Float(v) => self.bivalens_from_scalar_truthiness(v != 0.0, recovery, span),
            Value::Bool(v) => Ok(Value::Bool(v)),
            Value::Sermo(sermo) => match self.sermo_first_item(&sermo, span) {
                Ok(value) => self.convert_to_bivalens(value, recovery, span),
                Err(_) if recovery.is_some() => {
                    self.recover_or_fail(recovery, span, "sermo to bivalens conversion failed")
                }
                Err(error) => Err(error),
            },
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to bivalens from {other:?}"),
            )),
        }
    }

    /// Map scalar truthiness to `bivalens`, honoring `⊥` default on falsy inputs.
    fn bivalens_from_scalar_truthiness(
        &mut self,
        truthy: bool,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if truthy {
            return Ok(Value::Bool(true));
        }
        if let Some(recovery) = recovery {
            return self.eval_recovery(recovery, span);
        }
        Ok(Value::Bool(false))
    }

    pub(crate) fn convert_to_octeti(&self, value: Value, span: Span) -> Result<Value, RunnerError> {
        Ok(match value {
            Value::Str(text) => Value::Octeti(text.into_bytes()),
            Value::Octeti(bytes) => Value::Octeti(bytes),
            Value::Array(items) => {
                let bytes = items
                    .borrow()
                    .iter()
                    .map(|item| match item {
                        Value::Int(value) => u8::try_from(*value).ok(),
                        Value::UInt(value) => u8::try_from(*value).ok(),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        RunnerError::internal(
                            span,
                            "lista to octeti conversion requires byte-valued numerus elements",
                        )
                    })?;
                Value::Octeti(bytes)
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("unsupported conversio to octeti from {other:?}"),
                ));
            }
        })
    }

    pub(crate) fn convert_to_valor(
        &mut self,
        value: Value,
        source_ty: TypeId,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if let Value::Sermo(sermo) = value {
            return self.sermo_first_item(&sermo, span);
        }
        if matches!(
            self.types.get(source_ty),
            Type::Primitive(Primitive::Octeti)
        ) {
            let Value::Octeti(bytes) = value else {
                return Err(RunnerError::internal(
                    span,
                    "octeti conversio carried a non-octeti value",
                ));
            };
            return Ok(Value::Array(Rc::new(RefCell::new(
                bytes
                    .into_iter()
                    .map(|byte| Value::Int(i64::from(byte)))
                    .collect(),
            ))));
        }
        Ok(value)
    }

    pub(crate) fn convert_to_json(
        &mut self,
        value: Value,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let value = self.prepare_tensor_escape(value, span)?;
        if self.check_json_document(&value, span)? {
            self.materialize_json_value(value, span)
        } else {
            self.recover_or_fail(recovery, span, "valor to json conversion failed")
        }
    }

    /// Validate `value` as a JSON document, mirroring the prior
    /// `value_to_runtime_json_valor` -> `faber::Json::try_from` composition
    /// without the `Valor`/`Json` middleman.
    ///
    /// Returns `Ok(true)` for a valid object-rooted JSON document, `Ok(false)`
    /// when the shape is sound but is not a JSON document (non-object root or a
    /// non-finite number anywhere in the tree), and `Err` for the structural
    /// hard errors the prior bridge raised (non-textus map key, or a variant
    /// outside the kernel valor bridge).
    fn check_json_document(&mut self, value: &Value, span: Span) -> Result<bool, RunnerError> {
        let safe = self.json_node_is_safe(value, span)?;
        Ok(safe && self.is_json_object_root(value))
    }

    fn is_json_object_root(&self, value: &Value) -> bool {
        matches!(value, Value::Map(_) | Value::Record(..) | Value::Struct(..))
    }

    fn materialize_json_value(&mut self, value: Value, span: Span) -> Result<Value, RunnerError> {
        Ok(match value {
            Value::Array(items) => {
                let items = items
                    .borrow()
                    .iter()
                    .cloned()
                    .map(|item| self.materialize_json_value(item, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Array(Rc::new(RefCell::new(items)))
            }
            Value::Tensor(tensor) => {
                if tensor.borrow().has_unresolved_edge_shift() {
                    return Err(RunnerError::internal(
                        span,
                        "unresolved shifted tensor cannot be converted to json",
                    ));
                }
                let items = self
                    .tensor_logical_values(&tensor, span)?
                    .into_iter()
                    .map(|item| self.materialize_json_value(item, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Array(Rc::new(RefCell::new(items)))
            }
            Value::Map(items) => {
                let items = items.borrow();
                let mut out = FxHashMap::default();
                for (key, item) in items.iter() {
                    let MapKey::Str(text) = key else {
                        return Err(RunnerError::internal(span, "map key is not textus valor"));
                    };
                    out.insert(
                        MapKey::Str(text.clone()),
                        self.materialize_json_value(item.clone(), span)?,
                    );
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
            Value::Record(fields, _) => {
                let fields = fields.borrow();
                let mut out = FxHashMap::default();
                for (field, item) in fields.iter() {
                    out.insert(
                        MapKey::Str(self.interner.resolve(*field).to_owned()),
                        self.materialize_json_value(item.clone(), span)?,
                    );
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
            Value::Struct(def_id, fields) => {
                let fields = fields.borrow();
                let mut out = FxHashMap::default();
                for (field, item) in fields.iter() {
                    out.insert(
                        MapKey::Str(self.struct_wire_field_key(def_id, *field)),
                        self.materialize_json_value(item.clone(), span)?,
                    );
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
            // WHY: an `instans` field of a `@ json` genus boxes as its RFC3339
            // textus (parity with Rust codegen `.to_rfc3339()`); the JSON
            // carrier must stay JSON-native so Faber-side serializers see a
            // textus node, not an instans the wire encoder cannot spell.
            Value::Instans(value) => Value::Str(value.to_rfc3339()),
            other => other,
        })
    }

    #[allow(clippy::self_only_used_in_recursion)]
    fn json_node_is_safe(&mut self, value: &Value, span: Span) -> Result<bool, RunnerError> {
        match value {
            Value::Nil
            | Value::Unit
            | Value::Bool(_)
            | Value::Int(_)
            | Value::UInt(_)
            | Value::Magnus(_)
            | Value::Dec(..)
            | Value::Str(_)
            | Value::Ascii(_)
            | Value::Char(_)
            | Value::Instans(_)
            | Value::Octeti(_) => Ok(true),
            Value::Float(n) => Ok(n.is_finite()),
            Value::Array(items) => items.borrow().iter().try_fold(true, |acc, item| {
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Tensor(tensor) => {
                if tensor.borrow().has_unresolved_edge_shift() {
                    return Err(RunnerError::internal(
                        span,
                        "unresolved shifted tensor cannot be converted to json",
                    ));
                }
                let values = self.tensor_logical_values(tensor, span)?;
                values.iter().try_fold(true, |acc, item| {
                    Ok(acc && self.json_node_is_safe(item, span)?)
                })
            }
            Value::Map(items) => items.borrow().iter().try_fold(true, |acc, (key, item)| {
                if !matches!(key, MapKey::Str(_)) {
                    return Err(RunnerError::internal(span, "map key is not textus valor"));
                }
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Record(fields, _) => fields.borrow().iter().try_fold(true, |acc, (_, item)| {
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Struct(_, fields) => fields.borrow().iter().try_fold(true, |acc, (_, item)| {
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Function(_)
            | Value::Closure(_, _, _)
            | Value::ClosureEnvironment(_, _)
            | Value::SharedCell(_)
            | Value::Regex(_)
            | Value::Match(_)
            | Value::Sermo(_)
            | Value::Set(_)
            | Value::Ledger(_)
            | Value::EnumVariant(_, _)
            | Value::Tuple(_)
            | Value::Vector(_)
            | Value::Matrix(_)
            | Value::GradientHandle(_)
            | Value::Cursor(_) => Err(RunnerError::internal(
                span,
                "value variant is outside kernel valor bridge",
            )),
        }
    }

    /// Render `value` as JSON wire text, mirroring the prior
    /// `value_to_runtime_valor` (with `instans_as_textus = true`) ->
    /// `faber::Json::to_wire` composition. Maps, records, and structs emit keys
    /// in sorted order (matching the prior `BTreeMap` carrier). Assumes
    /// [`check_json_document`](Self::check_json_document) already accepted the
    /// value.
    fn value_to_json_wire(&mut self, value: &Value, span: Span) -> Result<String, RunnerError> {
        Ok(match value {
            Value::Nil | Value::Unit => "null".to_owned(),
            Value::Bool(v) => if *v { "true" } else { "false" }.to_owned(),
            Value::Int(v) => v.to_string(),
            Value::UInt(v) => v.to_string(),
            // An `inf` renders as its exact digits, a JSON number (ruling 40).
            Value::Magnus(v) => v.to_string(),
            Value::Dec(carrier, scale) => crate::decimal::format(*carrier, *scale),
            Value::Float(v) => render_json_fractus(*v),
            Value::Str(v) => render_json_string(v),
            Value::Ascii(b) => {
                let ch = char::from(*b);
                let mut buf = [0u8; 4];
                render_json_string(ch.encode_utf8(&mut buf))
            }
            Value::Char(ch) => {
                let mut buf = [0u8; 4];
                render_json_string(ch.encode_utf8(&mut buf))
            }
            Value::Instans(v) => render_json_string(&v.to_rfc3339()),
            Value::Octeti(bytes) => {
                let body = bytes
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                format!("[{body}]")
            }
            Value::Array(items) => {
                let body = items
                    .borrow()
                    .iter()
                    .map(|item| self.value_to_json_wire(item, span))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",");
                format!("[{body}]")
            }
            Value::Tensor(tensor) => {
                if tensor.borrow().has_unresolved_edge_shift() {
                    return Err(RunnerError::internal(
                        span,
                        "unresolved shifted tensor cannot be converted to json",
                    ));
                }
                let values = self.tensor_logical_values(tensor, span)?;
                let body = values
                    .iter()
                    .map(|item| self.value_to_json_wire(item, span))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",");
                format!("[{body}]")
            }
            Value::Map(items) => {
                let items = items.borrow();
                let mut entries: Vec<(String, String)> = Vec::with_capacity(items.len());
                for (key, item) in items.iter() {
                    let MapKey::Str(text) = key else {
                        return Err(RunnerError::internal(span, "map key is not textus valor"));
                    };
                    entries.push((text.clone(), self.value_to_json_wire(item, span)?));
                }
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let body = entries
                    .iter()
                    .map(|(key, value)| format!("{}:{}", render_json_string(key), value))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{body}}}")
            }
            Value::Record(fields, _) => {
                let fields = fields.borrow();
                let mut entries: Vec<(String, String)> = Vec::with_capacity(fields.len());
                for (field, item) in fields.iter() {
                    let key = self.interner.resolve(*field).to_owned();
                    entries.push((key, self.value_to_json_wire(item, span)?));
                }
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let body = entries
                    .iter()
                    .map(|(key, value)| format!("{}:{}", render_json_string(key), value))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{body}}}")
            }
            Value::Struct(def_id, fields) => {
                let fields = fields.borrow();
                let mut entries: Vec<(String, String)> = Vec::with_capacity(fields.len());
                for (field, item) in fields.iter() {
                    let key = self.struct_wire_field_key(*def_id, *field);
                    entries.push((key, self.value_to_json_wire(item, span)?));
                }
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let body = entries
                    .iter()
                    .map(|(key, value)| format!("{}:{}", render_json_string(key), value))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{body}}}")
            }
            // check_json_document rejects these before rendering; defensive guard.
            Value::Function(_)
            | Value::Closure(_, _, _)
            | Value::ClosureEnvironment(_, _)
            | Value::SharedCell(_)
            | Value::Regex(_)
            | Value::Match(_)
            | Value::Sermo(_)
            | Value::Set(_)
            | Value::Ledger(_)
            | Value::EnumVariant(_, _)
            | Value::Tuple(_)
            | Value::Vector(_)
            | Value::Matrix(_)
            | Value::GradientHandle(_)
            | Value::Cursor(_) => {
                return Err(RunnerError::internal(
                    span,
                    "value variant is outside kernel valor bridge",
                ));
            }
        })
    }

    pub(crate) fn require_json_value(
        &mut self,
        value: &Value,
        span: Span,
    ) -> Result<(), RunnerError> {
        let value = self.prepare_tensor_escape(value.clone(), span)?;
        if self.check_json_document(&value, span)? {
            Ok(())
        } else {
            Err(RunnerError::conversion(
                span,
                "valor to json conversion failed",
            ))
        }
    }

    fn materialize_sermo_textus(
        &mut self,
        sermo: Rc<RefCell<SermoValue>>,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // Every textus content frame, concatenated (Rust
        // `sermo_materialize_textus` parity).
        let mut text = String::new();
        for item in self.sermo_items(&sermo, span)? {
            let Value::Str(part) = item else {
                return self.recover_or_fail(recovery, span, "sermo to textus conversion failed");
            };
            text.push_str(&part);
        }
        Ok(Value::Str(text))
    }

    pub(crate) fn convert_to_ascii_format_pack(
        &mut self,
        value: Value,
        capacity: radix_types::IndexId,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Some(radix) = self.numerus_radix(params) else {
            return Err(RunnerError::internal(
                span,
                "ascii format pack requires Hex, Bin, or Oct",
            ));
        };
        let IndexExpr::Literal(width) = self.types.get_index(capacity) else {
            return Err(RunnerError::internal(
                span,
                "ascii format pack requires a literal capacity",
            ));
        };
        let Ok(width) = usize::try_from(*width) else {
            return self.recover_or_fail(recovery, span, "ascii_pack_width_overflow");
        };
        let magnitude = match value {
            Value::Int(v) if v < 0 => {
                return self.recover_or_fail(recovery, span, "ascii_pack_negative_source");
            }
            Value::Int(v) => u128::try_from(v)
                .map_err(|_| RunnerError::internal(span, "ascii_pack_negative_source"))?,
            Value::UInt(v) => u128::from(v),
            // `inf ↦ ascii<N> via Hex/Bin/Oct` (ruling 40): the same fits-N
            // check over the exact digits of any size.
            Value::Magnus(big) => {
                if big.is_negative() {
                    return self.recover_or_fail(recovery, span, "ascii_pack_negative_source");
                }
                let digits = big.to_string_radix(match radix {
                    16 => handroll::Radix::Hex,
                    2 => handroll::Radix::Bin,
                    _ => handroll::Radix::Oct,
                });
                return if digits.len() > width {
                    self.recover_or_fail(recovery, span, "ascii_pack_width_overflow")
                } else if width == 1 {
                    Ok(Value::Ascii(digits.as_bytes()[0]))
                } else {
                    Ok(Value::Str(format!("{digits:0>width$}")))
                };
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("ascii format pack requires a numerus source, got {other:?}"),
                ));
            }
        };
        match pack_ascii_digits(magnitude, width, radix) {
            Some(digits) if width == 1 => {
                let byte = digits.as_bytes()[0];
                Ok(Value::Ascii(byte))
            }
            Some(digits) => Ok(Value::Str(digits)),
            None => self.recover_or_fail(recovery, span, "ascii_pack_width_overflow"),
        }
    }

    /// `textus ↦ textus<N>`: the store-time width check. The value stays a
    /// text; a source of more than `N` Unicode scalars fails the conversion
    /// with the Rust carrier's text (`TextusN::try_from_textus`), and `⊥`
    /// recovery supplies the fallback.
    pub(crate) fn convert_to_bounded_textus(
        &mut self,
        value: Value,
        capacity: radix_types::IndexId,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let IndexExpr::Literal(capacity) = self.types.get_index(capacity) else {
            return Err(RunnerError::internal(
                span,
                "bounded textus requires a literal capacity",
            ));
        };
        let capacity = usize::try_from(*capacity).unwrap_or(usize::MAX);
        match value {
            Value::Str(text) => {
                if text.chars().count() > capacity {
                    self.recover_or_fail(
                        recovery,
                        span,
                        "textus to bounded textus conversion failed",
                    )
                } else {
                    Ok(Value::Str(text))
                }
            }
            other => Err(RunnerError::internal(
                span,
                format!("bounded textus conversio requires a textus source, got {other:?}"),
            )),
        }
    }

    /// `ascii<N> ↦ ascii<M>`: the D7 resize. The initialized prefix is the
    /// text itself, so grow and identity copies keep it and a shrink keeps it
    /// when it fits `M`; a source of more than `M` characters fails the
    /// conversion, and `⊥` recovery supplies the fallback.
    pub(crate) fn convert_ascii_to_bounded_ascii(
        &mut self,
        value: Value,
        capacity: radix_types::IndexId,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let IndexExpr::Literal(capacity) = self.types.get_index(capacity) else {
            return Err(RunnerError::internal(
                span,
                "bounded ascii requires a literal capacity",
            ));
        };
        let capacity = usize::try_from(*capacity).unwrap_or(usize::MAX);
        match value {
            Value::Str(text) => {
                if text.chars().count() > capacity {
                    self.recover_or_fail(recovery, span, "ascii to bounded ascii conversion failed")
                } else {
                    Ok(Value::Str(text))
                }
            }
            other => Err(RunnerError::internal(
                span,
                format!("bounded ascii resize requires a text source, got {other:?}"),
            )),
        }
    }

    /// `octeti ↦ octeti<N>`: the store-time capacity check. The value stays a
    /// byte vector; a source longer than `N` fails the conversion with the
    /// Rust carrier's text (`OctetiN::try_from_octeti`), and `⊥` recovery
    /// supplies the fallback. A `lista<b8>` source gathers its byte patterns,
    /// as the bulk byte rows do. (`appende` on a bounded buffer enforces the
    /// same bound at the runtime push; this is the store-time check only.)
    pub(crate) fn convert_to_bounded_octeti(
        &mut self,
        value: Value,
        capacity: radix_types::IndexId,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let IndexExpr::Literal(capacity) = self.types.get_index(capacity) else {
            return Err(RunnerError::internal(
                span,
                "bounded octeti requires a literal capacity",
            ));
        };
        let capacity = usize::try_from(*capacity).unwrap_or(usize::MAX);
        let bytes = match value {
            Value::Octeti(bytes) => bytes,
            Value::Array(items) => bytes_of_list(&items.borrow(), span)?,
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("bounded octeti conversio requires an octeti source, got {other:?}"),
                ));
            }
        };
        if bytes.len() > capacity {
            self.recover_or_fail(recovery, span, "octeti to bounded octeti conversion failed")
        } else {
            Ok(Value::Octeti(bytes))
        }
    }

    pub(crate) fn convert_to_ascii(
        &mut self,
        value: Value,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Ascii(b) => Ok(Value::Ascii(b)),
            // D10.6 `n ↦ ascii`: the digits, which are always ASCII.
            Value::Int(n) => Ok(Value::Str(n.to_string())),
            Value::UInt(n) => Ok(Value::Str(n.to_string())),
            Value::Magnus(n) => Ok(Value::Str(n.to_string())),
            Value::Str(text) => {
                if text.is_ascii() {
                    Ok(Value::Str(text))
                } else {
                    self.recover_or_fail(recovery, span, "textus to ascii conversion failed")
                }
            }
            Value::Octeti(bytes) => {
                if bytes.is_ascii() {
                    String::from_utf8(bytes)
                        .map(Value::Str)
                        .map_err(|_| RunnerError::internal(span, "ascii bytes failed UTF-8 decode"))
                } else {
                    self.recover_or_fail(recovery, span, "octeti to ascii conversion failed")
                }
            }
            _ => Err(RunnerError::internal(
                span,
                "ascii conversio requires textus or octeti source",
            )),
        }
    }

    /// `textus ↦ regex`: validate and compile the pattern once, here. A pattern
    /// that fails to compile fails the conversion with the construct-id payload
    /// of RD-4 (`regex_program`).
    pub(crate) fn convert_to_regex(
        &mut self,
        value: Value,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Regex(program) => Ok(Value::Regex(program)),
            Value::Str(pattern) => match RegexProgram::compile(&pattern) {
                Ok(program) => Ok(Value::Regex(Rc::new(program))),
                Err(payload) => self.recover_or_fail(recovery, span, &payload),
            },
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to regex from {other:?}"),
            )),
        }
    }

    pub(crate) fn convert_to_instans(
        &mut self,
        value: Value,
        target: &Type,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let precision = instans_precision(target);
        match value {
            Value::Sermo(sermo) => {
                let value = self.sermo_first_item(&sermo, span)?;
                self.convert_to_instans(value, target, recovery, span)
            }
            Value::Instans(value) => Ok(Value::Instans(value.ad_praecisionem(precision))),
            // String sources parse as RFC3339 wire text; recovery honors `⊥`.
            Value::Str(text) => match Instans::from_rfc3339(&text, precision) {
                Some(value) => Ok(Value::Instans(value)),
                None => self.recover_or_fail(recovery, span, "textus to instans conversion failed"),
            },
            // Integer sources are interpreted as epoch units at the target precision.
            Value::Int(epoch) => Ok(Value::Instans(Instans::from_epoch_integer(
                epoch, precision,
            ))),
            Value::UInt(epoch) => match i64::try_from(epoch) {
                Ok(epoch) => Ok(Value::Instans(Instans::from_epoch_integer(
                    epoch, precision,
                ))),
                Err(_) => self.recover_or_fail(
                    recovery,
                    span,
                    "numerus to instans conversion out of range",
                ),
            },
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to instans from {other:?}"),
            )),
        }
    }
}

#[derive(Clone, Copy)]
enum DecimalTextError {
    Invalid,
    OutOfRange,
}

fn decimal_source_width(types: &TypeTable, ty: TypeId) -> Option<NumericWidth> {
    effective_width(types, ty).filter(|width| width.is_decimal())
}

fn decimal_text_error(error: DecimalTextError, span: Span) -> RunnerError {
    // WHY: parse/width failures are recoverable conversion failures (SA-002).
    match error {
        DecimalTextError::Invalid => {
            RunnerError::conversion(span, "textus to numerus conversion failed")
        }
        DecimalTextError::OutOfRange => {
            RunnerError::conversion(span, "textus to numerus conversion out of range")
        }
    }
}

fn decimal_carrier_value(value: i64, width: NumericWidth, span: Span) -> Result<i64, RunnerError> {
    decimal_carrier_from_raw(i128::from(value), width)
        .map_err(|_| RunnerError::internal(span, "decimal carrier is outside declared width"))
}

fn decimal_carrier_from_raw(value: i128, width: NumericWidth) -> Result<i64, DecimalTextError> {
    let (lo, hi) = int_width_range(width);
    if !(lo..=hi).contains(&value) {
        return Err(DecimalTextError::OutOfRange);
    }
    i64::try_from(value).map_err(|_| DecimalTextError::OutOfRange)
}

fn decimal_integer_to_carrier(value: i128, width: NumericWidth) -> Result<i64, DecimalTextError> {
    let scale = decimal_scale_factor(width).ok_or(DecimalTextError::OutOfRange)?;
    let value = value
        .checked_mul(scale)
        .ok_or(DecimalTextError::OutOfRange)?;
    decimal_carrier_from_raw(value, width)
}

fn decimal_text_to_carrier(text: &str, width: NumericWidth) -> Result<i64, DecimalTextError> {
    // S07-12: the carrier math lives only in `radix_codegen_shared::decimal`.
    // The shared tier reports every failure as `None`; the runner's boundary
    // keeps its two recoverable messages distinct (SA-002) — malformed digit
    // text vs a carrier the width cannot hold — by asking the shared parse
    // oracle (`radix_types`) whether the text was well formed, never by
    // re-computing the carrier.
    if let Some(carrier) = radix_codegen_shared::decimal::decimal_text_to_carrier(text, width) {
        return Ok(carrier);
    }
    let digits = match text.strip_prefix('-') {
        Some(digits) => digits,
        None => text.strip_prefix('+').unwrap_or(text),
    };
    if radix_types::numeric_width::decimal_text_to_scaled(digits).is_some() {
        Err(DecimalTextError::OutOfRange)
    } else {
        Err(DecimalTextError::Invalid)
    }
}

/// The carrier of an `f64` source at a decimal width, parsed through the
/// shared decimal oracle and mapped to the recoverable conversion error.
fn decimal_float_carrier(value: f64, width: NumericWidth, span: Span) -> Result<i64, RunnerError> {
    decimal_text_to_carrier(&value.to_string(), width)
        .map_err(|error| decimal_text_error(error, span))
}

/// Render a stored decimal carrier: `d64` is the only decimal width, so its
/// scale is 8 and the stored-carrier renderer applies unchanged.
fn format_decimal_carrier(value: i64, width: NumericWidth) -> String {
    crate::decimal::format(i128::from(value), width.decimal_scale().unwrap_or(0))
}

/// `true` when `v` fits the signed range of `width`. Mirrors Rust codegen's
/// `.parse::<W>()` accept/reject boundary for `textus ↦ numerus<W>` (SA-002);
/// uses the runner's existing target-neutral integer width table.
fn numerus_fits_width(v: i64, width: NumericWidth) -> bool {
    exact_fits_width(i128::from(v), width)
}

/// Whether an exact integer lies in the declared width's range.
fn exact_fits_width(v: i128, width: NumericWidth) -> bool {
    let (lo, hi) = int_width_range(width);
    (lo..=hi).contains(&v)
}

/// The carrier of an exact integer already range-checked against its width:
/// `Int` while it fits `i64`, `UInt` for the upper half of `u64`.
fn exact_numerus_value(v: i128) -> Value {
    match i64::try_from(v) {
        Ok(v) => Value::Int(v),
        Err(_) => Value::UInt(u64::try_from(v).unwrap_or(u64::MAX)),
    }
}

/// Render an f64 as JSON number wire text, ensuring a decimal point
/// (`5` -> `5.0`). Mirrors `faber_runtime::json::render_fractus` byte-for-byte.
fn render_json_fractus(value: f64) -> String {
    let rendered = value.to_string();
    if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
        rendered
    } else {
        format!("{rendered}.0")
    }
}

/// Render a string as a JSON string literal (quoted, with JSON escapes).
/// Mirrors `faber_runtime::json::render_string` byte-for-byte.
fn render_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
