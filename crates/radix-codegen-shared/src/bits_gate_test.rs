//! The width-changing list decode gate: a list conversion with
//! an endian hint and a different element bit width is refused by name; the
//! same-width list conversion is not matched.

use super::*;
use radix_types::{BitsWidth, NumericWidth, Primitive};

fn list_of(types: &mut TypeTable, element: TypeId) -> TypeId {
    types.array(element)
}

#[test]
fn width_changing_list_decode_is_refused_by_name() {
    let mut types = TypeTable::new();
    let b8 = types.bits(BitsWidth::B8);
    let u16_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U16);
    let (source, target) = (list_of(&mut types, b8), list_of(&mut types, u16_ty));
    let error = reject_list_width_decode(&types, Some(source), target, true, "HirRust")
        .expect_err("a width-changing list decode is refused");
    assert_eq!(
        error.message,
        "the conversion `list<b8>` to `list<u16>` through a byte order (`via Le` or `via Be`) \
         is not supported on the HirRust target yet"
    );
    assert!(
        error
            .args
            .iter()
            .any(|arg| format!("{arg:?}").contains("bits_conversion_unsupported_on_target"))
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
