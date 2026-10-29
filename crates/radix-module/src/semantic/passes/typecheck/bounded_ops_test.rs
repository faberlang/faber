//! BB-8R pins: shared ops, prefix equality, and overflow alternate exit
//! across `lista<T, magnitudo N>`, `textus<N>`, `ascii<N>`, and `octeti<N>`.

use super::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";
const FAILABLE_ISSUE: &str = "failable_call_requires_handler";

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_ops_longitudo_and_vacua_are_len_not_capacity() {
    assert_accepts_la(
        r"functio probe(
    lista<numerus, magnitudo 8> xs,
    textus<magnitudo 8> t,
    ascii<magnitudo 8> a,
    lista<b8, magnitudo 8> o
) → vacuum {
    sit numerus n_xs ← xs.longitudo()
    sit bivalens e_xs ← xs.vacua()
    sit numerus n_t ← t.longitudo()
    sit bivalens e_t ← t.vacua()
    sit numerus n_a ← a.longitudo()
    sit bivalens e_a ← a.vacua()
    sit numerus n_o ← o.longitudo()
    sit bivalens e_o ← o.vacua()
    nota n_xs
    nota e_xs
    nota n_t
    nota e_t
    nota n_a
    nota e_a
    nota n_o
    nota e_o
}

incipit {
    nota 0
}",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_ops_accipe_and_sectio_typecheck() {
    // need Q3 (operator 2026-10-17): list `sectio` (slice) reports
    // on the error channel the way `at` does, so a helper calling it needs
    // `⇥ textus`; textus/ascii `sectio` keep trimming and need no handler.
    assert_accepts_la(
        r"functio probe(
    lista<numerus, magnitudo 8> xs,
    textus<magnitudo 8> t,
    ascii<magnitudo 8> a,
    lista<b8, magnitudo 8> o
) → vacuum ⇥ textus {
    sit numerus ∪ nihil got ← xs.accipe(0)
    sit lista<numerus> slice ← xs.sectio(0, 2)
    nota got
    nota slice
    nota t.accipe(0)
    nota t.sectio(0, 2)
    nota a.accipe(0)
    nota a.sectio(0, 2)
    nota o.accipe(0)
    nota o.sectio(0, 2)
}

incipit {
    nota 0
}",
    );
    // The ruling's SEM011 pin: the same list `sectio` in a helper without
    // an error channel is rejected.
    assert_rejects_with_issue_la(
        r"functio probe(lista<numerus, magnitudo 8> xs) → vacuum {
    nota xs.sectio(0, 2)
}

incipit {
    nota 0
}",
        FAILABLE_ISSUE,
    );
}

#[test]
fn bounded_ops_appende_requires_alternate_exit() {
    assert_rejects_with_issue_la(
        // D17.8: `incipit` (main) is exempt; the rule applies in a helper.
        r"functio grow() → vacuum {
    varia lista<numerus, magnitudo 8> xs ← vacua
    xs.appende(1)
}",
        FAILABLE_ISSUE,
    );
    assert_rejects_with_issue_la(
        r#"functio grow() → vacuum {
    varia textus raw ← "ab"
    varia textus<magnitudo 8> t ← raw ↦ textus<magnitudo 8>
    t.appende('c')
}

incipit {
    nota 0
}"#,
        FAILABLE_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio grow() → vacuum {
    varia ascii<magnitudo 8> a ← 'ab'
    a.appende('c')
}

incipit {
    nota 0
}",
        FAILABLE_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio grow() → vacuum {
    varia lista<b8> raw ← |61|
    varia lista<b8, magnitudo 8> o ← raw ↦ lista<b8, magnitudo 8>
    o.appende(0x62 ∷ u8)
}

incipit {
    nota 0
}",
        FAILABLE_ISSUE,
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_ops_appende_propagates_on_textus_channel() {
    assert_accepts_la(
        r#"functio grow_lista() → vacuum ⇥ textus {
    varia lista<numerus, magnitudo 8> xs ← vacua
    xs.appende(1)
}

functio grow_textus() → vacuum ⇥ textus {
    varia textus raw ← "ab"
    varia textus<magnitudo 8> t ← raw ↦ textus<magnitudo 8>
    t.appende('c')
}

functio grow_ascii() → vacuum ⇥ textus {
    varia ascii<magnitudo 8> a ← 'ab'
    a.appende('c')
}

functio grow_octeti() → vacuum ⇥ textus {
    varia lista<b8> raw ← |61|
    varia lista<b8, magnitudo 8> o ← raw ↦ lista<b8, magnitudo 8>
    o.appende(0x62 ∷ u8)
}

incipit {
    nota 0
}"#,
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_ops_equality_ignores_spare_slots() {
    // Same initialized prefix, independently converted so spare `len..N` is
    // unread (carrier PartialEq is prefix-only). Typecheck accepts `≡`.
    assert_accepts_la(
        r"incipit {
    sit lista<numerus> raw ← [1, 2]
    sit lista<numerus, magnitudo 8> a ← raw ↦ lista<numerus, magnitudo 8>
    sit lista<numerus, magnitudo 8> b ← raw ↦ lista<numerus, magnitudo 8>
    si a ≡ b { nota 1 }
}",
    );
    assert_accepts_la(
        r#"incipit {
    sit textus raw ← "ab"
    sit textus<magnitudo 8> a ← raw ↦ textus<magnitudo 8>
    sit textus<magnitudo 8> b ← raw ↦ textus<magnitudo 8>
    si a ≡ b { nota 1 }
}"#,
    );
    assert_accepts_la(
        r"incipit {
    sit ascii<magnitudo 8> a ← 'ab'
    sit ascii<magnitudo 8> b ← 'ab'
    si a ≡ b { nota 1 }
}",
    );
    assert_accepts_la(
        r"incipit {
    sit lista<b8> raw ← |61 62|
    sit lista<b8, magnitudo 8> a ← raw ↦ lista<b8, magnitudo 8>
    sit lista<b8, magnitudo 8> b ← raw ↦ lista<b8, magnitudo 8>
    si a ≡ b { nota 1 }
}",
    );
}

#[test]
fn bounded_ops_equality_stays_same_n() {
    assert_rejects_with_issue_la(
        r"functio same(lista<numerus, magnitudo 8> a, lista<numerus, magnitudo 4> b) → bivalens {
    redde a ≡ b
}

incipit {
    nota 0
}",
        EXACT_EQ_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio same(textus<magnitudo 8> a, textus<magnitudo 4> b) → bivalens {
    redde a ≡ b
}

incipit {
    nota 0
}",
        EXACT_EQ_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio same(ascii<magnitudo 8> a, ascii<magnitudo 4> b) → bivalens {
    redde a ≡ b
}

incipit {
    nota 0
}",
        EXACT_EQ_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio same(lista<b8, magnitudo 8> a, lista<b8, magnitudo 4> b) → bivalens {
    redde a ≡ b
}

incipit {
    nota 0
}",
        EXACT_EQ_ISSUE,
    );
}

#[test]
fn bounded_ops_do_not_invent_capacitas() {
    // Unknown methods on intrinsic receivers infer rather than error. The pin
    // is that `capacitas` is not a registered op on any family in this unit.
    assert!(radix_intrinsics::lookup_method(radix_intrinsics::IntrinsicReceiver::Lista, "capacitas", 0).is_none());
    assert!(radix_intrinsics::lookup_method(radix_intrinsics::IntrinsicReceiver::Textus, "capacitas", 0).is_none());
    assert!(radix_intrinsics::lookup_method(radix_intrinsics::IntrinsicReceiver::Ascii, "capacitas", 0).is_none());
}
