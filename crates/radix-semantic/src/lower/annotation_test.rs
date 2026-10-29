//! `@ radix typus` allowed-domain validation: a member must be a concrete
//! type. A compound member that holds `ignotum` is no more concrete than a
//! bare `ignotum`, whichever collection wraps it.

use crate::test_support::{analyze_source, assert_rejects_with_issue};

const NOT_CONCRETE: &str = "radix_typus_allowed_type_not_concrete";

/// One generic loader whose `T` domain is `textus` plus `MEMBER`.
fn domain_source(member: &str) -> String {
    let template = "\
@ radix typus T in textus MEMBER
functio load<T>(T value) → T {
    redde value
}
";
    template.replace("MEMBER", member)
}

fn assert_member_is_not_concrete(member: &str) {
    assert_rejects_with_issue(&domain_source(member), NOT_CONCRETE);
}

fn assert_member_is_concrete(member: &str) {
    let fixture = analyze_source(&domain_source(member));
    assert!(
        fixture
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.issue() != Some(NOT_CONCRETE)),
        "{member} is concrete: {:?}",
        fixture.diagnostics
    );
}

#[test]
fn lista_holding_ignotum_is_not_concrete() {
    assert_member_is_not_concrete("lista<ignotum>");
}

#[test]
fn queue_holding_ignotum_is_not_concrete() {
    assert_member_is_not_concrete("queue<ignotum>");
}

#[test]
fn stack_holding_ignotum_is_not_concrete() {
    assert_member_is_not_concrete("stack<ignotum>");
}

#[test]
fn bounded_queue_holding_ignotum_is_not_concrete() {
    assert_member_is_not_concrete("queue<ignotum, magnitudo 8>");
}

#[test]
fn bounded_stack_holding_ignotum_is_not_concrete() {
    assert_member_is_not_concrete("stack<ignotum, magnitudo 8>");
}

#[test]
fn ledger_holding_ignotum_is_not_concrete() {
    assert_member_is_not_concrete("tabulae<textus, ignotum>");
}

#[test]
fn concrete_collections_stay_admitted() {
    for member in [
        "queue<textus>",
        "stack<textus>",
        "queue<textus, magnitudo 8>",
        "stack<textus, magnitudo 8>",
        "tabulae<textus, numerus>",
    ] {
        assert_member_is_concrete(member);
    }
}
