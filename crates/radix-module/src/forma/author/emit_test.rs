use super::*;
use crate::lexer::Span;

/// Latin reader pack for direct-emit assertions (tests emit Latin).
fn latin_pack() -> crate::locale::LocalePack {
    crate::locale::latin_locale_pack()
}

#[test]
fn test_write_discerne_arms_output() {
    let interner = Interner::new();
    let mut w = CodeWriter::new();

    let discerne = DiscerneStmt {
        exhaustive: false,
        subjects: Vec::new(),
        arms: vec![CasuArm {
            patterns: vec![Pattern::Wildcard(Span::default())],
            body: IfBody::Block(BlockStmt { statements: Vec::new(), end_trivia: Vec::new(), span: Span::default() }),
            span: Span::default(),
        }],
        default: Some(CeterumDefault {
            body: IfBody::Block(BlockStmt { statements: Vec::new(), end_trivia: Vec::new(), span: Span::default() }),
            span: Span::default(),
        }),
    };

    let pack = latin_pack();
    let surface = crate::locale::KeywordSurface::new(&pack);
    AuthorEmitter::write_discerne_arms(&discerne, &surface, &interner, &mut w);
    let output = w.finish();
    assert!(output.contains("casu"));
    assert!(output.contains("ceterum"));
}

#[test]
fn test_write_conversio_multiple_type_params() {
    let mut interner = Interner::new();
    let mut w = CodeWriter::new();

    let mut id = |s: &str| {
        let sym = interner.intern(s);
        Ident { name: sym, span: Span::default() }
    };

    let type_a =
        TypeExpr { nullable: false, mode: None, kind: TypeExprKind::Named(id("A"), Vec::new()), span: Span::default() };
    let type_b =
        TypeExpr { nullable: false, mode: None, kind: TypeExprKind::Named(id("B"), Vec::new()), span: Span::default() };

    let conv = ConversioExpr {
        expr: Box::new(Expr { id: 0, kind: ExprKind::Literal(Literal::Int(42)), span: Span::default() }),
        target: ConversioTarget::Explicit(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(id("Target"), Vec::new()),
            span: Span::default(),
        }),
        type_params: vec![type_a, type_b],
        recovery: None,
    };

    let pack = latin_pack();
    let surface = crate::locale::KeywordSurface::new(&pack);
    AuthorEmitter::write_conversio(&conv, &surface, &interner, &mut w);
    let output = w.finish();
    assert!(
        output.contains("<A, B>"),
        "expected '<A, B>' in conversio output, got: {output}"
    );
    assert!(
        !output.contains("><"),
        "must not emit separate brackets per param, got: {output}"
    );
}

#[test]
fn test_write_type_expr_nullable_emits_union_nihil() {
    let mut interner = Interner::new();
    let mut w = CodeWriter::new();

    let mut id = |s: &str| {
        let sym = interner.intern(s);
        Ident { name: sym, span: Span::default() }
    };

    let ty = TypeExpr {
        nullable: true,
        mode: None,
        kind: TypeExprKind::Named(id("Textus"), Vec::new()),
        span: Span::default(),
    };

    let pack = latin_pack();
    let surface = crate::locale::KeywordSurface::new(&pack);
    AuthorEmitter::write_type_expr(&ty, &surface, &interner, &mut w);
    let output = w.finish();
    assert!(
        output.contains(" ∪ nihil"),
        "expected ' ∪ nihil' in nullable type expr output, got: {output}"
    );
}

#[test]
fn test_interna_annotation_round_trips_through_author_emit() {
    let source = "@ interna\nfunctio hidden() → vacuum {\n}\n";
    let result = crate::forma::compile_author_str("interna.fab", source);
    assert!(result.success(), "author emit failed: {:?}", result.diagnostics);
    let code = result.output.map(|output| output.code).unwrap_or_default();
    assert!(code.contains("@ interna"), "missing @ interna: {code}");
}

#[test]
fn test_interna_member_annotation_round_trips_through_author_emit() {
    let source = "genus Demo {\n    @ interna\n    functio hidden() → vacuum {\n    }\n}\n";
    let result = crate::forma::compile_author_str("interna-member.fab", source);
    assert!(result.success(), "author emit failed: {:?}", result.diagnostics);
    let code = result.output.map(|output| output.code).unwrap_or_default();
    assert!(code.contains("@ interna"), "missing @ interna: {code}");
}

#[test]
fn test_bare_gradient_round_trips_through_author_emit() {
    let source = "functio f() {\n    redde loss(x, y) ∇\n}\n";
    let result = crate::forma::compile_author_str("gradient-bare.fab", source);
    assert!(result.success(), "author emit failed: {:?}", result.diagnostics);
    let code = result.output.map(|output| output.code).unwrap_or_default();
    assert_eq!(code.trim_end(), source.trim_end(), "bare ∇ must round-trip");
}

#[test]
fn test_selected_gradient_canonicalizes_through_author_emit() {
    // Whitespace-insensitive `∇[x, y]` parses; author emit canonicalizes to
    // the spaced form with one space on both sides of `∇`.
    let source = "functio f() {\n    redde loss(x, y) ∇[x, y]\n}\n";
    let result = crate::forma::compile_author_str("gradient-selected.fab", source);
    assert!(result.success(), "author emit failed: {:?}", result.diagnostics);
    let code = result.output.map(|output| output.code).unwrap_or_default();
    let expected = "functio f() {\n    redde loss(x, y) ∇ [x, y]\n}\n";
    assert_eq!(
        code.trim_end(),
        expected.trim_end(),
        "author emit must use the canonical spaced selector form"
    );
}

#[test]
fn test_write_type_expr_nullable_false_no_union_nihil() {
    let mut interner = Interner::new();
    let mut w = CodeWriter::new();

    let mut id = |s: &str| {
        let sym = interner.intern(s);
        Ident { name: sym, span: Span::default() }
    };

    let ty = TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Named(id("Numerus"), Vec::new()),
        span: Span::default(),
    };

    let pack = latin_pack();
    let surface = crate::locale::KeywordSurface::new(&pack);
    AuthorEmitter::write_type_expr(&ty, &surface, &interner, &mut w);
    let output = w.finish();
    assert!(
        !output.contains(" ∪ nihil"),
        "must not emit null marker for non-nullable type, got: {output}"
    );
}

#[test]
fn scribe_postfix_chains_stay_flat_and_idempotent() {
    // Index / member / optional-chain objects used parent_prec 13, which
    // wrapped same-precedence postfix as `(a[i])[j]`. After `scribe` that
    // form re-parses as a call: `(scribe(a[i]))[j]`.
    let source = "\
incipit {
scribe matrix[1][0]
scribe obj.field[0]
scribe obj?.field?.inner
}
";
    let first = crate::forma::compile_author_str("scribe-postfix.fab", source);
    assert!(first.success(), "author emit failed: {:?}", first.diagnostics);
    let first_code = first.output.map(|output| output.code).unwrap_or_default();
    assert!(
        !first_code.contains("(matrix[1])")
            && !first_code.contains("(obj.field)")
            && !first_code.contains("(obj?.field)"),
        "postfix chains must stay flat so scribe does not re-parse as a call:\n{first_code}"
    );
    assert!(
        first_code.contains("scribe matrix[1][0]")
            && first_code.contains("scribe obj.field[0]")
            && first_code.contains("scribe obj?.field?.inner"),
        "expected flat scribe postfix chains:\n{first_code}"
    );
    let second = crate::forma::compile_author_str("scribe-postfix.fab", &first_code);
    assert!(second.success(), "second author emit failed: {:?}", second.diagnostics);
    let second_code = second.output.map(|output| output.code).unwrap_or_default();
    assert_eq!(first_code, second_code, "scribe postfix chains must be author-idempotent");
}
