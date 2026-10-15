//! A5/U10: `sermo<O, R>` conversation typing (DECISIONS D6.11, D6.12).
//!
//! A route served by a Faber handler in the static table types its `ad`
//! call site as `sermo<O, R>`; host routes and bare `sermo` stay
//! `sermo<valor, valor>`. The type arguments are compile-time only: the
//! finalized HIR carries the bare `sermo` carrier every backend matches.

use crate::test_support::{
    analyze_source_la, assert_accepts_la, assert_rejects_with_issue_la, entry_local_tys,
};
use radix_types::Type;

/// `saluta(textus) → numerus`: O = textus, R = numerus.
const TEXT_TO_NUM: &str = "@ ad 'x:y'\nfunctio f(textus n) → numerus { redde 1 }\n";
/// Zero-parameter handler: O = nihil, R = textus.
const NO_OPENER: &str = "@ ad 'x:z'\nfunctio g() → textus { redde \"a\" }\n";
/// Cursor handler: R = numerus, many items.
const CURSOR: &str =
    "@ ad 'x:c'\nfunctio h(textus n) fiunt → numerus {\n    cede 1\n    cede 2\n}\n";
/// Vacuum handler: R = vacuum.
const VACUUM: &str = "@ ad 'x:v'\nfunctio v(textus n) { }\n";

fn program(handlers: &str, body: &str) -> String {
    format!("{handlers}\nincipit {{\n{body}\n}}\n")
}

fn accepts(handlers: &str, body: &str) {
    assert_accepts_la(&program(handlers, body));
}

fn rejects(handlers: &str, body: &str, issue: &str) {
    assert_rejects_with_issue_la(&program(handlers, body), issue);
}

// ---- call-site typing and inference ----

#[test]
fn materialize_to_reply_type_is_accepted() {
    accepts(
        TEXT_TO_NUM,
        "sit numerus n ← ad 'x:y' (\"a\") ↦ numerus\nnota n",
    );
}

#[test]
fn materialize_to_other_type_is_a_reply_mismatch() {
    rejects(
        TEXT_TO_NUM,
        "sit textus t ← ad 'x:y' (\"a\") ↦ textus\nnota t",
        "sermo_reply_type_mismatch",
    );
}

#[test]
fn inferred_local_carries_route_types() {
    rejects(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nsit textus t ← s ↦ textus\nnota t",
        "sermo_reply_type_mismatch",
    );
    accepts(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nsit numerus n ← s ↦ numerus\nnota n",
    );
}

#[test]
fn opener_must_match_handler_parameter() {
    rejects(
        TEXT_TO_NUM,
        "sit numerus n ← ad 'x:y' (5) ↦ numerus\nnota n",
        "ad_route_opener_type_mismatch",
    );
}

#[test]
fn missing_opener_is_rejected_when_handler_takes_one() {
    rejects(
        TEXT_TO_NUM,
        "sit numerus n ← ad 'x:y' ↦ numerus\nnota n",
        "ad_route_opener_missing",
    );
}

#[test]
fn opener_is_rejected_when_handler_takes_none() {
    rejects(
        NO_OPENER,
        "sit textus t ← ad 'x:z' (\"a\") ↦ textus\nnota t",
        "ad_route_opener_unexpected",
    );
    accepts(NO_OPENER, "sit textus t ← ad 'x:z' ↦ textus\nnota t");
}

#[test]
fn cursor_handler_materializes_as_lista_of_reply() {
    accepts(
        CURSOR,
        "sit lista<numerus> xs ← ad 'x:c' (\"a\") ↦ lista<numerus>\nnota xs",
    );
    rejects(
        CURSOR,
        "sit lista<textus> xs ← ad 'x:c' (\"a\") ↦ lista<textus>\nnota xs",
        "sermo_reply_type_mismatch",
    );
}

#[test]
fn vacuum_handler_materializes_as_vacuum() {
    accepts(VACUUM, "sit vacuum u ← ad 'x:v' (\"a\") ↦ vacuum");
    rejects(
        VACUUM,
        "sit textus t ← ad 'x:v' (\"a\") ↦ textus\nnota t",
        "sermo_reply_type_mismatch",
    );
}

// ---- directional views ----

#[test]
fn tuus_type_must_match_reply() {
    rejects(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nsit t ← s.tuus<textus>()\nnota t.fini()",
        "sermo_view_type_mismatch",
    );
    accepts(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nsit t ← s.tuus<numerus>()\nnota t.fini()",
    );
}

#[test]
fn omitted_tuus_type_is_inferred_from_reply() {
    accepts(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nsit t ← s.tuus()\nsit scrinium<numerus> ∪ nihil f ← t.accipe()\nnota f",
    );
    rejects(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nsit t ← s.tuus()\nsit scrinium<textus> ∪ nihil f ← t.accipe()\nnota f",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn meus_type_must_match_opener() {
    rejects(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nnota s.meus<numerus>().fini()",
        "sermo_view_type_mismatch",
    );
}

#[test]
fn meus_close_is_legal_on_a_faber_route() {
    accepts(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nnota s.meus<textus>().fini()\nnota s.meus().fini()",
    );
}

#[test]
fn meus_send_toward_a_faber_handler_is_rejected() {
    rejects(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\ns.meus<textus>().da(\"b\")",
        "ad_handler_send_closed",
    );
    rejects(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nsit m ← s.meus<textus>()\nm.da(\"b\")",
        "ad_handler_send_closed",
    );
}

// ---- assignability and written forms ----

#[test]
fn typed_conversation_assigns_to_bare_sermo() {
    accepts(
        TEXT_TO_NUM,
        "sit sermo s ← ad 'x:y' (\"a\")\nsit textus t ← s ↦ textus\nnota t",
    );
}

#[test]
fn written_typed_form_matching_the_route_is_accepted() {
    accepts(
        TEXT_TO_NUM,
        "sit sermo<textus, numerus> s ← ad 'x:y' (\"a\")\nsit numerus n ← s ↦ numerus\nnota n",
    );
}

#[test]
fn written_typed_form_disagreeing_with_the_route_is_rejected() {
    rejects(
        TEXT_TO_NUM,
        "sit sermo<textus, textus> s ← ad 'x:y' (\"a\")\nnota s ↦ textus",
        "sermo_type_arguments_mismatch",
    );
}

#[test]
fn bare_sermo_does_not_assign_to_a_typed_form() {
    rejects(
        "",
        "sit sermo<textus, textus> s ← ad 'runtime:echo' (\"a\")\nnota s ↦ textus",
        "sermo_typed_from_untyped",
    );
}

#[test]
fn written_sermo_takes_zero_or_two_type_arguments() {
    rejects(
        "",
        "sit sermo<textus> s ← ad 'runtime:echo' (\"a\")\nnota s ↦ textus",
        "sermo_arity",
    );
}

#[test]
fn sermo_of_valor_valor_is_the_bare_form() {
    accepts(
        "",
        "sit sermo<valor, valor> s ← ad 'runtime:echo' (\"a\")\nsit sermo b ← s\nsit textus t ← b ↦ textus\nnota t",
    );
}

// ---- host routes and bare sermo keep today's meaning ----

#[test]
fn host_route_stays_valor_valor() {
    accepts(
        "",
        "sit textus t ← ad 'solum:lege' (5) ↦ textus\nsit s ← ad 'solum:lege' (\"a\")\nsit v ← s.tuus<numerus>()\nnota t\nnota v.fini()",
    );
}

#[test]
fn bare_sermo_parameter_accepts_a_typed_conversation() {
    assert_accepts_la(
        "@ ad 'x:y'\nfunctio f(textus n) → numerus { redde 1 }\n\
         functio lege(sermo s) → textus { redde s ↦ textus }\n\
         incipit {\n    nota lege(ad 'x:y' (\"a\"))\n}\n",
    );
}

// ---- erasure ----

#[test]
fn finalized_hir_carries_the_bare_sermo_carrier() {
    let result = analyze_source_la(&program(
        TEXT_TO_NUM,
        "sit s ← ad 'x:y' (\"a\")\nnota s ↦ numerus",
    ));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    let tys = entry_local_tys(&result);
    assert_eq!(tys.len(), 1);
    assert!(
        matches!(result.types.get(tys[0]), Type::Struct(_)),
        "typed sermo must erase to the bare carrier, got {:?}",
        result.types.get(tys[0])
    );
}
