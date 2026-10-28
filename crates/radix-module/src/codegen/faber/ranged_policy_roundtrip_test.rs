//! Ranged store policies survive Faber re-emission.
//!
//! `saturating<u32, 0‥8>` re-emits with the policy word, the width and the
//! range in the target locale; the half-open `‥` stays half-open, a
//! whole-width range prints as the existing spelling it abbreviates, and
//! re-emitting the output is byte-identical.

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

const BODY: &str = "fn f(int p) → int {\n    let saturating<u32, 0‥8> a ← 20\n    let trapping<i32, -2‥3> b ← 2\n    let wrapping<u8, 3‥9> c ← 12\n    let saturating<i32, -∞‥8> d ← 99\n    let saturating<u8, 0…255> e ← 7\n    let wrapping<i32, -∞…∞> f ← 0\n    print a\n    print b\n    print c\n    print d\n    print e\n    print f\n    return p\n}\n\nmain {\n    print f(1)\n}\n";

fn en_source() -> String {
    format!("+++\nlocale = \"en\"\n+++\n{BODY}")
}

#[test]
fn en_format_keeps_the_authored_range_glyph_and_unbounded_ends() {
    let formatted = reemit(&en_source(), "en");
    assert!(formatted.contains("let saturating<u32, 0‥8> a ← 20\n"), "{formatted}");
    assert!(formatted.contains("let trapping<i32, -2‥3> b ← 2\n"), "{formatted}");
    assert!(formatted.contains("let wrapping<u8, 3‥9> c ← 12\n"), "{formatted}");
    assert!(formatted.contains("let saturating<i32, -∞‥8> d ← 99\n"), "{formatted}");
    assert!(formatted.contains("let saturating<u8, 0…255> e ← 7\n"), "{formatted}");
    assert!(formatted.contains("let wrapping<i32, -∞…∞> f ← 0\n"), "{formatted}");
}

#[test]
fn a_whole_width_range_keeps_its_authored_spelling() {
    let formatted = reemit(&en_source(), "en");
    assert!(formatted.contains("let saturating<u8, 0…255> e ← 7\n"), "{formatted}");
}

#[test]
fn re_emission_is_a_fixed_point() {
    let first = reemit(&en_source(), "en");
    let second = reemit(&format!("+++\nlocale = \"en\"\n+++\n{first}"), "en");
    assert_eq!(first, second);
}

#[test]
fn en_to_la_round_trip_translates_the_policy_words_and_keeps_the_range() {
    let la = reemit(&en_source(), "la");
    assert!(la.contains("saturatus<u32, 0‥8> a"), "{la}");
    assert!(la.contains("exactus<i32, -2‥3> b"), "{la}");
    assert!(la.contains("modulus<u8, 3‥9> c"), "{la}");
    assert_eq!(reemit(&la, "en"), reemit(&en_source(), "en"));
}
