//! Tests for the tensor element type conversion lattice.
//!
//! These tests prove that pairwise conversion rules between element types
//! are stable, symmetrical, and reject invalid family-width combinations.

use super::tensor_element_lattice::{conversio_allowed, lattice_pair_counts, SPECS};
use super::TypeTable;

#[test]
fn tensor_element_lattice_pair_counts_are_stable() {
    let mut types = TypeTable::new();
    let (allowed, rejected, emits_convert) = lattice_pair_counts(&mut types);
    let total = allowed + rejected;
    assert_eq!(total, SPECS.len().pow(2));
    // WHY: golden counts document the v1 lattice surface; update only on deliberate policy change.
    assert_eq!(allowed, 56, "allowed lattice pairs");
    assert_eq!(rejected, 44, "rejected lattice pairs");
    assert_eq!(emits_convert, 46, "pairs that emit convert_elements");
}

#[test]
fn tensor_element_lattice_rejects_cross_family_unsigned_to_signed() {
    let mut types = TypeTable::new();
    let u8 = SPECS
        .iter()
        .find(|spec| spec.label == "numerus_u8")
        .expect("u8 spec");
    let i8 = SPECS
        .iter()
        .find(|spec| spec.label == "numerus_i8")
        .expect("i8 spec");
    assert!(!conversio_allowed(&mut types, u8, i8));
    assert!(!conversio_allowed(&mut types, i8, u8));
}
