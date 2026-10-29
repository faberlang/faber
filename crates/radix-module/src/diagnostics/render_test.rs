//! Tests for diagnostic rendering (plain, expanded, with reader packs).
//! Proves: plain/expanded formats include code, location, source line;
//! bidi controls are escaped; reader-pack template substitution works;
//! missing templates degrade gracefully.

use super::{
    render_expanded, render_expanded_diagnostics, render_expanded_diagnostics_with_pack, render_expanded_with_pack,
    render_one_text, render_plain, render_plain_with_pack,
};
use crate::diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase, Severity};
use crate::lexer::Span;
use crate::locale::{DiagnosticTemplateRenderError, LocalePack, LocalePackIssue, LocalePackLoadError, TemplateField};

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

/// Need: PKG001 arms carry ad-hoc prose with no catalog row and no
/// locale-pack template, so an issue-less PKG001 must render its message;
/// an issue'd PKG001 stays compact (the `package_path_missing` contract in
/// `crates/faber/tests/run_integration_test.rs` forbids raw io prose).
#[test]
fn plain_render_shows_message_for_issueless_pkg001() {
    let diagnostic = Diagnostic::error("could not encode FHIR package: boom")
        .with_code("PKG001")
        .with_file("foo");

    let rendered = render_plain(&diagnostic);

    assert!(rendered.contains("error[PKG001]: foo"), "{rendered}");
    assert!(rendered.contains(diagnostic.message.as_str()), "{rendered}");
}

#[test]
fn plain_render_keeps_compact_header_for_pkg001_with_issue() {
    let diagnostic = Diagnostic::error("could not encode FHIR package: boom")
        .with_code("PKG001")
        .with_file("foo")
        .with_args(vec![DiagnosticArg::new("issue", "package_path_missing")]);

    let rendered = render_plain(&diagnostic);

    assert!(rendered.contains("error[PKG001:package_path_missing]: foo"), "{rendered}");
    assert!(!rendered.contains(diagnostic.message.as_str()), "{rendered}");
}

const PKG001_ROW_PACK: &str = r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.PKG001]
message = "package build failed"

[diagnostics.PKG001.issues.with_row]
message = "row for {thing}"
help = "row help"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#;

#[test]
fn pack_render_shows_pkg001_issue_row_with_args_and_help() {
    let pack = LocalePack::from_toml_str(PKG001_ROW_PACK).expect("pack");
    let diagnostic = Diagnostic::error("prose fallback")
        .with_code("PKG001")
        .with_file("foo")
        .with_args(vec![
            DiagnosticArg::new("issue", "with_row"),
            DiagnosticArg::new("thing", "cargo"),
        ]);

    let rendered = render_plain_with_pack(&diagnostic, &pack).expect("render");

    assert!(rendered.contains("error[PKG001:with_row]: foo: row for cargo"), "{rendered}");
    assert!(rendered.contains("= help: row help"), "{rendered}");
}

#[test]
fn pack_render_keeps_rowless_and_issueless_pkg001_unchanged() {
    let pack = LocalePack::from_toml_str(PKG001_ROW_PACK).expect("pack");
    let rowless = Diagnostic::error("prose fallback")
        .with_code("PKG001")
        .with_file("foo")
        .with_args(vec![DiagnosticArg::new("issue", "no_row")]);
    let issueless = Diagnostic::error("prose fallback")
        .with_code("PKG001")
        .with_file("foo");

    let rowless = render_plain_with_pack(&rowless, &pack).expect("render");
    let issueless = render_plain_with_pack(&issueless, &pack).expect("render");

    assert_eq!(rowless, "error[PKG001:no_row]: foo");
    assert_eq!(issueless, "error[PKG001]: foo: prose fallback");
}

#[test]
fn plain_render_omits_the_empty_location_slot_of_a_file_less_diagnostic() {
    let diagnostic = Diagnostic::error("no file here").with_code("PKG001");

    assert_eq!(render_plain(&diagnostic), "error[PKG001]: no file here");
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

/// A hostile source line must not drive the terminal: OSC, C0, and DEL bytes
/// render as their `U+XXXX` spellings, and bidi isolation still wraps the snippet.
#[test]
fn plain_render_escapes_osc_c0_and_del_bytes_in_source_line() {
    let line = "ok \u{1B}]0;pwned\u{7}x \u{1}mid\u{7F}";
    let diagnostic = Diagnostic::error("bad token")
        .with_phase(DiagnosticPhase::Parse)
        .with_file("evil.fab")
        .with_span(Span { start: 0, end: 1 })
        .with_source_line(line);

    let rendered = render_plain(&diagnostic);

    assert!(
        !rendered.contains('\u{1B}')
            && !rendered.contains('\u{7}')
            && !rendered.contains('\u{1}')
            && !rendered.contains('\u{7F}'),
        "raw control byte survived: {rendered:?}"
    );
    assert!(
        rendered.contains("U+001B]0;pwnedU+0007x U+0001midU+007F"),
        "escaped form missing: {rendered:?}"
    );
    assert!(
        rendered.contains('\u{2068}'),
        "bidi isolation must still wrap the snippet: {rendered:?}"
    );
}

#[test]
fn expanded_render_escapes_osc_c0_and_del_bytes_in_source_line() {
    let diagnostic = Diagnostic::error("bad token")
        .with_phase(DiagnosticPhase::Parse)
        .with_file("evil.fab")
        .with_span(Span { start: 0, end: 1 })
        .with_source_line("ok \u{1B}]0;pwned\u{7}x \u{1}mid\u{7F}");

    let rendered = render_expanded(&diagnostic);

    assert!(
        !rendered.contains('\u{1B}') && !rendered.contains('\u{7}') && !rendered.contains('\u{7F}'),
        "raw control byte survived: {rendered:?}"
    );
    assert!(
        rendered.contains("source: \u{2068}ok U+001B]0;pwnedU+0007x U+0001midU+007F\u{2069}"),
        "escaped, isolated source line missing: {rendered:?}"
    );
}

/// The ariadne path reads the raw source (span offsets stay byte-exact) and
/// the control policy is applied to what it rendered. The renderer's own SGR
/// color sequences must survive; every attacker escape use loses its ESC.
#[test]
fn ariadne_render_escapes_source_controls_and_keeps_labeled_span() {
    let source = "main {\n  let evil = \"a\u{1B}]0;pwned\u{7}b\";\n}\n";
    let diagnostic = Diagnostic::error("bad token")
        .with_phase(DiagnosticPhase::Parse)
        .with_file("evil.fab")
        .with_span(Span { start: 20, end: 22 });

    let rendered =
        render_one_text(&diagnostic, &[("evil.fab".to_owned(), source.to_owned())], None).expect("ariadne render");

    assert!(!rendered.contains('\u{7}'), "raw BEL survived: {rendered:?}");
    assert!(!rendered.contains("\u{1B}]"), "raw OSC survived: {rendered:?}");

    let visible = strip_sgr_sequences(&rendered);
    assert!(visible.contains("U+001B]0;pwnedU+0007"), "escaped form missing: {visible:?}");
    assert!(
        visible.contains("let evil"),
        "raw-source echo missing (spans must stay on raw offsets): {visible:?}"
    );
    assert!(
        visible.contains("evil.fab:2:14"),
        "span label must land on the raw-offset line:column: {visible:?}"
    );
    assert!(visible.contains("bad token"), "span label message missing: {visible:?}");
}

/// Drop well-formed SGR sequences so assertions see the report's visible text.
fn strip_sgr_sequences(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let sgr_len = if rest.starts_with('\u{1B}') {
            super::sgr_sequence_len(rest)
        } else {
            0
        };
        if sgr_len > 0 {
            rest = &rest[sgr_len..];
        } else {
            let ch = rest.chars().next().expect("non-empty rest");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

#[test]
fn expanded_render_degrades_missing_file_and_span() {
    let diagnostic = Diagnostic::warning("target ignored")
        .with_phase(DiagnosticPhase::Tool)
        .with_severity(Severity::Info);

    assert_eq!(
        render_expanded(&diagnostic),
        "info[uncoded] tool <unknown>\nphase: tool\nfile: <unknown>\nspan: <none>\nmessage: target ignored"
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
        "error[uncoded] tool a.fab\nphase: tool\nfile: a.fab\nspan: <none>\nmessage: first\n\nwarning[uncoded] tool b.fab\nphase: tool\nfile: b.fab\nspan: <none>\nmessage: second"
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
        "error[PARSE011] parse token.fab\nphase: parse\nfile: token.fab\nspan: <none>\nmessage: MSG:salve:KW_FUNCTIO\nhelp: HELP:TY_TEXTUS"
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
        "error[PARSE011] parse token.fab\nphase: parse\nfile: token.fab\nspan: <none>\nmessage: MSG:salve:KW_FUNCTIO\nhelp: HELP:TY_TEXTUS\n\nwarning[WARN999] tool tool.fab\nphase: tool\nfile: tool.fab\nspan: <none>"
    );
}

const MISSING_PLACEHOLDER_PACK: &str = r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.PARSE011]
message = "expected {missing}"
help = "add {missing} here"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#;

/// A template placeholder the emit site did not supply must not abort the run
///: the base message renders with a note naming the placeholder.
#[test]
fn render_with_pack_falls_back_when_placeholder_is_missing() {
    let pack = LocalePack::from_toml_str(MISSING_PLACEHOLDER_PACK).expect("pack should validate");
    let diagnostic = Diagnostic::error("expected function name")
        .with_code("PARSE011")
        .with_file("bad.fab");

    let plain = render_plain_with_pack(&diagnostic, &pack).expect("missing placeholder falls back");
    assert!(plain.contains("expected function name"), "base message kept: {plain}");
    assert!(
        plain.contains("[locale pack template missing placeholder: PARSE011.message {missing}]"),
        "note names the placeholder: {plain}"
    );

    let expanded =
        render_expanded_with_pack(&diagnostic, &pack).expect("missing placeholder falls back in expanded mode");
    assert!(
        expanded.contains("[locale pack template missing placeholder: PARSE011.message {missing}]"),
        "{expanded}"
    );

    // Batch rendering keeps going: the other diagnostic still renders.
    let other = Diagnostic::error("other failure").with_file("bad.fab");
    let batch =
        render_expanded_diagnostics_with_pack(&[diagnostic, other], &pack).expect("batch render does not abort");
    assert!(batch.contains("other failure"), "{batch}");
}

/// A pack that must quote Faber's own braced syntax (LEX003's `u{...}`,
/// SEM016's `_ { … }`) uses `{{` as the literal-brace escape.
#[test]
fn render_with_pack_doubled_brace_renders_literal_brace() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.LEX003]
message = "unknown escape {escape}; write u{{...} for a braced hex escape {name}"
help = "replace {escape} with u{{...}"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("doubled brace is a legal template");
    let diagnostic = Diagnostic::error("unknown escape")
        .with_code("LEX003")
        .with_args(vec![
            DiagnosticArg::new("escape", "\\x"),
            DiagnosticArg::new("name", "hex_escape"),
        ]);

    let rendered = pack
        .render_diagnostic_text(&diagnostic)
        .expect("doubled brace renders without error")
        .expect("LEX003 template row");
    assert!(
        rendered.message.contains("u{...}"),
        "doubled brace renders one literal brace: {:?}",
        rendered.message
    );
    assert!(
        !rendered.message.contains("missing placeholder"),
        "literal brace is not a missing placeholder: {:?}",
        rendered.message
    );
    // A normal placeholder after the escape still resolves.
    assert!(
        rendered.message.contains("hex_escape"),
        "placeholder after the escape still resolves: {:?}",
        rendered.message
    );
    assert_eq!(
        rendered.help.as_deref(),
        Some("replace \\x with u{...}"),
        "help escapes the same way"
    );
}

/// The `{{` escape must not swallow real placeholder errors.
#[test]
fn render_with_pack_single_brace_placeholder_behavior_is_unchanged() {
    // A single opening brace with no closing brace stays an error. Validation
    // rejects it at load; if a pack ever reaches the renderer with one, the
    // renderer still reports `UnclosedPlaceholder`.
    let unclosed = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.LEX003]
message = "write u{..."

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    );
    match unclosed {
        Ok(pack) => {
            let diagnostic = Diagnostic::error("unclosed").with_code("LEX003");
            assert_eq!(
                pack.render_diagnostic_text(&diagnostic),
                Err(DiagnosticTemplateRenderError::UnclosedPlaceholder {
                    code: "LEX003".to_owned(),
                    field: TemplateField::Message,
                }),
                "a single opening brace is still unclosed"
            );
        }
        Err(LocalePackLoadError::Validation(issues)) => assert!(
            issues.contains(&LocalePackIssue::UnclosedTemplatePlaceholder {
                code: "LEX003".to_owned(),
                field: TemplateField::Message,
            }),
            "an unclosed single brace must stay a validation error: {issues:?}"
        ),
        Err(err) => panic!("unexpected load error: {err:?}"),
    }

    // A single `{nope}` with no matching arg still reports the missing
    // placeholder rather than rendering a literal brace.
    let missing = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.LEX003]
message = "unknown escape {nope}"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("single-brace placeholder is legal");
    let diagnostic = Diagnostic::error("unknown escape").with_code("LEX003");
    assert_eq!(
        missing.render_diagnostic_text(&diagnostic),
        Err(DiagnosticTemplateRenderError::MissingPlaceholder {
            code: "LEX003".to_owned(),
            field: TemplateField::Message,
            placeholder: "nope".to_owned(),
        }),
        "a single-brace placeholder without an arg still reports missing"
    );
}

/// The issue row owns the prose; the base row is only for code-only diagnostics.
#[test]
fn render_with_pack_uses_issue_row_when_present() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.LEX004]
message = "unexpected character"
help = "remove the character"

[diagnostics.LEX004.issues.homoglyph_did_you_mean]
message = "unexpected character: '{char}'; did you mean '{suggestion}'?"
help = "replace '{char}' with the canonical spelling '{suggestion}'"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("pack should validate");
    let diagnostic = Diagnostic::error("unexpected character")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("LEX004")
        .with_args(vec![
            DiagnosticArg::new("issue", "homoglyph_did_you_mean"),
            DiagnosticArg::new("char", "ᵗ"),
            DiagnosticArg::new("suggestion", "ᵀ"),
        ]);

    let rendered = pack
        .render_diagnostic_text(&diagnostic)
        .expect("issue row should render")
        .expect("LEX004 template row");
    assert_eq!(rendered.message, "unexpected character: 'ᵗ'; did you mean 'ᵀ'?");
    assert_eq!(rendered.help.as_deref(), Some("replace 'ᵗ' with the canonical spelling 'ᵀ'"));
}

/// A missing issue row must degrade loudly (DEF S14-1): silently rendering the
/// base row is how the shipped homoglyph prose became invisible.
#[test]
fn render_with_pack_names_missing_issue_row_instead_of_silent_base_fallback() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.LEX004]
message = "unexpected character"
help = "remove the character"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("pack should validate");
    let diagnostic = Diagnostic::error("unexpected character")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("LEX004")
        .with_args(vec![
            DiagnosticArg::new("issue", "homoglyph_did_you_mean"),
            DiagnosticArg::new("char", "ᵗ"),
            DiagnosticArg::new("suggestion", "ᵀ"),
        ]);

    let rendered = pack
        .render_diagnostic_text(&diagnostic)
        .expect("base row should render")
        .expect("LEX004 template row");
    assert!(
        rendered.message.contains("LEX004.homoglyph_did_you_mean"),
        "missing issue row must be named in the rendered message: {:?}",
        rendered.message
    );
    assert!(
        !rendered.message.contains("did you mean"),
        "base row must not masquerade as issue prose: {:?}",
        rendered.message
    );
}

/// Every installed reader pack carries the LEX004 homoglyph did-you-mean row
/// and renders it with the emitted `{char}`/`{suggestion}` facts.
#[test]
fn every_installed_pack_renders_homoglyph_did_you_mean() {
    const INSTALLED_LOCALES: &[&str] = &["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];
    let diagnostic = Diagnostic::error("unexpected character")
        .with_phase(DiagnosticPhase::Parse)
        .with_code("LEX004")
        .with_args(vec![
            DiagnosticArg::new("issue", "homoglyph_did_you_mean"),
            DiagnosticArg::new("char", "ᵗ"),
            DiagnosticArg::new("suggestion", "ᵀ"),
        ]);

    for locale in INSTALLED_LOCALES {
        let source = std::fs::read_to_string(crate::locale::workspace_locale_pack_path(locale))
            .unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
        let pack = LocalePack::from_toml_str(&source)
            .unwrap_or_else(|err| panic!("installed {locale} pack should parse: {err:?}"));

        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .unwrap_or_else(|err| panic!("{locale} LEX004 should render: {err:?}"))
            .expect("LEX004 template row");
        assert!(
            rendered.message.contains("'ᵗ'") && rendered.message.contains("'ᵀ'"),
            "{locale} LEX004 homoglyph_did_you_mean must name the char and the canonical suggestion: {:?}",
            rendered.message
        );
        assert!(
            !rendered.message.contains("missing row"),
            "{locale} must carry the homoglyph_did_you_mean row: {:?}",
            rendered.message
        );
    }
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

// Renderer-contract: the MIR fragment errors own English prose renderers
// (`message()`); these tests pin how each issue's args substitute into it.
// Moved from the mir fragment tests, where they asserted `.message()` prose.

#[test]
fn fragment_composition_message_carries_cycle_and_expansion() {
    use crate::mir::fragment_composition::{FragmentCompositionError, ISSUE_CYCLE, ISSUE_EXPANSION};
    let cycle_error = FragmentCompositionError {
        issue: ISSUE_CYCLE,
        reason: "cycle",
        span: crate::test_support::test_span(),
        definition_span: None,
        args: vec![DiagnosticArg::new("cycle", "ping → pong → ping")],
    };
    assert_eq!(cycle_error.message(), "nucleum fragment call cycle: ping → pong → ping");

    let expansion_error = FragmentCompositionError {
        issue: ISSUE_EXPANSION,
        reason: "depth",
        span: crate::test_support::test_span(),
        definition_span: None,
        args: vec![
            DiagnosticArg::new("kind", "depth"),
            DiagnosticArg::new("observed", "17"),
            DiagnosticArg::new("limit", "16"),
            DiagnosticArg::new("chain", "k → a → b"),
        ],
    };
    assert_eq!(
        expansion_error.message(),
        "nucleum fragment expansion exceeds depth limit 16 (observed 17) along k → a → b"
    );
}

#[test]
fn fragment_instantiation_message_carries_type_and_call_site_context() {
    use crate::mir::fragment_instantiation::{FragmentInstantiationError, ISSUE_CONTEXT, ISSUE_TYPE};
    let type_error = FragmentInstantiationError {
        issue: ISSUE_TYPE,
        reason: "argument",
        span: crate::test_support::test_span(),
        definition_span: None,
        args: vec![
            DiagnosticArg::new("expected", "f32"),
            DiagnosticArg::new("actual", "u32"),
        ],
    };
    assert_eq!(type_error.message(), "@ nucleum fragment argument has type u32, expected f32");

    let context_error = FragmentInstantiationError {
        issue: ISSUE_CONTEXT,
        reason: "nested_dispatch",
        span: crate::test_support::test_span(),
        definition_span: None,
        args: vec![DiagnosticArg::new("caller", "entry")],
    };
    assert_eq!(context_error.message(), "@ nucleum cannot call another @ nucleum entry");

    let definition = FragmentInstantiationError {
        issue: ISSUE_CONTEXT,
        reason: "host_callable",
        span: crate::test_support::test_span(),
        definition_span: None,
        args: Vec::new(),
    };
    assert_eq!(
        definition.message(),
        "@ nucleum fragment cannot call a host function or other non-device-safe callable"
    );
    let instantiation = FragmentInstantiationError {
        issue: ISSUE_CONTEXT,
        reason: "host_callable",
        span: crate::test_support::test_span(),
        definition_span: None,
        args: vec![DiagnosticArg::new("caller", "host")],
    };
    assert_eq!(instantiation.message(), "host code cannot call an @ nucleum fragment");
}

#[test]
fn mixed_script_advisory_renders_prose_in_every_installed_pack() {
    let source = "let _ аdd ← 1\n";
    let advisory = crate::lexer::lex(source)
        .mixed_script_advisories
        .first()
        .expect("mixed-script advisory")
        .clone();
    let diagnostic = super::super::convert::convert_from_mixed_script_advisory("advisory.fab", source, &advisory);

    for locale in ["ar", "en", "hi", "la", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let path = crate::locale::workspace_locale_pack_path(locale);
        let pack = LocalePack::from_toml_path(path)
            .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"));
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .expect("render should not error")
            .unwrap_or_else(|| panic!("{locale} pack must own a LOCALE004 prose row"));
        assert!(
            rendered.message.contains("аdd"),
            "{locale} prose must substitute the identifier: {}",
            rendered.message
        );
    }
}
