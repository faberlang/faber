//! `lista<bN> ↔ lista<number | bits>` across a width change, with a
//! `via Le|Be` clause (need, B5 and B6).
//!
//! WHY: a bits list at one width is a byte string seen through that width. A
//! width change is decode-then-encode through the unsigned pattern: every
//! source element is written as `width/8` bytes in the hinted byte order, and
//! the byte string is read back in `target-width/8` groups. A total that is
//! not a whole number of target elements is the recoverable
//! `bits list length mismatch` failure.

use super::{bits_pattern, float_from_pattern, pattern_from_float};
use crate::value::Value;
use crate::{Host, Runner, RunnerError};
use radix_codegen_shared::hints::{self, EndianHint};
use radix_lexer::{Span, Symbol};
use radix_mir::MirOperand;
use radix_types::{BitsWidth, NumericWidth, Primitive, Type, TypeId};
use std::cell::RefCell;
use std::rc::Rc;

/// How one list element reads its bit pattern.
#[derive(Clone, Copy)]
enum Cell {
    Raw(u32),
    Unsigned(u32),
    Signed(u32),
    Float(NumericWidth, u32),
}

impl Cell {
    const fn bits(self) -> u32 {
        match self {
            Self::Raw(bits) | Self::Unsigned(bits) | Self::Signed(bits) | Self::Float(_, bits) => {
                bits
            }
        }
    }
}

fn bits_width(bits: u32) -> Option<BitsWidth> {
    match bits {
        8 => Some(BitsWidth::B8),
        16 => Some(BitsWidth::B16),
        32 => Some(BitsWidth::B32),
        64 => Some(BitsWidth::B64),
        _ => None,
    }
}

fn mask(bits: u32) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1_u64 << bits) - 1
    }
}

impl<H: Host + ?Sized> Runner<'_, H> {
    fn bits_list_cell(&self, mut ty: TypeId) -> Option<Cell> {
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = *inner;
        }
        Some(match self.types.get(ty) {
            Type::Bits(width) => Cell::Raw(width.bits()),
            Type::SizedNumeric(Primitive::Numerus, width) => match width {
                NumericWidth::U8 => Cell::Unsigned(8),
                NumericWidth::U16 => Cell::Unsigned(16),
                NumericWidth::U32 => Cell::Unsigned(32),
                NumericWidth::U64 => Cell::Unsigned(64),
                NumericWidth::I8 => Cell::Signed(8),
                NumericWidth::I16 => Cell::Signed(16),
                NumericWidth::I32 => Cell::Signed(32),
                NumericWidth::I64 => Cell::Signed(64),
                _ => return None,
            },
            Type::SizedNumeric(Primitive::Fractus, width) => match width {
                NumericWidth::F16 | NumericWidth::BF16 => Cell::Float(*width, 16),
                NumericWidth::F32 => Cell::Float(*width, 32),
                NumericWidth::F64 => Cell::Float(*width, 64),
                _ => return None,
            },
            _ => return None,
        })
    }

    /// `Some(result)` when `source_ty ↦ target` is a bits-list width change
    /// carrying an endian hint; `None` leaves the ordinary rows in charge.
    pub(super) fn convert_bits_list_width(
        &mut self,
        value: &Value,
        source_ty: TypeId,
        target: &Type,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Option<Result<Value, RunnerError>> {
        let Type::Array(target_elem) = target else {
            return None;
        };
        let Value::Array(cell) = value else {
            return None;
        };
        let mut source_ty = source_ty;
        while let Type::Alias(_, inner) = self.types.get(source_ty) {
            source_ty = *inner;
        }
        let Type::Array(source_elem) = self.types.get(source_ty) else {
            return None;
        };
        let (from, to) = (
            self.bits_list_cell(*source_elem)?,
            self.bits_list_cell(*target_elem)?,
        );
        let raw = |cell: Cell| matches!(cell, Cell::Raw(_));
        if from.bits() == to.bits() || !(raw(from) || raw(to)) {
            return None;
        }
        let endian = hints::endian_hint(params, |param| {
            hints::classify(self.interner.resolve(*param))
        })?;
        let (from_bytes, to_bytes) = ((from.bits() / 8) as usize, (to.bits() / 8) as usize);
        let mut bytes = Vec::with_capacity(cell.borrow().len() * from_bytes);
        for item in cell.borrow().iter() {
            let Some(pattern) = encode_pattern(item, from) else {
                return Some(Err(RunnerError::internal(
                    span,
                    "bits list conversio met an element that is not a bit pattern",
                )));
            };
            match endian {
                EndianHint::Le => bytes.extend_from_slice(&pattern.to_le_bytes()[..from_bytes]),
                EndianHint::Be => bytes.extend_from_slice(&pattern.to_be_bytes()[8 - from_bytes..]),
            }
        }
        if bytes.len() % to_bytes != 0 {
            return Some(self.recover_or_fail(recovery, span, "bits list length mismatch"));
        }
        let mut items = Vec::with_capacity(bytes.len() / to_bytes);
        for chunk in bytes.chunks_exact(to_bytes) {
            let mut pattern = 0_u64;
            match endian {
                EndianHint::Le => {
                    for (index, byte) in chunk.iter().enumerate() {
                        pattern |= u64::from(*byte) << (8 * index);
                    }
                }
                EndianHint::Be => {
                    for byte in chunk {
                        pattern = (pattern << 8) | u64::from(*byte);
                    }
                }
            }
            let Some(item) = decode_pattern(pattern, to) else {
                return Some(Err(RunnerError::internal(
                    span,
                    "bits list conversio target element is unsupported",
                )));
            };
            items.push(item);
        }
        Some(Ok(Value::Array(Rc::new(RefCell::new(items)))))
    }
}

impl<H: Host + ?Sized> Runner<'_, H> {
    /// The `bN` width (8, 16 or 32) of a bits list's element, looking
    /// through aliases. `None` for every other source.
    pub(super) fn bits_text_width(&self, ty: TypeId) -> Option<u32> {
        let mut ty = ty;
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = *inner;
        }
        let Type::Array(elem) = self.types.get(ty) else {
            return None;
        };
        self.bits_text_element_width(*elem)
    }

    /// The text width of a `bN` element type (8, 16 or 32).
    pub(super) fn bits_text_element_width(&self, elem: TypeId) -> Option<u32> {
        match self.bits_list_cell(elem)? {
            Cell::Raw(bits @ (8 | 16 | 32)) => Some(bits),
            _ => None,
        }
    }

    /// `lista<bN> ↦ textus`: UTF-8 (N = 8), UTF-16 (16) or UTF-32 (32)
    /// decode. The elements are the code units, so no byte order is
    /// involved. An ill-formed sequence is the recoverable failure.
    pub(super) fn convert_bits_list_to_textus(
        &mut self,
        value: &Value,
        width: u32,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Value::Array(cell) = value else {
            return Err(RunnerError::internal(
                span,
                "bits list to textus conversio met a value that is not a list",
            ));
        };
        let mut units = Vec::with_capacity(cell.borrow().len());
        for item in cell.borrow().iter() {
            let Some(pattern) = bits_pattern(item) else {
                return Err(RunnerError::internal(
                    span,
                    "bits list to textus conversio met an element that is not a bit pattern",
                ));
            };
            units.push(pattern & mask(width));
        }
        let decoded = match width {
            8 => units
                .iter()
                .map(|unit| u8::try_from(*unit).ok())
                .collect::<Option<Vec<_>>>()
                .and_then(|bytes| String::from_utf8(bytes).ok()),
            16 => units
                .iter()
                .map(|unit| u16::try_from(*unit).ok())
                .collect::<Option<Vec<_>>>()
                .and_then(|words| String::from_utf16(&words).ok()),
            _ => units
                .iter()
                .map(|unit| u32::try_from(*unit).ok().and_then(char::from_u32))
                .collect::<Option<String>>(),
        };
        match decoded {
            Some(text) => Ok(Value::Str(text)),
            None => self.recover_or_fail(recovery, span, "bits list to textus conversion failed"),
        }
    }

    /// `textus ↦ lista<bN>`: the infallible UTF-8, UTF-16 or UTF-32 encode.
    pub(super) fn convert_textus_to_bits_list(text: &str, width: u32) -> Value {
        let items: Vec<Value> = match width {
            8 => text
                .bytes()
                .map(|unit| Value::UInt(u64::from(unit)))
                .collect(),
            16 => text
                .encode_utf16()
                .map(|unit| Value::UInt(u64::from(unit)))
                .collect(),
            _ => text
                .chars()
                .map(|ch| Value::UInt(u64::from(u32::from(ch))))
                .collect(),
        };
        Value::Array(Rc::new(RefCell::new(items)))
    }
}

fn encode_pattern(value: &Value, cell: Cell) -> Option<u64> {
    match cell {
        Cell::Raw(bits) | Cell::Unsigned(bits) | Cell::Signed(bits) => {
            Some(bits_pattern(value)? & mask(bits))
        }
        Cell::Float(width, bits) => match pattern_from_float(value, width, bits_width(bits)?)? {
            Value::UInt(pattern) => Some(pattern),
            _ => None,
        },
    }
}

fn decode_pattern(pattern: u64, cell: Cell) -> Option<Value> {
    Some(match cell {
        Cell::Raw(_) | Cell::Unsigned(64) => Value::UInt(pattern),
        Cell::Unsigned(_) => Value::Int(i64::from_le_bytes(pattern.to_le_bytes())),
        Cell::Signed(bits) => {
            let shift = 64 - bits;
            Value::Int(i64::from_le_bytes((pattern << shift).to_le_bytes()) >> shift)
        }
        Cell::Float(width, bits) => float_from_pattern(pattern, bits_width(bits)?, width)?,
    })
}
