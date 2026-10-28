//! Reader-locale rendered diagnostic text contracts for package check paths.
//!
//! These tests assert renderer and reader-pack *text* contracts: plain render
//! format, bidi isolation markers, and stable `code:issue` slugs in shipped
//! output. They do not assert package manifest loading, library resolution, or
//! build layout behavior.

use super::config_with_locale;
use crate::package::faber_analysis_seams;
use radix::program::analyze::check_package as check_package_direct;

fn check_with_faber_seams(
    config: &radix::module::driver::Config,
    input: &std::path::Path,
) -> Vec<radix::module::diagnostics::Diagnostic> {
    check_package_direct(config, input, &faber_analysis_seams())
}
use radix::module::codegen::Target;
use radix::module::diagnostics::{Diagnostic, DiagnosticArg};
use radix::module::locale::LocalePack;
use std::path::PathBuf;

const ISSUE_INITIALIZER_ANNOTATION_MISMATCH: &str = "initializer_annotation_mismatch";
const BIDI_ISOLATE_START: &str = "\u{2068}";
const BIDI_ISOLATE_END: &str = "\u{2069}";

fn locale_examples_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus-product/reader-locale")
}

fn locale_fault_path(locale: &str, rel: &str) -> PathBuf {
    locale_examples_root().join(locale).join(rel)
}

fn diagnostics_with_issue<'a>(diagnostics: &[&'a Diagnostic], issue: &str) -> Vec<&'a Diagnostic> {
    diagnostics
        .iter()
        .copied()
        .filter(|diag| diag.args.contains(&DiagnosticArg::new("issue", issue)))
        .collect()
}

fn render_plain(diag: &Diagnostic, pack: &LocalePack) -> String {
    radix::module::diagnostics::render_plain_with_pack(diag, pack).expect("render diagnostic")
}

fn assert_plain_render_contract(rendered: &str, header: &str) {
    assert!(rendered.contains(header));
    assert!(rendered.contains(BIDI_ISOLATE_START));
    assert!(rendered.contains(BIDI_ISOLATE_END));
}

/// Assert that a locale's reader pack loads and contains the expected diagnostic template.
#[expect(
    dead_code,
    reason = "kept as a focused reader-pack contract helper for locale additions"
)]
fn assert_locale_pack_contains(locale: &str, fault_rel: &str, expected_code: &[&str]) {
    let fault = locale_fault_path(locale, fault_rel);
    let (_config, pack) =
        config_with_locale(Target::HirRust, &fault, Some(locale), None).expect("reader config");
    let pack = pack.expect("reader pack");
    for code in expected_code {
        assert!(
            pack.diagnostics.contains_key(*code),
            "locale {locale} missing diagnostic template {code}"
        );
    }
}

#[test]
fn package_render_lex004_unexpected_character_zh_hans() {
    assert_single_locale_lex004("zh-Hans");
}

#[test]
fn package_render_lex004_unexpected_character_zh_hant() {
    assert_single_locale_lex004("zh-Hant");
}

#[test]
fn package_render_lex004_unexpected_character_ar() {
    assert_single_locale_lex004("ar");
}

#[test]
fn package_render_lex004_unexpected_character_hi() {
    assert_single_locale_lex004("hi");
}

#[test]
fn package_render_lex004_unexpected_character_vi() {
    assert_single_locale_lex004("vi");
}

fn assert_single_locale_lex004(locale: &str) {
    let fault = locale_fault_path(locale, "faults/non-ascii-number.fab");
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some(locale), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert_eq!(pack.metadata.id.as_str(), locale);
    assert!(
        pack.diagnostics
            .get("LEX004")
            .and_then(|template| template.issues.get("unexpected_character"))
            .is_some()
    );

    let diagnostics = check_with_faber_seams(&config, &fault);
    let lex004: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("LEX004"))
        .collect();
    let unexpected_character = diagnostics_with_issue(&lex004, "unexpected_character");

    assert!(
        !unexpected_character.is_empty(),
        "expected {locale} LEX004 unexpected_character; diagnostics: {diagnostics:?}"
    );
    for diag in unexpected_character {
        assert!(diag.args.iter().any(|arg| arg.name == "char"));
        assert!(
            pack.render_diagnostic_text(diag)
                .expect("LEX004 template should render")
                .is_some()
        );
        assert_plain_render_contract(
            &render_plain(diag, &pack),
            "error[LEX004:unexpected_character]",
        );
    }
}

#[test]
fn package_render_emits_thai_sem010_initializer_mismatch() {
    let fault = locale_fault_path("th-TH", "faults/type-mismatch.fab");
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some("th-TH"), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM010"));

    let diagnostics = check_with_faber_seams(&config, &fault);
    let sem010: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM010"))
        .collect();
    assert!(
        !sem010.is_empty(),
        "expected SEM010 diagnostics: {diagnostics:?}"
    );
    let issue_sem010 = diagnostics_with_issue(&sem010, ISSUE_INITIALIZER_ANNOTATION_MISMATCH);
    assert!(
        !issue_sem010.is_empty(),
        "expected initializer_annotation_mismatch diagnostics: {sem010:?}"
    );

    let rendered = issue_sem010
        .iter()
        .map(|diag| render_plain(diag, &pack))
        .collect::<Vec<_>>()
        .join("\n");

    assert_plain_render_contract(&rendered, "error[SEM010:initializer_annotation_mismatch]");
}

#[test]
fn package_render_preserves_bidi_for_arabic_sem010() {
    let fault = locale_fault_path("ar", "faults/type-mismatch.fab");
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some("ar"), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(
        pack.diagnostics
            .get("SEM010")
            .and_then(|template| template.issues.get(ISSUE_INITIALIZER_ANNOTATION_MISMATCH))
            .is_some()
    );

    let diagnostics = check_with_faber_seams(&config, &fault);
    let sem010: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM010"))
        .collect();
    assert!(
        !sem010.is_empty(),
        "expected SEM010 diagnostics: {diagnostics:?}"
    );
    let issue_sem010 = diagnostics_with_issue(&sem010, ISSUE_INITIALIZER_ANNOTATION_MISMATCH);
    assert!(
        !issue_sem010.is_empty(),
        "expected initializer_annotation_mismatch diagnostics: {sem010:?}"
    );

    let rendered = issue_sem010
        .iter()
        .map(|diag| render_plain(diag, &pack))
        .collect::<Vec<_>>()
        .join("\n");

    assert_plain_render_contract(&rendered, "error[SEM010:initializer_annotation_mismatch]");
}

#[test]
fn package_render_emits_sem010_initializer_mismatch_zh_hans() {
    assert_single_locale_sem010("zh-Hans", "faults/type-mismatch.fab");
}

#[test]
fn package_render_emits_sem010_initializer_mismatch_zh_hant() {
    assert_single_locale_sem010_or_sem001("zh-Hant", "faults/type-mismatch.fab");
}

#[test]
fn package_render_emits_sem010_initializer_mismatch_hi() {
    assert_single_locale_sem010_or_sem001("hi", "faults/type-mismatch.fab");
}

#[test]
fn package_render_emits_sem010_initializer_mismatch_vi() {
    assert_single_locale_sem010_or_sem001_renders("vi", "faults/type-mismatch.fab");
}

fn assert_single_locale_sem010(locale: &str, fault_rel: &str) {
    let fault = locale_fault_path(locale, fault_rel);
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some(locale), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM010"));

    let diagnostics = check_with_faber_seams(&config, &fault);
    let sem010: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM010"))
        .collect();
    assert!(
        !sem010.is_empty(),
        "expected SEM010 diagnostics for {locale}: {diagnostics:?}"
    );
    let issue_sem010 = diagnostics_with_issue(&sem010, ISSUE_INITIALIZER_ANNOTATION_MISMATCH);
    assert!(
        !issue_sem010.is_empty(),
        "expected initializer_annotation_mismatch diagnostics for {locale}: {sem010:?}"
    );

    let rendered = issue_sem010
        .iter()
        .map(|diag| render_plain(diag, &pack))
        .collect::<Vec<_>>()
        .join("\n");

    assert_plain_render_contract(&rendered, "error[SEM010:initializer_annotation_mismatch]");
}

/// Some locales produce SEM010, others SEM001. Accept either and verify rendering.
fn assert_single_locale_sem010_or_sem001(locale: &str, fault_rel: &str) {
    let fault = locale_fault_path(locale, fault_rel);
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some(locale), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM010"));

    let diagnostics = check_with_faber_seams(&config, &fault);

    let sem010: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM010"))
        .collect();
    if sem010.is_empty() {
        let sem001: Vec<_> = diagnostics
            .iter()
            .filter(|diag| diag.code == Some("SEM001"))
            .collect();
        assert!(
            !sem001.is_empty(),
            "expected SEM001 diagnostics for {locale}: {diagnostics:?}"
        );
        let unknown_id = diagnostics_with_issue(&sem001, "unknown_identifier");
        assert!(
            !unknown_id.is_empty(),
            "missing unknown_identifier fact for {locale}: {sem001:?}"
        );
        for diag in unknown_id {
            assert!(
                pack.render_diagnostic_text(diag)
                    .expect("SEM001 template should render")
                    .is_some()
            );
            assert_plain_render_contract(
                &render_plain(diag, &pack),
                "error[SEM001:unknown_identifier]",
            );
        }
    } else {
        let issue_sem010 = diagnostics_with_issue(&sem010, ISSUE_INITIALIZER_ANNOTATION_MISMATCH);
        assert!(
            !issue_sem010.is_empty(),
            "missing initializer_annotation_mismatch fact for {locale}: {sem010:?}"
        );
        for diag in issue_sem010 {
            assert!(
                pack.render_diagnostic_text(diag)
                    .expect("SEM010 template should render")
                    .is_some()
            );
            assert_plain_render_contract(
                &render_plain(diag, &pack),
                "error[SEM010:initializer_annotation_mismatch]",
            );
        }
    }
}

/// The fault must surface as a semantic error (SEM010 or SEM001) and render
/// with the pack template. The retired reader-locale fallback warning is no
/// longer a fallback path (S0-U5): if neither semantic error appears, the
/// test fails loudly.
fn assert_single_locale_sem010_or_sem001_renders(locale: &str, fault_rel: &str) {
    let fault = locale_fault_path(locale, fault_rel);
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some(locale), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM010"));

    let diagnostics = check_with_faber_seams(&config, &fault);

    // Try SEM010 first, then SEM001.
    let sem010: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM010"))
        .collect();
    if !sem010.is_empty() {
        let issue_sem010 = diagnostics_with_issue(&sem010, ISSUE_INITIALIZER_ANNOTATION_MISMATCH);
        assert!(
            !issue_sem010.is_empty(),
            "missing initializer_annotation_mismatch fact for {locale}: {sem010:?}"
        );
        for diag in issue_sem010 {
            assert!(
                pack.render_diagnostic_text(diag)
                    .expect("SEM010 template should render")
                    .is_some()
            );
            assert_plain_render_contract(
                &render_plain(diag, &pack),
                "error[SEM010:initializer_annotation_mismatch]",
            );
        }
        return;
    }

    let sem001: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM001"))
        .collect();
    assert!(
        !sem001.is_empty(),
        "expected SEM001 diagnostics for {locale}: {diagnostics:?}"
    );
    let unknown_id = diagnostics_with_issue(&sem001, "unknown_identifier");
    assert!(
        !unknown_id.is_empty(),
        "missing unknown_identifier fact for {locale}: {sem001:?}"
    );
    for diag in unknown_id {
        assert!(
            pack.render_diagnostic_text(diag)
                .expect("SEM001 template should render")
                .is_some()
        );
        assert_plain_render_contract(
            &render_plain(diag, &pack),
            "error[SEM001:unknown_identifier]",
        );
    }
}

#[test]
fn package_render_emits_sem001_unknown_identifier_zh_hans() {
    assert_single_locale_sem001_or_reader001("zh-Hans", "faults/undefined-variable.fab");
}

#[test]
fn package_render_emits_sem001_unknown_identifier_zh_hant() {
    assert_single_locale_sem001_or_reader001("zh-Hant", "faults/undefined-variable.fab");
}

#[test]
fn package_render_emits_sem001_unknown_identifier_ar() {
    assert_single_locale_sem001_or_reader001("ar", "faults/undefined-variable.fab");
}

#[test]
fn package_render_emits_sem001_unknown_identifier_hi() {
    assert_single_locale_sem001_or_reader001("hi", "faults/undefined-variable.fab");
}

#[test]
fn package_render_emits_sem001_unknown_identifier_vi() {
    assert_single_locale_sem001_or_reader001("vi", "faults/undefined-variable.fab");
}

fn assert_single_locale_sem001_or_reader001(locale: &str, fault_rel: &str) {
    let fault = locale_fault_path(locale, fault_rel);
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some(locale), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM001"));

    let diagnostics = check_with_faber_seams(&config, &fault);

    // SEM001 is the only acceptance path now; the retired reader-locale
    // fallback warning is no longer produced (S0-U5).
    let sem001: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM001"))
        .collect();
    assert!(
        !sem001.is_empty(),
        "expected SEM001 diagnostics for {locale}: {diagnostics:?}"
    );
    assert!(
        sem001.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "unknown_identifier")),
        "expected SEM001 unknown_identifier issue for {locale}: {diagnostics:?}"
    );
    let unknown_identifier = diagnostics_with_issue(&sem001, "unknown_identifier");
    assert!(
        !unknown_identifier.is_empty(),
        "missing SEM001 unknown_identifier fact for {locale}: {sem001:?}"
    );
    for diag in unknown_identifier {
        assert!(
            pack.render_diagnostic_text(diag)
                .expect("SEM001 template should render")
                .is_some()
        );
        assert_plain_render_contract(
            &render_plain(diag, &pack),
            "error[SEM001:unknown_identifier]",
        );
    }
}

#[test]
fn package_render_emits_sem001_suggestion_for_vietnamese_name() {
    // semantic-name-suggestion.fab produces LOCALE002 lexer suggestions plus a
    // PARSE030 error. The test verifies LOCALE002 spelling suggestions.
    let fault = locale_fault_path("vi", "faults/semantic-name-suggestion.fab");
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some("vi"), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("LOCALE002"));

    let diagnostics = check_with_faber_seams(&config, &fault);
    let reader002: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("LOCALE002"))
        .collect();
    assert!(
        !reader002.is_empty(),
        "expected LOCALE002 diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(radix::module::Diagnostic::is_error),
        "misspelled identifier must not be accepted as valid source: {diagnostics:?}"
    );

    let suggestion = reader002
        .iter()
        .find(|diag| {
            diag.args
                .iter()
                .any(|arg| arg.name == "suggestion" && arg.value == "bắt_đầu")
        })
        .expect("LOCALE002 suggestion diagnostic");
    assert!(
        suggestion
            .args
            .iter()
            .any(|arg| arg.name == "spelling" && arg.value == "bắtđầu")
    );

    assert_plain_render_contract(&render_plain(suggestion, &pack), "warning[LOCALE002");
}

#[test]
fn package_render_emits_reader002_accented_keyword_suggestion() {
    let fault = locale_fault_path("vi", "faults/keyword-suggestion.fab");
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some("vi"), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("LOCALE002"));

    let diagnostics = check_with_faber_seams(&config, &fault);
    let reader002: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("LOCALE002"))
        .collect();
    assert!(
        !reader002.is_empty(),
        "expected LOCALE002 diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(radix::module::Diagnostic::is_error),
        "misspelled keyword must not be accepted as valid source: {diagnostics:?}"
    );

    let suggestion = reader002[0];
    assert!(!suggestion.is_error());
    assert!(
        suggestion
            .args
            .iter()
            .any(|arg| arg.name == "spelling" && arg.value == "ham")
    );
    assert!(
        suggestion
            .args
            .iter()
            .any(|arg| arg.name == "suggestion" && arg.value == "hàm")
    );

    assert_plain_render_contract(&render_plain(suggestion, &pack), "warning[LOCALE002]");
}

#[test]
fn package_render_emits_reader002_typo_keyword_suggestion() {
    let fault = locale_fault_path("vi", "faults/keyword-edit-distance.fab");
    let (config, pack) =
        config_with_locale(Target::HirRust, &fault, Some("vi"), None).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("LOCALE002"));

    let diagnostics = check_with_faber_seams(&config, &fault);
    let reader002: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("LOCALE002"))
        .collect();
    assert!(
        !reader002.is_empty(),
        "expected LOCALE002 diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(radix::module::Diagnostic::is_error),
        "misspelled keyword must not be accepted as valid source: {diagnostics:?}"
    );

    let suggestion = reader002[0];
    assert!(!suggestion.is_error());
    assert!(
        suggestion
            .args
            .iter()
            .any(|arg| arg.name == "spelling" && arg.value == "hamm")
    );
    assert!(
        suggestion
            .args
            .iter()
            .any(|arg| arg.name == "suggestion" && arg.value == "hàm")
    );

    assert_plain_render_contract(&render_plain(suggestion, &pack), "warning[LOCALE002]");
}
