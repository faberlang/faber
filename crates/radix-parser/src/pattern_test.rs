//! Tests for pattern parsing: path patterns, binding lists, separator syntax, literal patterns.
//!
//! Contract: each pattern form parses to the expected `Pattern` variant.

use crate::test_support::parse_ok_la;
use radix_syntax::{
    FiguraExpr, GenericArgExpr, IfBody, Pattern, PatternBind, StmtKind, TypeExprKind,
};

#[test]
fn parses_path_patterns_with_binding_lists_and_et_separators() {
    let result = parse_ok_la(
        r"
discerne event {
    casu Result.Ok sit payload, error et _ ergo redde payload
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
    let result = parse_ok_la(
        "genus Formal { varia textus greeting }\nfunctio f(Formal ∪ nihil u) → vacuum {\n    discerne u {\n        casu Formal sit f { tacet }\n        casu nihil { tacet }\n    }\n}",
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

/// N7c: the retired `numerus<u8>` spelling re-emits as the bare marker, so a
/// bare width marker (and fused sugar) must be a type pattern, not a binding.
#[test]
fn bare_width_markers_are_type_patterns() {
    let result = parse_ok_la(
        "functio f(u8 ∪ textus u) → vacuum {\n    discerne u {\n        casu u8 sit b { tacet }\n        casu textus { tacet }\n    }\n}",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Discerne(stmt) = &body.statements[0].kind else {
        panic!("expected discerne statement");
    };
    assert!(matches!(
        stmt.arms[0].patterns[0],
        Pattern::Type(_, Some(_))
    ));
    assert!(matches!(stmt.arms[1].patterns[0], Pattern::Type(_, None)));
}

#[test]
fn parses_width_hole_and_tensor_figure_patterns() {
    let result = parse_ok_la(
        r"
incipit {
    discerne value {
        casu lista<numerus, _> ergo tacet
        casu tensor<f32, [2, 4]> ergo tacet
    }
}
",
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
fn derived_width_pattern_bounds_parse_for_semantic_rejection() {
    // Affine figure arithmetic is a generic-argument form (tensor shapes), so
    // the parser carries `N+1` as a size term; width patterns are rejected over
    // a closed set by `discerne_pattern_not_in_closed_set` (radix-semantic
    // `derived_width_list_pattern_is_rejected`).
    let result = parse_ok_la("incipit { discerne value { casu lista<numerus, N+1> ergo tacet } }");
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
    assert!(matches!(
        args[1],
        GenericArgExpr::Size(FiguraExpr::Add(_, 1))
    ));
}

#[test]
fn parses_ascii_and_octeti_literal_patterns() {
    let result = parse_ok_la(
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

/// Arm-head patterns of the first `discerne` statement in `source`.
fn discerne_patterns(result: &crate::ParseResult) -> &[Pattern] {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };
    &stmt.arms[0].patterns
}

#[test]
fn leading_minus_before_a_number_is_a_negated_literal_pattern() {
    // D22.2: the lexer never produces a signed number, so the pattern parser
    // owns the sign for integer, decimal, and infinity literals.
    let result = parse_ok_la("discerne n {\n    casu -1 et -2.5 et -∞ { tacet }\n}");
    let patterns = discerne_patterns(&result);
    assert_eq!(patterns.len(), 3);
    assert!(matches!(
        patterns[0],
        Pattern::NegatedLiteral(radix_syntax::Literal::Int(1), _)
    ));
    assert!(matches!(
        &patterns[1],
        Pattern::NegatedLiteral(radix_syntax::Literal::FloatText { value, .. }, _) if *value == 2.5
    ));
    assert!(matches!(
        &patterns[2],
        Pattern::NegatedLiteral(radix_syntax::Literal::Float(value), _) if value.is_infinite() && *value > 0.0
    ));
}

#[test]
fn minus_before_a_non_number_is_not_a_pattern() {
    let result = crate::test_support::parse_program_la("discerne n {\n    casu -x { tacet }\n}");
    assert!(!result.success(), "`-x` must not parse as a pattern");
}

#[test]
fn nonnumerus_in_pattern_position_is_a_nan_literal_not_a_binding() {
    // D22.3: the NaN keyword claims the literal slot so lowering can reject
    // it, instead of silently binding a catch-all name.
    let result = parse_ok_la("discerne x {\n    casu nonnumerus { tacet }\n}");
    let patterns = discerne_patterns(&result);
    assert!(matches!(
        &patterns[0],
        Pattern::Literal(radix_syntax::Literal::Float(value), _) if value.is_nan()
    ));
}

#[test]
fn or_alternative_arm_parses_as_one_pattern() {
    let result =
        parse_ok_la("discerne color {\n    casu Color.Rubrum aut Color.Caeruleum { tacet }\n}");
    let patterns = discerne_patterns(&result);
    let Pattern::Or(alternatives) = &patterns[0] else {
        panic!("expected or pattern");
    };
    assert_eq!(alternatives.len(), 2);
    assert!(matches!(alternatives[0], Pattern::Path(_)));
    assert!(matches!(alternatives[1], Pattern::Path(_)));
}

#[test]
fn or_alternative_arm_accepts_negated_literals_and_subject_separators() {
    let result =
        parse_ok_la("discerne left et right {\n    casu -1 aut -2 et -3 aut -4 ergo tacet\n}");
    let patterns = discerne_patterns(&result);
    assert_eq!(patterns.len(), 2);
    assert!(matches!(patterns[0], Pattern::Or(ref alternatives) if alternatives.len() == 2));
    assert!(matches!(patterns[1], Pattern::Or(ref alternatives) if alternatives.len() == 2));
}
