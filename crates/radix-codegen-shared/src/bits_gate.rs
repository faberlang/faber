//! Fail-closed gate for the raw-bits types `b8 b16 b32 b64`.
//!
//! A `bN` is exactly N bits: no arithmetic, no width above its own, bit
//! operations at the type's width (operator ruling 2026-10-10, need).
//! The Faber source emitter (which prints the marker), the MIR runner (which
//! executes it) and the Phase 3 emitters named in [`target_admits_bits`]
//! implement the type. Every other emitter would read the
//! width as "not an integer" or print a plain integer carrier, which is a wrong
//! program, not a missing one. This gate rejects any module whose type table
//! names a `bN` before an emitter runs, naming the type and the target, so no
//! backend can miscompile it silently. A target that gains a `bN` carrier is
//! added to [`target_admits_bits`].

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_target::Target;
use radix_types::{BitsWidth, Primitive, Type, TypeId, TypeTable};

const ISSUE: &str = "bits_type_unsupported_on_target";

/// Targets that carry `bN` faithfully. The Faber re-emitter keeps the authored
/// marker; the MIR runner executes the type and does not reach codegen. The
/// Phase 3 emitters carry it as an unsigned carrier: Rust and Go as u8..u64,
/// Swift as UInt8..UInt64, TypeScript as `number` (b8, b16, b32) and `bigint`
/// (b64), the MIR sexp target as a non-negative exact Racket integer kept
/// inside its N bits, the MIR wasm target as i32 (b8..b32) and i64 (b64) kept
/// zero-extended in N bits, and the MIR llvm target as iN.
fn target_admits_bits(target: Target) -> bool {
    matches!(
        target,
        Target::HirFaber
            | Target::MirStepper
            | Target::HirRust
            | Target::HirTypeScript
            | Target::HirGo
            | Target::HirSwift
            | Target::MirSexp
            | Target::MirWasm
            | Target::MirLlvm
    )
}

/// # Errors
///
/// Returns a [`CodegenError`] (`bits_type_unsupported_on_target`) when the
/// module's type table names a raw-bits type and `target` has no carrier for it.
pub fn reject_bits_type(types: &TypeTable, target: Target) -> Result<(), CodegenError> {
    if target_admits_bits(target) {
        return Ok(());
    }
    // `first_authored_bits` names only a written `bN` marker: the seeded
    // `lista<b8>` behind the `octeti` word is not one.
    let Some(marker) = types.first_authored_bits().map(BitsWidth::faber_marker) else {
        return Ok(());
    };
    Err(CodegenError {
        message: format!(
            "the raw-bits type `{marker}` is not supported on the {target:?} target: \
             only the Faber source emitter and the MIR runner implement it"
        ),
        args: vec![
            DiagnosticArg::new("issue", ISSUE),
            DiagnosticArg::new("type", marker),
            DiagnosticArg::new("target", format!("{target:?}")),
        ],
    })
}

const LIST_DECODE_ISSUE: &str = "bits_conversion_unsupported_on_target";

/// The bit width a list element type carries, `None` for a number with no
/// fixed width (`numerus`, `inf`) or a non-number.
fn element_bits(types: &TypeTable, ty: TypeId) -> (bool, Option<u32>) {
    use radix_types::NumericWidth as W;
    let ty = types.peel_access_marker(ty).1;
    match types.get(ty) {
        Type::Bits(width) => (true, Some(width.bits())),
        Type::SizedNumeric(_, width) => (
            false,
            match width {
                W::I8 | W::U8 => Some(8),
                W::I16 | W::U16 | W::F16 | W::BF16 => Some(16),
                W::I32 | W::U32 | W::F32 => Some(32),
                W::I64 | W::U64 | W::F64 | W::D64 => Some(64),
                W::Inf => None,
            },
        ),
        _ => (false, None),
    }
}

fn element_label(types: &TypeTable, ty: TypeId) -> String {
    let ty = types.peel_access_marker(ty).1;
    match types.get(ty) {
        Type::Bits(width) => width.faber_marker().to_owned(),
        Type::SizedNumeric(_, width) => format!("{width:?}").to_lowercase(),
        Type::Primitive(primitive) => format!("{primitive:?}").to_lowercase(),
        other => format!("{other:?}"),
    }
}

/// The `(from, to)` list spellings of a conversion between two lists whose
/// element bit width differs and at least one of which is a raw-bits `bN`, or
/// `None` for any other pair. Such a conversion carries a `via Le|Be` clause
/// and decodes or encodes through the byte stream; the
/// same-width list conversion has no clause and is not matched.
#[must_use]
pub fn list_width_change(
    types: &TypeTable,
    source: TypeId,
    target: TypeId,
) -> Option<(String, String)> {
    let (Type::Array(from), Type::Array(to)) = (
        types.get(types.peel_access_marker(source).1),
        types.get(types.peel_access_marker(target).1),
    ) else {
        return None;
    };
    let ((from_bits, from_width), (to_bits, to_width)) =
        (element_bits(types, *from), element_bits(types, *to));
    if !(from_bits || to_bits) {
        return None;
    }
    if matches!((from_width, to_width), (Some(a), Some(b)) if a == b) {
        return None;
    }
    Some((
        format!("list<{}>", element_label(types, *from)),
        format!("list<{}>", element_label(types, *to)),
    ))
}

/// Refuse a list conversion that carries an endian hint and changes the
/// element bit width, naming the conversion and the `backend` (the target's
/// Debug name). Emitters other than the runner and the Faber re-emitter have
/// no byte-stream decode: they would print a per-element numeric cast, which
/// is a wrong program, not a missing one.
///
/// # Errors
///
/// Returns a [`CodegenError`] (`bits_conversion_unsupported_on_target`) for a
/// width-changing list conversion with an endian hint.
pub fn reject_list_width_decode(
    types: &TypeTable,
    source: Option<TypeId>,
    target: TypeId,
    has_endian_hint: bool,
    backend: &str,
) -> Result<(), CodegenError> {
    if !has_endian_hint {
        return Ok(());
    }
    let Some(source) = source else {
        return Ok(());
    };
    let Some((from, to)) = list_width_change(types, source, target) else {
        return Ok(());
    };
    Err(CodegenError {
        message: format!(
            "the conversion `{from}` to `{to}` through a byte order (`via Le` or `via Be`) \
             is not supported on the {backend} target yet"
        ),
        args: vec![
            DiagnosticArg::new("issue", LIST_DECODE_ISSUE),
            DiagnosticArg::new("from", from),
            DiagnosticArg::new("to", to),
            DiagnosticArg::new("target", backend),
        ],
    })
}

/// The `(from, to)` spellings of a conversion between a list of raw-bits
/// `bN` elements and text (`string`), in either direction, or `None` for any
/// other pair. `lista<b8> ↦ string` is a UTF-8 decode, `lista<b16>` UTF-16 and
/// `lista<b32>` UTF-32 (each failable), and `string ↦ lista<bN>` is the encode
///; no emitter other than the runner and the Faber re-emitter
/// implements either direction.
#[must_use]
pub fn list_text_conversion(
    types: &TypeTable,
    source: TypeId,
    target: TypeId,
) -> Option<(String, String)> {
    let side = |ty: TypeId| match types.get(types.peel_access_marker(ty).1) {
        Type::Array(element) => element_bits(types, *element)
            .0
            .then(|| (true, format!("list<{}>", element_label(types, *element)))),
        Type::Primitive(Primitive::Textus) => Some((false, "string".to_owned())),
        _ => None,
    };
    let ((from_list, from), (to_list, to)) = (side(source)?, side(target)?);
    (from_list != to_list).then_some((from, to))
}

/// True for the 8-bit pair of [`list_text_conversion`]: `lista<b8> ↦ string`
/// (the UTF-8 decode) and `string ↦ lista<b8>` (the UTF-8 encode). The word
/// `octeti` (en `bytes`) is `lista<b8>` (need, ruling B9), so this is
/// the pair its text conversions take; an emitter that implements it narrows
/// its refusal to the 16- and 32-bit pairs.
#[must_use]
pub fn is_byte_text_conversion(types: &TypeTable, source: TypeId, target: TypeId) -> bool {
    let bytes = |ty: TypeId| match types.get(types.peel_access_marker(ty).1) {
        Type::Array(element) => matches!(
            types.get(types.peel_access_marker(*element).1),
            Type::Bits(width) if width.bits() == 8
        ),
        _ => false,
    };
    let text = |ty: TypeId| {
        matches!(
            types.get(types.peel_access_marker(ty).1),
            Type::Primitive(Primitive::Textus)
        )
    };
    (bytes(source) && text(target)) || (text(source) && bytes(target))
}

/// Refuse a `lista<bN>` to text or text to `lista<bN>` conversion, naming the
/// conversion and the `backend` (the target's Debug name). Without this the
/// emitters print a stringify, a cast or an opaque handle re-tag, which is a
/// wrong program, not a missing one.
///
/// # Errors
///
/// Returns a [`CodegenError`] (`bits_conversion_unsupported_on_target`) for a
/// list-of-bits and text conversion.
pub fn reject_list_text_conversion(
    types: &TypeTable,
    source: Option<TypeId>,
    target: TypeId,
    backend: &str,
) -> Result<(), CodegenError> {
    let Some(source) = source else {
        return Ok(());
    };
    let Some((from, to)) = list_text_conversion(types, source, target) else {
        return Ok(());
    };
    Err(CodegenError {
        message: format!(
            "the conversion `{from}` to `{to}` (UTF-8, UTF-16 or UTF-32 text) \
             is not supported on the {backend} target yet"
        ),
        args: vec![
            DiagnosticArg::new("issue", LIST_DECODE_ISSUE),
            DiagnosticArg::new("from", from),
            DiagnosticArg::new("to", to),
            DiagnosticArg::new("target", backend),
        ],
    })
}

#[cfg(test)]
#[path = "bits_gate_test.rs"]
mod tests;
