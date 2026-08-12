//! Semantic type model for the Radix compiler (foundation crate).
//!
//! Owns [`Type`], [`TypeTable`], [`Primitive`], index/width helpers, and the
//! shared [`DefId`] definition-identity handle used by both the type system and
//! HIR. Package `radix` re-exports these through `radix::semantic` for path
//! stability.
//!
//! This crate must not depend on HIR nodes, semantic passes, parser, or codegen.

mod def_id;
mod index;
pub mod instans_precision;
pub mod numeric_width;
mod schema;
mod types;

pub use def_id::DefId;

/// [`DefId`] partition threshold: [`DefId`]s below this value are compiler-injected
/// builtins allocated via `fresh_builtin_def_id()`; [`DefId`]s at or above this
/// value are user declarations allocated via `fresh_def_id()`.
///
/// The constant lives here (not in `radix`) so that leaf crates such as
/// `radix-hir-faber` can import it without depending on the main
/// compiler crate.
pub const USER_DEF_ID_BASE: u32 = 0x0000_1000;
pub use index::{
    compute_cotangent_reduction_axes, literal_index_i64s, IndexExpr, IndexId, IndexVar,
};
pub use instans_precision::{
    effective_instans_precision, instans_assignable, lower_sized_instans, InstansPrecision,
    InstansPrecisionError, InstansPrecisionRank,
};
pub use numeric_width::{
    effective_width, is_conversio_radix_hint, is_integer_index_scalar, is_integer_index_vector,
    is_numeric_width_type_sugar, is_tensor_element_type, is_tensor_numeric_element,
    lista_sugar_width_marker, lower_modular_word, lower_sized_primitive,
    lower_standalone_width_type, matrix_sugar_width_marker, numeric_assignable,
    sparsa_sugar_width_marker, standalone_width_type, tensor_element_conversio_allowed,
    tensor_media_width, tensor_method_requires_numeric_element, tensor_sugar_width_marker,
    vector_sugar_width_marker, NumericWidth, WidthError, TENSOR_ELEMENT_NUMERIC_MSG,
    TENSOR_INDEX_VECTOR_MSG,
};
pub use schema::{SchemaColumnDef, SchemaDef};
pub use types::{
    collection_type_specs, is_atomic_element_type, is_intervallum_bound_type,
    lookup_collection_type_spec, lookup_primitive_type_spec, lookup_tensor_family_type_spec,
    primitive_type_specs, tensor_family_type_specs, CollectionKind, CollectionTypeSpec, FuncSig,
    InferVar, ParamType, Primitive, PrimitiveTypeSpec, SemanticMutability, SemanticParamMode,
    TensorFamilyKind, TensorFamilyTypeSpec, Type, TypeId, TypeParamConstraint, TypeTable,
    TypeTableSnapshot, ATOMIC_ELEMENT_MSG, INTERVALLUM_BOUND_MSG,
};

#[cfg(test)]
#[path = "lattice_property_test.rs"]
mod lattice_property_tests;

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn type_table_seeds_primitives() {
        let table = TypeTable::new();
        for prim in [
            Primitive::Textus,
            Primitive::Numerus,
            Primitive::Fractus,
            Primitive::Bivalens,
            Primitive::Nihil,
            Primitive::Vacuum,
            Primitive::Numquam,
            Primitive::Ignotum,
            Primitive::Octeti,
            Primitive::Regex,
            Primitive::Json,
            Primitive::Valor,
            Primitive::Ascii,
            Primitive::Instans,
        ] {
            let id = table.primitive(prim);
            assert!(
                matches!(table.get(id), Type::Primitive(p) if *p == prim),
                "expected {prim:?} to be seeded, got {id:?}"
            );
        }
    }
}
