//! Parse exempla `.fab` files into [`crate::explain::Entry`] values.

use crate::explain::{Entry, ExplainError, Kind};
use radix::module::driver::split_frontmatter;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct ExemplaFrontmatter {
    term: Option<String>,
    kind: Option<String>,
    category: Option<String>,
    canonical: Option<bool>,
    canonical_term: Option<String>,
    locale: Option<String>,
    summary: Option<String>,
    syntax: Option<String>,
    #[serde(default)]
    grammar: Vec<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    related: Vec<String>,
}

/// Parse one exempla file into an explain registry entry.
///
/// # Errors
///
/// Returns an error when the frontmatter is missing or cannot be split, when it
/// does not parse as TOML, when it lacks the `kind`, `category`, `summary` or
/// `syntax` field, when `kind` is unknown, or when the assembled entry fails
/// validation.
pub fn entry_from_exempla(
    path_label: &str,
    source: &str,
    term_override: &str,
) -> Result<Entry, ExplainError> {
    let split = split_frontmatter(source)
        .map_err(|error| ExplainError::new(format!("{path_label}: {error}")))?;

    let Some(frontmatter_text) = split.frontmatter_text else {
        return Err(ExplainError::new(format!(
            "{path_label}: missing +++ frontmatter"
        )));
    };

    let fm: ExemplaFrontmatter = toml::from_str(frontmatter_text)
        .map_err(|error| ExplainError::new(format!("{path_label}: {error}")))?;

    let kind_value = fm
        .kind
        .as_deref()
        .ok_or_else(|| ExplainError::new(format!("{path_label}: frontmatter missing kind")))?;
    let category = fm
        .category
        .ok_or_else(|| ExplainError::new(format!("{path_label}: frontmatter missing category")))?;
    let summary =
        normalize_summary(fm.summary.as_deref().ok_or_else(|| {
            ExplainError::new(format!("{path_label}: frontmatter missing summary"))
        })?);
    let syntax = fm
        .syntax
        .ok_or_else(|| ExplainError::new(format!("{path_label}: frontmatter missing syntax")))?;

    let source_locale = fm.locale.filter(|locale| locale != "en");
    let (comment_header, fab_source) = split_header_and_source(split.body);
    let body = build_entry_body(&comment_header, &fab_source, source_locale.as_deref());

    let aliases = if fm.term.as_deref() == Some(term_override) {
        fm.aliases
    } else {
        Vec::new()
    };

    let entry = Entry {
        term: term_override.to_owned(),
        kind: parse_exempla_kind(path_label, kind_value)?,
        category,
        canonical: fm.canonical.unwrap_or(true),
        summary,
        syntax,
        examples: Vec::new(),
        grammar: fm.grammar,
        aliases,
        legacy: Vec::new(),
        canonical_term: fm.canonical_term,
        related: fm.related,
        body,
    };

    validate_exempla_entry(path_label, &entry)?;
    Ok(entry)
}

/// Build a legacy explain entry from a redirect row and its canonical target.
///
/// # Errors
///
/// Returns an error if the constructed entry fails validation: empty syntax or
/// summary, an empty body, or a body without a fab code example.
pub fn legacy_entry_from_redirect(
    redirect_term: &str,
    canonical: &str,
    message: &str,
    canonical_entry: &Entry,
) -> Result<Entry, ExplainError> {
    let path_label = format!("legacy-redirects.toml:{redirect_term}");
    let body = legacy_body(redirect_term, canonical, canonical_entry);

    let entry = Entry {
        term: redirect_term.to_owned(),
        kind: Kind::Legacy,
        category: canonical_entry.category.clone(),
        canonical: false,
        summary: normalize_summary(message),
        syntax: canonical_entry.syntax.clone(),
        examples: Vec::new(),
        grammar: Vec::new(),
        aliases: Vec::new(),
        legacy: Vec::new(),
        canonical_term: Some(canonical.to_owned()),
        related: canonical_entry.related.clone(),
        body,
    };

    validate_exempla_entry(&path_label, &entry)?;
    Ok(entry)
}

fn build_entry_body(comment_header: &str, fab_source: &str, locale: Option<&str>) -> String {
    let mut body = String::new();
    let prose = comment_header_to_prose(comment_header);
    if !prose.is_empty() {
        body.push_str(&prose);
        body.push_str("\n\n");
    }

    let fab_source = fab_source.trim();
    if !fab_source.is_empty() {
        let example = with_source_locale_frontmatter(fab_source, locale);
        push_fab_block(&mut body, &example);
    }

    body
}

/// Prefix the example with a source frontmatter when the exemplum declares a
/// non-en reader locale, so the extracted block is a complete file that
/// selects its own reader locale from frontmatter instead of parsing as en.
fn with_source_locale_frontmatter(source: &str, locale: Option<&str>) -> String {
    match locale {
        Some(locale) => format!("+++\nlocale = \"{locale}\"\n+++\n{source}"),
        None => source.to_owned(),
    }
}

/// Append a fenced `fab` block, guaranteeing a trailing newline inside the
/// fence even when `code` does not end in one.
fn push_fab_block(body: &mut String, code: &str) {
    body.push_str("```fab\n");
    body.push_str(code);
    if !code.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("```\n");
}

fn legacy_body(redirect_term: &str, canonical: &str, canonical_entry: &Entry) -> String {
    let mut body =
        format!("`{redirect_term}` is not canonical Faber source. Use `{canonical}`.\n\n");
    if let Some(example) = first_fab_block(&canonical_entry.body) {
        push_fab_block(&mut body, example);
    } else if !canonical_entry.body.trim().is_empty() {
        body.push_str(&canonical_entry.body);
    }
    body
}

fn split_header_and_source(body: &str) -> (String, String) {
    let mut header_lines = Vec::new();
    let mut source_lines = Vec::new();
    let mut past_header = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if !past_header && (trimmed.is_empty() || trimmed.starts_with('#')) {
            header_lines.push(line);
            continue;
        }
        past_header = true;
        source_lines.push(line);
    }

    (header_lines.join("\n"), source_lines.join("\n"))
}

fn comment_header_to_prose(header: &str) -> String {
    let mut lines = Vec::new();
    for line in header.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix('#') else {
            continue;
        };
        let content = rest.trim_start();
        if !content.is_empty() {
            lines.push(content.to_string());
        }
    }
    lines.join("\n")
}

fn first_fab_block(body: &str) -> Option<&str> {
    let start = body.find("```fab")?;
    let code_start = body[start..].find('\n')? + start + 1;
    let code_end = body[code_start..].find("```")? + code_start;
    Some(body[code_start..code_end].trim())
}

fn normalize_summary(summary: &str) -> String {
    let summary = summary.trim().to_owned();
    if summary.ends_with('.') {
        summary
    } else {
        format!("{summary}.")
    }
}

fn parse_exempla_kind(path_label: &str, value: &str) -> Result<Kind, ExplainError> {
    match value {
        "keyword" => Ok(Kind::Keyword),
        "operator" | "operator-group" => Ok(Kind::Operator),
        "annotation" => Ok(Kind::Annotation),
        "literal" => Ok(Kind::Literal),
        "type" | "existing-home" => Ok(Kind::Type),
        "modifier" => Ok(Kind::Modifier),
        "legacy" => Ok(Kind::Legacy),
        "concept" => Ok(Kind::Concept),
        "conversion" | "conversio" => Ok(Kind::Conversion),
        other => Err(ExplainError::new(format!(
            "{path_label}: unknown kind {other}"
        ))),
    }
}

fn validate_exempla_entry(path_label: &str, entry: &Entry) -> Result<(), ExplainError> {
    if entry.syntax.trim().is_empty() {
        return Err(ExplainError::new(format!(
            "{path_label}: syntax must not be empty"
        )));
    }
    if entry.summary.trim().is_empty() {
        return Err(ExplainError::new(format!(
            "{path_label}: summary must not be empty"
        )));
    }
    if entry.body.trim().is_empty() {
        return Err(ExplainError::new(format!(
            "{path_label}: body must not be empty"
        )));
    }
    if first_fab_block(&entry.body).is_none() {
        return Err(ExplainError::new(format!(
            "{path_label}: body must contain a fab code example"
        )));
    }
    Ok(())
}
