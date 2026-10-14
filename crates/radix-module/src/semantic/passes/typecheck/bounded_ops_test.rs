//! BB-8R pins: shared ops, prefix equality, and overflow alternate exit
//! across `lista<T, N>`, `textus<N>`, `ascii<N>`, and `octeti<N>`.

use super::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";
const FAILABLE_ISSUE: &str = "failable_call_requires_handler";

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_ops_longitudo_and_vacua_are_len_not_capacity() {
    assert_accepts_la(
        r"functio probe(
    lista<numerus, 8> xs,
    textus<8> t,
    ascii<8> a,
    lista<b8, 8> o
) → vacuum {
    fixum numerus n_xs ← xs.longitudo()
    fixum bivalens e_xs ← xs.vacua()
    fixum numerus n_t ← t.longitudo()
    fixum bivalens e_t ← t.vacua()
    fixum numerus n_a ← a.longitudo()
    fixum bivalens e_a ← a.vacua()
    fixum numerus n_o ← o.longitudo()
    fixum bivalens e_o ← o.vacua()
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
    assert_accepts_la(
        r"functio probe(
    lista<numerus, 8> xs,
    textus<8> t,
    ascii<8> a,
    lista<b8, 8> o
) → vacuum {
    fixum numerus ∪ nihil got ← xs.accipe(0)
    fixum lista<numerus> slice ← xs.sectio(0, 2)
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
}

#[test]
fn bounded_ops_appende_requires_alternate_exit() {
    assert_rejects_with_issue_la(
        // D17.8: `incipit` (main) is exempt; the rule applies in a helper.
        r"functio grow() → vacuum {
    varia lista<numerus, 8> xs ← vacua
    xs.appende(1)
}",
        FAILABLE_ISSUE,
    );
    assert_rejects_with_issue_la(
        r#"functio grow() → vacuum {
    varia textus raw ← "ab"
    varia textus<8> t ← raw ↦ textus<8>
    t.appende('c')
}

incipit {
    nota 0
}"#,
        FAILABLE_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio grow() → vacuum {
    varia ascii<8> a ← 'ab'
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
    varia lista<b8, 8> o ← raw ↦ lista<b8, 8>
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
    varia lista<numerus, 8> xs ← vacua
    xs.appende(1)
}

functio grow_textus() → vacuum ⇥ textus {
    varia textus raw ← "ab"
    varia textus<8> t ← raw ↦ textus<8>
    t.appende('c')
}

functio grow_ascii() → vacuum ⇥ textus {
    varia ascii<8> a ← 'ab'
    a.appende('c')
}

functio grow_octeti() → vacuum ⇥ textus {
    varia lista<b8> raw ← |61|
    varia lista<b8, 8> o ← raw ↦ lista<b8, 8>
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
    fixum lista<numerus> raw ← [1, 2]
    fixum lista<numerus, 8> a ← raw ↦ lista<numerus, 8>
    fixum lista<numerus, 8> b ← raw ↦ lista<numerus, 8>
    si a ≡ b { nota 1 }
}",
    );
    assert_accepts_la(
        r#"incipit {
    fixum textus raw ← "ab"
    fixum textus<8> a ← raw ↦ textus<8>
    fixum textus<8> b ← raw ↦ textus<8>
    si a ≡ b { nota 1 }
}"#,
    );
    assert_accepts_la(
        r"incipit {
    fixum ascii<8> a ← 'ab'
    fixum ascii<8> b ← 'ab'
    si a ≡ b { nota 1 }
}",
    );
    assert_accepts_la(
        r"incipit {
    fixum lista<b8> raw ← |61 62|
    fixum lista<b8, 8> a ← raw ↦ lista<b8, 8>
    fixum lista<b8, 8> b ← raw ↦ lista<b8, 8>
    si a ≡ b { nota 1 }
}",
    );
}

#[test]
fn bounded_ops_equality_stays_same_n() {
    assert_rejects_with_issue_la(
        r"functio same(lista<numerus, 8> a, lista<numerus, 4> b) → bivalens {
    redde a ≡ b
}

incipit {
    nota 0
}",
        EXACT_EQ_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio same(textus<8> a, textus<4> b) → bivalens {
    redde a ≡ b
}

incipit {
    nota 0
}",
        EXACT_EQ_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio same(ascii<8> a, ascii<4> b) → bivalens {
    redde a ≡ b
}

incipit {
    nota 0
}",
        EXACT_EQ_ISSUE,
    );
    assert_rejects_with_issue_la(
        r"functio same(lista<b8, 8> a, lista<b8, 4> b) → bivalens {
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
