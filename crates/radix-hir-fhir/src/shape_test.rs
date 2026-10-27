//! Proofs that the wire-shape hash is derived from the types: changing a
//! serialized type changes the hash with no constant edited anywhere.

use super::{shape_hash_of, trace_registry, wire_shape_hash};
use crate::artifact::HirArtifact;
use serde::{Deserialize, Serialize};
use serde_reflection::{ContainerFormat, Format};

#[derive(Serialize, Deserialize)]
struct Leaf {
    id: u32,
}

#[derive(Serialize, Deserialize)]
struct Base {
    version: u32,
    leaf: Leaf,
}

/// `Base` plus one appended field: the exact shape of an additive change
/// that would shift every later `postcard` byte.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Base")]
struct BaseWithAddedField {
    version: u32,
    leaf: Leaf,
    added: Vec<u32>,
}

/// `Base` whose nested struct gained a field.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Base")]
struct BaseWithNestedField {
    version: u32,
    leaf: LeafWithField,
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "Leaf")]
struct LeafWithField {
    id: u32,
    extra: bool,
}

/// `Base` with its two fields swapped.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Base")]
struct BaseReordered {
    leaf: Leaf,
    version: u32,
}

#[derive(Serialize, Deserialize)]
enum Choice {
    One,
    Two(u32),
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "Choice")]
enum ChoiceWithVariant {
    One,
    Two(u32),
    Three { flag: bool },
}

#[derive(Serialize, Deserialize)]
struct HoldsChoice {
    choice: Choice,
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "HoldsChoice")]
struct HoldsChoiceWithVariant {
    choice: ChoiceWithVariant,
}

#[test]
fn artifact_shape_is_traceable() {
    let derived = shape_hash_of::<HirArtifact>().expect("the artifact shape must trace");
    assert_ne!(derived, 0, "0 is the untraceable-shape fallback");
    assert_eq!(wire_shape_hash(), derived);
}

#[test]
fn shape_hash_is_deterministic() {
    let first = shape_hash_of::<Base>().expect("trace");
    let second = shape_hash_of::<Base>().expect("trace");
    assert_eq!(first, second);
    assert_eq!(wire_shape_hash(), wire_shape_hash());
    assert_eq!(
        wire_shape_hash(),
        shape_hash_of::<HirArtifact>().expect("trace")
    );
}

#[test]
fn adding_a_field_to_a_serialized_struct_changes_the_hash() {
    let before = shape_hash_of::<Base>().expect("trace");
    let after = shape_hash_of::<BaseWithAddedField>().expect("trace");
    assert_ne!(
        before, after,
        "an appended field must change the shape hash"
    );
}

#[test]
fn adding_a_field_to_a_nested_struct_changes_the_hash() {
    let before = shape_hash_of::<Base>().expect("trace");
    let after = shape_hash_of::<BaseWithNestedField>().expect("trace");
    assert_ne!(
        before, after,
        "a field added to a reachable struct must change the shape hash"
    );
}

#[test]
fn reordering_fields_changes_the_hash() {
    let before = shape_hash_of::<Base>().expect("trace");
    let after = shape_hash_of::<BaseReordered>().expect("trace");
    assert_ne!(
        before, after,
        "positional wire: field order is part of the shape"
    );
}

#[test]
fn adding_an_enum_variant_changes_the_hash() {
    let before = shape_hash_of::<HoldsChoice>().expect("trace");
    let after = shape_hash_of::<HoldsChoiceWithVariant>().expect("trace");
    assert_ne!(
        before, after,
        "a variant added to a reachable enum must change the shape hash"
    );
}

/// The real artifact's description reaches the presentation sidecar fields
/// that were added without a version bump, so the same mechanism would have
/// caught them.
#[test]
fn artifact_shape_covers_nested_presentation_fields() {
    let registry = trace_registry::<HirArtifact>().expect("trace artifact shape");
    let Some(ContainerFormat::Struct(fields)) = registry.get("HirPresentation") else {
        panic!("HirPresentation must be reachable from HirArtifact");
    };
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    for expected in ["radical_quarta_ids", "template_holes"] {
        assert!(names.contains(&expected), "missing {expected} in {names:?}");
    }
    let Some(ContainerFormat::Struct(root)) = registry.get("HirArtifact") else {
        panic!("HirArtifact must be the traced root");
    };
    assert!(
        root.iter()
            .any(|field| field.name == "shape_hash" && matches!(field.value, Format::U64)),
        "the artifact header must carry the shape hash as a u64"
    );
}
