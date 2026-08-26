use crate::test_support::{assert_parse_error_matches, parse_ok, ParseExpectation};
use radix_syntax::{BindingPattern, ExprKind, IteraMode, StmtKind};

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
fn parses_ab_range_source_series_n2_with_exact_counts() {
    // NR-1: `ab` admits the comma-separated range source series into the
    // existing `sources` vector; binder names stay the `itera_binding` series.
    // Whether every member is a range is NR-2's semantic law, not a parse
    // decision.
    let result = parse_ok("itera ab 0‥m, 0‥n fixum i, j { tacet }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert_eq!(
        std::mem::discriminant(&iter_stmt.mode),
        std::mem::discriminant(&IteraMode::Ab)
    );
    assert_eq!(iter_stmt.sources.len(), 2);
    assert!(
        iter_stmt
            .sources
            .iter()
            .all(|source| matches!(source.kind, ExprKind::Intervallum(_))),
        "n2 range sources must each be a range expression"
    );
    assert_eq!(iter_stmt.bindings.len(), 2);
    let names = iter_stmt
        .bindings
        .iter()
        .map(|binding| match binding {
            BindingPattern::Ident(ident) => result.interner.resolve(ident.name),
            other => panic!("expected name binder, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["i", "j"]);
}

#[test]
fn parses_ab_range_source_series_n3_with_exact_counts() {
    let result = parse_ok("itera ab 0‥m, 0‥n, 0‥k fixum i, j, p { tacet }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert_eq!(iter_stmt.sources.len(), 3);
    assert!(
        iter_stmt
            .sources
            .iter()
            .all(|source| matches!(source.kind, ExprKind::Intervallum(_))),
        "n3 range sources must each be a range expression"
    );
    assert_eq!(iter_stmt.bindings.len(), 3);
    let names = iter_stmt
        .bindings
        .iter()
        .map(|binding| match binding {
            BindingPattern::Ident(ident) => result.interner.resolve(ident.name),
            other => panic!("expected name binder, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["i", "j", "p"]);
}

#[test]
fn parses_ab_single_source_shape_unchanged() {
    // N=1 `ab` stays exactly one source and one binder through the widened
    // series gate.
    let result = parse_ok("itera ab 0‥m fixum i { tacet }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert_eq!(iter_stmt.sources.len(), 1);
    assert_eq!(iter_stmt.bindings.len(), 1);
}

#[test]
fn parses_mixed_ab_source_series_for_semantic_phase() {
    // `0‥m, n` must parse into the series; the range-membership decline is
    // NR-2's dedicated SEM010 issue, never a generic parser error.
    let result = parse_ok("itera ab 0‥m, n fixum i, j { tacet }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert_eq!(iter_stmt.sources.len(), 2);
    assert!(matches!(
        iter_stmt.sources[0].kind,
        ExprKind::Intervallum(_)
    ));
    assert!(!matches!(
        iter_stmt.sources[1].kind,
        ExprKind::Intervallum(_)
    ));
    assert_eq!(iter_stmt.bindings.len(), 2);
}

#[test]
fn de_source_series_still_rejected() {
    // `de` keeps one source: the comma is not a series separator there, so
    // the head declines at the mutability marker (the parser is already past
    // the single source expression when the comma appears).
    assert_parse_error_matches(
        "itera de ks, vs fixum k { tacet }",
        ParseExpectation::Args(&[
            ("issue", "mutability_marker"),
            ("expected", "'fixum' or 'varia'"),
        ]),
    );
}
