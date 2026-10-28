//! Shared MIR type-resolution helpers used by all MIR text probes.
//!
//! These helpers walk the semantic `TypeTable` to resolve target-neutral
//! `MirType` queries: option payload unwrapping, nullable chain bases, constant
//! type spelling, and place-base type lookup. Each probe maps `MirTypeError`
//! into its own error type (typically via a `From` impl so `?` auto-converts).
//!
//! Projection-aware type resolution stays per-target because projection
//! lowering is target-specific (LLVM declaration gathering vs Wasm import
//! gathering produce side effects beyond the result type).

use crate::{
    MirConstant, MirFunctionId, MirFunctionIndex, MirLocalId, MirPlace, MirPlaceBase, MirProgram,
    MirTempId, MirType,
};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};

const MIR_TYPE_RESOLUTION_PREFIX: &str = "MIR type resolution unsupported";

/// Spanless error from shared MIR type resolution.
///
/// Probes map this into their own error types at the call boundary. This is
/// type *resolution* failure (the MIR shape is valid but the probe cannot
/// determine a type), not validation failure (the MIR is malformed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirTypeError {
    pub message: String,
    pub issue: &'static str,
}

impl MirTypeError {
    pub fn unsupported(shape: impl Into<String>) -> Self {
        Self {
            message: format!("{MIR_TYPE_RESOLUTION_PREFIX}: {}", shape.into()),
            issue: "mir_type_unsupported",
        }
    }

    #[must_use]
    pub fn unknown_function(id: MirFunctionId) -> Self {
        Self {
            message: format!("{MIR_TYPE_RESOLUTION_PREFIX}: unknown function f{}", id.0),
            issue: "mir_type_unknown_function",
        }
    }

    #[must_use]
    pub fn undefined_local(id: MirLocalId) -> Self {
        Self {
            message: format!(
                "{MIR_TYPE_RESOLUTION_PREFIX}: undefined local type l{}",
                id.0
            ),
            issue: "mir_type_undefined_local",
        }
    }

    #[must_use]
    pub fn undefined_temp(id: MirTempId) -> Self {
        Self {
            message: format!(
                "{MIR_TYPE_RESOLUTION_PREFIX}: undefined temp type t{}",
                id.0
            ),
            issue: "mir_type_undefined_temp",
        }
    }
}

impl std::fmt::Display for MirTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for MirTypeError {}

/// Shared `MirType` lookup for place-base resolution, re-exported here for
/// the public `radix_mir::ty` surface (the trait itself lives in
/// [`crate::local_ty`], where the shared params-then-locals lookup also lives).
pub use crate::local_ty::MirTypeLookup;

/// Unwrap one layer of `Option<T>` / nullable `T ∪ nihil` / alias chain to the
/// payload type. Non-option primitives pass through unchanged so callers can
/// treat `nihil` and bare payloads uniformly.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`MirTypeError`].
pub fn option_payload_ty(ty: MirType, types: &TypeTable) -> Result<MirType, MirTypeError> {
    match types.get(ty.semantic_id()) {
        Type::Option(inner) => Ok(MirType::semantic(*inner)),
        Type::Union(members) => {
            let nihil = types.primitive(Primitive::Nihil);
            members
                .iter()
                .find(|member| **member != nihil)
                .map(|member| MirType::semantic(*member))
                .ok_or_else(|| MirTypeError::unsupported("nullable union payload type"))
        }
        Type::Alias(_, target) => option_payload_ty(MirType::semantic(*target), types),
        Type::Primitive(_) => Ok(ty),
        other => Err(MirTypeError::unsupported(format!("option type {other:?}"))),
    }
}

/// Resolve the inner type of an option-chain projection base.
///
/// Infallible: non-option types pass through unchanged. This is used by
/// projection chains that may start from a nullable base (`foo?.bar`) where the
/// first projection operates on the payload type.
#[must_use]
pub fn option_chain_base_ty(ty: MirType, types: &TypeTable) -> MirType {
    match types.get(ty.semantic_id()) {
        Type::Option(inner) => MirType::semantic(*inner),
        Type::Alias(_, target) => option_chain_base_ty(MirType::semantic(*target), types),
        _ => ty,
    }
}

/// Resolve the `MirType` of a constant.
///
/// Function constants resolve to the function's return type via the program's
/// function table. All other constants map to their primitive type.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`MirTypeError`].
pub fn constant_ty(
    constant: &MirConstant,
    types: &TypeTable,
    program: &MirProgram,
) -> Result<MirType, MirTypeError> {
    let function_index = program.function_index();
    constant_ty_with_index(constant, types, program, &function_index)
}

/// Resolve a constant using a caller-owned function-id index.
///
/// This is the repeated-lookup form used by immutable backend probes. The
/// compatibility [`constant_ty`] wrapper remains for one-shot raw callers.
///
/// # Errors
///
/// Returns [`MirTypeError`] when a `MirConstant::Function` id is absent from
/// `function_index`.
pub fn constant_ty_with_index(
    constant: &MirConstant,
    types: &TypeTable,
    program: &MirProgram,
    function_index: &MirFunctionIndex,
) -> Result<MirType, MirTypeError> {
    let primitive = match constant {
        MirConstant::Int(_) => Primitive::Numerus,
        MirConstant::UInt(_) => return Ok(sized_numerus_or_fallback(types, NumericWidth::U64)),
        MirConstant::Bool(_) => Primitive::Bivalens,
        MirConstant::Unit => Primitive::Vacuum,
        MirConstant::String(_) => Primitive::Textus,
        MirConstant::Ascii(_) => Primitive::Ascii,
        MirConstant::Nil => Primitive::Nihil,
        MirConstant::Octeti(_) => Primitive::Octeti,
        MirConstant::Regex { .. } => Primitive::Regex,
        MirConstant::Function(id) => {
            return program.indexed_function(function_index, *id).map_or_else(
                || Err(MirTypeError::unknown_function(*id)),
                |function| Ok(function.return_ty),
            );
        }
        // A shape-size scalar is a float-sized compile-time value (the
        // symbolic companion divides by it at runtime).
        MirConstant::Float(_) | MirConstant::ShapeSize { .. } => Primitive::Fractus,
        // An exact bignum constant is the unbounded integer; the sized type
        // is interned whenever an `inf` is declared.
        MirConstant::Magnus(_) => return Ok(sized_numerus_or_fallback(types, NumericWidth::Inf)),
    };
    Ok(MirType::semantic(types.primitive(primitive)))
}

/// The interned `numerus<width>` row, or the unsized `numerus` row when that
/// width is not declared.
fn sized_numerus_or_fallback(types: &TypeTable, width: NumericWidth) -> MirType {
    types
        .find_sized_numeric(Primitive::Numerus, width)
        .map_or_else(
            || MirType::semantic(types.primitive(Primitive::Numerus)),
            MirType::semantic,
        )
}

/// Locate the `ascii<1>` row (the `'x'` scalar) without interning — a scan of
/// the append-only arena. The typecheck `itera`/index arms intern it for any
/// source that reaches a bounded byte index projection, so callers may treat
/// a miss as a type-resolution failure.
pub fn find_bounded_ascii_one(types: &TypeTable) -> Option<TypeId> {
    (0..u32::try_from(types.type_count()).ok()?)
        .map(TypeId)
        .find(|&id| match types.get(id) {
            Type::BoundedAscii { capacity } => {
                matches!(types.get_index(*capacity), IndexExpr::Literal(1))
            }
            _ => false,
        })
}

/// Result type for an index projection on a supported collection or text base.
///
/// `octeti` follows the same rule as `lista<numerus<u8>>`: index yields the u8
/// element type. See DEFER-029 / DEFER-030 parity.
/// # Errors
/// Returns `MirTypeError` when the base type admits no index projection.
pub fn index_projection_result_ty(
    base_ty: MirType,
    types: &TypeTable,
) -> Result<MirType, MirTypeError> {
    match types.get(base_ty.semantic_id()) {
        // A run-time lane select on a vector register value (the constant lane
        // is the `VectorLane` projection).
        Type::Array(inner)
        | Type::BoundedArray { element: inner, .. }
        | Type::RangedArray { element: inner, .. }
        | Type::Tensor(inner, _)
        | Type::Vector(inner, _) => Ok(MirType::semantic(*inner)),
        Type::Map(_, value) | Type::Ledger(_, value) => Ok(MirType::semantic(*value)),
        Type::Primitive(Primitive::Textus) => {
            Ok(MirType::semantic(types.primitive(Primitive::Textus)))
        }
        Type::Primitive(Primitive::Octeti) => types
            .find_sized_numeric(Primitive::Numerus, NumericWidth::U8)
            .map(MirType::semantic)
            .ok_or_else(|| MirTypeError::unsupported("octeti element type")),
        // AEU-7: the bounded scalar buffer element rows — `octeti<N>[i]` is
        // `numerus<u8>`, `ascii<N>[i]` is `littera` (D10.3) and
        // `textus<N>[i]` is `ascii<1>` (D14 / D17; F5 held), matching the
        // semantic index row exactly.
        Type::BoundedOcteti { .. } => types
            .find_sized_numeric(Primitive::Numerus, NumericWidth::U8)
            .map(MirType::semantic)
            .ok_or_else(|| MirTypeError::unsupported("bounded octeti element type")),
        Type::BoundedAscii { .. } => Ok(MirType::semantic(types.primitive(Primitive::Littera))),
        Type::BoundedTextus { .. } => find_bounded_ascii_one(types)
            .map(MirType::semantic)
            .ok_or_else(|| MirTypeError::unsupported("ascii<size 1> element type")),
        Type::Tuple(_) => Err(MirTypeError::unsupported(
            "tuple index projection (use tuple_index_projection for physical aggregate tuples)",
        )),
        Type::Alias(_, target) => index_projection_result_ty(MirType::semantic(*target), types),
        _ => Err(MirTypeError::unsupported("index projection base")),
    }
}

/// Resolve the base `MirType` of a place (before projections).
///
/// Projections are resolved per-target because they depend on target-specific
/// declaration/import catalogs.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`MirTypeError`].
pub fn place_base_ty(
    place: &MirPlace,
    lookup: &impl MirTypeLookup,
) -> Result<MirType, MirTypeError> {
    match place.base {
        MirPlaceBase::Local(id) => lookup
            .local_ty(id)
            .ok_or_else(|| MirTypeError::undefined_local(id)),
        MirPlaceBase::Temp(id) => lookup
            .temp_ty(id)
            .ok_or_else(|| MirTypeError::undefined_temp(id)),
    }
}
