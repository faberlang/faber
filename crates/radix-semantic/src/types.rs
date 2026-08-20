//! Re-exports the Radix type model for the semantic module's public API.
//!
//! This file is the semantic-facing re-export boundary. Type table, primitive,
//! collection, and tensor-family representations come from `radix-types`.
//! Schema metadata is a separate foundation module re-exported here so
//! semantic callers keep one path.

pub use radix_types::{
    collection_type_specs, is_atomic_element_type, is_intervallum_bound_type,
    lookup_collection_type_spec, lookup_primitive_type_spec, lookup_tensor_family_type_spec,
    primitive_type_specs, tensor_family_type_specs, AccessMarker, CollectionKind,
    CollectionTypeSpec, FuncSig, InferVar, MarkerFamily, ParamType, Primitive, PrimitiveTypeSpec,
    SemanticMutability, SemanticParamMode, TensorFamilyKind, TensorFamilyTypeSpec, Type, TypeId,
    TypeParamConstraint, TypeTable, TypeTableSnapshot, ATOMIC_ELEMENT_MSG, INTERVALLUM_BOUND_MSG,
};
pub use radix_types::{SchemaColumnDef, SchemaDef};

/// Stable lowering issue for a marker on a scalar carrier.
pub const ISSUE_SCALAR_ACCESS_MARKER: &str = "scalar_access_marker";

/// Map a type-position syntax marker onto the type-table access fact.
#[must_use]
pub fn access_marker_from_type_mode(mode: radix_syntax::TypeMode) -> AccessMarker {
    match mode {
        radix_syntax::TypeMode::De => AccessMarker::Ref,
        radix_syntax::TypeMode::In => AccessMarker::Mut,
        radix_syntax::TypeMode::Own => AccessMarker::Own,
        radix_syntax::TypeMode::Copy => AccessMarker::Copy,
    }
}
