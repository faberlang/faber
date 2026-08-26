use crate::test_support::{assert_parse_error_matches, parse_ok, ParseExpectation};
use radix_syntax::{BindingPattern, StmtKind};

#[test]
fn rejects_de_var_reproduction_with_const_repair() {
    let source = "itera de ys varia y { y ← y + 5 }";
    assert_parse_error_matches(
        source,
        ParseExpectation::Args(&[
            ("issue", "itera_de_requires_fixum"),
            ("expected", "the const form `fixum`"),
        ]),
    );
}

#[test]
fn parses_range_underscore_binder_as_identifier() {
    let result = parse_ok("itera ex 0‥3 fixum _ { tacet }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    let BindingPattern::Ident(binding) = &iter_stmt.bindings[0] else {
        panic!("range underscore binder must remain an identifier");
    };
    assert_eq!(result.interner.resolve(binding.name), "_");
}

#[test]
fn parses_lockstep_source_series_with_matching_binder_series() {
    // Need: `ex` alone admits the comma-separated series; the
    // binder series is names. Arity is a semantic law, so a count mismatch
    // still parses (and is pinned by corpus/itera/lockstep-decline.fab).
    let result = parse_ok("itera ex ks, vs, ts apud [i] fixum k, v, t { tacet }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert_eq!(iter_stmt.sources.len(), 3);
    assert_eq!(iter_stmt.bindings.len(), 3);
    assert!(iter_stmt
        .bindings
        .iter()
        .all(|binding| matches!(binding, BindingPattern::Ident(_))));
    assert_eq!(iter_stmt.apud.len(), 1);
}

#[test]
fn lockstep_source_series_stays_ex_only() {
    // `de` and `ab` keep one source: the comma is not a series separator
    // there, so the head declines at the mutability marker (the parser is
    // already past the single source expression when the comma appears).
    assert_parse_error_matches(
        "itera de ks, vs fixum k { tacet }",
        ParseExpectation::Args(&[
            ("issue", "mutability_marker"),
            ("expected", "'fixum' or 'varia'"),
        ]),
    );
}
