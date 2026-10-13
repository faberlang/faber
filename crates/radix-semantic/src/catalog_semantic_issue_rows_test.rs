//! Completeness gate: every issue slug the semantic passes emit has an `en`
//! reader-pack message row.
//!
//! The reader pack looks a row up by `(code, issue)`. A slug with no row falls
//! back to the code's base text with a `[locale pack missing row: ...]` suffix,
//! so `faber check --diagnostics` loses the specific message. The gate scans the
//! production sources of this crate for slug literals in the four emission
//! shapes the passes use and fails with the slugs that no `en` row names.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::SemanticErrorKind;
use radix_diagnostics::DiagnosticArg;

const EN_PACK: &str = "../../locale/en/pack.toml";

fn production_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            production_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_test.rs") || name.ends_with("_tests.rs"))
        {
            out.push(path);
        }
    }
}

/// The quoted slug starting at the first `"` of `text`, when it is a plain
/// lowercase identifier.
fn leading_slug(text: &str) -> Option<&str> {
    let body = text.trim_start().strip_prefix('"')?;
    let slug = &body[..body.find('"')?];
    (!slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
    .then_some(slug)
}

/// The slug of a `reject_with_arg(span, "slug", ...)` call, given the text
/// after its opening paren: the first string literal after the span argument,
/// which may sit on a later line.
fn reject_with_arg_slug(call_rest: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (at, ch) in call_rest.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => return leading_slug(&call_rest[at + 1..]),
            _ => {}
        }
    }
    None
}

fn emitted_issue_slugs(source: &str) -> BTreeSet<String> {
    let mut slugs = BTreeSet::new();
    let lines: Vec<&str> = source.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        // `const ISSUE_X: &str = "slug";` (value may sit on the next line).
        if let Some(rest) = line.split("const ISSUE_").nth(1)
            && let Some(value) = rest.split(": &str =").nth(1)
        {
            let value = if value.trim().is_empty() {
                lines.get(index + 1).copied().unwrap_or("")
            } else {
                value
            };
            if let Some(slug) = leading_slug(value) {
                slugs.insert(slug.to_owned());
            }
        }
        // `("issue", "slug")` / `.with_arg("issue", "slug")`.
        for (at, _) in line.match_indices("\"issue\",") {
            if let Some(slug) = leading_slug(&line[at + "\"issue\",".len()..]) {
                slugs.insert(slug.to_owned());
            }
        }
        // `undefined_member("slug", span)`.
        for (at, _) in line.match_indices("undefined_member(") {
            if let Some(slug) = leading_slug(&line[at + "undefined_member(".len()..]) {
                slugs.insert(slug.to_owned());
            }
        }
    }
    // `reject_with_arg(span, "slug", "name", value)`; the arguments usually sit
    // on their own lines, so this shape reads the whole source, not one line.
    for (at, _) in source.match_indices("reject_with_arg(") {
        if let Some(slug) = reject_with_arg_slug(&source[at + "reject_with_arg(".len()..]) {
            slugs.insert(slug.to_owned());
        }
    }
    slugs
}

fn en_pack_issue_rows() -> BTreeSet<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(manifest.join(EN_PACK)).expect("read en pack");
    let pack: toml::Table = text.parse().expect("en pack parses");
    let diagnostics = pack
        .get("diagnostics")
        .and_then(toml::Value::as_table)
        .expect("en pack has a diagnostics table");
    diagnostics
        .values()
        .filter_map(|code| code.get("issues").and_then(toml::Value::as_table))
        .flat_map(|issues| issues.keys().cloned())
        .collect()
}

#[test]
fn every_emitted_semantic_issue_slug_has_an_en_message_row() {
    let mut files = Vec::new();
    production_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut emitted = BTreeSet::new();
    for file in &files {
        emitted.extend(emitted_issue_slugs(
            &fs::read_to_string(file).expect("read source"),
        ));
    }
    assert!(
        emitted.len() > 300,
        "slug scan found only {} slugs; the scanner is broken",
        emitted.len()
    );

    let rows = en_pack_issue_rows();
    let missing: Vec<&String> = emitted
        .iter()
        .filter(|slug| !rows.contains(*slug))
        .collect();
    assert!(
        missing.is_empty(),
        "issue slugs emitted by radix-semantic with no en pack row (locale/en/pack.toml): {missing:?}"
    );
}

#[test]
fn scanner_reads_the_slug_of_a_one_line_reject_with_arg_call() {
    let source = r#"self.reject_with_arg(span, "some_slug", "method", &method);"#;
    assert_eq!(
        emitted_issue_slugs(source),
        BTreeSet::from(["some_slug".to_owned()])
    );
}

#[test]
fn scanner_reads_the_slug_of_a_split_reject_with_arg_call() {
    let source = r#"
        self.reject_with_arg(
            expr.span,
            "split_slug",
            "causa",
            "not_bounded",
        );
    "#;
    assert_eq!(
        emitted_issue_slugs(source),
        BTreeSet::from(["split_slug".to_owned()])
    );
}

#[test]
fn scanner_skips_the_reject_with_arg_definition() {
    let source = r#"
    fn reject_with_arg(
        &mut self,
        span: Span,
        issue: &'static str,
        name: &'static str,
        value: impl Into<String>,
    ) {
    }
    "#;
    assert!(emitted_issue_slugs(source).is_empty());
}

/// The slug scan above covers the `en` row for `tensor_method_receiver`; the
/// catalog arm still needs its own proof: an issue with no
/// `air_purity_reason` arm leaves the help renderer at its fallthrough and the
/// diagnostic loses its baseline help.
#[test]
fn air_purity_tensor_method_receiver_issue_renders_through_the_catalog() {
    let args = [
        DiagnosticArg::new("issue", "tensor_method_receiver"),
        DiagnosticArg::new("lane", "air"),
        DiagnosticArg::new("method", "gelu"),
    ];
    let help = super::semantic_help(SemanticErrorKind::AirPurityViolation, &args)
        .expect("catalog recognizes the tensor_method_receiver air-purity issue");
    assert_eq!(help, super::AIR_PURITY_HELP);
}
