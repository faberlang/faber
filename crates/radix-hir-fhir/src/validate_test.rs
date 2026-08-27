use crate::decode::{decode, encode};
use crate::test_support::stub_artifact;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind, HirLiteral, HirParam,
    HirParamMode, HirRangeKind, HirStatement, HirStatementKind,
};
use radix_lexer::{Span, Symbol};
use radix_types::{DefId, Type, TypeTable, USER_DEF_ID_BASE};

fn lit(id: u32, value: u64) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: None,
        span: Span::default(),
    }
}

fn stub_function(artifact: &mut crate::artifact::HirArtifact) -> &mut radix_hir::HirFunction {
    match &mut artifact.hir.items[0].kind {
        HirItemKind::Function(func) => func,
        _ => panic!("stub artifact item must be a function"),
    }
}

#[test]
fn decode_accepts_intervallum_child_hir_ids() {
    let mut artifact = stub_artifact();
    let body = HirBlock::simple(
        Vec::new(),
        Some(Box::new(HirExpression {
            id: HirId(1),
            kind: HirExpressionKind::Intervallum {
                start: Box::new(lit(2, 0)),
                end: Box::new(lit(3, 10)),
                step: Some(Box::new(lit(4, 1))),
                kind: HirRangeKind::Exclusive,
            },
            ty: None,
            span: Span::default(),
        })),
        Span::default(),
    );
    stub_function(&mut artifact).body = Some(body);

    let bytes = encode(&artifact).expect("encode artifact");
    decode(&bytes).expect("intervallum child HirIds must be in the registry");
}

#[test]
fn decode_accepts_def_sites_inside_parameter_default() {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let ty = types.intern(Type::Error);
    artifact.types = types.snapshot();

    let closure = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Clausura(
            vec![HirParam {
                def_id: DefId(USER_DEF_ID_BASE + 2),
                name: Symbol(1),
                ty,
                mode: HirParamMode::Owned,
                optional: false,
                sponte: false,
                default: None,
                span: Span::default(),
            }],
            None,
            None,
            Box::new(lit(2, 0)),
            false,
        ),
        ty: None,
        span: Span::default(),
    };
    stub_function(&mut artifact).params.push(HirParam {
        def_id: DefId(USER_DEF_ID_BASE + 1),
        name: Symbol(0),
        ty,
        mode: HirParamMode::Owned,
        optional: true,
        sponte: false,
        default: Some(closure),
        span: Span::default(),
    });

    let bytes = encode(&artifact).expect("encode artifact");
    decode(&bytes).expect("closure params in a default must register as def sites");
}

#[test]
fn generic_applied_arg_preserves_index_kind() {
    let mut types = radix_types::TypeTable::new();
    let base = types.intern(radix_types::Type::Param(radix_lexer::Symbol(10)));
    let size = types.intern_index(radix_types::IndexExpr::Literal(3));
    let applied = types.intern(radix_types::Type::Applied(
        base,
        vec![radix_types::AppliedArg::Size(size)],
    ));
    assert!(matches!(
        types.get(applied),
        radix_types::Type::Applied(_, args)
            if matches!(args.as_slice(), [radix_types::AppliedArg::Size(id)] if *id == size)
    ));
}

// ―─ NR-5 (nary-range-walk): Cartesian range-product proofs ―──────────────

/// Stub artifact whose single function body is one N=2 `IteraRange` product
/// over two exclusive intervallum ranges, with caller-supplied binder rows.
fn range_product_artifact(bindings: Vec<(DefId, Symbol)>) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    artifact.interner = vec![
        "main".to_string(),
        "test".to_string(),
        "i".to_string(),
        "j".to_string(),
    ];
    let product = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::IteraRange {
            ranges: vec![
                HirExpression {
                    id: HirId(2),
                    kind: HirExpressionKind::Intervallum {
                        start: Box::new(lit(3, 0)),
                        end: Box::new(lit(4, 2)),
                        step: None,
                        kind: HirRangeKind::Exclusive,
                    },
                    ty: None,
                    span: Span::default(),
                },
                HirExpression {
                    id: HirId(5),
                    kind: HirExpressionKind::Intervallum {
                        start: Box::new(lit(6, 0)),
                        end: Box::new(lit(7, 3)),
                        step: None,
                        kind: HirRangeKind::Exclusive,
                    },
                    ty: None,
                    span: Span::default(),
                },
            ],
            bindings,
            // Body references the second binder by path: a well-formed
            // product must resolve in-body binder uses through the
            // registered binder defs.
            body: HirBlock::simple(
                vec![HirStatement {
                    id: HirId(8),
                    kind: HirStatementKind::Expr(HirExpression {
                        id: HirId(9),
                        kind: HirExpressionKind::Path(DefId(USER_DEF_ID_BASE + 51)),
                        ty: None,
                        span: Span::default(),
                    }),
                    span: Span::default(),
                }],
                None,
                Span::default(),
            ),
        },
        ty: None,
        span: Span::default(),
    };
    stub_function(&mut artifact).body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(10),
            kind: HirStatementKind::Expr(product),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    artifact
}

fn range_product_bindings() -> Vec<(DefId, Symbol)> {
    vec![
        (DefId(USER_DEF_ID_BASE + 50), Symbol(2)),
        (DefId(USER_DEF_ID_BASE + 51), Symbol(3)),
    ]
}

#[test]
fn itera_range_product_passes_referential_integrity() {
    // A well-formed N=2 product validates: binder defs/symbols register,
    // every range expression and body statement resolves, and the series
    // survives the wire with order and arity intact.
    let artifact = range_product_artifact(range_product_bindings());
    let bytes = encode(&artifact).expect("encode artifact");
    let mut decoded = decode(&bytes).expect("well-formed range product must validate");
    let function = stub_function(&mut decoded);
    let body = function.body.as_ref().expect("function body");
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected product expr statement");
    };
    let HirExpressionKind::IteraRange {
        ranges,
        bindings,
        body,
    } = &expr.kind
    else {
        panic!("expected IteraRange product");
    };
    assert_eq!(ranges.len(), 2, "both range axes survive the wire");
    assert_eq!(bindings.len(), 2, "both binder rows survive the wire");
    assert_eq!(bindings[0].1, Symbol(2), "first binder keeps source order");
    assert_eq!(bindings[1].1, Symbol(3), "second binder keeps source order");
    assert_eq!(body.statements.len(), 1, "product body survives the wire");
}

#[test]
fn itera_range_dangling_range_def_fails_closed() {
    // Binder defs are definition sites (they self-register); the dangling
    // surface is a use. A range extent referencing an unregistered def must
    // fail closed through the shared DanglingIndex path — never a silent
    // accept.
    let mut artifact = range_product_artifact(range_product_bindings());
    let HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    let body = function.body.as_mut().expect("function body");
    let HirStatementKind::Expr(product) = &mut body.statements[0].kind else {
        panic!("expected product expr statement");
    };
    let HirExpressionKind::IteraRange { ranges, .. } = &mut product.kind else {
        panic!("expected IteraRange product");
    };
    let HirExpressionKind::Intervallum { end, .. } = &mut ranges[1].kind else {
        panic!("expected intervallum range axis");
    };
    *end = Box::new(HirExpression {
        id: HirId(11),
        kind: HirExpressionKind::Path(DefId(USER_DEF_ID_BASE + 999)),
        ty: None,
        span: Span::default(),
    });
    let bytes = encode(&artifact).expect("encode artifact");
    let err = decode(&bytes).expect_err("dangling range def must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex {
                kind: "DefId",
                index,
                ..
            } if index == USER_DEF_ID_BASE + 999
        ),
        "dangling range def must surface as a DefId DanglingIndex: {err:?}"
    );
}

#[test]
fn itera_range_dangling_binder_symbol_fails_closed() {
    // A binder Symbol outside the interner must fail closed through the
    // shared DanglingIndex path.
    let mut bindings = range_product_bindings();
    bindings[1].1 = Symbol(97);
    let artifact = range_product_artifact(bindings);
    let bytes = encode(&artifact).expect("encode artifact");
    let err = decode(&bytes).expect_err("dangling binder symbol must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex {
                kind: "Symbol",
                index,
                ..
            } if index == 97
        ),
        "dangling binder symbol must surface as a Symbol DanglingIndex: {err:?}"
    );
}

// ── SCR-04 capability provenance validation ──────────────────────────────

#[test]
fn dangling_provenance_identity_fails_closed() {
    let mut artifact = stub_artifact();
    artifact.capability_provenance = crate::artifact::FunctionProvenanceTableWire {
        functions: vec![crate::artifact::FunctionProvenanceWire {
            function: DefId(0),
            returned_borrow: crate::artifact::ReturnedBorrowWire::None,
            required_regions: Vec::new(),
            last_uses: vec![crate::artifact::LastUseWire {
                borrower: DefId(0),
                site: radix_hir::HirId(9_999),
            }],
        }],
    };
    let err = crate::decode(&postcard::to_allocvec(&artifact).expect("encode"))
        .expect_err("dangling last-use HirId must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "HirId", .. }
        ),
        "dangling last-use site must surface as a HirId DanglingIndex: {err:?}"
    );
}
