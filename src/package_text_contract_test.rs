//! Reader-locale rendered diagnostic text contracts for package check paths.
//!
//! These tests assert renderer and reader-pack *text* contracts: plain render
//! format, bidi isolation markers, and stable `code:issue` slugs in shipped
//! output. They do not assert package manifest loading, library resolution, or
//! build layout behavior.

use super::{check_package, config_with_reader_locale};
use radix::codegen::Target;
use radix::diagnostics::{Diagnostic, DiagnosticArg};
use radix::reader_locale::ReaderLocalePack;
use std::path::{Path, PathBuf};

const ISSUE_INITIALIZER_ANNOTATION_MISMATCH: &str = "initializer_annotation_mismatch";
const BIDI_ISOLATE_START: &str = "\u{2068}";
const BIDI_ISOLATE_END: &str = "\u{2069}";

fn reader_locale_examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale")
}

fn reader_locale_fault_path(locale: &str, rel: &str) -> PathBuf {
    reader_locale_examples_root().join(locale).join(rel)
}

fn diagnostics_with_issue<'a>(diagnostics: &[&'a Diagnostic], issue: &str) -> Vec<&'a Diagnostic> {
    diagnostics
        .iter()
        .copied()
        .filter(|diag| diag.args.contains(&DiagnosticArg::new("issue", issue)))
        .collect()
}

fn render_plain(diag: &Diagnostic, pack: &ReaderLocalePack) -> String {
    radix::diagnostics::render_plain_with_pack(diag, pack).expect("render diagnostic")
}

fn assert_plain_render_contract(rendered: &str, header: &str) {
    assert!(rendered.contains(header));
    assert!(rendered.contains(BIDI_ISOLATE_START));
    assert!(rendered.contains(BIDI_ISOLATE_END));
}

/// Assert that a locale's reader pack loads and contains the expected diagnostic template.
fn assert_reader_pack_contains(locale: &str, fault_rel: &str, expected_code: &[&str]) {
    let fault = reader_locale_fault_path(locale, fault_rel);
    let (_config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some(locale)).expect("reader config");
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
    let fault = reader_locale_fault_path(locale, "src/non-ascii-number.fab");
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some(locale)).expect("reader config");
    let pack = pack.expect("reader pack");
    assert_eq!(pack.metadata.id.as_str(), locale);
    assert!(pack
        .diagnostics
        .get("LEX004")
        .and_then(|template| template.issues.get("unexpected_character"))
        .is_some());

    let diagnostics = check_package(&config, &fault);
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
        assert!(pack
            .render_diagnostic_text(diag)
            .expect("LEX004 template should render")
            .is_some());
        assert_plain_render_contract(
            &render_plain(diag, &pack),
            "error[LEX004:unexpected_character]",
        );
    }
}

#[test]
fn package_render_emits_thai_sem010_initializer_mismatch() {
    let fault = reader_locale_fault_path("th-TH", "src/type-mismatch.fab");
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some("th-TH")).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM010"));

    let diagnostics = check_package(&config, &fault);
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
    let fault = reader_locale_fault_path("ar", "src/type-mismatch.fab");
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some("ar")).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack
        .diagnostics
        .get("SEM010")
        .and_then(|template| template.issues.get(ISSUE_INITIALIZER_ANNOTATION_MISMATCH))
        .is_some());

    let diagnostics = check_package(&config, &fault);
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
    assert_single_locale_sem010("zh-Hans", "src/type-mismatch.fab");
}

#[test]
fn package_render_emits_sem010_initializer_mismatch_zh_hant() {
    assert_single_locale_sem010_or_sem001("zh-Hant", "src/type-mismatch.fab");
}

#[test]
fn package_render_emits_sem010_initializer_mismatch_hi() {
    assert_single_locale_sem010_or_sem001("hi", "src/type-mismatch.fab");
}

#[test]
fn package_render_emits_sem010_initializer_mismatch_vi() {
    assert_single_locale_sem010_accepts_reader001("vi", "src/type-mismatch.fab");
}

fn assert_single_locale_sem010(locale: &str, fault_rel: &str) {
    let fault = reader_locale_fault_path(locale, fault_rel);
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some(locale)).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM010"));

    let diagnostics = check_package(&config, &fault);
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
    let fault = reader_locale_fault_path(locale, fault_rel);
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some(locale)).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM010"));

    let diagnostics = check_package(&config, &fault);

    let sem010: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM010"))
        .collect();
    if !sem010.is_empty() {
        let issue_sem010 =
            diagnostics_with_issue(&sem010, ISSUE_INITIALIZER_ANNOTATION_MISMATCH);
        assert!(
            !issue_sem010.is_empty(),
            "missing initializer_annotation_mismatch fact for {locale}: {sem010:?}"
        );
        for diag in issue_sem010 {
            assert!(pack
                .render_diagnostic_text(diag)
                .expect("SEM010 template should render")
                .is_some());
            assert_plain_render_contract(
                &render_plain(diag, &pack),
                "error[SEM010:initializer_annotation_mismatch]",
            );
        }
    } else {
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
            assert!(pack
                .render_diagnostic_text(diag)
                .expect("SEM001 template should render")
                .is_some());
            assert_plain_render_contract(
                &render_plain(diag, &pack),
                "error[SEM001:unknown_identifier]",
            );
        }
    }
}

/// Some locales produce READER001 warnings instead of semantic errors. Accept those.
fn assert_single_locale_sem010_accepts_reader001(locale: &str, fault_rel: &str) {
    let fault = reader_locale_fault_path(locale, fault_rel);
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some(locale)).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("SEM010"));

    let diagnostics = check_package(&config, &fault);

    // Try SEM010 first, fall back to SEM001, then accept READER001 lexer warnings.
    let sem010: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM010"))
        .collect();
    if !sem010.is_empty() {
        let issue_sem010 =
            diagnostics_with_issue(&sem010, ISSUE_INITIALIZER_ANNOTATION_MISMATCH);
        assert!(
            !issue_sem010.is_empty(),
            "missing initializer_annotation_mismatch fact for {locale}: {sem010:?}"
        );
        for diag in issue_sem010 {
            assert!(pack
                .render_diagnostic_text(diag)
                .expect("SEM010 template should render")
                .is_some());
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
    if !sem001.is_empty() {
        let unknown_id = diagnostics_with_issue(&sem001, "unknown_identifier");
        assert!(
            !unknown_id.is_empty(),
            "missing unknown_identifier fact for {locale}: {sem001:?}"
        );
        for diag in unknown_id {
            assert!(pack
                .render_diagnostic_text(diag)
                .expect("SEM001 template should render")
                .is_some());
            assert_plain_render_contract(
                &render_plain(diag, &pack),
                "error[SEM001:unknown_identifier]",
            );
        }
        return;
    }

    // vi locale produces only READER001/READER002 lexer diagnostics plus PARSE030.
    let reader001: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("READER001"))
        .collect();
    assert!(
        !reader001.is_empty(),
        "expected READER001 diagnostics for {locale}: {diagnostics:?}"
    );
    for diag in &reader001 {
        assert!(pack
            .render_diagnostic_text(diag)
            .expect("READER001 template should render")
            .is_some());
        assert_plain_render_contract(&render_plain(diag, &pack), "warning[READER001");
    }
}

#[test]
fn package_render_emits_sem001_unknown_identifier_zh_hans() {
    assert_single_locale_sem001_or_reader001("zh-Hans", "src/undefined-variable.fab");
}

#[test]
fn package_render_emits_sem001_unknown_identifier_zh_hant() {
    assert_single_locale_sem001_or_reader001("zh-Hant", "src/undefined-variable.fab");
}

#[test]
fn package_render_emits_sem001_unknown_identifier_ar() {
    assert_single_locale_sem001_or_reader001("ar", "src/undefined-variable.fab");
}

#[test]
fn package_render_emits_sem001_unknown_identifier_hi() {
    assert_single_locale_sem001_or_reader001("hi", "src/undefined-variable.fab");
}

#[test]
fn package_render_emits_sem001_unknown_identifier_vi() {
    assert_single_locale_sem001_or_reader001("vi", "src/undefined-variable.fab");
}

fn assert_single_locale_sem001_or_reader001(locale: &str, fault_rel: &str) {
    let fault = reader_locale_fault_path(locale, fault_rel);
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some(locale)).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(
        pack.diagnostics.contains_key("SEM001") || pack.diagnostics.contains_key("READER001")
    );

    let diagnostics = check_package(&config, &fault);

    // Try SEM001 first (zh-Hans/Hant/ar/hi path), fall back to READER001 (vi path).
    let sem001: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM001"))
        .collect();
    if sem001.is_empty() {
        let reader001: Vec<_> = diagnostics
            .iter()
            .filter(|diag| diag.code == Some("READER001"))
            .collect();
        assert!(
            !reader001.is_empty(),
            "expected READER001 diagnostics for {locale}: {diagnostics:?}"
        );
        for diag in &reader001 {
            assert!(pack
                .render_diagnostic_text(diag)
                .expect("READER001 template should render")
                .is_some());
            assert_plain_render_contract(&render_plain(diag, &pack), "warning[READER001");
        }
    } else {
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
            assert!(pack
                .render_diagnostic_text(diag)
                .expect("SEM001 template should render")
                .is_some());
            assert_plain_render_contract(
                &render_plain(diag, &pack),
                "error[SEM001:unknown_identifier]",
            );
        }
    }
}

#[test]
fn package_render_emits_sem001_suggestion_for_vietnamese_name() {
    // After radix reader locale changes, semantic-name-suggestion.fab
    // produces READER001/READER002 lexer diagnostics with a PARSE030
    // error. The test verifies READER002 spelling suggestions.
    let fault = reader_locale_fault_path("vi", "src/semantic-name-suggestion.fab");
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some("vi")).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("READER002"));

    let diagnostics = check_package(&config, &fault);
    let reader002: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("READER002"))
        .collect();
    assert!(
        !reader002.is_empty(),
        "expected READER002 diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(radix::Diagnostic::is_error),
        "misspelled identifier must not be accepted as valid source: {diagnostics:?}"
    );

    let suggestion = reader002
        .iter()
        .find(|diag| {
            diag.args
                .iter()
                .any(|arg| arg.name == "suggestion" && arg.value == "bắt_đầu")
        })
        .expect("READER002 suggestion diagnostic");
    assert!(suggestion
        .args
        .iter()
        .any(|arg| arg.name == "spelling" && arg.value == "bắtđầu"));

    assert_plain_render_contract(&render_plain(suggestion, &pack), "warning[READER002");
}

#[test]
fn package_render_emits_reader002_accented_keyword_suggestion() {
    let fault = reader_locale_fault_path("vi", "src/keyword-suggestion.fab");
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some("vi")).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("READER002"));

    let diagnostics = check_package(&config, &fault);
    let reader002: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("READER002"))
        .collect();
    assert!(
        !reader002.is_empty(),
        "expected READER002 diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(radix::Diagnostic::is_error),
        "misspelled keyword must not be accepted as valid source: {diagnostics:?}"
    );

    let suggestion = reader002[0];
    assert!(!suggestion.is_error());
    assert!(suggestion
        .args
        .iter()
        .any(|arg| arg.name == "spelling" && arg.value == "ham"));
    assert!(suggestion
        .args
        .iter()
        .any(|arg| arg.name == "suggestion" && arg.value == "hàm"));

    assert_plain_render_contract(&render_plain(suggestion, &pack), "warning[READER002]");
}

#[test]
fn package_render_emits_reader002_typo_keyword_suggestion() {
    let fault = reader_locale_fault_path("vi", "src/keyword-edit-distance.fab");
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some("vi")).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack.diagnostics.contains_key("READER002"));

    let diagnostics = check_package(&config, &fault);
    let reader002: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("READER002"))
        .collect();
    assert!(
        !reader002.is_empty(),
        "expected READER002 diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(radix::Diagnostic::is_error),
        "misspelled keyword must not be accepted as valid source: {diagnostics:?}"
    );

    let suggestion = reader002[0];
    assert!(!suggestion.is_error());
    assert!(suggestion
        .args
        .iter()
        .any(|arg| arg.name == "spelling" && arg.value == "hamm"));
    assert!(suggestion
        .args
        .iter()
        .any(|arg| arg.name == "suggestion" && arg.value == "hàm"));

    assert_plain_render_contract(&render_plain(suggestion, &pack), "warning[READER002]");
}
