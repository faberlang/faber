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

/// Render one issue registry-direct and prove it does not fall through to the
/// unknown-issue fallback. Text assertions stay out of this file; the issue
/// identity plus the Some/None split is the contract.
fn assert_registry_direct_arm(issue: &'static str, arg_pairs: &[(&'static str, &'static str)]) {
    let mut pairs = vec![("issue", issue)];
    pairs.extend_from_slice(arg_pairs);
    let rendered = parse_message(ParseErrorKind::Expected, &args(&pairs))
        .unwrap_or_else(|| panic!("{issue} must render registry-direct"));
    let fallback = parse_message(
        ParseErrorKind::Expected,
        &args(&[("issue", "no_such_parse_issue")]),
    );
    assert_ne!(
        Some(rendered.as_str()),
        fallback.as_deref(),
        "{issue} must use its own arm, not the generic fallback: {rendered}"
    );
}

#[test]
fn expected_mutability_keyword_registry_direct_has_an_arm() {
    assert_registry_direct_arm(
        "expected_mutability_keyword",
        &[("fixum", "fixum"), ("varia", "varia")],
    );
}

#[test]
fn expected_method_declaration_registry_direct_has_an_arm() {
    assert_registry_direct_arm("expected_method_declaration", &[("functio", "functio")]);
}

#[test]
fn expected_praepara_hook_registry_direct_has_an_arm() {
    assert_registry_direct_arm(
        "expected_praepara_hook",
        &[
            ("praepara", "praepara"),
            ("praeparabit", "praeparabit"),
            ("postpara", "postpara"),
            ("postparabit", "postparabit"),
        ],
    );
}

#[test]
fn expected_scribe_keyword_registry_direct_has_an_arm() {
    assert_registry_direct_arm(
        "expected_scribe_keyword",
        &[
            ("scribe", "scribe"),
            ("vide", "vide"),
            ("mone", "mone"),
            ("nota", "nota"),
        ],
    );
}

#[test]
fn expected_incipit_keyword_registry_direct_has_an_arm() {
    assert_registry_direct_arm(
        "expected_incipit_keyword",
        &[("incipit", "incipit"), ("incipiet", "incipiet")],
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
