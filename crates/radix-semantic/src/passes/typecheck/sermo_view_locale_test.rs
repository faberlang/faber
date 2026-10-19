//! Reader-locale handling of the directional channel (`sermo`) view methods.
//!
//! en spells the view constructors `send<T>()` / `recv<T>()` and the view
//! methods `get` / `finish` / `drain` / `cursor`; the Latin spellings are a
//! wrong-locale miss (`unknown_method`) under en, like any intrinsic method,
//! and stay the only spelling under la. `da` is not a projected row (its
//! rename belongs to the Sink<T> need) and keeps one spelling everywhere.

use crate::locale::LocalePack;
use crate::test_support::{self, AnalyzedFixture};
use crate::{PassConfig, SemanticErrorKind};
use radix_target::Target;

fn pack(id: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
        .expect("installed pack should load")
}

fn analyze(id: &str, source: &str) -> AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(pack(id));
    test_support::analyze_source_with_config(source, config)
}

fn unknown_method_count(result: &AnalyzedFixture) -> usize {
    result
        .diagnostics
        .iter()
        .filter(|error| {
            error.kind == SemanticErrorKind::MemberUndefined
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "unknown_method")
        })
        .count()
}

fn en_program(body: &str) -> String {
    format!("main {{\n    let channel s ← call 'runtime:echo' (\"salve\")\n{body}\n}}\n")
}

fn la_program(body: &str) -> String {
    format!("incipit {{\n    sit sermo s ← ad 'runtime:echo' (\"salve\")\n{body}\n}}\n")
}

#[test]
fn en_accepts_every_projected_channel_view_spelling() {
    let source = en_program(
        "    let send<string> m ← s.send<string>()\n\
         \x20   m.da(\"ping\")\n\
         \x20   let status out ← m.finish()\n\
         \x20   let recv<string> t ← s.recv<string>()\n\
         \x20   for from t.cursor() let frame {\n    }\n\
         \x20   let status done ← t.finish()\n\
         \x20   let recv<string> t2 ← s.recv<string>()\n\
         \x20   let frame<string> ∪ none got ← t2.get()\n\
         \x20   let recv<string> t3 ← s.recv<string>()\n\
         \x20   let string drained ← t3.drain()",
    );
    let result = analyze("en", &source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn en_refuses_each_latin_channel_view_spelling() {
    // One Latin spelling per case: the view constructors on the conversation
    // and each projected method on its view.
    let cases = [
        "    let send<string> m ← s.meus<string>()",
        "    let recv<string> t ← s.tuus<string>()",
        "    let send<string> m ← s.send<string>()\n    let status x ← m.fini()",
        "    let recv<string> t ← s.recv<string>()\n    let status x ← t.fini()",
        "    let recv<string> t ← s.recv<string>()\n    let frame<string> ∪ none g ← t.accipe()",
        "    let recv<string> t ← s.recv<string>()\n    let string d ← t.exhauri()",
    ];
    for body in cases {
        let result = analyze("en", &en_program(body));
        assert!(
            unknown_method_count(&result) >= 1,
            "expected unknown_method for {body:?}, got {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn la_keeps_the_latin_channel_view_spellings_and_refuses_the_en_ones() {
    let latin = la_program(
        "    sit meus<textus> m ← s.meus<textus>()\n\
         \x20   m.da(\"ping\")\n\
         \x20   sit status out ← m.fini()\n\
         \x20   sit tuus<textus> t ← s.tuus<textus>()\n\
         \x20   sit scrinium<textus> ∪ nihil got ← t.accipe()\n\
         \x20   sit tuus<textus> t2 ← s.tuus<textus>()\n\
         \x20   sit textus drained ← t2.exhauri()",
    );
    let result = analyze("la", &latin);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);

    for body in [
        "    sit meus<textus> m ← s.send<textus>()",
        "    sit tuus<textus> t ← s.recv<textus>()",
    ] {
        let result = analyze("la", &la_program(body));
        assert!(
            !result.is_ok(),
            "la must refuse the en spelling in {body:?}"
        );
    }
}
