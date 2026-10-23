//! Raw bits (`b8 b16 b32 b64`) in the C leaf: the class is carried as the
//! unsigned C cell of its width, and every surface the leaf does not yet
//! carry is refused by name rather than emitted through a promoted integer
//! carrier, which would be a wrong program (operator ruling 2026-10-10:
//! `bits have their width always`).

use crate::conversio::bits_conversion_width_mismatch;
use crate::error::{UNSUPPORTED_ISSUE, unsupported};
use crate::test_support::{emitted, program};
use crate::ty::{Ty, int_width_of_bits, refuse_bits_operand};
use radix_codegen_shared::CodegenError;
use radix_types::{BitsWidth, TypeTable};

/// The `construct` fact of a refusal, after asserting the `issue` and
/// `target` facts.
fn construct_of(error: &CodegenError) -> String {
    let arg = |wanted: &str| {
        error
            .args
            .iter()
            .find(|arg| arg.name == wanted)
            .map_or_else(
                || panic!("missing diagnostic arg {wanted}"),
                |arg| arg.value.clone(),
            )
    };
    assert_eq!(arg("issue"), UNSUPPORTED_ISSUE);
    assert_eq!(arg("target"), "c");
    arg("construct")
}

/// Same-width scalar conversions in the corpus reinterpret one cell rather
/// than range-checking or converting through a wider signed value.
#[test]
fn bits_scalar_conversion_reads_the_pattern() {
    let corpus = include_str!("../../../corpus/bits/bits-scalar-conversion.fab");
    let body = corpus
        .split_once("\n+++\n")
        .expect("corpus frontmatter closes")
        .1;
    let code = emitted(body);
    assert!(code.contains("faber_bits_read_i8"), "{code}");
    assert!(code.contains("((uint8_t)("), "{code}");
}

/// Floating-point bit conversions preserve the declared IEEE width in both
/// directions instead of casting through an integer value.
#[test]
fn bits_float_conversion_copies_the_pattern() {
    let code = emitted(
        "main {\n    let f32 single ← 1.5\n    let b32 single_bits ← single ↦ b32\n    let f32 single_back ← single_bits ↦ f32\n    let f64 double ← 1.5\n    let b64 double_bits ← double ↦ b64\n    let f64 double_back ← double_bits ↦ f64\n}",
    );
    assert!(code.contains("faber_bits_of_f32"), "{code}");
    assert!(code.contains("faber_f32_of_bits"), "{code}");
    assert!(code.contains("faber_bits_of_f64"), "{code}");
    assert!(code.contains("faber_f64_of_bits"), "{code}");
}

/// Even though semantic analysis normally diagnoses this width change first,
/// the C boundary independently refuses `b8 ↦ b16` as a named unsupported
/// construct if mismatched HIR reaches it.
#[test]
fn bits_conversion_mixing_is_still_refused() {
    let construct =
        bits_conversion_width_mismatch(Some(Ty::Bits(BitsWidth::B8)), Ty::Bits(BitsWidth::B16))
            .expect("cross-width bits conversion must be named");
    let error = unsupported(construct);
    assert_eq!(
        construct_of(&error),
        "raw bits conversion at a different width"
    );
}

/// A raw-bits operand refuses every operator by name. Authored `bN` reaches
/// this leaf now that the C target carries it, but the front end refuses bits
/// arithmetic first with SEM011 `bits_arithmetic_refused`. Pin the target guard
/// at the class it reads: an operand whose type classifies to `Ty::Bits`.
#[test]
fn bits_arithmetic_refused_by_name() {
    let mut types = TypeTable::new();
    let int = types.sized_numeric(
        radix_types::Primitive::Numerus,
        radix_types::NumericWidth::I64,
    );
    for width in [
        BitsWidth::B8,
        BitsWidth::B16,
        BitsWidth::B32,
        BitsWidth::B64,
    ] {
        let ty = types.bits(width);
        let error = refuse_bits_operand(&types, Some(ty), "arithmetic on raw bits")
            .expect_err("a raw-bits operand must be refused");
        assert_eq!(construct_of(&error), "arithmetic on raw bits");
        let error = refuse_bits_operand(&types, Some(ty), "bitwise operator on raw bits")
            .expect_err("a raw-bits operand must be refused");
        assert_eq!(construct_of(&error), "bitwise operator on raw bits");
    }
    // An ordinary integer operand still passes: the guard names bits only.
    assert!(refuse_bits_operand(&types, Some(int), "arithmetic on raw bits").is_ok());
    assert!(refuse_bits_operand(&types, None, "arithmetic on raw bits").is_ok());
}

/// A `list<bN>` literal carries raw bits through its own C family and shows
/// each element as a width-padded lowercase hexadecimal pattern.
#[test]
fn bits_list_round_trip() {
    let code = program(
        "bits_display.fab",
        include_str!("../tests/fixtures/run/bits_display.fab"),
    );
    assert!(code.contains("faber_list_b8_of(faber_a,"), "{code}");
    assert!(code.contains("faber_list_b8_show(faber_a,"), "{code}");

    // Empty construction and append exercise the distinct family entrypoints,
    // in addition to the corpus literal's bulk construction and display.
    let mutating = crate::test_support::emitted(
        "main {\n    mut list<b8> bytes ← empty\n    bytes.append(0x0f)\n}",
    );
    assert!(mutating.contains("faber_list_b8_new"), "{mutating}");
    assert!(mutating.contains("faber_list_b8_push"), "{mutating}");
}

/// The C emitter keeps every operation in a raw-bits value at its own width.
#[test]
fn bits_operations_act_at_the_width() {
    let code = crate::test_support::program(
        "bits_operations.fab",
        include_str!("../tests/fixtures/run/bits_operations.fab"),
    );
    assert!(code.contains("faber_bits_shl_b8"), "{code}");
    assert!(code.contains("~(uint8_t)l_a"), "{code}");
    // The source pipeline currently stops `a + mask` at SEM011 before C;
    // pin the C target's unsupported construct guard at its own boundary.
    let mut types = TypeTable::new();
    let bits = types.bits(BitsWidth::B8);
    let error = refuse_bits_operand(&types, Some(bits), "arithmetic on raw bits")
        .expect_err("arithmetic on raw bits must remain unsupported");
    assert_eq!(construct_of(&error), "arithmetic on raw bits");
}

/// Raw-bits locals use their own unsigned C cell rather than the promoted
/// integer value class.
#[test]
fn bits_store_is_the_unsigned_width_cell() {
    let code = crate::test_support::emitted(
        "main {\n    let b8 one ← 0x01\n    let b64 wide ← 0x0000000000000001\n}",
    );
    assert!(code.contains("uint8_t l_one = "), "{code}");
    assert!(!code.contains("const int64_t l_one ="), "{code}");
    assert!(code.contains("uint64_t l_wide = "), "{code}");
    assert!(!code.contains("const int64_t l_wide ="), "{code}");
}

/// Raw-bits printing uses the width-specific hexadecimal conversion macro and
/// passes its unsigned cell through the variadic-call carrier.
#[test]
fn bits_print_is_hex_at_full_width() {
    let code = crate::test_support::emitted(
        "main {\n    let b8 one8 ← 0x01\n    let b16 one16 ← 0x0001\n    let b32 one32 ← 0x00000001\n    let b64 one64 ← 0x0000000000000001\n    print one8\n    print one16\n    print one32\n    print one64\n}",
    );
    assert!(code.contains("0x%02\" PRIx8"), "{code}");
    assert!(code.contains("0x%04\" PRIx16"), "{code}");
    assert!(code.contains("0x%08\" PRIx32"), "{code}");
    assert!(code.contains("0x%016\" PRIx64"), "{code}");
    assert!(code.contains("(uint64_t)l_one8"), "{code}");
    assert!(code.contains("(uint64_t)l_one64"), "{code}");
}

/// `lista<b8> ↦ string` is the UTF-8 decode and `string ↦ lista<b8>` the
/// encode: the byte list and the counted text are the same bytes, so the
/// emitted program calls the two runtime helpers rather than a list show or a
/// cast.
#[test]
fn bits_bytes_decode_and_encode() {
    let code = emitted(
        "main {\n    let string word ← \"héllo\"\n    let list<b8> utf8 ← word ↦ list<b8>\n    print utf8 ↦ string ⊥ \"?\"\n}",
    );
    assert!(code.contains("faber_text_bytes(faber_a,"), "{code}");
    assert!(code.contains("faber_try_bytes_text(faber_a,"), "{code}");
}

/// The 16- and 32-bit text pairs stay refused by name: the C leaf carries the
/// UTF-8 pair only, and a UTF-16 or UTF-32 unit list printed as a list would
/// be a wrong program, not a missing one.
#[test]
fn bits_bytes_text_wide_pairs_are_refused() {
    let construct = crate::test_support::rejected(
        "main {\n    let list<b16> units ← [0x0068]\n    let string word ← units ↦ string ⊥ \"?\"\n}",
    );
    assert_eq!(construct, "list of raw bits and text");
}

/// The carrier: a `bN` is held in the unsigned C cell of its own width, and
/// `b64` reads exact (it fills a `uint64_t` that no `int64_t` holds).
#[test]
fn bits_carrier_is_the_unsigned_cell() {
    assert_eq!(int_width_of_bits(BitsWidth::B8).c_type(), "uint8_t");
    assert_eq!(int_width_of_bits(BitsWidth::B16).c_type(), "uint16_t");
    assert_eq!(int_width_of_bits(BitsWidth::B32).c_type(), "uint32_t");
    assert_eq!(int_width_of_bits(BitsWidth::B64).c_type(), "uint64_t");
    assert_eq!(Ty::Bits(BitsWidth::B8).builtin_c_type(), Some("uint8_t"));
    assert_eq!(Ty::Bits(BitsWidth::B64).builtin_c_type(), Some("uint64_t"));
}
