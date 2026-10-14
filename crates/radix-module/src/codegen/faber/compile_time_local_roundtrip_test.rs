//! Local compile-time constants survive Faber re-emission and reach the Rust
//! backend as ordinary immutable locals (DECISIONS D5.10).
//!
//! `fixum T x = e` re-emits with `=` (never `←`) in the target locale, and
//! the authored initializer is printed as written. The constant needs no new
//! backend code: Rust sees a plain `let` with the authored initializer.

use crate::driver::reemit_faber;
use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};
use crate::locale::LocalePack;

fn pack(locale: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

fn reemit(source: &str, to: &str) -> String {
    let session = Session::new(
        Config::default()
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

const BODY: &str = "fn f(int p) → int {\n    const int k = 3 * 4\n    const _ m = 7\n    const int r ← p + k\n    return r + m\n}\n\nmain {\n    print f(1)\n}\n";

fn en_source() -> String {
    format!("+++\nlocale = \"en\"\n+++\n{BODY}")
}

#[test]
fn en_format_keeps_the_equals_glyph_and_authored_initializer() {
    let formatted = reemit(&en_source(), "en");
    assert!(formatted.contains("    const int k = 3 * 4\n"), "{formatted}");
    assert!(formatted.contains("    const _ m = 7\n"), "{formatted}");
    assert!(formatted.contains("    const int r ← p + k\n"), "{formatted}");
}

#[test]
fn en_to_la_round_trip_keeps_the_equals_glyph() {
    let la = reemit(&en_source(), "la");
    assert!(la.contains("    fixum numerus k = 3 * 4\n"), "{la}");
    assert!(la.contains("    fixum numerus r ← p + k\n"), "{la}");
    assert_eq!(reemit(&la, "en"), reemit(&en_source(), "en"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_an_ordinary_immutable_local() {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_dev_locale_root(),
    );
    let result = crate::driver::compile(&session, "<test>", &en_source(), ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let crate::Output::Rust(out) = result.output.expect("output") else {
        panic!("expected Rust output");
    };
    assert!(out.code.contains("let k: i64 ="), "{}", out.code);
    assert!(!out.code.contains("static K"), "{}", out.code);
}
