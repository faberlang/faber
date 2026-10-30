//! Conversion, cast, and reference-shape checks.
//!
//! This file owns expression forms that intentionally change the apparent type
//! of a value: dereference, runtime conversion (`↦` / `conversio`), and typed
//! conversion/constructor syntax (`∷` / `verte`). The pass validates only the
//! semantic contracts it can prove from HIR and the `TypeTable`; representation
//! details, fallback coercions, and target-specific conversion code remain
//! backend work.
//!
//! ## `↦` (conversio)
//!
//! Operand checking is intentionally light: backends own parse/truthiness/narrowing
//! lowering. Regex targets are the exception — no literal form, so `⊥` default
//! is rejected here.
//!
//! ## `∷` (verte)
//!
//! Target shape drives validation. `valor ∷ T` is rejected for every `T` —
//! dynamic JSON shape is not assertable at compile time; use `↦ T` (with `⊥`
//! for recovery) instead.
//!
//! ERROR AND ESCAPE POLICY
//! =======================
//! Invalid operands emit diagnostics and return the shared error type where the
//! expression cannot be trusted. Explicit targets are otherwise respected,
//! including `ignotum` and union-like map values when the surrounding code has
//! intentionally chosen an escape or broad shape.

use super::store::StoreSite;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::HirStoreKind;

use crate::lista_tensor_conversio::{
    ListaToTensorError, RectangularListaLiteralError, analyze_rectangular_lista_literal,
    ensure_lista_numerus_type, is_numeric_element, padded_literal_facts,
    resolve_bounded_lista_padded_shape, validate_lista_to_tensor_source,
};

const ISSUE_INTERVALLUM_CLAMP_RECOVERY: &str = "intervallum_clamp_recovery_unsupported";
const ISSUE_INTERVALLUM_TARGET_NOT_RANGE: &str = "intervallum_target_not_range";
const ISSUE_INTERVALLUM_CLAMP_BOUND_MISMATCH: &str = "intervallum_clamp_bound_mismatch";
const ISSUE_INTERVALLUM_VALUE_STEP_UNSUPPORTED: &str = "intervallum_value_step_unsupported";
const ISSUE_INTERVALLUM_HELPER_BOUND_UNSUPPORTED: &str = "intervallum_helper_bound_unsupported";
const ISSUE_SERMO_MATERIALIZE_TARGET_UNSUPPORTED: &str = "sermo_materialize_target_unsupported";
const ISSUE_INTERVALLUM_MATERIALIZE_LISTA_ELEMENT_MISMATCH: &str =
    "intervallum_materialize_lista_element_mismatch";
const ISSUE_INTERVALLUM_MATERIALIZE_TENSOR_ELEMENT_MISMATCH: &str =
    "intervallum_materialize_tensor_element_mismatch";
const ISSUE_VALOR_TO_REGEX_UNSUPPORTED: &str = "valor_to_regex_unsupported";
const ISSUE_VALOR_TO_SIZED_NUMERIC_UNSUPPORTED: &str = "valor_to_sized_numeric_unsupported";
const ISSUE_VALOR_TO_CURSOR_BOUNDARY: &str = "valor_to_cursor_boundary";
const ISSUE_VALOR_TO_PROMISSUM_BOUNDARY: &str = "valor_to_promissum_boundary";
const ISSUE_PROMISSUM_TO_VALOR_BOUNDARY: &str = "promissum_to_valor_boundary";
const ISSUE_CURSOR_TO_VALOR_BOUNDARY: &str = "cursor_to_valor_boundary";
const ISSUE_CONVERSIO_MAP_KEY_NOT_TEXTUS: &str = "conversio_map_key_not_textus";
const ISSUE_NUMERIC_TO_OCTETI_UNSPECIFIED: &str = "numeric_to_octeti_unspecified";
const ISSUE_OCTETI_TO_NUMERUS_UNSPECIFIED: &str = "octeti_to_numerus_unspecified";
const ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH: &str = "octeti_endian_width_mismatch";
const ISSUE_ASCII_FORMAT_HINT_NOT_CAPACITY: &str = "ascii_format_hint_not_capacity";
const ISSUE_ASCII_LITERAL_TOO_LONG: &str = "ascii_literal_too_long";
const ISSUE_TEXTUS_LITERAL_TOO_LONG: &str = "textus_literal_too_long";
const ISSUE_ASCII_PACK_WIDTH_OVERFLOW: &str = "ascii_pack_width_overflow";
const ISSUE_ASCII_PACK_NEGATIVE_SOURCE: &str = "ascii_pack_negative_source";
const ISSUE_OCTETI_TO_FRACTUS_UNSPECIFIED: &str = "octeti_to_fractus_unspecified";
const ISSUE_OCTETI_TO_TENSOR_UNSPECIFIED: &str = "list_b8_to_tensor_unspecified";
const ISSUE_OCTETI_TO_TENSOR_ELEMENT_UNSUPPORTED: &str = "list_b8_to_tensor_element_unsupported";
const ISSUE_OCTETI_TO_TENSOR_LENGTH_MISMATCH: &str = "list_b8_to_tensor_length_mismatch";
const ISSUE_OCTETI_TO_TENSOR_RECORD_BIG_ENDIAN: &str = "list_b8_to_tensor_record_big_endian";
const ISSUE_TENSOR_TO_OCTETI_UNSPECIFIED: &str = "tensor_to_list_b8_unspecified";
const ISSUE_TENSOR_TO_OCTETI_ELEMENT_UNSUPPORTED: &str = "tensor_to_list_b8_element_unsupported";
// decimal-widths D2 (wildcard audit): the conversio `via` hints are
// integer-family machinery. Radix text parse (Hex/Bin/Oct), endian unpack
// (Be/Le) all reject decimal targets/sources with named teaching issues
// instead of a generic mismatch.
const ISSUE_DECIMAL_RADIX_HINT_REJECTED: &str = "decimal_radix_hint_rejected";
const ISSUE_DECIMAL_ENDIAN_HINT_REJECTED: &str = "decimal_endianness_hint_rejected";
// Need: a float or decimal does not become an integer bare; the
// rounding is named (`via Floor|Ceil|HalfEven|Trunc`).
const ISSUE_FLOAT_TO_INTEGER_REQUIRES_ROUNDING: &str = "float_to_integer_requires_rounding";
const ISSUE_CONVERSIO_VIA_UNKNOWN_HINT: &str = "conversio_via_unknown_hint";
const ISSUE_OCTETI_TO_BIVALENS_UNSUPPORTED: &str = "octeti_to_bivalens_unsupported";
const ISSUE_LISTA_TO_TEXTUS_UNSUPPORTED: &str = "lista_to_textus_unsupported";
const ISSUE_TEXTUS_TO_LISTA_UNSUPPORTED: &str = "textus_to_lista_unsupported";
const ISSUE_PROMISSUM_TARGET_BOUNDARY: &str = "promissum_target_boundary";
const ISSUE_PROMISSUM_SOURCE_BOUNDARY: &str = "promissum_source_boundary";
const ISSUE_SPARSA_COLLECTION_UNSUPPORTED: &str = "sparsa_collection_unsupported";
const ISSUE_SPARSA_TENSOR_ELEMENT_MISMATCH: &str = "sparsa_tensor_element_mismatch";
const ISSUE_SPARSA_TENSOR_SHAPE_MISMATCH: &str = "sparsa_tensor_shape_mismatch";
const ISSUE_TENSOR_TO_SCALAR_UNSUPPORTED: &str = "tensor_to_scalar_unsupported";
const ISSUE_SCALAR_TO_TENSOR_UNSUPPORTED: &str = "scalar_to_tensor_unsupported";
const ISSUE_TENSOR_SHAPE_MISMATCH: &str = "tensor_shape_mismatch";
const ISSUE_TENSOR_ELEMENT_MISMATCH: &str = "tensor_element_mismatch";
const ISSUE_INTERVALLUM_CLAMP_OPERAND_TYPE_MISMATCH: &str =
    "intervallum_clamp_operand_type_mismatch";
const ISSUE_CONVERSIO_FAILURE_CHANNEL_TYPE_MISMATCH: &str =
    "conversio_failure_channel_type_mismatch";
const ISSUE_CONVERSIO_RECOVERY_TYPE_MISMATCH: &str = "conversio_recovery_type_mismatch";
const ISSUE_BIVALENS_RECOVERY_UNSUPPORTED: &str = "bivalens_recovery_unsupported";
const ISSUE_WORD_CONVERSIO_RECOVERY_UNSUPPORTED: &str = "word_conversio_recovery_unsupported";
const ISSUE_REGEX_CONVERSION_OPERAND_TYPE_MISMATCH: &str = "regex_conversion_operand_type_mismatch";
const ISSUE_CONVERSIO_FAILURE_CHANNEL_TEXTUS_REQUIRED: &str =
    "conversio_failure_channel_textus_required";
const ISSUE_CONVERSIO_FAILURE_CHANNEL_VALUE_REQUIRED: &str =
    "conversio_failure_channel_value_required";
const ISSUE_FAILABLE_CONVERSIO_REQUIRES_HANDLER: &str = "failable_conversio_requires_handler";
const ISSUE_REGEX_OPERAND_UNSUPPORTED: &str = "regex_operand_unsupported";
const ISSUE_REGEX_RECOVERY_UNSUPPORTED: &str = "regex_recovery_unsupported";
const ISSUE_UNSUPPORTED_CONVERSIO: &str = "unsupported_conversio";
const ISSUE_RECORD_CONVERSION_REQUIRES_RECONSTRUCTION: &str =
    "record_conversion_requires_reconstruction";
const ISSUE_RATIO_FIELD_MISSING: &str = "ratio_field_missing";
const ISSUE_UNKNOWN_RECORD_FIELD: &str = "unknown_record_field";
const ISSUE_FIELD_INITIALIZER_TYPE_MISMATCH: &str = "field_initializer_type_mismatch";
const ISSUE_VALOR_VERTE_UNSUPPORTED: &str = "valor_verte_unsupported";
pub(super) const ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE: &str =
    "inferred_constructor_no_expected_type";
pub(super) const ISSUE_INFERRED_CONSTRUCTOR_ENTRIES_NEED_MAP: &str =
    "inferred_constructor_entries_need_map";
pub(super) const ISSUE_INFERRED_CONSTRUCTOR_FIELDS_FOR_MAP: &str =
    "inferred_constructor_fields_for_map";
pub(super) const ISSUE_INFERRED_CONSTRUCTOR_AMBIGUOUS_UNION: &str =
    "inferred_constructor_ambiguous_union";
const ISSUE_NUMERIC_ASCRIPTION_REQUIRES_CONVERSION: &str = "numeric_ascription_requires_conversion";
const ISSUE_NOMINAL_ASCRIPTION_NOT_PROVABLE: &str = "nominal_ascription_not_provable";
const ISSUE_INVALID_CAST: &str = "invalid_cast";
const ISSUE_ARRAY_ELEMENT_TYPE_MISMATCH: &str = "array_element_type_mismatch";
const ISSUE_ARRAY_SPREAD_ELEMENT_TYPE_MISMATCH: &str = "array_spread_element_type_mismatch";
const ISSUE_VECTOR_ELEMENT_TYPE_MISMATCH: &str = "vector_element_type_mismatch";
const ISSUE_MAP_KEY_TYPE_MISMATCH: &str = "map_key_type_mismatch";
const ISSUE_MAP_SPREAD_KEY_TYPE_MISMATCH: &str = "map_spread_key_type_mismatch";
const ISSUE_MAP_SPREAD_VALUE_TYPE_MISMATCH: &str = "map_spread_value_type_mismatch";
const ISSUE_MAP_VALUE_TYPE_MISMATCH: &str = "map_value_type_mismatch";
const ISSUE_LISTA_TO_TABULA_UNSUPPORTED: &str = "lista_to_tabula_unsupported";
const ISSUE_ARRAY_ASCRIPTION_TO_MAP: &str = "array_ascription_to_map";
const ISSUE_ARRAY_ASCRIPTION_TO_TENSOR: &str = "array_ascription_to_tensor";
const ISSUE_CROSS_COLLECTION_UNSUPPORTED: &str = "cross_collection_unsupported";
const ISSUE_LISTA_TO_TENSOR_NESTED: &str = "lista_to_tensor_nested";
const ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH: &str = "lista_to_tensor_element_mismatch";
const ISSUE_LISTA_TO_TENSOR_NON_NUMERIC: &str = "lista_to_tensor_non_numeric";
const ISSUE_LISTA_TO_TENSOR_CAPACITY_MISMATCH: &str = "lista_to_tensor_capacity_mismatch";
const ISSUE_RECTANGULAR_LISTA_LITERAL_TARGET: &str = "rectangular_lista_literal_target";
const ISSUE_RECTANGULAR_LISTA_LITERAL_RAGGED: &str = "rectangular_lista_literal_ragged";
const ISSUE_RECTANGULAR_LISTA_LITERAL_ROW_LENGTH: &str = "rectangular_lista_literal_row_length";
const ISSUE_RECTANGULAR_LISTA_LITERAL_SHAPE: &str = "rectangular_lista_literal_shape";
const ISSUE_RECTANGULAR_LISTA_LITERAL_SPREAD: &str = "rectangular_lista_literal_spread";
const ISSUE_RECTANGULAR_LISTA_LITERAL_LEAF: &str = "rectangular_lista_literal_leaf";
const ISSUE_LISTA_TO_VECTOR_LITERAL_REQUIRED: &str = "lista_to_vector_literal_required";
const ISSUE_LISTA_TO_VECTOR_ELEMENT_MISMATCH: &str = "lista_to_vector_element_mismatch";
const ISSUE_LISTA_TO_VECTOR_LITERAL_WIDTH: &str = "lista_to_vector_literal_width";
const ISSUE_OCTETI_TO_VECTOR_UNSPECIFIED: &str = "octeti_to_vector_unspecified";
const ISSUE_VECTOR_TO_OCTETI_UNSPECIFIED: &str = "vector_to_octeti_unspecified";
const ISSUE_REGISTER_WINDOW_WIDTH_MISMATCH: &str = "register_window_width_mismatch";
const ISSUE_REGISTER_OUT_ELEMENT_MISMATCH: &str = "register_out_element_mismatch";
const ISSUE_REGISTER_BRIDGE_WIDTH_MISMATCH: &str = "register_bridge_width_mismatch";
const ISSUE_REGISTER_BRIDGE_ELEMENT_UNSUPPORTED: &str = "register_bridge_element_unsupported";
const ISSUE_MATRIX_MATERIALIZE_UNSUPPORTED: &str = "matrix_materialize_unsupported";
const ISSUE_DEREF_REQUIRES_REFERENCE: &str = "deref_requires_reference";
const ISSUE_ARRAY_SPREAD_REQUIRES_LISTA: &str = "array_spread_requires_lista";
const ISSUE_VECTOR_CONSTRUCTOR_FORBIDS_SPREAD: &str = "vector_constructor_forbids_spread";
const ISSUE_OBJECT_SPREAD_REQUIRES_TABULA: &str = "object_spread_requires_tabula";
const ISSUE_OBJECT_FIELD_REQUIRES_VALUE: &str = "object_field_requires_value";
const ISSUE_JSON_GENUS_CONVERSION_REQUIRES_JSON: &str =
    "json_genus_conversion_requires_json_annotation";
const ISSUE_CONVERSION_REGISTRY_DUPLICATE_PAIR: &str = "conversio_registry_duplicate_pair";
const ISSUE_CONVERSION_REGISTRY_BUILTIN_SHADOW: &str = "conversio_registry_builtin_shadow";

enum PrimitiveConversioFamily {
    Numerus,
    Fractus,
    Bivalens,
    Textus,
    Ascii,
    Octeti,
    Regex,
    Other,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum CollectionConversioFamily {
    Array,
    Set,
    Map,
    Tensor,
    Cursor,
    Promissum,
}

impl CollectionConversioFamily {
    fn name(self) -> &'static str {
        match self {
            Self::Array => "lista",
            Self::Set => "copia",
            Self::Map => "tabula",
            Self::Tensor => "tensor",
            Self::Cursor => "cursor",
            Self::Promissum => "promissum",
        }
    }
}

fn lista_to_tensor_issue(error: ListaToTensorError) -> Option<&'static str> {
    match error {
        ListaToTensorError::NonListaSource => None,
        ListaToTensorError::NestedLista => Some(ISSUE_LISTA_TO_TENSOR_NESTED),
        ListaToTensorError::ElementMismatch => Some(ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH),
        ListaToTensorError::NonNumericElement => Some(ISSUE_LISTA_TO_TENSOR_NON_NUMERIC),
        ListaToTensorError::CapacityMismatch => Some(ISSUE_LISTA_TO_TENSOR_CAPACITY_MISMATCH),
    }
}

fn endian_read_width_bytes(ty: &Type) -> Option<usize> {
    match ty {
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I16 | NumericWidth::U16) => Some(2),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I32 | NumericWidth::U32)
        | Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => Some(4),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64 | NumericWidth::U64)
        | Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => Some(8),
        _ => None,
    }
}

/// Byte width of one element of an `octeti ↦ tensor<E, [..]> via Be|Le`
/// reinterpretation: the scalar endian row's widths plus the one-byte kinds.
fn octeti_tensor_element_width_bytes(ty: &Type) -> Option<usize> {
    match ty {
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I8 | NumericWidth::U8) => Some(1),
        other => endian_read_width_bytes(other),
    }
}

fn static_integer_magnitude(source: &HirExpression) -> Option<i128> {
    match &source.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => Some(i128::from(*value)),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => match &inner.kind {
            HirExpressionKind::Literal(HirLiteral::Int(value)) => Some(-i128::from(*value)),
            _ => None,
        },
        _ => None,
    }
}

fn static_float_magnitude(source: &HirExpression) -> Option<f64> {
    match &source.kind {
        HirExpressionKind::Literal(
            HirLiteral::Float(value) | HirLiteral::FloatText { value, .. },
        ) => Some(*value),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => match &inner.kind {
            HirExpressionKind::Literal(
                HirLiteral::Float(value) | HirLiteral::FloatText { value, .. },
            ) => Some(-*value),
            _ => None,
        },
        _ => None,
    }
}

/// One arithmetic widening family for F3 `∷` facts (DECISIONS D1.11).
///
/// Ranks order lossless widening inside a family; distinct variants are
/// distinct families, and mixing them is a `↦` conversion. `BF16` stays
/// rankless: no exactness proof without soft-float arithmetic.
#[derive(Copy, Clone, PartialEq, Eq)]
enum NumericFactFamily {
    SignedInt {
        rank: u8,
    },
    UnsignedInt {
        rank: u8,
    },
    Modular {
        rank: u8,
    },
    ModularSigned {
        rank: u8,
    },
    SaturatingSigned {
        rank: u8,
    },
    SaturatingUnsigned {
        rank: u8,
    },
    Float {
        rank: Option<u8>,
    },
    Decimal,
    /// The unbounded `inf` (ruling 40): no rank, no range. Every bounded
    /// trapping width flows into it; it flows into nothing bounded.
    Unbounded,
}

impl NumericFactFamily {
    /// Whether every value of the integer-like family `self` converts into
    /// the trapping integer family `other` without a range check failing.
    fn integer_flows_into(self, other: Self) -> bool {
        // Ruling 40: every integer family, words included, converts into `inf`
        // exactly; `inf` converts into `inf` unchanged.
        if other == Self::Unbounded {
            return !matches!(self, Self::Float { .. } | Self::Decimal);
        }
        let (signed_from, rank_from) = match self {
            Self::SignedInt { rank }
            | Self::ModularSigned { rank }
            | Self::SaturatingSigned { rank } => (true, rank),
            Self::UnsignedInt { rank }
            | Self::Modular { rank }
            | Self::SaturatingUnsigned { rank } => (false, rank),
            _ => return false,
        };
        match other {
            Self::SignedInt { rank } => {
                if signed_from {
                    rank_from <= rank
                } else {
                    rank_from < rank
                }
            }
            Self::UnsignedInt { rank } => !signed_from && rank_from <= rank,
            _ => false,
        }
    }

    /// Whether a value of `self` flows into `other` unchanged.
    fn admits(self, other: Self) -> bool {
        match (self, other) {
            (Self::SignedInt { rank: a }, Self::SignedInt { rank: b })
            | (Self::UnsignedInt { rank: a }, Self::UnsignedInt { rank: b })
            | (Self::Modular { rank: a }, Self::Modular { rank: b })
            | (Self::ModularSigned { rank: a }, Self::ModularSigned { rank: b })
            | (Self::SaturatingSigned { rank: a }, Self::SaturatingSigned { rank: b })
            | (Self::SaturatingUnsigned { rank: a }, Self::SaturatingUnsigned { rank: b }) => {
                a <= b
            }
            (Self::Decimal, Self::Decimal) | (Self::Unbounded, Self::Unbounded) => true,
            // The lattice edge of ruling 35: every bounded trapping width
            // (`u64` included) widens into `inf`.
            (Self::SignedInt { .. } | Self::UnsignedInt { .. }, Self::Unbounded) => true,
            (Self::Float { rank: a }, Self::Float { rank: b }) => {
                matches!((a, b), (Some(x), Some(y)) if x <= y)
            }
            _ => false,
        }
    }
}

fn signed_width_rank(width: NumericWidth) -> Option<u8> {
    match width {
        NumericWidth::I8 => Some(0),
        NumericWidth::I16 => Some(1),
        NumericWidth::I32 => Some(2),
        NumericWidth::I64 => Some(3),
        _ => None,
    }
}

fn unsigned_width_rank(width: NumericWidth) -> Option<u8> {
    match width {
        NumericWidth::U8 => Some(0),
        NumericWidth::U16 => Some(1),
        NumericWidth::U32 => Some(2),
        NumericWidth::U64 => Some(3),
        _ => None,
    }
}

fn float_width_rank(width: NumericWidth) -> Option<u8> {
    match width {
        NumericWidth::F16 => Some(0),
        NumericWidth::F32 => Some(1),
        NumericWidth::F64 => Some(2),
        _ => None,
    }
}

/// A scalar numeric type as F3 sees it: widening family plus the carrier
/// width for the literal-holds clause.
struct NumericFact {
    family: NumericFactFamily,
    width: Option<NumericWidth>,
}

fn numeric_fact(ty: &Type) -> Option<NumericFact> {
    let fact = |family| NumericFact {
        family,
        width: None,
    };
    match ty {
        Type::Primitive(Primitive::Numerus) => Some(fact(NumericFactFamily::SignedInt { rank: 3 })),
        Type::Primitive(Primitive::Fractus) => {
            Some(fact(NumericFactFamily::Float { rank: Some(2) }))
        }
        Type::SizedNumeric(_, width) => {
            let family = if width.is_unbounded() {
                NumericFactFamily::Unbounded
            } else if let Some(rank) = signed_width_rank(*width) {
                NumericFactFamily::SignedInt { rank }
            } else if let Some(rank) = unsigned_width_rank(*width) {
                NumericFactFamily::UnsignedInt { rank }
            } else if width.is_decimal() {
                NumericFactFamily::Decimal
            } else {
                float_width_rank(*width)
                    .map(|rank| NumericFactFamily::Float { rank: Some(rank) })?
            };
            Some(NumericFact {
                family,
                width: Some(*width),
            })
        }
        Type::ModularWord(width) => {
            let family = if let Some(rank) = signed_width_rank(*width) {
                NumericFactFamily::ModularSigned { rank }
            } else {
                NumericFactFamily::Modular {
                    rank: unsigned_width_rank(*width)?,
                }
            };
            Some(NumericFact {
                family,
                width: Some(*width),
            })
        }
        Type::SaturatingWord(width) => {
            let family = if let Some(rank) = signed_width_rank(*width) {
                NumericFactFamily::SaturatingSigned { rank }
            } else {
                NumericFactFamily::SaturatingUnsigned {
                    rank: unsigned_width_rank(*width)?,
                }
            };
            Some(NumericFact {
                family,
                width: Some(*width),
            })
        }
        // A ranged cell is a value of its width in its policy's family; the
        // range itself is judged where a value is stored into it.
        Type::RangedInt { policy, width, .. } => {
            let family = match policy {
                radix_types::IntPolicy::Trapping => {
                    if let Some(rank) = signed_width_rank(*width) {
                        NumericFactFamily::SignedInt { rank }
                    } else {
                        NumericFactFamily::UnsignedInt {
                            rank: unsigned_width_rank(*width)?,
                        }
                    }
                }
                radix_types::IntPolicy::Wrapping => {
                    if let Some(rank) = signed_width_rank(*width) {
                        NumericFactFamily::ModularSigned { rank }
                    } else {
                        NumericFactFamily::Modular {
                            rank: unsigned_width_rank(*width)?,
                        }
                    }
                }
                radix_types::IntPolicy::Saturating => {
                    if let Some(rank) = signed_width_rank(*width) {
                        NumericFactFamily::SaturatingSigned { rank }
                    } else {
                        NumericFactFamily::SaturatingUnsigned {
                            rank: unsigned_width_rank(*width)?,
                        }
                    }
                }
            };
            Some(NumericFact {
                family,
                width: Some(*width),
            })
        }
        // N7e D3: a conversion into or out of a float cell is the conversion
        // of the bare float of its width, so the cell is that float here.
        Type::PolicyFloat(_, width) => Some(NumericFact {
            family: NumericFactFamily::Float {
                rank: float_width_rank(*width),
            },
            width: Some(*width),
        }),
        _ => None,
    }
}

/// Inclusive value range an integer-family width holds; `None` for
/// non-integer widths.
fn integer_width_range(width: NumericWidth) -> Option<(i128, i128)> {
    match width {
        NumericWidth::I8 => Some((i128::from(i8::MIN), i128::from(i8::MAX))),
        NumericWidth::I16 => Some((i128::from(i16::MIN), i128::from(i16::MAX))),
        NumericWidth::I32 => Some((i128::from(i32::MIN), i128::from(i32::MAX))),
        NumericWidth::I64 => Some((i128::from(i64::MIN), i128::from(i64::MAX))),
        NumericWidth::U8 => Some((0, i128::from(u8::MAX))),
        NumericWidth::U16 => Some((0, i128::from(u16::MAX))),
        NumericWidth::U32 => Some((0, i128::from(u32::MAX))),
        NumericWidth::U64 => Some((0, i128::from(u64::MAX))),
        _ => None,
    }
}

/// Whether a float rounded by the `via` mode lies in `lo..=hi`, decided in
/// the exact `i128` domain: converting the bounds to `f64` rounds up for the
/// 64-bit widths (2^63, 2^64), so compare the rounded value after checked
/// integer conversion instead. NaN and infinities never fit. `mode` is the
/// named rounding (`Floor`, `Ceil`, `HalfEven`, `Trunc`); no hint rounds toward
/// zero, as the runner does.
fn float_rounding_fits(value: f64, mode: Option<&str>, (lo, hi): (i128, i128)) -> bool {
    let rounded = match mode {
        Some("floor" | "Floor") => value.floor(),
        Some("ceil" | "Ceil") => value.ceil(),
        Some("half_even" | "HalfEven") => value.round_ties_even(),
        _ => value.trunc(),
    };
    if !rounded.is_finite() {
        return false;
    }
    let Some(integer) = radix_types::Magnus::from_f64(rounded)
        .ok()
        .and_then(|value| value.to_i128())
    else {
        return false;
    };
    (lo..=hi).contains(&integer)
}

/// Whether a float width can hold the value after rounding. `f16`/`bf16` have
/// no rounding proof without soft-float arithmetic, so they hold nothing here.
fn float_width_holds(width: NumericWidth, value: f64) -> bool {
    match width {
        NumericWidth::F64 => true,
        NumericWidth::F32 => {
            // The exact overflow midpoint between the largest finite f32 and
            // infinity; values below it round to a finite f32.
            let overflow_midpoint = 2.0_f64.powi(128) - 2.0_f64.powi(103);
            value.is_finite() && value.abs() < overflow_midpoint
        }
        _ => false,
    }
}

fn ascii_pack_digit_count(value: u128, radix: u32) -> u64 {
    if value == 0 {
        return 1;
    }
    let mut remaining = value;
    let mut digits = 0u64;
    while remaining > 0 {
        remaining /= u128::from(radix);
        digits += 1;
    }
    digits
}

fn static_exclusive_range_len(index: &HirExpression) -> Option<usize> {
    let HirExpressionKind::Intervallum {
        start, end, kind, ..
    } = &index.kind
    else {
        return None;
    };
    let HirExpressionKind::Literal(HirLiteral::Int(start)) = start.kind else {
        return None;
    };
    let HirExpressionKind::Literal(HirLiteral::Int(end)) = end.kind else {
        return None;
    };
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    let end = match kind {
        radix_hir::HirRangeKind::Exclusive => end,
        radix_hir::HirRangeKind::Inclusive => end.checked_add(1)?,
    };
    end.checked_sub(start)
}

fn rectangular_lista_literal_issue(error: RectangularListaLiteralError) -> &'static str {
    match error {
        RectangularListaLiteralError::NonNestedLiteral => ISSUE_LISTA_TO_TENSOR_NESTED,
        RectangularListaLiteralError::NonLiteralTarget => ISSUE_RECTANGULAR_LISTA_LITERAL_TARGET,
        RectangularListaLiteralError::Ragged => ISSUE_RECTANGULAR_LISTA_LITERAL_RAGGED,
        RectangularListaLiteralError::RowLengthMismatch => {
            ISSUE_RECTANGULAR_LISTA_LITERAL_ROW_LENGTH
        }
        RectangularListaLiteralError::ShapeMismatch => ISSUE_RECTANGULAR_LISTA_LITERAL_SHAPE,
        RectangularListaLiteralError::Spread => ISSUE_RECTANGULAR_LISTA_LITERAL_SPREAD,
        RectangularListaLiteralError::NonLiteralLeaf => ISSUE_RECTANGULAR_LISTA_LITERAL_LEAF,
        RectangularListaLiteralError::AbsentLeafType
        | RectangularListaLiteralError::ElementMismatch => ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH,
    }
}

/// Build the dual-span structured diagnostic for one coherence-registry
/// enrollment conflict, mirroring the union duplicate-arm check: the error
/// span merges both declarations and both source spans ride along as
/// `first_span` / `second_span` args.
pub(crate) fn conversion_registry_conflict_error(
    pair: crate::scope::ConversionPair,
    conflict: crate::scope::ConversionRowConflict,
) -> SemanticError {
    let (issue, first_span, second_span) = match conflict {
        crate::scope::ConversionRowConflict::Duplicate {
            first_span,
            second_span,
        } => (
            ISSUE_CONVERSION_REGISTRY_DUPLICATE_PAIR,
            first_span,
            second_span,
        ),
        crate::scope::ConversionRowConflict::BuiltinShadow {
            builtin_span,
            user_span,
        } => (
            ISSUE_CONVERSION_REGISTRY_BUILTIN_SHADOW,
            builtin_span,
            user_span,
        ),
    };
    let dual_span = first_span.merge(second_span);
    SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, dual_span)
        .with_arg("issue", issue)
        .with_arg("source", pair.source.0.to_string())
        .with_arg("destination", pair.destination.0.to_string())
        .with_arg(
            "first_span",
            format!("{}..{}", first_span.start, first_span.end),
        )
        .with_arg(
            "second_span",
            format!("{}..{}", second_span.start, second_span.end),
        )
}

impl TypeChecker<'_> {
    pub fn check_deref(&mut self, expr: &mut HirExpression, span: radix_lexer::Span) -> TypeId {
        let expr_ty = self.check_expr(expr);
        if let Type::Ref(_, inner) = self.types.get(self.resolve_inferred_type(expr_ty)) {
            *inner
        } else {
            self.invalid_operand_types(ISSUE_DEREF_REQUIRES_REFERENCE, span);
            self.error_type
        }
    }

    /// Checks a runtime value conversion (`↦` / `conversio`).
    ///
    /// The source is always visited for diagnostics. An inline `⊥` default
    /// expression must be a value of the declared target type `T`. This function
    /// returns the declared target type rather than inventing a common type
    /// between source and recovery. A built-in `ordo` row or registered
    /// `@ conversio` pair that serves the site is written to `route`.
    pub fn check_conversio(
        &mut self,
        source: &mut HirExpression,
        target: &mut HirConversioTarget,
        params: &[Symbol],
        mut recovery: Option<&mut HirExpression>,
        route: &mut Option<radix_hir::HirConversioRoute>,
        span: radix_lexer::Span,
    ) -> TypeId {
        *route = None;
        // Ruling 34: an integer constant is untyped and exact, so a constant
        // beyond every 64-bit type is the source of the conversion as an `inf`.
        let source_ty =
            if !self.current_device_function && super::integer_range::is_const_int(source) {
                self.check_const_operand(source)
            } else {
                self.check_expr(source)
            };
        if let HirConversioTarget::Intervallum(operand) = target {
            if recovery.is_some() {
                self.invalid_conversion(ISSUE_INTERVALLUM_CLAMP_RECOVERY, span);
            }
            return self.check_intervallum_clamp_conversio(source_ty, operand, span);
        }

        let HirConversioTarget::Type(target) = target else {
            return self.error_type;
        };
        let target_resolved = self.resolve_inferred_type(*target);
        // Ruling 20 for constants of any size: a constant beyond the float's
        // finite range is a compile error, never infinity.
        if !self.check_float_conversion_constant(source, target_resolved, span) {
            return self.error_type;
        }
        // Need: float/decimal into an integer family names its
        // rounding; a rounding hint on any other pairing is an unknown hint.
        let Ok(rounded) = self.check_rounding_conversio(source_ty, target_resolved, params, span)
        else {
            // The refusal is reported; the declared target keeps the
            // surrounding expression typed instead of cascading mismatches.
            return target_resolved;
        };
        // decimal-widths D2 (wildcard audit): any conversio `via` hint aimed
        // at a decimal numerus target rejects here — radix text parse, endian
        // and unpack are integer-family only.
        if let Some(issue) = self.decimal_target_hint_issue(target_resolved, params) {
            self.invalid_conversion(issue, span);
            return self.error_type;
        }
        // Need unit 1.4a: same-width `bN ↔ uN` rows and the named
        // refusals for every other number pairing.
        if let Some(result_ty) = self.check_bits_conversio(source_ty, target_resolved, params, span)
        {
            if !self.types.equals(result_ty, self.error_type)
                && let Some(recovery) = recovery.as_deref_mut()
            {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return result_ty;
        }
        // Need B4: `lista<bN> ↦ textus` decodes UTF-8/16/32 and
        // `textus ↦ lista<bN>` encodes; `b64` has no text conversion.
        if let Some(result_ty) = self.check_bits_text_conversio(
            source_ty,
            target_resolved,
            params,
            recovery.as_deref_mut(),
            span,
        ) {
            return result_ty;
        }
        // D10.6: `littera` text rows and the `Code` hint.
        if let Some(result_ty) = self.check_littera_conversio(
            source_ty,
            target_resolved,
            params,
            recovery.as_deref_mut(),
            span,
        ) {
            return result_ty;
        }
        // D9.4: built-in `ordo` rows. Pairs outside the three rows fall
        // through to the ordinary rules (and their `unsupported_conversio`).
        if let Some((result_ty, ordo_route)) =
            self.check_ordo_conversio(source_ty, target_resolved, recovery.as_deref_mut(), span)
        {
            *route = ordo_route;
            return result_ty;
        }

        // D11.6 ruling 24: `↦` into `wrapping<W>` / `saturating<W>` applies the
        // target's policy (reduce mod 2^W / clamp) and cannot fail: no `⊥`
        // recovery, no failure channel.
        if self.is_policy_word(target_resolved) {
            if !rounded && !self.is_policy_word_conversio_source(source_ty) {
                self.invalid_conversion(ISSUE_UNSUPPORTED_CONVERSIO, span);
                return self.error_type;
            }
            if recovery.is_some() {
                self.invalid_conversion(ISSUE_WORD_CONVERSIO_RECOVERY_UNSUPPORTED, span);
            } else if self.float_word_conversio_can_fail(source, source_ty)
                && self.current_error.is_some()
            {
                // A float into a word fails only at NaN and ±∞ (need
                // e97acf45): an active channel carries the text payload like
                // any other failable numeric `↦`, so a direct `fac`/`cape`
                // infers `textus` instead of the `ignotum` escape.
                self.check_text_failure_channel(source_ty, target_resolved, span);
            }
            return target_resolved;
        }

        // S0-G2: `↦` dispatch is value-transparent through applied aliases —
        // the shape arms below (vector/matrix/tensor materialization) see the
        // substituted builtin carrier, while a successful conversion keeps
        // returning the written target identity.
        let target_carrier = self.value_carrier_type(target_resolved);

        if let Some(result_ty) = self.check_json_genus_conversio(source_ty, target_resolved, span) {
            self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            return result_ty;
        }

        let faber_served = matches!(
            &source.kind,
            HirExpressionKind::Ad { route, .. } if self.ad_routes.contains_key(route)
        );
        if let Some(result_ty) = self.check_sermo_materialize_conversio(
            source_ty,
            target_resolved,
            recovery.is_none(),
            faber_served,
            span,
        ) {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return result_ty;
        }

        if let Some(result_ty) =
            self.check_intervallum_materialize_conversio(source_ty, target_resolved, span)
        {
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return result_ty;
        }

        if let Some(error_ty) = self.check_valor_conversio(source_ty, target_resolved, span) {
            return error_ty;
        }

        if let Some(result_ty) = self.check_ascii_format_pack_conversio(
            source,
            source_ty,
            target_resolved,
            params,
            recovery.is_some(),
            span,
        ) {
            if !self.types.equals(result_ty, self.error_type) {
                self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            }
            return result_ty;
        }

        if self.is_bounded_ascii_conversio(source_ty, target_resolved) {
            if let Some(error_ty) =
                self.check_bounded_string_literal_overflow(source, target_resolved, span)
            {
                return error_ty;
            }
            self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            return target_resolved;
        }

        if self.is_bounded_textus_conversio(source_ty, target_resolved) {
            if let Some(error_ty) =
                self.check_bounded_string_literal_overflow(source, target_resolved, span)
            {
                return error_ty;
            }
            self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            return target_resolved;
        }

        if self.is_bounded_octeti_conversio(source_ty, target_resolved) {
            self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            return target_resolved;
        }

        if self.is_bounded_lista_conversio(source_ty, target_resolved) {
            // A literal source has a statically known length: outside the
            // target's length range it is a compile-time refusal, not a
            // recovery value.
            if let Some(error_ty) =
                self.check_ranged_literal_conversio(source, target_resolved, span)
            {
                return error_ty;
            }
            self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            return target_resolved;
        }

        // Route C (AEU-5): numeric `lista<T, magnitudo N> ↦ tensor<T, [N]>` is a padded
        // snapshot. The inline `⊥` value names the dead-slot fill (element
        // type `T`), not a target-typed failure recovery — padding always
        // succeeds structurally, and the fill must be a real value, never a
        // read of the carrier's uninitialized slots.
        if let Some((fill_elem, _bound)) =
            resolve_bounded_lista_padded_shape(source_ty, target_resolved, self.types)
                .or_else(|| self.literal_padded_shape(source, source_ty, target_resolved))
        {
            // The MIR padded-snapshot arm lowers its shape carrier as
            // `lista<numerus>`; intern it here like the flat path does.
            self.register_lista_to_tensor_conversio_prereqs();
            if let Some(recovery) = recovery {
                self.note_conversio_recovery_checked(recovery, fill_elem);
            } else {
                self.check_conversio_propagation_channel(source_ty, target_resolved, span);
            }
            return target_resolved;
        }

        if matches!(
            self.types.get(target_resolved),
            Type::Primitive(Primitive::Regex)
        ) && let Some(error_ty) =
            self.check_regex_conversio(source, source_ty, recovery.is_some(), span)
        {
            return error_ty;
        }

        if let Some(error_ty) =
            self.check_lista_to_textus_conversio(source_ty, target_resolved, target_carrier, span)
        {
            return error_ty;
        }

        if let Some(error_ty) =
            self.check_textus_to_lista_conversio(source_ty, target_resolved, target_carrier, span)
        {
            return error_ty;
        }

        if let Some(result_ty) =
            self.check_numeric_to_bounded_octeti_conversio(source_ty, target_resolved, params, span)
        {
            return result_ty;
        }

        if let Some(error_ty) = self.check_rejected_primitive_conversio(
            source,
            source_ty,
            target_resolved,
            params,
            span,
        ) {
            return error_ty;
        }

        if let Some(result_ty) =
            self.check_octeti_to_lista_conversio(source_ty, target_resolved, span)
        {
            if !self.types.equals(result_ty, self.error_type) {
                self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            }
            return result_ty;
        }

        // `tensor<E, [shape]> ↦ octeti via Be|Le`: the infallible inverse of
        // the row below (the byte length is product(shape) x width).
        if let Some(result_ty) =
            self.check_tensor_to_octeti_conversio(source_ty, target_resolved, params, span)
        {
            return result_ty;
        }

        if let Some(result_ty) = self.check_octeti_to_tensor_conversio(
            source,
            source_ty,
            target_resolved,
            target_carrier,
            params,
            span,
        ) {
            if !self.types.equals(result_ty, self.error_type) {
                self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            }
            return result_ty;
        }

        if recovery.is_some()
            && self.is_infallible_numeric_truthiness_conversio(source_ty, target_resolved)
        {
            self.invalid_conversion(ISSUE_BIVALENS_RECOVERY_UNSUPPORTED, span);
        }

        if let Some(error_ty) =
            self.check_promissum_conversio_boundary(source_ty, target_resolved, span)
        {
            return error_ty;
        }

        if let Some(error_ty) = self.check_sparse_tensor_conversio(source_ty, target_carrier, span)
        {
            return error_ty;
        }

        // Ruling 16: `tensor ↔ vector` and `tensor ↔ matrix`
        // are family crossings, so they always need the explicit `↦` — but the
        // crossing itself is exact. This runs before the literal-shaped
        // lista/vector/matrix arms, which would otherwise refuse a tensor
        // source as `lista_to_vector_literal_required`.
        if let Some(result_ty) = self.check_collection_family_bridge_conversio(
            source_ty,
            target_resolved,
            recovery.as_deref_mut(),
            span,
        ) {
            return result_ty;
        }

        if let Some(result_ty) =
            self.check_lista_to_matrix_conversio(source, source_ty, target_carrier, span)
        {
            if !self.types.equals(result_ty, self.error_type)
                && let Some(recovery) = recovery
            {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return self.conversio_written_identity(result_ty, target_carrier, target_resolved);
        }

        if let Some(result_ty) =
            self.check_register_conversio(source, source_ty, target_carrier, params, span)
        {
            if !self.types.equals(result_ty, self.error_type)
                && let Some(recovery) = recovery
            {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return self.conversio_written_identity(result_ty, target_carrier, target_resolved);
        }

        if !matches!(source.kind, HirExpressionKind::Array(_))
            && let Some(result_ty) =
                self.check_bounded_lista_to_vector_conversio(source_ty, target_carrier, span)
        {
            if !self.types.equals(result_ty, self.error_type)
                && let Some(recovery) = recovery
            {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return self.conversio_written_identity(result_ty, target_carrier, target_resolved);
        }

        if let Some(result_ty) =
            self.check_lista_to_vector_conversio(source, source_ty, target_carrier, span)
        {
            if !self.types.equals(result_ty, self.error_type)
                && let Some(recovery) = recovery
            {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            return self.conversio_written_identity(result_ty, target_carrier, target_resolved);
        }

        if let Some(witnessed) =
            super::tensor_index::conversio_value_shape_target(self, source_ty, *target)
        {
            // A value-shape target figura (`flat ↦ tensor<f32, shape>`) is
            // authoritative through its binding's shape witness: rewrite the
            // HIR target so check and MIR lowering share the static extents.
            *target = witnessed;
            let target_resolved = self.resolve_inferred_type(*target);
            if let Some(error_ty) =
                self.check_collection_conversio(source, source_ty, target_resolved, span)
            {
                return error_ty;
            }
            self.settle_conversio_result(recovery, source_ty, target_resolved, span);
            return target_resolved;
        }

        if let Some(error_ty) =
            self.check_collection_conversio(source, source_ty, target_resolved, span)
        {
            return error_ty;
        }

        if let Some(error_ty) = self.check_tensor_conversio(source_ty, target_carrier, span) {
            return error_ty;
        }

        if self.record_conversion_requires_reconstruction(source_ty, target_resolved) {
            // A record and a genus with exactly the same fields convert field
            // by field in either direction; the lowerers dispatch on the pair.
            if self.record_genus_conversion_is_structural(source_ty, target_resolved) {
                self.settle_conversio_result(recovery, source_ty, target_resolved, span);
                return target_resolved;
            }
            self.invalid_conversion(ISSUE_RECORD_CONVERSION_REQUIRES_RECONSTRUCTION, span);
            return self.error_type;
        }

        // CTR-02 / D9.3: a registered user row is the authority for its
        // ordered pair (direct-only lookup — no transitive composition), and
        // the explicit `↦` calls its function. A row whose conversion function
        // declares `⇥ E` makes a bare `↦` failable with that declared channel:
        // it needs a `⊥` default or an active channel that accepts `E`;
        // inline recovery stays target-typed.
        if let Some(row) = self
            .resolver
            .conversion_row_for(self.resolve_inferred_type(source_ty), target_resolved)
            && let Some(fn_id) = row.fn_def_id
        {
            let channel = self.functions.get(&fn_id).and_then(|sig| sig.err);
            if let Some(channel) = channel {
                if let Some(recovery) = recovery {
                    self.note_conversio_recovery_checked(recovery, target_resolved);
                } else if self.current_error.is_none() {
                    if self.reject_unhandled_conversio(source_ty, target_resolved, span) {
                        return self.error_type;
                    }
                } else {
                    self.check_registered_row_channel(channel, span);
                }
            }
            *route = Some(radix_hir::HirConversioRoute::Function {
                def_id: fn_id,
                failable: channel.is_some(),
            });
            return target_resolved;
        }

        // All specialized checks above handle known conversion paths; anything
        // reaching here must be between scalar-like types or identical types.
        if !self
            .types
            .equals(self.resolve_inferred_type(source_ty), target_resolved)
            && !self.conversio_fallback_allowed(source_ty, target_resolved)
        {
            self.invalid_conversion(ISSUE_UNSUPPORTED_CONVERSIO, span);
            return self.error_type;
        }

        if let Some(recovery) = recovery {
            self.note_conversio_recovery_checked(recovery, target_resolved);
        } else {
            self.check_conversio_propagation_channel(source_ty, target_resolved, span);
            if self.numeric_conversio_can_fail(source, source_ty, target_resolved, params) {
                self.check_text_failure_channel(source_ty, target_resolved, span);
            }
        }
        target_resolved
    }

    /// Map a shape-arm success back to the written conversion target (S0-G2).
    ///
    /// The `↦` shape arms dispatch on the value carrier — an applied alias
    /// target expands to its builtin carrier — so a success that returns the
    /// carrier is the written annotation's own conversion succeeding; the
    /// written identity (alias application included) is the result type.
    fn conversio_written_identity(
        &self,
        result: TypeId,
        carrier: TypeId,
        written: TypeId,
    ) -> TypeId {
        if self.types.equals(result, carrier) {
            written
        } else {
            result
        }
    }

    /// Settle a successful `↦` arm: an inline `⊥` value is checked against the
    /// declared target, while a bare conversion propagates through the active
    /// alternate-exit channel.
    fn settle_conversio_result(
        &mut self,
        recovery: Option<&mut HirExpression>,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) {
        if let Some(recovery) = recovery {
            self.note_conversio_recovery_checked(recovery, target_resolved);
        } else {
            self.check_conversio_propagation_channel(source_ty, target_resolved, span);
        }
    }

    /// `x ↦ intervallum` / `r1 ↦ r2`: clamp; value clamp yields bound `T`, range clamp yields `intervallum<T>`.
    fn check_intervallum_clamp_conversio(
        &mut self,
        source_ty: TypeId,
        operand: &mut HirExpression,
        span: radix_lexer::Span,
    ) -> TypeId {
        let operand_ty = self.check_expr(operand);
        let Some(target_intervallum) = self.resolve_intervallum_type(operand_ty, operand.span)
        else {
            self.invalid_conversion(ISSUE_INTERVALLUM_TARGET_NOT_RANGE, operand.span);
            return self.error_type;
        };

        if let Some(source_intervallum) = self.intervallum_type_id(source_ty) {
            if !self.types.equals(source_intervallum, target_intervallum) {
                self.invalid_conversion(ISSUE_INTERVALLUM_CLAMP_BOUND_MISMATCH, span);
                return self.error_type;
            }
            let Some(bound) = self.intervallum_bound_type(target_intervallum) else {
                return self.error_type;
            };
            if !self.is_intervallum_runtime_numerus_bound(bound) {
                self.invalid_operand_types(ISSUE_INTERVALLUM_HELPER_BOUND_UNSUPPORTED, span);
                return self.error_type;
            }
            return target_intervallum;
        }

        // A value clamped into a stepped range would lose the stride: the
        // result is a plain bound value. No ruling gives that a meaning, so
        // refuse by name instead of dropping the step.
        if !self.intervallum_step_is_one(target_intervallum) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_INTERVALLUM_VALUE_STEP_UNSUPPORTED,
                span,
            );
            return self.error_type;
        }

        let Some(bound) = self.intervallum_bound_type(target_intervallum) else {
            return self.error_type;
        };
        if !self.is_intervallum_runtime_numerus_bound(bound) {
            self.invalid_operand_types(ISSUE_INTERVALLUM_HELPER_BOUND_UNSUPPORTED, span);
            return self.error_type;
        }
        self.require_intervallum_bound(source_ty, span);
        self.unify_assign_issue(
            source_ty,
            bound,
            span,
            ISSUE_INTERVALLUM_CLAMP_OPERAND_TYPE_MISMATCH,
        );
        bound
    }

    /// `sermo ↦ T` materialization. Source must be a `sermo` handle; `T` must
    /// be a type the materializer policy supports.
    fn check_sermo_materialize_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        propagates: bool,
        faber_served: bool,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !self.is_sermo_type(source_ty) {
            return None;
        }
        if self.check_typed_sermo_materialize(source_ty, target_resolved, span) == Some(false) {
            return Some(self.error_type);
        }
        if !self.is_sermo_materialize_target(target_resolved) {
            self.invalid_conversion(ISSUE_SERMO_MATERIALIZE_TARGET_UNSUPPORTED, span);
            return Some(self.error_type);
        }
        if propagates {
            self.check_sermo_failure_channel(source_ty, target_resolved, faber_served, span);
        }
        Some(target_resolved)
    }

    fn is_sermo_type(&self, ty: TypeId) -> bool {
        self.sermo_shape(ty).is_some()
    }

    fn is_sermo_materialize_target(&self, ty: TypeId) -> bool {
        let ty = self.resolve_inferred_type(ty);
        matches!(
            self.types.get(ty),
            Type::Param(_)
                | Type::Primitive(
                    Primitive::Vacuum
                        | Primitive::Valor
                        | Primitive::Numerus
                        | Primitive::Fractus
                        | Primitive::Bivalens
                        | Primitive::Textus
                        | Primitive::Ascii
                        | Primitive::Octeti
                        | Primitive::Instans
                )
                | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
                | Type::SizedInstans(_)
                | Type::Array(_)
                | Type::BoundedArray { .. }
        )
    }

    /// `intervallum<T> ↦ lista<T>` / `intervallum<T> ↦ tensor<T>` materialization.
    fn check_intervallum_materialize_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_intervallum = self.intervallum_type_id(source_ty)?;
        let bound = self.intervallum_bound_type(source_intervallum)?;
        match self.types.get(target_resolved) {
            Type::Array(elem) if self.types.equals(bound, *elem) => {
                if self.check_intervallum_materialize_runtime_bound(bound, span) {
                    Some(target_resolved)
                } else {
                    Some(self.error_type)
                }
            }
            Type::Tensor(elem, _) if self.types.equals(bound, *elem) => {
                if self.check_intervallum_materialize_runtime_bound(bound, span) {
                    Some(target_resolved)
                } else {
                    Some(self.error_type)
                }
            }
            Type::Array(_) => {
                self.invalid_conversion(ISSUE_INTERVALLUM_MATERIALIZE_LISTA_ELEMENT_MISMATCH, span);
                Some(self.error_type)
            }
            Type::Tensor(_, _) => {
                self.invalid_conversion(
                    ISSUE_INTERVALLUM_MATERIALIZE_TENSOR_ELEMENT_MISMATCH,
                    span,
                );
                Some(self.error_type)
            }
            _ => None,
        }
    }

    fn check_intervallum_materialize_runtime_bound(
        &mut self,
        bound: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        if self.is_intervallum_runtime_numerus_bound(bound) {
            return true;
        }
        self.invalid_operand_types(ISSUE_INTERVALLUM_HELPER_BOUND_UNSUPPORTED, span);
        false
    }

    /// Reject `valor` conversio holdouts and async/lazy boundary crossings.
    fn check_valor_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let source_is_valor = matches!(
            self.types.get(source_resolved),
            Type::Primitive(Primitive::Valor)
        );
        let target_is_valor = matches!(
            self.types.get(target_resolved),
            Type::Primitive(Primitive::Valor)
        );

        let issue = if source_is_valor {
            match self.types.get(target_resolved) {
                Type::Primitive(Primitive::Regex) => Some(ISSUE_VALOR_TO_REGEX_UNSUPPORTED),
                _ if self.valor_extraction_contains_unsupported_sized_numeric(target_resolved) => {
                    Some(ISSUE_VALOR_TO_SIZED_NUMERIC_UNSUPPORTED)
                }
                Type::Cursor(_) | Type::AsyncCursor(_, _) => Some(ISSUE_VALOR_TO_CURSOR_BOUNDARY),
                Type::Promissum(_) | Type::PromissumFailable(_, _) => {
                    Some(ISSUE_VALOR_TO_PROMISSUM_BOUNDARY)
                }
                _ if self.valor_extraction_contains_non_textus_map_key(target_resolved) => {
                    Some(ISSUE_CONVERSIO_MAP_KEY_NOT_TEXTUS)
                }
                _ => None,
            }
        } else if target_is_valor {
            match self.types.get(source_resolved) {
                Type::Promissum(_) | Type::PromissumFailable(_, _) => {
                    Some(ISSUE_PROMISSUM_TO_VALOR_BOUNDARY)
                }
                Type::Cursor(_) | Type::AsyncCursor(_, _) => Some(ISSUE_CURSOR_TO_VALOR_BOUNDARY),
                _ if self.valor_extraction_contains_non_textus_map_key(source_resolved) => {
                    Some(ISSUE_CONVERSIO_MAP_KEY_NOT_TEXTUS)
                }
                _ => None,
            }
        } else {
            None
        };

        if let Some(issue) = issue {
            self.invalid_conversion(issue, span);
            return Some(self.error_type);
        }
        None
    }

    fn check_json_genus_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        match (
            self.types.get(source_resolved),
            self.types.get(target_resolved),
        ) {
            (Type::Struct(def_id), Type::Primitive(Primitive::Json))
            | (Type::Primitive(Primitive::Json), Type::Struct(def_id)) => {
                if self.json_structs.contains(def_id) {
                    Some(target_resolved)
                } else {
                    self.invalid_conversion(ISSUE_JSON_GENUS_CONVERSION_REQUIRES_JSON, span);
                    Some(self.error_type)
                }
            }
            _ => None,
        }
    }

    fn valor_extraction_contains_unsupported_sized_numeric(&self, target: TypeId) -> bool {
        let mut seen = handroll::FxHashSet::default();
        self.valor_extraction_type_needs_missing_width_runtime(target, false, &mut seen)
    }

    /// `valor ↦ tabula<K, V>` needs a textus key in both directions: a
    /// conversion never parses keys, so a non-text key anywhere inside the
    /// value being extracted is the same error (need, G-01a). The
    /// walk mirrors [`Self::valor_extraction_type_needs_missing_width_runtime`]
    /// so nested `lista<tabula<numerus, V>>` shapes cannot slip through.
    fn valor_extraction_contains_non_textus_map_key(&self, ty: TypeId) -> bool {
        let mut seen = handroll::FxHashSet::default();
        self.valor_extraction_type_has_non_textus_map_key(ty, &mut seen)
    }

    fn valor_extraction_type_has_non_textus_map_key(
        &self,
        ty: TypeId,
        seen: &mut handroll::FxHashSet<TypeId>,
    ) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        if !seen.insert(resolved) {
            return false;
        }

        match self.types.get(resolved) {
            Type::Map(key, value) => {
                !self.is_textus(*key)
                    || self.valor_extraction_type_has_non_textus_map_key(*value, seen)
            }
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Matrix(inner, _)
            | Type::Vector(inner, _)
            | Type::Option(inner)
            | Type::Ref(_, inner)
            | Type::Alias(_, inner)
            | Type::Intervallum(inner, _)
            | Type::Atomic(inner) => {
                self.valor_extraction_type_has_non_textus_map_key(*inner, seen)
            }
            Type::Struct(def_id) => self.structs.get(def_id).is_some_and(|fields| {
                fields
                    .fields
                    .values()
                    .any(|field| self.valor_extraction_type_has_non_textus_map_key(field.ty, seen))
            }),
            Type::Union(types) => types
                .iter()
                .any(|ty| self.valor_extraction_type_has_non_textus_map_key(*ty, seen)),
            Type::Applied(base, args) => {
                self.valor_extraction_type_has_non_textus_map_key(*base, seen)
                    || args.iter().any(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => {
                            self.valor_extraction_type_has_non_textus_map_key(*id, seen)
                        }
                        radix_types::AppliedArg::Size(_) => false,
                    })
            }
            _ => false,
        }
    }

    fn valor_extraction_type_needs_missing_width_runtime(
        &self,
        ty: TypeId,
        inside_from_valor_runtime: bool,
        seen: &mut handroll::FxHashSet<(TypeId, bool)>,
    ) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        if !seen.insert((resolved, inside_from_valor_runtime)) {
            return false;
        }

        match self.types.get(resolved) {
            Type::SizedNumeric(Primitive::Fractus, _) => true,
            Type::SizedNumeric(Primitive::Numerus, _) => inside_from_valor_runtime,
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Matrix(inner, _)
            | Type::Vector(inner, _)
            | Type::Option(inner)
            | Type::Ref(_, inner)
            | Type::Alias(_, inner)
            | Type::Intervallum(inner, _)
            | Type::Atomic(inner) => {
                self.valor_extraction_type_needs_missing_width_runtime(*inner, true, seen)
            }
            Type::Map(key, value) => {
                self.valor_extraction_type_needs_missing_width_runtime(*key, true, seen)
                    || self.valor_extraction_type_needs_missing_width_runtime(*value, true, seen)
            }
            Type::Struct(def_id) => self.structs.get(def_id).is_some_and(|fields| {
                fields.fields.values().any(|field| {
                    self.valor_extraction_type_needs_missing_width_runtime(field.ty, false, seen)
                })
            }),
            Type::Union(types) => types.iter().any(|ty| {
                self.valor_extraction_type_needs_missing_width_runtime(
                    *ty,
                    inside_from_valor_runtime,
                    seen,
                )
            }),
            Type::Applied(base, args) => {
                self.valor_extraction_type_needs_missing_width_runtime(
                    *base,
                    inside_from_valor_runtime,
                    seen,
                ) || args.iter().any(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => self
                        .valor_extraction_type_needs_missing_width_runtime(
                            *id,
                            inside_from_valor_runtime,
                            seen,
                        ),
                    radix_types::AppliedArg::Size(_) => false,
                })
            }
            _ => false,
        }
    }

    /// `octeti ↦ lista<numerus<u8>>` is a representation change only.
    /// Other lista element types fail closed — no silent widen, no pad.
    fn check_octeti_to_lista_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !(matches!(
            self.types.get(self.resolve_inferred_type(source_ty)),
            Type::Primitive(Primitive::Octeti)
        ) || self.is_bytes_list(source_ty))
        {
            return None;
        }
        let Type::Array(elem) = self.types.get(target_resolved).clone() else {
            return None;
        };
        // Bits elements are the bits rows' business (`lista<b8>` identity,
        // width changes with `via Le|Be`), decided before this row.
        if self.is_numerus_u8(elem) || self.bits_width_of(elem).is_some() {
            return Some(target_resolved);
        }
        self.invalid_conversion(ISSUE_UNSUPPORTED_CONVERSIO, span);
        Some(self.error_type)
    }

    /// `W ↦ octeti<N> via Be|Le` is the infallible write inverse of
    /// `octeti ↦ W` (need, ruling E1). The byte length is exactly the
    /// element width, so the row takes no `⊥` recovery and no failure channel.
    /// A source without an exact endian width (`int`, `u8`/`i8`, `f16`,
    /// `bf16`, policy floats) or a missing/foreign hint stays with
    /// `numeric_to_octeti_unspecified`; a capacity that is not the literal
    /// width is `octeti_endian_width_mismatch`. An 8-bit source is not built
    /// (ruling B6: an 8-bit value has no byte order).
    fn check_numeric_to_bounded_octeti_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Type::BoundedOcteti { capacity } = self
            .types
            .get(self.resolve_inferred_type(target_resolved))
            .clone()
        else {
            return None;
        };
        if self.is_inf_type(source_ty)
            || !matches!(
                self.primitive_conversio_family(source_ty),
                PrimitiveConversioFamily::Numerus | PrimitiveConversioFamily::Fractus
            )
        {
            return None;
        }
        let Some(width) =
            endian_read_width_bytes(self.types.get(self.resolve_inferred_type(source_ty)))
        else {
            self.invalid_conversion(ISSUE_NUMERIC_TO_OCTETI_UNSPECIFIED, span);
            return Some(self.error_type);
        };
        if !self.conversio_has_endian_hint(params) {
            self.invalid_conversion(ISSUE_NUMERIC_TO_OCTETI_UNSPECIFIED, span);
            return Some(self.error_type);
        }
        let IndexExpr::Literal(capacity) = self.types.get_index(capacity).clone() else {
            self.invalid_conversion(ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH, span);
            return Some(self.error_type);
        };
        if usize::try_from(capacity) != Ok(width) {
            self.invalid_conversion(ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH, span);
            return Some(self.error_type);
        }
        Some(target_resolved)
    }

    /// `tensor<E, [d1, ..]> ↦ octeti via Be|Le` is the exact inverse of
    /// `octeti ↦ tensor`: element `i` becomes bytes `[i*w, (i+1)*w)` in the
    /// hinted order, no numeric conversion. The hint is required for every
    /// element wider than 8 bits and the row cannot fail (the length is
    /// `product(shape) x w`).
    fn check_tensor_to_octeti_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !(matches!(
            self.types.get(self.resolve_inferred_type(target_resolved)),
            Type::Primitive(Primitive::Octeti)
        ) || self.is_bytes_list(target_resolved))
        {
            return None;
        }
        let Type::Tensor(elem, _) = self
            .types
            .get(self.resolve_inferred_type(source_ty))
            .clone()
        else {
            return None;
        };
        // Ruling B6: an 8-bit element has no byte order, so it
        // takes no `via` clause and a clause written on it is refused; every
        // wider element still requires one.
        let one_byte = matches!(
            self.types.get(self.resolve_inferred_type(elem)),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::I8 | NumericWidth::U8)
        );
        if one_byte {
            if !params.is_empty() {
                self.invalid_conversion(super::bits::BITS_VIA_CLAUSE_NOT_ALLOWED, span);
                return Some(self.error_type);
            }
            return Some(target_resolved);
        }
        if !self.conversio_has_endian_hint(params) {
            self.invalid_conversion(ISSUE_TENSOR_TO_OCTETI_UNSPECIFIED, span);
            return Some(self.error_type);
        }
        if octeti_tensor_element_width_bytes(self.types.get(self.resolve_inferred_type(elem)))
            .is_none()
        {
            self.invalid_conversion(ISSUE_TENSOR_TO_OCTETI_ELEMENT_UNSUPPORTED, span);
            return Some(self.error_type);
        }
        Some(target_resolved)
    }

    /// `octeti ↦ tensor<E, [d1, ..]> via Be|Le` is a faithful bit
    /// reinterpretation of the bytes (row-major, byte order from the hint).
    /// The hint is required for every element wider than 8 bits and forbidden
    /// on 8-bit ones; `E` is one of u8 i8 i16 u16 i32
    /// u32 i64 u64 f32 f64, or a fixed-layout genus (need: little-
    /// endian only, `Le` required, width from the layout law). The byte length
    /// must equal `product(shape) x width` exactly: checked here only when the
    /// source length is static (an octeti literal or a constant window), every
    /// extent is a literal and the element is a scalar; otherwise it is a
    /// runtime check (a genus row's width is not known to the checker).
    fn check_octeti_to_tensor_conversio(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        target_carrier: TypeId,
        params: &[Symbol],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !self.is_octeti_tensor_source(source_ty) {
            return None;
        }
        let Type::Tensor(elem, shape) = self.types.get(target_carrier).clone() else {
            return None;
        };
        // A genus element is a fixed-layout record whose byte
        // image is the device layout law's: little-endian, field offsets from
        // `resolve_struct_layout`. The checker cannot see that layout, so it
        // admits the element, requires the explicit `Le` clause (a `Be` clause
        // is refused by name) and leaves the byte count to the run-time check:
        // truncation, misalignment and overflow are named conversion failures.
        if matches!(
            self.types.get(self.resolve_inferred_type(elem)),
            Type::Struct(_)
        ) {
            if !self.conversio_has_endian_hint(params) {
                self.invalid_conversion(ISSUE_OCTETI_TO_TENSOR_UNSPECIFIED, span);
                return Some(self.error_type);
            }
            if self.conversio_has_big_endian_hint(params) {
                self.invalid_conversion(ISSUE_OCTETI_TO_TENSOR_RECORD_BIG_ENDIAN, span);
                return Some(self.error_type);
            }
            return Some(target_resolved);
        }
        // Ruling B6: an 8-bit element has no byte order, so
        // it takes no `via` clause and a clause written on it is refused;
        // every wider element still requires one.
        let one_byte = matches!(
            self.types.get(self.resolve_inferred_type(elem)),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::I8 | NumericWidth::U8)
        );
        if one_byte {
            if !params.is_empty() {
                self.invalid_conversion(super::bits::BITS_VIA_CLAUSE_NOT_ALLOWED, span);
                return Some(self.error_type);
            }
        } else if !self.conversio_has_endian_hint(params) {
            self.invalid_conversion(ISSUE_OCTETI_TO_TENSOR_UNSPECIFIED, span);
            return Some(self.error_type);
        }
        let Some(width) =
            octeti_tensor_element_width_bytes(self.types.get(self.resolve_inferred_type(elem)))
        else {
            self.invalid_conversion(ISSUE_OCTETI_TO_TENSOR_ELEMENT_UNSUPPORTED, span);
            return Some(self.error_type);
        };
        let expected = radix_types::i64s_from_literal_index(shape, self.types).and_then(|dims| {
            dims.iter().try_fold(width as u128, |acc, dim| {
                acc.checked_mul(u128::try_from(*dim).ok()?)
            })
        });
        if let (Some(expected), Some(actual)) = (expected, self.static_octeti_window_len(source))
            && expected != actual as u128
        {
            self.errors.push(
                SemanticError::structured(SemanticErrorKind::ConversionInvalid, span)
                    .with_arg("issue", ISSUE_OCTETI_TO_TENSOR_LENGTH_MISMATCH)
                    .with_arg("expected", expected.to_string())
                    .with_arg("actual", actual.to_string()),
            );
            return Some(self.error_type);
        }
        Some(target_resolved)
    }

    /// The byte-string forms of a conversion side: `lista<b8>` (`bytes`) and
    /// the bounded `octeti<N>` form. One helper for every byte row (bulk
    /// decode, endian windows, register targets, the tensor inverse).
    fn is_octeti_tensor_source(&self, source_ty: TypeId) -> bool {
        match self.types.get(self.resolve_inferred_type(source_ty)) {
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => true,
            _ => self.is_bytes_list(source_ty),
        }
    }

    /// `lista<b8>`: the unbounded raw-bits byte list `octeti`/`bytes` names.
    fn is_bytes_list(&self, ty: TypeId) -> bool {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Array(element) => {
                self.bits_width_of(*element) == Some(radix_types::BitsWidth::B8)
            }
            _ => false,
        }
    }

    /// The conversion family used by the refusing primitive rows: a byte list
    /// is the `Octeti` family on either side, every other type keeps its
    /// ordinary family.
    fn bytes_conversio_family(&self, ty: TypeId) -> PrimitiveConversioFamily {
        if self.is_bytes_list(ty) {
            PrimitiveConversioFamily::Octeti
        } else {
            self.primitive_conversio_family(ty)
        }
    }

    fn is_numerus_u8(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8)
        )
    }

    /// CTFP-1 register-class rows. Register-in windows
    /// (`octeti<N> ↦ vector<T, N/width>`), register-out
    /// (`vector<T, size N> ↦ lista<T>` / `↦ octeti<N>`), and element bridges
    /// (`vector<T, size N> ↦ vector<U, size N>`). v1 window elements are u16/u32 only;
    /// every window needs a static byte length with `widths × count = N` and a
    /// `Be`/`Le` hint. Out-of-set bridges and wrong widths fail closed.
    fn check_register_conversio(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        match (
            self.types.get(source_resolved).clone(),
            self.types.get(target_resolved).clone(),
        ) {
            // Register-in: exact static octeti window into a vector register.
            (source_kind, Type::Vector(elem, width))
                if matches!(
                    source_kind,
                    Type::BoundedOcteti { .. } | Type::Primitive(Primitive::Octeti)
                ) || self.is_bytes_list(source_resolved) =>
            {
                if !self.conversio_has_endian_hint(params) {
                    self.invalid_conversion(ISSUE_OCTETI_TO_VECTOR_UNSPECIFIED, span);
                    return Some(self.error_type);
                }
                let Some(count) = crate::literal_vector_width(width, self.types) else {
                    self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_WIDTH, span);
                    return Some(self.error_type);
                };
                let Some(element_bytes) = self.register_element_width_bytes(elem) else {
                    self.invalid_conversion(ISSUE_REGISTER_WINDOW_WIDTH_MISMATCH, span);
                    return Some(self.error_type);
                };
                let Some(window_bytes) = self.register_window_len(source, source_resolved) else {
                    self.invalid_conversion(ISSUE_OCTETI_TO_VECTOR_UNSPECIFIED, span);
                    return Some(self.error_type);
                };
                if u64::from(count) * u64::from(element_bytes) != window_bytes {
                    self.invalid_conversion(ISSUE_REGISTER_WINDOW_WIDTH_MISMATCH, span);
                    return Some(self.error_type);
                }
                Some(target_resolved)
            }
            // Register-out / bridges from a vector register.
            (Type::Vector(elem, width), target_ty) => {
                let Some(count) = crate::literal_vector_width(width, self.types) else {
                    self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_WIDTH, span);
                    return Some(self.error_type);
                };
                match target_ty {
                    Type::Array(target_elem) => {
                        if self.types.equals(elem, target_elem) {
                            Some(target_resolved)
                        } else {
                            self.invalid_conversion(ISSUE_REGISTER_OUT_ELEMENT_MISMATCH, span);
                            Some(self.error_type)
                        }
                    }
                    Type::BoundedOcteti { capacity } => {
                        if !self.conversio_has_endian_hint(params) {
                            self.invalid_conversion(ISSUE_VECTOR_TO_OCTETI_UNSPECIFIED, span);
                            return Some(self.error_type);
                        }
                        let Some(element_bytes) = self.register_element_width_bytes(elem) else {
                            self.invalid_conversion(ISSUE_REGISTER_WINDOW_WIDTH_MISMATCH, span);
                            return Some(self.error_type);
                        };
                        let IndexExpr::Literal(capacity) = self.types.get_index(capacity).clone()
                        else {
                            self.invalid_conversion(ISSUE_REGISTER_WINDOW_WIDTH_MISMATCH, span);
                            return Some(self.error_type);
                        };
                        if u64::from(count) * u64::from(element_bytes) != capacity {
                            self.invalid_conversion(ISSUE_REGISTER_WINDOW_WIDTH_MISMATCH, span);
                            return Some(self.error_type);
                        }
                        Some(target_resolved)
                    }
                    Type::Vector(target_elem, target_width) => {
                        let target_count = crate::literal_vector_width(target_width, self.types);
                        if target_count != Some(count) {
                            self.invalid_conversion(ISSUE_REGISTER_BRIDGE_WIDTH_MISMATCH, span);
                            return Some(self.error_type);
                        }
                        if self.register_bridge_element_allowed(elem, target_elem) {
                            Some(target_resolved)
                        } else {
                            self.invalid_conversion(
                                ISSUE_REGISTER_BRIDGE_ELEMENT_UNSUPPORTED,
                                span,
                            );
                            Some(self.error_type)
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// v1 register window element widths: u16 and u32 numerus only.
    fn register_element_width_bytes(&self, elem: TypeId) -> Option<u8> {
        match self.types.get(self.resolve_inferred_type(elem)) {
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U16) => Some(2),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32) => Some(4),
            _ => None,
        }
    }

    /// Static byte length of a register-in source: a literal/index window, or
    /// a bounded `octeti<N>` value with a literal capacity.
    fn register_window_len(&self, source: &HirExpression, source_resolved: TypeId) -> Option<u64> {
        if let Some(len) = self.static_octeti_window_len(source) {
            return Some(len as u64);
        }
        match self.types.get(source_resolved) {
            Type::BoundedOcteti { capacity } => match self.types.get_index(*capacity) {
                IndexExpr::Literal(capacity) => Some(*capacity),
                _ => None,
            },
            _ => None,
        }
    }

    /// Bridge element set: numeric scalar → numeric scalar (numerus/fractus).
    fn register_bridge_element_allowed(&self, elem: TypeId, target_elem: TypeId) -> bool {
        let numeric = |ty: TypeId| {
            matches!(
                self.types.get(self.resolve_inferred_type(ty)),
                Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                    | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
            )
        };
        numeric(elem) && numeric(target_elem)
    }

    /// Reject primitive `↦` pairs with no single obvious meaning (byte reinterpretation, etc.).
    /// Operator ruling 05ac0534: a list of anything but a
    /// bits marker is not text. `lista<T> ↦ textus` and `lista<T, magnitudo N> ↦
    /// textus<M>` have no grammar row for any `T`; the byte buffer
    /// (`octeti`) is a distinct type and keeps its row. A registered user
    /// conversion row for the pair stays authoritative.
    fn check_lista_to_textus_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        target_carrier: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !matches!(
            self.types.get(target_carrier),
            Type::Primitive(Primitive::Textus) | Type::BoundedTextus { .. }
        ) {
            return None;
        }
        let source = self.resolve_inferred_type(source_ty);
        let element = match self.types.get(source) {
            Type::Array(element)
            | Type::BoundedArray { element, .. }
            | Type::RangedArray { element, .. } => *element,
            _ => return None,
        };
        let element = self.resolve_inferred_type(element);
        if matches!(
            self.types.get(element),
            Type::Bits(_)
                | Type::Primitive(Primitive::Octeti | Primitive::Valor | Primitive::Ignotum)
                | Type::Param(_)
                | Type::Infer(_)
        ) {
            return None;
        }
        if self
            .resolver
            .conversion_row_for(source, target_resolved)
            .is_some()
        {
            return None;
        }
        self.invalid_conversion(ISSUE_LISTA_TO_TEXTUS_UNSUPPORTED, span);
        Some(self.error_type)
    }

    /// The mirror of [`Self::check_lista_to_textus_conversio`]: a string does
    /// not become a list of anything — the element type does not say which
    /// encoding it is. `textus ↦ lista<bN>` is the encoded text row and is
    /// handled earlier by `check_bits_text_conversio`; a registered user
    /// conversion row for the pair stays authoritative.
    fn check_textus_to_lista_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        target_carrier: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !matches!(
            self.types.get(target_carrier),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        ) {
            return None;
        }
        let source = self.resolve_inferred_type(source_ty);
        if !matches!(
            self.types.get(source),
            Type::Primitive(Primitive::Textus) | Type::BoundedTextus { .. }
        ) {
            return None;
        }
        if self
            .resolver
            .conversion_row_for(source, target_resolved)
            .is_some()
        {
            return None;
        }
        self.invalid_conversion(ISSUE_TEXTUS_TO_LISTA_UNSUPPORTED, span);
        Some(self.error_type)
    }

    fn check_rejected_primitive_conversio(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let target_family = self.bytes_conversio_family(target_resolved);
        // A bounded `octeti<N>` source is the read inverse of the write row
        // (`W ↦ octeti<N> via Be|Le`): route it to the endian-decode rows when
        // the target is a numeric scalar. Every other pair (bool, textus,
        // ascii) keeps its ordinary family below.
        let source_family = match self.types.get(self.resolve_inferred_type(source_ty)) {
            Type::BoundedOcteti { .. }
                if matches!(
                    target_family,
                    PrimitiveConversioFamily::Numerus | PrimitiveConversioFamily::Fractus
                ) =>
            {
                PrimitiveConversioFamily::Octeti
            }
            _ => self.bytes_conversio_family(source_ty),
        };
        let issue = match (source_family, target_family) {
            (
                PrimitiveConversioFamily::Numerus
                | PrimitiveConversioFamily::Fractus
                | PrimitiveConversioFamily::Bivalens,
                PrimitiveConversioFamily::Octeti,
            ) => {
                // Ruling 40: `inf ↦ octeti via Be|Le` is the one integer row
                // with a total byte form (minimal two's complement).
                (!(self.is_inf_type(source_ty) && self.conversio_has_endian_hint(params)))
                    .then_some(ISSUE_NUMERIC_TO_OCTETI_UNSPECIFIED)
            }
            (PrimitiveConversioFamily::Octeti, PrimitiveConversioFamily::Numerus) => {
                self.octeti_to_numerus_issue(source, source_ty, target_resolved, params)
            }
            (PrimitiveConversioFamily::Octeti, PrimitiveConversioFamily::Fractus) => {
                self.octeti_to_fractus_issue(source, source_ty, target_resolved, params)
            }
            (PrimitiveConversioFamily::Octeti, PrimitiveConversioFamily::Bivalens) => {
                Some(ISSUE_OCTETI_TO_BIVALENS_UNSUPPORTED)
            }
            _ => None,
        };

        if let Some(issue) = issue {
            self.invalid_conversion(issue, span);
            return Some(self.error_type);
        }
        None
    }

    /// `octeti ↦ W via Be|Le` is allowed when width is 2/4/8 and a
    /// static window matches. Without Be/Le, or on i8/u8/unsized numerus,
    /// the family reject stays.
    fn octeti_to_numerus_issue(
        &self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
    ) -> Option<&'static str> {
        if !self.conversio_has_endian_hint(params) {
            return Some(ISSUE_OCTETI_TO_NUMERUS_UNSPECIFIED);
        }
        // Ruling 40: `octeti ↦ inf via Be|Le` decodes any length (the exact
        // inverse of the minimal two's-complement encode); no window width.
        if self.is_inf_type(target_resolved) {
            return None;
        }
        let Some(need) = endian_read_width_bytes(self.types.get(target_resolved)) else {
            return Some(ISSUE_OCTETI_TO_NUMERUS_UNSPECIFIED);
        };
        match self.static_octeti_window_len(source) {
            Some(got) if got != need => Some(ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH),
            _ => match self.bounded_octeti_literal_capacity(source_ty) {
                Some(capacity) if capacity < need => Some(ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH),
                _ => None,
            },
        }
    }

    /// `octeti ↦ W via Be|Le` is allowed only for the exact IEEE width
    /// and a static window that matches it. Without Be/Le, or on f16/unsized
    /// fractus, the family reject stays.
    fn octeti_to_fractus_issue(
        &self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
    ) -> Option<&'static str> {
        if !self.conversio_has_endian_hint(params) {
            return Some(ISSUE_OCTETI_TO_FRACTUS_UNSPECIFIED);
        }
        let Some(need) = endian_read_width_bytes(self.types.get(target_resolved)) else {
            return Some(ISSUE_OCTETI_TO_FRACTUS_UNSPECIFIED);
        };
        match self.static_octeti_window_len(source) {
            Some(got) if got != need => Some(ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH),
            _ => match self.bounded_octeti_literal_capacity(source_ty) {
                Some(capacity) if capacity < need => Some(ISSUE_OCTETI_ENDIAN_WIDTH_MISMATCH),
                _ => None,
            },
        }
    }

    /// Literal byte capacity of a bounded `octeti<N>` source. A read whose
    /// target width exceeds the carrier capacity can never succeed: an
    /// `octeti<2>` holds at most two bytes, so `↦ u32` fails closed at
    /// typecheck. A capacity larger than `need` stays accepted (the live
    /// length may equal `need`, checked at run time).
    fn bounded_octeti_literal_capacity(&self, source_ty: TypeId) -> Option<usize> {
        match self.types.get(self.resolve_inferred_type(source_ty)) {
            Type::BoundedOcteti { capacity } => match self.types.get_index(*capacity) {
                IndexExpr::Literal(capacity) => usize::try_from(*capacity).ok(),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether a `Be` clause is among `params` (the record row admits `Le` only).
    fn conversio_has_big_endian_hint(&self, params: &[Symbol]) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        params
            .iter()
            .any(|param| matches!(interner.resolve(*param), "be" | "Be"))
    }

    pub(super) fn conversio_has_endian_hint(&self, params: &[Symbol]) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        params
            .iter()
            .any(|param| matches!(interner.resolve(*param), "be" | "Be" | "le" | "Le"))
    }

    /// Need: the rounding rule of `↦`. A float or decimal source into
    /// an integer-family target (`numerus`, sized integers, `inf`, `wrapping`
    /// and `saturating` words) needs a `via Floor|Ceil|HalfEven|Trunc` hint;
    /// the bare conversion is the named `float_to_integer_requires_rounding`
    /// error. A rounding hint on any other pairing is the unknown-hint error.
    /// `Ok(true)` means a rounding hint was accepted; the conversion then
    /// continues down the ordinary (failable) numeric rows.
    fn check_rounding_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        span: radix_lexer::Span,
    ) -> Result<bool, ()> {
        let hint = self.interner.and_then(|interner| {
            params
                .iter()
                .find_map(|param| match interner.resolve(*param) {
                    "floor" => Some("floor"),
                    "Floor" => Some("Floor"),
                    "ceil" => Some("ceil"),
                    "Ceil" => Some("Ceil"),
                    "half_even" => Some("half_even"),
                    "HalfEven" => Some("HalfEven"),
                    "trunc" => Some("trunc"),
                    "Trunc" => Some("Trunc"),
                    _ => None,
                })
        });
        let source = self.peel_alias_chain(self.resolve_inferred_type(source_ty));
        let from_real = matches!(
            numeric_fact(self.types.get(source)).map(|fact| fact.family),
            Some(NumericFactFamily::Float { .. } | NumericFactFamily::Decimal)
        );
        let to_integer = self.is_integer_conversio_operand(target_resolved);
        match (from_real && to_integer, hint) {
            (true, None) => {
                self.invalid_conversion(ISSUE_FLOAT_TO_INTEGER_REQUIRES_ROUNDING, span);
                Err(())
            }
            (false, Some(hint)) => {
                self.invalid_conversion_with_args(
                    ISSUE_CONVERSIO_VIA_UNKNOWN_HINT,
                    span,
                    &[("hint", hint)],
                );
                Err(())
            }
            (rounds, _) => Ok(rounds),
        }
    }

    /// decimal-widths D2 (wildcard audit): named reject for conversio `via`
    /// hints aimed at a decimal numerus target — `textus ↦ d64 via Hex`,
    /// `octeti ↦ d64 via Be`, and every other hint spelling.
    fn decimal_target_hint_issue(
        &self,
        target_resolved: TypeId,
        params: &[Symbol],
    ) -> Option<&'static str> {
        if !matches!(
            self.types.get(target_resolved),
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal()
        ) {
            return None;
        }
        let interner = self.interner?;
        params
            .iter()
            .find_map(|param| match interner.resolve(*param) {
                "hex" | "Hex" | "bin" | "Bin" | "oct" | "Oct" => {
                    Some(ISSUE_DECIMAL_RADIX_HINT_REJECTED)
                }
                "be" | "Be" | "le" | "Le" => Some(ISSUE_DECIMAL_ENDIAN_HINT_REJECTED),
                _ => None,
            })
    }

    /// `n ↦ ascii<N> via Hex|Bin|Oct`: N is digit width; Hex/Bin/Oct are `via`
    /// format hints carried in conversio params. Float sources reject; a
    /// const-foldable negative or overflowing source fail-closes at typecheck
    /// unless `⊥` default is present (runtime still fail-closes).
    fn check_ascii_format_pack_conversio(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        has_recovery: bool,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !self.conversio_has_ascii_format_hint(params) {
            return None;
        }
        match self.types.get(target_resolved) {
            Type::BoundedAscii { capacity } => {
                let capacity = *capacity;
                if let PrimitiveConversioFamily::Numerus =
                    self.primitive_conversio_family(source_ty)
                {
                } else {
                    self.invalid_conversion(ISSUE_UNSUPPORTED_CONVERSIO, span);
                    return Some(self.error_type);
                }
                // decimal-widths D2 (wildcard audit): `n ↦ ascii<N> via Hex` is
                // integer digit packing; a scaled decimal has no radix digits.
                if matches!(
                    self.types.get(self.resolve_inferred_type(source_ty)),
                    Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal()
                ) {
                    self.invalid_conversion(ISSUE_DECIMAL_RADIX_HINT_REJECTED, span);
                    return Some(self.error_type);
                }

                // AP-2: a hole is a const-folding witness, not a runtime
                // capacity inference. A literal numerus source supplies the
                // digit count through the shared index unifier; a runtime
                // source keeps the hole open so finalization reports the
                // established bounded_capacity_unsolved diagnostic.
                let resolved_capacity = self.resolve_index(capacity);
                if self.is_unresolved_index_hole(resolved_capacity) {
                    if let Some(value) = static_integer_magnitude(source) {
                        if value >= 0 {
                            let radix = self
                                .ascii_format_radix(params)
                                .expect("ascii format hint checked above");
                            let digits = ascii_pack_digit_count(value.unsigned_abs(), radix);
                            let witness = self.types.intern_index(IndexExpr::Literal(digits));
                            self.constrain_index_equal(capacity, witness, span);
                        } else if !has_recovery {
                            self.invalid_conversion(ISSUE_ASCII_PACK_NEGATIVE_SOURCE, span);
                            return Some(self.error_type);
                        }
                    }
                    return Some(target_resolved);
                }

                if !has_recovery
                    && let Some(issue) =
                        self.ascii_format_pack_const_issue(source, resolved_capacity, params)
                {
                    self.invalid_conversion(issue, span);
                    return Some(self.error_type);
                }
                Some(target_resolved)
            }
            Type::Primitive(Primitive::Ascii) => {
                self.invalid_conversion(ISSUE_ASCII_FORMAT_HINT_NOT_CAPACITY, span);
                Some(self.error_type)
            }
            _ => None,
        }
    }

    fn conversio_has_ascii_format_hint(&self, params: &[Symbol]) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        params
            .iter()
            .any(|param| crate::numeric_width::is_ascii_format_hint(interner.resolve(*param)))
    }

    fn ascii_format_radix(&self, params: &[Symbol]) -> Option<u32> {
        let interner = self.interner?;
        params
            .iter()
            .find_map(|param| match interner.resolve(*param) {
                "hex" | "Hex" => Some(16),
                "bin" | "Bin" => Some(2),
                "oct" | "Oct" => Some(8),
                _ => None,
            })
    }

    fn ascii_format_pack_const_issue(
        &self,
        source: &HirExpression,
        capacity: radix_types::IndexId,
        params: &[Symbol],
    ) -> Option<&'static str> {
        let value = static_integer_magnitude(source)?;
        if value < 0 {
            return Some(ISSUE_ASCII_PACK_NEGATIVE_SOURCE);
        }
        let IndexExpr::Literal(width) = self.types.get_index(capacity) else {
            return None;
        };
        let radix = self.ascii_format_radix(params)?;
        let digits = ascii_pack_digit_count(value.unsigned_abs(), radix);
        if digits > *width {
            Some(ISSUE_ASCII_PACK_WIDTH_OVERFLOW)
        } else {
            None
        }
    }

    /// Compile-time window length: a whole octeti literal, or a constant
    /// `lo‥hi` / `lo…hi` index. Unknown length is a runtime check.
    fn static_octeti_window_len(&self, source: &HirExpression) -> Option<usize> {
        match &source.kind {
            HirExpressionKind::Literal(HirLiteral::Octeti(sym)) => {
                let raw = self.interned_literal_payload(*sym)?;
                radix_lexer::octeti_literal::decode_octeti_hex(raw)
                    .ok()
                    .map(|bytes| bytes.len())
            }
            HirExpressionKind::Index(_, index) => static_exclusive_range_len(index),
            _ => None,
        }
    }

    /// Reject async/sync `promissum` boundaries unless both sides are `promissum`.
    fn check_promissum_conversio_boundary(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let source_promissum = matches!(self.types.get(source_resolved), Type::Promissum(_));
        let target_promissum = matches!(self.types.get(target_resolved), Type::Promissum(_));

        let issue = match (source_promissum, target_promissum) {
            (false, true) => Some(ISSUE_PROMISSUM_TARGET_BOUNDARY),
            (true, false) => Some(ISSUE_PROMISSUM_SOURCE_BOUNDARY),
            _ => None,
        };

        if let Some(issue) = issue {
            self.invalid_conversion(issue, span);
            return Some(self.error_type);
        }
        None
    }

    /// Validate dense/sparse representation `↦` pairs.
    fn check_sparse_tensor_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let source = self.types.get(source_resolved);
        let target = self.types.get(target_resolved);

        let Some((from_elem, from_shape, to_elem, to_shape)) = sparse_tensor_pair(source, target)
        else {
            if matches!(source, Type::Sparsa(_, _)) || matches!(target, Type::Sparsa(_, _)) {
                self.invalid_conversion(ISSUE_SPARSA_COLLECTION_UNSUPPORTED, span);
                return Some(self.error_type);
            }
            return None;
        };

        if !self.types.equals(from_elem, to_elem) {
            self.invalid_conversion(ISSUE_SPARSA_TENSOR_ELEMENT_MISMATCH, span);
            return Some(self.error_type);
        }

        if !self.index_shapes_unify(from_shape, to_shape) {
            self.invalid_conversion(ISSUE_SPARSA_TENSOR_SHAPE_MISMATCH, span);
            return Some(self.error_type);
        }

        None
    }

    /// Validate tensor `↦` pairs: element-width bridges, scalar-extract holdouts.
    fn check_tensor_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let tensor_pair = match (
            self.types.get(source_resolved),
            self.types.get(target_resolved),
        ) {
            (Type::Tensor(from_elem, from_shape), Type::Tensor(to_elem, to_shape)) => {
                Some((*from_elem, *from_shape, *to_elem, *to_shape))
            }
            _ => None,
        };
        let shapes_unify = tensor_pair.is_none_or(|(_, from_shape, _, to_shape)| {
            self.index_shapes_unify(from_shape, to_shape)
        });
        let issue = match (
            self.types.get(source_resolved),
            self.types.get(target_resolved),
        ) {
            (Type::Tensor(_, _), target) if Self::is_tensor_scalar_extract_target(target) => {
                Some(ISSUE_TENSOR_TO_SCALAR_UNSUPPORTED)
            }
            (source, Type::Tensor(_, _)) if Self::is_tensor_scalar_extract_target(source) => {
                Some(ISSUE_SCALAR_TO_TENSOR_UNSUPPORTED)
            }
            (Type::Tensor(_, _), Type::Tensor(_, _)) if !shapes_unify => {
                Some(ISSUE_TENSOR_SHAPE_MISMATCH)
            }
            (Type::Tensor(from_elem, _), Type::Tensor(to_elem, _))
                if !crate::numeric_width::tensor_element_conversio_allowed(
                    self.types, *from_elem, *to_elem,
                ) =>
            {
                Some(ISSUE_TENSOR_ELEMENT_MISMATCH)
            }
            _ => None,
        };

        if let Some(issue) = issue {
            self.invalid_conversion(issue, span);
            return Some(self.error_type);
        }
        None
    }

    /// Ruling 16: the exact `tensor ↔ vector` and
    /// `tensor ↔ matrix` family crossings. Accepting the pair needs equal
    /// element types and equal static extents; the extent and element refusals
    /// reuse the tensor issues. Pairs outside these four (non-static extents,
    /// another rank, every other family pairing) are not this bridge and keep
    /// their existing refusal.
    fn check_collection_family_bridge_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        recovery: Option<&mut HirExpression>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        let (source_elem, target_elem, source_extents, target_extents) =
            self.collection_family_bridge_pair(source_resolved, target_resolved)?;
        if source_extents != target_extents {
            self.invalid_conversion(ISSUE_TENSOR_SHAPE_MISMATCH, span);
            return Some(self.error_type);
        }
        if !self.types.equals(source_elem, target_elem) {
            self.invalid_conversion(ISSUE_TENSOR_ELEMENT_MISMATCH, span);
            return Some(self.error_type);
        }
        self.settle_conversio_result(recovery, source_ty, target_resolved, span);
        Some(target_resolved)
    }

    /// The four accepted bridge pairs, as `(source element, target element,
    /// source extents, target extents)`. Only a static rank-1 tensor against a
    /// literal vector width, or a static rank-2 tensor against a literal
    /// matrix shape, names a pair at all.
    fn collection_family_bridge_pair(
        &mut self,
        source_resolved: TypeId,
        target_resolved: TypeId,
    ) -> Option<(TypeId, TypeId, Vec<u64>, Vec<u64>)> {
        let pair = match (
            self.types.get(source_resolved).clone(),
            self.types.get(target_resolved).clone(),
        ) {
            (Type::Tensor(elem, shape), Type::Vector(vector_elem, width)) => {
                let width = u64::from(crate::literal_vector_width(width, self.types)?);
                let extents = self.static_tensor_extents(shape)?;
                let [extent] = extents.as_slice() else {
                    return None;
                };
                (elem, vector_elem, vec![*extent], vec![width])
            }
            (Type::Vector(vector_elem, width), Type::Tensor(elem, shape)) => {
                let width = u64::from(crate::literal_vector_width(width, self.types)?);
                let extents = self.static_tensor_extents(shape)?;
                let [extent] = extents.as_slice() else {
                    return None;
                };
                (vector_elem, elem, vec![width], vec![*extent])
            }
            (Type::Tensor(elem, shape), Type::Matrix(matrix_elem, matrix_shape)) => {
                let (rows, columns) = crate::literal_matrix_shape(matrix_shape, self.types)?;
                let extents = self.static_tensor_extents(shape)?;
                let [rows_extent, columns_extent] = extents.as_slice() else {
                    return None;
                };
                (
                    elem,
                    matrix_elem,
                    vec![*rows_extent, *columns_extent],
                    vec![u64::from(rows), u64::from(columns)],
                )
            }
            (Type::Matrix(matrix_elem, matrix_shape), Type::Tensor(elem, shape)) => {
                let (rows, columns) = crate::literal_matrix_shape(matrix_shape, self.types)?;
                let extents = self.static_tensor_extents(shape)?;
                let [rows_extent, columns_extent] = extents.as_slice() else {
                    return None;
                };
                (
                    matrix_elem,
                    elem,
                    vec![u64::from(rows), u64::from(columns)],
                    vec![*rows_extent, *columns_extent],
                )
            }
            _ => return None,
        };
        Some(pair)
    }

    /// Static extents of a tensor figura. A reduced rank-1 tensor keeps its
    /// surviving dim as a bare `Literal` shape rather than a one-element tuple
    /// (`tensor_index::sectio_axis0_shape` reads both spellings), so both mean
    /// one dim here.
    fn static_tensor_extents(&mut self, shape: crate::IndexId) -> Option<Vec<u64>> {
        let resolved = self.resolve_index(shape);
        let dims = match self.types.get_index(resolved) {
            IndexExpr::Tuple(dims) => dims.clone(),
            IndexExpr::Literal(value) => return Some(vec![*value]),
            _ => return None,
        };
        let mut extents = Vec::with_capacity(dims.len());
        for dim in dims {
            extents.push(self.static_index_extent(dim)?);
        }
        Some(extents)
    }

    fn static_index_extent(&mut self, index: crate::IndexId) -> Option<u64> {
        let resolved = self.resolve_index(index);
        match self.types.get_index(resolved) {
            IndexExpr::Literal(value) => Some(*value),
            _ => None,
        }
    }

    fn is_tensor_scalar_extract_target(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
        )
    }

    fn index_shapes_unify(&mut self, from_shape: crate::IndexId, to_shape: crate::IndexId) -> bool {
        let mut subs = handroll::FxHashMap::default();
        let mut index_unifier = crate::IndexUnifier::new(self.types, &mut subs);
        index_unifier.unify(from_shape, to_shape).is_ok()
    }

    /// Validate cross-collection `↦` pairs. Returns `Some(error_type)` when the
    /// conversion is rejected; `None` when checking should continue.
    fn check_collection_conversio(
        &mut self,
        source: &mut HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        match validate_lista_to_tensor_source(source_resolved, target_resolved, self.types) {
            Ok(()) => {
                // The target type is authoritative: a single `_` extent in the
                // target figura infers from the flat source's element count,
                // the same solver the from_flat row uses. Unsolvable hole
                // forms stay unsolved for the finalize hole check.
                let target_shape = match self.types.get(target_resolved) {
                    Type::Tensor(_, shape) => Some(*shape),
                    _ => None,
                };
                if let Some(shape) = target_shape {
                    super::tensor_index::constrain_conversio_target_hole_from_flat_count(
                        self, source, shape, span,
                    );
                }
                self.register_lista_to_tensor_conversio_prereqs();
                return None;
            }
            Err(ListaToTensorError::NonListaSource) => {}
            Err(ListaToTensorError::NestedLista) => {
                return self.check_rectangular_lista_literal_conversio(
                    source,
                    target_resolved,
                    span,
                );
            }
            Err(error) => return self.reject_lista_to_tensor_conversio(error, span),
        }
        let source_shape = self.collection_conversio_shape(source_resolved)?;
        let target_shape = self.collection_conversio_shape(target_resolved)?;
        if source_shape.family == target_shape.family {
            return None;
        }

        if self.is_allowed_collection_conversio(&source_shape, &target_shape) {
            return None;
        }

        let issue = Self::collection_conversio_rejection_issue(&source_shape, &target_shape);
        self.invalid_conversion_with_args(
            issue,
            span,
            &[
                ("source_family", source_shape.family.name()),
                ("target_family", target_shape.family.name()),
            ],
        );
        Some(self.error_type)
    }

    fn check_lista_to_vector_conversio(
        &mut self,
        source: &mut HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Type::Vector(target_elem, width) = self.types.get(target_resolved).clone() else {
            return None;
        };

        if crate::literal_vector_width(width, self.types).is_none() {
            self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_WIDTH, span);
            return Some(self.error_type);
        }

        if matches!(source.kind, HirExpressionKind::Array(_)) {
            self.check_verte_vector_target(source, target_elem, width, span);
            return Some(target_resolved);
        }

        let source_resolved = self.resolve_inferred_type(source_ty);
        let Type::Array(source_elem) = self.types.get(source_resolved).clone() else {
            self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_REQUIRED, span);
            return Some(self.error_type);
        };
        if !self.types.equals(source_elem, target_elem) {
            self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_ELEMENT_MISMATCH, span);
            return Some(self.error_type);
        }
        self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_REQUIRED, span);
        Some(self.error_type)
    }

    /// CTFP-1: non-literal `lista<T, magnitudo N> ↦ vector<T, size N>` — a bounded lista with
    /// a literal capacity that exactly matches the vector width materializes
    /// into the register; unbounded `lista<T>` stays literal-only.
    fn check_bounded_lista_to_vector_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Type::Vector(target_elem, width) = self.types.get(target_resolved).clone() else {
            return None;
        };
        let target_width = crate::literal_vector_width(width, self.types)?;
        let source_resolved = self.resolve_inferred_type(source_ty);
        let Type::BoundedArray {
            element: source_elem,
            capacity,
        } = self.types.get(source_resolved).clone()
        else {
            return None;
        };
        if !self.types.equals(source_elem, target_elem) {
            self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_ELEMENT_MISMATCH, span);
            return Some(self.error_type);
        }
        match self.types.get_index(capacity) {
            IndexExpr::Literal(capacity) if u64::from(target_width) == *capacity => {
                Some(target_resolved)
            }
            _ => {
                self.invalid_conversion(ISSUE_LISTA_TO_VECTOR_LITERAL_WIDTH, span);
                Some(self.error_type)
            }
        }
    }

    /// Nested `lista` literal → register `matrix<T, [R, C]>` (CPU subset ≤4×4).
    ///
    /// A nested literal source validates strictly against a literal shape
    /// (row count, per-row column count, element types). A runtime `lista<T>`
    /// source materializes into a literal or dimension-neutral (parametric)
    /// shape on an element-type contract: the runtime extent is the
    /// materialization's own contract, never a static check.
    fn check_lista_to_matrix_conversio(
        &mut self,
        source: &mut HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Type::Matrix(target_elem, shape) = self.types.get(target_resolved).clone() else {
            return None;
        };

        let Some((rows, columns)) = crate::literal_matrix_shape(shape, self.types) else {
            // S0-G9: dimension-neutral target — a parametric shape has no
            // statically known extent, so only a runtime lista source can
            // materialize; a literal source cannot be matched to unknown dims.
            if matches!(source.kind, HirExpressionKind::Array(_)) {
                self.invalid_conversion(ISSUE_MATRIX_MATERIALIZE_UNSUPPORTED, span);
                return Some(self.error_type);
            }
            return self.check_lista_source_materializes_to_matrix(
                source_ty,
                target_resolved,
                span,
            );
        };
        if rows > 4 || columns > 4 {
            self.invalid_conversion(ISSUE_MATRIX_MATERIALIZE_UNSUPPORTED, span);
            return Some(self.error_type);
        }

        let HirExpressionKind::Array(row_elements) = &mut source.kind else {
            // S0-G9: a runtime lista source materializes into a literal shape;
            // the element type is the static contract, the extent is runtime.
            return self.check_lista_source_materializes_to_matrix(
                source_ty,
                target_resolved,
                span,
            );
        };
        if row_elements.len() != usize::from(rows) {
            self.invalid_conversion(ISSUE_RECTANGULAR_LISTA_LITERAL_SHAPE, span);
            return Some(self.error_type);
        }

        for row in row_elements.iter_mut() {
            match row {
                HirArrayElement::Spread(expr) => {
                    self.check_expr(expr);
                    self.invalid_conversion(ISSUE_RECTANGULAR_LISTA_LITERAL_SPREAD, span);
                    return Some(self.error_type);
                }
                HirArrayElement::Infer(_) => {
                    self.invalid_operand_types(
                        super::aggregate::ISSUE_SHAPE_HOLE_REQUIRES_SHAPE_ARGUMENT,
                        span,
                    );
                    return Some(self.error_type);
                }
                HirArrayElement::Expr(row_expr) => {
                    let HirExpressionKind::Array(cells) = &mut row_expr.kind else {
                        self.check_expr(row_expr);
                        self.invalid_conversion(ISSUE_LISTA_TO_TENSOR_NESTED, span);
                        return Some(self.error_type);
                    };
                    if cells.len() != usize::from(columns) {
                        self.invalid_conversion(ISSUE_RECTANGULAR_LISTA_LITERAL_ROW_LENGTH, span);
                        return Some(self.error_type);
                    }
                    for cell in cells.iter_mut() {
                        match cell {
                            HirArrayElement::Spread(expr) => {
                                self.check_expr(expr);
                                self.invalid_conversion(
                                    ISSUE_RECTANGULAR_LISTA_LITERAL_SPREAD,
                                    span,
                                );
                                return Some(self.error_type);
                            }
                            HirArrayElement::Infer(_) => {
                                self.invalid_operand_types(
                                    super::aggregate::ISSUE_SHAPE_HOLE_REQUIRES_SHAPE_ARGUMENT,
                                    span,
                                );
                                return Some(self.error_type);
                            }
                            HirArrayElement::Expr(cell_expr) => {
                                let cell_ty =
                                    self.check_expr_with_expected(cell_expr, Some(target_elem));
                                self.unify_join(
                                    cell_ty,
                                    target_elem,
                                    cell_expr.span,
                                    ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH,
                                );
                            }
                        }
                    }
                }
            }
        }
        Some(target_resolved)
    }

    /// Runtime `lista<T>` → `matrix<T, [R, C]>` materialization: the element
    /// type must match; the runtime extent (R×C cells) is the conversion's
    /// own contract. Bounded and unbounded listas both materialize.
    fn check_lista_source_materializes_to_matrix(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Type::Matrix(target_elem, _) = self.types.get(target_resolved).clone() else {
            return None;
        };
        let source_resolved = self.resolve_inferred_type(source_ty);
        let (Type::Array(source_elem)
        | Type::BoundedArray {
            element: source_elem,
            ..
        }
        | Type::RangedArray {
            element: source_elem,
            ..
        }) = self.types.get(source_resolved).clone()
        else {
            self.invalid_conversion(ISSUE_MATRIX_MATERIALIZE_UNSUPPORTED, span);
            return Some(self.error_type);
        };
        if !self.types.equals(source_elem, target_elem) {
            self.invalid_conversion(ISSUE_LISTA_TO_TENSOR_ELEMENT_MISMATCH, span);
            return Some(self.error_type);
        }
        Some(target_resolved)
    }

    fn check_rectangular_lista_literal_conversio(
        &mut self,
        source: &mut HirExpression,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if !matches!(source.kind, HirExpressionKind::Array(_)) {
            self.invalid_conversion(
                rectangular_lista_literal_issue(RectangularListaLiteralError::NonNestedLiteral),
                span,
            );
            return Some(self.error_type);
        }

        if let Some(expected) = self.expected_nested_lista_literal_type(target_resolved) {
            self.check_expr_with_expected(source, Some(expected));
        }

        match analyze_rectangular_lista_literal(source, target_resolved, self.types) {
            Ok(_) => {
                self.register_lista_to_tensor_conversio_prereqs();
                None
            }
            Err(error) => {
                self.invalid_conversion(rectangular_lista_literal_issue(error), span);
                Some(self.error_type)
            }
        }
    }

    /// Padded-snapshot facts for a list literal whose element type differs
    /// from the tensor element only because its constants are untyped: every
    /// item must adopt the tensor element (an integer constant into a numeric
    /// element, a float constant into a float element) or already be of it.
    /// A non-literal source never gets here: it meets the flat route's strict
    /// element rule in `resolve_bounded_lista_padded_shape`.
    fn literal_padded_shape(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
    ) -> Option<(TypeId, i64)> {
        let HirExpressionKind::Array(items) = &source.kind else {
            return None;
        };
        let source_resolved = self.resolve_inferred_type(source_ty);
        let (elem, target_elem, dim) =
            padded_literal_facts(source_resolved, target_resolved, self.types)?;
        let adopts = items.iter().all(|item| match item {
            HirArrayElement::Expr(expr) => self.literal_item_adopts(expr, target_elem),
            HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => false,
        });
        adopts.then_some((elem, dim))
    }

    fn literal_item_adopts(&mut self, expr: &HirExpression, target_elem: TypeId) -> bool {
        if static_integer_magnitude(expr).is_some() {
            return is_numeric_element(target_elem, self.types);
        }
        if static_float_magnitude(expr).is_some() {
            return matches!(
                self.types.get(target_elem),
                Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _)
            );
        }
        expr.ty.is_some_and(|ty| {
            let resolved = self.resolve_inferred_type(ty);
            self.types.equals(resolved, target_elem)
        })
    }

    fn register_lista_to_tensor_conversio_prereqs(&mut self) {
        ensure_lista_numerus_type(self.types);
    }

    fn reject_lista_to_tensor_conversio(
        &mut self,
        error: ListaToTensorError,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let issue = lista_to_tensor_issue(error)?;
        self.invalid_conversion(issue, span);
        Some(self.error_type)
    }

    fn expected_nested_lista_literal_type(&mut self, target_resolved: TypeId) -> Option<TypeId> {
        let Type::Tensor(elem, _) = self.types.get(target_resolved).clone() else {
            return None;
        };
        let inner = self.types.array(elem);
        Some(self.types.array(inner))
    }

    fn collection_conversio_shape(&self, ty: TypeId) -> Option<CollectionConversioShape> {
        match self.types.get(ty) {
            Type::Array(elem)
            | Type::BoundedArray { element: elem, .. }
            | Type::RangedArray { element: elem, .. } => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Array,
                elem: Some(*elem),
                key: None,
                value: None,
            }),
            Type::Set(elem) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Set,
                elem: Some(*elem),
                key: None,
                value: None,
            }),
            Type::Map(key, value) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Map,
                elem: None,
                key: Some(*key),
                value: Some(*value),
            }),
            Type::Tensor(elem, _) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Tensor,
                elem: Some(*elem),
                key: None,
                value: None,
            }),
            Type::Cursor(elem) => Some(CollectionConversioShape {
                family: CollectionConversioFamily::Cursor,
                elem: Some(*elem),
                key: None,
                value: None,
            }),
            Type::Promissum(elem) | Type::PromissumFailable(elem, _) => {
                Some(CollectionConversioShape {
                    family: CollectionConversioFamily::Promissum,
                    elem: Some(*elem),
                    key: None,
                    value: None,
                })
            }
            _ => None,
        }
    }

    fn is_allowed_collection_conversio(
        &self,
        source: &CollectionConversioShape,
        target: &CollectionConversioShape,
    ) -> bool {
        match (source.family, target.family) {
            (
                CollectionConversioFamily::Array,
                CollectionConversioFamily::Set
                | CollectionConversioFamily::Tensor
                | CollectionConversioFamily::Cursor,
            )
            | (
                CollectionConversioFamily::Set
                | CollectionConversioFamily::Tensor
                | CollectionConversioFamily::Cursor,
                CollectionConversioFamily::Array,
            ) => self.collection_elems_match(source.elem, target.elem),
            (CollectionConversioFamily::Map, CollectionConversioFamily::Array) => {
                let (Some(key), Some(value), Some(elem)) = (source.key, source.value, target.elem)
                else {
                    return false;
                };
                self.types.equals(key, elem) || self.types.equals(value, elem)
            }
            _ => false,
        }
    }

    fn collection_elems_match(&self, left: Option<TypeId>, right: Option<TypeId>) -> bool {
        match (left, right) {
            (Some(a), Some(b)) => self.types.equals(a, b),
            _ => false,
        }
    }

    fn collection_conversio_rejection_issue(
        source: &CollectionConversioShape,
        target: &CollectionConversioShape,
    ) -> &'static str {
        match (source.family, target.family) {
            (CollectionConversioFamily::Array, CollectionConversioFamily::Map) => {
                ISSUE_LISTA_TO_TABULA_UNSUPPORTED
            }
            _ => ISSUE_CROSS_COLLECTION_UNSUPPORTED,
        }
    }

    /// D7's explicit ASCII buffer bridges. Bounded values copy their valid
    /// prefix into an unbounded carrier; the reverse direction is fallible and
    /// keeps the declared capacity for the Rust carrier to enforce.
    /// `ascii<N> ↔ ascii<M>` resizes by copying the initialized prefix with a
    /// fail-closed overflow when the payload outgrows the target capacity.
    fn is_bounded_ascii_conversio(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        matches!(
            (
                self.types.get(self.resolve_inferred_type(source_ty)),
                self.types.get(self.resolve_inferred_type(target_ty)),
            ),
            (Type::BoundedAscii { .. }, Type::BoundedAscii { .. })
                | (
                    Type::BoundedAscii { .. },
                    Type::Primitive(Primitive::Ascii | Primitive::Textus)
                )
                | (Type::Primitive(Primitive::Ascii), Type::BoundedAscii { .. })
        )
    }

    /// D7 fail-closed overflow for literal sources: a string literal entering
    /// a bounded carrier must fit within the declared capacity. Non-literal
    /// sources keep the runtime-carrier overflow contract.
    fn check_bounded_string_literal_overflow(
        &mut self,
        source: &HirExpression,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let HirExpressionKind::Literal(literal) = &source.kind else {
            return None;
        };
        let (sym, ascii) = match literal {
            HirLiteral::Ascii(sym) => (*sym, true),
            HirLiteral::String(sym) => (*sym, false),
            _ => return None,
        };
        let target = self.types.get(self.resolve_inferred_type(target_resolved));
        let capacity = match target {
            Type::BoundedAscii { capacity } if ascii => *capacity,
            Type::BoundedTextus { capacity } if !ascii => *capacity,
            _ => return None,
        };
        let resolved_capacity = self.resolve_index(capacity);
        let IndexExpr::Literal(capacity) = self.types.get_index(resolved_capacity) else {
            return None;
        };
        let raw = self.interned_literal_payload(sym)?;
        let decoded_len: u64 = if ascii {
            radix_lexer::ascii_literal::decode_ascii_literal(raw)
                .ok()?
                .len() as u64
        } else {
            radix_lexer::textus_literal::decode_textus_literal(raw)
                .chars()
                .count() as u64
        };
        if decoded_len > *capacity {
            let issue = if ascii {
                ISSUE_ASCII_LITERAL_TOO_LONG
            } else {
                ISSUE_TEXTUS_LITERAL_TOO_LONG
            };
            self.invalid_operand_types_with_args(
                issue,
                span,
                &[("capacity", capacity.to_string())],
            );
            return Some(self.error_type);
        }
        None
    }

    /// D7's explicit bounded-byte bridges. Bounded → unbounded copies the
    /// initialized prefix; unbounded → bounded keeps `N` for the host carrier
    /// to enforce with a fail-closed overflow when `len > N`.
    fn is_bounded_octeti_conversio(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        matches!(
            (
                self.types.get(self.resolve_inferred_type(source_ty)),
                self.types.get(self.resolve_inferred_type(target_ty)),
            ),
            (
                Type::BoundedOcteti { .. },
                Type::Primitive(Primitive::Octeti)
            ) | (
                Type::Primitive(Primitive::Octeti),
                Type::BoundedOcteti { .. }
            )
        )
    }

    /// D7's explicit bounded-text bridges. Bounded → unbounded copies the
    /// initialized scalar prefix; unbounded → bounded keeps `N` for the host
    /// carrier to enforce with a fail-closed overflow.
    fn is_bounded_textus_conversio(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        matches!(
            (
                self.types.get(self.resolve_inferred_type(source_ty)),
                self.types.get(self.resolve_inferred_type(target_ty)),
            ),
            (
                Type::BoundedTextus { .. },
                Type::Primitive(Primitive::Textus)
            ) | (
                Type::Primitive(Primitive::Textus),
                Type::BoundedTextus { .. }
            )
        )
    }

    /// `[a, b, c] ↦ list<T, size lo‥hi>`: a list literal's length is known when
    /// nothing is spread into it, so a length outside the target's range is
    /// refused here (`Some(error)`); every other source is checked when the
    /// conversion runs.
    fn check_ranged_literal_conversio(
        &mut self,
        source: &HirExpression,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let target = self.resolve_inferred_type(target_resolved);
        if !matches!(self.types.get(target), Type::RangedArray { .. }) {
            return None;
        }
        let HirExpressionKind::Array(elements) = &source.kind else {
            return None;
        };
        if !elements
            .iter()
            .all(|element| matches!(element, HirArrayElement::Expr(_)))
        {
            return None;
        }
        let checked = self.check_ranged_literal_length(target, elements.len(), span);
        (checked == self.error_type).then_some(checked)
    }

    /// D7's explicit bounded-list bridges. Bounded → unbounded copies the
    /// initialized prefix; unbounded → bounded keeps `N` for the host carrier
    /// to enforce with a fail-closed overflow.
    fn is_bounded_lista_conversio(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        match (
            self.types.get(self.resolve_inferred_type(source_ty)),
            self.types.get(self.resolve_inferred_type(target_ty)),
        ) {
            (
                Type::BoundedArray { element: from, .. } | Type::RangedArray { element: from, .. },
                Type::Array(to),
            )
            | (
                Type::Array(from),
                Type::BoundedArray { element: to, .. } | Type::RangedArray { element: to, .. },
            ) => self.types.equals(*from, *to),
            // A length range converts to and from the bounded list and to
            // another range: the destination range is checked when the
            // conversion runs.
            (
                Type::BoundedArray { element: from, .. } | Type::RangedArray { element: from, .. },
                Type::RangedArray { element: to, .. },
            )
            | (Type::RangedArray { element: from, .. }, Type::BoundedArray { element: to, .. }) => {
                self.types.equals(*from, *to)
            }
            _ => false,
        }
    }

    fn primitive_conversio_family(&self, ty: TypeId) -> PrimitiveConversioFamily {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Primitive(Primitive::Numerus)
            | Type::SizedNumeric(Primitive::Numerus, _)
            | Type::RangedInt { .. } => PrimitiveConversioFamily::Numerus,
            Type::Primitive(Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Fractus, _)
            | Type::PolicyFloat(_, _) => PrimitiveConversioFamily::Fractus,
            Type::Primitive(Primitive::Bivalens) => PrimitiveConversioFamily::Bivalens,
            Type::Primitive(Primitive::Textus) => PrimitiveConversioFamily::Textus,
            Type::Primitive(Primitive::Ascii) => PrimitiveConversioFamily::Ascii,
            // D17 / BB-2: `'x'` is `ascii<1>`. Same conversio family as bare
            // ascii so `'a' ↦ textus` / `s[i] ↦ numerus` stay explicit widens.
            // Multi-char `ascii<N>` converts through the D7 bounded-string
            // bridges instead of this scalar family.
            Type::BoundedAscii { capacity }
                if matches!(self.types.get_index(*capacity), IndexExpr::Literal(1)) =>
            {
                PrimitiveConversioFamily::Ascii
            }
            Type::Primitive(Primitive::Octeti) => PrimitiveConversioFamily::Octeti,
            // `octeti` (en `bytes`) is `lista<b8>`: the byte-string family.
            Type::Array(element)
                if self.bits_width_of(*element) == Some(radix_types::BitsWidth::B8) =>
            {
                PrimitiveConversioFamily::Octeti
            }
            Type::Primitive(Primitive::Regex) => PrimitiveConversioFamily::Regex,
            _ => PrimitiveConversioFamily::Other,
        }
    }

    /// Fallthrough conversio: scalar-to-scalar, valor extraction, collection
    /// conversions, and same-type conversions are allowed by backend-defined
    /// rules. Nominal types (struct, enum, func) converted to unrelated
    /// scalars are rejected here.
    fn conversio_fallback_allowed(&self, source_ty: TypeId, target_resolved: TypeId) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        let target = target_resolved;

        if self.types.equals(source, target) {
            return true;
        }

        // D10.5: `littera` converts only along its widening chain
        // (`ascii<1> ↦ littera ↦ textus`) here. The remaining text and code
        // point conversions are D10.6 rows, not scalar fallthrough.
        if self.is_littera(source) || self.is_littera(target) {
            return self.types.is_assignable(source, target);
        }

        if self.is_record_type(source) || self.is_record_type(target) {
            return false;
        }

        // D11.1: `saturatus<W>` converts only to and from integer families
        // (a range-checked narrowing, never a clamp or a text parse).
        if self.is_saturating_word(source) || self.is_saturating_word(target) {
            return self.is_integer_conversio_operand(source)
                && self.is_integer_conversio_operand(target);
        }

        for ty in [source, target] {
            match self.types.get(ty) {
                Type::Primitive(Primitive::Valor | Primitive::Ignotum)
                | Type::Param(_)
                | Type::Infer(_) => return true,
                _ => {}
            }
        }

        // Collection and structural types have their own conversion rules
        // handled by earlier specialized checks — don't block them here.
        if self.is_structural_or_collection_type(source)
            || self.is_structural_or_collection_type(target)
        {
            return true;
        }

        // Bounded `octeti<N>` → numeric scalar is the endian-decode read row
        // vetted by `check_rejected_primitive_conversio` above; the fallback
        // need not re-judge the family.
        if matches!(self.types.get(source), Type::BoundedOcteti { .. })
            && matches!(
                self.primitive_conversio_family(target),
                PrimitiveConversioFamily::Numerus | PrimitiveConversioFamily::Fractus
            )
        {
            return true;
        }

        let source_family = self.primitive_conversio_family(source);
        let target_family = self.primitive_conversio_family(target);
        let is_scalar = |family: PrimitiveConversioFamily, ty: TypeId| -> bool {
            !matches!(family, PrimitiveConversioFamily::Other)
                || matches!(
                    self.types.get(ty),
                    Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
                )
        };
        if is_scalar(source_family, source) && is_scalar(target_family, target) {
            return true;
        }

        false
    }

    /// Specialized conversio rules for these types are handled earlier.
    fn is_structural_or_collection_type(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(ty),
            Type::Array(_)
                | Type::BoundedArray { .. }
                | Type::RangedArray { .. }
                | Type::Set(_)
                | Type::Map(_, _)
                | Type::Tensor(_, _)
                | Type::Cursor(_)
                | Type::AsyncCursor(_, _)
                | Type::Promissum(_)
                | Type::PromissumFailable(_, _)
                | Type::Vector(_, _)
                | Type::Matrix(_, _)
                | Type::Sparsa(_, _)
                | Type::Option(_)
                | Type::Ref(_, _)
                | Type::Atomic(_)
                | Type::Intervallum(..)
                | Type::Alias(_, _)
                | Type::Union(_)
                | Type::Tuple(_)
                | Type::ModularWord(_)
                | Type::RangedInt { .. }
        )
    }

    fn peel_alias_chain(&self, ty: TypeId) -> TypeId {
        let mut ty = ty;
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = *inner;
        }
        ty
    }

    fn is_policy_word(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.peel_alias_chain(ty)),
            Type::ModularWord(_)
                | Type::SaturatingWord(_)
                | Type::RangedInt {
                    policy: radix_types::IntPolicy::Wrapping | radix_types::IntPolicy::Saturating,
                    ..
                }
        )
    }

    /// A float source into a policy word can fail at run time (NaN, ±∞)
    /// unless the source is a finite literal.
    fn float_word_conversio_can_fail(&self, source: &HirExpression, source_ty: TypeId) -> bool {
        let source_ty = self.peel_alias_chain(self.resolve_inferred_type(source_ty));
        matches!(
            self.types.get(source_ty),
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _)
        ) && static_float_magnitude(source).is_none_or(|value| !value.is_finite())
    }

    /// Sources of a `wrapping`/`saturating` conversion (ruling 24): integers of
    /// any policy family, and `d64`. Float and other sources are not admitted.
    fn is_policy_word_conversio_source(&self, ty: TypeId) -> bool {
        let ty = self.resolve_inferred_type(ty);
        self.is_integer_conversio_operand(ty)
            || matches!(
                self.types.get(self.peel_alias_chain(ty)),
                Type::SizedNumeric(Primitive::Numerus, width) if *width == NumericWidth::D64
            )
    }

    fn is_saturating_word(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.peel_alias_chain(ty)),
            Type::SaturatingWord(_)
                | Type::RangedInt {
                    policy: radix_types::IntPolicy::Saturating,
                    ..
                }
        )
    }

    /// Integer operand of a saturating-word conversion: `numerus<W>` (not a
    /// decimal width), `modulus<W>`, or `saturatus<W>`.
    fn is_integer_conversio_operand(&self, ty: TypeId) -> bool {
        match self.types.get(self.peel_alias_chain(ty)) {
            Type::Primitive(Primitive::Numerus)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. } => true,
            Type::SizedNumeric(Primitive::Numerus, width) => !width.is_decimal(),
            _ => false,
        }
    }

    fn is_record_type(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Record(_)
        )
    }

    /// Non-identity conversio involving `Type::Record` must reconstruct.
    ///
    /// Identical Record↔Record `↦` stays (equals). Ratio↔genus and
    /// genus↔ratio take the structural path only when
    /// [`Self::record_genus_conversion_is_structural`] proves the field sets
    /// match; mismatched Record↔genus and Record↔Record reconstruct.
    fn record_conversion_requires_reconstruction(
        &self,
        source_ty: TypeId,
        target_resolved: TypeId,
    ) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        if self.types.equals(source, target_resolved) {
            return false;
        }
        self.is_record_type(source) || self.is_record_type(target_resolved)
    }

    /// Operator ruling 2026-10-14: `r ↦ Point` and `p ↦ record<…>` are
    /// explicit field-by-field conversions when both sides carry exactly the
    /// same field symbols and exact field types — field order ignored, no
    /// width subtyping. Returns `true` for a structurally matching
    /// Record↔genus pair in either direction.
    fn record_genus_conversion_is_structural(
        &self,
        source_ty: TypeId,
        target_resolved: TypeId,
    ) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        let target = self.resolve_inferred_type(target_resolved);
        let (record_fields, struct_def) = match (self.types.get(source), self.types.get(target)) {
            (Type::Record(fields), Type::Struct(def))
            | (Type::Struct(def), Type::Record(fields)) => (fields.clone(), *def),
            _ => return false,
        };
        let Some(info) = self.structs.get(&struct_def) else {
            return false;
        };
        record_fields.len() == info.fields.len()
            && record_fields.iter().all(|(name, record_field_ty)| {
                info.fields.get(name).is_some_and(|field| {
                    self.types.equals(
                        self.resolve_inferred_type(*record_field_ty),
                        self.resolve_inferred_type(field.ty),
                    )
                })
            })
    }

    /// Bare failable `↦` propagates through the active alternate-exit channel.
    /// The propagated payload is text in v1, so reject incompatible channels at
    /// typecheck instead of letting Rust codegen or the MIR runner rediscover it.
    pub(super) fn check_conversio_propagation_channel(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) {
        if !self.conversio_can_propagate_failure(source_ty, target_resolved) {
            return;
        }
        self.check_text_failure_channel(source_ty, target_resolved, span);
    }

    /// D17.8: a failable `↦` with no `⊥` default and no enclosing handler
    /// (`fac`/`cape`, `capta`, or a function declaring `⇥ E`) is a compile
    /// error, the same rule failable calls meet. The `main {}` body is
    /// exempt: an unhandled failure there panics. Returns `true` when the
    /// conversion was rejected.
    fn reject_unhandled_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        if self.in_main_frame {
            return false;
        }
        let source = self.format_union_constituent_type(self.resolve_inferred_type(source_ty));
        let target = self.format_union_constituent_type(target_resolved);
        self.invalid_operand_types_with_args(
            ISSUE_FAILABLE_CONVERSIO_REQUIRES_HANDLER,
            span,
            &[("source", source), ("target", target)],
        );
        true
    }

    /// A bare failable `↦` propagates its text failure payload through the
    /// active alternate-exit channel, which must accept `textus`.
    pub(super) fn check_text_failure_channel(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        span: radix_lexer::Span,
    ) {
        let Some(error_ty) = self.current_error.map(|sink| match sink {
            ErrorSink::Function(ty) | ErrorSink::Local(ty) => ty,
            ErrorSink::Trap(var) => self.trap_sink_type(var),
        }) else {
            self.reject_unhandled_conversio(source_ty, target_resolved, span);
            return;
        };
        // UT: a bare failable `↦` inside a `capta` accumulates its text
        // failure payload into the trap hole, exactly as a failable call's
        // error type does (`reject_failable_call`).
        if matches!(self.current_error, Some(ErrorSink::Trap(_))) {
            let textus = self.textus_type();
            self.unify_assign_issue(
                textus,
                error_ty,
                span,
                super::control::ISSUE_TRAP_MEMBER_TYPE_MISMATCH,
            );
            return;
        }
        let error_ty = self.resolve_inferred_type(error_ty);
        if self.is_infer(error_ty) {
            let textus = self.textus_type();
            self.unify_assign_issue(
                textus,
                error_ty,
                span,
                ISSUE_CONVERSIO_FAILURE_CHANNEL_TYPE_MISMATCH,
            );
            return;
        }
        if matches!(
            self.types.get(error_ty),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        ) {
            return;
        }
        self.invalid_conversion(ISSUE_CONVERSIO_FAILURE_CHANNEL_TEXTUS_REQUIRED, span);
    }

    /// A bare failable `sermo ↦ T` materialization propagates the leaf failure
    /// record (`valor`, holding `kind` and `path`), except for a route served by
    /// a visible Faber handler, whose thrown failure is `textus`. For a leaf
    /// failure, the active channel can accept `valor` via the `ignotum` escape,
    /// a `valor` channel, or a registered `@ conversio` row from `valor` to the
    /// declared channel type. The row's own `⇥ E` composes
    /// through the use-site channel exactly as `reject_failable_call` does,
    /// and its function is recorded on `pending_conversion` so lowering applies
    /// it. Without an active channel the hard-failure posture stands: the
    /// materialization panics (or is caught by a surrounding `fac`/`cape`).
    pub(super) fn check_sermo_failure_channel(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        faber_served: bool,
        span: radix_lexer::Span,
    ) {
        let Some(error_ty) = self.current_error.map(|sink| match sink {
            ErrorSink::Function(ty) | ErrorSink::Local(ty) => ty,
            ErrorSink::Trap(var) => self.trap_sink_type(var),
        }) else {
            return;
        };
        if faber_served {
            self.check_text_failure_channel(source_ty, target_resolved, span);
            return;
        }
        let value_ty = self.types.primitive(Primitive::Valor);
        // UT: a bare failable `↦` inside a `capta` accumulates its failure
        // payload into the trap hole, exactly as a failable call's error type
        // does (`reject_failable_call`).
        if matches!(self.current_error, Some(ErrorSink::Trap(_))) {
            self.unify_assign_issue(
                value_ty,
                error_ty,
                span,
                super::control::ISSUE_TRAP_MEMBER_TYPE_MISMATCH,
            );
            return;
        }
        let error_ty = self.resolve_inferred_type(error_ty);
        if self.is_infer(error_ty) {
            self.unify_assign_issue(
                value_ty,
                error_ty,
                span,
                ISSUE_CONVERSIO_FAILURE_CHANNEL_TYPE_MISMATCH,
            );
            return;
        }
        if matches!(
            self.types.get(error_ty),
            Type::Primitive(Primitive::Valor | Primitive::Ignotum)
        ) {
            return;
        }
        if let Some(row) = self.resolver.conversion_row_for(value_ty, error_ty)
            && let Some(fn_id) = row.fn_def_id
        {
            if let Some(channel) = self.functions.get(&fn_id).and_then(|sig| sig.err)
                && !self.check_registered_row_channel(channel, span)
            {
                return;
            }
            self.pending_conversion = Some(fn_id);
            return;
        }
        self.invalid_conversion(ISSUE_CONVERSIO_FAILURE_CHANNEL_VALUE_REQUIRED, span);
    }

    /// Compose a registered row's declared `⇥ E` channel through the active
    /// use-site channel (CTR-02).
    ///
    /// A bare `↦` through a registered row whose conversion function declares
    /// `⇥ E` propagates failure with payload type `E`. The use-site channel
    /// must accept `E`: either `E` is assignable to the sink type, the sink is
    /// the `ignotum` escape, or the sink is an inference variable unified with
    /// `E`. A mismatch retains the SEM010-shaped `failable_call_conversion_absent`
    /// diagnostic with the declared channel as `source` and the use-site
    /// channel as `destination`. Returns `false` when the mismatch was
    /// reported.
    pub(super) fn check_registered_row_channel(
        &mut self,
        channel: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        let Some(error_ty) = self.current_error.map(|sink| match sink {
            ErrorSink::Function(ty) | ErrorSink::Local(ty) => ty,
            ErrorSink::Trap(var) => self.trap_sink_type(var),
        }) else {
            return true;
        };
        let error_ty = self.resolve_inferred_type(error_ty);
        if self.is_infer(error_ty) {
            self.unify_assign_issue(
                channel,
                error_ty,
                span,
                ISSUE_CONVERSIO_FAILURE_CHANNEL_TYPE_MISMATCH,
            );
            return true;
        }
        if matches!(
            self.types.get(error_ty),
            Type::Primitive(Primitive::Ignotum)
        ) {
            return true;
        }
        if self
            .types
            .is_assignable(self.resolve_inferred_type(channel), error_ty)
        {
            return true;
        }
        self.invalid_operand_types_with_args(
            super::call::ISSUE_FAILABLE_CALL_CONVERSION_ABSENT,
            span,
            &[
                ("source", self.format_union_constituent_type(channel)),
                ("destination", self.format_union_constituent_type(error_ty)),
            ],
        );
        false
    }

    /// Whether a bare numeric `↦` into a trapping integer type can fail at run
    /// time: integer narrowing or signedness change, float -> integer, and
    /// decimal -> integer. Widening and a literal proven to fit cannot fail.
    /// Policy-word targets (`wrapping`/`saturating`) never reach here.
    fn numeric_conversio_can_fail(
        &self,
        source: &HirExpression,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
    ) -> bool {
        let source_ty = self.peel_alias_chain(self.resolve_inferred_type(source_ty));
        let target_ty = self.peel_alias_chain(target_resolved);
        let (Some(from), Some(to)) = (
            numeric_fact(self.types.get(source_ty)),
            numeric_fact(self.types.get(target_ty)),
        ) else {
            return false;
        };
        // Ruling 40, into `inf`: an integer of any family converts exactly; a
        // float fails only at NaN and ±∞ (a finite constant is proven); a
        // decimal truncates into it.
        if to.family == NumericFactFamily::Unbounded {
            return match from.family {
                NumericFactFamily::Float { .. } => {
                    static_float_magnitude(source).is_none_or(|value| !value.is_finite())
                }
                _ => false,
            };
        }
        // Ruling 40, out of `inf` into `d64`: checked into the carrier range
        // (integer part at most 92,233,720,368), so failable unless a constant
        // proves it fits. Every other decimal target keeps the row below.
        if from.family == NumericFactFamily::Unbounded && to.family == NumericFactFamily::Decimal {
            const D64_INTEGER_PART_MAX: i128 = 92_233_720_368;
            return static_integer_magnitude(source)
                .is_none_or(|value| value.abs() > D64_INTEGER_PART_MAX);
        }
        // A trapping ranged target checks the range, not just the width: only
        // a literal inside it, or a ranged cell it contains, cannot fail.
        let ranged_target = match self.types.ranged_int_parts(target_ty) {
            Some((radix_types::IntPolicy::Trapping, width, lo, hi)) => Some((width, (lo, hi))),
            _ => None,
        };
        let Some(target_width) = ranged_target
            .map(|(width, _)| width)
            .or_else(|| self.trapping_integer_width(target_ty, &to))
        else {
            return false;
        };
        let target_range = ranged_target
            .map(|(_, range)| range)
            .or_else(|| integer_width_range(target_width));
        if ranged_target.is_some() && self.types.is_assignable(source_ty, target_ty) {
            return false;
        }
        match from.family {
            NumericFactFamily::Float { .. } => {
                let Some(value) = static_float_magnitude(source) else {
                    return true;
                };
                let mode = self.interner.and_then(|interner| {
                    params
                        .iter()
                        .find_map(|param| match interner.resolve(*param) {
                            "floor" | "Floor" => Some("floor"),
                            "ceil" | "Ceil" => Some("ceil"),
                            "half_even" | "HalfEven" => Some("half_even"),
                            "trunc" | "Trunc" => Some("trunc"),
                            _ => None,
                        })
                });
                !target_range.is_some_and(|range| float_rounding_fits(value, mode, range))
            }
            NumericFactFamily::Decimal => {
                // d64 integer parts fit i64.
                ranged_target.is_some()
                    || !matches!(to.family, NumericFactFamily::SignedInt { rank: t } if t >= 3)
            }
            // `inf` into a bounded trapping integer is the magnitude-checked
            // narrowing: failable unless a constant proves it fits.
            NumericFactFamily::Unbounded
            | NumericFactFamily::SignedInt { .. }
            | NumericFactFamily::UnsignedInt { .. }
            | NumericFactFamily::Modular { .. }
            | NumericFactFamily::ModularSigned { .. }
            | NumericFactFamily::SaturatingSigned { .. }
            | NumericFactFamily::SaturatingUnsigned { .. } => {
                if ranged_target.is_none() && from.family.integer_flows_into(to.family) {
                    return false;
                }
                let fits = static_integer_magnitude(source)
                    .zip(target_range)
                    .is_some_and(|(value, (lo, hi))| (lo..=hi).contains(&value));
                !fits
            }
        }
    }

    /// Carrier width of a trapping (non-policy, non-decimal) integer target.
    fn trapping_integer_width(
        &self,
        target_ty: TypeId,
        fact: &NumericFact,
    ) -> Option<NumericWidth> {
        match (self.types.get(target_ty), fact.family) {
            (Type::Primitive(Primitive::Numerus), NumericFactFamily::SignedInt { .. }) => {
                Some(NumericWidth::I64)
            }
            (
                Type::SizedNumeric(Primitive::Numerus, width),
                NumericFactFamily::SignedInt { .. } | NumericFactFamily::UnsignedInt { .. },
            ) => Some(*width),
            _ => None,
        }
    }

    fn conversio_can_propagate_failure(&self, source_ty: TypeId, target_resolved: TypeId) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        let target = self.resolve_inferred_type(target_resolved);
        if self.is_ordo_from_numerus(source, target) {
            return true;
        }
        if matches!(self.types.get(source), Type::Primitive(Primitive::Valor))
            && !matches!(self.types.get(target), Type::Primitive(Primitive::Valor))
        {
            return true;
        }
        if matches!(self.types.get(target), Type::Primitive(Primitive::Json)) {
            return matches!(
                self.types.get(source),
                Type::Primitive(Primitive::Textus | Primitive::Valor) | Type::Struct(_)
            );
        }
        if matches!(self.types.get(source), Type::Primitive(Primitive::Json))
            && matches!(self.types.get(target), Type::Struct(_))
        {
            return true;
        }
        if self.is_instans_type(target) {
            return matches!(
                self.types.get(source),
                Type::Primitive(Primitive::Textus | Primitive::Valor)
            );
        }
        match (
            self.primitive_conversio_family(source),
            self.primitive_conversio_family(target),
        ) {
            (
                PrimitiveConversioFamily::Textus | PrimitiveConversioFamily::Ascii,
                PrimitiveConversioFamily::Numerus
                | PrimitiveConversioFamily::Fractus
                | PrimitiveConversioFamily::Regex,
            )
            | (
                PrimitiveConversioFamily::Textus | PrimitiveConversioFamily::Octeti,
                PrimitiveConversioFamily::Ascii,
            )
            | (PrimitiveConversioFamily::Octeti, PrimitiveConversioFamily::Textus) => true,
            _ => matches!(
                (self.types.get(source), self.types.get(target)),
                (
                    Type::Array(_),
                    Type::Tensor(_, _) | Type::BoundedArray { .. }
                ) | (
                    // Into a length range any list can fail: the source
                    // length may fall outside `[min, max]`.
                    Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. },
                    Type::RangedArray { .. }
                ) | (
                    Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. },
                    Type::Tensor(_, _)
                )
            ),
        }
    }

    fn is_infallible_numeric_truthiness_conversio(
        &self,
        source_ty: TypeId,
        target_resolved: TypeId,
    ) -> bool {
        let source = self.resolve_inferred_type(source_ty);
        let target = self.resolve_inferred_type(target_resolved);
        matches!(
            (
                self.primitive_conversio_family(source),
                self.primitive_conversio_family(target)
            ),
            (
                PrimitiveConversioFamily::Numerus | PrimitiveConversioFamily::Fractus,
                PrimitiveConversioFamily::Bivalens
            )
        )
    }

    fn is_instans_type(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
        )
    }

    /// Validate object-literal `∷` field entries before typing the placeholder source.
    ///
    /// Returns `Some(target)` when entries own the evidence (struct/map shapes).
    fn check_verte_object_entries(
        &mut self,
        target_resolved: TypeId,
        entries: Option<&mut Vec<HirObjectField>>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let entries = entries?;
        match self.types.get(target_resolved).clone() {
            Type::Struct(def_id) => {
                // Bare non-generic target (or a generic target whose witnesses
                // could not be resolved): validate against the declared fields.
                self.check_struct_fields(def_id, entries, &[], target_resolved, span);
                Some(target_resolved)
            }
            Type::Applied(base, args) => {
                // The applied target carries the construction arguments;
                // substitute them into the field types. Size args bind
                // magnitudo slots (S0-G1), so the full list flows through —
                // filtering to type args would leave widths unsubstituted.
                if let Type::Struct(def_id) =
                    self.types.get(self.resolve_inferred_type(base)).clone()
                {
                    self.check_struct_fields(def_id, entries, &args, target_resolved, span);
                    return Some(target_resolved);
                }
                None
            }
            Type::Map(key_ty, value_ty) | Type::Ledger(key_ty, value_ty) => Some(
                self.check_verte_map_target(entries, key_ty, value_ty, span, target_resolved)
                    .unwrap_or(target_resolved),
            ),
            Type::Record(fields) => {
                self.check_record_fields(&fields, entries, span);
                Some(target_resolved)
            }
            _ => None,
        }
    }

    /// Exact field-set construction for `Type::Record` (ratio and CLI records).
    ///
    /// Unknown init fields reuse `unknown_record_field`; missing labels raise
    /// `ratio_field_missing`; each initializer typechecks against its slot.
    fn check_record_fields(
        &mut self,
        fields: &FxHashMap<Symbol, TypeId>,
        entries: &mut [HirObjectField],
        span: radix_lexer::Span,
    ) {
        let mut supplied = FxHashSet::default();
        for field in entries.iter_mut() {
            match &mut field.key {
                HirObjectKey::Ident(name) | HirObjectKey::String(name) => {
                    let Some(field_ty) = fields.get(name).copied() else {
                        let error_span = field.value.as_ref().map_or(span, |expr| expr.span);
                        self.error_issue(
                            SemanticErrorKind::VariableUndefined,
                            ISSUE_UNKNOWN_RECORD_FIELD,
                            error_span,
                        );
                        continue;
                    };
                    supplied.insert(*name);
                    let Some(value) = &mut field.value else {
                        self.invalid_operand_types(ISSUE_OBJECT_FIELD_REQUIRES_VALUE, span);
                        continue;
                    };
                    let site = StoreSite::new(HirStoreKind::Field).named(*name);
                    let value_ty = self.check_store_expr(value, field_ty, site);
                    self.unify_carrier_assign(
                        value_ty,
                        field_ty,
                        value.span,
                        ISSUE_FIELD_INITIALIZER_TYPE_MISMATCH,
                    );
                }
                HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => {
                    self.check_expr(expr);
                    self.error_issue(
                        SemanticErrorKind::VariableUndefined,
                        ISSUE_UNKNOWN_RECORD_FIELD,
                        expr.span,
                    );
                }
            }
        }
        for name in fields.keys() {
            if !supplied.contains(name) {
                self.undefined_member(ISSUE_RATIO_FIELD_MISSING, span);
            }
        }
    }

    /// Require inline `⊥` default to be a value of the conversio target type `T`.
    pub(super) fn note_conversio_recovery_checked(
        &mut self,
        recovery: &mut HirExpression,
        target_resolved: TypeId,
    ) {
        // Check under the target as expected type so literal operands erase
        // their exact bounds exactly like a declared-type initializer
        // (`sit lista<numerus> x ← [0]`).
        let site = StoreSite::new(HirStoreKind::ConversionDefault);
        let recovery_ty = self.check_store_expr(recovery, target_resolved, site);
        self.unify_assign_issue(
            recovery_ty,
            target_resolved,
            recovery.span,
            ISSUE_CONVERSIO_RECOVERY_TYPE_MISMATCH,
        );
    }

    /// Validates `↦ regex` operand shapes and rejects structurally empty `⊥` default.
    ///
    /// Returns `Some(error_type)` when the operand type is invalid; `None` when
    /// checking should continue toward the declared regex target.
    fn check_regex_conversio(
        &mut self,
        source: &mut HirExpression,
        source_ty: TypeId,
        recovery_present: bool,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source_resolved = self.resolve_inferred_type(source_ty);
        if self.is_ascii(source_resolved) {
            let textus = self.textus_type();
            self.unify_assign_issue(
                source_ty,
                textus,
                source.span,
                ISSUE_REGEX_CONVERSION_OPERAND_TYPE_MISMATCH,
            );
        } else if !self.is_textus(source_resolved) {
            self.invalid_conversion(ISSUE_REGEX_OPERAND_UNSUPPORTED, span);
            return Some(self.error_type);
        }

        // WHY: the `⊥` default slot is structurally empty for `↦ regex`.
        // Every coherent `↦ T ⊥ <value>` form requires a recovery of type T
        // (e.g. `"bad" ↦ numerus ⊥ 0`). `regex` has no literal form; its
        // only constructor is the failable conversion itself, so a recovery
        // would have to be another `pat ↦ regex` — recursion with no base.
        // The conversion itself is failable (an invalid pattern fails to
        // compile at run time): a bare `↦ regex` follows the ordinary text
        // failure channel, see `conversio_can_propagate_failure`.
        if recovery_present {
            self.invalid_conversion(ISSUE_REGEX_RECOVERY_UNSUPPORTED, span);
        }
        None
    }

    /// Checks `∷` / `verte` typed conversion and construction forms.
    ///
    /// The resolved target type decides the validation mode. Inference targets
    /// unify with the source because there is no concrete shape to validate yet.
    /// Constructor sources receive the effective nominal target as expected
    /// context so generic union/class construction substitutes applied type
    /// args into field payloads. The enclosing `expected` supplies witnesses
    /// when the declared target is a bare generic name.
    pub fn check_verte(
        &mut self,
        source: &mut HirExpression,
        target: TypeId,
        entries: Option<&mut Vec<HirObjectField>>,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> TypeId {
        let target_resolved = self.resolve_inferred_type(target);
        // Only nominal struct/enum targets drive constructor substitution.
        // Interface-boxing ascriptions (`Civis { … } ∷ Nominatum`) stay
        // trusting: the concrete construction is checked against its own
        // fields and the interface target is adopted by the cast.
        let target_is_nominal = self.is_nominal_construction_target(target_resolved);
        let constructor_source = target_is_nominal
            && (entries.is_some()
                || self.source_is_variant_constructor(source)
                || matches!(
                    &source.kind,
                    HirExpressionKind::Verte {
                        entries: Some(_),
                        ..
                    }
                ));
        let effective_target =
            self.effective_constructor_target(target_resolved, expected, constructor_source);
        // WHY: a quoted-key JSON literal is `json` data, not a genus value, so
        // `{ "k": v } ∷ Genus` names the entries-need-a-map error exactly as
        // `_ { "k": v }` does. The empty literal is the genus construction and
        // reaches here as `entries`, never as a JSON source.
        let json_entries_for_genus = entries.is_none()
            && matches!(
                &source.kind,
                HirExpressionKind::Literal(HirLiteral::JsonValor(radix_syntax::JsonValue::Object(
                    _
                )))
            )
            && self.is_genus_ascription_target(target_resolved);

        if let Some(result) = self.check_verte_object_entries(effective_target, entries, span) {
            return result;
        }
        if json_entries_for_genus {
            self.invalid_conversion(ISSUE_INFERRED_CONSTRUCTOR_ENTRIES_NEED_MAP, span);
            // The named error stands alone: the node takes the target's type.
            return effective_target;
        }

        // WHY: an empty list has no element evidence to check in the array
        // target loop below. Its authored list ascription supplies that fact;
        // an erased target such as `valor` does not.
        let empty_list_source =
            matches!(&source.kind, HirExpressionKind::Array(elements) if elements.is_empty());
        let list_target = matches!(
            self.types.get(effective_target),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        );
        // WHY: a list literal ascribed to a list whose slot is optional
        // (`[null, 1] ∷ list<int ∪ none>`) must be typed against that slot
        // exactly as a declaration cell types it; without the expected type
        // the bare literal joins `none` with `int` and fails SEM010.
        let optional_slot_list_source = list_target
            && matches!(&source.kind, HirExpressionKind::Array(_))
            && self.list_slot_carries_optional(effective_target);
        // WHY: a JSON-object literal ascribed to a map, or listed inside an
        // ascribed list whose slot is a map, is typed against that map exactly
        // as a declaration cell types it, so `json_nested_value_requires_valor`
        // and per-entry value checks apply to both spellings.
        let json_literal_source = matches!(
            &source.kind,
            HirExpressionKind::Literal(HirLiteral::JsonValor(_))
        ) && matches!(
            self.types.get(effective_target),
            Type::Map(..) | Type::Ledger(..)
        );
        let map_slot_list_source = list_target
            && matches!(&source.kind, HirExpressionKind::Array(_))
            && self.list_slot_carries_map(effective_target);
        // WHY: a `Verte` whose source is a JSON object literal names its data
        // type even when no typed cell is present. A later unit makes an
        // absent expected type on that source a named error, so the check must
        // not pass `None`; a non-map expected type makes
        // `check_json_valor_literal` return `json` exactly as `None` does.
        let json_object_source = matches!(
            &source.kind,
            HirExpressionKind::Literal(HirLiteral::JsonValor(radix_syntax::JsonValue::Object(_)))
        );
        let source_expected = (constructor_source
            || (empty_list_source && list_target)
            || optional_slot_list_source
            || json_literal_source
            || map_slot_list_source)
            .then_some(effective_target)
            .or_else(|| json_object_source.then(|| self.json_type()));
        let source_ty = if matches!(
            self.types.get(effective_target),
            Type::ModularWord(_)
                | Type::SaturatingWord(_)
                | Type::PolicyFloat(_, _)
                | Type::RangedInt { .. }
        ) {
            // WHY: modular ascription shapes integer literals so the ordinary
            // literal checker enforces the u32 range. Non-literal numerus
            // values remain a different family and require explicit `↦`.
            self.check_expr_with_expected(source, Some(effective_target))
        } else {
            self.check_expr_with_expected(source, source_expected)
        };
        let source_resolved = self.resolve_inferred_type(source_ty);

        if matches!(
            self.types.get(source_resolved),
            Type::Primitive(Primitive::Valor)
        ) {
            self.invalid_conversion(ISSUE_VALOR_VERTE_UNSUPPORTED, span);
            return self.error_type;
        }

        // WHY: an infer target carries no conversion shape, so the source use
        // site is the only evidence available to constrain it.
        if self.is_infer(effective_target) {
            return self.unify_join(source_ty, target, source.span, ISSUE_INVALID_CAST);
        }

        match self.types.get(effective_target).clone() {
            Type::Array(elem_ty) => {
                self.check_verte_array_target(source, elem_ty);
            }
            Type::Vector(elem_ty, width) => {
                self.check_verte_vector_target(source, elem_ty, width, span);
            }
            Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(_, _)
            | Type::SizedNumeric(..)
            | Type::Primitive(Primitive::Numerus | Primitive::Fractus) => {
                self.check_verte_numeric_fact(source, source_resolved, effective_target, span);
            }
            Type::Map(..) => {
                self.check_array_source_cross_kind_ascription(
                    source_resolved,
                    ISSUE_ARRAY_ASCRIPTION_TO_MAP,
                    span,
                );
            }
            Type::Tensor(..) => {
                self.check_array_source_cross_kind_ascription(
                    source_resolved,
                    ISSUE_ARRAY_ASCRIPTION_TO_TENSOR,
                    span,
                );
            }
            // Non-collection targets already typechecked the source above
            // (with the effective target as constructor context when the
            // source is a constructor); re-checking here would duplicate
            // diagnostics for ill-typed sources.
            _ => {}
        }
        // Nominal targets carry the D1.11 proof law: two genus types with no
        // declared relation are not an ascription, they are a conversion.
        // Constructor sources are construction, already checked against the
        // target above.
        if !constructor_source {
            self.check_verte_nominal_fact(source_resolved, effective_target, span);
        }
        effective_target
    }

    /// Reject an array source ascribed to a cross-container-kind target —
    /// a map or a tensor, whatever the target's key or shape is.
    ///
    /// An array is neither a map nor a tensor: `∷` asserts a type the
    /// expression already has, and crossing container kinds is a conversion,
    /// never an ascription (operator rulings 2026-09-21). Index-keying does
    /// not make an array a map, and shape agreement does not make it a
    /// tensor: matching rank and lengths is a coincidence, while structural
    /// compatibility is an assumption the source does not establish — like
    /// casting a signed integer to unsigned, the result is probably not what
    /// you expected. The sanctioned path is the conversion operator `↦`.
    /// Only a source whose family is definite fires the rule: an identifier
    /// or unresolved-type expression is what ascription is for
    /// (`raw ∷ lista<textus>` stays legal).
    fn check_array_source_cross_kind_ascription(
        &mut self,
        source: TypeId,
        issue: &'static str,
        span: radix_lexer::Span,
    ) {
        if matches!(
            self.types.get(source),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        ) {
            self.invalid_conversion(issue, span);
        }
    }

    /// Enforce F3 on scalar numeric `∷` targets (DECISIONS D1.11, fork F3):
    /// the ascription must state a provable fact — an integer literal the
    /// target width holds, a float literal that rounds to a finite value at
    /// the target width, or lossless widening inside one arithmetic family.
    /// Narrowing, float precision loss, and int↔float are conversions and take
    /// `↦`. Sources outside these families (valor, unresolved shapes, error) keep
    /// their own law.
    fn check_verte_numeric_fact(
        &mut self,
        source: &HirExpression,
        source_resolved: TypeId,
        target: TypeId,
        span: radix_lexer::Span,
    ) {
        let source_ty = self.resolve_inferred_type(source_resolved);
        let target_ty = self.resolve_inferred_type(target);
        if matches!(self.types.get(source_ty), Type::Error | Type::Infer(_)) {
            return;
        }
        let (Some(source_fact), Some(target_fact)) = (
            numeric_fact(self.types.get(source_ty)),
            numeric_fact(self.types.get(target_ty)),
        ) else {
            return;
        };

        // A ranged target is stated by ascription only when the range is a
        // proven fact: a literal inside it, or a ranged cell it contains.
        // Every other value lands in it through a store or an authored `↦`.
        if let Some((_, _, lo, hi)) = self.types.ranged_int_parts(target_ty) {
            let holds = match static_integer_magnitude(source) {
                Some(value) => (lo..=hi).contains(&value),
                None => self.types.is_assignable(source_ty, target_ty),
            };
            if !holds {
                self.invalid_conversion(ISSUE_NUMERIC_ASCRIPTION_REQUIRES_CONVERSION, span);
            }
            return;
        }

        // A folded integer literal is a fact when the target width holds the
        // value; int↔float and int→decimal stay conversions (decimal law:
        // integer literals keep their own family).
        if let Some(value) = static_integer_magnitude(source) {
            // `inf` holds every integer constant (ruling 34).
            let holds = target_fact.family == NumericFactFamily::Unbounded
                || target_fact
                    .width
                    .and_then(integer_width_range)
                    .is_some_and(|(min, max)| value >= min && value <= max);
            if !holds {
                self.invalid_conversion(ISSUE_NUMERIC_ASCRIPTION_REQUIRES_CONVERSION, span);
            }
            return;
        }
        if let Some(value) = static_float_magnitude(source) {
            let holds = match target_fact.family {
                NumericFactFamily::Float { .. } => target_fact
                    .width
                    .is_some_and(|width| float_width_holds(width, value)),
                _ => false,
            };
            if !holds {
                self.invalid_conversion(ISSUE_NUMERIC_ASCRIPTION_REQUIRES_CONVERSION, span);
            }
            return;
        }

        // N7e D4: a float cell is its own family. A non-literal value never
        // enters or leaves one by ascription; it needs an authored `↦`.
        let is_cell = |ty: TypeId| matches!(self.types.get(ty), Type::PolicyFloat(_, _));
        if (is_cell(source_ty) || is_cell(target_ty)) && !self.types.equals(source_ty, target_ty) {
            self.invalid_conversion(ISSUE_NUMERIC_ASCRIPTION_REQUIRES_CONVERSION, span);
            return;
        }

        // Non-literal sources: lossless widening inside one family only.
        if !source_fact.family.admits(target_fact.family) {
            self.invalid_conversion(ISSUE_NUMERIC_ASCRIPTION_REQUIRES_CONVERSION, span);
        }
    }

    /// Enforce DECISIONS D1.11 for `∷` between nominal types: the ascription
    /// must state a fact the compiler can prove. The proved facts are the same
    /// nominal type — a `type` alias resolves to its target before this runs —
    /// a variant ascribed to its own parent enum (the pattern/test that
    /// narrowed it already proved the membership), and a genus ascribed to an
    /// interface its declaration names in `implet` (D1.3). Class inheritance
    /// was removed (D8.1), so every other nominal pair (class→class,
    /// enum→enum, interface→class) is unprovable and takes the explicit `↦`
    /// conversion.
    ///
    /// Constructor sources are construction, not ascription: their fields are
    /// checked against the target directly, so they are not this law's
    /// business.
    fn check_verte_nominal_fact(
        &mut self,
        source: TypeId,
        target: TypeId,
        span: radix_lexer::Span,
    ) {
        let source_ty = self.resolve_inferred_type(source);
        let target_ty = self.resolve_inferred_type(target);
        if self.types.equals(source_ty, target_ty) {
            return;
        }
        if self.nominal_def(source_ty).is_none() || self.nominal_def(target_ty).is_none() {
            return;
        }
        if self.variant_belongs_to_enum(source_ty, target_ty) {
            return;
        }
        if self.nominal_declares_interface(source_ty, target_ty) {
            return;
        }
        // A generic constructor-like call can reach this law with its witness
        // still unsolved (`mk(1)` is `Box<?>` until something names `?`). The
        // ascription is the statement that names it, so unifying the applied
        // arguments proves the pair and refines the result. A trial failure is
        // rolled back and the ordinary rejection below still fires.
        if self.verte_inferred_source_refines(source_ty, target_ty, span) {
            return;
        }
        let found = self.format_union_constituent_type(source_ty);
        let expected = self.format_union_constituent_type(target_ty);
        self.error_issue_with_args(
            SemanticErrorKind::TypeMismatch,
            ISSUE_NOMINAL_ASCRIPTION_NOT_PROVABLE,
            span,
            &[("found", found), ("expected", expected)],
        );
    }

    /// Whether an ascription source still carrying an inference hole is
    /// refined into `target` by unifying the applied arguments.
    ///
    /// The source of a generic constructor-like call (`mk(1)` typed `Box<?>`)
    /// reaches the nominal law carrying the call-site infer id inside the
    /// applied arguments, even when the witness is already solved; `Box<?>` and
    /// `Box<numerus>` are then table-distinct although the ascription is the
    /// plain refinement that names the witness. The trial runs through the
    /// ordinary assignment unification so the difference is bridged and genuine
    /// mismatches (different genus bases, unrelated witnesses) still fail: on
    /// failure both the diagnostics and the substitutions are rolled back and
    /// the caller rejects as before.
    fn verte_inferred_source_refines(
        &mut self,
        source: TypeId,
        target: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        if !self.verte_type_has_inference_hole(source) {
            return false;
        }
        let errors_before = self.errors.len();
        let substitutions_before = self.substitutions.borrow().clone();
        self.unify_assign_issue(source, target, span, ISSUE_NOMINAL_ASCRIPTION_NOT_PROVABLE);
        if self.errors.len() == errors_before {
            return true;
        }
        self.errors.truncate(errors_before);
        *self.substitutions.borrow_mut() = substitutions_before;
        self.occurs_cache.clear();
        false
    }

    /// Whether an applied source type still holds an inference id in an
    /// argument position.
    ///
    /// The id is inspected raw, without substitution resolution: a generic
    /// call's result keeps the call-site infer id even after the witness has
    /// been solved, so `Box<?>` is table-distinct from `Box<numerus>` although
    /// `?` already names `numerus`. That stale id is the only difference
    /// unification must bridge, and a source with no infer id at all is a
    /// plain nominal pair the law judges as before. Container and applied
    /// argument shapes are walked so a nested stale id is caught too.
    pub(super) fn verte_type_has_inference_hole(&self, ty: TypeId) -> bool {
        let mut seen = FxHashSet::default();
        self.verte_type_has_inference_hole_inner(ty, &mut seen)
    }

    fn verte_type_has_inference_hole_inner(
        &self,
        ty: TypeId,
        seen: &mut FxHashSet<TypeId>,
    ) -> bool {
        if !seen.insert(ty) {
            return false;
        }
        match self.types.get(ty) {
            Type::Infer(_) => true,
            Type::Array(inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::RangedArray { element: inner, .. }
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Matrix(inner, _)
            | Type::Vector(inner, _)
            | Type::Atomic(inner)
            | Type::Intervallum(inner, _)
            | Type::Option(inner) => self.verte_type_has_inference_hole_inner(*inner, seen),
            Type::Map(key, value) | Type::Ledger(key, value) => {
                self.verte_type_has_inference_hole_inner(*key, seen)
                    || self.verte_type_has_inference_hole_inner(*value, seen)
            }
            Type::Ref(_, inner) | Type::Alias(_, inner) => {
                self.verte_type_has_inference_hole_inner(*inner, seen)
            }
            Type::Union(types) => types
                .iter()
                .any(|member| self.verte_type_has_inference_hole_inner(*member, seen)),
            Type::Tuple(types) => types
                .iter()
                .any(|member| self.verte_type_has_inference_hole_inner(*member, seen)),
            Type::Applied(base, args) => {
                self.verte_type_has_inference_hole_inner(*base, seen)
                    || args.iter().any(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => {
                            self.verte_type_has_inference_hole_inner(*id, seen)
                        }
                        radix_types::AppliedArg::Size(_) => false,
                    })
            }
            _ => false,
        }
    }

    /// Whether `source` is one of `target`'s closed-set variants. A binding
    /// narrowed by `discerne`/`est` carries the variant's own nominal
    /// (`Type::Struct(variant_def)`) while the parent is the `discretio`
    /// (`Type::Enum(enum_def)`); the shared `variant_parent` table records
    /// the membership, so it is the proof and no second table is needed.
    fn variant_belongs_to_enum(&self, source: TypeId, target: TypeId) -> bool {
        let Some(variant_def) = self.nominal_def(source) else {
            return false;
        };
        let Some(enum_def) = self.enum_def_from_type(target) else {
            return false;
        };
        self.variant_parent.get(&variant_def).copied() == Some(enum_def)
    }

    /// The struct/enum/interface declaration a resolved nominal type names,
    /// after peeling aliases, access markers and applied arguments.
    fn nominal_def(&self, ty: TypeId) -> Option<DefId> {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Struct(def) | Type::Enum(def) | Type::Interface(def) => Some(*def),
            Type::Ref(_, inner) => self.nominal_def(*inner),
            Type::Applied(base, _) => self.nominal_def(*base),
            _ => None,
        }
    }

    /// Whether the source genus declares the target interface in its `implet`
    /// clause. This is the one nominal ascription that states a fact beyond
    /// type identity: the declaration already commits the genus to the
    /// contract (D1.3), so casting a value of the genus to the contract adds
    /// no unproven claim.
    fn nominal_declares_interface(&self, source: TypeId, target: TypeId) -> bool {
        let target_resolved = self.resolve_inferred_type(target);
        // `None` in a slot is a size argument on the contract application:
        // the contract identity is the proof, and the size slot is not this
        // law's evidence.
        let (target_def, target_args): (DefId, Vec<Option<TypeId>>) =
            match self.types.get(target_resolved) {
                Type::Interface(def) => (*def, Vec::new()),
                Type::Applied(base, args) => {
                    let Type::Interface(def) = self.types.get(self.resolve_inferred_type(*base))
                    else {
                        return false;
                    };
                    let args = args
                        .iter()
                        .map(|arg| match arg {
                            radix_types::AppliedArg::Type(ty) => Some(*ty),
                            radix_types::AppliedArg::Size(_) => None,
                        })
                        .collect();
                    (*def, args)
                }
                _ => return false,
            };
        let Some(source_def) = self.struct_def_from_type(source) else {
            return false;
        };
        let Some(declared) = self.implemented_contracts.get(&source_def) else {
            return false;
        };
        declared.iter().any(|contract| {
            contract.contract == target_def
                && contract.args.len() == target_args.len()
                && contract
                    .args
                    .iter()
                    .zip(&target_args)
                    .all(|(declared_arg, target_arg)| {
                        target_arg.is_none_or(|target_arg| {
                            self.declared_contract_arg_matches(
                                source,
                                source_def,
                                *declared_arg,
                                target_arg,
                            )
                        })
                    })
        })
    }

    /// Whether one declared `implet` argument matches the target's. A declared
    /// argument that is a source type parameter reads the source's applied
    /// witness at that position; when the source is unapplied the declaration
    /// itself is the proof, so the argument is accepted rather than rejected.
    fn declared_contract_arg_matches(
        &self,
        source: TypeId,
        source_def: DefId,
        declared: TypeId,
        target: TypeId,
    ) -> bool {
        let declared = self.resolve_inferred_type(declared);
        let target = self.resolve_inferred_type(target);
        let Type::Param(sym) = self.types.get(declared) else {
            return self.types.equals(declared, target);
        };
        let Some(position) = self
            .type_params_by_def
            .get(&source_def)
            .and_then(|params| params.iter().position(|param| *param == *sym))
        else {
            return true;
        };
        let Type::Applied(_, args) = self.types.get(self.resolve_inferred_type(source)) else {
            return true;
        };
        let Some(radix_types::AppliedArg::Type(witness)) = args.get(position) else {
            return true;
        };
        self.types
            .equals(self.resolve_inferred_type(*witness), target)
    }

    /// Whether a `∷` source is a variant-constructor call (finge-lowered
    /// `Call` whose callee path resolves to a variant).
    fn source_is_variant_constructor(&self, source: &HirExpression) -> bool {
        let HirExpressionKind::Call(callee, _, _, _) = &source.kind else {
            return false;
        };
        let HirExpressionKind::Path(def_id) = &callee.kind else {
            return false;
        };
        self.variant_parent.contains_key(def_id)
    }

    /// Whether a resolved type is a genus (bare or applied struct), the only
    /// ascription target that a keyed literal constructs.
    fn is_genus_ascription_target(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved) {
            Type::Struct(_) => true,
            Type::Applied(base, _) => matches!(
                self.types.get(self.resolve_inferred_type(*base)),
                Type::Struct(_)
            ),
            _ => false,
        }
    }

    /// Whether a resolved type is a struct/enum construction target (bare or
    /// applied nominal). Interface/collection/other targets are not.
    fn is_nominal_construction_target(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved) {
            Type::Struct(_) | Type::Enum(_) => true,
            Type::Applied(base, _) => matches!(
                self.types.get(self.resolve_inferred_type(*base)),
                Type::Struct(_) | Type::Enum(_)
            ),
            _ => false,
        }
    }

    /// Compute the effective nominal target for a constructor `∷` form.
    ///
    /// An already-applied target is used as-is. A bare `Enum`/`Struct` target
    /// whose def declares type params adopts the enclosing expected type's
    /// witnesses (or fresh inference variables) so payload/field checking can
    /// substitute them. Non-constructor conversions keep the trusting
    /// bare-target behavior.
    fn effective_constructor_target(
        &mut self,
        target_resolved: TypeId,
        expected: Option<TypeId>,
        constructor_source: bool,
    ) -> TypeId {
        if !constructor_source || matches!(self.types.get(target_resolved), Type::Applied(..)) {
            return target_resolved;
        }
        let (def_id, base) = match self.types.get(target_resolved).clone() {
            Type::Enum(def_id) => (def_id, Type::Enum(def_id)),
            Type::Struct(def_id) => (def_id, Type::Struct(def_id)),
            _ => return target_resolved,
        };
        if !self.nominal_is_generic(def_id) {
            return target_resolved;
        }
        // Contextual instantiation (S0-G1): a bare generic head under an
        // enclosing applied expected type of the same nominal adopts that
        // type's arguments — magnitudo witnesses included — instead of fresh
        // inference variables, so `Gen { … }` under `→ Gen<3>` checks its
        // fields against `N ↦ 3`.
        if let Some(expected) = expected {
            let resolved_expected = self.resolve_inferred_type(expected);
            if let Type::Applied(expected_base, _) = self.types.get(resolved_expected)
                && let Type::Struct(expected_def) =
                    self.types.get(self.resolve_inferred_type(*expected_base))
                && *expected_def == def_id
            {
                return resolved_expected;
            }
        }
        let witnesses =
            self.constructor_witnesses(def_id, &[], expected, radix_lexer::Span::default());
        let base_ty = self.types.intern(base);
        let witnesses = witnesses
            .into_iter()
            .map(radix_types::AppliedArg::Type)
            .collect();
        self.types.intern(Type::Applied(base_ty, witnesses))
    }

    /// True when `ty` is a list whose element slot is optional, directly or
    /// through nested lists (`list<int ∪ none>`, `list<list<int ∪ none>>`).
    fn list_slot_carries_optional(&self, ty: TypeId) -> bool {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Array(elem)
            | Type::BoundedArray { element: elem, .. }
            | Type::RangedArray { element: elem, .. } => self.slot_carries_optional(*elem),
            _ => false,
        }
    }

    /// True when `ty` is a list whose element slot is a map, directly or
    /// through nested lists (`list<map<string, int>>`).
    fn list_slot_carries_map(&self, ty: TypeId) -> bool {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Array(elem)
            | Type::BoundedArray { element: elem, .. }
            | Type::RangedArray { element: elem, .. } => self.slot_carries_map(*elem),
            _ => false,
        }
    }

    fn slot_carries_map(&self, slot: TypeId) -> bool {
        let slot = self.resolve_inferred_type(slot);
        matches!(self.types.get(slot), Type::Map(..) | Type::Ledger(..))
            || self.list_slot_carries_map(slot)
    }

    fn slot_carries_optional(&self, slot: TypeId) -> bool {
        let slot = self.resolve_inferred_type(slot);
        matches!(self.types.get(slot), Type::Option(_)) || self.list_slot_carries_optional(slot)
    }

    fn check_verte_array_target(&mut self, source: &mut HirExpression, elem_ty: TypeId) {
        let HirExpressionKind::Array(elements) = &mut source.kind else {
            self.check_expr(source);
            return;
        };

        let optional_slot = self.slot_carries_optional(elem_ty) || self.slot_carries_map(elem_ty);
        for element in elements {
            match element {
                HirArrayElement::Expr(expr) => {
                    // WHY: an optional (or nested optional) or map slot types
                    // its element literals against the slot, as a declaration
                    // does.
                    let element_ty = if optional_slot {
                        self.check_expr_with_expected(expr, Some(elem_ty))
                    } else {
                        self.check_expr(expr)
                    };
                    self.unify_join(
                        element_ty,
                        elem_ty,
                        expr.span,
                        ISSUE_ARRAY_ELEMENT_TYPE_MISMATCH,
                    );
                }
                HirArrayElement::Spread(expr) => {
                    let spread_ty = self.check_expr(expr);
                    match self.types.get(self.resolve_inferred_type(spread_ty)) {
                        Type::Array(inner)
                        | Type::BoundedArray { element: inner, .. }
                        | Type::RangedArray { element: inner, .. } => {
                            self.unify_join(
                                *inner,
                                elem_ty,
                                expr.span,
                                ISSUE_ARRAY_SPREAD_ELEMENT_TYPE_MISMATCH,
                            );
                        }
                        _ => {
                            self.invalid_operand_types(
                                ISSUE_ARRAY_SPREAD_REQUIRES_LISTA,
                                expr.span,
                            );
                        }
                    }
                }
                HirArrayElement::Infer(_) => {
                    self.invalid_operand_types(
                        super::aggregate::ISSUE_SHAPE_HOLE_REQUIRES_SHAPE_ARGUMENT,
                        source.span,
                    );
                }
            }
        }
    }

    fn check_verte_vector_target(
        &mut self,
        source: &mut HirExpression,
        elem_ty: TypeId,
        width: crate::IndexId,
        span: radix_lexer::Span,
    ) {
        let HirExpressionKind::Array(elements) = &mut source.kind else {
            self.check_expr(source);
            return;
        };

        if let Some(width) = crate::literal_vector_width(width, self.types)
            && elements.len() != usize::from(width)
        {
            self.wrong_arity("vector_constructor_width_arity", span);
        }

        for element in elements {
            match element {
                HirArrayElement::Expr(expr) => {
                    let element_ty = self.check_expr_with_expected(expr, Some(elem_ty));
                    self.unify_join(
                        element_ty,
                        elem_ty,
                        expr.span,
                        ISSUE_VECTOR_ELEMENT_TYPE_MISMATCH,
                    );
                }
                HirArrayElement::Spread(expr) => {
                    self.check_expr(expr);
                    self.invalid_operand_types(ISSUE_VECTOR_CONSTRUCTOR_FORBIDS_SPREAD, expr.span);
                }
                HirArrayElement::Infer(_) => {
                    self.invalid_operand_types(
                        super::aggregate::ISSUE_SHAPE_HOLE_REQUIRES_SHAPE_ARGUMENT,
                        source.span,
                    );
                }
            }
        }
    }

    /// Validate tabula `verte` entries. Returns a fully inferred map type when
    /// the declared value slot is still `infer` and entry values supplied evidence.
    fn check_verte_map_target(
        &mut self,
        entries: &mut [HirObjectField],
        key_ty: TypeId,
        value_ty: TypeId,
        span: radix_lexer::Span,
        target_resolved: TypeId,
    ) -> Option<TypeId> {
        let mut inferred_values = Vec::new();
        for field in entries {
            match &mut field.key {
                HirObjectKey::Ident(_) | HirObjectKey::String(_) => {
                    let textus = self.textus_type();
                    self.unify_join(textus, key_ty, span, ISSUE_MAP_KEY_TYPE_MISMATCH);
                }
                HirObjectKey::Computed(key) => {
                    // Row 12: a key lands in the declared key type as a store.
                    let actual_key_ty = self.check_store_by_type(
                        key,
                        key_ty,
                        StoreSite::new(HirStoreKind::Element),
                    );
                    self.unify_join(actual_key_ty, key_ty, key.span, ISSUE_MAP_KEY_TYPE_MISMATCH);
                }
                HirObjectKey::Spread(expr) => {
                    let spread_ty = self.check_expr(expr);
                    match self
                        .types
                        .get(self.resolve_inferred_type(spread_ty))
                        .clone()
                    {
                        Type::Map(spread_key_ty, spread_value_ty)
                        | Type::Ledger(spread_key_ty, spread_value_ty) => {
                            self.unify_join(
                                spread_key_ty,
                                key_ty,
                                expr.span,
                                ISSUE_MAP_SPREAD_KEY_TYPE_MISMATCH,
                            );
                            inferred_values.push(self.resolve_inferred_type(spread_value_ty));
                            if !self.is_infer(self.resolve_inferred_type(value_ty)) {
                                self.unify_join(
                                    spread_value_ty,
                                    value_ty,
                                    expr.span,
                                    ISSUE_MAP_SPREAD_VALUE_TYPE_MISMATCH,
                                );
                            }
                        }
                        _ => {
                            self.invalid_operand_types(
                                ISSUE_OBJECT_SPREAD_REQUIRES_TABULA,
                                expr.span,
                            );
                        }
                    }
                    continue;
                }
            }

            let Some(value) = &mut field.value else {
                self.invalid_operand_types(ISSUE_OBJECT_FIELD_REQUIRES_VALUE, span);
                continue;
            };
            let value_ty_actual =
                self.check_store_by_type(value, value_ty, StoreSite::new(HirStoreKind::Element));
            inferred_values.push(self.resolve_inferred_type(value_ty_actual));
            let value_resolved = self.resolve_inferred_type(value_ty);
            // WHY: infer/ignotum/union value targets are already broad enough to
            // accept the entry while preserving the explicit map shape.
            if !(self.is_infer(value_resolved)
                || matches!(
                    self.types.get(value_resolved),
                    Type::Primitive(Primitive::Ignotum)
                )
                || matches!(self.types.get(value_resolved), Type::Union(_)))
            {
                self.unify_join(
                    value_ty_actual,
                    value_ty,
                    value.span,
                    ISSUE_MAP_VALUE_TYPE_MISMATCH,
                );
            }
        }

        if !self.is_infer(self.resolve_inferred_type(value_ty)) {
            return None;
        }

        let inferred_value_ty = match inferred_values.as_slice() {
            [] => value_ty,
            [single] => *single,
            _ => self.types.intern(Type::Union(inferred_values)),
        };
        Some(
            if matches!(
                self.types.get(self.resolve_inferred_type(target_resolved)),
                Type::Ledger(..)
            ) {
                self.types.ledger(key_ty, inferred_value_ty)
            } else {
                self.types.map(key_ty, inferred_value_ty)
            },
        )
    }
}

fn sparse_tensor_pair(
    source: &Type,
    target: &Type,
) -> Option<(TypeId, crate::IndexId, TypeId, crate::IndexId)> {
    match (source, target) {
        (Type::Tensor(from_elem, from_shape), Type::Sparsa(to_elem, to_shape))
        | (Type::Sparsa(from_elem, from_shape), Type::Tensor(to_elem, to_shape)) => {
            Some((*from_elem, *from_shape, *to_elem, *to_shape))
        }
        _ => None,
    }
}

struct CollectionConversioShape {
    family: CollectionConversioFamily,
    elem: Option<TypeId>,
    key: Option<TypeId>,
    value: Option<TypeId>,
}
