use radix_codegen_shared::hints::{EndianHint, classify, endian_hint};

#[test]
fn recognizes_endian_hints_and_no_bits_hint() {
    assert_eq!(
        endian_hint(&["Unused", "Le", "Bits"], |name| classify(name)),
        Some(EndianHint::Le)
    );
    // `via Bits` retired: `Bits` is no hint at all.
    assert_eq!(endian_hint(&["Bits"], |name| classify(name)), None);
}
