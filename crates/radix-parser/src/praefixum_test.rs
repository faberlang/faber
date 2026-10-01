//! `praefixum { … }` build-time initializer bodies (DECISIONS D8.4).
//!
//! Contract: `praefixum` is followed by a block; the body parses as
//! statements ending in `redde`. The retired Zig-era `praefixum(expr)` form
//! is the migration diagnostic `praefixum_expression_form_retired`.

use crate::test_support::{parse_ok, parse_program};
use radix_syntax::{ExprKind, StmtKind};

#[test]
fn static_initializer_parses_block_body() {
    let result =
        parse_ok("fixum numerus N = praefixum {\n    varia numerus s ← 1\n    redde s + 1\n}\n");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected static var decl");
    };
    let init = decl.init.as_ref().expect("initializer");
    let ExprKind::Praefixum(praefixum) = &init.kind else {
        panic!("expected praefixum initializer, got {:?}", init.kind);
    };
    assert_eq!(praefixum.body.statements.len(), 2);
}

#[test]
fn retired_expression_form_gets_migration_diagnostic() {
    let result = parse_program("fixum numerus N = praefixum(4 * 1024)\n");
    assert!(
        result.errors.iter().any(|err| err
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "praefixum_expression_form_retired")),
        "got {:?}",
        result.errors
    );
}
