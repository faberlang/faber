//! Diagnostic-code lookup for `faber explain`.
//!
//! Language reference lookup is backed by the exempla reference pack. Diagnostic
//! lookup is backed by reader packs because human diagnostic prose is locale
//! data, not compiler transport data.

use crate::explain::ExplainError;
use crate::io_buf::writeln_buf;
use radix::reader_locale::{DiagnosticTemplate, ReaderLocalePack};
use serde::Serialize;
use std::path::{Path, PathBuf};

const DEFAULT_READER_LOCALE: &str = "la";

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticLookupKey {
    code: String,
    issue: Option<String>,
}

/// Renderable diagnostic explanation resolved from one reader pack row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticExplanation {
    /// Original query supplied by the user.
    pub query: String,

    /// Stable diagnostic code such as `SEM010`.
    pub code: String,

    /// Optional issue row selected under the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,

    /// Reader locale pack that supplied this explanation.
    pub reader_locale: String,

    /// Pack-owned message template. Placeholder braces are kept when the
    /// explanation is not tied to a concrete diagnostic instance.
    pub message: String,

    /// Pack-owned help template, if the row defines one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl DiagnosticExplanation {
    fn from_template(
        query: &str,
        key: DiagnosticLookupKey,
        pack: &ReaderLocalePack,
        template: &DiagnosticTemplate,
    ) -> Self {
        Self {
            query: query.to_owned(),
            code: key.code,
            issue: key.issue,
            reader_locale: pack.metadata.id.clone(),
            message: template.message.clone(),
            help: template.help.clone(),
        }
    }

    pub fn lookup_key(&self) -> String {
        match self.issue.as_deref() {
            Some(issue) => format!("{}.{}", self.code, issue),
            None => self.code.clone(),
        }
    }
}

/// Return whether `query` has diagnostic-code lookup syntax.
pub fn is_diagnostic_query(query: &str) -> bool {
    DiagnosticLookupKey::parse(query).is_some()
}

/// Resolve a diagnostic explanation from an installed reader pack.
pub fn lookup_installed_diagnostic(
    query: &str,
    reader_locale: Option<&str>,
) -> Result<Option<DiagnosticExplanation>, ExplainError> {
    let Some(key) = DiagnosticLookupKey::parse(query) else {
        return Ok(None);
    };
    let locale = reader_locale.unwrap_or(DEFAULT_READER_LOCALE);
    let pack = load_installed_reader_pack(locale)?;
    Ok(resolve_diagnostic_in_pack(query, &pack, key))
}

/// Resolve a diagnostic explanation from an already loaded reader pack.
pub fn lookup_diagnostic_in_pack(
    query: &str,
    pack: &ReaderLocalePack,
) -> Option<DiagnosticExplanation> {
    let key = DiagnosticLookupKey::parse(query)?;
    resolve_diagnostic_in_pack(query, pack, key)
}

fn resolve_diagnostic_in_pack(
    query: &str,
    pack: &ReaderLocalePack,
    key: DiagnosticLookupKey,
) -> Option<DiagnosticExplanation> {
    let code_template = pack.diagnostics.get(&key.code)?;
    let selected = match key.issue.as_deref() {
        Some(issue) => code_template.issues.get(issue)?,
        None => code_template,
    };
    Some(DiagnosticExplanation::from_template(
        query, key, pack, selected,
    ))
}

/// Render one diagnostic explanation for terminal output.
pub fn render_plain(explanation: &DiagnosticExplanation) -> String {
    let mut out = String::new();
    header(&mut out, &explanation.lookup_key());
    let name = format!(
        "{} - diagnostic {}",
        explanation.lookup_key(),
        explanation.message
    );
    section(&mut out, "NAME", [name.as_str()]);
    section(&mut out, "KIND", ["diagnostic"]);
    section(
        &mut out,
        "READER LOCALE",
        [explanation.reader_locale.as_str()],
    );
    section(&mut out, "MESSAGE", [explanation.message.as_str()]);

    if let Some(help) = explanation.help.as_deref() {
        section(&mut out, "HELP", [help]);
    }

    let mut lookup = vec![format!("code: {}", explanation.code)];
    if let Some(issue) = explanation.issue.as_deref() {
        lookup.push(format!("issue: {issue}"));
    }
    let lookup = lookup.join("\n");
    section(&mut out, "LOOKUP", [lookup.as_str()]);
    out
}

/// Render one diagnostic explanation as JSON.
pub fn render_json(explanation: &DiagnosticExplanation) -> Result<String, ExplainError> {
    serde_json::to_string_pretty(explanation)
        .map_err(|err| ExplainError::new(format!("failed to render diagnostic JSON: {err}")))
}

fn load_installed_reader_pack(locale: &str) -> Result<ReaderLocalePack, ExplainError> {
    if locale.trim().is_empty() {
        return Err(ExplainError::new("--reader-locale must not be empty"));
    }
    let path = installed_reader_pack_path(locale.trim());
    let pack = ReaderLocalePack::from_toml_path(&path).map_err(|err| {
        ExplainError::new(format!(
            "failed to load reader locale '{}' pack '{}': {err}",
            locale,
            path.display()
        ))
    })?;
    if pack.metadata.id != locale {
        return Err(ExplainError::new(format!(
            "reader locale '{}' selected pack '{}' with id '{}'",
            locale,
            path.display(),
            pack.metadata.id
        )));
    }
    Ok(pack)
}

fn installed_reader_pack_path(locale: &str) -> PathBuf {
    normalize_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../radix/stdlib")
            .join("reader")
            .join(locale)
            .join("pack.toml"),
    )
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

impl DiagnosticLookupKey {
    fn parse(query: &str) -> Option<Self> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        let (code, issue) = match query.split_once('.') {
            Some((code, issue)) if !issue.is_empty() => (code, Some(issue)),
            Some(_) => return None,
            None => (query, None),
        };
        if !is_diagnostic_code(code) {
            return None;
        }
        if let Some(issue) = issue {
            if !is_diagnostic_issue(issue) {
                return None;
            }
        }
        Some(Self {
            code: code.to_owned(),
            issue: issue.map(str::to_owned),
        })
    }
}

fn is_diagnostic_code(code: &str) -> bool {
    let mut chars = code.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_uppercase()
        && code.chars().any(|ch| ch.is_ascii_digit())
        && code
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
}

fn is_diagnostic_issue(issue: &str) -> bool {
    issue
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn header(out: &mut String, lookup_key: &str) {
    let page = format!("{}(7)", lookup_key.to_uppercase());
    let center = "Faber Diagnostic Reference";
    let width = 78usize;
    let fixed = page.len() * 2 + center.len();
    if fixed + 2 > width {
        writeln_buf(out, format_args!("{page}  {center}"));
        out.push('\n');
        return;
    }

    let spaces = width - fixed;
    let left_spaces = spaces / 2;
    let right_spaces = spaces - left_spaces;
    writeln_buf(
        out,
        format_args!(
            "{}{}{}{}{}",
            page,
            " ".repeat(left_spaces),
            center,
            " ".repeat(right_spaces),
            page
        ),
    );
    out.push('\n');
}

fn section<I, S>(out: &mut String, title: &str, lines: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    writeln_buf(out, title);
    for line in lines {
        for part in line.as_ref().lines() {
            writeln_buf(out, format_args!("    {part}"));
        }
    }
    out.push('\n');
}
