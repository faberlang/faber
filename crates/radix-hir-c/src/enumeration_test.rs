//! Enumerations and tagged unions: a tagged pointer, static unit variants,
//! variant patterns, and the `ordo` conversions.

use crate::test_support::{emitted, rejected};

const COLOR: &str = "enum Color {\n    red,\n    green = 5\n}\n";
const SHAPE: &str = "union Shape {\n    Circle { int radius },\n    Nothing\n}\n";

#[test]
fn a_unit_variant_is_a_static_object_with_its_position_as_tag() {
    let code = emitted(&format!(
        "{COLOR}main {{\n    const Color c ← Color.green\n    print c\n}}"
    ));
    assert!(
        code.contains("static __attribute__((unused)) g_Color k_Color_green = { .tag = 1 };"),
        "{code}"
    );
    assert!(
        code.contains("g_Color *const l_c = (&k_Color_green);"),
        "{code}"
    );
    // A payload-less enumeration shows from a name table and takes no arena.
    assert!(code.contains("g_Color_show(l_c)"), "{code}");
    assert!(!code.contains("faber_a"), "{code}");
}

#[test]
fn a_payload_variant_is_one_allocation_with_tag_and_fields() {
    let code = emitted(&format!(
        "{SHAPE}main {{\n    const Shape s ← variant Circle {{ radius = 3 }} ∷ Shape\n    print s\n}}"
    ));
    assert!(code.contains("int64_t m_Circle_radius;"), "{code}");
    assert!(
        code.contains("faber_t0->tag = 0; faber_t0->m_Circle_radius = INT64_C(3);"),
        "{code}"
    );
    assert!(
        code.contains("\"Shape.Circle { \", \"radius = \""),
        "{code}"
    );
}

#[test]
fn a_variant_pattern_tests_the_tag_and_binds_the_payload() {
    let code = emitted(&format!(
        "{SHAPE}fn area(Shape s) → int {{\n    match s {{\n        case Circle const radius {{\n            return radius\n        }}\n        case Nothing {{\n            return 0\n        }}\n    }}\n}}\nmain {{\n    print area(variant Nothing ∷ Shape)\n}}"
    ));
    assert!(
        code.contains("(faber_c0->tag == 0)") || code.contains("->tag == 0)"),
        "{code}"
    );
    assert!(
        code.contains("= faber_c0->m_Circle_radius;") || code.contains("->m_Circle_radius;"),
        "{code}"
    );
}

#[test]
fn equality_of_payload_less_values_is_pointer_equality() {
    let code = emitted(&format!(
        "{COLOR}main {{\n    const bool b ← Color.red ≡ Color.green\n    print b\n}}"
    ));
    assert!(
        code.contains("(&k_Color_red) == (&k_Color_green)") || code.contains("== (&k_Color_green)"),
        "{code}"
    );
}

#[test]
fn equality_of_payload_values_is_a_generated_function() {
    let code = emitted(&format!(
        "{SHAPE}main {{\n    const Shape a ← variant Nothing ∷ Shape\n    const bool b ← a ≡ a\n    print b\n}}"
    ));
    assert!(code.contains("g_Shape_equal("), "{code}");
    assert!(
        code.contains("if (a->tag != b->tag) { return false; }"),
        "{code}"
    );
}

#[test]
fn ordo_conversions_use_the_route_discriminants() {
    let code = emitted(&format!(
        "{COLOR}main {{\n    print Color.green ↦ int\n    const Color c ← 5 ↦ Color ⊥ Color.red\n    print c ↦ string\n}}"
    ));
    assert!(code.contains("INT64_C(5)"), "{code}");
    assert!(
        code.contains("faber_abort") || code.contains("k_Color_green"),
        "{code}"
    );
}

#[test]
fn a_union_over_a_generic_parameter_is_rejected_by_name() {
    assert_eq!(
        rejected("union Box<T> {\n    Full { T value },\n    Empty\n}\nmain {\n    print \"x\"\n}"),
        "generic union"
    );
}

#[test]
fn a_union_with_shared_fields_is_rejected_by_name() {
    assert_eq!(
        rejected(
            "union Event {\n    @ shared\n    string message\n\n    Click,\n    Key { int code }\n}\nmain {\n    print \"x\"\n}"
        ),
        "union with shared (commune) fields"
    );
}

#[test]
fn a_genus_with_no_fields_shows_as_empty_braces() {
    let code = emitted("class Marker {\n}\nmain {\n    const Marker m ← Marker {}\n    print m\n}");
    assert!(code.contains("return \"Marker {}\";"), "{code}");
}

#[test]
fn a_statement_with_a_genus_argument_prints_one_line_each() {
    let code = emitted(
        "class Marker {\n}\nmain {\n    const Marker m ← Marker {}\n    const int n ← 1\n    print n, m\n}",
    );
    assert_eq!(code.matches("printf(").count(), 2, "{code}");
}

#[test]
fn a_statement_of_sized_integers_prints_one_joined_line() {
    let code = emitted("main {\n    const u8 a ← 200\n    const i16 b ← -5000\n    print a, b\n}");
    assert_eq!(code.matches("printf(").count(), 1, "{code}");
}
