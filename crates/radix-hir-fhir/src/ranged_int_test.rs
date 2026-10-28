//! A ranged store policy on the FHIR wire (schema 50, need).
//!
//! `Type::RangedInt` carries the policy, the width and the finite inclusive
//! range in the type table, so a loaded unit lowers the same store. The
//! range is part of the type's identity: two ranges stay two types.

use crate::decode::{decode, encode};
use crate::test_support::stub_artifact;
use radix_types::{IntPolicy, NumericWidth, Type, TypeTable};

#[test]
fn a_ranged_type_round_trips_with_policy_width_and_range() {
    let mut types = TypeTable::new();
    let saturating = types.ranged_int(IntPolicy::Saturating, NumericWidth::U32, 0, 7);
    let trapping = types.ranged_int(IntPolicy::Trapping, NumericWidth::I32, -2, 2);
    let wrapping = types.ranged_int(IntPolicy::Wrapping, NumericWidth::U8, 3, 9);
    let mut artifact = stub_artifact();
    artifact.types = types.snapshot();

    let decoded = decode(&encode(&artifact).expect("encode")).expect("decode");
    assert_eq!(artifact, decoded);
    let restored = TypeTable::from_snapshot(decoded.types).expect("restore the type table");
    assert_eq!(
        restored.ranged_int_parts(saturating),
        Some((IntPolicy::Saturating, NumericWidth::U32, 0, 7))
    );
    assert_eq!(
        restored.ranged_int_parts(trapping),
        Some((IntPolicy::Trapping, NumericWidth::I32, -2, 2))
    );
    assert_eq!(
        restored.ranged_int_parts(wrapping),
        Some((IntPolicy::Wrapping, NumericWidth::U8, 3, 9))
    );
}

#[test]
fn the_whole_width_range_never_reaches_the_wire_as_a_ranged_type() {
    // `saturating<u8, 0…255>` is the existing `saturating<u8>`: one identity.
    let mut types = TypeTable::new();
    let full = types.ranged_int(IntPolicy::Saturating, NumericWidth::U8, 0, 255);
    assert_eq!(types.ranged_int_parts(full), None);
    let mut artifact = stub_artifact();
    artifact.types = types.snapshot();
    let decoded = decode(&encode(&artifact).expect("encode")).expect("decode");
    let restored = TypeTable::from_snapshot(decoded.types).expect("restore the type table");
    assert!(matches!(
        restored.get(full),
        Type::SaturatingWord(NumericWidth::U8)
    ));
}

#[test]
fn the_wire_schema_version_moved_with_the_new_type_variant() {
    assert!(
        crate::artifact::SCHEMA_VERSION >= 50,
        "Type::RangedInt is part of the serialized type enum from schema 50"
    );
}

#[test]
fn a_ranged_type_round_trips_through_the_msgpack_envelope() {
    // The envelope has no 128-bit integer: the ends of a range travel as two
    // 64-bit halves, including a u64 range past `i64::MAX` and a negative end.
    let mut types = TypeTable::new();
    let high = types.ranged_int(
        IntPolicy::Saturating,
        NumericWidth::U64,
        5,
        i128::from(u64::MAX) - 1,
    );
    let negative = types.ranged_int(
        IntPolicy::Trapping,
        NumericWidth::I64,
        i128::from(i64::MIN) + 1,
        9,
    );
    let mut artifact = stub_artifact();
    artifact.types = types.snapshot();

    let bytes = crate::msgpack::encode_msgpack(&artifact).expect("msgpack encode");
    let decoded = crate::msgpack::decode_msgpack(&bytes).expect("msgpack decode");
    let restored = TypeTable::from_snapshot(decoded.types).expect("restore the type table");
    assert_eq!(
        restored.ranged_int_parts(high),
        Some((
            IntPolicy::Saturating,
            NumericWidth::U64,
            5,
            i128::from(u64::MAX) - 1
        ))
    );
    assert_eq!(
        restored.ranged_int_parts(negative),
        Some((
            IntPolicy::Trapping,
            NumericWidth::I64,
            i128::from(i64::MIN) + 1,
            9
        ))
    );
}
