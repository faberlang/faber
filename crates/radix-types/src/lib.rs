//! Semantic type model for the Radix compiler (foundation crate).
//!
//! Owns [`Type`], [`TypeTable`], [`Primitive`], index/width helpers, the
//! shared [`DefId`] definition-identity handle used by both the type system and
//! HIR, and the provider-neutral `filtrum` relational plan model
//! (census-types S3). Package `radix` re-exports these through
//! `radix::semantic` for path stability.
//!
//! This crate must not depend on HIR nodes, semantic passes, parser, or codegen.

mod capability;
mod def_id;
mod filtrum;
pub mod format_spec;
mod index;
pub mod instans_precision;
pub mod numeric_width;
pub mod ordering;
mod schema;
mod schema_subset;
mod types;
pub mod vocabulary;
pub mod width_pattern;
pub use width_pattern::{
    WidthFamily, WidthPattern, is_family_catchall, literal_length_guard, width_pattern,
    width_pattern_applies, width_pattern_covers,
};

pub use capability::{
    CAPABILITY_VOCABULARY_VERSION, CapabilityAnswer, CapabilityFamily, CapabilityKind,
    CapabilityReason, CapturePosture, CaptureRequirement, EffectPosture, ExportVisibility,
    FunctionCapabilityAnswer, FunctionCapabilityKind, FunctionCapabilityProof,
    FunctionCapabilitySummary, IndexCapabilityAnswer, IndexCapabilityKind, IndexCapabilityProof,
    IndexCapabilitySummary, Liveness, ParameterAccess, ParameterAccessPosture,
    PlaceCapabilityAnswer, PlaceCapabilityKind, PlaceCapabilityProof, RecursionOutcome,
    RegionRelationship, RequiredRegionRelationships, ReturnedBorrowProvenance,
    SolvedIndexedIdentity, TypeCapabilityAnswer, TypeCapabilityKind, TypeCapabilityProof,
    TypeCapabilitySummary,
};
pub use def_id::{
    CLOSURE_LIFT_DEF_ID_BASE, COMPANION_DEF_ID_FLAG, DefId, HIR_SYNTHETIC_DEF_ID_BASE,
    PACKAGE_MIR_SYNTHETIC_DEF_BASE, PROVIDER_DEF_ID_BASE,
};
pub use filtrum::{
    CapabilityRequirement, ColumnRef, ComparisonOp, FILTRUM_ENCODING_VERSION_V1, FiltrumPlan,
    OrderRow, Predicate, PredicateValue, SortDirection, SourceIdentity, Window, WindowError,
};
pub use vocabulary::{
    CapabilityGap, KernelModule, Lowerability, MirAtomicOp, MirCollectionOp, MirKernelAxis,
    MirKernelBuiltin,
};

/// [`DefId`] partition threshold: [`DefId`]s below this value are compiler-injected
/// builtins allocated via `fresh_builtin_def_id()`; [`DefId`]s at or above this
/// value are user declarations allocated via `fresh_def_id()`.
///
/// The constant lives here (not in `radix`) so that leaf crates such as
/// `radix-hir-faber` can import it without depending on the main
/// compiler crate.
pub const USER_DEF_ID_BASE: u32 = 0x0000_1000;
pub use index::{
    IndexExpr, IndexId, IndexVar, compute_cotangent_reduction_axes, i64s_from_literal_index,
    parse_natural_capacity,
};
pub use instans_precision::{
    InstansPrecision, InstansPrecisionError, InstansPrecisionRank, effective_instans_precision,
    is_instans_assignable, lower_sized_instans,
};
pub use numeric_width::{
    DECIMAL_INDEX_SCALAR_MSG, DECIMAL_TENSOR_MEDIA_MSG, NumericWidth, TENSOR_ELEMENT_NUMERIC_MSG,
    TENSOR_INDEX_VECTOR_MSG, WidthError, WidthLowerIssue, effective_width, is_ascii_format_hint,
    is_conversio_radix_hint, is_decimal_index_scalar, is_integer_index_scalar,
    is_integer_index_vector, is_numeric_assignable, is_numeric_width_type_sugar,
    is_tensor_element_type, is_tensor_numeric_element, lista_sugar_width_marker,
    lower_modular_word, lower_sized_primitive, lower_standalone_width_type,
    matrix_sugar_width_marker, sparsa_sugar_width_marker, standalone_width_type,
    tensor_element_conversio_allowed, tensor_media_width, tensor_method_requires_numeric_element,
    tensor_sugar_width_marker, vector_sugar_width_marker,
};
pub use schema::{SchemaColumnDef, SchemaDef};
pub use schema_subset::{
    SchemaSubsetError, extra_input_columns, schema_column_count, schema_subset,
};
pub use types::{
    ATOMIC_ELEMENT_MSG, AccessMarker, AppliedArg, CollectionKind, CollectionTypeSpec, ContractRef,
    FuncSig, GenericBindingSource, GenericParamDescriptor, GenericParamSort, GenericWitness,
    INTERVALLUM_BOUND_MSG, InferVar, MarkerFamily, ParamType, Primitive, PrimitiveTypeSpec,
    ResolvedGenericBinding, ResolvedGenericCall, SemanticGenericParamDefault, SemanticMutability,
    SemanticParamMode, TensorFamilyKind, TensorFamilyTypeSpec, Type, TypeId, TypeParamConstraint,
    TypeTable, TypeTableSnapshot, collection_type_specs, is_atomic_element_type,
    is_intervallum_bound_type, lookup_collection_type_spec, lookup_primitive_type_spec,
    lookup_tensor_family_type_spec, primitive_type_specs, tensor_family_type_specs,
};

#[cfg(test)]
#[path = "capability_test.rs"]
mod capability_test;

#[cfg(test)]
#[path = "lattice_property_test.rs"]
mod lattice_property_tests;

#[cfg(test)]
#[path = "width_pattern_test.rs"]
mod width_pattern_tests;

#[cfg(test)]
#[path = "schema_subset_test.rs"]
mod schema_subset_tests;

#[cfg(test)]
#[path = "filtrum_test.rs"]
mod filtrum_tests;

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
