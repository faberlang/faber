//! Width markers for sized `numerus<W>` and `fractus<W>` primitives.
//!
//! Width spellings are reserved target tokens (`i32`, `u8`, `f64`, …), not
//! user-defined type parameters. Family constraints and widening lattices are
//! centralized here so resolve, typecheck, conversio, and codegen share one
//! source of truth.

use crate::{FloatPolicy, InferVar, IntPolicy, MarkerFamily, Primitive, Type, TypeId, TypeTable};
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
    /// Decimal-encoded numerus width (scaled integer, d64 = i64×10⁻⁸, D11.7).
    D64,
    /// Opt-in unbounded integer (`inf`, inf-delivery ruling 32): every
    /// integer, no range, no ∞. A trapping-family integer width whose
    /// `integer_range()` is `None` by design. Appended last so the serialized
    /// indices of the existing widths never move.
    Inf,
}

/// Raw-bits width marker (`b8`, `b16`, `b32`, `b64`).
///
/// Bits are not numbers: a `bN` is exactly N bits with no arithmetic and no
/// numeric widening. The markers are locale-neutral, like `u8` and `f16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BitsWidth {
    B8,
    B16,
    B32,
    B64,
}

impl BitsWidth {
    /// Parse a bare `bN` marker spelling.
    #[must_use]
    pub fn from_marker(marker: &str) -> Option<Self> {
        match marker {
            "b8" => Some(Self::B8),
            "b16" => Some(Self::B16),
            "b32" => Some(Self::B32),
            "b64" => Some(Self::B64),
            _ => None,
        }
    }

    /// Number of bits in the container.
    #[must_use]
    pub fn bits(self) -> u32 {
        match self {
            Self::B8 => 8,
            Self::B16 => 16,
            Self::B32 => 32,
            Self::B64 => 64,
        }
    }

    /// Source marker spelling (also the diagnostic display).
    #[must_use]
    pub fn faber_marker(self) -> &'static str {
        match self {
            Self::B8 => "b8",
            Self::B16 => "b16",
            Self::B32 => "b32",
            Self::B64 => "b64",
        }
    }
}

/// The removed `d32` marker spelling (D11.7): diagnosed, never parsed.
pub const REMOVED_D32_MARKER: &str = "d32";

/// Family mismatch or unknown width marker for a primitive family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthError {
    UnrecognizedMarker,
    /// `d32`, removed as a type (D11.7); `d64` is the only decimal width.
    RemovedDecimal,
    FloatOnNumerus,
    IntOnFractus,
}

/// Typed failure identity for width-family type lowering.
///
/// Producers report this value instead of prose so callers attach the
/// diagnostic issue directly; locale packs own the user-facing sentences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthLowerIssue {
    /// Width sugar (`f32`, `tf32`, …) cannot accept type parameters.
    Params,
    /// `d32` was removed (D11.7); `d64` is the only decimal width.
    RemovedDecimalWidth,
    /// `modulus` requires exactly one width marker.
    ModularWordRequired,
    /// `modulus` argument is not a bare width marker.
    ModularWordShape,
    /// `modulus` marker is not one of u8/u16/u32/u64.
    ModularWordUnsupported,
    /// `modulus` marker is a float spelling.
    ModularWordUnsignedRequired,
    /// `saturatus` requires exactly one width marker.
    SaturatingWordRequired,
    /// `saturatus` argument is not a bare width marker.
    SaturatingWordShape,
    /// `saturatus` marker is not one of i8…i64 / u8…u64.
    SaturatingWordUnsupported,
    /// `exactus` (en `trapping`) requires exactly one width marker.
    TrappingWordRequired,
    /// `exactus` argument is not a bare width marker.
    TrappingWordShape,
    /// `exactus` marker is not an integer width or `d64`.
    TrappingWordUnsupported,
    /// A range after a store policy's width has an end that is a name (a size
    /// or a constant); only integer literals and `∞` fold at compile time.
    RangedBoundNotConstant,
    /// A range follows a bare width marker (`u8<0‥8>`): the policy word is
    /// mandatory, so the edge behaviour is always visible.
    RangedPolicyWordRequired,
    /// A store policy takes the width and then at most one range.
    RangedArity,
    /// The range holds no value (`5‥5`, `5…3`).
    RangedEmpty,
    /// A bound lies outside the width the range refines (including `∞` as a
    /// lower bound or `-∞` as an upper bound).
    RangedOutsideWidth,
    /// An unsigned width with a negative lower bound.
    RangedUnsignedNegative,
    /// A float, decimal or `inf` width with a range: ranged floats are not
    /// built, and the unbounded integer has no width to refine.
    RangedWidthUnsupported,
    /// A list length range (`list<T, size lo‥hi>`) has an end that is a name or a
    /// size; only integer literals and `∞` fold at compile time.
    RangedListBoundNotConstant,
    /// A list length range holds no length (`4‥4`, `5…3`, `∞‥∞`).
    RangedListEmpty,
    /// A list length range with a negative end: a length is a count.
    RangedListBoundNegative,
    /// A store policy word with a range around a list
    /// (`saturating<list<T>, 0‥3>`, `wrapping<list<T>, 0‥3>`,
    /// `trapping<list<T>, 1…3>`): a list length range is spelled
    /// `list<T, size lo‥hi>` and always traps, so no policy word wraps a list. The
    /// one-argument shapes (`wrapping<list<T>>`) keep their own width-shape
    /// issues.
    RangedListPolicyWord,
}

impl WidthLowerIssue {
    /// Canonical issue names; locale packs key diagnostic prose on these.
    pub const WIDTH_MARKER_PARAMS: &str = "width_marker_params";
    pub const REMOVED_DECIMAL_WIDTH: &str = "removed_decimal_width_d32";
    pub const MODULAR_WORD_WIDTH_REQUIRED: &str = "modular_word_width_required";
    pub const MODULAR_WORD_WIDTH_SHAPE: &str = "modular_word_width_shape";
    pub const UNSUPPORTED_MODULAR_WORD_WIDTH: &str = "unsupported_modular_word_width";
    pub const MODULAR_WORD_UNSIGNED_WIDTH_REQUIRED: &str = "modular_word_unsigned_width_required";
    pub const SATURATING_WORD_WIDTH_REQUIRED: &str = "saturating_word_width_required";
    pub const SATURATING_WORD_WIDTH_SHAPE: &str = "saturating_word_width_shape";
    pub const UNSUPPORTED_SATURATING_WORD_WIDTH: &str = "unsupported_saturating_word_width";
    pub const TRAPPING_WORD_WIDTH_REQUIRED: &str = "trapping_word_width_required";
    pub const TRAPPING_WORD_WIDTH_SHAPE: &str = "trapping_word_width_shape";
    pub const UNSUPPORTED_TRAPPING_WORD_WIDTH: &str = "unsupported_trapping_word_width";
    pub const RANGED_BOUND_NOT_CONSTANT: &str = "ranged_policy_bound_not_constant";
    pub const RANGED_POLICY_WORD_REQUIRED: &str = "ranged_policy_word_required";
    pub const RANGED_ARITY: &str = "ranged_policy_arity";
    pub const RANGED_EMPTY: &str = "ranged_policy_range_empty";
    pub const RANGED_OUTSIDE_WIDTH: &str = "ranged_policy_range_outside_width";
    pub const RANGED_UNSIGNED_NEGATIVE: &str = "ranged_policy_unsigned_negative_bound";
    pub const RANGED_WIDTH_UNSUPPORTED: &str = "ranged_policy_width_unsupported";
    pub const RANGED_LIST_BOUND_NOT_CONSTANT: &str = "ranged_list_bound_not_constant";
    pub const RANGED_LIST_EMPTY: &str = "ranged_list_range_empty";
    pub const RANGED_LIST_BOUND_NEGATIVE: &str = "ranged_list_bound_negative";
    pub const RANGED_LIST_POLICY_WORD: &str = "ranged_list_policy_word";
    /// A list count written without the `size` marker: a bare number or name
    /// (`list<T, 4>`, retired) or a bare range (`list<T, 1…8>`, which would
    /// bound a value). The count is `list<T, size 4>` or `list<T, size 1…8>`.
    pub const LIST_COUNT_NEEDS_SIZE: &str = "list_count_needs_size";
    /// A capacity written without the `size` marker on a bounded head other
    /// than the list: `queue<T, 4>`, `stack<T, 4>`, `string<4>`, `ascii<4>`
    /// (retired; need). The capacity is `queue<T, size 4>` and so on.
    pub const CAPACITY_NEEDS_SIZE: &str = "capacity_needs_size";
    /// A vector lane count written without the `size` marker in the long form:
    /// a bare number, name or affine term (`vector<f32, 4>`, retired). The count
    /// is `vector<f32, size 4>`. The `vf32[4]` shorthand is unchanged: its
    /// brackets already mark an extent.
    pub const VECTOR_WIDTH_NEEDS_SIZE: &str = "vector_width_needs_size";

    /// Diagnostic issue identity for this failure.
    #[must_use]
    pub fn issue(self) -> &'static str {
        match self {
            Self::Params => Self::WIDTH_MARKER_PARAMS,
            Self::RemovedDecimalWidth => Self::REMOVED_DECIMAL_WIDTH,
            Self::ModularWordRequired => Self::MODULAR_WORD_WIDTH_REQUIRED,
            Self::ModularWordShape => Self::MODULAR_WORD_WIDTH_SHAPE,
            Self::ModularWordUnsupported => Self::UNSUPPORTED_MODULAR_WORD_WIDTH,
            Self::ModularWordUnsignedRequired => Self::MODULAR_WORD_UNSIGNED_WIDTH_REQUIRED,
            Self::SaturatingWordRequired => Self::SATURATING_WORD_WIDTH_REQUIRED,
            Self::SaturatingWordShape => Self::SATURATING_WORD_WIDTH_SHAPE,
            Self::SaturatingWordUnsupported => Self::UNSUPPORTED_SATURATING_WORD_WIDTH,
            Self::TrappingWordRequired => Self::TRAPPING_WORD_WIDTH_REQUIRED,
            Self::TrappingWordShape => Self::TRAPPING_WORD_WIDTH_SHAPE,
            Self::TrappingWordUnsupported => Self::UNSUPPORTED_TRAPPING_WORD_WIDTH,
            Self::RangedBoundNotConstant => Self::RANGED_BOUND_NOT_CONSTANT,
            Self::RangedPolicyWordRequired => Self::RANGED_POLICY_WORD_REQUIRED,
            Self::RangedArity => Self::RANGED_ARITY,
            Self::RangedEmpty => Self::RANGED_EMPTY,
            Self::RangedOutsideWidth => Self::RANGED_OUTSIDE_WIDTH,
            Self::RangedUnsignedNegative => Self::RANGED_UNSIGNED_NEGATIVE,
            Self::RangedWidthUnsupported => Self::RANGED_WIDTH_UNSUPPORTED,
            Self::RangedListBoundNotConstant => Self::RANGED_LIST_BOUND_NOT_CONSTANT,
            Self::RangedListEmpty => Self::RANGED_LIST_EMPTY,
            Self::RangedListBoundNegative => Self::RANGED_LIST_BOUND_NEGATIVE,
            Self::RangedListPolicyWord => Self::RANGED_LIST_POLICY_WORD,
        }
    }
}

impl NumericWidth {
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
            // d-markers are valid only on numerus (the surgical
            // FloatOnNumerus relaxation — f-markers still reject). `d64` is
            // the only decimal width; `d32` was removed (D11.7).
            ("d64", Primitive::Numerus) => Ok(Self::D64),
            // Locale-invariant ASCII marker like `d64` (ruling 33); no pack row.
            ("inf", Primitive::Numerus) => Ok(Self::Inf),
            (REMOVED_D32_MARKER, Primitive::Numerus) => Err(WidthError::RemovedDecimal),
            ("f16" | "f32" | "f64" | "bf16", Primitive::Numerus) => Err(WidthError::FloatOnNumerus),
            ("f16", Primitive::Fractus) => Ok(Self::F16),
            ("bf16", Primitive::Fractus) => Ok(Self::BF16),
            ("f32", Primitive::Fractus) => Ok(Self::F32),
            ("f64", Primitive::Fractus) => Ok(Self::F64),
            (
                "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "inf",
                Primitive::Fractus,
            ) => Err(WidthError::IntOnFractus),
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

    /// Whether the width has no value bound (`inf`).
    #[must_use]
    pub fn is_unbounded(self) -> bool {
        self == Self::Inf
    }

    /// Rust primitive spelling for codegen.
    ///
    /// `inf` has no Rust primitive: the marker spelling is returned for
    /// diagnostics and pretty-printing only; the Rust backend carries it as a
    /// bignum and fails closed until that carrier lands (inf track U6r).
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
            Self::D64 => "d64",
            Self::Inf => "inf",
        }
    }

    /// Faber width marker spelling for pretty-print.
    #[must_use]
    pub fn faber_marker(self) -> &'static str {
        self.rust_type()
    }

    /// Family-local widening rank: a larger rank is strictly wider.
    fn signed_rank(self) -> Option<u8> {
        match self {
            Self::I8 => Some(0),
            Self::I16 => Some(1),
            Self::I32 => Some(2),
            Self::I64 => Some(3),
            _ => None,
        }
    }

    /// Family-local widening rank: a larger rank is strictly wider.
    fn unsigned_rank(self) -> Option<u8> {
        match self {
            Self::U8 => Some(0),
            Self::U16 => Some(1),
            Self::U32 => Some(2),
            Self::U64 => Some(3),
            _ => None,
        }
    }

    /// Family-local widening rank: a larger rank is strictly wider.
    ///
    /// `bf16` has no rank: its 8-bit significand is narrower than `f16`'s
    /// 11-bit one while its exponent range is wider, so neither contains the
    /// other and no widening edge exists between them.
    fn float_rank(self) -> Option<u8> {
        match self {
            Self::F16 => Some(0),
            Self::F32 => Some(1),
            Self::F64 => Some(2),
            _ => None,
        }
    }

    /// Decimal-digit scale exponent of the scaled-integer carrier.
    ///
    /// d64 = i64×10⁻⁸ (D11.7): 8 fraction digits, range ±92,233,720,368.54775807.
    #[must_use]
    pub fn decimal_scale(self) -> Option<u32> {
        match self {
            Self::D64 => Some(8),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_decimal(self) -> bool {
        self == Self::D64
    }

    /// Whether `from` may flow into `to` implicitly (assignment, argument
    /// passing): lossless widening inside one number family only.
    ///
    /// Integer -> float never widens implicitly (d11-6 ruling 16); `u64` has
    /// no signed target because it does not fit `i64`.
    #[must_use]
    pub fn is_widening_assignable(from: Self, to: Self) -> bool {
        if from == to {
            return true;
        }
        // Ruling 35: `inf` is the implicit lossless target of every integer
        // width (including `u64`) and widens into nothing.
        if to == Self::Inf {
            return from.is_signed_int() || from.is_unsigned_int();
        }
        if let (Some(a), Some(b)) = (from.signed_rank(), to.signed_rank()) {
            return a <= b;
        }
        if let (Some(a), Some(b)) = (from.unsigned_rank(), to.unsigned_rank()) {
            return a <= b;
        }
        // Unsigned -> strictly wider signed is lossless (u64 has none).
        if let (Some(a), Some(b)) = (from.unsigned_rank(), to.signed_rank()) {
            return a < b;
        }
        if let (Some(a), Some(b)) = (from.float_rank(), to.float_rank()) {
            return a <= b;
        }
        false
    }

    /// Whether an explicit `↦` from `from` to `to` is ordinary widening (not a
    /// narrowing): everything implicitly assignable, plus the signed-integer
    /// chain into `f64`, which needs `↦` because it can round above 2^53.
    #[must_use]
    pub fn is_conversio_widening(from: Self, to: Self) -> bool {
        // Unsigned -> signed is an implicit edge but not an ordinary tensor
        // element widening; that pair set is pinned by the tensor lattice.
        let unsigned_to_signed = from.unsigned_rank().is_some() && to.signed_rank().is_some();
        (Self::is_widening_assignable(from, to) && !unsigned_to_signed)
            || (from.is_signed_int() && to == Self::F64)
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

/// Conversio hints, spelled in the `via` clause after the target (D11.9).
///
/// `Hex` / `Bin` / `Oct` are text-parse radices. `Be` / `Le` are endian
/// unpack rules for `octeti` windows — same clause, not bases. (`Bits`
/// retired with need: a bit pattern is a `bN` value, written
/// `↦ bN`.) `Code` (D10.6) reads a `littera` as its code point
/// (`'A' ↦ u32 via Code`); it is not a radix either. Each hint is accepted in
/// its title-case spelling and in lower case.
#[must_use]
pub fn is_conversio_radix_hint(marker: &str) -> bool {
    matches!(
        marker,
        "hex" | "Hex" | "bin" | "Bin" | "oct" | "Oct" | "be" | "Be" | "le" | "Le"
    ) || is_code_point_hint(marker)
}

/// D10.6 code-point hint: `'A' ↦ u32 via Code` and `65 ↦ littera via Code`.
#[must_use]
pub fn is_code_point_hint(marker: &str) -> bool {
    marker == "code" || marker == "Code"
}

/// Ascii format-pack hints: `n ↦ ascii<N> via Hex|Bin|Oct`.
///
/// Distinct from [`is_conversio_radix_hint`]: `Be` / `Le` are endian unpack
/// rules, never ascii digit bases. The type argument of a bounded family stays capacity.
#[must_use]
pub fn is_ascii_format_hint(marker: &str) -> bool {
    matches!(marker, "hex" | "Hex" | "bin" | "Bin" | "oct" | "Oct")
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

/// Whether `name` is a bare width-marker type spelling: live sugar (`f32`) or
/// the removed `d32`, which stays a type-position name so its use reports the
/// dedicated removal diagnostic instead of an unknown identifier.
#[must_use]
pub fn is_standalone_width_name(name: &str) -> bool {
    name == REMOVED_D32_MARKER
        || BitsWidth::from_marker(name).is_some()
        || standalone_width_type(name).is_some()
}

/// Whether `name` is bare width sugar (`f32`), tensor sugar (`tf32`), vector
/// sugar (`vf32`), matrix sugar (`mf32`), sparsa sugar (`sf32`), or lista sugar (`lf32`).
#[must_use]
pub fn is_numeric_width_type_sugar(name: &str) -> bool {
    is_standalone_width_name(name)
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

/// The `inf` marker is never prefixed sugar (ruling 32): `linf`, `sinf`, …
/// stay ordinary identifiers (`sinf` is C sine, `linf` the L∞ norm), and
/// `tinf`/`vinf`/`minf` would name device shapes ruling 39 rejects anyway.
fn width_prefixed_sugar_marker(name: &str, prefix: char) -> Option<&str> {
    let marker = name.strip_prefix(prefix)?;
    if marker == NumericWidth::Inf.faber_marker() {
        return None;
    }
    standalone_width_type(marker).map(|_| marker)
}

/// Lower standalone width-marker type syntax when `name` is a reserved marker spelling.
pub fn lower_standalone_width_type(
    name: &str,
    params: &[radix_syntax::TypeExpr],
    types: &mut TypeTable,
    report: &mut dyn FnMut(WidthLowerIssue),
) -> Option<TypeId> {
    if name == REMOVED_D32_MARKER {
        report(WidthLowerIssue::RemovedDecimalWidth);
        return Some(types.intern(Type::Error));
    }
    if let Some(bits) = BitsWidth::from_marker(name) {
        if !params.is_empty() {
            report(WidthLowerIssue::Params);
            return Some(types.intern(Type::Error));
        }
        return Some(types.bits(bits));
    }
    let (prim, width) = standalone_width_type(name)?;
    if !params.is_empty() {
        report(WidthLowerIssue::Params);
        return Some(types.intern(Type::Error));
    }
    Some(types.sized_numeric(prim, width))
}

/// Lower the compiler-owned modular word family.
///
/// Admits the integer widths i8…i64 and u8…u64 (D11.8: signed wrapping is the
/// ring Z/2^W with signed representatives); float and decimal markers reject
/// with their own structured issues.
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
            family: MarkerFamily::Modulus,
            var: InferVar(params[0].span.start),
        });
    }
    let Some(marker) = type_expr_width_marker(&params[0], interner) else {
        report(WidthLowerIssue::ModularWordShape);
        return types.intern(Type::Error);
    };
    match NumericWidth::from_marker(marker, Primitive::Numerus) {
        Ok(width) if width.is_unsigned_int() || width.is_signed_int() => {
            types.intern(Type::ModularWord(width))
        }
        // Ruling 32: `wrapping<inf>` names the unbounded integer, which has
        // nothing to wrap; it is the same type as bare `inf`.
        Ok(NumericWidth::Inf) => types.sized_numeric(Primitive::Numerus, NumericWidth::Inf),
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

/// Lower the compiler-owned saturating family (D11.1, N7e).
///
/// Admits every integer width i8…i64 / u8…u64 (arithmetic on the result
/// clamps at both ends of the width) and the float widths f16, bf16, f32 and
/// f64 (the policy-float cell, D5: IEEE arithmetic, its own type identity).
/// Decimal markers reject.
pub fn lower_saturating_word(
    params: &[radix_syntax::TypeExpr],
    interner: &radix_lexer::Interner,
    types: &mut TypeTable,
    report: &mut dyn FnMut(WidthLowerIssue),
) -> TypeId {
    if params.len() != 1 {
        report(WidthLowerIssue::SaturatingWordRequired);
        return types.intern(Type::Error);
    }
    if matches!(params[0].kind, radix_syntax::TypeExprKind::Infer) {
        return types.intern(Type::MarkerHole {
            family: MarkerFamily::Saturatus,
            var: InferVar(params[0].span.start),
        });
    }
    let Some(marker) = type_expr_width_marker(&params[0], interner) else {
        report(WidthLowerIssue::SaturatingWordShape);
        return types.intern(Type::Error);
    };
    match NumericWidth::from_marker(marker, Primitive::Numerus) {
        Ok(width) if width.is_signed_int() || width.is_unsigned_int() => {
            types.intern(Type::SaturatingWord(width))
        }
        // Ruling 32: `saturating<inf>` is bare `inf` (no bound to clamp at).
        Ok(NumericWidth::Inf) => types.sized_numeric(Primitive::Numerus, NumericWidth::Inf),
        Err(WidthError::FloatOnNumerus) => {
            policy_float_cell(FloatPolicy::Saturating, marker, types).unwrap_or_else(|| {
                report(WidthLowerIssue::SaturatingWordUnsupported);
                types.intern(Type::Error)
            })
        }
        _ => {
            report(WidthLowerIssue::SaturatingWordUnsupported);
            types.intern(Type::Error)
        }
    }
}

/// One lowered end of a ranged store policy's range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeEnd {
    /// An integer literal.
    Value(i128),
    /// `∞`: no bound on the upper side.
    PosInf,
    /// `-∞`: no bound on the lower side.
    NegInf,
}

/// Resolve the range of a ranged integer cell against its width.
///
/// `inclusive` is `lo…hi`; the half-open `lo‥hi` holds `lo` through `hi - 1`.
/// `∞` / `-∞` mean no bound on that side, so they resolve to the width's own
/// end. The result is the inclusive pair a [`Type::RangedInt`] carries, always
/// inside the width. Unsigned widths need `lo ≥ 0`; signed widths take
/// negative bounds.
///
/// # Errors
/// The named [`WidthLowerIssue`] for an empty range, a bound outside the
/// width, a negative bound on an unsigned width, `∞` on the wrong side, or a
/// float, decimal or `inf` width.
pub fn resolve_ranged_int(
    width: NumericWidth,
    lo: RangeEnd,
    hi: RangeEnd,
    inclusive: bool,
) -> Result<(i128, i128), WidthLowerIssue> {
    if matches!(lo, RangeEnd::PosInf) || matches!(hi, RangeEnd::NegInf) {
        return Err(WidthLowerIssue::RangedOutsideWidth);
    }
    if width.is_unbounded() {
        return Err(WidthLowerIssue::RangedWidthUnsupported);
    }
    let Some((width_lo, width_hi)) = width.integer_range() else {
        return Err(WidthLowerIssue::RangedWidthUnsupported);
    };
    let lo_value = match lo {
        RangeEnd::Value(value) => value,
        _ => width_lo,
    };
    let hi_value = match hi {
        RangeEnd::Value(value) if inclusive => value,
        RangeEnd::Value(value) => value - 1,
        _ => width_hi,
    };
    if matches!(lo, RangeEnd::Value(_)) && width_lo == 0 && lo_value < 0 {
        return Err(WidthLowerIssue::RangedUnsignedNegative);
    }
    if lo_value > hi_value {
        return Err(WidthLowerIssue::RangedEmpty);
    }
    if lo_value < width_lo || hi_value > width_hi {
        return Err(WidthLowerIssue::RangedOutsideWidth);
    }
    Ok((lo_value, hi_value))
}

/// Whether a generic-argument list carries a range argument.
#[must_use]
pub fn has_range_arg(params: &[radix_syntax::GenericArgExpr]) -> bool {
    params
        .iter()
        .any(|param| matches!(param, radix_syntax::GenericArgExpr::Range(_)))
}

/// Lower a store policy with a range, `policy<W, lo‥hi>`.
///
/// `None` when `params` has no range argument, so the caller keeps the
/// ordinary one-width path. Otherwise the width must be the first argument and
/// the range the second; the range is checked against the width, and a range
/// that is the whole width interns as the plain form it abbreviates.
pub fn lower_ranged_word(
    policy: IntPolicy,
    params: &[radix_syntax::GenericArgExpr],
    interner: &radix_lexer::Interner,
    types: &mut TypeTable,
    report: &mut dyn FnMut(WidthLowerIssue),
) -> Option<TypeId> {
    use radix_syntax::{GenericArgExpr, RangeBoundExpr, RangeKind};
    if !has_range_arg(params) {
        return None;
    }
    let mut fail = |issue: WidthLowerIssue, types: &mut TypeTable| {
        report(issue);
        types.intern(Type::Error)
    };
    let [width_arg, GenericArgExpr::Range(range)] = params else {
        return Some(fail(WidthLowerIssue::RangedArity, types));
    };
    let shape_issue = match policy {
        IntPolicy::Trapping => WidthLowerIssue::TrappingWordShape,
        IntPolicy::Wrapping => WidthLowerIssue::ModularWordShape,
        IntPolicy::Saturating => WidthLowerIssue::SaturatingWordShape,
    };
    let marker = match width_arg {
        GenericArgExpr::Type(ty) => type_expr_width_marker(ty, interner),
        GenericArgExpr::Name(ident) => Some(interner.resolve(ident.name)),
        _ => None,
    };
    let Some(marker) = marker else {
        let names_list =
            matches!(width_arg, GenericArgExpr::Type(ty) if type_expr_names_list(ty, interner));
        return Some(fail(
            if names_list {
                WidthLowerIssue::RangedListPolicyWord
            } else {
                shape_issue
            },
            types,
        ));
    };
    let width = match NumericWidth::from_marker(marker, Primitive::Numerus) {
        Ok(width) => width,
        Err(WidthError::FloatOnNumerus) => {
            return Some(fail(WidthLowerIssue::RangedWidthUnsupported, types));
        }
        Err(_) => return Some(fail(shape_issue, types)),
    };
    let end = |bound: &RangeBoundExpr| match bound {
        RangeBoundExpr::Literal { value, .. } => Some(RangeEnd::Value(*value)),
        RangeBoundExpr::Unbounded { negative: true, .. } => Some(RangeEnd::NegInf),
        RangeBoundExpr::Unbounded {
            negative: false, ..
        } => Some(RangeEnd::PosInf),
        RangeBoundExpr::Name { .. } => None,
    };
    let (Some(lo), Some(hi)) = (end(&range.lo), end(&range.hi)) else {
        return Some(fail(WidthLowerIssue::RangedBoundNotConstant, types));
    };
    // `inf` has no width to refine; `trapping<inf, -∞…∞>` is bare `inf`.
    if width.is_unbounded() {
        if lo == RangeEnd::NegInf && hi == RangeEnd::PosInf {
            return Some(types.sized_numeric(Primitive::Numerus, NumericWidth::Inf));
        }
        return Some(fail(WidthLowerIssue::RangedWidthUnsupported, types));
    }
    if width.is_decimal() {
        return Some(fail(WidthLowerIssue::RangedWidthUnsupported, types));
    }
    match resolve_ranged_int(width, lo, hi, matches!(range.kind, RangeKind::Inclusive)) {
        Ok((lo, hi)) => Some(types.ranged_int(policy, width, lo, hi)),
        Err(issue) => Some(fail(issue, types)),
    }
}

/// The policy-float cell of a float marker (`f16`, `bf16`, `f32`, `f64`);
/// `None` when the marker is not a float width.
fn policy_float_cell(policy: FloatPolicy, marker: &str, types: &mut TypeTable) -> Option<TypeId> {
    let width = NumericWidth::from_marker(marker, Primitive::Fractus).ok()?;
    Some(types.intern(Type::PolicyFloat(policy, width)))
}

/// Lower the trapping policy word (D11.8): `exactus<W>` (en `trapping<W>`).
///
/// Names the default overflow policy explicitly, so the result is the very
/// type the bare marker `W` denotes (`trapping<u8>` is `u8`, same `TypeId`).
/// Admits the integer widths and `d64`. A float width is the trapping-float
/// cell (D11.8 cell (e), N7e): its own type, checked where it is stored.
pub fn lower_trapping_word(
    params: &[radix_syntax::TypeExpr],
    interner: &radix_lexer::Interner,
    types: &mut TypeTable,
    report: &mut dyn FnMut(WidthLowerIssue),
) -> TypeId {
    if params.len() != 1 {
        report(WidthLowerIssue::TrappingWordRequired);
        return types.intern(Type::Error);
    }
    let Some(marker) = type_expr_width_marker(&params[0], interner) else {
        report(WidthLowerIssue::TrappingWordShape);
        return types.intern(Type::Error);
    };
    match NumericWidth::from_marker(marker, Primitive::Numerus) {
        Ok(width) => types.sized_numeric(Primitive::Numerus, width),
        Err(WidthError::FloatOnNumerus) => policy_float_cell(FloatPolicy::Trapping, marker, types)
            .unwrap_or_else(|| {
                report(WidthLowerIssue::TrappingWordUnsupported);
                types.intern(Type::Error)
            }),
        Err(_) => {
            report(WidthLowerIssue::TrappingWordUnsupported);
            types.intern(Type::Error)
        }
    }
}

/// Whether a type expression is a `lista<…>` application: a list takes the
/// trapping length range only, never a store policy word around it.
fn type_expr_names_list(ty: &radix_syntax::TypeExpr, interner: &radix_lexer::Interner) -> bool {
    matches!(
        &ty.kind,
        radix_syntax::TypeExprKind::Named(ident, _)
            if crate::CollectionKind::from_name(interner.resolve(ident.name))
                == Some(crate::CollectionKind::Lista)
    )
}

/// Resolve the length range of `list<T, size lo‥hi>` (need, 5/5).
///
/// `inclusive` is `lo…hi`; the half-open `lo‥hi` holds `lo` through `hi - 1`.
/// `∞` is a high end that means no upper bound; `-∞` as a low end means no
/// lower bound (zero). The result is the inclusive `(min, max)` pair a
/// [`Type::RangedArray`] carries, `max` being `None` for no upper end.
///
/// # Errors
/// The named [`WidthLowerIssue`] for a negative end, `∞` as a low end (no list
/// is that long), `-∞` as a high end, or an empty range.
pub fn resolve_ranged_list(
    lo: RangeEnd,
    hi: RangeEnd,
    inclusive: bool,
) -> Result<(u64, Option<u64>), WidthLowerIssue> {
    let min = match lo {
        RangeEnd::Value(value) if value < 0 => {
            return Err(WidthLowerIssue::RangedListBoundNegative);
        }
        RangeEnd::Value(value) => {
            u64::try_from(value).map_err(|_| WidthLowerIssue::RangedListEmpty)?
        }
        RangeEnd::NegInf => 0,
        RangeEnd::PosInf => return Err(WidthLowerIssue::RangedListEmpty),
    };
    let max = match hi {
        RangeEnd::Value(value) if value < 0 => {
            return Err(WidthLowerIssue::RangedListBoundNegative);
        }
        RangeEnd::Value(value) => {
            let top = if inclusive { value } else { value - 1 };
            if top < 0 {
                return Err(WidthLowerIssue::RangedListEmpty);
            }
            Some(u64::try_from(top).map_err(|_| WidthLowerIssue::RangedListEmpty)?)
        }
        RangeEnd::PosInf => None,
        RangeEnd::NegInf => return Err(WidthLowerIssue::RangedListBoundNegative),
    };
    if max.is_some_and(|max| min > max) {
        return Err(WidthLowerIssue::RangedListEmpty);
    }
    Ok((min, max))
}

/// Lower the length range of a list, `list<T, size lo‥hi>`.
///
/// A list length range is always a trapping range, so it takes no policy
/// word. The ends fold at compile time (integer literals and `∞`); the sugar
/// forms keep one identity (`0…∞` is `lista<T>`, `0…N` is `lista<T, magnitudo N>`).
pub fn lower_ranged_list(
    element: TypeId,
    range: &radix_syntax::RangeArgExpr,
    types: &mut TypeTable,
    report: &mut dyn FnMut(WidthLowerIssue),
) -> TypeId {
    use radix_syntax::{RangeBoundExpr, RangeKind};
    let end = |bound: &RangeBoundExpr| match bound {
        RangeBoundExpr::Literal { value, .. } => Some(RangeEnd::Value(*value)),
        RangeBoundExpr::Unbounded { negative: true, .. } => Some(RangeEnd::NegInf),
        RangeBoundExpr::Unbounded {
            negative: false, ..
        } => Some(RangeEnd::PosInf),
        RangeBoundExpr::Name { .. } => None,
    };
    let (Some(lo), Some(hi)) = (end(&range.lo), end(&range.hi)) else {
        report(WidthLowerIssue::RangedListBoundNotConstant);
        return types.intern(Type::Error);
    };
    match resolve_ranged_list(lo, hi, matches!(range.kind, RangeKind::Inclusive)) {
        Ok((min, max)) => types.ranged_array(element, min, max),
        Err(issue) => {
            report(issue);
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

/// Significand bits (including the implicit one) and largest exponent of a
/// float width; `None` for integer and decimal widths.
fn float_format(width: NumericWidth) -> Option<(u32, i32)> {
    match width {
        NumericWidth::F16 => Some((11, 15)),
        NumericWidth::BF16 => Some((8, 127)),
        NumericWidth::F32 => Some((24, 127)),
        NumericWidth::F64 => Some((53, 1023)),
        _ => None,
    }
}

/// Whether the finite magnitude `value` rounds to infinity in a float width
/// (IEEE round-to-nearest). A constant that does is out of the float's finite
/// range (d11-6 ruling 20(2)). `f64` never overflows a finite `f64`.
#[must_use]
pub fn float_overflows(width: NumericWidth, value: f64) -> bool {
    let Some((precision, emax)) = float_format(width) else {
        return false;
    };
    if emax >= 1023 || !value.is_finite() {
        return false;
    }
    // Largest finite value plus half an ulp is the first magnitude that rounds up.
    let threshold = 2f64.powi(emax + 1) - 2f64.powi(emax - precision.cast_signed());
    value.abs() >= threshold
}

/// Whether an integer is exactly representable in a float width (ruling
/// 20(1)): its significant bits fit the significand and it is finite there.
// The cast only feeds an overflow-range check; rounding cannot move a
// magnitude across a float format's finite bound in a way that matters
// once the significand check above has passed.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn float_holds_integer(width: NumericWidth, value: i128) -> bool {
    let Some((precision, _)) = float_format(width) else {
        return false;
    };
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return true;
    }
    let significant = 128 - magnitude.leading_zeros() - magnitude.trailing_zeros();
    significant <= precision && !float_overflows(width, magnitude as f64)
}

/// Effective width for bare or sized numeric types.
#[must_use]
#[allow(clippy::match_same_arms)]
pub fn effective_width(types: &TypeTable, ty: TypeId) -> Option<NumericWidth> {
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => Some(NumericWidth::I64),
        Type::Primitive(Primitive::Fractus) => Some(NumericWidth::F64),
        Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, width) => Some(*width),
        Type::ModularWord(width) | Type::SaturatingWord(width) => Some(*width),
        Type::RangedInt { width, .. } => Some(*width),
        Type::Alias(_, inner) => effective_width(types, *inner),
        _ => None,
    }
}

/// Integer width of a `numerus` type (bare `numerus` is `i64`); `None` for
/// fractus, decimal, modular, and non-numeric types. `inf` is an integer
/// width (ruling 32) whose `integer_range()` is `None`: callers that need a
/// range must branch on `is_unbounded()` first.
#[must_use]
pub fn numerus_integer_width(types: &TypeTable, ty: TypeId) -> Option<NumericWidth> {
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => Some(NumericWidth::I64),
        Type::SizedNumeric(Primitive::Numerus, width)
            if width.integer_range().is_some() || width.is_unbounded() =>
        {
            Some(*width)
        }
        Type::RangedInt {
            policy: IntPolicy::Trapping,
            width,
            ..
        } => Some(*width),
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

/// Failure text of a `fractus ↦ int<W>` conversion that fails its check; the
/// MIR runner and every backend that aborts on it use this exact wording.
pub const FLOAT_INTEGER_CONVERSIO_MESSAGE: &str = "fractus to numerus conversion out of range";

/// D1.11 / D11.8: the integer target width of an explicit `↦` from a float
/// (`fractus`, any float width) into a trapping integer type. Such a conversion
/// truncates toward zero and fails (never saturates or wraps) on NaN, ±Inf, or a
/// truncation outside the width's range. `None` for any other source/target pair.
#[must_use]
pub fn float_integer_conversio(
    types: &TypeTable,
    from: TypeId,
    to: TypeId,
) -> Option<NumericWidth> {
    fn is_float(types: &TypeTable, ty: TypeId) -> bool {
        match types.get(ty) {
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => true,
            Type::Alias(_, inner) => is_float(types, *inner),
            _ => false,
        }
    }
    if !is_float(types, from) {
        return None;
    }
    numerus_integer_width(types, to)
}

/// D11.1: the `(source, target)` integer widths of a `↦` into or out of a
/// `saturatus<W>`, when both sides are integer widths. Such conversions are
/// range-checked like `numerus` narrowing — a conversion never clamps.
#[must_use]
pub fn saturating_integer_conversio(
    types: &TypeTable,
    from: TypeId,
    to: TypeId,
) -> Option<(NumericWidth, NumericWidth)> {
    if !is_saturating_word(types, from) && !is_saturating_word(types, to) {
        return None;
    }
    let integer_width = |ty: TypeId| match types.get(ty) {
        Type::SaturatingWord(width) => Some(*width),
        _ => numerus_integer_width(types, ty),
    };
    let unalias = |ty: TypeId| {
        let mut ty = ty;
        while let Type::Alias(_, inner) = types.get(ty) {
            ty = *inner;
        }
        ty
    };
    Some((integer_width(unalias(from))?, integer_width(unalias(to))?))
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
/// positions. A `numerus<d64>` index would scale-address storage — the
/// bracket index boundary is integer-only.
pub const DECIMAL_INDEX_SCALAR_MSG: &str = "decimal-width numerus cannot index; decimal widths are scaled values, not positions — use an integer width such as i32";

/// decimal-widths D2 (wildcard audit): tensor `media` reduces over a float
/// carrier to avoid integer division; a decimal element has no defined media
/// carrier yet (conversio rows are D3), so it fails closed here.
pub const DECIMAL_TENSOR_MEDIA_MSG: &str =
    "tensor media does not accept decimal-width elements; convert to f64 first";

/// Whether `ty` is a `lista<integer>` usable as a tensor index vector (DEFER-064).
///
/// Accepts integer widths that fit in `i64` (`numerus`, `numerus<u32>`, …);
/// rejects `u64`, non-integer lists (`lista<f32>`, `lista<textus>`, …), and
/// non-list types.
#[must_use]
pub fn is_integer_index_vector(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Array(elem)
        | Type::BoundedArray { element: elem, .. }
        | Type::RangedArray { element: elem, .. } => is_integer_index_scalar(types, *elem),
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
        // `u64` and `inf` do not fit the `i64` index boundary.
        Type::SizedNumeric(Primitive::Numerus, width) => {
            *width != NumericWidth::U64 && !width.is_decimal() && !width.is_unbounded()
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
        // Ruling 39: `inf` has no device layout and is never a tensor element.
        Type::SizedNumeric(_, width) if width.is_unbounded() => false,
        Type::Primitive(Primitive::Numerus | Primitive::Fractus) => true,
        Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _) => true,
        Type::Alias(_, inner) => is_tensor_numeric_element(types, *inner),
        _ => false,
    }
}

/// Whether `ty` is a floating-point tensor element (`fractus` or `fractus<W>`).
///
/// The lifted transcendental math functions (`radix`, `exponentia`, ...) take
/// float elements only, as their scalar functions take `fractus` only: an
/// integer element crosses to a float family with an explicit `↦`.
#[must_use]
pub fn is_tensor_float_element(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => true,
        Type::Alias(_, inner) => is_tensor_float_element(types, *inner),
        _ => false,
    }
}

/// Issue name of a tensor-family element that is the unbounded `inf`
/// (ruling 39(a)): a tensor element must have a device layout.
pub const TENSOR_ELEMENT_UNBOUNDED_ISSUE: &str = "tensor_element_unbounded";

/// Whether `ty` is the unbounded integer `inf` (through aliases). The
/// tensor/sparsa/vector/matrix lowerers reject it by name before the general
/// numeric-element gate.
#[must_use]
pub fn is_unbounded_integer_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::SizedNumeric(Primitive::Numerus, width) => width.is_unbounded(),
        Type::Alias(_, inner) => is_unbounded_integer_type(types, *inner),
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
            | "subtracta"
            | "multiplicata"
            | "maximus"
            | "minimus"
            | "matmul"
            | "summa"
            | "media"
            | "silu"
            // K5: the lifted `abs` takes any numeric element; the float-only
            // members are gated separately (`is_tensor_float_element`).
            | "absolutum"
    )
}

/// The effective widths of two numeric types that share one arithmetic policy
/// family; `None` on a policy mismatch or a non-numeric side.
///
/// WHY: modular and saturating words share an integer carrier with `numerus`,
/// but their arithmetic policy is part of the type family. Only an explicit
/// `↦` conversion can cross that boundary.
fn same_family_widths(
    types: &TypeTable,
    from: TypeId,
    to: TypeId,
) -> Option<(NumericWidth, NumericWidth)> {
    if is_modular_word(types, from) != is_modular_word(types, to)
        || is_saturating_word(types, from) != is_saturating_word(types, to)
    {
        return None;
    }
    match (effective_width(types, from), effective_width(types, to)) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    }
}

/// Whether two numeric types are assignable via equality or widening.
#[must_use]
pub fn is_numeric_assignable(types: &TypeTable, from: TypeId, to: TypeId) -> bool {
    // A ranged destination takes a value without a check only from a ranged
    // cell of the same policy and width whose range it contains; every other
    // value lands through a store that applies the range.
    if let Some((policy, width, lo, hi)) = types.ranged_int_parts(to) {
        return types.ranged_int_parts(from).is_some_and(
            |(from_policy, from_width, from_lo, from_hi)| {
                from_policy == policy && from_width == width && lo <= from_lo && from_hi <= hi
            },
        );
    }
    same_family_widths(types, from, to)
        .is_some_and(|(a, b)| NumericWidth::is_widening_assignable(a, b))
}

/// Whether `from` -> `to` is ordinary widening for an explicit `↦` (looser
/// than implicit assignability: signed integers may convert into `f64`).
fn is_numeric_conversio_widening(types: &TypeTable, from: TypeId, to: TypeId) -> bool {
    same_family_widths(types, from, to)
        .is_some_and(|(a, b)| NumericWidth::is_conversio_widening(a, b))
}

fn is_saturating_word(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::SaturatingWord(_)
        | Type::RangedInt {
            policy: IntPolicy::Saturating,
            ..
        } => true,
        Type::Alias(_, inner) => is_saturating_word(types, *inner),
        _ => false,
    }
}

fn is_modular_word(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::ModularWord(_)
        | Type::RangedInt {
            policy: IntPolicy::Wrapping,
            ..
        } => true,
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
    if is_numeric_conversio_widening(types, from, to) {
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
        if from == to || Self::is_conversio_widening(from, to) {
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

    /// Decimal digits of the width's inclusive minimum and its exclusive
    /// maximum (`max + 1`, a power of two exact in `f64`). A truncated float `t`
    /// fits when `min <= t && t < max_exclusive`. `None` for non-integer widths.
    #[must_use]
    pub fn float_truncation_bounds(self) -> Option<(String, String)> {
        let (lo, hi) = self.integer_range()?;
        Some((lo.to_string(), (hi + 1).to_string()))
    }
}

#[cfg(test)]
#[path = "numeric_width_test.rs"]
mod tests;

#[cfg(test)]
#[path = "decimal_width_test.rs"]
mod decimal_width_test;

#[cfg(test)]
#[path = "inf_width_test.rs"]
mod inf_width_test;
