use crate::diagnostic_explain::{
    is_diagnostic_query, lookup_diagnostic_in_pack, lookup_installed_diagnostic, render_json,
    render_plain,
};
use radix::reader_locale::ReaderLocalePack;

fn synthetic_pack() -> ReaderLocalePack {
    ReaderLocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.SEM010]
message = "CODE_MSG"
help = "CODE_HELP"

[diagnostics.SEM010.issues.initializer_annotation_mismatch]
message = "ISSUE_MSG"
help = "ISSUE_HELP"

[llm]
system_prompt_snippet = "emit test Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("synthetic pack should validate")
}

#[test]
fn diagnostic_query_parser_accepts_code_and_issue_keys() {
    assert!(is_diagnostic_query("SEM010"));
    assert!(is_diagnostic_query(
        "SEM010.initializer_annotation_mismatch"
    ));
    assert!(is_diagnostic_query("WARN001"));
    assert!(!is_diagnostic_query("functio"));
    assert!(!is_diagnostic_query("SEM010."));
    assert!(!is_diagnostic_query(
        "sem010.initializer_annotation_mismatch"
    ));
}

#[test]
fn diagnostic_lookup_selects_issue_row_before_code_row() {
    let pack = synthetic_pack();
    let explanation = lookup_diagnostic_in_pack("SEM010.initializer_annotation_mismatch", &pack)
        .expect("issue row should resolve");

    assert_eq!(explanation.code, "SEM010");
    assert_eq!(
        explanation.issue.as_deref(),
        Some("initializer_annotation_mismatch")
    );
    assert_eq!(explanation.reader_locale, "la-test");
    assert_eq!(explanation.message, "ISSUE_MSG");
    assert_eq!(explanation.help.as_deref(), Some("ISSUE_HELP"));
}

#[test]
fn diagnostic_lookup_uses_code_row_for_code_only_query() {
    let pack = synthetic_pack();
    let explanation = lookup_diagnostic_in_pack("SEM010", &pack).expect("code row should resolve");

    assert_eq!(explanation.lookup_key(), "SEM010");
    assert_eq!(explanation.message, "CODE_MSG");
    assert_eq!(explanation.help.as_deref(), Some("CODE_HELP"));
}

#[test]
fn diagnostic_lookup_does_not_fallback_to_code_row_for_missing_issue() {
    let pack = synthetic_pack();

    assert!(lookup_diagnostic_in_pack("SEM010.no_such_issue", &pack).is_none());
}

#[test]
fn installed_default_pack_resolves_diagnostic_issue_structurally() {
    let explanation = lookup_installed_diagnostic("SEM010.initializer_annotation_mismatch", None)
        .expect("installed lookup should not fail")
        .expect("installed issue should resolve");

    assert_eq!(explanation.code, "SEM010");
    assert_eq!(
        explanation.issue.as_deref(),
        Some("initializer_annotation_mismatch")
    );
    assert_eq!(explanation.reader_locale, "la");
    assert!(!explanation.message.is_empty());
    assert!(explanation
        .help
        .as_deref()
        .is_some_and(|help| !help.is_empty()));
}

#[test]
fn installed_nonlatin_pack_resolves_diagnostic_issue_structurally() {
    let explanation =
        lookup_installed_diagnostic("SEM010.initializer_annotation_mismatch", Some("zh-Hans"))
            .expect("installed lookup should not fail")
            .expect("installed issue should resolve");

    assert_eq!(explanation.code, "SEM010");
    assert_eq!(
        explanation.issue.as_deref(),
        Some("initializer_annotation_mismatch")
    );
    assert_eq!(explanation.reader_locale, "zh-Hans");
    assert!(!explanation.message.is_empty());
}

#[test]
fn diagnostic_plain_render_exposes_lookup_contract() {
    let pack = synthetic_pack();
    let explanation = lookup_diagnostic_in_pack("SEM010.initializer_annotation_mismatch", &pack)
        .expect("issue row should resolve");
    let rendered = render_plain(&explanation);

    assert!(rendered.contains("Faber Diagnostic Reference"));
    assert!(rendered.contains("SEM010.initializer_annotation_mismatch"));
    assert!(rendered.contains("MESSAGE"));
    assert!(rendered.contains("ISSUE_MSG"));
    assert!(rendered.contains("HELP"));
    assert!(rendered.contains("ISSUE_HELP"));
    assert!(rendered.contains("READER LOCALE"));
}

#[test]
fn diagnostic_json_render_preserves_code_issue_and_locale() {
    let pack = synthetic_pack();
    let explanation = lookup_diagnostic_in_pack("SEM010.initializer_annotation_mismatch", &pack)
        .expect("issue row should resolve");
    let rendered = render_json(&explanation).expect("json render");
    let json: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");

    assert_eq!(json["code"], "SEM010");
    assert_eq!(json["issue"], "initializer_annotation_mismatch");
    assert_eq!(json["reader_locale"], "la-test");
    assert_eq!(json["message"], "ISSUE_MSG");
}
