//! Tests for `+++` frontmatter delimiter parsing.
//!
//! Contract: frontmatter must start on line 1 with `+++`, be valid TOML, and be terminated by `+++`.

use super::{FrontmatterError, split_frontmatter};

#[test]
fn no_frontmatter_returns_full_source_as_body() {
    let source = "functio ping() → vacuum {}\n";
    let split = split_frontmatter(source).expect("split");
    assert_eq!(split.frontmatter_text, None);
    assert_eq!(split.body, source);
    assert_eq!(split.body_byte_offset, 0);
}

#[test]
fn strips_optional_frontmatter_and_preserves_body() {
    let source = "+++\nsectio = \"smoke\"\n+++\n\nfunctio ping() → vacuum {}\n";
    let split = split_frontmatter(source).expect("split");
    assert_eq!(split.frontmatter_text, Some("sectio = \"smoke\"\n"));
    assert_eq!(split.body, "\nfunctio ping() → vacuum {}\n");
    assert_eq!(
        &source[split.body_byte_offset as usize..],
        split.body,
        "body slice must align with body_byte_offset"
    );
}

#[test]
fn frontmatter_only_file_yields_empty_body() {
    let source = "+++\nsectio = \"smoke\"\n+++\n";
    let split = split_frontmatter(source).expect("split");
    assert_eq!(split.frontmatter_text, Some("sectio = \"smoke\"\n"));
    assert_eq!(split.body, "");
    assert_eq!(split.body_byte_offset as usize, source.len());
}

#[test]
fn empty_toml_between_delimiters_is_valid() {
    let source = "+++\n+++\nincipit {}\n";
    let split = split_frontmatter(source).expect("split");
    assert_eq!(split.frontmatter_text, Some(""));
    assert_eq!(split.body, "incipit {}\n");
}

#[test]
fn rejects_comment_before_opening_delimiter() {
    let source = "# not allowed\n+++\nsectio = \"smoke\"\n+++\n";
    let err = split_frontmatter(source).expect_err("comment before +++");
    assert_eq!(err, FrontmatterError::OpeningDelimiterNotFirstLine);
}

#[test]
fn rejects_whitespace_before_opening_delimiter() {
    let source = " +++\nsectio = \"smoke\"\n+++\n";
    let err = split_frontmatter(source).expect_err("space-padded delimiter is not line 1");
    assert_eq!(err, FrontmatterError::OpeningDelimiterNotFirstLine);
}

#[test]
fn rejects_byte_order_mark() {
    let source = "\u{feff}+++\nsectio = \"smoke\"\n+++\n";
    let err = split_frontmatter(source).expect_err("bom");
    assert_eq!(err, FrontmatterError::ByteOrderMark);
}

#[test]
fn rejects_unterminated_frontmatter() {
    let source = "+++\nsectio = \"smoke\"\n";
    let err = split_frontmatter(source).expect_err("unterminated");
    assert_eq!(err, FrontmatterError::Unterminated);
}

#[test]
fn accepts_crlf_delimiters() {
    let source = "+++\r\nsectio = \"smoke\"\r\n+++\r\nincipit {}\r\n";
    let split = split_frontmatter(source).expect("split");
    assert_eq!(split.frontmatter_text, Some("sectio = \"smoke\"\r\n"));
    assert_eq!(split.body, "incipit {}\r\n");
}

#[test]
fn rejects_yaml_style_delimiters() {
    let source = "---\nsectio = \"smoke\"\n---\nincipit {}\n";
    let split = split_frontmatter(source).expect("split");
    assert_eq!(split.frontmatter_text, None);
    assert_eq!(split.body, source);
}

#[test]
fn empty_source_returns_empty_body() {
    let source = "";
    let split = split_frontmatter(source).expect("split");
    assert_eq!(split.frontmatter_text, None);
    assert_eq!(split.body, "");
    assert_eq!(split.body_byte_offset, 0);
}

#[test]
fn body_with_leading_ppp_not_confused_as_frontmatter() {
    // "+++" appearing in the body (not on line 1) must not be treated as
    // frontmatter delimiter. Since the opening delimiter must be on line 1,
    // this source has no frontmatter.
    let source = "functio ping() → vacuum {}\n+++not frontmatter\n";
    let split = split_frontmatter(source).expect("split");
    assert_eq!(split.frontmatter_text, None);
    assert_eq!(split.body, source);
}

#[test]
fn only_newline_before_opening_delimiter_rejected() {
    let source = "\n+++\nsectio = \"x\"\n+++\n";
    let err = split_frontmatter(source).expect_err("newline before +++");
    assert_eq!(err, FrontmatterError::OpeningDelimiterNotFirstLine);
}
