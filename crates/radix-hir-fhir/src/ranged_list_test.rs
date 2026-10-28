//! A list length range on the FHIR wire (schema 56, need).
//!
//! `Type::RangedArray` carries the element and the inclusive length range in
//! the type table, so a loaded unit lowers the same checked push, removal and
//! conversion. The sugar forms never reach the wire as a range.

use crate::decode::{decode, encode};
use crate::test_support::stub_artifact;
use radix_types::{Primitive, Type, TypeTable};

#[test]
fn a_length_range_round_trips_with_element_and_ends() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let finite = types.ranged_array(int, 4, Some(7));
    let open = types.ranged_array(int, 1, None);
    let mut artifact = stub_artifact();
    artifact.types = types.snapshot();

    let decoded = decode(&encode(&artifact).expect("encode")).expect("decode");
    assert_eq!(artifact, decoded);
    let restored = TypeTable::from_snapshot(decoded.types).expect("restore the type table");
    assert!(matches!(
        restored.get(finite),
        Type::RangedArray {
            min: 4,
            max: Some(7),
            ..
        }
    ));
    assert!(matches!(
        restored.get(open),
        Type::RangedArray {
            min: 1,
            max: None,
            ..
        }
    ));
}

#[test]
fn a_length_range_round_trips_through_the_msgpack_envelope() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let huge = types.ranged_array(int, 1, Some(u64::MAX));
    let mut artifact = stub_artifact();
    artifact.types = types.snapshot();

    let bytes = crate::msgpack::encode_msgpack(&artifact).expect("msgpack encode");
    let decoded = crate::msgpack::decode_msgpack(&bytes).expect("msgpack decode");
    let restored = TypeTable::from_snapshot(decoded.types).expect("restore the type table");
    assert!(matches!(
        restored.get(huge),
        Type::RangedArray {
            min: 1,
            max: Some(u64::MAX),
            ..
        }
    ));
}

#[test]
fn the_wire_schema_version_moved_with_the_new_type_variant() {
    assert!(
        crate::artifact::SCHEMA_VERSION >= 56,
        "Type::RangedArray is part of the serialized type enum from schema 56"
    );
}
