//! D8.3 closing: `sermo.close()`, the `@ radix contract "closing"` mark,
//! `sermo`'s built-in conformance, and the `closable_in_cycle` warning.
//!
//! Fixtures declare the marked contract locally (the shape norma ships).

use crate::test_support;
use crate::{SemanticErrorKind, WarningKind};

const CLOSABLE: &str = "\
@ radix contract \"closing\"
implendum Closable {
    functio close() → bivalens ⇥ textus
}

functio claude<T implet Closable>(T res) → bivalens ⇥ textus {
    redde res.close()
}

";

fn with_closable(body: &str) -> String {
    format!("{CLOSABLE}{body}")
}

fn cycle_warnings(source: &str) -> Vec<(String, String)> {
    test_support::analyze_source_la(source)
        .diagnostics
        .iter()
        .filter(|diag| diag.kind == SemanticErrorKind::Warning(WarningKind::ClosableInCycle))
        .map(|diag| {
            assert_eq!(diag.issue(), Some("closable_in_cycle"));
            let arg = |name: &str| {
                diag.args
                    .iter()
                    .find(|arg| arg.name == name)
                    .map(|arg| arg.value.clone())
                    .unwrap_or_default()
            };
            (arg("type"), arg("field"))
        })
        .collect()
}

#[test]
fn sermo_close_defaults_on_failure() {
    test_support::assert_accepts_la(
        "incipit {\n\
         sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         sit bivalens clausa ← s.close() ⊥ falsum\n\
         nota clausa\n\
         }\n",
    );
}

#[test]
fn typed_sermo_close_propagates_textus() {
    test_support::assert_accepts_la(
        "@ ad 'numeri:bis'\n\
         functio bis(numerus n) → numerus { redde n * 2 }\n\
         functio finis(sermo<numerus, numerus> s) → bivalens ⇥ textus {\n\
         redde s.close()\n\
         }\n\
         incipit {\n\
         sit _ s ← ad 'numeri:bis' (2)\n\
         nota finis(s) ⊥ falsum\n\
         }\n",
    );
}

#[test]
fn sermo_close_needs_an_error_sink() {
    test_support::assert_rejects_with_issue_la(
        // D17.8: `incipit` (main) is exempt; the rule applies in a helper.
        "functio g() → vacuum {\n\
         sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         sit bivalens clausa ← s.close()\n\
         }\n",
        "failable_call_requires_handler",
    );
}

#[test]
fn sermo_close_takes_no_arguments() {
    test_support::assert_rejects_with_issue_la(
        "incipit {\n\
         sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         sit bivalens clausa ← s.close(1) ⊥ falsum\n\
         }\n",
        "method_accepts_no_arguments",
    );
}

#[test]
fn sermo_satisfies_the_closing_contract() {
    test_support::assert_accepts_la(&with_closable(
        "incipit {\n\
         sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         nota claude(s) ⊥ falsum\n\
         }\n",
    ));
}

#[test]
fn genus_without_implet_does_not_satisfy_closing() {
    test_support::assert_rejects_with_issue_la(
        &with_closable(
            "genus Lapis {\n    varia numerus massa\n}\n\
             incipit {\n\
             nota claude(Lapis { massa = 1 }) ⊥ falsum\n\
             }\n",
        ),
        "generic_type_arg_bound",
    );
}

#[test]
fn closing_mark_requires_the_close_shape() {
    test_support::assert_rejects_with_issue_la(
        "@ radix contract \"closing\"\nimplendum Closable {\n    functio close() → vacuum\n}\n",
        "radix_contract_closing_shape",
    );
}

#[test]
fn sermo_field_on_a_cycle_warns() {
    let warnings = cycle_warnings(
        "genus Nodus {\n    varia Nodus ∪ nihil sequens\n    varia sermo canalis\n}\nincipit {\n    sit sermo s ← ad 'runtime:echo' (1)\n}\n",
    );
    assert_eq!(warnings, vec![("Nodus".to_owned(), "canalis".to_owned())]);
}

#[test]
fn sermo_list_on_a_mutual_cycle_warns() {
    let warnings = cycle_warnings(
        "genus A {\n    varia B ∪ nihil b\n    varia lista<sermo> canales\n}\n\
         genus B {\n    varia lista<A> filii\n}\nincipit {\n    sit sermo s ← ad 'runtime:echo' (1)\n}\n",
    );
    assert_eq!(warnings, vec![("A".to_owned(), "canales".to_owned())]);
}

#[test]
fn sermo_field_off_a_cycle_does_not_warn() {
    let warnings = cycle_warnings(
        "genus Piscina {\n    varia tabula<textus, sermo> canales\n}\nincipit {\n    sit sermo s ← ad 'runtime:echo' (1)\n}\n",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn closable_genus_on_a_cycle_warns() {
    let warnings = cycle_warnings(&with_closable(
        "genus Fasciculus implet Closable {\n    varia Fasciculus ∪ nihil proximus\n\n    \
         functio close() → bivalens ⇥ textus { redde verum }\n}\nincipit {}\n",
    ));
    assert_eq!(
        warnings,
        vec![("Fasciculus".to_owned(), "proximus".to_owned())]
    );
}

#[test]
fn closable_held_through_a_plain_genus_on_a_cycle_warns() {
    // A-194: `Tenens` is off the cycle, so its own `canalis` never warns; the
    // owner's field that holds a `Tenens` is the one that must.
    let warnings = cycle_warnings(
        "genus Tenens {\n    varia sermo canalis\n}\n\
         genus Nodus {\n    varia Nodus ∪ nihil sequens\n    varia Tenens t\n}\n\
         incipit {\n    sit sermo s ← ad 'runtime:echo' (1)\n}\n",
    );
    assert_eq!(warnings, vec![("Nodus".to_owned(), "t".to_owned())]);
}

#[test]
fn closable_held_through_a_struct_that_is_itself_cyclic_warns_once() {
    // The cyclic holder reports its own field; the field that merely points
    // at it does not repeat the warning.
    let warnings = cycle_warnings(
        "genus A {\n    varia B ∪ nihil b\n}\n\
         genus B {\n    varia A ∪ nihil a\n    varia sermo canalis\n}\n\
         incipit {\n    sit sermo s ← ad 'runtime:echo' (1)\n}\n",
    );
    assert_eq!(warnings, vec![("B".to_owned(), "canalis".to_owned())]);
}

#[test]
fn failable_promissum_of_a_sermo_on_a_cycle_warns() {
    // A-195: `promissum<T ⇥ E>` is `PromissumFailable`, not `Promissum`.
    let warnings = cycle_warnings(
        "genus Nodus {\n    varia Nodus ∪ nihil sequens\n    varia promissum<sermo ⇥ textus> f\n}\n\
         incipit {\n    sit sermo s ← ad 'runtime:echo' (1)\n}\n",
    );
    assert_eq!(warnings, vec![("Nodus".to_owned(), "f".to_owned())]);
}

#[test]
fn infallible_promissum_of_a_sermo_on_a_cycle_warns() {
    let warnings = cycle_warnings(
        "genus Nodus {\n    varia Nodus ∪ nihil sequens\n    varia promissum<sermo> f\n}\n\
         incipit {\n    sit sermo s ← ad 'runtime:echo' (1)\n}\n",
    );
    assert_eq!(warnings, vec![("Nodus".to_owned(), "f".to_owned())]);
}

#[test]
fn intersection_with_the_closing_contract_on_a_cycle_warns() {
    let warnings = cycle_warnings(&with_closable(
        "implendum Aliud {\n    functio aliud() → textus\n}\n\
         genus Nodus {\n    varia Nodus ∪ nihil sequens\n    varia Closable ∩ Aliud x\n}\n\
         incipit {}\n",
    ));
    assert_eq!(warnings, vec![("Nodus".to_owned(), "x".to_owned())]);
}
