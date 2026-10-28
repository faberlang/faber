//! Semantic type model for the Radix compiler (foundation crate).
//!
//! Owns [`Type`], [`TypeTable`], [`Primitive`], index/width helpers, the
//! shared [`DefId`] definition-identity handle used by both the type system and
//! HIR, and the provider-neutral `filtrum` relational plan model
//! (census-types S3). Package `radix` re-exports these through
//! `radix::semantic` for path stability.
//!
//! This crate must not depend on HIR nodes, semantic passes, parser, or codegen.

pub mod big_literal;
mod capability;
mod def_id;
pub mod exact_rational;
mod filtrum;
pub mod format_spec;
mod index;
pub mod instans_precision;
pub mod numeric_range;
pub mod numeric_width;
pub mod ordering;
mod schema;
mod schema_subset;
pub mod type_graph;
mod types;
pub mod vocabulary;
pub use big_literal::Magnus;

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
    CapabilityGap, KernelLowering, KernelModule, Lowerability, MirAtomicOp, MirCollectionOp,
    MirKernelAxis, MirKernelBuiltin, MirMathFunction,
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
    EXPLICIT_SIZE_MARKER, ExplicitSize, IndexArithmeticError, IndexExpr, IndexId, IndexVar,
    RuntimeExtentId, add_index_constant, compose_index_affine_constant,
    compute_cotangent_reduction_axes, divide_index_exact, evaluate_index_dimension,
    i64s_from_literal_index, parse_explicit_size, parse_natural_capacity, parse_spelled_shape,
    spell_explicit_size, spell_natural_shape,
};
pub use instans_precision::{
    InstansPrecision, InstansPrecisionError, InstansPrecisionRank, effective_instans_precision,
    is_instans_assignable, lower_sized_instans,
};
pub use numeric_width::{
    BitsWidth, DECIMAL_INDEX_SCALAR_MSG, DECIMAL_TENSOR_MEDIA_MSG, NumericWidth, RangeEnd,
    TENSOR_ELEMENT_NUMERIC_MSG, TENSOR_INDEX_VECTOR_MSG, WidthError, WidthLowerIssue,
    effective_width, float_holds_integer, float_overflows, has_range_arg, is_ascii_format_hint,
    is_code_point_hint, is_conversio_radix_hint, is_decimal_index_scalar, is_integer_index_scalar,
    is_integer_index_vector, is_numeric_assignable, is_numeric_width_type_sugar,
    is_tensor_element_type, is_tensor_float_element, is_tensor_numeric_element,
    lista_sugar_width_marker, lower_modular_word, lower_ranged_word, lower_saturating_word,
    lower_standalone_width_type, lower_trapping_word, matrix_sugar_width_marker,
    resolve_ranged_int, sparsa_sugar_width_marker, standalone_width_type,
    tensor_element_conversio_allowed, tensor_media_width, tensor_method_requires_numeric_element,
    tensor_sugar_width_marker, vector_sugar_width_marker,
};
pub use schema::{SchemaColumnDef, SchemaDef};
pub use schema_subset::{
    SchemaSubsetError, extra_input_columns, schema_column_count, schema_subset,
};
pub use types::{
    ATOMIC_ELEMENT_MSG, AccessMarker, AppliedArg, BuiltinTensorEdgePolicy, CollectionKind,
    CollectionTypeSpec, ContractRef, FloatPolicy, FuncSig, GenericBindingSource,
    GenericParamDescriptor, GenericParamSort, GenericWitness, INTERVALLUM_BOUND_MSG, InferVar,
    IntPolicy, MarkerFamily, ParamType, Primitive, PrimitiveTypeSpec, ResolvedGenericBinding,
    ResolvedGenericCall, SemanticGenericParamDefault, SemanticMutability, SemanticParamMode,
    TensorFamilyKind, TensorFamilyTypeSpec, Type, TypeId, TypeParamConstraint, TypeTable,
    TypeTableSnapshot, collection_type_specs, is_atomic_element_type, is_intervallum_bound_type,
    lookup_collection_type_spec, lookup_primitive_type_spec, lookup_tensor_family_type_spec,
    primitive_type_specs, tensor_family_type_specs,
};

#[cfg(test)]
#[path = "capability_test.rs"]
mod capability_test;

#[cfg(test)]
#[path = "lattice_property_test.rs"]
mod lattice_property_tests;

#[cfg(test)]
#[path = "schema_subset_test.rs"]
mod schema_subset_tests;

#[cfg(test)]
#[path = "filtrum_test.rs"]
mod filtrum_tests;

#[cfg(test)]
#[path = "smoke_test.rs"]
mod smoke_tests;
