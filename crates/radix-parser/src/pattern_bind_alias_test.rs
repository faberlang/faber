//! Tests for `ut` alias syntax in pattern bindings.
//!
//! Contract: `fixum <name> ut <alias>` in pattern bindings is parsed and preserved through
//! the syntax tree.

use crate::test_support::parse_ok;
use radix_syntax::{Pattern, PatternBind, StmtKind};

#[test]
fn parses_variant_payload_bindings_with_ut_aliases() {
    let result = parse_ok(
        r"
discerne m {
    casu Loquere sit verba ut v {
        redde v
    }
    casu Moveo sit locus ut loc, passus ut n {
        redde loc
    }
}
",
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };

    assert_eq!(stmt.arms.len(), 2);

    let Pattern::Ident(_, Some(PatternBind::Bindings { bindings, .. })) = &stmt.arms[0].patterns[0]
    else {
        panic!("expected Loquere binding list");
    };
    assert_eq!(result.interner.resolve(bindings[0].name.name), "verba");
    assert_eq!(
        result
            .interner
            .resolve(bindings[0].alias.as_ref().expect("alias").name),
        "v"
    );

    let Pattern::Ident(_, Some(PatternBind::Bindings { bindings, .. })) = &stmt.arms[1].patterns[0]
    else {
        panic!("expected Moveo binding list");
    };
    assert_eq!(result.interner.resolve(bindings[0].name.name), "locus");
    assert_eq!(
        result
            .interner
            .resolve(bindings[0].alias.as_ref().expect("alias").name),
        "loc"
    );
    assert_eq!(result.interner.resolve(bindings[1].name.name), "passus");
    assert_eq!(
        result
            .interner
            .resolve(bindings[1].alias.as_ref().expect("alias").name),
        "n"
    );
}

/// A single binding with no alias should parse correctly.
#[test]
fn single_binding_with_no_alias() {
    let result = parse_ok(
        r"
discerne m {
    casu Loquere sit verba {
        redde verba
    }
}
",
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };

    assert_eq!(stmt.arms.len(), 1);
    let Pattern::Ident(_, Some(PatternBind::Bindings { bindings, .. })) = &stmt.arms[0].patterns[0]
    else {
        panic!("expected binding list");
    };
    assert_eq!(bindings.len(), 1);
    assert_eq!(result.interner.resolve(bindings[0].name.name), "verba");
    assert!(bindings[0].alias.is_none(), "no alias should be None");
}

/// A wildcard pattern with no binding list should parse.
#[test]
fn wildcard_pattern_has_no_bindings() {
    let result = parse_ok(
        r"
discerne m {
    casu _ ergo nulla
}
",
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };
    assert_eq!(stmt.arms.len(), 1);
    assert!(matches!(stmt.arms[0].patterns[0], Pattern::Wildcard(_)));
}

/// Three bindings with mixed aliases should parse and preserve ordering.
#[test]
fn multiple_bindings_with_mixed_aliases() {
    let result = parse_ok(
        r"
discerne m {
    casu Moveo sit locus ut loc, passus, gradus ut g {
        redde loc
    }
}
",
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };
    let Pattern::Ident(_, Some(PatternBind::Bindings { bindings, .. })) = &stmt.arms[0].patterns[0]
    else {
        panic!("expected binding list");
    };
    assert_eq!(bindings.len(), 3);
    assert_eq!(result.interner.resolve(bindings[0].name.name), "locus");
    assert!(bindings[0].alias.is_some());
    assert_eq!(result.interner.resolve(bindings[1].name.name), "passus");
    assert!(bindings[1].alias.is_none());
    assert_eq!(result.interner.resolve(bindings[2].name.name), "gradus");
    assert!(bindings[2].alias.is_some());
}
