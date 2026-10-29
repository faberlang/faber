//! `octeti ↦ tensor<E, [shape]> via Le|Be`: faithful bulk reinterpretation,
//! and its inverse `tensor<E, [shape]> ↦ octeti via Le|Be`.
//!
//! WHY: loading a weight tensor element by element through the interpreter
//! costs microseconds per element. This row maps the byte string onto a typed
//! tensor in one pass: pure bit reinterpretation (no numeric conversion, so
//! NaN payloads, `-0.0` and subnormals survive exactly), element `i` at bytes
//! `[i*w, (i+1)*w)`, row-major, byte order from the `via` hint, and a byte
//! length of EXACTLY `product(shape) * w`.
//!
//! Elements with a dense kind (f32 f64 i32 i64 u8 u32 u64) land in typed
//! storage with no `Value` boxing. `i8`, `i16` and `u16` have no dense kind
//! and decode natively into boxed `Values` (correct, slower).
//!
//! A fixed-layout genus element keeps the bytes as they are:
//! the input `Vec<u8>` becomes the tensor's `DenseStorage::Record` rows with no
//! per-element decode and no copy. A row reads back as a struct only when an
//! element is read. The byte image is little-endian by the layout law; a
//! wrong byte count is a named, recoverable conversion failure: overflow,
//! misalignment, truncation or trailing bytes.

use crate::dense::{DenseKind, DenseStorage, RecordLayout, dense_kind_of_type};
use crate::value::{TensorValue, Value};
use crate::{Host, Runner, RunnerError};
use radix_codegen_shared::hints::{self, EndianHint};
use radix_lexer::{Span, Symbol};
use radix_mir::MirOperand;
use radix_mir::lista_tensor_conversio::literal_tensor_dims;
use radix_runtime_contract::trap::text as trap_text;
use radix_types::{NumericWidth, Type, TypeId, effective_width};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Stable issue for a target with run-time extents that arrived without its
/// shape operand: lowering always supplies one, so this is an invariant break.
const SHAPE_OPERAND_MISSING_ISSUE: &str = "list_b8_to_tensor_shape_operand_missing";

/// Stable issues of the record-element row. The first four are recoverable
/// conversion failures (`⊥`, `fac/cape`, `⇥`); the last two are invariant
/// breaks the checker and the layout law make unreachable from checked source.
pub(crate) const RECORD_OVERFLOW_ISSUE: &str = "runner_list_b8_to_tensor_record_overflow";
pub(crate) const RECORD_MISALIGNED_ISSUE: &str = "runner_list_b8_to_tensor_record_misaligned";
pub(crate) const RECORD_TRUNCATED_ISSUE: &str = "runner_list_b8_to_tensor_record_truncated";
pub(crate) const RECORD_TRAILING_ISSUE: &str = "runner_list_b8_to_tensor_record_trailing_bytes";
const RECORD_LAYOUT_REFUSED_ISSUE: &str = "list_b8_to_tensor_record_layout_refused";
const RECORD_BIG_ENDIAN_ISSUE: &str = "list_b8_to_tensor_record_big_endian";

/// Elements that decode into boxed `Values` (no `DenseKind`).
#[derive(Clone, Copy)]
enum BoxedElement {
    I8,
    I16,
    U16,
}

impl BoxedElement {
    const fn width(self) -> usize {
        match self {
            Self::I8 => 1,
            Self::I16 | Self::U16 => 2,
        }
    }
}

enum Element {
    Dense(DenseKind),
    Boxed(BoxedElement),
}

impl Element {
    const fn width(&self) -> usize {
        match self {
            Self::Dense(kind) => kind.width(),
            Self::Boxed(element) => element.width(),
        }
    }
}

/// One bulk pass over `$bytes` into a `Vec<$ty>`, byte order chosen once
/// outside the loop so the inner loop stays branch-free.
macro_rules! decode_vec {
    ($ty:ty, $bytes:expr, $endian:expr) => {{
        const WIDTH: usize = std::mem::size_of::<$ty>();
        let chunks = $bytes.as_chunks::<WIDTH>().0;
        match $endian {
            EndianHint::Le => chunks
                .iter()
                .map(|chunk| <$ty>::from_le_bytes(*chunk))
                .collect::<Vec<$ty>>(),
            EndianHint::Be => chunks
                .iter()
                .map(|chunk| <$ty>::from_be_bytes(*chunk))
                .collect::<Vec<$ty>>(),
        }
    }};
}

impl<H: Host + ?Sized> Runner<'_, H> {
    /// Convert `Value::Octeti(bytes)` into the tensor `target_ty` names.
    ///
    /// `shape` is the evaluated shape operand of a target with run-time
    /// extents; a literal target reads its dims from the type instead.
    ///
    /// A wrong byte length is the recoverable `runner_conversion` failure with
    /// issue `runner_list_b8_to_tensor_length_mismatch` (`⊥`, `fac/cape`, `⇥`
    /// route through [`Runner::recover_or_fail`] exactly like the scalar row).
    pub(crate) fn convert_octeti_to_tensor(
        &mut self,
        bytes: Vec<u8>,
        target_ty: TypeId,
        shape: Option<Vec<i64>>,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Type::Tensor(element_ty, _) = self.types.get(target_ty) else {
            return Err(RunnerError::internal(
                span,
                "list<b8> tensor conversio target is not a tensor",
            ));
        };
        let element_ty = *element_ty;
        let Some(shape) = literal_tensor_dims(target_ty, self.types).or(shape) else {
            return Err(RunnerError::internal_with_issue(
                span,
                "list<b8> tensor conversio needs a shape operand for run-time extents",
                SHAPE_OPERAND_MISSING_ISSUE,
            ));
        };
        if let Type::Struct(_) = self.types.get(self.resolve_conversion_target(element_ty)) {
            return self
                .convert_octeti_to_record_tensor(bytes, element_ty, shape, params, recovery, span);
        }
        let Some(element) = self.octeti_tensor_element(element_ty) else {
            return Err(RunnerError::internal(
                span,
                "list<b8> tensor element unsupported",
            ));
        };
        // An 8-bit element has no byte order (ruling B6): the checker admits
        // no clause on it, so the order is immaterial and `Le` stands in.
        let endian = hints::endian_hint(params, |param| {
            hints::classify(self.interner.resolve(*param))
        });
        let Some(endian) = endian.or((element.width() == 1).then_some(EndianHint::Le)) else {
            return Err(RunnerError::internal(
                span,
                "list<b8> tensor conversio needs a Be or Le hint",
            ));
        };
        let expected = shape
            .iter()
            .try_fold(element.width(), |acc, dim| {
                usize::try_from(*dim).ok().and_then(|d| acc.checked_mul(d))
            })
            .filter(|expected| *expected == bytes.len());
        if expected.is_none() {
            return self.recover_or_fail(recovery, span, trap_text::BYTES_TENSOR_LENGTH_MISMATCH);
        }
        let storage = match element {
            Element::Dense(kind) => decode_dense(kind, bytes, endian),
            Element::Boxed(element) => decode_boxed(element, &bytes, endian),
        };
        let tensor = TensorValue::try_contiguous_typed(storage, shape).ok_or_else(|| {
            RunnerError::internal(span, "tensor shape has unrepresentable strides")
        })?;
        Ok(Value::Tensor(Rc::new(RefCell::new(tensor))))
    }

    /// `octeti ↦ tensor<Record, [shape]> via Le`: the bytes become the tensor's
    /// record rows as they are. The row width is `resolve_struct_layout`'s
    /// size, so a layout the law refuses is an invariant break here (the
    /// device gate and the dispatcher refuse the same genus by name).
    fn convert_octeti_to_record_tensor(
        &mut self,
        bytes: Vec<u8>,
        element_ty: TypeId,
        shape: Vec<i64>,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let endian = hints::endian_hint(params, |param| {
            hints::classify(self.interner.resolve(*param))
        });
        if endian != Some(EndianHint::Le) {
            return Err(RunnerError::internal_with_issue(
                span,
                "record tensor conversio needs a Le hint (record layouts are little-endian)",
                RECORD_BIG_ENDIAN_ISSUE,
            ));
        }
        let layout = RecordLayout::resolve(element_ty, self.context).map_err(|refusal| {
            RunnerError::internal_with_issue(
                span,
                format!(
                    "record tensor element has no fixed layout: {}",
                    refusal.reason()
                ),
                RECORD_LAYOUT_REFUSED_ISSUE,
            )
        })?;
        let size = layout.size();
        let rows = shape.iter().try_fold(1_usize, |acc, dim| {
            usize::try_from(*dim).ok().and_then(|d| acc.checked_mul(d))
        });
        let expected = rows.and_then(|rows| rows.checked_mul(size));
        let failure = match expected {
            None => Some((RECORD_OVERFLOW_ISSUE, "record tensor extent overflows")),
            Some(_) if !bytes.len().is_multiple_of(size) => Some((
                RECORD_MISALIGNED_ISSUE,
                "record tensor bytes are not a whole number of rows",
            )),
            Some(expected) if bytes.len() < expected => Some((
                RECORD_TRUNCATED_ISSUE,
                "record tensor bytes are shorter than the shape",
            )),
            Some(expected) if bytes.len() > expected => Some((
                RECORD_TRAILING_ISSUE,
                "record tensor bytes are longer than the shape",
            )),
            Some(_) => None,
        };
        if let Some((issue, message)) = failure {
            if let Some(recovery) = recovery {
                return self.eval_recovery(recovery, span);
            }
            let mut error = RunnerError::conversion_with_issue(span, message, issue);
            error.args = vec![
                ("record_bytes".to_owned(), size.to_string()),
                ("actual".to_owned(), bytes.len().to_string()),
                (
                    "expected".to_owned(),
                    expected.map_or_else(|| "overflow".to_owned(), |n| n.to_string()),
                ),
            ];
            return Err(error);
        }
        let storage = DenseStorage::from_record_bytes(Arc::new(layout), bytes)
            .ok_or_else(|| RunnerError::internal(span, "record tensor bytes are misaligned"))?;
        let tensor = TensorValue::try_contiguous_typed(storage, shape).ok_or_else(|| {
            RunnerError::internal(span, "tensor shape has unrepresentable strides")
        })?;
        Ok(Value::Tensor(Rc::new(RefCell::new(tensor))))
    }

    /// `tensor<E, [shape]> ↦ octeti via Le|Be`: row-major, element `i` at
    /// bytes `[i*w, (i+1)*w)` in the hinted byte order, pure bit
    /// reinterpretation. It cannot fail on length.
    pub(crate) fn convert_tensor_to_octeti(
        &self,
        tensor: &TensorValue,
        source_ty: TypeId,
        params: &[Symbol],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let mut source_ty = source_ty;
        while let Type::Alias(_, inner) = self.types.get(source_ty) {
            source_ty = *inner;
        }
        let Type::Tensor(element_ty, _) = self.types.get(source_ty) else {
            return Err(RunnerError::internal(
                span,
                "tensor list<b8> conversio source is not a tensor",
            ));
        };
        let Some(endian) = hints::endian_hint(params, |param| {
            hints::classify(self.interner.resolve(*param))
        }) else {
            return Err(RunnerError::internal(
                span,
                "tensor list<b8> conversio needs a Be or Le hint",
            ));
        };
        let Some(element) = self.octeti_tensor_element(*element_ty) else {
            return Err(RunnerError::internal(
                span,
                "tensor list<b8> element unsupported",
            ));
        };
        let width = element.width();
        let float = match effective_width(self.types, *element_ty) {
            Some(NumericWidth::F32) => Some(true),
            Some(NumericWidth::F64) => Some(false),
            _ => None,
        };
        let Some(values) = tensor.logical_values() else {
            return Err(RunnerError::internal(
                span,
                "tensor list<b8> conversio could not read the tensor elements",
            ));
        };
        let mut bytes = Vec::with_capacity(values.len() * width);
        for value in &values {
            let bits: u64 = match (value, float) {
                (Value::Float(v), Some(true)) => u64::from((*v as f32).to_bits()),
                (Value::Float(v), Some(false)) => v.to_bits(),
                (Value::Int(v), None) => u64::from_le_bytes(v.to_le_bytes()),
                (Value::UInt(v), None) => *v,
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "tensor list<b8> conversio met a non-numeric element",
                    ));
                }
            };
            match endian {
                EndianHint::Le => bytes.extend_from_slice(&bits.to_le_bytes()[..width]),
                EndianHint::Be => bytes.extend_from_slice(&bits.to_be_bytes()[8 - width..]),
            }
        }
        Ok(Value::Octeti(bytes))
    }

    fn octeti_tensor_element(&self, element_ty: TypeId) -> Option<Element> {
        match dense_kind_of_type(self.types, element_ty) {
            Some(DenseKind::Bool) => None,
            Some(kind) => Some(Element::Dense(kind)),
            None => match effective_width(self.types, element_ty)? {
                NumericWidth::I8 => Some(Element::Boxed(BoxedElement::I8)),
                NumericWidth::I16 => Some(Element::Boxed(BoxedElement::I16)),
                NumericWidth::U16 => Some(Element::Boxed(BoxedElement::U16)),
                _ => None,
            },
        }
    }
}

/// `bytes.len()` is already proven `n * kind.width()`; the input is consumed
/// (a `u8` tensor takes the allocation itself).
fn decode_dense(kind: DenseKind, bytes: Vec<u8>, endian: EndianHint) -> DenseStorage {
    match kind {
        DenseKind::U8 => DenseStorage::U8(bytes),
        // Never reached: `octeti_tensor_element` refuses a bool element.
        DenseKind::Bool => DenseStorage::Bool(bytes.into_iter().map(|byte| byte != 0).collect()),
        DenseKind::F32 => DenseStorage::F32(decode_vec!(f32, bytes, endian)),
        DenseKind::F64 => DenseStorage::F64(decode_vec!(f64, bytes, endian)),
        DenseKind::I32 => DenseStorage::I32(decode_vec!(i32, bytes, endian)),
        DenseKind::I64 => DenseStorage::I64(decode_vec!(i64, bytes, endian)),
        DenseKind::U32 => DenseStorage::U32(decode_vec!(u32, bytes, endian)),
        DenseKind::U64 => DenseStorage::U64(decode_vec!(u64, bytes, endian)),
    }
}

fn decode_boxed(element: BoxedElement, bytes: &[u8], endian: EndianHint) -> DenseStorage {
    let values = match element {
        BoxedElement::I8 => bytes
            .iter()
            .map(|byte| Value::Int(i64::from(i8::from_ne_bytes([*byte]))))
            .collect(),
        BoxedElement::I16 => decode_vec!(i16, bytes, endian)
            .into_iter()
            .map(|v| Value::Int(i64::from(v)))
            .collect(),
        BoxedElement::U16 => decode_vec!(u16, bytes, endian)
            .into_iter()
            .map(|v| Value::Int(i64::from(v)))
            .collect(),
    };
    DenseStorage::Values(values)
}
