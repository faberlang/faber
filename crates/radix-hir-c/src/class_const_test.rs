//! Type-level constants (`const T X = e`): a genus member that belongs to
//! the type, read through the type name, never stored on an instance.

use crate::test_support::emitted;

#[test]
fn a_type_level_constant_reads_through_the_type() {
    let code = emitted(
        "class Tab {\n    const int LIMIT = 3\n    const int TWICE = 3 * 2\n}\nmain {\n    print Tab.LIMIT\n    print Tab.TWICE\n}",
    );
    assert!(
        code.contains("static const int64_t k_Tab_LIMIT __attribute__((unused)) = INT64_C(3);"),
        "{code}"
    );
    assert!(
        code.contains("static const int64_t k_Tab_TWICE __attribute__((unused)) = INT64_C(6);"),
        "{code}"
    );
    assert!(code.contains("k_Tab_LIMIT"), "{code}");
    assert!(code.contains("k_Tab_TWICE"), "{code}");
}

#[test]
fn a_text_type_level_constant_reads_through_the_type() {
    let code = emitted(
        "class Color {\n    const string RUBER = \"#ff0000\"\n}\nmain {\n    print Color.RUBER\n}",
    );
    assert!(
        code.contains(
            "static const char *const k_Color_RUBER __attribute__((unused)) = \"#ff0000\";"
        ),
        "{code}"
    );
}

#[test]
fn a_type_level_constant_is_not_an_instance_member() {
    let code = emitted(
        "class Tab {\n    const int LIMIT = 3\n    mut int w = 1\n}\nmain {\n    mut Tab t ← { w = 4 }\n    print t.w, Tab.LIMIT\n}",
    );
    assert!(!code.contains("m_LIMIT"), "{code}");
    assert!(code.contains("m_w"), "{code}");
}
