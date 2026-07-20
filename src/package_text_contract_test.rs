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

#[test]
fn package_render_rejects_non_ascii_numerals_with_lex004_template() {
    for locale in ["zh-Hans", "zh-Hant", "ar", "hi", "vi"] {
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
        assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == Some("READER001")),
            "numeral rejection must not rely on Latin fallback for {locale}: {diagnostics:?}"
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
fn package_render_emits_sem010_for_installed_locales() {
    for locale in ["zh-Hans", "zh-Hant", "hi", "vi"] {
        let fault = reader_locale_fault_path(locale, "src/type-mismatch.fab");
        let (config, pack) =
            config_with_reader_locale(Target::Rust, &fault, Some(locale)).expect("reader config");
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
            "expected SEM010 diagnostics for {locale}: {diagnostics:?}"
        );
        let issue_sem010 = diagnostics_with_issue(&sem010, ISSUE_INITIALIZER_ANNOTATION_MISMATCH);
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
    }
}

#[test]
fn package_render_emits_sem001_for_installed_locales() {
    for locale in ["zh-Hans", "zh-Hant", "ar", "hi", "vi"] {
        let fault = reader_locale_fault_path(locale, "src/undefined-variable.fab");
        let (config, pack) =
            config_with_reader_locale(Target::Rust, &fault, Some(locale)).expect("reader config");
        let pack = pack.expect("reader pack");
        assert!(pack
            .diagnostics
            .get("SEM001")
            .and_then(|template| template.issues.get("unknown_identifier"))
            .is_some());

        let diagnostics = check_package(&config, &fault);
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
    let fault = reader_locale_fault_path("vi", "src/semantic-name-suggestion.fab");
    let (config, pack) =
        config_with_reader_locale(Target::Rust, &fault, Some("vi")).expect("reader config");
    let pack = pack.expect("reader pack");
    assert!(pack
        .diagnostics
        .get("SEM001")
        .and_then(|template| template.issues.get("unknown_identifier"))
        .is_some());

    let diagnostics = check_package(&config, &fault);
    let sem001: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM001"))
        .collect();
    assert!(
        !sem001.is_empty(),
        "expected SEM001 diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(radix::Diagnostic::is_error),
        "misspelled identifier must not be accepted as valid source: {diagnostics:?}"
    );

    let suggestion = sem001
        .iter()
        .find(|diag| {
            diag.args
                .iter()
                .any(|arg| arg.name == "suggestion" && arg.value == "thông_điệp")
        })
        .expect("SEM001 suggestion diagnostic");
    assert!(suggestion
        .args
        .iter()
        .any(|arg| arg.name == "spelling" && arg.value == "thong_diep"));

    assert_plain_render_contract(
        &render_plain(suggestion, &pack),
        "error[SEM001:unknown_identifier]",
    );
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
