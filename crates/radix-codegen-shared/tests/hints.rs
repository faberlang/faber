use radix_codegen_shared::hints::{EndianHint, classify, endian_hint, has_bits_hint};

fn classify_names(names: &[&str]) -> (Option<EndianHint>, bool) {
    (
        endian_hint(names, |name| classify(name)),
        has_bits_hint(names, |name| classify(name)),
    )
}

#[test]
fn recognizes_endian_and_bits_hints() {
    assert_eq!(
        classify_names(&["Unused", "Le", "Bits"]),
        (Some(EndianHint::Le), true)
    );
}
