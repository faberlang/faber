//! GPD-4: named type applications use the declaration's ordered defaults.

use crate::test_support;

#[test]
fn zero_argument_named_types_apply_trailing_defaults() {
    test_support::assert_accepts(
        r#"typus Alias<T = textus> = T

genus Box<T = textus> {
    varia T value
}

discretio Maybe<T = textus> {
    Some { T value }
}

implendum Contract<T = textus> {
    functio read(T value) → textus
}

functio use_alias(Alias value) → textus {
    redde value
}

functio make_box() → Box {
    redde Box { value = "salve" }
}

functio make_maybe() → Maybe {
    redde finge Some { value = "salve" } ∷ Maybe
}

functio use_contract(Contract value) → numerus {
    redde 1
}"#,
    );
}

#[test]
fn explicit_prefix_and_full_named_type_arguments_share_the_descriptor_law() {
    test_support::assert_accepts(
        r"genus Pair<T, U = textus> {
    varia T first
    varia U second
}

functio prefix(Pair<numerus> value) → numerus {
    redde 1
}

functio full(Pair<numerus, textus> value) → numerus {
    redde 1
}

genus Sized<magnitudo N = 4> {
    varia vector<f32, magnitudo N> lanes
}

functio sized_default(Sized value) → numerus {
    redde 1
}

functio sized_explicit(Sized<8> value) → numerus {
    redde 1
}",
    );
}

#[test]
fn named_type_application_rejects_wrong_kind() {
    test_support::assert_rejects_with_issue(
        r"genus Box<T = textus> {
    varia T value
}

functio use(Box<4> value) → numerus {
    redde 1
}",
        "generic_argument_kind_mismatch",
    );
}

#[test]
fn named_type_application_rejects_excess_arguments() {
    test_support::assert_rejects_with_issue(
        r"genus Box<T = textus> {
    varia T value
}

functio use(Box<numerus, textus> value) → numerus {
    redde 1
}",
        "generic_argument_arity_mismatch",
    );
}

#[test]
fn named_type_application_rejects_missing_required_arguments() {
    test_support::assert_rejects_with_issue(
        r"genus Pair<T, U = textus> {
    varia T first
    varia U second
}

functio use(Pair value) → numerus {
    redde 1
}",
        "generic_argument_arity_mismatch",
    );
}
