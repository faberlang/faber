//! The paired rounding delimiters: `⌊x⌋` floor, `⌈x⌉` ceiling, `⌊x⌉` nearest
//!. `⌈x⌋` records `PARSE030 ceiling_floor_delimiters` and
//! still parses (as nearest) so recovery continues.

use super::ParseResult;
use crate::test_support::parse_program_la;

fn parse_source(source: &str) -> ParseResult {
    parse_program_la(source)
}

#[test]
fn rounding_glyph_forms_parse_without_errors() {
    for source in [
        "incipit { sit _ a ← ⌊x⌋ }",
        "incipit { sit _ a ← ⌈x⌉ }",
        "incipit { sit _ a ← ⌊x + 1.5⌉ }",
        "incipit { sit _ a ← ⌊⌈x⌉⌋ }",
    ] {
        let result = parse_source(source);
        assert!(result.errors.is_empty(), "{source}: {:?}", result.errors);
    }
}

#[test]
fn ceiling_opened_floor_closed_is_a_named_error() {
    let result = parse_source("incipit { sit _ a ← ⌈x⌋ }");
    let named = result.errors.iter().any(|err| {
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "ceiling_floor_delimiters")
    });
    assert!(named, "errors: {:?}", result.errors);
}

#[test]
fn unclosed_rounding_glyph_is_an_error() {
    let result = parse_source("incipit { sit _ a ← ⌊x }");
    assert!(!result.errors.is_empty());
}
