//! `littera` text conversions and the `Code` hint (DECISIONS D10.6).
//!
//! `littera` means a one-letter string. A number converts to its digits
//! (`7 ↦ littera` = `'7'`, fails outside 0–9); a littera parses as its digit;
//! a `textus` converts only when it holds exactly one scalar. The `Code` hint
//! reads or builds the code point (`'A' ↦ u32 via Code` = 65,
//! `65 ↦ littera via Code` = `'A'`, failing above U+10FFFF and on surrogates).
//!
//! A `littera` value reaches the runner as `Value::Char`, or as the
//! `Value::Ascii` byte of a widened `ascii<1>` literal (D10.5).

use super::{MirOperand, Runner, RunnerError, Span, Symbol, Value};

impl<H: super::super::Host + ?Sized> Runner<'_, H> {
    pub(crate) fn convert_to_littera(
        &mut self,
        value: Value,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let converted = if self.has_code_point_hint(params) {
            integer_value(&value)
                .and_then(|n| u32::try_from(n).ok())
                .and_then(char::from_u32)
        } else {
            match value {
                Value::Char(ch) => Some(ch),
                Value::Ascii(byte) => Some(char::from(byte)),
                Value::Str(text) => only_scalar(&text),
                other => integer_value(&other)
                    .and_then(|n| u32::try_from(n).ok())
                    .and_then(|digit| char::from_digit(digit, 10)),
            }
        };
        match converted {
            Some(ch) => Ok(Value::Char(ch)),
            None => self.recover_or_fail(recovery, span, "littera conversion failed"),
        }
    }

    /// `littera ↦ numerus<W>` parses the digit; with `Code` it is the scalar
    /// value (`numerus<u32>` only, admitted by typecheck).
    pub(crate) fn convert_littera_to_numerus(
        &mut self,
        ch: char,
        params: &[Symbol],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if self.has_code_point_hint(params) {
            return Ok(Value::UInt(u64::from(u32::from(ch))));
        }
        match ch.to_digit(10) {
            Some(digit) => Ok(Value::Int(i64::from(digit))),
            None => self.recover_or_fail(recovery, span, "littera to numerus conversion failed"),
        }
    }

    pub(super) fn has_code_point_hint(&self, params: &[Symbol]) -> bool {
        params
            .iter()
            .any(|param| radix_types::is_code_point_hint(self.interner.resolve(*param)))
    }
}

/// The littera carried by a `littera`-typed value, if it is one.
pub(super) fn littera_value(value: &Value) -> Option<char> {
    match value {
        Value::Char(ch) => Some(*ch),
        Value::Ascii(byte) => Some(char::from(*byte)),
        _ => None,
    }
}

fn integer_value(value: &Value) -> Option<i128> {
    match value {
        Value::Int(n) => Some(i128::from(*n)),
        Value::UInt(n) => Some(i128::from(*n)),
        _ => None,
    }
}

fn only_scalar(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}
