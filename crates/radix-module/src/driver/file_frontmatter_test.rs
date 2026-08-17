//! Tests for parsed file-frontmatter accessors (sectio, group, package, locale, etc.).
//!
//! Contract: accessors return `Some` for present keys of the expected type and `None` for absent or type-mismatched keys.

use super::parse_file_frontmatter;

#[test]
fn empty_frontmatter_text_deserializes_to_empty_table() {
    let parsed = parse_file_frontmatter("").expect("empty");
    assert!(parsed.as_table().is_empty());
    assert_eq!(parsed.group(), None);
    assert_eq!(parsed.sectio(), None);
}

#[test]
fn parses_package_tooling_keys_via_accessors() {
    let parsed = parse_file_frontmatter(
        r#"
group = "exempla.directiva"
sectio = "smoke"

[package]
name = "demo"
version = "0.1.0"

[build]
target = "rust"

[dependentia.norma]
via = "library/norma"

[probanda]
tags = ["smoke", "integration"]
"#,
    )
    .expect("parse");

    assert_eq!(parsed.group(), Some("exempla.directiva"));
    assert_eq!(parsed.sectio(), Some("smoke"));
    assert_eq!(parsed.package_name(), Some("demo"));
    assert_eq!(parsed.build_target(), Some("rust"));
    assert_eq!(parsed.dependentia_via("norma"), Some("library/norma"));
    assert_eq!(parsed.probanda_tags(), vec!["smoke".to_owned(), "integration".to_owned()]);
}

#[test]
fn explain_parity_keys_are_accepted() {
    let parsed = parse_file_frontmatter(
        r#"
term = "discerne"
kind = "keyword"
category = "control-flow"
summary = "Starts an exhaustive pattern match."
syntax = "discerne <subject> <block>"
related = ["discretio", "casu"]
"#,
    )
    .expect("parse");

    assert_eq!(parsed.get("term").and_then(|v| v.as_str()), Some("discerne"));
    assert_eq!(parsed.get("kind").and_then(|v| v.as_str()), Some("keyword"));
    assert_eq!(
        parsed
            .get("related")
            .and_then(|v| v.as_array())
            .map(std::vec::Vec::len),
        Some(2)
    );
    assert_eq!(parsed.group(), None);
}

#[test]
fn package_and_exempla_keys_coexist() {
    let parsed = parse_file_frontmatter(
        r#"
sectio = "smoke"
term = "directiva"
syntax = "fabFile := frontmatter? program"
"#,
    )
    .expect("parse");

    assert_eq!(parsed.sectio(), Some("smoke"));
    assert_eq!(parsed.get("term").and_then(|v| v.as_str()), Some("directiva"));
}

#[test]
fn locale_accessor_returns_present_value() {
    let parsed = parse_file_frontmatter(r#"locale = "zh-Hans""#).expect("parse");
    assert_eq!(parsed.locale(), Some("zh-Hans"));
}

#[test]
fn locale_accessor_returns_none_when_absent() {
    let parsed = parse_file_frontmatter(r#"sectio = "smoke""#).expect("parse");
    assert_eq!(parsed.locale(), None);
}

#[test]
fn locale_result_returns_ok_for_valid_string() {
    let parsed = parse_file_frontmatter(r#"locale = "zh-Hans""#).expect("parse");
    assert_eq!(parsed.locale_result(), Some(Ok("zh-Hans")));
}

#[test]
fn locale_result_returns_err_for_non_string() {
    let parsed = parse_file_frontmatter("locale = 42").expect("parse");
    assert_eq!(parsed.locale_result(), Some(Err(())));
}

#[test]
fn locale_result_returns_none_when_absent() {
    let parsed = parse_file_frontmatter(r#"sectio = "smoke""#).expect("parse");
    assert_eq!(parsed.locale_result(), None);
}

#[test]
fn group_accessor_returns_none_for_non_string_value() {
    let parsed = parse_file_frontmatter("group = 42").expect("parse");
    assert_eq!(parsed.group(), None);
}

#[test]
fn sectio_accessor_returns_none_for_non_string_value() {
    let parsed = parse_file_frontmatter("sectio = [\"a\", \"b\"]").expect("parse");
    assert_eq!(parsed.sectio(), None);
}

#[test]
fn package_name_accessor_returns_none_for_empty_table() {
    let parsed = parse_file_frontmatter("[package]").expect("parse");
    assert_eq!(parsed.package_name(), None);
}
