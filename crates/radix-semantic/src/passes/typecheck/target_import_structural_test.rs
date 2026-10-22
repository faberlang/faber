//! Target-native import stores into aggregate cells: the STRUCTURAL DOOR law
//! says an `ignotum` value arriving through a target import may not be stored
//! into an aggregate cell without an authored conversion. The refusal is named
//! `import_conversion_required`; a forced `↦` conversion lands the store.
//!
//! Tests assert issue identity, never message text.

use crate::test_support;
use radix_target::Target;

#[test]
fn ignotum_into_lista_cell_is_refused_by_name() {
    test_support::assert_rejects_with_issue_for_target_la(
        r#"
importa python "math" m

incipit {
    sit lista<numerus> xs ← m.arange(5)
}
"#,
        Target::HirPython,
        "import_conversion_required",
    );
}

#[test]
fn ignotum_into_tensor_cell_is_refused_by_name() {
    test_support::assert_rejects_with_issue_for_target_la(
        r#"
importa python "math" m

incipit {
    sit tensor<f32, [4]> t ← m.zeros(4)
}
"#,
        Target::HirPython,
        "import_conversion_required",
    );
}

#[test]
fn ignotum_into_map_cell_is_refused_by_name() {
    test_support::assert_rejects_with_issue_for_target_la(
        r#"
importa python "math" m

incipit {
    sit tabula<textus, numerus> t ← m.sizes
}
"#,
        Target::HirPython,
        "import_conversion_required",
    );
}

#[test]
fn forced_conversion_lands_the_trust_transfer() {
    test_support::assert_accepts_for_target_la(
        r#"
importa python "math" m

incipit {
    sit lista<numerus> xs ← m.arange(5) ↦ lista<numerus>
}
"#,
        Target::HirPython,
    );
}
