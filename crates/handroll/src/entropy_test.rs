use super::{os_entropy, uuid_v4, Rng};

#[test]
fn seed_from_u64_pins_reference_sequence() {
    // Regression anchors for the algorithm itself: first 4 outputs of
    // xoshiro256** seeded via SplitMix64. Derived from the reference
    // implementation (prng.di.unimi.it); changing these without changing
    // the algorithm is a behavioral break.
    let mut seed_42 = Rng::seed_from_u64(42);
    assert_eq!(
        [
            seed_42.next_u64(),
            seed_42.next_u64(),
            seed_42.next_u64(),
            seed_42.next_u64(),
        ],
        [
            0x1578_0b2e_0c2e_c716,
            0x6104_d986_6d11_3a7e,
            0xae17_5332_39e4_99a1,
            0xecb8_ad47_03b3_60a1,
        ]
    );
}

#[test]
#[allow(clippy::float_cmp)] // deterministic equality of two identical draws
fn seeded_rng_produces_identical_sequences() {
    let mut a = Rng::seed_from_u64(42);
    let mut b = Rng::seed_from_u64(42);
    for _ in 0..8 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
    let mut fa = [0_u8; 64];
    let mut fb = [0_u8; 64];
    a.fill_bytes(&mut fa);
    b.fill_bytes(&mut fb);
    assert_eq!(fa, fb);
    for _ in 0..8 {
        assert_eq!(a.gen_f64(), b.gen_f64());
        assert_eq!(
            a.gen_i64_inclusive(-1000, 1000),
            b.gen_i64_inclusive(-1000, 1000)
        );
    }
}

#[test]
fn different_seeds_produce_different_sequences() {
    let mut a = Rng::seed_from_u64(1);
    let mut b = Rng::seed_from_u64(2);
    let mut same = 0;
    for _ in 0..64 {
        if a.next_u64() == b.next_u64() {
            same += 1;
        }
    }
    assert!(
        same == 0,
        "distinct seeds must not collide on any of 64 draws"
    );
}

#[test]
fn gen_f64_stays_in_unit_interval_and_spreads() {
    let mut rng = Rng::seed_from_u64(7);
    let mut min = 1.0_f64;
    let mut max = 0.0_f64;
    for _ in 0..10_000 {
        let value = rng.gen_f64();
        assert!(
            (0.0..1.0).contains(&value),
            "gen_f64 outside [0, 1): {value}"
        );
        min = min.min(value);
        max = max.max(value);
    }
    assert!(
        min < 0.25,
        "expected coverage of the low end, saw min {min}"
    );
    assert!(
        max > 0.75,
        "expected coverage of the high end, saw max {max}"
    );
}

#[test]
fn gen_i64_inclusive_respects_bounds() {
    let mut rng = Rng::seed_from_u64(11);
    for _ in 0..1000 {
        assert_eq!(rng.gen_i64_inclusive(5, 5), 5);
        let v = rng.gen_i64_inclusive(-5, 5);
        assert!((-5..=5).contains(&v));
        let v = rng.gen_i64_inclusive(-1_000_000, 1_000_000);
        assert!((-1_000_000..=1_000_000).contains(&v));
        let v = rng.gen_i64_inclusive(i64::MIN, i64::MAX);
        let _ = v; // every i64 is in range by construction; full-span test below.
    }
}

#[test]
fn gen_i64_inclusive_full_range_spans_both_ends() {
    let mut rng = Rng::seed_from_u64(13);
    let mut saw_negative = false;
    let mut saw_positive = false;
    for _ in 0..100_000 {
        let v = rng.gen_i64_inclusive(i64::MIN, i64::MAX);
        saw_negative |= v < 0;
        saw_positive |= v >= 0;
    }
    assert!(
        saw_negative && saw_positive,
        "full-span draws must hit both signs"
    );
}

#[test]
fn fill_bytes_is_deterministic_and_varied() {
    let mut a = Rng::seed_from_u64(99);
    let mut b = Rng::seed_from_u64(99);
    let mut buf_a = [0_u8; 128];
    let mut buf_b = [0_u8; 128];
    a.fill_bytes(&mut buf_a);
    b.fill_bytes(&mut buf_b);
    assert_eq!(buf_a, buf_b, "same seed must reproduce identical bytes");
    assert!(
        buf_a.iter().any(|&b| b != 0),
        "fill_bytes output must not be all zero"
    );
    assert!(
        buf_a.iter().any(|&b| b != buf_a[0]),
        "fill_bytes output must not be constant"
    );
}

#[test]
fn os_entropy_fills_the_buffer() {
    let mut buf = [0_u8; 64];
    os_entropy(&mut buf).expect("OS entropy source must be available in tests");
    assert!(
        buf.iter().any(|&b| b != 0),
        "OS entropy must not return an all-zero buffer"
    );
}

#[test]
fn uuid_v4_matches_canonical_v4_shape() {
    for _ in 0..256 {
        let uuid = uuid_v4().expect("OS entropy source must be available in tests");
        assert!(
            is_canonical_v4_shape(&uuid),
            "uuid `{uuid}` does not match the canonical v4 shape"
        );
        assert_eq!(uuid.len(), 36, "uuid must be 36 chars, saw `{uuid}`");
    }
}

#[test]
fn uuid_v4_pins_version_and_variant_bits() {
    for _ in 0..64 {
        let uuid = uuid_v4().expect("OS entropy source must be available in tests");
        let hex = uuid.replace('-', "");
        let mut bytes = [0_u8; 16];
        for (i, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
            bytes[i] = u8::from_str_radix(std::str::from_utf8(pair).expect("hex pairs"), 16)
                .expect("valid hex");
        }
        assert_eq!(bytes[6] >> 4, 0x4, "version nibble must be 4");
        assert_eq!(bytes[8] >> 6, 0b10, "variant bits must be 10xx");
    }
}

#[test]
fn uuid_v4_unique_over_large_sample() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..4096 {
        let uuid = uuid_v4().expect("OS entropy source must be available in tests");
        assert!(seen.insert(uuid), "uuid collision in a 4096-sample run");
    }
}

/// Hand-rolled `8-4-4-4-12` v4 shape check: literal hyphens at 8/13/18/23,
/// version nibble `4` at 14, variant nibble `8|9|a|b` at 19, hex elsewhere.
fn is_canonical_v4_shape(uuid: &str) -> bool {
    let bytes = uuid.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| bytes[i] == b'-')
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(i, &b)| {
            let is_hyphen = matches!(i, 8 | 13 | 18 | 23);
            let is_hex = matches!(
                i,
                0..=7 | 9..=12 | 14..=17 | 19..=22 | 24..=35
            );
            (is_hyphen && b == b'-') || (is_hex && b.is_ascii_hexdigit())
        })
}
