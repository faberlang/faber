//! Coverage for the leading-underscore style warning (D5.6, WARN030).
//!
//! Each declaration kind warns once, at the declared identifier; uses of the
//! name do not warn again; a bare `_` never warns.

use crate::test_support::analyze_source;
use crate::{SemanticError, SemanticErrorKind, WarningKind};
use radix_diagnostics::DiagnosticArg;

fn underscore_findings(source: &str) -> Vec<SemanticError> {
    analyze_source(source)
        .diagnostics
        .iter()
        .filter(|diag| diag.kind == SemanticErrorKind::Warning(WarningKind::LeadingUnderscore))
        .cloned()
        .collect()
}

fn assert_single(source: &str, name: &str, kind: &str) {
    let findings = underscore_findings(source);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let finding = &findings[0];
    assert!(
        !finding.is_error(),
        "leading `_` is a warning, not an error"
    );
    assert_eq!(crate::semantic_spec(finding.kind).code, "WARN030");
    assert_eq!(
        finding.args,
        vec![
            DiagnosticArg::new("issue", "leading_underscore"),
            DiagnosticArg::new("name", name),
            DiagnosticArg::new("kind", kind),
        ]
    );
}

#[test]
fn function_name_warns() {
    assert_single(
        "functio _auxilium() → numerus { redde 1 }\nincipit { nota _auxilium() }",
        "_auxilium",
        "function",
    );
}

#[test]
fn method_name_warns() {
    assert_single(
        "genus Punctum {\n    varia numerus x\n    functio _norma() → numerus { redde ego.x }\n}",
        "_norma",
        "function",
    );
}

#[test]
fn type_name_warns() {
    assert_single(
        "genus _Punctum {\n    varia numerus x\n}",
        "_Punctum",
        "type",
    );
}

#[test]
fn type_alias_name_warns() {
    assert_single("typus _Numeri = numerus", "_Numeri", "type");
}

#[test]
fn field_name_warns() {
    assert_single("genus Punctum {\n    varia numerus _x\n}", "_x", "field");
}

#[test]
fn local_warns_once_at_declaration() {
    assert_single(
        "incipit {\n    fixum numerus _n ← 1\n    nota _n\n    nota _n\n}",
        "_n",
        "local",
    );
}

#[test]
fn parameter_warns() {
    assert_single(
        "functio duplex(numerus _n) → numerus { redde _n * 2 }\nincipit { nota duplex(2) }",
        "_n",
        "parameter",
    );
}

#[test]
fn enum_member_warns_as_constant() {
    assert_single("ordo Color { _Red, Green }", "_Red", "constant");
}

#[test]
fn enum_without_leading_underscore_never_warns() {
    let source = "ordo Color { Red, Green }";
    let result = analyze_source(source);
    assert!(result.is_ok(), "{:?}", result.diagnostics);
    assert!(underscore_findings(source).is_empty());
}

#[test]
fn union_variant_warns_as_type() {
    assert_single(
        "discretio Figura { _Circulus { fractus r }, Quadratum { fractus l } }",
        "_Circulus",
        "type",
    );
}

#[test]
fn static_warns() {
    assert_single("fixum numerus _LIMES = 4096", "_LIMES", "constant");
}

#[test]
fn top_level_const_warns_as_constant_and_local_const_as_local() {
    assert_single("fixum numerus _LIMES = 4096", "_LIMES", "constant");
    assert_single(
        "incipit {\n    fixum numerus _n = 4\n    nota _n\n}",
        "_n",
        "local",
    );
}

#[test]
fn bare_hole_never_warns() {
    let source = "incipit {\n    fixum lista<numerus> triplex ← [1, 2, 3]\n    \
        fixum [_, medius, _] ← triplex\n    fixum _ x ← medius\n    nota x\n}";
    let result = analyze_source(source);
    assert!(result.is_ok(), "{:?}", result.diagnostics);
    assert!(underscore_findings(source).is_empty());
}

#[test]
fn plain_names_never_warn() {
    let source = "functio duplex(numerus n) → numerus { redde n * 2 }\n\
        incipit { fixum numerus n_x ← duplex(2)\n nota n_x }";
    let result = analyze_source(source);
    assert!(result.is_ok(), "{:?}", result.diagnostics);
    assert!(underscore_findings(source).is_empty());
}
