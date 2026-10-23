//! Raw bits (`b8 b16 b32 b64`) in the C leaf: the class is carried as the
//! unsigned C cell of its width, and every surface the leaf does not yet
//! carry is refused by name rather than emitted through a promoted integer
//! carrier, which would be a wrong program (operator ruling 2026-10-10:
//! `bits have their width always`).

use crate::error::UNSUPPORTED_ISSUE;
use crate::test_support::{inline, rejected_construct};
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

/// A raw-bits operand refuses every operator by name. Authored `bN` cannot
/// reach this leaf yet (the module gate keeps target c out until unit
/// 3e80d069-1.1, and the front end refuses bits arithmetic first with SEM011
/// `bits_arithmetic_refused`), so the leaf's guard is pinned at the class it
/// reads: an operand whose type classifies to `Ty::Bits`.
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

/// A `list<bN>` is refused by name. `bytes` (`octeti`) is the seeded
/// `lista<b8>`, so it reaches this leaf through an authored word and pins the
/// array arm's refusal without the authored-marker gate.
#[test]
fn bits_list_refused_by_name() {
    let source = inline("main {\n    let bytes b ← |de ad be ef|\n    print b\n}");
    assert_eq!(rejected_construct("case.fab", &source), "list of raw bits");
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
