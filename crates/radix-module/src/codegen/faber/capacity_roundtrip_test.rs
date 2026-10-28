//! Queue, stack, string and ascii capacities survive Faber re-emission with the
//! `size` marker: `queue<int, size 4>`, `stack<int, size N>`,
//! `string<size 8>`, `ascii<size 1>`. The `_` hole stays bare. The marker is
//! translated with the locale, and re-emitting the output is byte-identical.

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

const BODY: &str = "fn f(queue<int, size 4> q, stack<int, size 2> s, string<size 8> t, ascii<size 1> c) → int {\n    return q.length() + s.length() + t.length()\n}\n\nfn k<size N>(queue<int, size N> q, stack<int, size N> s, string<size N> t, ascii<size N> a) → int {\n    return q.length()\n}\n\nmain {\n    mut queue<int, size 4> q ← empty\n    mut stack<int, size 2> s ← empty\n    let string<size 8> t ← \"ab\" ↦ string<size 8>\n    let ascii<size 1> c ← 'x'\n    mut queue<int, _> h ← empty\n    print q.length()\n    print s.length()\n    print t\n    print c\n    print h.length()\n}\n";

fn en_source() -> String {
    format!("+++\nlocale = \"en\"\n+++\n{BODY}")
}

#[test]
fn en_format_keeps_the_size_marker() {
    let formatted = reemit(&en_source(), "en");
    for spelled in [
        "queue<int, size 4> q,",
        "stack<int, size 2> s,",
        "string<size 8> t,",
        "ascii<size 1> c)",
        "queue<int, size N> q,",
        "stack<int, size N> s,",
        "string<size N> t,",
        "ascii<size N> a)",
        "mut queue<int, size 4> q ← empty",
        "let string<size 8> t ←",
        "let ascii<size 1> c ← 'x'",
        "mut queue<int, _> h ← empty",
    ] {
        assert!(formatted.contains(spelled), "missing `{spelled}` in:\n{formatted}");
    }
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
    assert!(la.contains("queue<numerus, magnitudo 4> q"), "{la}");
    assert!(la.contains("stack<numerus, magnitudo 2> s"), "{la}");
    assert!(la.contains("textus<magnitudo 8> t"), "{la}");
    assert!(la.contains("ascii<magnitudo 1> c"), "{la}");
    assert!(la.contains("queue<numerus, _> h"), "{la}");
    assert_eq!(reemit(&la, "en"), reemit(&en_source(), "en"));
}

#[test]
fn en_to_a_non_latin_locale_translates_the_marker() {
    let zh = reemit(&en_source(), "zh-Hans");
    assert!(zh.contains("队列<整数, 维度 4> q"), "{zh}");
    assert!(zh.contains("栈<整数, 维度 2> s"), "{zh}");
    assert!(zh.contains("文本<维度 8> t"), "{zh}");
    assert!(zh.contains("窄字串<维度 1> c"), "{zh}");
    assert!(zh.contains("队列<整数, _> h"), "{zh}");
}
