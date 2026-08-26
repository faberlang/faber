//! Tests for pattern parsing: path patterns, binding lists, separator syntax, literal patterns.
//!
//! Contract: each pattern form parses to the expected `Pattern` variant.

use crate::parse;
use crate::test_support::parse_ok;
use radix_lexer::lex;
use radix_syntax::{
    FiguraExpr, GenericArgExpr, IfBody, Pattern, PatternBind, StmtKind, TypeExprKind,
};

#[test]
fn parses_path_patterns_with_binding_lists_and_et_separators() {
    let result = parse_ok(
        r"
discerne event {
    casu Result.Ok fixum payload, error et _ ergo redde payload
}
",
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };

    assert_eq!(stmt.arms.len(), 1);
    assert_eq!(stmt.arms[0].patterns.len(), 2);

    let Pattern::Path(path) = &stmt.arms[0].patterns[0] else {
        panic!("expected path pattern");
    };
    assert_eq!(path.segments.len(), 2);
    assert_eq!(result.interner.resolve(path.segments[0].name), "Result");
    assert_eq!(result.interner.resolve(path.segments[1].name), "Ok");

    let Some(PatternBind::Bindings { bindings, .. }) = &path.bind else {
        panic!("expected binding list");
    };
    assert_eq!(
        bindings
            .iter()
            .map(|binding| result.interner.resolve(binding.name.name))
            .collect::<Vec<_>>(),
        ["payload", "error"]
    );

    assert!(matches!(stmt.arms[0].patterns[1], Pattern::Wildcard(_)));
    assert!(matches!(stmt.arms[0].body, IfBody::Ergo(_)));
}

#[test]
fn parses_type_pattern_atoms_and_null_member() {
    let result = parse_ok(
        "genus Formal { textus greeting }\nfunctio f(Formal ∪ nihil u) → vacuum {\n    discerne u {\n        casu Formal fixum f { tacet }\n        casu nihil { tacet }\n    }\n}",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[1].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Discerne(stmt) = &body.statements[0].kind else {
        panic!("expected discerne statement");
    };
    assert!(matches!(stmt.arms[0].patterns[0], Pattern::Type(_, _)));
    assert!(matches!(stmt.arms[1].patterns[0], Pattern::Type(_, None)));
}

#[test]
fn parses_width_hole_and_tensor_figure_patterns() {
    let result = parse_ok(
        r#"
incipit {
    discerne value {
        casu lista<numerus, _> ergo tacet
        casu tensor<f32, [2, 4]> ergo tacet
    }
}
"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(stmt) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(block) = &stmt.body else {
        panic!("expected incipit block");
    };
    let radix_syntax::StmtKind::Discerne(discerne) = &block.statements[0].kind else {
        panic!("expected discerne statement");
    };

    let Pattern::Type(ty, None) = &discerne.arms[0].patterns[0] else {
        panic!("expected lista type pattern");
    };
    let TypeExprKind::Named(_, args) = &ty.kind else {
        panic!("expected named lista type");
    };
    assert_eq!(args.len(), 2);
    assert!(
        matches!(args[1], GenericArgExpr::Type(ref ty) if matches!(ty.kind, TypeExprKind::Infer))
    );

    let Pattern::Type(ty, None) = &discerne.arms[1].patterns[0] else {
        panic!("expected tensor type pattern");
    };
    let TypeExprKind::Tensor(_, FiguraExpr::Tuple(dims)) = &ty.kind else {
        panic!("expected tensor figura pattern");
    };
    assert_eq!(dims.len(), 2);
    assert!(matches!(dims[0], FiguraExpr::Literal(2)));
    assert!(matches!(dims[1], FiguraExpr::Literal(4)));
}

#[test]
fn rejects_derived_width_pattern_bounds() {
    let result = parse(lex(
        "incipit { discerne value { casu lista<numerus, N+1> ergo tacet } }",
    ));
    assert!(
        !result.errors.is_empty(),
        "symbolic arithmetic in a width pattern must fail at parse/lowering boundary"
    );
}

#[test]
fn parses_ascii_and_octeti_literal_patterns() {
    let result = parse_ok(
        r"
discerne sig {
    casu 'pg:query' ergo redde 1
    casu |deadbeef| ergo redde 2
}
",
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };
    assert_eq!(stmt.arms.len(), 2);

    let Pattern::Literal(radix_syntax::Literal::Ascii(sym), _) = &stmt.arms[0].patterns[0] else {
        panic!("expected ascii literal pattern");
    };
    assert_eq!(result.interner.resolve(*sym), "pg:query");

    let Pattern::Literal(radix_syntax::Literal::Octeti(sym), _) = &stmt.arms[1].patterns[0] else {
        panic!("expected octeti literal pattern");
    };
    assert_eq!(result.interner.resolve(*sym), "deadbeef");
}
