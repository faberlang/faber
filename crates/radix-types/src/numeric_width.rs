//! Width markers for sized `numerus<W>` and `fractus<W>` primitives.
//!
//! Width spellings are reserved target tokens (`i32`, `u8`, `f64`, …), not
//! user-defined type parameters. Family constraints and widening lattices are
//! centralized here so resolve, typecheck, conversio, and codegen share one
//! source of truth.

use crate::{Primitive, Type, TypeId, TypeTable};
use serde::{Deserialize, Serialize};

/// Signed integer, unsigned integer, or float width marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NumericWidth {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F16,
    BF16,
    F32,
    F64,
    /// Decimal-encoded numerus width (scaled integer, d32 = i32×10⁻⁷).
    D32,
    /// Decimal-encoded numerus width (scaled integer, d64 = i64×10⁻⁹).
    D64,
}

/// Family mismatch or unknown width marker for a primitive family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthError {
    UnrecognizedMarker,
    FloatOnNumerus,
    IntOnFractus,
    TooManyParams,
}

/// Typed failure identity for width-family type lowering.
///
/// Producers report this value instead of prose so callers attach the
/// diagnostic issue directly; locale packs own the user-facing sentences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthLowerIssue {
    /// Width sugar (`f32`, `tf32`, …) cannot accept type parameters.
    Params,
    /// Width marker must be a bare reserved spelling.
    Shape,
    /// Float marker applied to `numerus`.
    FloatOnNumerus,
    /// Integer marker applied to `fractus`.
    IntOnFractus,
    /// Unrecognized marker for the primitive family.
    InvalidMarker(Primitive),
    /// Too many width markers for the primitive.
    TooManyParams(Primitive),
    /// `modulus` requires exactly one width marker.
    ModularWordRequired,
    /// `modulus` argument is not a bare width marker.
    ModularWordShape,
    /// `modulus` marker is not one of u8/u16/u32/u64.
    ModularWordUnsupported,
    /// `modulus` marker is a float spelling.
    ModularWordUnsignedRequired,
}

impl WidthLowerIssue {
    /// Canonical issue names; locale packs key diagnostic prose on these.
    pub const WIDTH_MARKER_PARAMS: &str = "width_marker_params";
    pub const WIDTH_MARKER_SHAPE: &str = "width_marker_shape";
    pub const FLOAT_WIDTH_ON_NUMERUS: &str = "float_width_on_numerus";
    pub const INTEGER_WIDTH_ON_FRACTUS: &str = "integer_width_on_fractus";
    pub const INVALID_NUMERUS_WIDTH_MARKER: &str = "invalid_numerus_width_marker";
    pub const INVALID_FRACTUS_WIDTH_MARKER: &str = "invalid_fractus_width_marker";
    pub const INVALID_WIDTH_MARKER: &str = "invalid_width_marker";
    pub const NUMERUS_WIDTH_PARAMS: &str = "numerus_width_params";
    pub const FRACTUS_WIDTH_PARAMS: &str = "fractus_width_params";
    pub const PRIMITIVE_WIDTH_PARAMS: &str = "primitive_width_params";
    pub const MODULAR_WORD_WIDTH_REQUIRED: &str = "modular_word_width_required";
    pub const MODULAR_WORD_WIDTH_SHAPE: &str = "modular_word_width_shape";
    pub const UNSUPPORTED_MODULAR_WORD_WIDTH: &str = "unsupported_modular_word_width";
    pub const MODULAR_WORD_UNSIGNED_WIDTH_REQUIRED: &str = "modular_word_unsigned_width_required";

    /// Diagnostic issue identity for this failure.
    #[must_use]
    pub fn issue(self) -> &'static str {
        match self {
            Self::Params => Self::WIDTH_MARKER_PARAMS,
            Self::Shape => Self::WIDTH_MARKER_SHAPE,
            Self::FloatOnNumerus => Self::FLOAT_WIDTH_ON_NUMERUS,
            Self::IntOnFractus => Self::INTEGER_WIDTH_ON_FRACTUS,
            Self::InvalidMarker(Primitive::Numerus) => Self::INVALID_NUMERUS_WIDTH_MARKER,
            Self::InvalidMarker(Primitive::Fractus) => Self::INVALID_FRACTUS_WIDTH_MARKER,
            Self::InvalidMarker(_) => Self::INVALID_WIDTH_MARKER,
            Self::TooManyParams(Primitive::Numerus) => Self::NUMERUS_WIDTH_PARAMS,
            Self::TooManyParams(Primitive::Fractus) => Self::FRACTUS_WIDTH_PARAMS,
            Self::TooManyParams(_) => Self::PRIMITIVE_WIDTH_PARAMS,
            Self::ModularWordRequired => Self::MODULAR_WORD_WIDTH_REQUIRED,
            Self::ModularWordShape => Self::MODULAR_WORD_WIDTH_SHAPE,
            Self::ModularWordUnsupported => Self::UNSUPPORTED_MODULAR_WORD_WIDTH,
            Self::ModularWordUnsignedRequired => Self::MODULAR_WORD_UNSIGNED_WIDTH_REQUIRED,
        }
    }
}

/// Producer-side mapping from a width parse failure to its lowering identity.
fn width_lower_issue(err: WidthError, prim: Primitive) -> WidthLowerIssue {
    match err {
        WidthError::FloatOnNumerus => WidthLowerIssue::FloatOnNumerus,
        WidthError::IntOnFractus => WidthLowerIssue::IntOnFractus,
        WidthError::UnrecognizedMarker => WidthLowerIssue::InvalidMarker(prim),
        WidthError::TooManyParams => WidthLowerIssue::TooManyParams(prim),
    }
}

impl NumericWidth {
    /// Default width for bare `numerus` / `fractus`.
    ///
    /// Returns `None` for non-numeric primitives.
    #[must_use]
    pub fn default_for(prim: Primitive) -> Option<Self> {
        match prim {
            Primitive::Numerus => Some(Self::I64),
            Primitive::Fractus => Some(Self::F64),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_default_for(self, prim: Primitive) -> bool {
        Self::default_for(prim).is_some_and(|d| self == d)
    }

    /// Parse a width marker spelling for the given primitive family.
    ///
    /// # Errors
    ///
    /// Returns an error if the marker is not valid for the given primitive family.
    pub fn from_marker(marker: &str, prim: Primitive) -> Result<Self, WidthError> {
        match (marker, prim) {
            ("i8", Primitive::Numerus) => Ok(Self::I8),
            ("i16", Primitive::Numerus) => Ok(Self::I16),
            ("i32", Primitive::Numerus) => Ok(Self::I32),
            ("i64", Primitive::Numerus) => Ok(Self::I64),
            ("u8", Primitive::Numerus) => Ok(Self::U8),
            ("u16", Primitive::Numerus) => Ok(Self::U16),
            ("u32", Primitive::Numerus) => Ok(Self::U32),
            ("u64", Primitive::Numerus) => Ok(Self::U64),
            // decimal-widths D1: d-markers are valid only on numerus (the
            // surgical FloatOnNumerus relaxation — f-markers still reject).
            ("d32", Primitive::Numerus) => Ok(Self::D32),
            ("d64", Primitive::Numerus) => Ok(Self::D64),
            ("f16" | "f32" | "f64" | "bf16", Primitive::Numerus) => Err(WidthError::FloatOnNumerus),
            ("f16", Primitive::Fractus) => Ok(Self::F16),
            ("bf16", Primitive::Fractus) => Ok(Self::BF16),
            ("f32", Primitive::Fractus) => Ok(Self::F32),
            ("f64", Primitive::Fractus) => Ok(Self::F64),
            ("i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64", Primitive::Fractus) => {
                Err(WidthError::IntOnFractus)
            }
            _ => Err(WidthError::UnrecognizedMarker),
        }
    }

    #[must_use]
    pub fn is_signed_int(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    #[must_use]
    pub fn is_unsigned_int(self) -> bool {
        matches!(self, Self::U8 | Self::U16 | Self::U32 | Self::U64)
    }

    /// Maximum value of an unsigned integer width; `None` for signed/float widths.
    #[must_use]
    pub fn unsigned_max(self) -> Option<u64> {
        match self {
            Self::U8 => Some(u64::from(u8::MAX)),
            Self::U16 => Some(u64::from(u16::MAX)),
            Self::U32 => Some(u64::from(u32::MAX)),
            Self::U64 => Some(u64::MAX),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_float(self) -> bool {
        matches!(self, Self::F16 | Self::BF16 | Self::F32 | Self::F64)
    }

    /// Rust primitive spelling for codegen.
    #[must_use]
    pub fn rust_type(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F16 => "f16",
            Self::BF16 => "bf16",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::D32 => "d32",
            Self::D64 => "d64",
        }
    }

    /// Faber width marker spelling for pretty-print.
    #[must_use]
    pub fn faber_marker(self) -> &'static str {
        self.rust_type()
    }

    fn signed_rank(self) -> Option<u8> {
        match self {
            Self::I8 => Some(0),
            Self::I16 => Some(1),
            Self::I32 => Some(2),
            Self::I64 => Some(3),
            _ => None,
        }
    }

    fn unsigned_rank(self) -> Option<u8> {
        match self {
            Self::U8 => Some(0),
            Self::U16 => Some(1),
            Self::U32 => Some(2),
            Self::U64 => Some(3),
            _ => None,
        }
    }

    fn float_rank(self) -> Option<u8> {
        match self {
            Self::F16 => Some(0),
            Self::F32 => Some(1),
            Self::F64 => Some(2),
            _ => None,
        }
    }

    /// Decimal-width lattice rank; `d32 → d64` is the only decimal widening.
    fn decimal_rank(self) -> Option<u8> {
        match self {
            Self::D32 => Some(0),
            Self::D64 => Some(1),
            _ => None,
        }
    }

    /// Decimal-digit scale exponent of the scaled-integer carrier.
    ///
    /// d32 = i32×10⁻⁷, d64 = i64×10⁻⁹ (CTO ruling, decimal-widths D0).
    #[must_use]
    pub fn decimal_scale(self) -> Option<u32> {
        match self {
            Self::D32 => Some(7),
            Self::D64 => Some(9),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_decimal(self) -> bool {
        matches!(self, Self::D32 | Self::D64)
    }

    /// Whether `from` may flow into `to` along the v1 widening lattice.
    #[must_use]
    pub fn is_widening_assignable(from: Self, to: Self) -> bool {
        if from == to {
            return true;
        }
        if let (Some(a), Some(b)) = (from.signed_rank(), to.signed_rank()) {
            return a <= b;
        }
        if let (Some(a), Some(b)) = (from.unsigned_rank(), to.unsigned_rank()) {
            return a <= b;
        }
        if let (Some(a), Some(b)) = (from.float_rank(), to.float_rank()) {
            return a <= b;
        }
        // decimal-widths D1: d32 → d64 is the only decimal widening; decimals
        // never widen into or out of the integer/fractus lattices.
        if let (Some(a), Some(b)) = (from.decimal_rank(), to.decimal_rank()) {
            return a <= b;
        }
        // Signed integer chain may widen into fractus<f64>.
        if from.is_signed_int() && to == Self::F64 {
            return true;
        }
        false
    }
}

/// Parse decimal digit text into an exact scaled representation:
/// `(mantissa, exp10)` with `value == mantissa × 10^exp10` (decimal-widths D1).
///
/// Accepts the lexer's cleaned decimal spelling (`4.2`, `1e-3`, `42`); returns
/// `None` for malformed text or mantissa overflow of the i128 carrier.
#[must_use]
pub fn decimal_text_to_scaled(text: &str) -> Option<(i128, i32)> {
    let mut mantissa: i128 = 0;
    let mut exp10: i32 = 0;
    let mut seen_digit = false;
    let mut rest = text;
    while let Some(digit) = rest.chars().next().filter(char::is_ascii_digit) {
        mantissa = mantissa
            .checked_mul(10)?
            .checked_add(i128::from(digit as u8 - b'0'))?;
        seen_digit = true;
        rest = &rest[1..];
    }
    if let Some(frac) = rest.strip_prefix('.') {
        rest = frac;
        while let Some(digit) = rest.chars().next().filter(char::is_ascii_digit) {
            mantissa = mantissa
                .checked_mul(10)?
                .checked_add(i128::from(digit as u8 - b'0'))?;
            exp10 -= 1;
            seen_digit = true;
            rest = &rest[1..];
        }
    }
    if let Some(exp) = rest.strip_prefix(['e', 'E']) {
        let (sign, exp) = match exp.strip_prefix('-') {
            Some(e) => (-1i32, e),
            None => (1i32, exp.strip_prefix('+').unwrap_or(exp)),
        };
        let e: i32 = exp.parse().ok()?;
        exp10 = exp10.checked_add(sign.checked_mul(e)?)?;
    } else if !rest.is_empty() {
        return None;
    }
    if seen_digit {
        Some((mantissa, exp10))
    } else {
        None
    }
}

/// Conversio slot-2 hints that follow an optional width marker.
///
/// `Hex` / `Bin` / `Oct` are text-parse radices. `Be` / `Le` are endian
/// unpack rules for `octeti` windows — same slot, not bases. `Bits` is an
/// exact-width bitcast (`u32↔f32`, `u64↔f64`) — reinterpretation, not a
/// radix, and never legal in a `from_str_radix` base.
#[must_use]
pub fn is_conversio_radix_hint(marker: &str) -> bool {
    matches!(marker, "Hex" | "Bin" | "Oct" | "Be" | "Le" | "Bits")
}

/// Ascii format-pack slot-2 hints: `n ↦ ascii<N, Hex|Bin|Oct>`.
///
/// Distinct from [`is_conversio_radix_hint`]: `Be` / `Le` are endian unpack
/// rules, never ascii digit bases. Slot 1 of a bounded family stays capacity.
#[must_use]
pub fn is_ascii_format_hint(marker: &str) -> bool {
    matches!(marker, "Hex" | "Bin" | "Oct")
}

/// Standalone width-marker type sugar (`f32` → `fractus<f32>`, `i64` → `numerus<i64>`).
#[must_use]
pub fn standalone_width_type(name: &str) -> Option<(Primitive, NumericWidth)> {
    NumericWidth::from_marker(name, Primitive::Numerus)
        .ok()
        .map(|width| (Primitive::Numerus, width))
        .or_else(|| {
            NumericWidth::from_marker(name, Primitive::Fractus)
                .ok()
                .map(|width| (Primitive::Fractus, width))
        })
}

/// Whether `name` is bare width sugar (`f32`), tensor sugar (`tf32`), vector
/// sugar (`vf32`), matrix sugar (`mf32`), sparsa sugar (`sf32`), or lista sugar (`lf32`).
#[must_use]
pub fn is_numeric_width_type_sugar(name: &str) -> bool {
    standalone_width_type(name).is_some()
        || width_prefixed_sugar_marker(name, 't').is_some()
        || width_prefixed_sugar_marker(name, 'v').is_some()
        || width_prefixed_sugar_marker(name, 'm').is_some()
        || width_prefixed_sugar_marker(name, 's').is_some()
        || width_prefixed_sugar_marker(name, 'l').is_some()
}

/// Width marker after `t` in tensor type sugar (`tf32` → element `f32`).
#[must_use]
pub fn tensor_sugar_width_marker(name: &str) -> Option<&str> {
    width_prefixed_sugar_marker(name, 't')
}

/// Width marker after `v` in vector type sugar (`vf32` → element `f32`).
#[must_use]
pub fn vector_sugar_width_marker(name: &str) -> Option<&str> {
    width_prefixed_sugar_marker(name, 'v')
}

/// Width marker after `m` in matrix type sugar (`mf32` → element `f32`).
#[must_use]
pub fn matrix_sugar_width_marker(name: &str) -> Option<&str> {
    width_prefixed_sugar_marker(name, 'm')
}

/// Width marker after `s` in sparsa type sugar (`sf32` → element `f32`).
#[must_use]
pub fn sparsa_sugar_width_marker(name: &str) -> Option<&str> {
    width_prefixed_sugar_marker(name, 's')
}

/// Width marker after `l` in lista type sugar (`lf32` → `lista<f32>`).
#[must_use]
pub fn lista_sugar_width_marker(name: &str) -> Option<&str> {
    width_prefixed_sugar_marker(name, 'l')
}

fn width_prefixed_sugar_marker(name: &str, prefix: char) -> Option<&str> {
    let marker = name.strip_prefix(prefix)?;
    standalone_width_type(marker).map(|_| marker)
}

/// Lower standalone width-marker type syntax when `name` is a reserved marker spelling.
pub fn lower_standalone_width_type(
    name: &str,
    params: &[radix_syntax::TypeExpr],
    types: &mut TypeTable,
    report: &mut dyn FnMut(WidthLowerIssue),
) -> Option<TypeId> {
    let (prim, width) = standalone_width_type(name)?;
    if !params.is_empty() {
        report(WidthLowerIssue::Params);
        return Some(types.intern(Type::Error));
    }
    Some(types.sized_numeric(prim, width))
}

/// Lower `numerus` / `fractus` with optional width type arguments.
pub fn lower_sized_primitive(
    prim: Primitive,
    params: &[radix_syntax::TypeExpr],
    interner: &radix_lexer::Interner,
    types: &mut TypeTable,
    report: &mut dyn FnMut(WidthLowerIssue),
) -> TypeId {
    match params.len() {
        0 => types.primitive(prim),
        1 => {
            if matches!(params[0].kind, radix_syntax::TypeExprKind::Infer) {
                let family = match prim {
                    Primitive::Numerus => crate::MarkerFamily::Numerus,
                    Primitive::Fractus => crate::MarkerFamily::Fractus,
                    _ => {
                        report(WidthLowerIssue::Shape);
                        return types.intern(Type::Error);
                    }
                };
                return types.intern(Type::MarkerHole {
                    family,
                    var: crate::InferVar(params[0].span.start),
                });
            }
            if let Some(marker) = type_expr_width_marker(&params[0], interner) {
                match NumericWidth::from_marker(marker, prim) {
                    Ok(width) => types.sized_numeric(prim, width),
                    Err(err) => {
                        report(width_lower_issue(err, prim));
                        types.intern(Type::Error)
                    }
                }
            } else {
                report(WidthLowerIssue::Shape);
                types.intern(Type::Error)
            }
        }
        _ => {
            report(WidthLowerIssue::TooManyParams(prim));
            types.intern(Type::Error)
        }
    }
}

/// Lower the compiler-owned modular word family.
///
/// Admits exactly the unsigned integer widths u8/u16/u32/u64; signed and
/// float markers reject with their own structured issues.
#[allow(clippy::match_same_arms)]
pub fn lower_modular_word(
    params: &[radix_syntax::TypeExpr],
    interner: &radix_lexer::Interner,
    types: &mut TypeTable,
    report: &mut dyn FnMut(WidthLowerIssue),
) -> TypeId {
    if params.len() != 1 {
        report(WidthLowerIssue::ModularWordRequired);
        return types.intern(Type::Error);
    }
    if matches!(params[0].kind, radix_syntax::TypeExprKind::Infer) {
        return types.intern(Type::MarkerHole {
            family: crate::MarkerFamily::Modulus,
            var: crate::InferVar(params[0].span.start),
        });
    }
    let Some(marker) = type_expr_width_marker(&params[0], interner) else {
        report(WidthLowerIssue::ModularWordShape);
        return types.intern(Type::Error);
    };
    match NumericWidth::from_marker(marker, Primitive::Numerus) {
        Ok(width) if width.is_unsigned_int() => types.intern(Type::ModularWord(width)),
        Ok(_) => {
            report(WidthLowerIssue::ModularWordUnsupported);
            types.intern(Type::Error)
        }
        Err(WidthError::FloatOnNumerus) => {
            report(WidthLowerIssue::ModularWordUnsignedRequired);
            types.intern(Type::Error)
        }
        Err(_) => {
            report(WidthLowerIssue::ModularWordUnsupported);
            types.intern(Type::Error)
        }
    }
}

fn type_expr_width_marker<'a>(
    ty: &'a radix_syntax::TypeExpr,
    interner: &'a radix_lexer::Interner,
) -> Option<&'a str> {
    match &ty.kind {
        radix_syntax::TypeExprKind::Named(ident, params) if params.is_empty() => {
            Some(interner.resolve(ident.name))
        }
        _ => None,
    }
}

/// Effective width for bare or sized numeric types.
#[must_use]
#[allow(clippy::match_same_arms)]
pub fn effective_width(types: &TypeTable, ty: TypeId) -> Option<NumericWidth> {
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => Some(NumericWidth::I64),
        Type::Primitive(Primitive::Fractus) => Some(NumericWidth::F64),
        Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, width) => Some(*width),
        Type::ModularWord(width) => Some(*width),
        Type::Alias(_, inner) => effective_width(types, *inner),
        _ => None,
    }
}

/// Integer width of a `numerus` type (bare `numerus` is `i64`); `None` for
/// fractus, decimal, modular, and non-numeric types.
#[must_use]
pub fn numerus_integer_width(types: &TypeTable, ty: TypeId) -> Option<NumericWidth> {
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => Some(NumericWidth::I64),
        Type::SizedNumeric(Primitive::Numerus, width) if width.integer_range().is_some() => {
            Some(*width)
        }
        Type::Alias(_, inner) => numerus_integer_width(types, *inner),
        _ => None,
    }
}

/// D1.11: the `(source, target)` integer widths of a `numerus ↦ numerus<W>`
/// conversion that must magnitude-check its value, or `None` when the pair is
/// not integer numerus on both sides or the target range holds every source
/// value. Every backend's `↦` narrowing arm keys off this one predicate.
#[must_use]
pub fn checked_integer_conversio(
    types: &TypeTable,
    from: TypeId,
    to: TypeId,
) -> Option<(NumericWidth, NumericWidth)> {
    let from_width = numerus_integer_width(types, from)?;
    let to_width = numerus_integer_width(types, to)?;
    NumericWidth::is_checked_integer_conversio(from_width, to_width)
        .then_some((from_width, to_width))
}

pub const TENSOR_ELEMENT_NUMERIC_MSG: &str = "tensor arithmetic requires numeric element type";

/// DEFER-064: tensor `accipe` / `ponde` (and other `NumerusArray` slots) accept
/// `lista<integer>` arguments that fit the canonical runtime boundary. The
/// boundary stays `lista<numerus>` (`&[i64]`); widening from `u32` / `i32` / …
/// happens at the backend boundary in `write_lista_i64_arg`. This is a
/// structural exception for index vectors only — it does **not** widen the
/// global signed↔unsigned lattice.
pub const TENSOR_INDEX_VECTOR_MSG: &str = "tensor index vector requires lista of integer elements";

/// decimal-widths D2 (wildcard audit): decimal widths are value scalars, not
/// positions. A `numerus<d32|d64>` index would scale-address storage — the
/// bracket index boundary is integer-only.
pub const DECIMAL_INDEX_SCALAR_MSG: &str = "decimal-width numerus cannot index; decimal widths are scaled values, not positions — use an integer width such as numerus<i32>";

/// decimal-widths D2 (wildcard audit): tensor `media` reduces over a float
/// carrier to avoid integer division; a decimal element has no defined media
/// carrier yet (conversio rows are D3), so it fails closed here.
pub const DECIMAL_TENSOR_MEDIA_MSG: &str =
    "tensor media does not accept decimal-width elements; convert to fractus<f64> first";

/// Whether `ty` is a `lista<integer>` usable as a tensor index vector (DEFER-064).
///
/// Accepts integer widths that fit in `i64` (`numerus`, `numerus<u32>`, …);
/// rejects `u64`, non-integer lists (`lista<f32>`, `lista<textus>`, …), and
/// non-list types.
#[must_use]
pub fn is_integer_index_vector(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Array(elem) | Type::BoundedArray { element: elem, .. } => {
            is_integer_index_scalar(types, *elem)
        }
        Type::Alias(_, inner) => is_integer_index_vector(types, *inner),
        _ => false,
    }
}

/// Whether `ty` is a single integer value that fits the tensor index boundary.
///
/// decimal-widths D2: decimal markers reject — a scaled value is not a
/// position (audit class: index scalars, wrong-to-inherit).
#[must_use]
pub fn is_integer_index_scalar(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => true,
        Type::SizedNumeric(Primitive::Numerus, width) => {
            *width != NumericWidth::U64 && !width.is_decimal()
        }
        Type::Alias(_, inner) => is_integer_index_scalar(types, *inner),
        _ => false,
    }
}

/// Whether `ty` is a decimal-width numerus scalar (index-audit reject probe).
#[must_use]
pub fn is_decimal_index_scalar(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::SizedNumeric(Primitive::Numerus, width) => width.is_decimal(),
        Type::Alias(_, inner) => is_decimal_index_scalar(types, *inner),
        _ => false,
    }
}

/// Target `fractus` width for tensor `media` over an element type.
///
/// `numerus` family reduces to `fractus<f64>` to avoid integer division;
/// `fractus` family keeps its own width so `tf32` media stays `f32`.
/// decimal-widths D2: decimal elements reject — there is no float media
/// carrier for a scaled integer yet (audit class: tensor media).
#[must_use]
pub fn tensor_media_width(types: &TypeTable, elem: TypeId) -> Option<NumericWidth> {
    match types.get(elem) {
        Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => None,
        Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
            Some(NumericWidth::F64)
        }
        Type::Primitive(Primitive::Fractus) => Some(NumericWidth::F64),
        Type::SizedNumeric(Primitive::Fractus, width) => Some(*width),
        Type::Alias(_, inner) => tensor_media_width(types, *inner),
        _ => None,
    }
}

/// Whether `ty` is a numeric element usable by tensor arithmetic and the dtype lattice.
#[must_use]
#[allow(clippy::match_same_arms)]
pub fn is_tensor_numeric_element(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus | Primitive::Fractus) => true,
        Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _) => true,
        Type::Alias(_, inner) => is_tensor_numeric_element(types, *inner),
        _ => false,
    }
}

/// Whether `ty` is a numeric tensor element.
///
/// Alias retained for conversio and lattice call sites that predate the Stage 6
/// element-relaxation split between construction and arithmetic.
#[must_use]
pub fn is_tensor_element_type(types: &TypeTable, ty: TypeId) -> bool {
    is_tensor_numeric_element(types, ty)
}

/// Deferred tensor methods that require a numeric element type.
#[must_use]
pub fn tensor_method_requires_numeric_element(method_name: &str) -> bool {
    matches!(
        method_name,
        // S6-C2: `addita_bias` is the bounded bias-broadcast add over a
        // numeric element type, same element gate as the general `addita`.
        "addita"
            | "addita_bias"
            | "subtrahe"
            | "multiplica"
            | "maximus"
            | "minimus"
            | "matmul"
            | "summa"
            | "media"
            | "silu"
    )
}

/// Whether two numeric types are assignable via equality or widening.
#[must_use]
pub fn is_numeric_assignable(types: &TypeTable, from: TypeId, to: TypeId) -> bool {
    // WHY: modular words share an integer carrier with `numerus`, but their
    // arithmetic policy is part of the type family. Only explicit `↦`
    // conversion can cross that boundary.
    if is_modular_word(types, from) != is_modular_word(types, to) {
        return false;
    }
    let from_w = effective_width(types, from);
    let to_w = effective_width(types, to);
    match (from_w, to_w) {
        (Some(a), Some(b)) => NumericWidth::is_widening_assignable(a, b),
        _ => false,
    }
}

fn is_modular_word(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::ModularWord(_) => true,
        Type::Alias(_, inner) => is_modular_word(types, *inner),
        _ => false,
    }
}

/// Whether `tensor<A> ↦ tensor<B>` may use the element-wise conversio arm.
#[must_use]
pub fn tensor_element_conversio_allowed(types: &TypeTable, from: TypeId, to: TypeId) -> bool {
    if types.equals(from, to) {
        return true;
    }
    if !is_tensor_element_type(types, from) || !is_tensor_element_type(types, to) {
        return false;
    }
    if is_numeric_assignable(types, from, to) {
        return true;
    }
    if let (
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _),
        Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _),
    ) = (types.get(from), types.get(to))
    {
        true
    } else {
        let (Some(from_w), Some(to_w)) = (effective_width(types, from), effective_width(types, to))
        else {
            return false;
        };
        NumericWidth::is_explicit_narrowing_conversio(from_w, to_w)
    }
}

impl NumericWidth {
    /// Lossy width narrowing allowed only through explicit `↦` (not bare `←`).
    #[must_use]
    pub fn is_explicit_narrowing_conversio(from: Self, to: Self) -> bool {
        if from == to || Self::is_widening_assignable(from, to) {
            return false;
        }
        if let (Some(a), Some(b)) = (from.signed_rank(), to.signed_rank()) {
            return a > b;
        }
        if let (Some(a), Some(b)) = (from.unsigned_rank(), to.unsigned_rank()) {
            return a > b;
        }
        if let (Some(a), Some(b)) = (from.float_rank(), to.float_rank()) {
            return a > b;
        }
        false
    }

    /// Inclusive value range of an integer width; `None` for float and
    /// decimal widths.
    #[must_use]
    pub fn integer_range(self) -> Option<(i128, i128)> {
        let range = match self {
            Self::I8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
            Self::I16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
            Self::I32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
            Self::I64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
            Self::U8 => (0, i128::from(u8::MAX)),
            Self::U16 => (0, i128::from(u16::MAX)),
            Self::U32 => (0, i128::from(u32::MAX)),
            Self::U64 => (0, i128::from(u64::MAX)),
            _ => return None,
        };
        Some(range)
    }

    /// D1.11: whether an explicit integer `↦` from `from` into `to` must
    /// magnitude-check the value — the target range does not contain the
    /// source range, so some source values do not fit and the conversion
    /// fails (or yields its `⇥` recovery) instead of wrapping or relabeling.
    #[must_use]
    pub fn is_checked_integer_conversio(from: Self, to: Self) -> bool {
        match (from.integer_range(), to.integer_range()) {
            (Some((from_lo, from_hi)), Some((to_lo, to_hi))) => from_lo < to_lo || from_hi > to_hi,
            _ => false,
        }
    }

    /// The target bounds a checked integer `↦` must test: `(lower, upper)`,
    /// each `Some` only when that target bound lies strictly inside the source
    /// range. A guard constant is therefore always representable in the
    /// source carrier, and an unreachable bound (`u16 < 0`) is never emitted.
    #[must_use]
    pub fn checked_integer_guards(from: Self, to: Self) -> (Option<i128>, Option<i128>) {
        match (from.integer_range(), to.integer_range()) {
            (Some((from_lo, from_hi)), Some((to_lo, to_hi))) => (
                (to_lo > from_lo).then_some(to_lo),
                (to_hi < from_hi).then_some(to_hi),
            ),
            _ => (None, None),
        }
    }

    /// decimal-widths D3: the decimal conversio matrix admits exactly the
    /// widening arm (`d32 → d64`, identity included); the reverse (`d64 → d32`)
    /// is a narrowing with no `↦` arm — decimals never narrow through
    /// conversio, unlike integer widths where explicit narrowing is a row.
    #[must_use]
    pub fn is_decimal_narrowing_conversio(from: Self, to: Self) -> bool {
        from.is_decimal() && to.is_decimal() && !Self::is_widening_assignable(from, to)
    }
}

#[cfg(test)]
#[path = "numeric_width_test.rs"]
mod tests;

#[cfg(test)]
#[path = "decimal_width_test.rs"]
mod decimal_width_test;
