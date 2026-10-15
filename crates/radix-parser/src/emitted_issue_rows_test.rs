//! Completeness gate: every issue slug the parser emits has an `en` reader-pack
//! message row.
//!
//! The reader pack looks a row up by `(code, issue)`. A slug with no row falls
//! back to the code's base text with a `[locale pack missing row: ...]` suffix,
//! so `faber check --diagnostics` loses the specific message. This is the
//! parser-side counterpart of `radix-semantic`'s
//! `catalog_semantic_issue_rows_test.rs`; `PARSE001.item_list_missing_separator`
//! went rowless because only the semantic crate had its gate.
//!
//! The scan reads the `"issue", "slug"` argument pair of `with_arg` calls,
//! including calls split over lines. Call sites that pass a variable are out of
//! reach and are skipped.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const EN_PACK: &str = "../../locale/en/pack.toml";

// Pre-existing gaps the scan reports today.
// debt: shrink only; need
const KNOWN_MISSING_ROWS: &[&str] = &[
    "annotation_field_name",
    "declaration_block_comma",
    "duplicate_nucleum",
    "enum_value_out_of_range",
    "exitus_value_out_of_range",
    "expression_too_long",
    "label_bind_distinction",
    "labeled_actual_spread_unsupported",
    "nesting_too_deep",
    "nucleum_fragment_conflicts_with_fragment",
    "promissum_alternate_uses_exit_arrow",
    "ratio_field_list_hole_not_sole",
    "ratio_type_arg_label_required",
    "test_modifier_integer_out_of_range",
    "unexpected_cap_after_union_hole",
    "unexpected_cup_after_union_hole",
];

fn production_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            production_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.ends_with("_test.rs")
                        || name.ends_with("_tests.rs")
                        || name == "test_support.rs"
                })
        {
            out.push(path);
        }
    }
}

fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The slug of the `"issue", "slug"` pair starting after the `"issue"` token,
/// when the next non-whitespace token is a comma and the literal after it is a
/// plain lowercase identifier.
fn issue_arg_slug(after_issue: &str) -> Option<&str> {
    let rest = after_issue.trim_start().strip_prefix(',')?.trim_start();
    let literal = rest.strip_prefix('"')?;
    let slug = &literal[..literal.find('"')?];
    is_slug(slug).then_some(slug)
}

fn emitted_issue_slugs(source: &str) -> BTreeSet<String> {
    let needle = "\"issue\"";
    let mut slugs = BTreeSet::new();
    let mut rest = source;
    while let Some(at) = rest.find(needle) {
        let after = &rest[at + needle.len()..];
        if let Some(slug) = issue_arg_slug(after) {
            slugs.insert(slug.to_owned());
        }
        rest = after;
    }
    slugs
}

/// Every issue key under `[diagnostics.<CODE>.issues.<slug>]`, scanned as lines
/// because this crate does not depend on `toml`.
fn en_pack_issue_rows() -> BTreeSet<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(manifest.join(EN_PACK)).expect("read en pack");
    let mut rows = BTreeSet::new();
    for line in text.lines() {
        let Some(header) = line.trim_start().strip_prefix("[diagnostics.") else {
            continue;
        };
        let Some(header) = header.split(']').next() else {
            continue;
        };
        let mut parts = header.split('.');
        let (Some(code), Some("issues"), Some(slug), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if !code.is_empty() && is_slug(slug) {
            rows.insert(slug.to_owned());
        }
    }
    rows
}

#[test]
fn emitted_issue_rows_all_have_en_pack_rows() {
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
        emitted.len() > 50,
        "slug scan found only {} slugs; the scanner is broken",
        emitted.len()
    );
    assert!(
        !KNOWN_MISSING_ROWS.contains(&"item_list_missing_separator")
            && !KNOWN_MISSING_ROWS.contains(&"item_list_trailing_comma"),
        "need landed; the item_list slugs no longer belong in the allowlist"
    );

    let rows = en_pack_issue_rows();
    let missing: Vec<&String> = emitted
        .iter()
        .filter(|slug| !rows.contains(*slug) && !KNOWN_MISSING_ROWS.contains(&slug.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "issue slugs emitted by radix-parser with no en pack row (locale/en/pack.toml): {missing:?}"
    );
}

/// The allowlist cannot rot: a slug with a row back must be removed from it.
#[test]
fn emitted_issue_rows_allowlist_stays_absent_from_the_en_pack() {
    let rows = en_pack_issue_rows();
    let stale: Vec<&&str> = KNOWN_MISSING_ROWS
        .iter()
        .filter(|slug| rows.contains(**slug))
        .collect();
    assert!(
        stale.is_empty(),
        "KNOWN_MISSING_ROWS slugs now have an en pack row; remove them: {stale:?}"
    );
}

#[test]
fn emitted_issue_rows_scanner_reads_split_calls_and_skips_variables() {
    let source = r#"
        .with_arg("issue", "item_list_missing_separator")
        .with_arg(
            "issue",
            "item_list_trailing_comma",
        )
        .with_arg("issue", issue)
        .with_arg("issue", "NotASlug")
    "#;
    assert_eq!(
        emitted_issue_slugs(source),
        BTreeSet::from([
            "item_list_missing_separator".to_owned(),
            "item_list_trailing_comma".to_owned(),
        ])
    );
}
