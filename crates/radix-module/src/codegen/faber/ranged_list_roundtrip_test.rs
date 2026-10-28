//! List size ranges survive Faber re-emission as authored.
//!
//! `list<int, size 4‥8>` keeps its half-open glyph, `size 4…8` keeps the
//! inclusive one, `size 0…∞` stays `size 0…∞` (it is not rewritten to the
//! plain `list<int>`), `size 0…8` is not rewritten to `size 8`, and `size N`
//! stays `size N`. The operator ruled that the formatter never normalizes what
//! somebody wrote; re-emitting the output is byte-identical.

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

const BODY: &str = "fn f(list<int, size 4‥8> a, list<int, size 4…8> b, list<int, size 0…∞> c, list<int, size 4> d, list<int, size 0…8> e, list<int, size 1…∞> g, list<int, size 2‥3> h, list<int> i, list<b8, size 0…8> j, list<b8, size 2‥6> m) → int {\n    return a.length() + b.length() + c.length() + d.length() + e.length() + g.length() + h.length() + i.length() + j.length() + m.length()\n}\n\nfn k<size N>(list<int, size N> xs) → int {\n    return xs.length()\n}\n\nmain {\n    let list<int, size 4‥8> a ← [1, 2, 3, 4]\n    let list<int, size 0…∞> c ← [1]\n    let list<int, size 4> d ← [1, 2, 3, 4]\n    print a\n    print c\n    print d\n}\n";

fn en_source() -> String {
    format!("+++\nlocale = \"en\"\n+++\n{BODY}")
}

#[test]
fn en_format_keeps_the_authored_range_spelling() {
    let formatted = reemit(&en_source(), "en");
    for spelled in [
        "list<int, size 4‥8> a,",
        "list<int, size 4…8> b,",
        "list<int, size 0…∞> c",
        "list<int, size 4> d",
        "list<int, size 0…8> e",
        "list<int, size 1…∞> g",
        "list<int, size 2‥3> h",
        "list<int> i,",
        "list<b8, size 0…8> j,",
        "list<b8, size 2‥6> m)",
        "list<int, size N> xs",
        "let list<int, size 4‥8> a ← [1, 2, 3, 4]",
        "let list<int, size 0…∞> c ← [1]",
        "let list<int, size 4> d ← [1, 2, 3, 4]",
    ] {
        assert!(formatted.contains(spelled), "missing `{spelled}` in:\n{formatted}");
    }
    for rewritten in ["size 4…7", "size 2…2", "size 8>", "list<int> c"] {
        assert!(!formatted.contains(rewritten), "`{rewritten}` appeared in:\n{formatted}");
    }
}

#[test]
fn re_emission_is_a_fixed_point() {
    let first = reemit(&en_source(), "en");
    let second = reemit(&format!("+++\nlocale = \"en\"\n+++\n{first}"), "en");
    assert_eq!(first, second);
}

#[test]
fn en_to_la_round_trip_keeps_the_range_and_translates_the_marker() {
    let la = reemit(&en_source(), "la");
    assert!(la.contains("lista<numerus, magnitudo 4‥8> a"), "{la}");
    assert!(la.contains("lista<numerus, magnitudo 4…8> b"), "{la}");
    assert!(la.contains("lista<numerus, magnitudo 0…∞> c"), "{la}");
    assert!(la.contains("lista<numerus, magnitudo 0…8> e"), "{la}");
    assert_eq!(reemit(&la, "en"), reemit(&en_source(), "en"));
}
