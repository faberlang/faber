//! Terminal rendering boundary for normalized diagnostics.
//!
//! Rendering is intentionally downstream of diagnostic construction. By the
//! time a value reaches this module, it already has a severity, optional code,
//! display file, source span, and named facts; this file decides only how that
//! contract appears in terminal and plain-text output.
//!
//! The pretty renderer delegates span layout and color handling to `ariadne`.
//! The plain renderer stays local because tests, CI logs, and non-TTY consumers
//! need deterministic text without depending on terminal capabilities.
//!
//! INVARIANTS
//! ==========
//! - Rendering must not panic when source text is missing; callers may report
//!   diagnostics for generated or filesystem-only failures.
//! - Coded diagnostics render as compact identities by default.
//! - Severity controls both ariadne report kind and label color.
//! - Embedded source text is wrapped in bidi isolation at deterministic text
//!   rendering boundaries; compiler spans and lexing stay in logical order.
//! - Source-derived snippet text has C0/C1/DEL and bidi controls escaped as
//!   `U+XXXX` before terminal output; ariadne reads raw source (spans stay
//!   byte-exact) and its rendered output is sanitized after the fact, with
//!   the renderer's own SGR color sequences preserved.

use super::{Diagnostic, DiagnosticPhase, Severity};
use crate::locale::{DiagnosticTemplateRenderError, LocalePack, TemplateField};
use ariadne::{Color, Label, Report, ReportKind, Source};
use std::fmt::Write;

const FIRST_STRONG_ISOLATE: char = '\u{2068}';
const POP_DIRECTIONAL_ISOLATE: char = '\u{2069}';

/// Render a batch of normalized diagnostics to stderr with source snippets.
///
/// `sources` is a simple `(display_name, source_text)` map because diagnostics
/// may be produced across package inputs. Missing entries are tolerated and
/// render as span-less context rather than failing the reporting path.
pub fn render_diagnostics(diagnostics: &[Diagnostic], sources: &[(String, String)]) {
    for diag in diagnostics {
        render_one(diag, sources, None).ok();
    }
}

/// Render a batch of normalized diagnostics through an explicit reader pack.
///
/// Missing template rows fall back to the diagnostic's normalized message/help.
/// Malformed localized templates return an error so callers can report bad pack
/// data visibly instead of silently switching languages.
///
/// # Errors
///
/// Returns [`DiagnosticTemplateRenderError`] if a template contains unclosed
/// placeholders or invalid format tokens. A missing placeholder value falls
/// back to the base message with a note naming it; it is not an error.
pub fn render_diagnostics_with_pack(
    diagnostics: &[Diagnostic],
    sources: &[(String, String)],
    pack: &LocalePack,
) -> Result<(), DiagnosticTemplateRenderError> {
    for diag in diagnostics {
        render_one(diag, sources, Some(pack))?;
    }
    Ok(())
}

fn render_one(
    diag: &Diagnostic,
    sources: &[(String, String)],
    pack: Option<&LocalePack>,
) -> Result<(), DiagnosticTemplateRenderError> {
    let rendered = render_one_text(diag, sources, pack)?;
    eprint!("{rendered}");
    Ok(())
}

/// Render one diagnostic through ariadne to sanitized text (the eprint payload).
///
/// The report reads the raw source so span offsets stay byte-exact; the
/// control-character policy applies to what ariadne rendered, after the fact.
fn render_one_text(
    diag: &Diagnostic,
    sources: &[(String, String)],
    pack: Option<&LocalePack>,
) -> Result<String, DiagnosticTemplateRenderError> {
    let text = diagnostic_display_text(diag, pack)?;
    let label = diag.display_label();
    let safe_text = SanitizedDiagnosticText::from_display_text(text.as_ref());
    let message = pretty_report_message(label.as_deref(), safe_text.message());
    let source = source_for_diagnostic(diag, sources);

    let mut builder = Report::build(
        report_kind(diag.severity),
        &diag.file,
        diag.span.map_or(0, |s| s.start as usize),
    )
    .with_message(message);

    if let Some(span) = diag.span {
        builder = builder.with_label(
            Label::new((&diag.file, span.start as usize..span.end as usize))
                .with_color(label_color(diag.severity))
                .with_message(
                    safe_text
                        .message()
                        .or(label.as_deref())
                        .unwrap_or("diagnostic"),
                ),
        );
    }

    if let Some(help) = safe_text.help() {
        builder = builder.with_help(help);
    }

    let mut rendered = Vec::new();
    builder
        .finish()
        .write((&diag.file, Source::from(source)), &mut rendered)
        .ok();
    Ok(sanitize_rendered_output(&String::from_utf8_lossy(&rendered)))
}

/// Render one diagnostic as deterministic plain text without terminal colors.
///
/// This mirrors the same semantic pieces as the ariadne renderer while keeping
/// output stable for snapshots and environments that cannot render rich spans.
#[must_use]
pub fn render_plain(diag: &Diagnostic) -> String {
    match render_plain_text(diag, None) {
        Ok(text) => text,
        Err(err) => format!("error: diagnostic render failed: {err}"),
    }
}

/// Render one diagnostic as deterministic plain text through a reader pack.
///
/// # Errors
///
/// Returns [`DiagnosticTemplateRenderError`] if the pack template contains
/// malformed placeholders. A missing placeholder value falls back to the base
/// message with a note naming it; it is not an error.
pub fn render_plain_with_pack(diag: &Diagnostic, pack: &LocalePack) -> Result<String, DiagnosticTemplateRenderError> {
    render_plain_text(diag, Some(pack))
}

fn render_plain_text(diag: &Diagnostic, pack: Option<&LocalePack>) -> Result<String, DiagnosticTemplateRenderError> {
    let severity = severity_name(diag.severity);

    let location = diag.display_location();

    let text = diagnostic_display_text(diag, pack)?;
    let label = diag.display_label();
    let safe_text = SanitizedDiagnosticText::from_display_text(text.as_ref());
    let mut output = plain_header(severity, label.as_deref(), &location, safe_text.message());

    if let Some(line) = &diag.source_line {
        write!(output, "\n  | {}", bidi_isolate_code(&sanitize_snippet_text(line))).ok();
    }

    if let Some(help) = safe_text.help() {
        write!(output, "\n  = help: {help}").ok();
    }

    Ok(output)
}

/// Render one diagnostic as an expanded deterministic developer record.
///
/// Diagnostics mode is intentionally more internal than normal terminal output:
/// it exposes phase and byte-span ownership so compiler developers can decide
/// where to start debugging without reverse-engineering a pretty report.
#[must_use]
pub fn render_expanded(diag: &Diagnostic) -> String {
    match render_expanded_text(diag, None) {
        Ok(text) => text,
        Err(err) => format!("error: diagnostic render failed: {err}"),
    }
}

/// Render one diagnostic as an expanded developer record through a reader pack.
///
/// # Errors
///
/// Returns [`DiagnosticTemplateRenderError`] if the pack template contains
/// malformed placeholders. A missing placeholder value falls back to the base
/// message with a note naming it; it is not an error.
pub fn render_expanded_with_pack(
    diag: &Diagnostic,
    pack: &LocalePack,
) -> Result<String, DiagnosticTemplateRenderError> {
    render_expanded_text(diag, Some(pack))
}

fn render_expanded_text(diag: &Diagnostic, pack: Option<&LocalePack>) -> Result<String, DiagnosticTemplateRenderError> {
    let severity = severity_name(diag.severity);
    let label = diag.display_label().unwrap_or_else(|| "uncoded".to_owned());
    let file = if diag.file.is_empty() {
        "<unknown>"
    } else {
        diag.file.as_str()
    };
    let span = diag
        .span
        .map_or_else(|| "<none>".to_owned(), |span| format!("{}..{}", span.start, span.end));

    let text = diagnostic_display_text(diag, pack)?;
    let safe_text = SanitizedDiagnosticText::from_display_text(text.as_ref());
    let mut output = expanded_header(severity, &label, diag.phase, file, &span, None);

    if let Some(line) = &diag.source_line {
        write!(output, "\nsource: {}", bidi_isolate_code(&sanitize_snippet_text(line))).ok();
    }

    if let Some(message) = safe_text.message() {
        write!(output, "\nmessage: {message}").ok();
    }

    if let Some(help) = safe_text.help() {
        write!(output, "\nhelp: {help}").ok();
    }

    Ok(output)
}

/// Render a batch of diagnostics as expanded records separated by blank lines.
pub fn render_expanded_diagnostics(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(render_expanded)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Render a batch of expanded records through a reader pack.
///
/// # Errors
///
/// Returns [`DiagnosticTemplateRenderError`] if any diagnostic template
/// contains malformed placeholders. A missing placeholder value falls back to
/// the base message with a note naming it; it is not an error.
pub fn render_expanded_diagnostics_with_pack(
    diagnostics: &[Diagnostic],
    pack: &LocalePack,
) -> Result<String, DiagnosticTemplateRenderError> {
    let records = diagnostics
        .iter()
        .map(|diagnostic| render_expanded_with_pack(diagnostic, pack))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records.join("\n\n"))
}

struct DiagnosticDisplayText {
    message: String,
    help: Option<String>,
}

struct SanitizedDiagnosticText {
    message: Option<String>,
    help: Option<String>,
}

impl SanitizedDiagnosticText {
    fn from_display_text(text: Option<&DiagnosticDisplayText>) -> Self {
        Self {
            message: text.map(|text| sanitize_diagnostic_text(&text.message)),
            help: text
                .and_then(|text| text.help.as_ref())
                .map(|help| sanitize_diagnostic_text(help)),
        }
    }

    fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }
}

fn diagnostic_display_text(
    diag: &Diagnostic,
    pack: Option<&LocalePack>,
) -> Result<Option<DiagnosticDisplayText>, DiagnosticTemplateRenderError> {
    // PKG001 is the package-layer catch-all code: most of its issues carry no
    // pack row (see the compact-header note below), and the `PKG001` base row
    // exists only to hold issue rows. A PKG001 diagnostic is rendered from the
    // pack only when its own issue row exists; every other PKG001 diagnostic
    // keeps the pack-less behavior, so the base row never replaces an
    // issue-less message and never adds a "missing row" note to a rowless issue.
    let pack = pack.filter(|pack| diag.code != Some("PKG001") || pack_has_pkg001_issue_row(pack, diag));
    if let Some(pack) = pack {
        match pack.render_diagnostic_text(diag) {
            Ok(Some(rendered)) => {
                return Ok(Some(DiagnosticDisplayText { message: rendered.message, help: rendered.help }));
            }
            Ok(None) => {}
            // A template placeholder the emit site did not supply must not
            // abort the whole run: show the base message and
            // name the missing placeholder so the pack defect stays visible.
            Err(DiagnosticTemplateRenderError::MissingPlaceholder { code, field, placeholder }) => {
                let field = match field {
                    TemplateField::Message => "message",
                    TemplateField::Help => "help",
                };
                return Ok(Some(DiagnosticDisplayText {
                    message: format!(
                        "{} [locale pack template missing placeholder: {code}.{field} {{{placeholder}}}]",
                        diag.message
                    ),
                    help: diag.help.clone(),
                }));
            }
            Err(err) => return Err(err),
        }
    }

    // Need: PKG001 arms carry ad-hoc prose with no catalog row and
    // no locale-pack template, so an issue-less PKG001 must render its message.
    // An issue'd PKG001 stays compact: the reader-locale contract in
    // crates/faber/tests/run_integration_test.rs
    // (reader_locale_missing_package_path_surfaces_missing_path_error) forbids
    // the raw io prose of `package_path_missing` from rendering.
    if diag.code.is_none()
        || diag.phase == DiagnosticPhase::Codegen
        || (diag.code == Some("PKG001") && diag.issue().is_none())
    {
        return Ok(Some(DiagnosticDisplayText {
            message: diag.message.clone(),
            help: diag.help.clone(),
        }));
    }

    Ok(None)
}

/// True when `pack` carries a `PKG001` issue row for the diagnostic's issue.
fn pack_has_pkg001_issue_row(pack: &LocalePack, diag: &Diagnostic) -> bool {
    diag.issue().is_some_and(|issue| {
        pack.diagnostics
            .get("PKG001")
            .is_some_and(|template| template.issues.contains_key(issue))
    })
}

fn report_kind(severity: Severity) -> ReportKind<'static> {
    match severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Info => ReportKind::Advice,
    }
}

fn label_color(severity: Severity) -> Color {
    match severity {
        Severity::Error => Color::Red,
        Severity::Warning => Color::Yellow,
        Severity::Info => Color::Blue,
    }
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

// Missing source is valid for backend and filesystem diagnostics.
fn source_for_diagnostic<'a>(diag: &Diagnostic, sources: &'a [(String, String)]) -> &'a str {
    sources
        .iter()
        .find(|(name, _)| name == &diag.file)
        .map_or("", |(_, content)| content.as_str())
}

fn pretty_report_message(label: Option<&str>, message: Option<&str>) -> String {
    match (label, message) {
        (Some(label), Some(message)) => format!("[{label}] {message}"),
        (Some(label), None) => format!("[{label}]"),
        (None, Some(message)) => message.to_owned(),
        (None, None) => String::new(),
    }
}

fn plain_header(severity: &str, label: Option<&str>, location: &str, message: Option<&str>) -> String {
    match (label, message) {
        // A file-less diagnostic has no location; leave the slot out rather than
        // printing an empty `: :` between the identity and the message.
        (Some(label), Some(message)) if location.is_empty() => format!("{severity}[{label}]: {message}"),
        (Some(label), Some(message)) => format!("{severity}[{label}]: {location}: {message}"),
        (Some(label), None) => format!("{severity}[{label}]: {location}"),
        (None, Some(message)) => format!("{severity}: {location}: {message}"),
        (None, None) => format!("{severity}: {location}"),
    }
}

fn expanded_header(
    severity: &str,
    label: &str,
    phase: impl std::fmt::Display,
    file: &str,
    span: &str,
    message: Option<&str>,
) -> String {
    match message {
        Some(message) => {
            format!("{severity}[{label}] {phase} {file}: {message}\nphase: {phase}\nfile: {file}\nspan: {span}")
        }
        None => format!("{severity}[{label}] {phase} {file}\nphase: {phase}\nfile: {file}\nspan: {span}"),
    }
}

fn bidi_isolate_code(text: &str) -> String {
    format!("{FIRST_STRONG_ISOLATE}{text}{POP_DIRECTIONAL_ISOLATE}")
}

/// Longest SGR parameter run the rendered-output sanitizer passes intact.
const MAX_SGR_PARAMS: usize = 32;

/// Escape C0/C1/DEL and bidi controls in source-derived snippet text as
/// `U+XXXX` (the `diagnostic_char` shape) before terminal output.
fn sanitize_snippet_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() || is_bidi_control(ch) {
            write!(out, "U+{:04X}", ch as u32).ok();
        } else {
            out.push(ch);
        }
    }
    out
}

/// Escape C0/C1/DEL and bidi controls in ariadne's rendered report while
/// passing the renderer's own SGR color sequences and layout newlines through
/// intact; every other escape use (OSC, cursor movement, device queries)
/// loses its ESC.
fn sanitize_rendered_output(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        let sgr_len = if ch == '\u{1B}' { sgr_sequence_len(rest) } else { 0 };
        if sgr_len > 0 {
            out.push_str(&rest[..sgr_len]);
            rest = &rest[sgr_len..];
            continue;
        }
        if ch != '\n' && (ch.is_control() || is_bidi_control(ch)) {
            write!(out, "U+{:04X}", ch as u32).ok();
        } else {
            out.push(ch);
        }
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// Length of a well-formed SGR sequence (`ESC [ <digits/;> m`) at the start of
/// `text`, or 0 when the ESC begins anything else or the run is unterminated.
fn sgr_sequence_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&0x1B) || bytes.get(1) != Some(&b'[') {
        return 0;
    }
    let mut params = 0;
    let mut i = 2;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' | b';' => {
                params += 1;
                i += 1;
            }
            b'm' => return if params <= MAX_SGR_PARAMS { i + 1 } else { 0 },
            _ => return 0,
        }
    }
    0
}

fn sanitize_diagnostic_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if is_bidi_control(ch) {
            write!(out, "U+{:04X}", ch as u32).ok();
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
#[path = "render_test.rs"]
mod tests;
