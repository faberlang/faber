//! Tests for diagnostic rendering (plain, expanded, with reader packs).
//! Proves: plain/expanded formats include code, location, source line;
//! bidi controls are escaped; reader-pack template substitution works;
//! missing templates degrade gracefully.

use super::{
    render_expanded, render_expanded_diagnostics, render_expanded_diagnostics_with_pack, render_expanded_with_pack,
    render_plain, render_plain_with_pack,
};
use crate::diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase, Severity};
use crate::lexer::Span;
use crate::locale::{DiagnosticTemplateRenderError, LocalePack, TemplateField};

#[test]
fn plain_render_includes_code_location_source_and_help() {
    let diagnostic = Diagnostic::error("expected identifier")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("PARSE001")
        .with_file("input.fab")
        .with_span(Span { start: 4, end: 9 })
        .with_source_line("fixum 123")
        .with_help("add a name");

    assert_eq!(
        render_plain(&diagnostic),
        "error[PARSE001]: input.fab:4\n  | \u{2068}fixum 123\u{2069}"
    );
}

/// Byte offsets printed as `file:N` leak past physical EOF (`main.fab:1846` in a 93-line file).
#[test]
fn plain_render_file_line_stays_within_physical_source_length() {
    let mut source = String::new();
    for _ in 0..5 {
        source.push_str("line\n");
    }
    source.push_str("boom token\n");
    let start = u32::try_from(source.find("boom").expect("boom")).expect("fits u32");
    let line_count = u32::try_from(source.lines().count()).expect("fits u32");
    assert!(
        start > line_count,
        "reproducer needs a byte offset past the physical line count ({start} > {line_count})"
    );

    let diagnostic = Diagnostic::error("expected identifier")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("PARSE001")
        .with_source_context("main.fab", &source, Span { start, end: start + 4 });
    let rendered = render_plain(&diagnostic);

    assert!(
        rendered.contains(&format!("main.fab:{line_count}")),
        "plain location must be a physical line ≤ {line_count}: {rendered}"
    );
    assert!(
        !rendered.contains(&format!("main.fab:{start}")),
        "byte offset must not leak as a line number: {rendered}"
    );
}

#[test]
fn plain_render_uses_issue_in_compact_code_label() {
    let diagnostic = Diagnostic::error("unknown identifier")
        .with_phase(DiagnosticPhase::Analysis)
        .with_code("SEM001")
        .with_file("input.fab")
        .with_span(Span { start: 5, end: 12 })
        .with_args(vec![DiagnosticArg::new("issue", "unknown_identifier")]);

    assert_eq!(render_plain(&diagnostic), "error[SEM001:unknown_identifier]: input.fab:5");
}

#[test]
fn plain_render_wraps_embedded_source_line_in_bidi_isolation() {
    let diagnostic = Diagnostic::error("expected identifier")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("PARSE001")
        .with_file("arabic.fab")
        .with_span(Span { start: 0, end: 9 })
        .with_source_line("إذا x { nota \"ok\" }");

    assert_eq!(
        render_plain(&diagnostic),
        "error[PARSE001]: arabic.fab:0\n  | \u{2068}إذا x { nota \"ok\" }\u{2069}"
    );
}

#[test]
fn text_render_escapes_bidi_controls_in_message_and_help() {
    let raw = '\u{202E}';
    let diagnostic = Diagnostic::error(format!("bad {raw} text"))
        .with_phase(DiagnosticPhase::Parse)
        .with_file("bidi.fab")
        .with_help(format!("remove {raw} control"));

    let plain = render_plain(&diagnostic);
    let expanded = render_expanded(&diagnostic);

    assert!(!plain.contains(raw), "{plain:?}");
    assert!(!expanded.contains(raw), "{expanded:?}");
    assert!(plain.contains("U+202E"), "{plain:?}");
    assert!(expanded.contains("U+202E"), "{expanded:?}");
}

#[test]
fn expanded_render_degrades_missing_file_and_span() {
    let diagnostic = Diagnostic::warning("target ignored")
        .with_phase(DiagnosticPhase::Tool)
        .with_severity(Severity::Info);

    assert_eq!(
        render_expanded(&diagnostic),
        "info[uncoded] tool <unknown>: target ignored\nphase: tool\nfile: <unknown>\nspan: <none>"
    );
}

#[test]
fn expanded_batch_uses_blank_line_separators() {
    let diagnostics = vec![
        Diagnostic::error("first").with_file("a.fab"),
        Diagnostic::warning("second").with_file("b.fab"),
    ];

    assert_eq!(
        render_expanded_diagnostics(&diagnostics),
        "error[uncoded] tool a.fab: first\nphase: tool\nfile: a.fab\nspan: <none>\n\nwarning[uncoded] tool b.fab: second\nphase: tool\nfile: b.fab\nspan: <none>"
    );
}

#[test]
fn expanded_render_wraps_source_line_in_bidi_isolation() {
    let diagnostic = Diagnostic::error("expected identifier")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("PARSE001")
        .with_file("arabic.fab")
        .with_span(Span { start: 0, end: 9 })
        .with_source_line("إذا x { nota \"ok\" }");

    assert_eq!(
        render_expanded(&diagnostic),
        "error[PARSE001] parse arabic.fab\nphase: parse\nfile: arabic.fab\nspan: 0..9\nsource: \u{2068}إذا x { nota \"ok\" }\u{2069}"
    );
}

#[test]
fn plain_render_with_pack_uses_explicit_pack_template_text() {
    let pack = token_pack();
    let diagnostic = Diagnostic::error("PARSE011")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("PARSE011")
        .with_file("token.fab")
        .with_span(Span { start: 0, end: 8 })
        .with_source_line("functio")
        .with_args(vec![DiagnosticArg::new("name", "salve")]);

    assert_eq!(
        render_plain_with_pack(&diagnostic, &pack).expect("pack plain render"),
        "error[PARSE011]: token.fab:0: MSG:salve:KW_FUNCTIO\n  | \u{2068}functio\u{2069}\n  = help: HELP:TY_TEXTUS"
    );
}

#[test]
fn plain_render_with_pack_missing_template_uses_code_only() {
    let pack = token_pack();
    let diagnostic = Diagnostic::warning("target ignored")
        .with_phase(DiagnosticPhase::Tool)
        .with_code("WARN999")
        .with_file("tool.fab")
        .with_help("remove it");

    assert_eq!(
        render_plain_with_pack(&diagnostic, &pack).expect("missing template falls back to code"),
        "warning[WARN999]: tool.fab"
    );
}

#[test]
fn expanded_render_with_pack_uses_explicit_pack_template_text() {
    let pack = token_pack();
    let diagnostic = Diagnostic::error("PARSE011")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("PARSE011")
        .with_file("token.fab")
        .with_args(vec![DiagnosticArg::new("name", "salve")]);

    assert_eq!(
        render_expanded_with_pack(&diagnostic, &pack).expect("pack expanded render"),
        "error[PARSE011] parse token.fab: MSG:salve:KW_FUNCTIO\nphase: parse\nfile: token.fab\nspan: <none>\nhelp: HELP:TY_TEXTUS"
    );
}

#[test]
fn expanded_batch_render_with_pack_keeps_default_text_when_template_missing() {
    let pack = token_pack();
    let diagnostics = vec![
        Diagnostic::error("PARSE011")
            .with_phase(DiagnosticPhase::Parse)
            .with_code("PARSE011")
            .with_file("token.fab")
            .with_args(vec![DiagnosticArg::new("name", "salve")]),
        Diagnostic::warning("target ignored")
            .with_phase(DiagnosticPhase::Tool)
            .with_code("WARN999")
            .with_file("tool.fab"),
    ];

    assert_eq!(
        render_expanded_diagnostics_with_pack(&diagnostics, &pack).expect("pack expanded batch"),
        "error[PARSE011] parse token.fab: MSG:salve:KW_FUNCTIO\nphase: parse\nfile: token.fab\nspan: <none>\nhelp: HELP:TY_TEXTUS\n\nwarning[WARN999] tool tool.fab\nphase: tool\nfile: tool.fab\nspan: <none>"
    );
}

#[test]
fn render_with_pack_propagates_template_errors() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.PARSE011]
message = "expected {missing}"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("pack should validate");
    let diagnostic = Diagnostic::error("expected function name")
        .with_code("PARSE011")
        .with_file("bad.fab");

    assert_eq!(
        render_plain_with_pack(&diagnostic, &pack).expect_err("missing placeholder should fail"),
        DiagnosticTemplateRenderError::MissingPlaceholder {
            code: "PARSE011".to_string(),
            field: TemplateField::Message,
            placeholder: "missing".to_string(),
        }
    );
}

fn token_pack() -> LocalePack {
    LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
functio = "KW_FUNCTIO"

[types]
textus = "TY_TEXTUS"

[diagnostics.PARSE011]
message = "MSG:{name}:{functio}"
help = "HELP:{textus}"

[llm]
system_prompt_snippet = "emit test Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("test pack should validate")
}
