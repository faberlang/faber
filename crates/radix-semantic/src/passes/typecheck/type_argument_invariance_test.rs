//! Type-argument positions are invariant (ruling 21 row 25, ruling 51 A7).
//!
//! A `list<u8>` is not a `list<int>`: if it were, the same storage could be
//! written through the `list<int>` name with 300, which the `u8` cell must
//! never hold. The store policy belongs to the destination cell, and a whole
//! container is not a read, so it needs `↦`. A container built fresh from a
//! literal is different: each element is a store into the element type.

use crate::test_support;

fn analyze_en(source: &str) -> test_support::AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    test_support::analyze_source_with_config(source, config)
}

fn accepts(source: &str) {
    let result = analyze_en(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

/// The type-mismatch issue names reported for `source`, empty when it checks.
fn mismatch_issues(source: &str) -> Vec<String> {
    let result = analyze_en(source);
    result
        .diagnostics
        .iter()
        .filter(|err| err.is_error())
        .filter_map(|err| test_support::issue(err).map(str::to_string))
        .collect()
}

fn rejects(source: &str) {
    let issues = mismatch_issues(source);
    assert!(
        issues
            .iter()
            .any(|issue| issue.ends_with("_type_mismatch")
                || issue.ends_with("_annotation_mismatch")),
        "expected a type mismatch, got {issues:?} for:\n{source}"
    );
}

#[test]
fn narrow_list_aliased_as_wide_list_is_refused() {
    // The aliasing case: both names would share the storage.
    rejects(
        "main {\n    let list<u8> small ← [1, 2]\n    mut list<int> big ← small\n    big.append(300)\n}",
    );
}

#[test]
fn wide_list_into_narrow_list_is_refused() {
    rejects("main {\n    let list<int> big ← [1, 2]\n    let list<u8> small ← big\n}");
}

#[test]
fn list_argument_of_another_width_is_refused() {
    rejects(
        "fn takes(list<int> xs) → int {\n    return xs.length()\n}\nmain {\n    let list<u8> ys ← [1, 2]\n    print takes(ys)\n}",
    );
}

#[test]
fn list_return_of_another_width_is_refused() {
    rejects("fn back(list<u8> xs) → list<int> {\n    return xs\n}\nmain { }");
}

#[test]
fn map_value_and_key_of_another_width_are_refused() {
    rejects("fn f(map<string, u8> m) → map<string, int> {\n    return m\n}\nmain { }");
    rejects("fn f(map<u8, string> m) → map<int, string> {\n    return m\n}\nmain { }");
}

#[test]
fn set_queue_stack_of_another_width_are_refused() {
    for kind in ["set", "queue", "stack"] {
        rejects(&format!(
            "fn f({kind}<u8> c) → {kind}<int> {{\n    return c\n}}\nmain {{ }}"
        ));
    }
}

#[test]
fn tensor_element_of_another_width_is_refused() {
    rejects("fn f(tensor<f32, [2]> t) → tensor<f64, [2]> {\n    return t\n}\nmain { }");
}

#[test]
fn tuple_members_of_another_width_are_refused() {
    rejects("fn f(tuple<u8, int> t) → tuple<int, int> {\n    return t\n}\nmain { }");
}

#[test]
fn function_types_of_another_width_are_refused() {
    rejects("fn narrow(u8 x) → u8 {\n    return x\n}\nmain {\n    let (int) → int f ← narrow\n}");
}

#[test]
fn same_type_arguments_are_accepted() {
    accepts(
        "fn same(list<u8> xs) → list<u8> {\n    return xs\n}\nmain {\n    let list<u8> a ← [1, 2]\n    let list<u8> b ← same(a)\n    print b\n}",
    );
}

#[test]
fn literal_elements_are_stores_into_the_element_type() {
    accepts(
        "main {\n    let list<int> xs ← [1, 2]\n    let list<u8> ys ← [1, 2]\n    let u8 one ← 1\n    let list<int> zs ← [one, 2]\n    print xs\n    print ys\n    print zs\n}",
    );
}

#[test]
fn explicit_conversion_between_element_widths_is_accepted() {
    accepts(
        "main {\n    let list<u8> small ← [1, 2]\n    let list<int> big ← small ↦ list<int>\n    print big\n}",
    );
}

#[test]
fn lifted_operator_result_stores_elementwise_into_the_declared_list() {
    // The sum is a fresh container whose elements are exact; the declared
    // `list<u16>` is the destination of each element store.
    accepts(
        "main {\n    let list<u8> a ← [200, 100]\n    let list<u8> b ← [100, 100]\n    let list<u16> wide ← a + b\n    print wide\n}",
    );
}

#[test]
fn named_list_keeps_its_element_type_at_a_call() {
    // Only a fresh result adopts the destination element type. A named
    // `list<int>` is never passed where `list<u16>` is wanted.
    rejects(
        "fn takes(list<u16> xs) → int {\n    return xs.length()\n}\nmain {\n    let list<int> a ← [1, 2]\n    print takes(a)\n}",
    );
}

#[test]
fn spread_copies_elements_as_stores() {
    accepts(
        "main {\n    let list<u8> small ← [1, 2]\n    let list<int> big ← [0, spread small]\n    print big\n}",
    );
}
