//! Need: inside a genus method the genus's own type parameter is as
//! rigid as a free generic function's, so a `T ∪ nihil` stored into a `T` slot
//! (a list element, a field, a return) is `SEM010 optional_into_plain_cell` in
//! both. A callee signature that mentions the genus's `T` (`lista<T>.appende`)
//! once instantiated a fresh inference variable for it, which swallowed the
//! optional silently.
//!
//! Fixtures use the latin spellings (the unit-test harness has no en pack).

use crate::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

const ISSUE: &str = "optional_into_plain_cell";

#[test]
fn free_generic_function_push_of_optional_read_is_rejected() {
    assert_rejects_with_issue_la(
        r"functio f<T>(lista<T> xs) → lista<T> {
    varia lista<T> out ← vacua
    out.appende(xs.accipe(0))
    redde out
}",
        ISSUE,
    );
}

#[test]
fn genus_method_push_of_optional_read_is_rejected() {
    // The Norma deque `to_list` shape.
    assert_rejects_with_issue_la(
        r"genus Box<T> {
    varia lista<T> head = vacua

    functio copia() → lista<T> {
        varia lista<T> out ← vacua
        out.appende(ego.head.accipe(0))
        redde out
    }
}",
        ISSUE,
    );
}

#[test]
fn genus_method_push_of_optional_parameter_is_rejected() {
    assert_rejects_with_issue_la(
        r"genus Box<T> {
    varia lista<T> head = vacua

    functio adde(T ∪ nihil x) {
        ego.head.appende(x)
    }
}",
        ISSUE,
    );
}

#[test]
fn genus_method_return_of_optional_is_rejected() {
    assert_rejects_with_issue_la(
        r"genus Box<T> {
    varia lista<T> head = vacua

    functio prima() → T {
        redde ego.head.accipe(0)
    }
}",
        ISSUE,
    );
}

#[test]
fn genus_method_field_store_of_optional_is_rejected() {
    assert_rejects_with_issue_la(
        r"genus Box<T> {
    varia T slot

    functio pone(T ∪ nihil x) {
        ego.slot ← x
    }
}",
        ISSUE,
    );
}

#[test]
fn generic_method_inside_generic_genus_push_is_rejected() {
    // Nested: the method's own `U` and the genus's `T` are both rigid.
    assert_rejects_with_issue_la(
        r"genus Box<T> {
    functio mixta<U>(U u, T ∪ nihil t) → lista<T> {
        varia lista<T> out ← vacua
        out.appende(t)
        redde out
    }
}",
        ISSUE,
    );
}

#[test]
fn generic_function_over_generic_genus_push_is_rejected() {
    assert_rejects_with_issue_la(
        r"genus Box<T> {
    varia lista<T> head = vacua
}

functio f<T>(Box<T> b) → lista<T> {
    varia lista<T> out ← vacua
    out.appende(b.head.accipe(0))
    redde out
}",
        ISSUE,
    );
}

#[test]
fn genus_method_coalesced_push_is_accepted() {
    assert_accepts_la(
        r"genus Box<T> {
    varia lista<T> head = vacua

    functio copia(T ceterum) → lista<T> {
        varia lista<T> out ← vacua
        out.appende(ego.head.accipe(0) vel ceterum)
        redde out
    }
}",
    );
}

#[test]
fn genus_method_index_read_push_is_accepted() {
    assert_accepts_la(
        r"genus Box<T> {
    varia lista<T> head = vacua

    functio copia() → lista<T> {
        varia lista<T> out ← vacua
        out.appende(ego.head[0])
        redde out
    }
}",
    );
}

#[test]
fn genus_method_push_of_optional_into_optional_list_is_accepted() {
    assert_accepts_la(
        r"genus Box<T> {
    varia lista<T ∪ nihil> head = vacua

    functio adde(T ∪ nihil x) {
        ego.head.appende(x)
    }
}",
    );
}
