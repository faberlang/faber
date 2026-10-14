//! Registry-direct parse-issue rendering (no installed pack).
//!
//! Unit bf951eff migrated keyword sites to structured issues; these tests
//! pin the catalog_syntax registry-direct arms so a render without an
//! installed pack still names the expected word.

use radix_diagnostics::DiagnosticArg;
use radix_parser::ParseErrorKind;
use radix_semantic::parse_message;

fn args(pairs: &[(&'static str, &'static str)]) -> Vec<DiagnosticArg> {
    pairs
        .iter()
        .map(|(name, value)| DiagnosticArg::new(name, *value))
        .collect()
}

#[test]
fn expected_keyword_registry_direct_names_the_expected_keyword() {
    let rendered = parse_message(
        ParseErrorKind::Expected,
        &args(&[("issue", "expected_keyword"), ("keyword", "si")]),
    );
    let rendered = rendered.expect("expected_keyword must render registry-direct");
    assert!(
        rendered.contains("'si'"),
        "expected_keyword must name the expected keyword: {rendered}"
    );
}

#[test]
fn lege_lineam_registry_direct_names_both_particles() {
    let rendered = parse_message(
        ParseErrorKind::Expected,
        &args(&[
            ("issue", "lege_lineam"),
            ("lege", "lege"),
            ("lineam", "lineam"),
        ]),
    );
    let rendered = rendered.expect("lege_lineam must render registry-direct");
    assert!(
        rendered.contains("'lege'") && rendered.contains("'lineam'"),
        "lege_lineam must name both particles: {rendered}"
    );
}

#[test]
fn reducta_source_registry_direct_has_an_arm() {
    let rendered = parse_message(
        ParseErrorKind::Expected,
        &args(&[("issue", "reducta_source")]),
    );
    let rendered =
        rendered.expect("reducta_source must render registry-direct, not the generic fallback");
    let fallback = parse_message(
        ParseErrorKind::Expected,
        &args(&[("issue", "no_such_parse_issue")]),
    );
    assert_ne!(
        Some(rendered.as_str()),
        fallback.as_deref(),
        "reducta_source must use its own arm, not the generic fallback: {rendered}"
    );
}
