//! Tests for expression parsing: conversio, coalesce, binary operators, JSON valor literals,
//! and retired-form diagnostics.
//!
//! Contract: new expressions parse to the expected `ExprKind` variant; retired surface forms
//! emit structured diagnostics.

use crate::ParseErrorKind;
use crate::ParseResult;
use crate::test_support::args_have_no_legacy_message;
use crate::test_support::{
    assert_parse_error_issue_la, parse_ok_la, parse_program_la,
    parse_recovering_ascii_comparison_la, span_for,
};
use radix_diagnostics::Span;
use radix_syntax::{
    AssignOp, BinOp, CallDefaultExpr, ConversioTarget, Expr, ExprKind, FiguraExpr, JsonValue,
    Literal, NonNullKind, OptionalChainKind, RangeKind, StmtKind, TypeExprKind, TypeFormLevel,
};

fn generic_arg_type(arg: &radix_syntax::GenericArgExpr) -> &radix_syntax::TypeExpr {
    match arg {
        radix_syntax::GenericArgExpr::Type(ty) => ty,
        other => panic!("expected type generic argument, got {other:?}"),
    }
}

#[test]
fn conversio_with_up_tack_default_parses() {
    // D17.7: `↦ T ⊥ value` populates the conversion's `recovery` slot.
    let result = parse_ok_la(r#"sit _ n ← "bad" ↦ numerus ⊥ 0"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    assert!(matches!(conversio.target, ConversioTarget::Explicit(_)));
    let recovery = conversio.recovery.as_deref().expect("recovery expression");
    assert!(matches!(recovery.kind, ExprKind::Literal(Literal::Int(0))));
}

#[test]
fn expression_array_keeps_shape_inference_holes_structural() {
    let result = parse_ok_la(r"sit _ shape ← [_, 4]");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ExprKind::Array(array) = &var.init.as_deref().expect("initializer").kind else {
        panic!("expected array expression");
    };
    assert!(matches!(
        &array.elements[0],
        radix_syntax::ArrayElement::Infer(_)
    ));
    assert!(matches!(
        array.elements[1],
        radix_syntax::ArrayElement::Expr(ref expr)
            if matches!(expr.kind, ExprKind::Literal(Literal::Int(4)))
    ));
}

#[test]
fn conversio_capitalized_user_type_target_parses_as_type() {
    let result = parse_ok_la(r"sit _ person ← payload ↦ Persona");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    assert!(matches!(conversio.target, ConversioTarget::Explicit(_)));
}

#[test]
fn conversio_qualified_capitalized_user_type_target_parses_as_type() {
    let result = parse_ok_la(r"sit _ person ← payload ↦ types.Person");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    let ConversioTarget::Explicit(target) = &conversio.target else {
        panic!("expected explicit conversio target");
    };
    assert!(matches!(
        target.kind,
        TypeExprKind::Qualified(ref path, ref params) if path.len() == 2 && params.is_empty()
    ));
}

#[test]
fn conversio_to_tokenless_type_keyword_target_parses_as_type() {
    // Batch A made `modulus`/`iuncta` tokenless; as conversio targets they must
    // still route to an explicit type, not the range fallback.
    let result = parse_ok_la(r"sit _ w ← checked ↦ modulus<u32>");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    assert!(matches!(conversio.target, ConversioTarget::Explicit(_)));
}

#[test]
fn tokenless_keyword_after_nihil_value_does_not_trigger_retired_prefix() {
    // Batch B regression: a `nihil` VALUE followed by a tokenless statement
    // keyword (`nota`) on the next line must not make the retired
    // `nihil x` prefix-predicate detection fire. The lookahead must treat
    // tokenless statement keywords as grammar structure, not value operands.
    let result = parse_ok_la(
        r"
incipit {
  sit numerus ∪ nihil maybe ← nulla
  nota 42
}
",
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
}

#[test]
fn conversio_tensor_sugar_target_parses_as_type() {
    let result = parse_ok_la(r"sit _ grid ← flat ↦ tf32[2, 2]");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    let ConversioTarget::Explicit(target) = &conversio.target else {
        panic!("expected explicit conversio target");
    };
    let TypeExprKind::Tensor(elem, figura) = &target.kind else {
        panic!("expected tensor sugar target");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected tensor element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "f32");
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple Figura");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(2)));
    assert!(matches!(dims[1], FiguraExpr::Literal(2)));
}

#[test]
fn conversio_lista_sugar_target_parses_as_type() {
    let result = parse_ok_la(r"sit _ xs ← wire ↦ lu32");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    let ConversioTarget::Explicit(target) = &conversio.target else {
        panic!("expected explicit conversio target");
    };
    let TypeExprKind::Named(lista, params) = &target.kind else {
        panic!("expected lista sugar target");
    };
    assert_eq!(result.interner.resolve(lista.name), "lista");
    let TypeExprKind::Named(elem, _) = &generic_arg_type(&params[0]).kind else {
        panic!("expected width element");
    };
    assert_eq!(result.interner.resolve(elem.name), "u32");
}

#[test]
fn conversio_followed_by_vel_parses_as_nullish_coalesce() {
    let result = parse_ok_la(r"sit _ n ← (a ↦ numerus) vel 0");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(coalesce) = &expr.kind else {
        panic!("expected nullish coalesce, got {:?}", expr.kind);
    };
    assert_eq!(coalesce.op, BinOp::Coalesce);
    let lhs = match &coalesce.lhs.kind {
        ExprKind::Conversio(_) => &coalesce.lhs,
        ExprKind::Paren(inner) => inner,
        other => panic!("expected conversio left-hand side, got {other:?}"),
    };
    assert!(matches!(lhs.kind, ExprKind::Conversio(_)));
    assert!(matches!(
        coalesce.rhs.kind,
        ExprKind::Literal(Literal::Int(0))
    ));
}

#[test]
fn mixed_or_and_coalesce_tokens_parse_as_one_expression() {
    let result = parse_ok_la("sit _ value ← a vel b aut c");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");

    let ExprKind::Binary(top) = &expr.kind else {
        panic!("expected binary expression");
    };
    assert_eq!(top.op, BinOp::Or);

    let ExprKind::Binary(lhs) = &top.lhs.kind else {
        panic!("expected coalesce left-hand side");
    };
    assert_eq!(lhs.op, BinOp::Coalesce);
}

/// Local nullable elimination: `vel` binds tighter than additive operators on the RHS.
#[test]
fn vel_binds_tighter_than_additive_on_rhs() {
    let result = parse_ok_la(r#"sit _ value ← prefix + item vel """#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(add) = &expr.kind else {
        panic!("expected additive root, got {:?}", expr.kind);
    };
    assert_eq!(add.op, BinOp::Add);
    let ExprKind::Binary(coalesce) = &add.rhs.kind else {
        panic!("expected coalesce on additive RHS, got {:?}", add.rhs.kind);
    };
    assert_eq!(coalesce.op, BinOp::Coalesce);
}

/// Local nullable elimination: `vel` binds tighter than additive operators on the LHS.
#[test]
fn vel_binds_tighter_than_additive_on_lhs() {
    let result = parse_ok_la(r#"sit _ value ← item vel "" + suffix"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(add) = &expr.kind else {
        panic!("expected additive root, got {:?}", expr.kind);
    };
    assert_eq!(add.op, BinOp::Add);
    let ExprKind::Binary(coalesce) = &add.lhs.kind else {
        panic!("expected coalesce on additive LHS, got {:?}", add.lhs.kind);
    };
    assert_eq!(coalesce.op, BinOp::Coalesce);
}

/// Local nullable elimination: `vel` binds tighter than multiplicative operators.
#[test]
fn vel_binds_tighter_than_multiplicative() {
    let result = parse_ok_la("sit _ value ← scale * factor vel 1");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(mul) = &expr.kind else {
        panic!("expected multiplicative root, got {:?}", expr.kind);
    };
    assert_eq!(mul.op, BinOp::Mul);
    let ExprKind::Binary(coalesce) = &mul.rhs.kind else {
        panic!(
            "expected coalesce on multiplicative RHS, got {:?}",
            mul.rhs.kind
        );
    };
    assert_eq!(coalesce.op, BinOp::Coalesce);
}

// ---------------------------------------------------------------------------
// Infix extrema (`⤒` / `⤓`) at additive precedence
// ---------------------------------------------------------------------------

#[test]
fn minmax_glyphs_bind_at_additive_precedence() {
    let result = parse_ok_la("sit _ value ← a ⤒ b * c + d ⤓ e");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(minimum) = &expr.kind else {
        panic!("expected minimum root, got {:?}", expr.kind);
    };
    assert_eq!(minimum.op, BinOp::Minimum);
    let ExprKind::Binary(add) = &minimum.lhs.kind else {
        panic!("expected additive lhs, got {:?}", minimum.lhs.kind);
    };
    assert_eq!(add.op, BinOp::Add);
    let ExprKind::Binary(maximum) = &add.lhs.kind else {
        panic!("expected maximum lhs, got {:?}", add.lhs.kind);
    };
    assert_eq!(maximum.op, BinOp::Maximum);
    let ExprKind::Binary(product) = &maximum.rhs.kind else {
        panic!("expected multiplicative RHS, got {:?}", maximum.rhs.kind);
    };
    assert_eq!(product.op, BinOp::Mul);
}

#[test]
fn minmax_glyph_chaining_is_left_associative() {
    let result = parse_ok_la("sit _ value ← a ⤒ b ⤓ c");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ExprKind::Binary(outer) = &var.init.as_deref().expect("initializer").kind else {
        panic!("expected binary expression");
    };
    assert_eq!(outer.op, BinOp::Minimum);
    let ExprKind::Binary(inner) = &outer.lhs.kind else {
        panic!("expected left-nested maximum");
    };
    assert_eq!(inner.op, BinOp::Maximum);
}

#[test]
fn minmax_glyphs_chain_beside_additive_operators() {
    let result = parse_ok_la("sit _ value ← a + b ⤒ c - d");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ExprKind::Binary(sub) = &var.init.as_deref().expect("initializer").kind else {
        panic!("expected subtraction root");
    };
    assert_eq!(sub.op, BinOp::Sub);
    let ExprKind::Binary(maximum) = &sub.lhs.kind else {
        panic!("expected maximum lhs");
    };
    assert_eq!(maximum.op, BinOp::Maximum);
    let ExprKind::Binary(add) = &maximum.lhs.kind else {
        panic!("expected addition under maximum");
    };
    assert_eq!(add.op, BinOp::Add);
}

// ---------------------------------------------------------------------------
// Glyph products (`·` `×` `⊗` `⊙` `⊘`) at multiplicative precedence
// ---------------------------------------------------------------------------

/// `a + b · c` parses as `a + (b · c)` — glyphs bind tighter than additive.
#[test]
fn glyph_products_bind_tighter_than_additive() {
    let result = parse_ok_la("sit _ value ← a + b · c");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(add) = &expr.kind else {
        panic!("expected additive root, got {:?}", expr.kind);
    };
    assert_eq!(add.op, BinOp::Add);
    let ExprKind::Binary(dot) = &add.rhs.kind else {
        panic!("expected glyph dot on additive RHS, got {:?}", add.rhs.kind);
    };
    assert_eq!(dot.op, BinOp::Dot);
}

#[test]
fn hadamard_divide_binds_at_multiplicative_precedence() {
    let result = parse_ok_la("sit _ value ← a + b ⊘ c");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(add) = &expr.kind else {
        panic!("expected additive root, got {:?}", expr.kind);
    };
    assert_eq!(add.op, BinOp::Add);
    let ExprKind::Binary(divide) = &add.rhs.kind else {
        panic!(
            "expected Hadamard divide on additive RHS, got {:?}",
            add.rhs.kind
        );
    };
    assert_eq!(divide.op, BinOp::HadamardDiv);
}

fn with_binary(source: &str, check: impl FnOnce(&radix_syntax::BinaryExpr)) {
    let result = parse_ok_la(source);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(binary) = &expr.kind else {
        panic!("expected binary root, got {:?}", expr.kind);
    };
    check(binary);
}

/// `÷` (true division) parses as a binary operator at `/`'s tier.
#[test]
fn true_division_parses_and_binds_like_slash() {
    with_binary("sit _ value ← a + b ÷ c", |add| {
        assert_eq!(add.op, BinOp::Add);
        let ExprKind::Binary(divide) = &add.rhs.kind else {
            panic!("expected ÷ on additive RHS, got {:?}", add.rhs.kind);
        };
        assert_eq!(divide.op, BinOp::TrueDiv);
    });
}

#[test]
fn true_division_is_left_associative() {
    with_binary("sit _ value ← a ÷ b ÷ c", |outer| {
        assert_eq!(outer.op, BinOp::TrueDiv);
        let ExprKind::Binary(inner) = &outer.lhs.kind else {
            panic!("expected ÷ on the left, got {:?}", outer.lhs.kind);
        };
        assert_eq!(inner.op, BinOp::TrueDiv);
    });
}

#[test]
fn true_division_mixes_with_slash_left_to_right() {
    with_binary("sit _ value ← a / b ÷ c", |outer| {
        assert_eq!(outer.op, BinOp::TrueDiv);
        let ExprKind::Binary(inner) = &outer.lhs.kind else {
            panic!("expected / on the left, got {:?}", outer.lhs.kind);
        };
        assert_eq!(inner.op, BinOp::Div);
    });
}

/// Equality glyphs retain distinct parse carriers for the promoted and fuzzy seats.
#[test]
fn equality_ladder_glyphs_pin_semantic_split() {
    fn parsed_op(spelling: &str) -> BinOp {
        let result = parse_ok_la(&format!("sit _ value ← a {spelling} b"));
        let program = result.program.as_ref().expect("program");
        let StmtKind::Var(var) = &program.statements[0].kind else {
            panic!("expected variable declaration for {spelling}");
        };
        let expr = var.init.as_deref().expect("initializer");
        let ExprKind::Binary(binary) = &expr.kind else {
            panic!(
                "expected binary expression for {spelling}, got {:?}",
                expr.kind
            );
        };
        binary.op
    }

    assert_eq!(parsed_op("≡"), BinOp::Eq);
    assert_eq!(parsed_op("≢"), BinOp::NotEq);
    assert_eq!(parsed_op("≅"), BinOp::ApproxEq);
    assert_eq!(parsed_op("≇"), BinOp::ApproxNotEq);
    assert_eq!(parsed_op("≈"), BinOp::FuzzyEq);
    assert_eq!(parsed_op("≉"), BinOp::FuzzyNotEq);
    assert_ne!(parsed_op("≅"), parsed_op("≈"));
    assert_ne!(parsed_op("≇"), parsed_op("≉"));
}

/// All five glyph spellings map to their pinned `BinOp` variants.
#[test]
fn glyph_products_map_to_pinned_binops() {
    for (spelling, expected) in [
        ("·", BinOp::Dot),
        ("×", BinOp::Cross),
        ("⊗", BinOp::TensorProduct),
        ("⊙", BinOp::Hadamard),
        ("⊘", BinOp::HadamardDiv),
    ] {
        let source = format!("sit _ value ← a {spelling} b");
        let result = parse_ok_la(&source);
        let program = result.program.as_ref().expect("program");
        let StmtKind::Var(var) = &program.statements[0].kind else {
            panic!("expected variable declaration");
        };
        let expr = var.init.as_deref().expect("initializer");
        let ExprKind::Binary(bin) = &expr.kind else {
            panic!("expected binary root for {spelling}, got {:?}", expr.kind);
        };
        assert_eq!(bin.op, expected, "glyph {spelling}");
    }
}

/// Glyph chains are left-associative at the multiplicative level.
#[test]
fn glyph_products_chain_left_associatively() {
    let result = parse_ok_la("sit _ value ← a · b × c");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(cross) = &expr.kind else {
        panic!("expected rightmost glyph at root, got {:?}", expr.kind);
    };
    assert_eq!(cross.op, BinOp::Cross);
    let ExprKind::Binary(dot) = &cross.lhs.kind else {
        panic!("expected dot on left, got {:?}", cross.lhs.kind);
    };
    assert_eq!(dot.op, BinOp::Dot);
}

/// Glyphs mix with `*` at the same precedence level.
#[test]
fn glyph_products_mix_with_star_at_same_precedence() {
    let result = parse_ok_la("sit _ value ← a * b ⊙ c");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(hadamard) = &expr.kind else {
        panic!("expected hadamard at root, got {:?}", expr.kind);
    };
    assert_eq!(hadamard.op, BinOp::Hadamard);
    let ExprKind::Binary(mul) = &hadamard.lhs.kind else {
        panic!("expected star on left, got {:?}", hadamard.lhs.kind);
    };
    assert_eq!(mul.op, BinOp::Mul);
}

/// Interval constructors complete as `vel` `RHS` values: not `(x vel 0)‥0`.
#[test]
fn vel_rhs_completes_intervallum_constructor() {
    let result = parse_ok_la("sit _ value ← maybeRange vel 0‥0");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(coalesce) = &expr.kind else {
        panic!("expected coalesce root, got {:?}", expr.kind);
    };
    assert_eq!(coalesce.op, BinOp::Coalesce);
    assert!(
        matches!(coalesce.rhs.kind, ExprKind::Intervallum(_)),
        "expected interval constructor RHS, got {:?}",
        coalesce.rhs.kind
    );
}

/// Both range positions scan the same operator spellings: the standalone
/// range tier and the `vel` fallback interval.
#[test]
fn range_operator_spellings_match_in_range_and_vel_positions() {
    for (op, inclusive) in [("‥", false), ("…", true), ("ante", false), ("usque", true)] {
        let result = parse_ok_la(&format!("sit _ value ← a {op} b"));
        let program = result.program.as_ref().expect("program");
        let StmtKind::Var(var) = &program.statements[0].kind else {
            panic!("expected variable declaration");
        };
        let ExprKind::Intervallum(range) = &var.init.as_deref().expect("initializer").kind else {
            panic!("expected interval for `{op}`");
        };
        assert_range_kind(range.kind, inclusive, op);

        let result = parse_ok_la(&format!("sit _ value ← maybe vel a {op} b"));
        let program = result.program.as_ref().expect("program");
        let StmtKind::Var(var) = &program.statements[0].kind else {
            panic!("expected variable declaration");
        };
        let ExprKind::Binary(coalesce) = &var.init.as_deref().expect("initializer").kind else {
            panic!("expected coalesce for `{op}`");
        };
        let ExprKind::Intervallum(range) = &coalesce.rhs.kind else {
            panic!("expected fallback interval for `{op}`");
        };
        assert_range_kind(range.kind, inclusive, op);
    }
}

fn assert_range_kind(kind: RangeKind, inclusive: bool, op: &str) {
    if inclusive {
        assert!(matches!(kind, RangeKind::Inclusive), "`{op}` is inclusive");
    } else {
        assert!(matches!(kind, RangeKind::Exclusive), "`{op}` is exclusive");
    }
}

/// `vel` chains left-associatively at the local defaulting tier.
#[test]
fn vel_chains_left_associatively() {
    let result = parse_ok_la(r#"sit _ value ← a vel b vel c vel "fallback""#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(outer) = &expr.kind else {
        panic!("expected coalesce root, got {:?}", expr.kind);
    };
    assert_eq!(outer.op, BinOp::Coalesce);
    let ExprKind::Binary(mid) = &outer.lhs.kind else {
        panic!("expected nested coalesce LHS");
    };
    assert_eq!(mid.op, BinOp::Coalesce);
    let ExprKind::Binary(inner) = &mid.lhs.kind else {
        panic!("expected innermost coalesce");
    };
    assert_eq!(inner.op, BinOp::Coalesce);
}

#[test]
fn non_est_parses_as_a_negative_type_test() {
    let result = parse_ok_la("sit _ present ← value non est nihil");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::TypeCheck(check) = &expr.kind else {
        panic!("expected type test, got {:?}", expr.kind);
    };
    assert!(!check.positive);
}

#[test]
fn est_non_is_not_the_identity_negation_spelling() {
    let result = parse_program_la("sit _ present ← value est non nihil");
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "est_non_spelling")
        })
        .expect("structured est non spelling diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
}

#[test]
fn expression_ad_parses_with_ascii_route_and_opener() {
    let result = parse_ok_la(r#"sit _ s ← ad 'runtime:echo' ("salve")"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Ad(ad) = &expr.kind else {
        panic!("expected ad expression, got {:?}", expr.kind);
    };
    assert_eq!(result.interner.resolve(ad.path), "runtime:echo");
    assert!(ad.opener.is_some());
}

// =============================================================================
// JSON valor literal parser (Goal Stage 2 of inline-json-valor)
// See docs/factory/inline-json-valor/contract.md for the full contract.
// =============================================================================

/// Extract the initializer expression from `sit name ← <expr>`.
fn json_literal_of(source: &str) -> JsonValue {
    let result = parse_ok_la(source);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!(
            "expected variable declaration, got {:?}",
            program.statements[0].kind
        );
    };
    let init = var.init.as_deref().expect("initializer");
    let ExprKind::Literal(Literal::JsonValor(value)) = &init.kind else {
        panic!("expected JSON valor literal, got {:?}", init.kind);
    };
    value.clone()
}

#[test]
fn json_literal_empty_object_is_valor() {
    assert!(
        matches!(json_literal_of("sit _ e ← {}"), JsonValue::Object(members) if members.is_empty())
    );
}

#[test]
fn json_literal_object_with_all_scalar_wire_tokens() {
    // true/false/null arrive as Ident (only verum/falsum/nihil are keywords).
    let value = json_literal_of(r#"sit _ v ← { "flag": true, "off": false, "gap": null }"#);
    let JsonValue::Object(members) = value else {
        panic!("expected object")
    };
    assert_eq!(members.len(), 3);
    assert!(matches!(members[0].value, JsonValue::Bool(true)));
    assert!(matches!(members[1].value, JsonValue::Bool(false)));
    assert!(matches!(members[2].value, JsonValue::Null));
}

#[test]
fn json_literal_number_mapping_splits_numerus_and_fractus() {
    // No `.` or `e` -> Numerus; with `.` or `e` -> Fractus (contract §5).
    let value =
        json_literal_of(r#"sit _ v ← { "int": 42, "float": 3.14, "big": 1000000, "exp": 1e5 }"#);
    let JsonValue::Object(members) = value else {
        panic!("expected object")
    };
    assert!(matches!(members[0].value, JsonValue::Numerus(42)));
    assert!(matches!(members[1].value, JsonValue::Fractus(_)));
    assert!(matches!(members[2].value, JsonValue::Numerus(1_000_000)));
    assert!(matches!(members[3].value, JsonValue::Fractus(_)));
}

#[test]
fn json_literal_negative_numbers_assemble_minus_and_magnitude() {
    let value = json_literal_of(r#"sit _ v ← { "a": -7, "b": -3.5 }"#);
    let JsonValue::Object(members) = value else {
        panic!("expected object")
    };
    assert!(matches!(members[0].value, JsonValue::Numerus(-7)));
    assert!(matches!(members[1].value, JsonValue::Fractus(f) if (f - (-3.5)).abs() < f64::EPSILON));
}

#[test]
fn json_literal_nested_objects_and_arrays() {
    let value = json_literal_of(
        r#"sit _ v ← { "paths": { "/health": { "get": {} } }, "ids": [1, "two", false, null] }"#,
    );
    let JsonValue::Object(members) = value else {
        panic!("expected object")
    };
    assert_eq!(members.len(), 2);
    // nested object under "paths"
    assert!(matches!(&members[0].value, JsonValue::Object(_)));
    // nested array under "ids"
    let JsonValue::Array(elements) = &members[1].value else {
        panic!("expected array")
    };
    assert_eq!(elements.len(), 4);
    assert!(matches!(elements[0], JsonValue::Numerus(1)));
    assert!(matches!(elements[1], JsonValue::String(_)));
    assert!(matches!(elements[2], JsonValue::Bool(false)));
    assert!(matches!(elements[3], JsonValue::Null));
}

#[test]
fn json_literal_trailing_commas_rejected_in_object_and_array() {
    for source in [
        r#"sit _ v ← { "x": 1, }"#,
        r#"sit _ v ← { "x": [1, 2, 3,] }"#,
    ] {
        let result = parse_program_la(source);
        assert!(
            result.errors.iter().any(|error| {
                error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "item_list_trailing_comma")
                    && error.args.iter().any(|arg| {
                        arg.name == "migration_help"
                            && arg.value == "remove the trailing comma; JSON follows RFC 8259"
                    })
            }),
            "expected strict JSON trailing-comma diagnostic, got {:?}",
            result.errors
        );
    }
}

#[test]
fn json_literal_rejects_faber_equals_field_syntax() {
    // The retired anonymous record form. Clean break: diagnose with a hint.
    let result = parse_program_la(r#"sit _ v ← { "x" = 1 }"#);
    assert!(
        !result.errors.is_empty(),
        "expected '=' to be rejected inside JSON literal"
    );
}

#[test]
fn json_literal_rejects_unterminated_object() {
    let result = parse_program_la(r#"sit _ v ← { "x": 1 "#);
    assert!(
        !result.errors.is_empty(),
        "expected unterminated-object diagnostic"
    );
}

#[test]
fn json_literal_genus_construction_keeps_faber_equals_grammar() {
    // Protected path: Type { field = expr } must still parse with `=`.
    // This is the genus construction regression guard (contract §12).
    let result = parse_ok_la(
        r"
genus Point { varia numerus x = 0 }
functio main() → numerus {
    sit _ p ← Point { x = 10 }
    redde 0
}
",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(_) = &program.statements[0].kind else {
        panic!("expected class declaration");
    };
    let StmtKind::Func(func) = &program.statements[1].kind else {
        panic!("expected function declaration");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Var(var) = &body.statements[0].kind else {
        panic!("expected var binding");
    };
    let init = var.init.as_deref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Verte(_)),
        "genus construction must stay Verte"
    );
}

#[test]
fn parses_ratio_construction_as_verte_with_labeled_type() {
    let result =
        parse_ok_la(r"incipit { sit _ point ← ratio<g: f32, w: f32> { g = 1.0, w = 2.0 } }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let radix_syntax::IfBody::Block(block) = &incipit.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let init = var.init.as_deref().expect("initializer");
    let ExprKind::Verte(verte) = &init.kind else {
        panic!(
            "expected ratio construction to be Verte, got {:?}",
            init.kind
        );
    };
    let TypeExprKind::Named(name, params) = &verte.ty.kind else {
        panic!("expected ratio constructor type");
    };
    assert_eq!(result.interner.resolve(name.name), "ratio");
    assert_eq!(params.len(), 2);
    assert_eq!(
        result.interner.resolve(
            generic_arg_type(&params[0])
                .iuncta_label
                .as_ref()
                .expect("g label")
                .name
        ),
        "g"
    );
    assert_eq!(
        result.interner.resolve(
            generic_arg_type(&params[1])
                .iuncta_label
                .as_ref()
                .expect("w label")
                .name
        ),
        "w"
    );
    let ExprKind::Object(object) = &verte.expr.kind else {
        panic!("expected ratio construction object");
    };
    assert_eq!(object.fields.len(), 2);
}

#[test]
fn rejects_unlabeled_ratio_constructor_type_argument() {
    // Unlabeled ratio tails no longer divert into the ratio constructor (the
    // `<` falls through to comparison parsing, per the two-token label
    // lookahead), so the specific `ratio_type_arg_label_required` issue is
    // gone — the unlabeled form must still fail at parse time.
    let result =
        parse_program_la(r"incipit { sit _ point ← ratio<f32, w: numerus> { g = 1.0, w = 2 } }");
    assert!(
        !result.success(),
        "unlabeled ratio constructor type argument must stay rejected, got {:?}",
        result.errors
    );
}

#[test]
fn record_field_list_hole_constructor_parses() {
    // `ratio<*> { g = 1.0, w = 2 }` (en `record<*>`): the expression-
    // position hole constructor is the same Verte node a labeled ratio
    // constructor builds, with the type back at `FieldListHole`.
    let result = parse_ok_la("incipit { sit ratio<*> p ← ratio<*> { g = 1.0, w = 2 } }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let radix_syntax::IfBody::Block(block) = &incipit.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let init = var.init.as_deref().expect("initializer");
    let ExprKind::Verte(verte) = &init.kind else {
        panic!(
            "expected ratio hole construction to be Verte, got {:?}",
            init.kind
        );
    };
    assert!(matches!(verte.ty.kind, TypeExprKind::FieldListHole));
    let ExprKind::Object(object) = &verte.expr.kind else {
        panic!("expected ratio construction object");
    };
    assert_eq!(object.fields.len(), 2);
}

#[test]
fn bare_field_literal_is_marked_and_hole_constructor_is_not() {
    // Operator ruling 2026-10-14: the bare `{ field = value }` literal is a
    // hole with a record default; `_ { field = value }` is a hole with none.
    // The parser records the difference on the `Verte` target's form so the
    // checker can default only the bare literal.
    let result = parse_ok_la("incipit { sit _ a ← { x = 1 }\n sit _ b ← _ { x = 1 } }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let radix_syntax::IfBody::Block(block) = &incipit.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(bare_var) = &block.statements[0].kind else {
        panic!("expected bare binding");
    };
    let bare_init = bare_var.init.as_deref().expect("bare initializer");
    let ExprKind::Verte(bare_verte) = &bare_init.kind else {
        panic!(
            "expected bare field literal to be Verte, got {:?}",
            bare_init.kind
        );
    };
    assert_eq!(bare_verte.ty.form, TypeFormLevel::BARE_LITERAL);

    let StmtKind::Var(hole_var) = &block.statements[1].kind else {
        panic!("expected hole binding");
    };
    let hole_init = hole_var.init.as_deref().expect("hole initializer");
    let ExprKind::Verte(hole_verte) = &hole_init.kind else {
        panic!(
            "expected hole constructor to be Verte, got {:?}",
            hole_init.kind
        );
    };
    assert_eq!(hole_verte.ty.form, TypeFormLevel::LONG);
}

#[test]
fn ratio_comparison_is_not_a_hole_constructor() {
    // `ratio < 5` has no `*`/`>`/`{` hole head, so the diversion stays a
    // comparison (recovered from the retired ASCII spelling).
    let result = parse_recovering_ascii_comparison_la("incipit { sit bivalens b ← ratio < 5 }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let radix_syntax::IfBody::Block(block) = &incipit.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let init = var.init.as_deref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Binary(_)),
        "expected comparison, got {:?}",
        init.kind
    );
}

#[test]
fn rejects_trailing_comma_in_ratio_constructor_type_arguments() {
    assert_parse_error_issue_la(
        r"incipit { sit _ point ← ratio<g: f32,> { g = 1.0 } }",
        "item_list_trailing_comma",
    );
}

#[test]
fn rejects_trailing_comma_in_ratio_constructor_fields() {
    assert_parse_error_issue_la(
        r"incipit { sit _ point ← ratio<g: f32> { g = 1.0, } }",
        "item_list_trailing_comma",
    );
}

#[test]
fn qualified_genus_construction_keeps_faber_equals_grammar() {
    let result = parse_ok_la(
        r#"
functio main() → vacuum {
    sit _ endpoint ← terminus.Terminus { hospes = "localhost", portus = 8080 }
}
"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Var(var) = &body.statements[0].kind else {
        panic!("expected var binding");
    };
    let init = var.init.as_deref().expect("initializer");
    let ExprKind::Verte(verte) = &init.kind else {
        panic!("expected qualified genus construction to parse as Verte");
    };
    let TypeExprKind::Qualified(path, params) = &verte.ty.kind else {
        panic!("expected qualified constructor type");
    };
    assert!(params.is_empty());
    assert_eq!(path.len(), 2);
    assert_eq!(result.interner.resolve(path[0].name), "terminus");
    assert_eq!(result.interner.resolve(path[1].name), "Terminus");
}

#[test]
fn vector_conversio_from_lista_literal_parses() {
    let result = parse_ok_la("sit _ v ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, magnitudo 4>");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ExprKind::Conversio(conversio) = &var.init.as_deref().expect("initializer").kind else {
        panic!("expected vector construction to parse as conversio");
    };
    assert!(matches!(conversio.expr.kind, ExprKind::Array(_)));
    let ConversioTarget::Explicit(target) = &conversio.target else {
        panic!("expected explicit conversio target");
    };
    let TypeExprKind::Vector(elem, FiguraExpr::Lift(extent)) = &target.kind else {
        panic!("expected vector conversio target type");
    };
    assert!(matches!(**extent, radix_syntax::LiftExtent::Literal(4)));
    let TypeExprKind::Named(name, _) = &elem.kind else {
        panic!("expected named vector element type");
    };
    assert_eq!(result.interner.resolve(name.name), "f32");
}

#[test]
fn matrix_conversio_from_nested_lista_literal_parses() {
    let result = parse_ok_la("sit _ m ← [[1.0, 0.0], [0.0, 1.0]] ↦ matrix<f32, [2, 2]>");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ExprKind::Conversio(conversio) = &var.init.as_deref().expect("initializer").kind else {
        panic!("expected matrix construction to parse as conversio");
    };
    assert!(matches!(conversio.expr.kind, ExprKind::Array(_)));
    let ConversioTarget::Explicit(target) = &conversio.target else {
        panic!("expected explicit conversio target");
    };
    let TypeExprKind::Matrix(elem, FiguraExpr::Tuple(dims)) = &target.kind else {
        panic!("expected matrix conversio target type");
    };
    let TypeExprKind::Named(name, _) = &elem.kind else {
        panic!("expected named matrix element type");
    };
    assert_eq!(result.interner.resolve(name.name), "f32");
    assert_eq!(dims.len(), 2);
}

#[test]
fn intervallum_materialize_conversio_parses_after_range() {
    let result = parse_ok_la("sit _ xs ← 0‥10 ↦ lista<numerus>");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    assert!(matches!(conversio.expr.kind, ExprKind::Intervallum(_)));
    assert!(matches!(conversio.target, ConversioTarget::Explicit(_)));
}

// bits-f16 U4 — a chained `↦` folds LEFT: `a ↦ T1 ↦ T2` parses as
// `(a ↦ T1) ↦ T2`, so `dbits ↦ f16 via Bits ↦ f32` applies
// the widen hop to the Bits hop's result (no intermediate binding).
#[test]
fn conversio_chain_folds_left() {
    let result = parse_ok_la("sit _ df ← dbits ↦ f16 via Bits ↦ f32");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(outer) = &expr.kind else {
        panic!("expected outer conversio, got {:?}", expr.kind);
    };
    // Outer hop targets f32 — the fold's final type.
    let ConversioTarget::Explicit(outer_target) = &outer.target else {
        panic!("expected explicit outer target");
    };
    let TypeExprKind::Named(outer_name, outer_params) = &outer_target.kind else {
        panic!("expected named outer target, got {:?}", outer_target.kind);
    };
    assert_eq!(result.interner.resolve(outer_name.name), "f32");
    assert!(outer_params.is_empty());
    // Left fold: the outer source is itself a conversio, not a bare operand.
    let ExprKind::Conversio(inner) = &outer.expr.kind else {
        panic!(
            "expected inner conversio under the left fold, got {:?}",
            outer.expr.kind
        );
    };
    let ConversioTarget::Explicit(inner_target) = &inner.target else {
        panic!("expected explicit inner target");
    };
    let TypeExprKind::Named(inner_name, inner_params) = &inner_target.kind else {
        panic!("expected named inner target, got {:?}", inner_target.kind);
    };
    assert_eq!(result.interner.resolve(inner_name.name), "f16");
    assert!(
        inner_params.is_empty(),
        "f16 via Bits: the hint is a clause"
    );
    let hint = inner.via.as_ref().expect("via clause on the inner hop");
    assert_eq!(result.interner.resolve(hint.name), "Bits");
    // The chain's leaf is the bare source operand.
    assert!(matches!(inner.expr.kind, ExprKind::Ident(_)));
}

// =============================================================================
// F-059: conversio target miss range tails with non-type identifiers
// F-060: speculative Integer/Float discards non-range in conversio target
// =============================================================================

#[test]
fn conversio_lowercase_ident_range_target_parses_as_intervallum() {
    // F-059: range tails after non-type idents in conversio target.
    // Lowercase identifiers that are not type keywords should be parsed
    // as range starts, producing an Intervallum target with the full range.
    let result = parse_ok_la(r"sit _ clamped ← value ↦ min‥max");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Conversio(conversio) = &expr.kind else {
        panic!("expected conversio expression, got {:?}", expr.kind);
    };
    assert!(
        matches!(conversio.target, ConversioTarget::Intervallum(_)),
        "expected intervallum target for lowercase ident range, got {:?}",
        conversio.target
    );
    let ConversioTarget::Intervallum(operand) = &conversio.target else {
        unreachable!()
    };
    assert!(
        matches!(operand.kind, ExprKind::Intervallum(_)),
        "expected IntervallumExpr inside target, got {:?}",
        operand.kind
    );
}

#[test]
fn conversio_integer_non_range_does_not_discard_token() {
    // F-060: speculative Integer/Float that does not form a range must
    // restore state so the token is not silently consumed. When followed
    // by a type keyword, the parser must not silently drop the Integer
    // and parse the type keyword as the target.
    let result = crate::test_support::parse_program_la(r"sit _ clamped ← value ↦ 42 numerus");
    assert!(
        !result.success(),
        "expected parse error for Integer followed by type keyword, got success"
    );
}

#[test]
fn parses_iuncta_construction_with_two_elements() {
    let result = parse_ok_la(r#"incipit { sit _ pair ← iuncta<textus, numerus> ["salve", 42] }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let radix_syntax::IfBody::Block(block) = &incipit.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Iuncta(iuncta) = &expr.kind else {
        panic!("expected iuncta expression, got {:?}", expr.kind);
    };
    assert_eq!(iuncta.type_args.len(), 2);
    let TypeExprKind::Named(first_type, _) = &iuncta.type_args[0].kind else {
        panic!("expected named type");
    };
    assert_eq!(result.interner.resolve(first_type.name), "textus");
    let TypeExprKind::Named(second_type, _) = &iuncta.type_args[1].kind else {
        panic!("expected named type");
    };
    assert_eq!(result.interner.resolve(second_type.name), "numerus");
    assert_eq!(iuncta.elements.len(), 2);
    assert!(matches!(
        iuncta.elements[0].kind,
        ExprKind::Literal(Literal::String(_))
    ));
    assert!(matches!(
        iuncta.elements[1].kind,
        ExprKind::Literal(Literal::Int(42))
    ));
}

#[test]
fn parses_labeled_iuncta_construction() {
    let result =
        parse_ok_la(r#"incipit { sit _ pair ← iuncta<gx: textus, gw: numerus> ["salve", 42] }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let radix_syntax::IfBody::Block(block) = &incipit.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Iuncta(iuncta) = &expr.kind else {
        panic!("expected iuncta expression, got {:?}", expr.kind);
    };
    assert_eq!(iuncta.type_args.len(), 2);
    let first_label = iuncta.type_args[0]
        .iuncta_label
        .as_ref()
        .expect("first element label");
    assert_eq!(result.interner.resolve(first_label.name), "gx");
    let second_label = iuncta.type_args[1]
        .iuncta_label
        .as_ref()
        .expect("second element label");
    assert_eq!(result.interner.resolve(second_label.name), "gw");
}

#[test]
fn parses_iuncta_construction_with_single_element() {
    let result = parse_ok_la(r#"incipit { sit _ wrap ← iuncta<textus> ["solum"] }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let radix_syntax::IfBody::Block(block) = &incipit.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Iuncta(iuncta) = &expr.kind else {
        panic!("expected iuncta expression, got {:?}", expr.kind);
    };
    assert_eq!(iuncta.type_args.len(), 1);
    assert_eq!(iuncta.elements.len(), 1);
}

#[test]
fn parses_iuncta_construction_with_nested_types() {
    let result = parse_ok_la(
        r#"incipit { sit _ nested ← iuncta<iuncta<textus, numerus>, textus> [iuncta<textus, numerus> ["a", 1], "b"] }"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let radix_syntax::IfBody::Block(block) = &incipit.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Iuncta(outer) = &expr.kind else {
        panic!("expected iuncta expression, got {:?}", expr.kind);
    };
    assert_eq!(outer.type_args.len(), 2);
    assert_eq!(outer.elements.len(), 2);
    // First element is a nested iuncta
    let ExprKind::Iuncta(inner) = &outer.elements[0].kind else {
        panic!("expected nested iuncta expression");
    };
    assert_eq!(inner.type_args.len(), 2);
    assert_eq!(inner.elements.len(), 2);
}

// ── JSON literal edge tests ─────────────────────────────────────────────────

#[test]
fn empty_array_literal_parses_as_array_expression() {
    let result = parse_ok_la("sit _ e ← []");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let init = var.init.as_deref().expect("initializer");
    let ExprKind::Array(array) = &init.kind else {
        panic!("expected array expression, got {:?}", init.kind);
    };
    assert!(array.elements.is_empty());
}

#[test]
fn json_literal_rejects_unterminated_array() {
    let result = parse_program_la("sit _ v ← [1, 2");
    assert!(
        !result.errors.is_empty(),
        "expected unterminated-array diagnostic"
    );
}

#[test]
fn json_literal_rejects_unquoted_key() {
    let result = parse_program_la(r"sit _ v ← { unquoted: 1 }");
    assert!(
        !result.errors.is_empty(),
        "expected diagnostic for unquoted JSON key"
    );
}

// ── non est edge tests ──────────────────────────────────────────────────────

#[test]
fn non_est_with_literal_rhs_is_rejected_as_a_type_test() {
    // `non est` takes a type right-hand side (D23.1); a literal is not type
    // syntax, so this is a parse error rather than a value comparison.
    let result = parse_program_la("sit _ v ← value non est 0");
    assert!(!result.success(), "literal RHS must not parse as a type");
}

#[test]
fn nihil_non_est_is_a_type_test_not_a_prefix_predicate() {
    // A value followed by `non est` starts a type test, not the retired
    // `nihil <value>` prefix predicate. Batch D made `non` tokenless; the
    // retired-prefix lookahead must not treat the operator as a value
    // operand start (`Operator` is not expression-primary).
    let result = parse_ok_la("sit _ v ← nulla non est Sacculus");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::TypeCheck(check) = &expr.kind else {
        panic!("expected type test, got {:?}", expr.kind);
    };
    assert!(!check.positive);
}

/// `≺`/`≻` are the canonical strict comparison glyphs; they parse as Lt/Gt.
#[test]
fn strict_comparison_glyphs_parse_as_lt_gt() {
    let result = parse_ok_la("sit _ a ← x ≺ y et z ≻ w");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(and) = &expr.kind else {
        panic!("expected and root, got {:?}", expr.kind);
    };
    assert_eq!(and.op, BinOp::And);
    let ExprKind::Binary(lt) = &and.lhs.kind else {
        panic!("expected comparison LHS, got {:?}", and.lhs.kind);
    };
    assert_eq!(lt.op, BinOp::Lt);
    let ExprKind::Binary(gt) = &and.rhs.kind else {
        panic!("expected comparison RHS, got {:?}", and.rhs.kind);
    };
    assert_eq!(gt.op, BinOp::Gt);
}

/// `≺` parses as the strict less-than comparison.
#[test]
fn glyph_comparison_parses_as_lt() {
    let result = parse_ok_la("sit _ a ← x ≺ y");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Binary(binary) = &expr.kind else {
        panic!("expected binary expression, got {:?}", expr.kind);
    };
    assert_eq!(binary.op, BinOp::Lt);
}

// =============================================================================
// CDA-U2 — ↤ (U+21A4) conversion-directed assignment: red parser tests
// =============================================================================
// These tests encode the `↤` grammar contract — assignment-level precedence,
// right associativity, `⊥` default legal only on `↤` — before the parser
// implements it (U3). Every fixture FAILS today at the lexer with
// `unexpected_character` for the U+21A4 glyph, the expected missing-feature
// reason. A red test that passes at this stage is a defect and must be
// corrected.

fn assign_expr(expr: &radix_syntax::Expr) -> &radix_syntax::AssignExpr {
    let ExprKind::Assign(assign) = &expr.kind else {
        panic!("expected assignment expression, got {:?}", expr.kind);
    };
    assign
}

fn expr_stmt(program: &radix_syntax::Program) -> &radix_syntax::ExprStmt {
    let StmtKind::Expr(stmt) = &program.statements[0].kind else {
        panic!("expected expression statement");
    };
    stmt
}

#[test]
fn conversio_assign_typed_fixum_initializer_parses_as_assignment() {
    let result = parse_ok_la(r#"sit numerus n ↤ "42""#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let assign = assign_expr(expr);
    // The parser must preserve which operator was written: `↤` is a distinct
    // assign operator, never erased into the ordinary `←`.
    assert!(
        !matches!(assign.op, AssignOp::Assign),
        "the ↤ operator must be preserved as a distinct assign op"
    );
    assert!(matches!(assign.target.kind, ExprKind::Ident(_)));
}

#[test]
fn conversio_assign_typed_varia_initializer_parses_as_assignment() {
    let result = parse_ok_la(r#"varia numerus n ↤ "42""#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    assert!(
        matches!(expr.kind, ExprKind::Assign(_)),
        "↤ initializer must parse as an assignment expression, got {:?}",
        expr.kind
    );
}

#[test]
fn conversio_assign_via_hint_precedes_default_in_declaration() {
    // Need: `a ↤ x via Trunc ⊥ 0` carries the hint on the assign
    // node, before the default, exactly as `x ↦ T via Trunc ⊥ 0` does.
    let result = parse_ok_la("sit numerus a ↤ x via Trunc ⊥ 0");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let assign = assign_expr(var.init.as_deref().expect("initializer"));
    assert!(matches!(assign.op, AssignOp::ConversioAssign));
    assert!(assign.via.is_some(), "the via hint must be recorded");
    assert!(assign.recovery.is_some(), "the ⊥ default must still attach");
}

#[test]
fn conversio_assign_via_hint_without_default_in_statement() {
    let result = parse_ok_la("a ↤ x via Floor");
    let program = result.program.as_ref().expect("program");
    let assign = assign_expr(&expr_stmt(program).expr);
    assert!(matches!(assign.op, AssignOp::ConversioAssign));
    assert!(assign.via.is_some());
    assert!(assign.recovery.is_none());
}

#[test]
fn plain_assign_has_no_via_hint() {
    let result = parse_ok_la("a ← x");
    let program = result.program.as_ref().expect("program");
    assert!(assign_expr(&expr_stmt(program).expr).via.is_none());
}

#[test]
fn conversio_assign_has_same_precedence_as_assign() {
    // `a ← b ↤ c` must bind as `a ← (b ↤ c)`: `↤` sits at the same
    // precedence level as `←`.
    let result = parse_ok_la("a ← b ↤ c");
    let program = result.program.as_ref().expect("program");
    let outer = assign_expr(&expr_stmt(program).expr);
    assert!(
        matches!(outer.op, AssignOp::Assign),
        "the outer written operator is `←`, so its op must stay Assign"
    );
    assert!(matches!(outer.target.kind, ExprKind::Ident(_)));
    let inner = assign_expr(&outer.value);
    assert!(
        !matches!(inner.op, AssignOp::Assign),
        "the inner operator is `↤` and must be preserved as a distinct op"
    );
}

#[test]
fn conversio_assign_is_right_associative() {
    // `a ↤ b ↤ c` must bind as `a ↤ (b ↤ c)` — right-associative like `←`.
    let result = parse_ok_la("a ↤ b ↤ c");
    let program = result.program.as_ref().expect("program");
    let outer = assign_expr(&expr_stmt(program).expr);
    assert!(matches!(outer.target.kind, ExprKind::Ident(_)));
    let inner = assign_expr(&outer.value);
    assert!(matches!(inner.target.kind, ExprKind::Ident(_)));
    assert!(
        !matches!(inner.op, AssignOp::Assign),
        "the inner operator is `↤` and must be preserved as a distinct op"
    );
}

#[test]
fn conversio_assign_recovery_attaches_to_nearest_conversio_assign_in_chain() {
    // Contract (AC4/AC9): `a ↤ b ↤ c ⊥ d` must parse with `⊥ d` attached to
    // the NEAREST `↤` in the right-associated chain (the `b ↤ (c ⊥ d)` step).
    let result = parse_ok_la("a ↤ b ↤ c ⊥ d");
    let program = result.program.as_ref().expect("program");
    let outer = assign_expr(&expr_stmt(program).expr);
    assert!(matches!(outer.target.kind, ExprKind::Ident(_)));
    let inner = assign_expr(&outer.value);
    assert!(matches!(inner.target.kind, ExprKind::Ident(_)));
    // Structural recovery assertion: the recovery rides the nearest `↤` in
    // the right-associated chain, and the outer `↤` does not claim it.
    assert!(
        inner.recovery.is_some(),
        "⊥ d must attach to the nearest ↤ (the inner b ↤ (c ⊥ d) step)"
    );
    assert!(
        outer.recovery.is_none(),
        "the outer ↤ must not claim the inner recovery"
    );
}

#[test]
fn conversio_assign_recovery_is_legal_on_conversio_assign() {
    // `a ↤ b ⊥ c` ≡ `a ← (b ↦ T ⊥ c)`: the default is an inline part of the
    // ↤ form and is carried through the syntax node's `recovery` slot.
    let result = parse_ok_la("a ↤ b ⊥ c");
    let program = result.program.as_ref().expect("program");
    let outer = assign_expr(&expr_stmt(program).expr);
    assert!(
        !matches!(outer.op, AssignOp::Assign),
        "the ↤ operator must be preserved as a distinct assign op"
    );
    assert!(outer.recovery.is_some(), "⊥ c must attach to the ↤ form");
    assert!(matches!(
        outer.recovery.as_deref().expect("recovery").kind,
        ExprKind::Ident(_)
    ));
}

#[test]
fn exit_arrow_value_is_rejected_on_ordinary_assign() {
    // D17.1: `⇥` never takes a value; on an ordinary `←` it is the same
    // structured migration diagnostic, not a silent leftover-token error.
    crate::test_support::assert_parse_error_issue_la("a ← b ⇥ c", "recovery_uses_exit_arrow");
}

#[test]
fn exit_arrow_value_after_conversion_is_a_migration_diagnostic() {
    // D17.2: `↦ T ⇥ value` is retired; `⇥` names an error type only.
    crate::test_support::assert_parse_error_issue_la(
        r#"sit _ n ← "bad" ↦ numerus ⇥ 0"#,
        "recovery_uses_exit_arrow",
    );
}

#[test]
fn exit_arrow_value_after_conversio_assign_is_a_migration_diagnostic() {
    // D17.2: `↤ … ⇥ value` is retired alongside `↦ T ⇥ value`.
    crate::test_support::assert_parse_error_issue_la("a ↤ b ⇥ c", "recovery_uses_exit_arrow");
}

#[test]
fn up_tack_default_binds_looser_than_conversion_target() {
    // D17.7 precedence: `x ↦ i32 ⊥ 0` is `(x ↦ i32) ⊥ 0`; the default is the
    // literal, not part of the target.
    let result = parse_ok_la(r"sit _ n ← x ↦ i32 ⊥ 0");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ExprKind::Conversio(conversio) = &var.init.as_deref().expect("init").kind else {
        panic!("expected conversio expression");
    };
    assert!(matches!(conversio.expr.kind, ExprKind::Ident(_)));
    assert!(matches!(
        conversio.recovery.as_deref().expect("default").kind,
        ExprKind::Literal(Literal::Int(0))
    ));
}

#[test]
fn up_tack_default_with_propagation_is_rejected() {
    // D17.7: one expression either propagates (`⇥ E`) or defaults (`⊥ v`).
    crate::test_support::assert_parse_error_issue_la(
        r#"sit _ n ← "bad" ↦ numerus ⊥ 0 ⇥ textus"#,
        "default_with_propagation",
    );
}

#[test]
fn up_tack_default_on_non_call_non_conversion_is_rejected() {
    // D17.7: `⊥` follows a failable call or a conversion; anything else is a
    // structured parse diagnostic.
    crate::test_support::assert_parse_error_issue_la(
        "sit _ n ← a ⊥ 0",
        "default_requires_failable",
    );
    crate::test_support::assert_parse_error_issue_la("a ← b ⊥ c", "default_requires_failable");
    crate::test_support::assert_parse_error_issue_la("f() ⊥ 0 ⊥ 1", "default_requires_failable");
}

fn call_default_expr(expr: &Expr) -> &CallDefaultExpr {
    let ExprKind::CallDefault(call_default) = &expr.kind else {
        panic!("expected call default, got {:?}", expr.kind);
    };
    call_default
}

#[test]
fn up_tack_default_after_call_parses_call_default() {
    // D17.7: `f(x) ⊥ v` wraps the call; the default is a unary operand.
    let result = parse_ok_la("sit _ n ← f(1) ⊥ 0");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let call_default = call_default_expr(var.init.as_deref().expect("init"));
    assert!(matches!(call_default.call.kind, ExprKind::Call(_)));
    assert!(matches!(
        call_default.default.kind,
        ExprKind::Literal(Literal::Int(0))
    ));
}

#[test]
fn up_tack_default_after_method_call_binds_the_whole_postfix_chain() {
    let result = parse_ok_la("sit _ n ← a.b(1).c() ⊥ 0 + 1");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    // `⊥` binds at the postfix tier, so `+ 1` applies to the defaulted call.
    let ExprKind::Binary(binary) = &var.init.as_deref().expect("init").kind else {
        panic!("expected binary");
    };
    let call_default = call_default_expr(&binary.lhs);
    assert!(matches!(call_default.call.kind, ExprKind::Call(_)));
}

#[test]
fn up_tack_call_default_with_propagation_is_rejected() {
    crate::test_support::assert_parse_error_issue_la(
        "sit _ n ← f() ⊥ 0 ⇥ textus",
        "default_with_propagation",
    );
}

#[test]
fn conversio_assign_bare_json_right_side_is_grammar_driven_not_type_driven() {
    // AC9: a bare `{ ... }` right side is JSON under the shipped bare-JSON
    // grammar. The parser never consults the destination's semantic type —
    // the destination here is `tabula<textus, numerus>`, which must not
    // select or change JSON grammar.
    let result = parse_ok_la(r#"sit tabula<textus, numerus> scores ↤ { "alice": 10 }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let assign = assign_expr(expr);
    assert!(
        !matches!(assign.op, AssignOp::Assign),
        "the ↤ operator must be preserved as a distinct assign op"
    );
    assert!(
        matches!(assign.value.kind, ExprKind::Literal(Literal::JsonValor(_))),
        "bare {{ ... }} must parse as a JSON literal, got {:?}",
        assign.value.kind
    );
}

// ---------------------------------------------------------------------------
// Qualified variant construction (uvf-u3): `finge pkg.Bonum { … } ∷ pkg.Exitus`
// ---------------------------------------------------------------------------

#[test]
fn qualified_finge_variant_path_and_cast_parse() {
    // uvf-u3 done_when (b): the qualified construction form parses. The
    // variant path carries the alias + variant segments and the `∷` cast is a
    // full type expression (Qualified), so the imported union identity
    // survives to lowering.
    let result = parse_ok_la(
        r#"functio main() → vacuum {
    sit _ e ← finge pkg.Bonum { nuntius = "x" } ∷ pkg.Exitus
    redde
}"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected functio declaration");
    };
    let Some(body) = &func.body else {
        panic!("expected function body");
    };
    let StmtKind::Var(var) = &body.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Finge(finge) = &expr.kind else {
        panic!("expected finge expression, got {:?}", expr.kind);
    };
    assert_eq!(
        finge.variant.len(),
        2,
        "qualified path must keep both segments"
    );
    let cast = finge.cast.as_ref().expect("qualified cast must parse");
    assert!(
        matches!(cast.kind, TypeExprKind::Qualified(ref path, ref params) if path.len() == 2 && params.is_empty()),
        "cast must be a qualified type with two segments, got {:?}",
        cast.kind
    );
}

#[test]
fn qualified_finge_cast_alone_parses() {
    // The qualified form where only the cast is qualified: `finge Bonum { … }
    // ∷ pkg.Exitus` — variant stays same-module, target type is the imported
    // union under its alias.
    let result = parse_ok_la(r#"sit _ e ← finge Bonum { nuntius = "x" } ∷ pkg.Exitus"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Finge(finge) = &expr.kind else {
        panic!("expected finge expression, got {:?}", expr.kind);
    };
    assert_eq!(
        finge.variant.len(),
        1,
        "same-module variant stays a single segment"
    );
    let cast = finge.cast.as_ref().expect("cast must parse");
    assert!(
        matches!(cast.kind, TypeExprKind::Qualified(ref path, _) if path.len() == 2),
        "cast must be the qualified type, got {:?}",
        cast.kind
    );
}

#[test]
fn same_module_finge_construction_unchanged() {
    // uvf-u3 done_when (c): same-module construction parses exactly as before
    // — a single-segment variant path and a plain named cast.
    let result = parse_ok_la(r#"sit _ e ← finge Bonum { nuntius = "x" } ∷ Exitus"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Finge(finge) = &expr.kind else {
        panic!("expected finge expression, got {:?}", expr.kind);
    };
    assert_eq!(finge.variant.len(), 1);
    let cast = finge.cast.as_ref().expect("cast must parse");
    assert!(
        matches!(cast.kind, TypeExprKind::Named(_, _)),
        "same-module cast stays a named type, got {:?}",
        cast.kind
    );
}

// ---------------------------------------------------------------------------
// Postfix transpose and gradient selection
// ---------------------------------------------------------------------------

#[test]
fn postfix_transpose_parses_as_zero_argument_method_marker() {
    let result = parse_ok_la("sit _ t ← valueᵀ");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ExprKind::Call(call) = &var.init.as_deref().expect("initializer").kind else {
        panic!("expected transpose call marker");
    };
    let ExprKind::Member(member) = &call.callee.kind else {
        panic!("expected transpose member marker");
    };
    assert_eq!(
        result.interner.resolve(member.member.name),
        "__faber_glyph_transpose"
    );
    assert!(call.args.is_empty());
    assert!(call.type_args.is_empty());
}

#[test]
fn transpose_binds_before_gradient_selection_after_multiplication() {
    let result = parse_ok_la("sit _ g ← a · bᵀ ∇ [x]");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ExprKind::Gradient(gradient) = &var.init.as_deref().expect("initializer").kind else {
        panic!("expected gradient selection, got {:?}", var.init);
    };
    let ExprKind::Binary(dot) = &gradient.call.kind else {
        panic!(
            "selection must wrap the multiplication, got {:?}",
            gradient.call.kind
        );
    };
    assert_eq!(dot.op, BinOp::Dot);
    assert!(matches!(dot.lhs.kind, ExprKind::Ident(_)));
    let ExprKind::Call(transpose) = &dot.rhs.kind else {
        panic!("transpose must stay on the dot RHS, got {:?}", dot.rhs.kind);
    };
    assert!(matches!(transpose.callee.kind, ExprKind::Member(_)));
    assert_eq!(gradient.selectors.len(), 1);
}

fn gradient_expr(result: &ParseResult, statement_index: usize) -> &radix_syntax::GradientExpr {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[statement_index].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Gradient(gradient) = &expr.kind else {
        panic!("expected gradient expression, got {:?}", expr.kind);
    };
    gradient
}

#[test]
fn bare_gradient_postfix_parses() {
    let result = parse_ok_la("sit _ g ← loss(x, y) ∇");
    let gradient = gradient_expr(&result, 0);
    assert!(gradient.selectors.is_empty(), "bare form has no selectors");
    assert!(
        matches!(gradient.call.kind, ExprKind::Call(_)),
        "gradient call must be a call, got {:?}",
        gradient.call.kind
    );
}

#[test]
fn single_selector_gradient_postfix_parses() {
    let result = parse_ok_la("sit _ g ← loss(x, y) ∇ [x]");
    let gradient = gradient_expr(&result, 0);
    assert_eq!(gradient.selectors.len(), 1, "one selector");
    assert!(
        matches!(gradient.selectors[0].kind, ExprKind::Ident(_)),
        "selector must parse as an expression, got {:?}",
        gradient.selectors[0].kind
    );
}

#[test]
fn ordered_selector_gradient_postfix_parses() {
    let result = parse_ok_la("sit _ g ← loss(x, y) ∇ [x, y]");
    let gradient = gradient_expr(&result, 0);
    assert_eq!(gradient.selectors.len(), 2);
    assert!(
        matches!(gradient.selectors[0].kind, ExprKind::Ident(_)),
        "first selector, got {:?}",
        gradient.selectors[0].kind
    );
    assert!(
        matches!(gradient.selectors[1].kind, ExprKind::Ident(_)),
        "second selector, got {:?}",
        gradient.selectors[1].kind
    );

    // Selector order is preserved verbatim: `[y, x]` stays `[y, x]`.
    let result = parse_ok_la("sit _ g ← loss(x, y) ∇ [y, x]");
    let gradient = gradient_expr(&result, 0);
    assert_eq!(gradient.selectors.len(), 2);
    assert!(
        matches!(gradient.selectors[0].kind, ExprKind::Ident(_)),
        "first selector, got {:?}",
        gradient.selectors[0].kind
    );
    assert!(
        matches!(gradient.selectors[1].kind, ExprKind::Ident(_)),
        "second selector, got {:?}",
        gradient.selectors[1].kind
    );
}

#[test]
fn gradient_selector_bracket_is_whitespace_insensitive() {
    // `∇[x]` (no space) parses to the same dedicated node; the formatters
    // re-emit the canonical ` ∇ [x]` spaced form.
    let result = parse_ok_la("sit _ g ← loss(x, y) ∇[x]");
    let gradient = gradient_expr(&result, 0);
    assert_eq!(gradient.selectors.len(), 1);
}

#[test]
fn ordinary_postfix_continuation_applies_after_gradient() {
    // The selector bracket is consumed as part of the gradient suffix, so a
    // later postfix op applies to the gradient result.
    let result = parse_ok_la("sit _ g ← loss(x, y) ∇ [x].magnitudo");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Member(member) = &expr.kind else {
        panic!(
            "member continuation must wrap the gradient, got {:?}",
            expr.kind
        );
    };
    assert!(
        matches!(member.object.kind, ExprKind::Gradient(_)),
        "member object must be the gradient expression, got {:?}",
        member.object.kind
    );
}

#[test]
fn prefix_nabla_fails_parse() {
    // `∇` is a postfix gradient operator only; a prefix `∇ x` has no grammar.
    let result = parse_program_la("sit _ g ← ∇ loss(x)");
    assert!(
        !result.success(),
        "prefix ∇ must fail parse; errors: {:?}",
        result.errors
    );
    assert!(
        result.errors.iter().any(|err| {
            err.kind == crate::ParseErrorKind::ExpressionInvalid
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "expected_expression")
        }),
        "prefix ∇ must report the expected-expression error, got {:?}",
        result.errors
    );
}

#[test]
fn empty_gradient_selector_list_fails_parse() {
    // The selector contract requires a non-empty list once `[` follows `∇`.
    crate::test_support::assert_parse_error_issue_la(
        "sit _ g ← loss(x, y) ∇ []",
        "gradient_empty_selector_list",
    );
}

#[test]
fn pd26_optional_non_null_postfix_matrix() {
    let forms = [
        ("sit _ value ← object?.member", true, "member"),
        ("sit _ value ← object?[index]", true, "index"),
        ("sit _ value ← object?(argument)", true, "call"),
        ("sit _ value ← object!.member", false, "member"),
        ("sit _ value ← object![index]", false, "index"),
        ("sit _ value ← object!(argument)", false, "call"),
    ];

    for (source, optional, payload) in forms {
        let result = parse_ok_la(source);
        let program = result.program.as_ref().expect("program");
        let StmtKind::Var(var) = &program.statements[0].kind else {
            panic!("expected variable declaration");
        };
        let expr = var.init.as_deref().expect("initializer");
        assert_eq!(
            expr.span,
            span_for(source, source.split_once("← ").unwrap().1)
        );

        if optional {
            let ExprKind::OptionalChain(chain) = &expr.kind else {
                panic!("expected optional-chain expression, got {:?}", expr.kind);
            };
            match (&chain.chain, payload) {
                (OptionalChainKind::Member(member), "member") => {
                    assert_eq!(result.interner.resolve(member.name), "member");
                }
                (OptionalChainKind::Index(index), "index") => {
                    assert!(matches!(index.kind, ExprKind::Ident(_)));
                }
                (OptionalChainKind::Call(args), "call") => {
                    assert_eq!(args.len(), 1);
                }
                (chain, payload) => panic!("unexpected optional {payload} payload: {chain:?}"),
            }
        } else {
            let ExprKind::NonNull(chain) = &expr.kind else {
                panic!("expected non-null expression, got {:?}", expr.kind);
            };
            match (&chain.chain, payload) {
                (NonNullKind::Member(member), "member") => {
                    assert_eq!(result.interner.resolve(member.name), "member");
                }
                (NonNullKind::Index(index), "index") => {
                    assert!(matches!(index.kind, ExprKind::Ident(_)));
                }
                (NonNullKind::Call(args), "call") => {
                    assert_eq!(args.len(), 1);
                }
                (chain, payload) => panic!("unexpected non-null {payload} payload: {chain:?}"),
            }
        }
    }

    let source = "sit _ value ← object?.member![index](argument) + offset";
    let result = parse_ok_la(source);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    assert_eq!(
        expr.span,
        span_for(source, "object?.member![index](argument) + offset")
    );
    let ExprKind::Binary(binary) = &expr.kind else {
        panic!("postfix must bind tighter than infix, got {:?}", expr.kind);
    };
    assert_eq!(binary.op, BinOp::Add);
    let ExprKind::Call(call) = &binary.lhs.kind else {
        panic!(
            "call must be the outermost postfix, got {:?}",
            binary.lhs.kind
        );
    };
    assert_eq!(call.args.len(), 1);
    assert_eq!(call.callee.span, span_for(source, "object?.member![index]"));
    let ExprKind::NonNull(non_null) = &call.callee.kind else {
        panic!(
            "non-null index must wrap optional member, got {:?}",
            call.callee.kind
        );
    };
    let NonNullKind::Index(index) = &non_null.chain else {
        panic!("expected non-null index payload, got {:?}", non_null.chain);
    };
    assert_eq!(index.span, span_for(source, "index"));
    assert_eq!(non_null.object.span, span_for(source, "object?.member"));
    let ExprKind::OptionalChain(optional) = &non_null.object.kind else {
        panic!(
            "expected optional member inside non-null index, got {:?}",
            non_null.object.kind
        );
    };
    assert_eq!(optional.object.span, span_for(source, "object"));
    assert!(matches!(optional.chain, OptionalChainKind::Member(_)));

    for (source, arg_name, expected) in [
        ("sit _ value ← a?.", "issue", "expected_identifier"),
        ("sit _ value ← a?[0", "issue", "optional_index_end"),
        ("sit _ value ← a?(1", "issue", "optional_call_arguments_end"),
        ("sit _ value ← a!.", "issue", "expected_identifier"),
        ("sit _ value ← a![0", "issue", "non_null_index_end"),
        ("sit _ value ← a!(1", "issue", "non_null_call_arguments_end"),
    ] {
        let result = parse_program_la(source);
        let error = result
            .errors
            .iter()
            .find(|error| {
                error.kind == ParseErrorKind::Expected
                    && error
                        .args
                        .iter()
                        .any(|arg| arg.name == arg_name && arg.value == expected)
            })
            .unwrap_or_else(|| panic!("expected {expected:?} error, got {:?}", result.errors));
        let expected_span = if expected == "expected_identifier" {
            span_for(source, if source.contains("?.") { "?." } else { "!." })
        } else {
            let end = u32::try_from(source.len()).unwrap();
            Span::new(end, end)
        };
        assert_eq!(error.span, expected_span, "source={source:?}");
    }
}

#[test]
fn template_call_carries_labeled_actual_on_string_literal_callee() {
    let result = parse_ok_la(r#"sit _ out ← "§{greet} world"(greet: "salve")"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Call(call) = &expr.kind else {
        panic!("expected template call, got {:?}", expr.kind);
    };
    assert_eq!(call.args.len(), 1);
    let label = call.args[0].label.expect("labeled actual");
    assert_eq!(result.interner.resolve(label), "greet");
    assert!(!call.args[0].is_spread);
}

#[test]
fn template_call_carries_labeled_actual_on_forma_literal_callee() {
    let result = parse_ok_la(r"sit _ out ← `… §{id}`(id: 1)");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let expr = var.init.as_deref().expect("initializer");
    let ExprKind::Call(call) = &expr.kind else {
        panic!("expected forma template call, got {:?}", expr.kind);
    };
    let label = call.args[0].label.expect("labeled actual");
    assert_eq!(result.interner.resolve(label), "id");
}

#[test]
fn labeled_actual_on_ordinary_call_is_rejected_at_parse() {
    assert_parse_error_issue_la(
        r#"sit _ out ← f(greet: "x")"#,
        "labeled_actual_non_template_call",
    );
}

#[test]
fn labeled_actual_on_optional_literal_call_is_rejected_at_parse() {
    assert_parse_error_issue_la(
        r#"sit _ out ← "§{greet}"?(greet: "x")"#,
        "labeled_actual_non_template_call",
    );
}

#[test]
fn equals_actual_reports_label_bind_distinction() {
    let result = crate::test_support::parse_program_la(r#"sit _ out ← "§ world"(greet = "x")"#);
    assert!(
        result.errors.iter().any(|error| {
            error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "label_bind_distinction")
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "label_operator" && arg.value == ":")
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "binding_operator" && arg.value == "=")
        }),
        "expected label/bind diagnostic, got {:?}",
        result.errors
    );
}

#[test]
fn duplicate_labeled_actual_is_rejected_at_parse() {
    assert_parse_error_issue_la(
        r#"sit _ out ← "§{greet}"(greet: "a", greet: "b")"#,
        "duplicate_tuple_element_label",
    );
}

#[test]
fn labeled_spread_actual_is_rejected_at_parse() {
    assert_parse_error_issue_la(
        r#"sit _ out ← "§{items}"(items: sparge values)"#,
        "labeled_actual_spread_unsupported",
    );
}

#[test]
fn index_comma_reports_coordinate_hint() {
    // A coordinate read is one bracketed list (`m[[r, c]]`); the comma after
    // the first index names the dedicated issue instead of the generic
    // `index_end`, while a bare unclosed index still reports `index_end`.
    assert_parse_error_issue_la("sit _ x ← m[r, c]", "index_coordinate_comma");
    assert_parse_error_issue_la("sit _ x ← a[1", "index_end");
}
