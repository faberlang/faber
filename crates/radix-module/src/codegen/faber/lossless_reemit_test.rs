//! Lossless re-emission regressions found by the format-sweep audit (fmtsw1).
//!
//! The 2026-08-27 corpus format sweep re-emitted exempla through the HIR-backed
//! emitter before it was lossless and silently dropped authored constructs: an
//! entry `exit` expression, field annotations, and `describe` suite nesting.
//! Each test here re-emits source carrying one such construct and locks that
//! the construct survives in the emitted text itself — `hir_meaning_diverged`
//! (the post-emit re-validation) must not be the only guard.

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;

const EN: &str = "+++\nlocale = \"en\"\n+++\n";

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

/// Format is a fixed point: re-emitting the emitted text changes nothing.
fn assert_fixed_point(emitted: &str, locale: &str) {
    // Emitted text carries no frontmatter; restore the reader locale first.
    let again = reemit(&format!("+++\nlocale = \"{locale}\"\n+++\n{emitted}"), locale);
    assert_eq!(again, emitted, "re-emission must be idempotent");
}

#[test]
fn entry_exit_expression_stays_in_the_entry_header() {
    let source = format!("{EN}@ cli {{ name = \"exit-smoke\" }}\nmain args args exit 1 {{\n}}\n");
    let emitted = reemit(&source, "en");
    assert!(
        emitted.contains("main args args exit 1 {"),
        "the entry exit expression must re-emit in the header, not as a body statement:\n{emitted}"
    );
    assert_fixed_point(&emitted, "en");
}

#[test]
fn entry_exit_expression_survives_locale_conversion() {
    let source = format!("{EN}@ cli {{ name = \"exit-smoke\" }}\nmain args args exit 7 {{\n    print args\n}}\n");
    let la = reemit(&source, "la");
    assert!(la.contains("exitus 7"), "{la}");
    assert!(
        !la.contains("\n    7\n"),
        "the exit value must not become a body statement:\n{la}"
    );
    let back = reemit(&la, "en");
    assert!(back.contains("exit 7 {"), "{back}");
}

#[test]
fn entry_without_exit_is_unchanged() {
    let source = format!("{EN}main {{\n    print \"salve\"\n}}\n");
    let emitted = reemit(&source, "en");
    assert!(!emitted.contains("exit"), "{emitted}");
}

#[test]
fn field_annotation_with_payload_survives_format() {
    // The `json` field annotation needs its library import, so this drives the
    // real corpus exemplum through the on-disk package resolver.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/json/json.fab");
    let source = std::fs::read_to_string(&path).expect("corpus/json/json.fab should exist");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root()
            .with_locale_pack(crate::locale::latin_locale_pack()),
    );
    let result = reemit_faber(&session, &pack("en"), path.to_str().expect("utf-8 path"), &source);
    assert!(
        result.success(),
        "json exemplum must re-emit: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue()))
            .collect::<Vec<_>>()
    );
    let emitted = result.output.expect("output").code;
    assert!(
        emitted.contains("@ json { name = \"wire_name\" }\n    mut string name"),
        "the field annotation must stay on its field:\n{emitted}"
    );
}

const SUITE: &str = "describe \"outer\" {\n    test \"first\" {\n        assert true\n    }\n\n    describe \"inner\" {\n        test \"second\" {\n            assert true\n        }\n    }\n}\n";

#[test]
fn describe_suites_keep_their_nesting() {
    let emitted = reemit(&format!("{EN}{SUITE}"), "en");
    assert!(
        emitted.contains("describe \"outer\" {"),
        "suite names must not be flattened away:\n{emitted}"
    );
    assert!(emitted.contains("describe \"inner\" {"), "{emitted}");
    assert_eq!(emitted.matches("describe").count(), 2, "{emitted}");
    assert_fixed_point(&emitted, "en");
}

#[test]
fn describe_suites_survive_locale_conversion() {
    let la = reemit(&format!("{EN}{SUITE}"), "la");
    assert!(la.contains("probandum \"outer\" {"), "{la}");
    assert!(la.contains("probandum \"inner\" {"), "{la}");
    let back = reemit(&la, "en");
    assert!(back.contains("describe \"outer\" {"), "{back}");
    assert!(back.contains("describe \"inner\" {"), "{back}");
}

#[test]
fn top_level_tests_stay_outside_suites() {
    let source = format!(
        "{EN}test \"alone\" {{\n    assert true\n}}\n\ndescribe \"group\" {{\n    test \"member\" {{\n        assert true\n    }}\n}}\n\ntest \"after\" {{\n    assert true\n}}\n"
    );
    let emitted = reemit(&source, "en");
    assert_eq!(emitted.matches("describe").count(), 1, "{emitted}");
    let group = emitted.find("describe \"group\"").expect("suite emitted");
    let alone = emitted
        .find("test \"alone\"")
        .expect("top-level test emitted");
    let after = emitted
        .find("test \"after\"")
        .expect("trailing test emitted");
    assert!(alone < group && group < after, "item order must be kept:\n{emitted}");
    assert_fixed_point(&emitted, "en");
}

// ── function modifier bindings (`errors e`, `options o`) ───────────────────
//
// Each modifier binds a runtime-provided HIR local with no source statement.
// Re-emission must write the authored modifier in the signature, or the
// re-analysis loses the binding and `hir_meaning_diverged` refuses the file.

#[test]
fn errors_modifier_binding_stays_in_the_signature() {
    let source = format!("{EN}fn parse(string raw) errors failure → int {{\n    return 0\n}}\n");
    let emitted = reemit(&source, "en");
    assert!(
        emitted.contains("fn parse(string raw) errors failure → int {"),
        "the `errors` binding must re-emit in the signature:\n{emitted}"
    );
    assert_fixed_point(&emitted, "en");
}

#[test]
fn options_modifier_binding_stays_in_the_signature() {
    let source = format!("{EN}fn mitte() options opts → string {{\n    return \"ok\"\n}}\n");
    let emitted = reemit(&source, "en");
    assert!(
        emitted.contains("fn mitte() options opts → string {"),
        "the `options` binding must re-emit in the signature:\n{emitted}"
    );
    assert_fixed_point(&emitted, "en");
}

#[test]
fn modifier_binding_keeps_its_own_kind_and_order() {
    let source = format!("{EN}fn run() options opts errors failure {{\n}}\n");
    let emitted = reemit(&source, "en");
    assert!(
        emitted.contains("fn run() options opts errors failure"),
        "each binding keeps its modifier keyword, varia authored order:\n{emitted}"
    );
}

#[test]
fn modifier_bindings_survive_locale_conversion() {
    let source = format!("{EN}fn parse(string raw) errors failure → int {{\n    return 0\n}}\n");
    let la = reemit(&source, "la");
    assert!(la.contains("errata failure"), "{la}");
    let back = reemit(&la, "en");
    assert!(back.contains("errors failure"), "{back}");
}

#[test]
fn method_modifier_binding_stays_in_the_signature() {
    let source = format!("{EN}class Box {{\n    fn probe() errors failure → int {{\n        return 0\n    }}\n}}\n");
    let emitted = reemit(&source, "en");
    assert!(emitted.contains("fn probe() errors failure → int {"), "{emitted}");
    assert_fixed_point(&emitted, "en");
}

#[test]
fn function_without_modifier_bindings_is_unchanged() {
    let source = format!("{EN}fn plain(string raw) → int {{\n    return 0\n}}\n");
    let emitted = reemit(&source, "en");
    assert!(!emitted.contains("errors") && !emitted.contains("options"), "{emitted}");
}

// ── labeled record annotations keep their authored element forms ───────────

#[test]
fn record_annotation_keeps_its_hole_and_authored_order() {
    let source = format!(
        "{EN}main {{\n    let record<g: _, w: int> hole ← record<g: f32, w: int> {{ g = 1.0, w = 2 }}\n    print hole.w\n}}\n"
    );
    let emitted = reemit(&source, "en");
    assert!(
        emitted.contains("record<g: _, w: int> hole"),
        "the `_` element hole must not be re-spelled as its solved type:\n{emitted}"
    );
    assert_fixed_point(&emitted, "en");
}

#[test]
fn record_annotation_keeps_authored_field_order() {
    let source = format!(
        "{EN}main {{\n    let record<w: int, g: _> hole ← record<g: f32, w: int> {{ g = 1.0, w = 2 }}\n    print hole.w\n}}\n"
    );
    let emitted = reemit(&source, "en");
    assert!(emitted.contains("record<w: int, g: _> hole"), "{emitted}");
}
