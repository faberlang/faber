use super::unicode_scalar_value;

#[test]
fn unicode_scalar_value_reads_ascii_scalar() {
    assert_eq!(unicode_scalar_value("a"), 'a' as u32);
}

#[test]
fn unicode_scalar_value_reads_non_ascii_scalar() {
    assert_eq!(unicode_scalar_value("Ω"), 'Ω' as u32);
}

#[test]
fn unicode_scalar_value_of_multi_byte_char() {
    assert_eq!(unicode_scalar_value("🔥"), '🔥' as u32);
}
