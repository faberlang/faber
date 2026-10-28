//! A vector lane count survives Faber re-emission with its `size` marker.
//!
//! The long form `vector<f32, size 4>` re-emits as written and the `vf32[4]`
//! shorthand stays the shorthand. Re-emitting the output is byte-identical, and
//! the en to la round trip translates the marker.

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;

fn pack(locale: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

fn reemit(source: &str, to: &str) -> String {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = reemit_faber(&session, &pack(to), "test.fab", source);
    assert!(
        result.success(),
        "source must re-emit to {to} and re-validate:\n{source}\n{:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    result
        .output
        .expect("successful re-emit must carry output")
        .code
}

const BODY: &str = "fn f(vector<f32, size 4> a, vf32[4] b, vector<f32, _> c, vf32 d) → int {\n    return 1\n}\n\nfn k<size N>(vector<f32, size N> x, vector<f32, size (N + 1)> y, vf32[N] z) → int {\n    return 2\n}\n\nmain {\n    let vector<f32, size 3> v ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>\n    print v\n}\n";

fn en_source() -> String {
    format!("+++\nlocale = \"en\"\n+++\n{BODY}")
}

#[test]
fn en_format_keeps_the_size_marker_and_the_shorthand() {
    let formatted = reemit(&en_source(), "en");
    for spelled in [
        "vector<f32, size 4> a",
        "vf32[4] b",
        "vector<f32, _> c",
        "vector<f32, size N> x",
        "vector<f32, size (N + 1)> y",
        "vf32[N] z",
        "let vector<f32, size 3> v",
    ] {
        assert!(formatted.contains(spelled), "missing `{spelled}` in:\n{formatted}");
    }
    assert!(!formatted.contains("vector<f32, 4>"), "{formatted}");
}

#[test]
fn re_emission_is_a_fixed_point() {
    let first = reemit(&en_source(), "en");
    let second = reemit(&format!("+++\nlocale = \"en\"\n+++\n{first}"), "en");
    assert_eq!(first, second);
}

#[test]
fn en_to_la_round_trip_translates_the_marker() {
    let la = reemit(&en_source(), "la");
    assert!(la.contains("vector<f32, magnitudo 4> a"), "{la}");
    assert!(la.contains("vector<f32, magnitudo N> x"), "{la}");
    assert_eq!(reemit(&la, "en"), reemit(&en_source(), "en"));
}
