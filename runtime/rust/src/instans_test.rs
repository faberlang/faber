use super::{Instans, InstansPraecisio};
use crate::valor::Valor;
use std::cmp::Ordering;

const SAMPLE_EPOCH_NANOS: i64 = 1_703_000_000_123_456_789;
const SAMPLE_UTC_WIRE: &str = "1979-05-27T07:32:00Z";
const SAMPLE_UTC_SUBSEC_WIRE: &str = "1979-05-27T07:32:00.123456Z";

// --- construction / truncation ---

#[test]
fn construction_truncates_sub_second_bits_for_seconds_precision() {
    let instant = Instans::from_nanos(SAMPLE_EPOCH_NANOS, InstansPraecisio::Secunda);
    assert_eq!(instant.nanos(), 1_703_000_000_000_000_000);
}

#[test]
fn construction_truncates_sub_millisecond_bits_for_millis_precision() {
    let instant = Instans::from_nanos(SAMPLE_EPOCH_NANOS, InstansPraecisio::Millisecunda);
    assert_eq!(instant.nanos(), 1_703_000_000_123_000_000);
}

#[test]
fn construction_truncates_pre_epoch_toward_earlier_second_bucket() {
    let valor = Valor::Instans("1969-12-31T23:59:59.999Z".to_string());
    let instant = Instans::try_from_valor(&valor, InstansPraecisio::Secunda).expect("parse");
    assert_eq!(instant.nanos(), -1_000_000_000);
    assert_ne!(instant.nanos(), 0);
}

#[test]
fn ad_praecisionem_narrows_storage() {
    let fine = Instans::from_nanos(SAMPLE_EPOCH_NANOS, InstansPraecisio::Nanosecunda);
    let coarse = fine.ad_praecisionem(InstansPraecisio::Millisecunda);
    assert_eq!(coarse.praecisio(), InstansPraecisio::Millisecunda);
    assert_eq!(coarse.nanos(), 1_703_000_000_123_000_000);
}

// --- equality / ordering ---

#[test]
fn equality_uses_declared_precision_bits() {
    let a = Instans::from_nanos(1_703_000_000_123_456_000, InstansPraecisio::Millisecunda);
    let b = Instans::from_nanos(1_703_000_000_123_456_789, InstansPraecisio::Millisecunda);
    assert_eq!(a, b);
}

#[test]
fn ord_compares_without_recursion() {
    let earlier = Instans::from_epoch_seconds(1, InstansPraecisio::Secunda);
    let later = Instans::from_epoch_seconds(2, InstansPraecisio::Secunda);
    assert_eq!(earlier.cmp(&later), Ordering::Less);
    assert!(earlier < later);
}

#[test]
fn coarser_partial_cmp_uses_millis_when_one_operand_is_millis() {
    let fine = Instans::from_nanos(1_703_000_000_123_456_000, InstansPraecisio::Nanosecunda);
    let coarse = Instans::from_nanos(1_703_000_000_123_999_000, InstansPraecisio::Millisecunda);
    assert_eq!(fine.partial_cmp_at_coarser(coarse), Some(Ordering::Equal));
}

#[test]
fn ord_uses_coarser_precision_for_mixed_precision_values() {
    let fine = Instans::from_nanos(1_703_000_000_123_456_000, InstansPraecisio::Nanosecunda);
    let coarse = Instans::from_nanos(1_703_000_000_123_999_000, InstansPraecisio::Millisecunda);
    assert_eq!(fine.cmp(&coarse), Ordering::Equal);
}

// --- valor / RFC3339 parse ---

#[test]
fn try_from_valor_parses_rfc3339_and_snaps_to_precision() {
    let valor = Valor::Instans(SAMPLE_UTC_SUBSEC_WIRE.to_string());
    let instant = Instans::try_from_valor(&valor, InstansPraecisio::Millisecunda).expect("parse");
    assert_eq!(instant.praecisio(), InstansPraecisio::Millisecunda);
    assert_eq!(instant.nanos(), 296_638_320_123_000_000);
}

#[test]
fn try_from_valor_interprets_numerus_as_epoch_units() {
    let valor = Valor::Numerus(1_703_000_000_123);
    let instant =
        Instans::try_from_valor(&valor, InstansPraecisio::Millisecunda).expect("epoch ms");
    assert_eq!(instant.nanos(), 1_703_000_000_123_000_000);
}

#[test]
fn try_from_valor_parses_rfc3339_textus_wire() {
    let valor = Valor::Textus("1979-05-27T07:32:00Z".to_string());
    let instant = Instans::try_from_valor(&valor, InstansPraecisio::Secunda).expect("text wire");
    assert_eq!(instant.to_rfc3339(), "1979-05-27T07:32:00Z");
}

#[test]
fn try_from_valor_rejects_unsupported_variants() {
    let valor = Valor::Bivalens(true);
    assert_eq!(
        Instans::try_from_valor(&valor, InstansPraecisio::Secunda),
        None
    );
}

#[test]
fn parse_rfc3339_normalizes_positive_offset_to_utc() {
    let utc = Instans::try_from_valor(
        &Valor::Instans("1979-05-27T11:32:00Z".to_string()),
        InstansPraecisio::Secunda,
    )
    .expect("utc wire");
    let offset = Instans::try_from_valor(
        &Valor::Instans("1979-05-27T07:32:00-04:00".to_string()),
        InstansPraecisio::Secunda,
    )
    .expect("offset wire");
    assert_eq!(utc.nanos(), offset.nanos());
}

#[test]
fn parse_rfc3339_normalizes_compact_offset_to_utc() {
    let utc = Instans::try_from_valor(
        &Valor::Instans(SAMPLE_UTC_WIRE.to_string()),
        InstansPraecisio::Secunda,
    )
    .expect("utc wire");
    let offset = Instans::try_from_valor(
        &Valor::Instans("1979-05-27T16:32:00+0900".to_string()),
        InstansPraecisio::Secunda,
    )
    .expect("compact offset");
    assert_eq!(utc.nanos(), offset.nanos());
}

#[test]
fn parse_rfc3339_rejects_missing_offset() {
    let valor = Valor::Instans("1979-05-27T07:32:00".to_string());
    assert_eq!(
        Instans::try_from_valor(&valor, InstansPraecisio::Secunda),
        None
    );
}

// --- RFC3339 emit ---

#[test]
fn to_rfc3339_emits_at_declared_precision() {
    let instant = Instans::from_nanos(296_638_320_123_456_789, InstansPraecisio::Millisecunda);
    assert_eq!(instant.to_rfc3339(), "1979-05-27T07:32:00.123Z");

    let seconds = Instans::from_nanos(296_638_320_123_456_789, InstansPraecisio::Secunda);
    assert_eq!(seconds.to_rfc3339(), SAMPLE_UTC_WIRE);
}

#[test]
fn to_rfc3339_roundtrips_utc_wire_at_same_precision() {
    let parsed = Instans::try_from_valor(
        &Valor::Instans(SAMPLE_UTC_SUBSEC_WIRE.to_string()),
        InstansPraecisio::Microsecunda,
    )
    .expect("parse");
    assert_eq!(parsed.to_rfc3339(), "1979-05-27T07:32:00.123456Z");
}

#[test]
fn epoch_zero_parses_and_roundtrips() {
    let valor = Valor::Instans("1970-01-01T00:00:00Z".to_string());
    let instant = Instans::try_from_valor(&valor, InstansPraecisio::Secunda).expect("epoch zero");
    assert_eq!(instant.nanos(), 0);
    assert_eq!(instant.to_rfc3339(), "1970-01-01T00:00:00Z");
}

#[test]
fn pre_epoch_negative_offset_normalizes_correctly() {
    // -08:00 offset with pre-epoch datetime
    let valor = Valor::Instans("1969-12-31T16:00:00-08:00".to_string());
    let instant =
        Instans::try_from_valor(&valor, InstansPraecisio::Secunda).expect("pre-epoch offset");
    assert_eq!(instant.nanos(), 0);
    assert_eq!(instant.to_rfc3339(), "1970-01-01T00:00:00Z");
}
