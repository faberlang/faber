//! `copy` over a value that holds something non-duplicable (ruling 7b25889f
//! R5): the refusal is SEM066 `copy_nonduplicable_member`, transitive through
//! fields, collection elements and unions, and names the member path.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack; `sermo` is the Latin carrier
//! name in every locale.

use crate::SemanticErrorKind;
use crate::test_support;

const ISSUE: &str = "copy_nonduplicable_member";

fn copy_nonduplicable_args(source: &str) -> Vec<Vec<(String, String)>> {
    test_support::analyze_source_la(source)
        .diagnostics
        .iter()
        .filter(|err| {
            err.is_error()
                && matches!(err.kind, SemanticErrorKind::CopyNonduplicable)
                && test_support::issue(err) == Some(ISSUE)
        })
        .map(|err| {
            err.args
                .iter()
                .map(|arg| (arg.name.to_owned(), arg.value.clone()))
                .collect()
        })
        .collect()
}

fn assert_one_rejection_with(source: &str, expected: &[(&str, &str)]) {
    let found = copy_nonduplicable_args(source);
    assert!(
        !found.is_empty(),
        "expected SEM066 {ISSUE}, got no rejection in {:?}",
        test_support::analyze_source_la(source).diagnostics
    );
    assert_eq!(
        found.len(),
        1,
        "expected exactly one refusal, got {found:?}"
    );
    for (name, value) in expected {
        let arg = found[0]
            .iter()
            .find(|(arg_name, _)| arg_name == name)
            .unwrap_or_else(|| panic!("expected arg {name:?} in {found:?}"));
        assert_eq!(arg.0, *name, "arg {name:?} name mismatch");
        assert_eq!(arg.1, *value, "arg {name:?} value mismatch");
    }
}

fn arg<'a>(args: &'a [(String, String)], name: &str) -> Option<&'a str> {
    args.iter()
        .find(|(arg_name, _)| arg_name == name)
        .map(|(_, value)| value.as_str())
}

#[test]
fn copy_of_genus_holding_sermo_names_the_member_path() {
    assert_one_rejection_with(
        "genus Capsa {\n\
         \x20   varia numerus n = 0\n\
         \x20   varia sermo s\n\
         }\n\
         incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   sit Capsa p ← Capsa { n = 1, s = s }\n\
         \x20   varia Capsa q ← copy p\n\
         \x20   nota q.n\n\
         }\n",
        &[("path", "Capsa.s"), ("head", "sermo")],
    );
}

#[test]
fn copy_of_sermo_itself_is_refused_without_a_path() {
    let found = copy_nonduplicable_args(
        "incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   varia sermo t ← copy s\n\
         \x20   nota t\n\
         }\n",
    );
    assert_eq!(found.len(), 1, "exactly one refusal, got {found:?}");
    assert_eq!(arg(&found[0], "head"), Some("sermo"));
    assert_eq!(arg(&found[0], "path"), None, "no member path");
}

#[test]
fn copy_through_a_nested_field_names_the_whole_path() {
    assert_one_rejection_with(
        "genus Intus {\n\
         \x20   varia sermo vinculum\n\
         }\n\
         genus Foris {\n\
         \x20   varia numerus n = 0\n\
         \x20   varia Intus intus\n\
         }\n\
         incipit {\n\
         \x20   sit Foris p ← Foris { n = 1, intus = Intus { vinculum = ad 'runtime:echo' (\"x\") } }\n\
         \x20   varia Foris q ← copy p\n\
         \x20   nota q.n\n\
         }\n",
        &[("path", "Foris.intus.vinculum"), ("head", "sermo")],
    );
}

#[test]
fn copy_through_a_list_element_names_the_element_path() {
    assert_one_rejection_with(
        "genus Intus {\n\
         \x20   varia sermo vinculum\n\
         }\n\
         genus Horreum {\n\
         \x20   varia lista<Intus> vincula = []\n\
         }\n\
         incipit {\n\
         \x20   sit Horreum h ← Horreum { vincula = [Intus { vinculum = ad 'runtime:echo' (\"x\") }] }\n\
         \x20   varia Horreum g ← copy h\n\
         \x20   nota g.vincula.longitudo()\n\
         }\n",
        &[("path", "Horreum.vincula[…].vinculum"), ("head", "sermo")],
    );
}

#[test]
fn copy_through_a_generic_genus_field_uses_its_instantiated_type() {
    assert_one_rejection_with(
        "genus Capsa<T> {\n\
         \x20   varia T res\n\
         }\n\
         incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   sit Capsa<sermo> p ← Capsa<sermo> { res = s }\n\
         \x20   varia Capsa<sermo> q ← copy p\n\
         \x20   nota q.res\n\
         }\n",
        &[("path", "Capsa.res"), ("head", "sermo")],
    );
}

#[test]
fn copy_through_a_map_value_names_the_element_path() {
    assert_one_rejection_with(
        "incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   varia tabula<textus, sermo> m ← {}\n\
         \x20   varia tabula<textus, sermo> n ← copy m\n\
         \x20   nota n.longitudo()\n\
         }\n",
        &[("path", "[…]"), ("head", "sermo")],
    );
}

#[test]
fn copy_through_an_optional_member_is_refused() {
    assert_one_rejection_with(
        "genus Fortuna {\n\
         \x20   varia sermo ∪ nihil forsitan\n\
         }\n\
         incipit {\n\
         \x20   sit Fortuna q ← Fortuna { forsitan = ad 'runtime:echo' (\"x\") }\n\
         \x20   varia Fortuna r ← copy q\n\
         \x20   nota r.forsitan\n\
         }\n",
        &[("path", "Fortuna.forsitan[…]"), ("head", "sermo")],
    );
}

#[test]
fn own_of_sermo_stays_legal() {
    test_support::assert_accepts_la(
        "functio vora(own sermo s) → numerus {\n\
         \x20   redde 1\n\
         }\n\
         incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   nota vora(own s)\n\
         \x20   sit sermo s2 ← ad 'runtime:echo' (\"y\")\n\
         \x20   varia sermo taken ← own s2\n\
         \x20   nota taken\n\
         }\n",
    );
}

#[test]
fn copy_of_ordinary_containers_is_unchanged() {
    test_support::assert_accepts_la(
        "genus Capsa {\n\
         \x20   varia fractus x = 0.0\n\
         \x20   varia lista<numerus> notae = []\n\
         }\n\
         incipit {\n\
         \x20   sit Capsa a ← Capsa { x = 1.0, notae = [1] }\n\
         \x20   varia Capsa b ← copy a\n\
         \x20   b.x ← 5.0\n\
         \x20   b.notae.appende(2)\n\
         \x20   nota a.x\n\
         \x20   nota a.notae.longitudo()\n\
         \x20   varia tabula<textus, numerus> ages ← {\"a\": 1}\n\
         \x20   varia tabula<textus, numerus> copied ← copy ages\n\
         \x20   nota copied.longitudo()\n\
         }\n",
    );
}

#[test]
fn copy_parameter_holding_sermo_is_refused_at_the_declaration() {
    let source = "genus Capsa {\n\
         \x20   varia sermo s\n\
         }\n\
         functio accipe(copy Capsa c) → numerus {\n\
         \x20   redde c.n\n\
         }\n";
    // The body reads `c.n`, which needs the field; declare it too.
    let source = &source.replace(
        "genus Capsa {\n    varia sermo s\n}",
        "genus Capsa {\n    varia numerus n = 0\n    varia sermo s\n}",
    );
    assert_one_rejection_with(
        &format!(
            "{source}incipit {{\n\
             \x20   sit Capsa c ← Capsa {{ n = 1, s = ad 'runtime:echo' (\"x\") }}\n\
             \x20   nota accipe(copy c)\n\
             }}\n"
        ),
        &[("path", "Capsa.s"), ("head", "sermo")],
    );
}

#[test]
fn generic_copy_parameter_decides_at_the_call_argument() {
    assert_one_rejection_with(
        "functio gemina<T>(copy T x) → numerus {\n\
         \x20   varia T y ← x\n\
         \x20   redde 1\n\
         }\n\
         incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   nota gemina(copy s)\n\
         }\n",
        &[("head", "sermo")],
    );
}

#[test]
fn generic_copy_parameter_checks_a_nested_generic_member_at_the_call() {
    assert_one_rejection_with(
        "genus Capsa<T> {\n\
         \x20   varia T res\n\
         }\n\
         functio gemina<T>(copy Capsa<T> x) → numerus {\n\
         \x20   redde 1\n\
         }\n\
         incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   sit Capsa<sermo> p ← Capsa<sermo> { res = s }\n\
         \x20   nota gemina(copy p)\n\
         }\n",
        &[("path", "Capsa.res"), ("head", "sermo")],
    );
}

#[test]
fn generic_copy_parameter_with_an_ordinary_argument_is_accepted() {
    test_support::assert_accepts_la(
        "functio gemina<T>(copy T x) → numerus {\n\
         \x20   varia T y ← x\n\
         \x20   redde 1\n\
         }\n\
         incipit {\n\
         \x20   varia lista<numerus> xs ← [1, 2]\n\
         \x20   nota gemina(copy xs)\n\
         }\n",
    );
}

#[test]
fn copy_of_a_closable_genus_is_refused() {
    assert_one_rejection_with(
        "@ radix contract \"closing\"\n\
         implendum Closable {\n\
         \x20   functio close() → bivalens ⇥ textus\n\
         }\n\
         genus Connus implet Closable {\n\
         \x20   varia numerus pes = 0\n\
         \x20   functio close() → bivalens ⇥ textus {\n\
         \x20       redde verum\n\
         \x20   }\n\
         }\n\
         incipit {\n\
         \x20   sit Connus c ← Connus { pes = 1 }\n\
         \x20   varia Connus d ← copy c\n\
         \x20   nota d.pes\n\
         }\n",
        &[("head", "Connus")],
    );
}

#[test]
fn value_side_copy_rebind_is_refused() {
    assert_one_rejection_with(
        "genus Capsa {\n\
         \x20   varia sermo s\n\
         }\n\
         incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   varia Capsa a ← Capsa { s = s }\n\
         \x20   varia Capsa b ← Capsa { s = s }\n\
         \x20   b ← copy a\n\
         \x20   nota b.s\n\
         }\n",
        &[("path", "Capsa.s"), ("head", "sermo")],
    );
}

#[test]
fn copy_of_a_sermo_directional_view_is_refused() {
    let found = copy_nonduplicable_args(
        "incipit {\n\
         \x20   sit sermo s ← ad 'runtime:echo' (\"x\")\n\
         \x20   sit meus<textus> m ← s.meus<textus>()\n\
         \x20   varia meus<textus> m2 ← copy m\n\
         \x20   nota m2\n\
         }\n",
    );
    assert_eq!(found.len(), 1, "exactly one refusal, got {found:?}");
    assert_eq!(arg(&found[0], "head"), Some("meus"));
}
