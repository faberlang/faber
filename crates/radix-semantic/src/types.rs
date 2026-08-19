//! Re-exports the Radix type model for the semantic module's public API.
//!
//! This file is the semantic-facing re-export boundary. Type table, primitive,
//! collection, and tensor-family representations come from `radix-types`.
//! Schema metadata is a separate foundation module re-exported here so
//! semantic callers keep one path.

pub use radix_types::{
    collection_type_specs, is_atomic_element_type, is_intervallum_bound_type,
    lookup_collection_type_spec, lookup_primitive_type_spec, lookup_tensor_family_type_spec,
    primitive_type_specs, tensor_family_type_specs, CollectionKind, CollectionTypeSpec, FuncSig,
    InferVar, MarkerFamily, ParamType, Primitive, PrimitiveTypeSpec, SemanticMutability,
    SemanticParamMode, TensorFamilyKind, TensorFamilyTypeSpec, Type, TypeId, TypeParamConstraint,
    TypeTable, TypeTableSnapshot, ATOMIC_ELEMENT_MSG, INTERVALLUM_BOUND_MSG,
};
pub use radix_types::{SchemaColumnDef, SchemaDef};
