//! Target-native imported values are `ignotum`; ordering comparisons keep
//! that unknown result until a condition observes it.
//!
//! Tests identify diagnostics by issue, never message text.

use crate::test_support;
use radix_target::Target;

#[test]
fn ordering_on_ignotum_is_admitted() {
    test_support::assert_accepts_for_target_la(
        r#"
importa python "math" m

incipit {
    sit ignotum c ← m.x ≻ 3
}
"#,
        Target::HirPython,
    );
}

#[test]
fn ordering_on_ignotum_yields_ignotum_not_bivalens() {
    test_support::assert_rejects_with_issue_for_target_la(
        r#"
importa python "math" m

incipit {
    sit lista<numerus> xs ← m.x ≻ m.y
}
"#,
        Target::HirPython,
        "import_conversion_required",
    );
}

#[test]
fn ordering_between_two_import_values_is_admitted() {
    test_support::assert_accepts_for_target_la(
        r#"
importa python "math" m

incipit {
    sit ignotum c ← m.x ≤ m.y
}
"#,
        Target::HirPython,
    );
}
