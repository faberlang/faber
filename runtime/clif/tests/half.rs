//! `f16` patterns and values convert both ways without losing a bit
//! (corpus `conversio/bits-conversio`).

use faber_rt_clif::{__faber_clif_v1_f16_from_bits, __faber_clif_v1_f16_to_bits};

#[test]
fn every_non_nan_pattern_round_trips() {
    for pattern in 0u16..=u16::MAX {
        let is_nan = pattern & 0x7c00 == 0x7c00 && pattern & 0x03ff != 0;
        if is_nan {
            continue;
        }
        let value = __faber_clif_v1_f16_from_bits(std::ptr::null(), i64::from(pattern));
        assert_eq!(
            __faber_clif_v1_f16_to_bits(std::ptr::null(), value),
            i64::from(pattern),
            "pattern {pattern:#06x}"
        );
    }
}

#[test]
fn known_values_and_nan_payloads() {
    assert_eq!(__faber_clif_v1_f16_from_bits(std::ptr::null(), 0x3c00), 1.0);
    assert_eq!(__faber_clif_v1_f16_from_bits(std::ptr::null(), 0x3e00), 1.5);
    assert_eq!(
        __faber_clif_v1_f16_from_bits(std::ptr::null(), 0x7c00),
        f64::INFINITY
    );
    assert_eq!(
        __faber_clif_v1_f16_from_bits(std::ptr::null(), 0xfc00),
        f64::NEG_INFINITY
    );
    // 0x7e00 is the quiet NaN: it survives the round trip bit for bit.
    let nan = __faber_clif_v1_f16_from_bits(std::ptr::null(), 0x7e00);
    assert!(nan.is_nan());
    assert_eq!(__faber_clif_v1_f16_to_bits(std::ptr::null(), nan), 0x7e00);
}
