//! `W ↦ octeti<N> via Le|Be`: a numeric scalar writes its own bytes.
//!
//! WHY: the checker admits the row only when the literal capacity equals the
//! source width (`endian_read_width_bytes`), so the write is infallible: no
//! bound failure and no `⊥` recovery. The encoding is bit-exact — two's
//! complement for integers, IEEE bits for floats — the same per-element
//! encoding the tensor bulk row uses.

use crate::value::Value;
use crate::{Host, Runner, RunnerError};
use radix_codegen_shared::hints::{self, EndianHint};
use radix_lexer::{Span, Symbol};
use radix_types::{IndexExpr, NumericWidth, TypeId, effective_width};

impl<H: Host + ?Sized> Runner<'_, H> {
    /// Encode one numeric scalar as its own width in the hinted byte order.
    /// `I16|U16|I32|U32|I64|U64|F32|F64` are the only admitted widths; a
    /// missing hint or any other width is an invariant break the checker
    /// rejects first.
    pub(crate) fn convert_scalar_to_bounded_octeti(
        &self,
        value: Value,
        source_ty: TypeId,
        capacity: radix_types::IndexId,
        params: &[Symbol],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Some(width) = effective_width(self.types, source_ty) else {
            return Err(RunnerError::internal(
                span,
                "scalar bounded list<b8> conversio source has no width",
            ));
        };
        let Some(width_bytes) = scalar_octeti_width_bytes(width) else {
            return Err(RunnerError::internal(
                span,
                "scalar bounded list<b8> conversio source width is not an endian width",
            ));
        };
        let Some(endian) = hints::endian_hint(params, |param| {
            hints::classify(self.interner.resolve(*param))
        }) else {
            return Err(RunnerError::internal(
                span,
                "scalar bounded list<b8> conversio needs a Be or Le hint",
            ));
        };
        let bits = match value {
            Value::Int(v) => v.cast_unsigned(),
            Value::UInt(v) => v,
            Value::Float(v) => match width {
                NumericWidth::F32 => u64::from((v as f32).to_bits()),
                NumericWidth::F64 => v.to_bits(),
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "scalar bounded list<b8> conversio float width is not F32 or F64",
                    ));
                }
            },
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("scalar bounded list<b8> conversio met a non-numeric value: {other:?}"),
                ));
            }
        };
        let bytes = match endian {
            EndianHint::Le => bits.to_le_bytes()[..width_bytes].to_vec(),
            EndianHint::Be => bits.to_be_bytes()[8 - width_bytes..].to_vec(),
        };
        let IndexExpr::Literal(literal) = self.types.get_index(capacity) else {
            return Err(RunnerError::internal(
                span,
                "bounded list<b8> requires a literal capacity",
            ));
        };
        if usize::try_from(*literal) != Ok(bytes.len()) {
            return Err(RunnerError::internal(
                span,
                "scalar bounded list<b8> conversio length is not the literal capacity",
            ));
        }
        Ok(Value::Octeti(bytes))
    }
}

/// Byte width of a scalar the checker admits for `W ↦ octeti<N> via Le|Be`.
fn scalar_octeti_width_bytes(width: NumericWidth) -> Option<usize> {
    match width {
        NumericWidth::I16 | NumericWidth::U16 => Some(2),
        NumericWidth::I32 | NumericWidth::U32 | NumericWidth::F32 => Some(4),
        NumericWidth::I64 | NumericWidth::U64 | NumericWidth::F64 => Some(8),
        _ => None,
    }
}
