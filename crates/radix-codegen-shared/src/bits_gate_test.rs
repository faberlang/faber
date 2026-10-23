//! The width-changing list decode gate: a list conversion with
//! an endian hint and a different element bit width is refused by name; the
//! same-width list conversion is not matched.

use super::*;
use radix_types::{BitsWidth, NumericWidth, Primitive};

fn list_of(types: &mut TypeTable, element: TypeId) -> TypeId {
    types.array(element)
}

#[test]
fn raw_bits_are_admitted_on_the_c_target() {
    let mut types = TypeTable::new();
    let _b8 = types.bits(BitsWidth::B8);

    assert!(reject_bits_type(&types, Target::HirC).is_ok());

    let error =
        reject_bits_type(&types, Target::HirFhir).expect_err("FHIR has no raw-bits carrier");
    assert!(
        error
            .args
            .iter()
            .any(|arg| { arg.name == "issue" && arg.value == "bits_type_unsupported_on_target" })
    );
}

#[test]
fn width_changing_list_decode_is_refused_by_name() {
    let mut types = TypeTable::new();
    let b8 = types.bits(BitsWidth::B8);
    let u16_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U16);
    let (source, target) = (list_of(&mut types, b8), list_of(&mut types, u16_ty));
    let error = reject_list_width_decode(&types, Some(source), target, true, "HirRust")
        .expect_err("a width-changing list decode is refused");
    let args = error
        .args
        .iter()
        .map(|arg| (arg.name, arg.value.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        [
            ("issue", "bits_conversion_unsupported_on_target"),
            ("from", "list<b8>"),
            ("to", "list<u16>"),
            ("target", "HirRust"),
        ],
        "{error:?}"
    );
}

#[test]
fn same_width_list_conversion_is_not_matched() {
    let mut types = TypeTable::new();
    let b8 = types.bits(BitsWidth::B8);
    let u8_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let (source, target) = (list_of(&mut types, b8), list_of(&mut types, u8_ty));
    assert!(list_width_change(&types, source, target).is_none());
    assert!(reject_list_width_decode(&types, Some(source), target, false, "HirRust").is_ok());
}

#[test]
fn list_text_conversion_matches_both_directions_of_bits_and_text() {
    let mut types = TypeTable::new();
    let text = types.primitive(Primitive::Textus);
    let b8 = types.bits(BitsWidth::B8);
    let b32 = types.bits(BitsWidth::B32);
    let (bytes, words) = (list_of(&mut types, b8), list_of(&mut types, b32));
    assert_eq!(
        list_text_conversion(&types, bytes, text),
        Some(("list<b8>".to_owned(), "string".to_owned()))
    );
    assert_eq!(
        list_text_conversion(&types, text, words),
        Some(("string".to_owned(), "list<b32>".to_owned()))
    );
    let error = reject_list_text_conversion(&types, Some(bytes), text, "HirGo")
        .expect_err("the text decode is refused");
    let args = error
        .args
        .iter()
        .map(|arg| (arg.name, arg.value.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        [
            ("issue", "bits_conversion_unsupported_on_target"),
            ("from", "list<b8>"),
            ("to", "string"),
            ("target", "HirGo"),
        ],
        "{error:?}"
    );
}

#[test]
fn list_text_conversion_leaves_other_pairs_alone() {
    let mut types = TypeTable::new();
    let text = types.primitive(Primitive::Textus);
    let b8 = types.bits(BitsWidth::B8);
    let u8_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let (bits, bytes) = (list_of(&mut types, b8), list_of(&mut types, u8_ty));
    assert!(list_text_conversion(&types, bytes, text).is_none());
    assert!(list_text_conversion(&types, text, bytes).is_none());
    assert!(list_text_conversion(&types, bits, bytes).is_none());
    assert!(list_text_conversion(&types, text, text).is_none());
    assert!(reject_list_text_conversion(&types, Some(bytes), text, "HirGo").is_ok());
    assert!(reject_list_text_conversion(&types, None, text, "HirGo").is_ok());
}

#[test]
fn byte_text_conversion_is_the_eight_bit_pair_in_both_directions() {
    let mut types = TypeTable::new();
    let text = types.primitive(Primitive::Textus);
    let (b8, b16) = (types.bits(BitsWidth::B8), types.bits(BitsWidth::B16));
    let (bytes, units) = (list_of(&mut types, b8), list_of(&mut types, b16));
    assert!(is_byte_text_conversion(&types, bytes, text));
    assert!(is_byte_text_conversion(&types, text, bytes));
    assert!(!is_byte_text_conversion(&types, units, text));
    assert!(!is_byte_text_conversion(&types, text, units));
    assert!(!is_byte_text_conversion(&types, bytes, bytes));
}
