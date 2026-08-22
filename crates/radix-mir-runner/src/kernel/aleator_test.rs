use handroll::uuid_v4;
use regex::Regex;

#[test]
fn uuid_v4_is_canonical_lowercase_v4_shape() {
    let shape =
        Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
            .expect("valid v4 shape regex");
    for _ in 0..64 {
        let uuid = uuid_v4().expect("entropy source available");
        assert!(
            shape.is_match(&uuid),
            "uuid `{uuid}` does not match the canonical v4 shape"
        );
    }
}

#[test]
fn uuid_v4_pins_version_and_variant_bits() {
    let uuid = uuid_v4().expect("entropy source available");
    let hex = uuid.replace('-', "");
    let mut bytes = [0_u8; 16];
    for (i, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        bytes[i] = u8::from_str_radix(std::str::from_utf8(pair).expect("hex pairs"), 16)
            .expect("valid hex");
    }
    assert_eq!(bytes[6] >> 4, 0x4, "version nibble must be 4");
    assert_eq!(bytes[8] >> 6, 0b10, "variant bits must be 10xx");
}

#[test]
fn uuid_v4_calls_differ() {
    assert_ne!(
        uuid_v4().expect("entropy source available"),
        uuid_v4().expect("entropy source available")
    );
}
