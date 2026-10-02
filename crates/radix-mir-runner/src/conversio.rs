#![allow(clippy::map_unwrap_or)]

//! Target-directed `↦` conversion for the MIR runner.
//!
//! WHY: MIR validation proves `MirConversion.target_ty`; guessing from the
//! source `Value` shape disagrees with typed contracts (e.g. `textus ↦ bivalens`).

use super::regex_program::RegexProgram;
use super::value::{
    MapKey, SermoValue, TensorValue, Value, decimal_scale_factor, float_to_numerus_width,
    float_truncation_fits_width, int_width_range, modular_word_result, round_half_even_division,
    saturating_word_result,
};
use super::{Runner, RunnerError};
use radix_lexer::{Span, Symbol};
use radix_mir::lista_tensor_conversio::{ListaToTensorShape, resolve_lista_to_tensor_shape};
use radix_mir::{MirConversion, MirNamedOperand, MirOperand, MirStructFieldDefaults};
use radix_runtime_contract::instans::{Instans, InstansPraecisio};
use radix_types::{
    InstansPrecision, NumericWidth, Primitive, Type, TypeId, TypeTable, effective_width,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::rc::Rc;

mod aggregate;
#[cfg(test)]
mod bits_f16_test;
mod inf;
mod littera;
#[cfg(test)]
mod littera_test;
mod scalar;

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
                self.convert_to_modular_word(value, source_ty, *width, span)
            }
            Type::SaturatingWord(width) => {
                self.convert_to_saturating_word(value, source_ty, *width, span)
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
            Type::Cursor(_) | Type::Tensor(_, _) | Type::Sparsa(_, _) => {
                self.convert_to_eager_sequence(value, target_ty, struct_defaults, span)
            }
            Type::Struct(def_id) => {
                self.convert_to_struct(value, source_ty, *def_id, recovery, struct_defaults, span)
            }
            // N7E-1: the policy-float cell runs only once N7E-3 lands; fail
            // closed with its own feature name rather than the generic arm.
            Type::PolicyFloat(_, _) => Err(RunnerError::unsupported(
                span,
                "runtime conversio to a policy-float cell",
            )),
            other => Err(RunnerError::unsupported(
                span,
                &format!("runtime conversio to {other:?}"),
            )),
        }
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
