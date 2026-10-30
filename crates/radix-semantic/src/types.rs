//! Re-exports the Radix type model for the semantic module's public API.
//!
//! This file is the semantic-facing re-export boundary. Type table, primitive,
//! collection, and tensor-family representations come from `radix-types`.
//! Schema metadata is a separate foundation module re-exported here so
//! semantic callers keep one path.

pub use radix_types::{
    ATOMIC_ELEMENT_MSG, AccessMarker, BuiltinTensorEdgePolicy, CollectionKind, CollectionTypeSpec,
    FuncSig, GenericBindingSource, GenericParamDescriptor, GenericParamSort, GenericWitness,
    INTERVALLUM_BOUND_MSG, InferVar, MarkerFamily, ParamType, Primitive, PrimitiveTypeSpec,
    ResolvedGenericBinding, ResolvedGenericCall, SemanticGenericParamDefault, SemanticMutability,
    SemanticParamMode, TensorFamilyKind, TensorFamilyTypeSpec, Type, TypeId, TypeParamConstraint,
    TypeTable, TypeTableSnapshot, collection_type_specs, is_atomic_element_type,
    is_intervallum_bound_type, lookup_collection_type_spec, lookup_primitive_type_spec,
    lookup_tensor_family_type_spec, primitive_type_specs, tensor_family_type_specs,
};
pub use radix_types::{SchemaColumnDef, SchemaDef};

/// Whether an explicit generic prefix is admissible under the shared
/// explicit-prefix/default-suffix binder law. A prefix must cover every
/// non-defaulted descriptor and may omit only the trailing defaulted suffix.
#[must_use]
pub fn generic_explicit_prefix_len_in_range(
    params: &[radix_types::GenericParamDescriptor],
    len: usize,
) -> bool {
    let required = params
        .iter()
        .take_while(|param| param.default.is_none())
        .count();
    len >= required && len <= params.len()
}

/// Stable lowering issue for a marker on a scalar carrier.
pub const ISSUE_SCALAR_ACCESS_MARKER: &str = "scalar_access_marker";

/// Mutating catalog intrinsic invoked through a `ref` root or view.
pub const ISSUE_MUTATING_RECEIVER_THROUGH_REF: &str = "mutating_receiver_through_ref";

/// Projection assignment through a `ref` root or view.
pub const ISSUE_ASSIGN_THROUGH_REF: &str = "assign_through_ref";
pub const ISSUE_ASSIGN_TO_DE_PARAMETER: &str = "assign_to_de_parameter";

/// Passing a `ref` value into a `mut` or `own` position.
pub const ISSUE_REF_TO_MUT_OR_OWN_POSITION: &str = "ref_to_mut_or_own_position";

/// Call argument lacks the marker its `mut`/`own`/`copy` parameter declares.
pub const ISSUE_CALL_MARKER_MISSING: &str = "call_marker_missing";

/// Call argument marker disagrees with the parameter's declared marker.
pub const ISSUE_CALL_MARKER_MISMATCH: &str = "call_marker_mismatch";

/// Call argument carries `mut`/`own`/`copy` against a read-only parameter.
pub const ISSUE_CALL_MARKER_ON_READ_ONLY: &str = "call_marker_on_read_only";

/// A call's explicit size argument was written without the `size` marker.
pub const ISSUE_SIZE_MARKER_REQUIRED: &str = "size_marker_required";

/// A call's explicit `size` argument has no `size` parameter to bind.
pub const ISSUE_SIZE_MARKER_UNEXPECTED: &str = "size_marker_unexpected";

/// Own-bind from a live unmarked source. Suggest `copy` (D-6).
pub const ISSUE_UNMARKED_INTO_OWN: &str = "unmarked_into_own";

/// Own-bind from a `ref` source. Suggest `copy` (D-6).
pub const ISSUE_REF_INTO_OWN: &str = "ref_into_own";

/// Own-bind from a `mut` source. Suggest `copy` (D-6).
pub const ISSUE_MUT_INTO_OWN: &str = "mut_into_own";

/// Local declared with a `mut` access marker (OQ-3: grammar allows, lints discourage).
pub const ISSUE_MUT_LOCAL: &str = "mut_local";

/// Own-bind from a field projection. Suggest `copy` (D-6).
pub const ISSUE_FIELD_INTO_OWN: &str = "field_into_own";

/// Own-bind from an index projection. Suggest `copy` (D-6).
pub const ISSUE_INDEX_INTO_OWN: &str = "index_into_own";

/// Own-bind from a dereference. Suggest `copy` (D-6).
pub const ISSUE_DEREF_INTO_OWN: &str = "deref_into_own";

/// Own-bind from optional/non-null chaining. Suggest `copy` (D-6).
pub const ISSUE_PROJECTION_INTO_OWN: &str = "projection_into_own";

/// Own-bind from any other shared/aliased RHS. Suggest `copy` (D-6).
pub const ISSUE_SHARED_INTO_OWN: &str = "shared_into_own";

/// Suggested spelling attached to every illegal own-bind.
pub const SUGGESTION_COPY: &str = "copy";

/// A `copy` acquisition whose value transitively holds a member that cannot
/// be duplicated (a conversation handle or another closable resource).
pub const ISSUE_COPY_NONDUPLICABLE_MEMBER: &str = "copy_nonduplicable_member";

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
