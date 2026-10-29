use crate::decode::{decode, encode};
use crate::test_support::stub_artifact;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind, HirLiteral, HirParam,
    HirParamMode, HirRangeKind, HirStatement, HirStatementKind,
};
use radix_lexer::{Span, Symbol};
use radix_syntax::TypeFormLevel;
use radix_types::{
    BitsWidth, DefId, FloatPolicy, IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable,
    USER_DEF_ID_BASE,
};

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
fn decode_checks_cli_argument_defaults() {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let ty = types.intern(Type::Error);
    artifact.types = types.snapshot();
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.cli_args = Some(HirParam {
        def_id: DefId(USER_DEF_ID_BASE + 1),
        name: Symbol(0),
        ty,
        mode: HirParamMode::Owned,
        optional: false,
        sponte: false,
        default: Some(HirExpression {
            id: HirId(1),
            kind: HirExpressionKind::Path(DefId(999_999)),
            ty: None,
            span: Span::default(),
        }),
        span: Span::default(),
    });

    let err = crate::decode(&crate::encode(&artifact).expect("encode artifact"))
        .expect_err("a CLI argument default's dangling definition must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "DefId", .. }
        ),
        "CLI argument defaults must be checked as expressions: {err:?}"
    );
}

#[test]
fn decode_accepts_synthesized_backward_companion_identity() {
    use crate::artifact::{RadixBackwardWire, RadixLaneEntryWire, RadixLaneWire};
    let mut artifact = stub_artifact();
    artifact.radix_lanes.push(RadixLaneEntryWire {
        def_id: DefId(0),
        lane: RadixLaneWire::Air,
        backward: Some(RadixBackwardWire {
            companion_name: Symbol(0),
            companion_def_id: DefId(radix_types::COMPANION_DEF_ID_FLAG),
            selected_params: None,
        }),
    });
    decode(&encode(&artifact).expect("encode artifact"))
        .expect("a companion identity is synthesized, not declared in the artifact");

    let backward = artifact.radix_lanes[0].backward.as_mut().expect("backward");
    backward.companion_def_id = DefId(999_999);
    let err = decode(&encode(&artifact).expect("encode artifact"))
        .expect_err("a companion identity outside the companion family must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "DefId", .. }
        ),
        "{err:?}"
    );
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
            mutable: false,
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
        ..
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
    **end = HirExpression {
        id: HirId(11),
        kind: HirExpressionKind::Path(DefId(USER_DEF_ID_BASE + 999)),
        ty: None,
        span: Span::default(),
    };
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

// ── F20 resolved-use wire validation ─────────────────────────────────────

#[test]
fn dangling_resolved_use_local_fails_closed() {
    let mut artifact = stub_artifact();
    artifact
        .resolved_uses
        .push(crate::artifact::ResolvedUseWire {
            key: crate::artifact::ResolvedUseKeyWire::Portable("pkg::item".to_string()),
            kind: crate::artifact::ResolvedUseKindWire::Path,
            enclosing: None,
            local: Some(DefId(999_999)),
        });
    let err = crate::decode(&postcard::to_allocvec(&artifact).expect("encode"))
        .expect_err("dangling resolved-use local DefId must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "DefId", .. }
        ),
        "dangling resolved-use local must surface as a DefId DanglingIndex: {err:?}"
    );
}

#[test]
fn valid_resolved_use_local_round_trips() {
    let mut artifact = stub_artifact();
    artifact.resolved_uses = vec![
        crate::artifact::ResolvedUseWire {
            key: crate::artifact::ResolvedUseKeyWire::Portable("pkg::item".to_string()),
            kind: crate::artifact::ResolvedUseKindWire::Path,
            enclosing: None,
            local: Some(DefId(0)),
        },
        crate::artifact::ResolvedUseWire {
            key: crate::artifact::ResolvedUseKeyWire::Portable("pkg::other".to_string()),
            kind: crate::artifact::ResolvedUseKindWire::ImportBinding,
            enclosing: None,
            local: None,
        },
    ];
    let bytes = encode(&artifact).expect("encode artifact");
    let decoded = decode(&bytes).expect("valid resolved-use local must round-trip");
    assert_eq!(decoded.resolved_uses, artifact.resolved_uses);
}

fn authored_type_artifact() -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_two = types.intern_index(IndexExpr::Literal(2));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim_two, dim_two]));
    let tensor_ty = types.tensor_with_shape(f32_ty, shape);
    let alias_def = DefId(USER_DEF_ID_BASE + 10);
    let alias_ty = types.intern(Type::Alias(alias_def, u32_ty));
    let parameter_def = DefId(USER_DEF_ID_BASE + 11);
    let alias_local_def = DefId(USER_DEF_ID_BASE + 12);
    let hole_local_def = DefId(USER_DEF_ID_BASE + 13);

    artifact.interner = vec![
        "main".to_owned(),
        "test".to_owned(),
        "Word".to_owned(),
        "word".to_owned(),
        "unknown".to_owned(),
    ];
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.params.push(HirParam {
        def_id: parameter_def,
        name: Symbol(3),
        ty: u32_ty,
        mode: HirParamMode::Owned,
        optional: false,
        sponte: false,
        default: None,
        span: Span::default(),
    });
    function.ret_ty = Some(tensor_ty);
    function.body = Some(HirBlock::simple(
        vec![
            HirStatement {
                id: HirId(1),
                kind: HirStatementKind::Local(radix_hir::HirLocal {
                    def_id: alias_local_def,
                    name: Symbol(3),
                    ty: Some(alias_ty),
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: Span::default(),
            },
            HirStatement {
                id: HirId(2),
                kind: HirStatementKind::Local(radix_hir::HirLocal {
                    def_id: hole_local_def,
                    name: Symbol(4),
                    ty: Some(u32_ty),
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: Span::default(),
            },
        ],
        None,
        Span::default(),
    ));
    artifact.hir.items.push(radix_hir::HirItem {
        annotations: Vec::new(),
        id: HirId(3),
        def_id: alias_def,
        kind: radix_hir::HirItemKind::TypeAlias(radix_hir::HirTypeAlias {
            name: Symbol(2),
            type_params: Vec::new(),
            ty: u32_ty,
        }),
        span: Span::default(),
    });
    artifact.types = types.snapshot();
    artifact.presentation.authored_types = vec![
        radix_hir::HirAuthoredTypeUse {
            site: radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Def(parameter_def),
                slot: radix_hir::HirTypeSlot::Parameter(0),
            },
            form: radix_hir::HirTypeForm {
                ty: u32_ty,
                head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::LONG),
                args: Vec::new(),
                label: None,
            },
        },
        radix_hir::HirAuthoredTypeUse {
            site: radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Def(DefId(0)),
                slot: radix_hir::HirTypeSlot::Return,
            },
            form: radix_hir::HirTypeForm {
                ty: tensor_ty,
                head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::LONG),
                args: vec![
                    radix_hir::HirTypeFormArg::Type(Box::new(radix_hir::HirTypeForm {
                        ty: f32_ty,
                        head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::SHORT),
                        args: Vec::new(),
                        label: None,
                    })),
                    radix_hir::HirTypeFormArg::Size(shape),
                ],
                label: None,
            },
        },
        radix_hir::HirAuthoredTypeUse {
            site: radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Def(alias_local_def),
                slot: radix_hir::HirTypeSlot::Declared,
            },
            form: radix_hir::HirTypeForm {
                ty: alias_ty,
                head: radix_hir::HirTypeFormHead::Named {
                    def_id: alias_def,
                    path: vec![Symbol(2)],
                },
                args: Vec::new(),
                label: None,
            },
        },
        radix_hir::HirAuthoredTypeUse {
            site: radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Def(hole_local_def),
                slot: radix_hir::HirTypeSlot::Declared,
            },
            form: radix_hir::HirTypeForm {
                ty: u32_ty,
                head: radix_hir::HirTypeFormHead::Infer,
                args: Vec::new(),
                label: None,
            },
        },
    ];
    artifact
}

/// Artifact with one declared local whose finalized type is
/// `lista<element>` and whose authored form is the zero-argument semantic
/// spelling.
fn zero_arg_array_artifact(element: Type) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let element_ty = types.intern(element);
    let array_ty = types.array(element_ty);
    let local_def = DefId(USER_DEF_ID_BASE + 20);
    artifact.interner = vec!["main".to_owned(), "test".to_owned(), "b".to_owned()];
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Local(radix_hir::HirLocal {
                def_id: local_def,
                name: Symbol(2),
                ty: Some(array_ty),
                init: None,
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    artifact.types = types.snapshot();
    artifact.presentation.authored_types = vec![radix_hir::HirAuthoredTypeUse {
        site: radix_hir::HirTypeSite {
            owner: radix_hir::HirOwnerRef::Def(local_def),
            slot: radix_hir::HirTypeSlot::Declared,
        },
        form: radix_hir::HirTypeForm {
            ty: array_ty,
            head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::LONG),
            args: Vec::new(),
            label: None,
        },
    }];
    artifact
}

#[test]
fn zero_argument_u8_array_form_validates() {
    // `octeti`'s semantic witness is the byte-list array; the zero-argument
    // authored form is accepted for the u8 element in either byte spelling.
    let byte_spellings = [
        Type::Bits(radix_types::BitsWidth::B8),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8),
    ];
    for element in byte_spellings {
        let artifact = zero_arg_array_artifact(element.clone());
        crate::decode(&crate::encode(&artifact).expect("encode u8-array artifact")).unwrap_or_else(
            |err| panic!("zero-argument u8 array {element:?} must validate: {err:?}"),
        );
    }
}

#[test]
fn zero_argument_non_u8_array_form_fails_closed() {
    let rejected_elements = [
        Type::Primitive(Primitive::Textus),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U16),
    ];
    for element in rejected_elements {
        let artifact = zero_arg_array_artifact(element.clone());
        let err = crate::decode(&crate::encode(&artifact).expect("encode non-u8-array artifact"))
            .expect_err("a zero-argument non-u8 array form must fail closed");
        assert!(
            matches!(
                err,
                crate::error::HirArtifactError::InvalidPresentation {
                    issue: "array_argument_arity",
                    ..
                }
            ),
            "zero-argument array over {element:?} must fail with array_argument_arity: {err:?}"
        );
    }
}

/// Artifact with one declared local whose finalized type is `witness(element)`
/// and whose authored form is an `ImportedAlias` head spelled `decl.Quad`.
fn imported_alias_artifact(
    witness: impl FnOnce(&mut TypeTable, radix_types::TypeId) -> radix_types::TypeId,
    path: Vec<Symbol>,
    args: Vec<radix_hir::HirTypeFormArg>,
) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let element_ty = types.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::U16));
    let alias_ty = witness(&mut types, element_ty);
    let local_def = DefId(USER_DEF_ID_BASE + 20);
    artifact.interner = vec![
        "main".to_owned(),
        "test".to_owned(),
        "b".to_owned(),
        "decl".to_owned(),
        "Quad".to_owned(),
    ];
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Local(radix_hir::HirLocal {
                def_id: local_def,
                name: Symbol(2),
                ty: Some(alias_ty),
                init: None,
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    artifact.types = types.snapshot();
    artifact.presentation.authored_types = vec![radix_hir::HirAuthoredTypeUse {
        site: radix_hir::HirTypeSite {
            owner: radix_hir::HirOwnerRef::Def(local_def),
            slot: radix_hir::HirTypeSlot::Declared,
        },
        form: radix_hir::HirTypeForm {
            ty: alias_ty,
            head: radix_hir::HirTypeFormHead::ImportedAlias { path },
            args,
            label: None,
        },
    }];
    artifact
}

#[test]
fn imported_alias_head_over_collection_bodies_round_trips() {
    // An imported alias reaches the importer as its resolved body, so the
    // head names the authored path and the witness is the whole presentation,
    // whatever shape the body has (a sized list is the case the corpus hit).
    let bodies: [fn(&mut TypeTable, radix_types::TypeId) -> radix_types::TypeId; 3] = [
        |types, element| types.array(element),
        |types, element| {
            let capacity = types.intern_index(radix_types::IndexExpr::Literal(4));
            types.bounded_array(element, capacity)
        },
        |types, element| types.ranged_array(element, 1, Some(8)),
    ];
    for body in bodies {
        let artifact = imported_alias_artifact(body, vec![Symbol(3), Symbol(4)], Vec::new());
        let decoded =
            crate::decode(&crate::encode(&artifact).expect("encode imported-alias artifact"))
                .expect("an imported alias head over a collection body must validate");
        let authored = &decoded.presentation.authored_types[0].form;
        assert_eq!(
            authored.head,
            radix_hir::HirTypeFormHead::ImportedAlias {
                path: vec![Symbol(3), Symbol(4)]
            }
        );
    }
}

#[test]
fn imported_alias_head_fails_closed_on_arguments_and_path() {
    let array = |types: &mut TypeTable, element| types.array(element);
    let child = |element| {
        radix_hir::HirTypeFormArg::Type(Box::new(radix_hir::HirTypeForm {
            ty: element,
            head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::LONG),
            args: Vec::new(),
            label: None,
        }))
    };
    let with_arguments = imported_alias_artifact(
        array,
        vec![Symbol(3), Symbol(4)],
        vec![child(radix_types::TypeId(0))],
    );
    let err = crate::decode(&crate::encode(&with_arguments).expect("encode"))
        .expect_err("an imported alias head carries no authored arguments");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "imported_alias_arguments",
                ..
            }
        ),
        "{err:?}"
    );

    let empty_path = imported_alias_artifact(array, Vec::new(), Vec::new());
    let err = crate::decode(&crate::encode(&empty_path).expect("encode"))
        .expect_err("an imported alias head names a path");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "empty_imported_alias_path",
                ..
            }
        ),
        "{err:?}"
    );

    let dangling = imported_alias_artifact(array, vec![Symbol(99)], Vec::new());
    crate::decode(&crate::encode(&dangling).expect("encode"))
        .expect_err("an imported alias path symbol must exist in the interner");
}

/// Artifact with one declared local whose finalized type is the bounded byte
/// buffer `list<b8, 8>` ([`Type::BoundedOcteti`]); the authored form is a
/// semantic head at `level` carrying the `args` built from the interned
/// `b8` element type.
fn bounded_octeti_artifact(
    level: TypeFormLevel,
    args: impl FnOnce(radix_types::TypeId) -> Vec<radix_hir::HirTypeFormArg>,
) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let b8 = types.intern(Type::Bits(BitsWidth::B8));
    let capacity = types.intern_index(radix_types::IndexExpr::Literal(8));
    let witness = types.bounded_array(b8, capacity);
    assert!(matches!(types.get(witness), Type::BoundedOcteti { .. }));
    let local_def = DefId(USER_DEF_ID_BASE + 20);
    artifact.interner = vec!["main".to_owned(), "test".to_owned(), "b".to_owned()];
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Local(radix_hir::HirLocal {
                def_id: local_def,
                name: Symbol(2),
                ty: Some(witness),
                init: None,
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    let args = args(b8);
    artifact.types = types.snapshot();
    artifact.presentation.authored_types = vec![radix_hir::HirAuthoredTypeUse {
        site: radix_hir::HirTypeSite {
            owner: radix_hir::HirOwnerRef::Def(local_def),
            slot: radix_hir::HirTypeSlot::Declared,
        },
        form: radix_hir::HirTypeForm {
            ty: witness,
            head: radix_hir::HirTypeFormHead::Semantic(level),
            args,
            label: None,
        },
    }];
    artifact
}

fn bits_child(ty: radix_types::TypeId) -> radix_hir::HirTypeFormArg {
    radix_hir::HirTypeFormArg::Type(Box::new(radix_hir::HirTypeForm {
        ty,
        head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::SHORT),
        args: Vec::new(),
        label: None,
    }))
}

#[test]
fn bounded_octeti_authored_list_form_validates() {
    // The authored `list<b8, N>`: a LONG list head over one `b8` element, the
    // capacity carried by the root witness.
    let artifact = bounded_octeti_artifact(TypeFormLevel::LONG, |b8| vec![bits_child(b8)]);
    crate::decode(&crate::encode(&artifact).expect("encode bounded octeti artifact"))
        .unwrap_or_else(|err| panic!("an authored list<b8, N> form must validate: {err:?}"));
}

fn assert_bounded_octeti_rejected(
    artifact: &crate::artifact::HirArtifact,
    issue: &str,
    what: &str,
) {
    let err = crate::decode(&crate::encode(artifact).expect("encode malformed artifact"))
        .expect_err("a malformed bounded octeti form must fail closed");
    assert!(
        matches!(
            &err,
            crate::error::HirArtifactError::InvalidPresentation { issue: got, .. } if *got == issue
        ),
        "{what}: expected {issue}, got {err:?}"
    );
}

#[test]
fn bounded_octeti_malformed_forms_fail_closed() {
    // No element argument (the old zero-argument spelling).
    let artifact = bounded_octeti_artifact(TypeFormLevel::LONG, |_| Vec::new());
    assert_bounded_octeti_rejected(&artifact, "bounded_octeti_shape", "no element argument");

    // A list head is LONG; a short level is not a list spelling.
    let artifact = bounded_octeti_artifact(TypeFormLevel::SHORT, |b8| vec![bits_child(b8)]);
    assert_bounded_octeti_rejected(&artifact, "bounded_octeti_shape", "short level");

    // A second argument has no place: the capacity is the root witness.
    let artifact = bounded_octeti_artifact(TypeFormLevel::LONG, |b8| {
        vec![bits_child(b8), bits_child(b8)]
    });
    assert_bounded_octeti_rejected(&artifact, "bounded_octeti_shape", "two arguments");

    // A size argument in place of the element type.
    let artifact = bounded_octeti_artifact(TypeFormLevel::LONG, |_| {
        vec![radix_hir::HirTypeFormArg::Size(radix_types::IndexId(0))]
    });
    assert_bounded_octeti_rejected(&artifact, "type_argument_kind", "size argument");

    // An element that is not the raw-byte word.
    let artifact = bounded_octeti_artifact(TypeFormLevel::LONG, |_| {
        vec![bits_child(radix_types::TypeId(0))]
    });
    assert_bounded_octeti_rejected(&artifact, "bounded_octeti_element", "non-b8 element");
}

/// Artifact with one generic transparent alias `type Pairs<T> = list<T>` and
/// one declared local whose finalized witness is `list<int>`; the authored
/// form names the alias head and carries `child` as its one type argument.
fn generic_alias_named_form_artifact(child: Type) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let int_ty = types.primitive(Primitive::Numerus);
    let child_ty = types.intern(child);
    let param_symbol = Symbol(3);
    let param_ty = types.intern(Type::Param(param_symbol));
    let alias_body = types.array(param_ty);
    let array_ty = types.array(int_ty);
    let pairs_def = DefId(USER_DEF_ID_BASE + 30);
    let param_def = DefId(USER_DEF_ID_BASE + 31);
    let local_def = DefId(USER_DEF_ID_BASE + 32);
    let _alias_ty = types.intern(Type::Alias(pairs_def, alias_body));
    // The producer also records the instantiated alias wrapper at the applied
    // site, which is why the generic fallback must run before the exact-body
    // witness (that witness carries no argument witnesses).
    let _instantiated_alias_ty = types.intern(Type::Alias(pairs_def, array_ty));

    artifact.interner = vec![
        "main".to_owned(),
        "test".to_owned(),
        "Pairs".to_owned(),
        "T".to_owned(),
    ];
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Local(radix_hir::HirLocal {
                def_id: local_def,
                name: Symbol(0),
                ty: Some(array_ty),
                init: None,
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    artifact.hir.items.push(radix_hir::HirItem {
        annotations: Vec::new(),
        id: HirId(2),
        def_id: pairs_def,
        kind: radix_hir::HirItemKind::TypeAlias(radix_hir::HirTypeAlias {
            name: Symbol(2),
            type_params: vec![radix_hir::HirTypeParam {
                def_id: param_def,
                kind: radix_syntax::GenericParamKind::Typus,
                name: param_symbol,
                span: Span::default(),
                constraint: radix_hir::HirTypeParamConstraint::Any,
                default: None,
            }],
            ty: alias_body,
        }),
        span: Span::default(),
    });
    artifact.types = types.snapshot();
    artifact.presentation.authored_types = vec![radix_hir::HirAuthoredTypeUse {
        site: radix_hir::HirTypeSite {
            owner: radix_hir::HirOwnerRef::Def(local_def),
            slot: radix_hir::HirTypeSlot::Declared,
        },
        form: radix_hir::HirTypeForm {
            ty: array_ty,
            head: radix_hir::HirTypeFormHead::Named {
                def_id: pairs_def,
                path: vec![Symbol(2)],
            },
            args: vec![radix_hir::HirTypeFormArg::Type(Box::new(
                radix_hir::HirTypeForm {
                    ty: child_ty,
                    head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::LONG),
                    args: Vec::new(),
                    label: None,
                },
            ))],
            label: None,
        },
    }];
    artifact
}

#[test]
fn generic_alias_named_form_annotation_loads() {
    // `type Pairs<T> = list<T>` applied as `Pairs<int>` at a `list<int>` site:
    // the recorded alias body is parametric, so the named form matches it only
    // through the generic fallback's structural binding of `T`.
    let artifact = generic_alias_named_form_artifact(Type::Primitive(Primitive::Numerus));
    crate::decode(&crate::encode(&artifact).expect("encode generic alias artifact"))
        .unwrap_or_else(|err| panic!("a generic transparent alias annotation must load: {err:?}"));
}

#[test]
fn generic_alias_named_form_wrong_arg_fails_closed() {
    // The site witness stays `list<int>` while the authored argument witnesses
    // `string`; binding `T` to `string` cannot match `list<int>`, so the
    // generic fallback refuses exactly like the non-generic witness path.
    let artifact = generic_alias_named_form_artifact(Type::Primitive(Primitive::Textus));
    let err = crate::decode(&crate::encode(&artifact).expect("encode generic alias artifact"))
        .expect_err("a generic alias argument that contradicts the site witness must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "named_witness_not_named",
                ..
            }
        ),
        "a mis-bound generic alias argument must fail with named_witness_not_named: {err:?}"
    );
}

/// Artifact with one generic transparent alias `type Pairs<T> = list<T>` and
/// one declared local whose finalized witness is `list<element>`; the authored
/// form names the alias head and carries `u32` as its one type argument.
fn generic_alias_type_artifact(element: Type) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let param_ty = types.intern(Type::Param(Symbol(2)));
    let body_ty = types.array(param_ty);
    let alias_def = DefId(USER_DEF_ID_BASE + 10);
    // The mere presence of the recorded alias wrapper is the alias fact the
    // generic fallback consumes; it binds to nothing at any site.
    let _alias_record_ty = types.intern(Type::Alias(alias_def, body_ty));
    let element_id = types.intern(element);
    let witness_ty = types.array(element_id);
    let local_def = DefId(USER_DEF_ID_BASE + 11);
    let param_def = DefId(USER_DEF_ID_BASE + 12);

    artifact.interner = vec!["main".to_owned(), "Pairs".to_owned(), "T".to_owned()];
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Local(radix_hir::HirLocal {
                def_id: local_def,
                name: Symbol(0),
                ty: Some(witness_ty),
                init: None,
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    artifact.hir.items.push(radix_hir::HirItem {
        annotations: Vec::new(),
        id: HirId(2),
        def_id: alias_def,
        kind: radix_hir::HirItemKind::TypeAlias(radix_hir::HirTypeAlias {
            name: Symbol(1),
            type_params: vec![radix_hir::HirTypeParam {
                def_id: param_def,
                kind: radix_syntax::GenericParamKind::Typus,
                name: Symbol(2),
                span: Span::default(),
                constraint: radix_hir::HirTypeParamConstraint::Any,
                default: None,
            }],
            ty: body_ty,
        }),
        span: Span::default(),
    });
    artifact.types = types.snapshot();
    artifact.presentation.authored_types = vec![radix_hir::HirAuthoredTypeUse {
        site: radix_hir::HirTypeSite {
            owner: radix_hir::HirOwnerRef::Def(local_def),
            slot: radix_hir::HirTypeSlot::Declared,
        },
        form: radix_hir::HirTypeForm {
            ty: witness_ty,
            head: radix_hir::HirTypeFormHead::Named {
                def_id: alias_def,
                path: vec![Symbol(1)],
            },
            args: vec![radix_hir::HirTypeFormArg::Type(Box::new(
                radix_hir::HirTypeForm {
                    ty: u32_ty,
                    head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::SHORT),
                    args: Vec::new(),
                    label: None,
                },
            ))],
            label: None,
        },
    }];
    artifact
}

#[test]
fn generic_alias_annotation_loads() {
    // `Pairs<u32>` applied at a `list<u32>` site: the recorded alias body is
    // parametric, so the authored named form agrees with the site witness
    // only through the generic fallback's structural binding of `T`.
    let element = Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32);
    let artifact = generic_alias_type_artifact(element);
    crate::decode(&crate::encode(&artifact).expect("encode generic alias artifact"))
        .unwrap_or_else(|err| panic!("a generic transparent alias annotation must load: {err:?}"));
}

#[test]
fn generic_alias_annotation_mismatched_element_fails_closed() {
    // The site witness is `list<u64>` while the authored argument binds `T` to
    // `u32`; the widths share the numeric variant, so only the structural
    // match catches the contradiction and it must refuse rather than accept.
    let element = Type::SizedNumeric(Primitive::Numerus, NumericWidth::U64);
    let artifact = generic_alias_type_artifact(element);
    let err = crate::decode(&crate::encode(&artifact).expect("encode generic alias artifact"))
        .expect_err("a generic alias whose element contradicts the authored argument must fail");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "named_witness_not_named",
                ..
            }
        ),
        "a mismatched generic alias element must fail with named_witness_not_named: {err:?}"
    );
}

#[test]
fn authored_types_v9_round_trip_preserves_nested_forms() {
    let artifact = authored_type_artifact();
    let bytes = crate::encode(&artifact).expect("encode authored type artifact");
    let decoded = crate::decode(&bytes).expect("schema-v9 authored type artifact must validate");
    assert_eq!(
        decoded.presentation.authored_types,
        artifact.presentation.authored_types
    );

    let msgpack = crate::msgpack::encode_msgpack(&artifact).expect("encode msgpack artifact");
    let decoded_msgpack =
        crate::msgpack::decode_msgpack(&msgpack).expect("decode msgpack artifact");
    assert_eq!(
        decoded_msgpack.presentation.authored_types,
        artifact.presentation.authored_types
    );
}

/// Rewrite the sole parameter's authored form into a zero-argument semantic
/// head over `witness` at `level`, keeping the HIR parameter type in step so
/// the root witness still matches the site.
fn semantic_width_artifact(witness: Type, level: TypeFormLevel) -> crate::artifact::HirArtifact {
    let mut artifact = authored_type_artifact();
    let mut types = TypeTable::from_snapshot(artifact.types.clone()).expect("restore type table");
    let witness_ty = types.intern(witness);
    artifact.types = types.snapshot();
    let HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.params[0].ty = witness_ty;
    artifact.presentation.authored_types[0].form = radix_hir::HirTypeForm {
        ty: witness_ty,
        head: radix_hir::HirTypeFormHead::Semantic(level),
        args: Vec::new(),
        label: None,
    };
    artifact
}

#[test]
fn bare_bits_annotation_validates() {
    // A bare `b8` width word is authored at SHORT level with no arguments;
    // the loader must accept the form the parser records for it.
    let artifact = semantic_width_artifact(Type::Bits(BitsWidth::B8), TypeFormLevel::SHORT);
    crate::decode(&crate::encode(&artifact).expect("encode bare bits artifact"))
        .unwrap_or_else(|err| panic!("a bare b8 annotation must validate: {err:?}"));
}

#[test]
fn trapping_float_policy_form_validates() {
    // `trapping<f32>` is authored at POLICY level with no arguments.
    let artifact = semantic_width_artifact(
        Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F32),
        TypeFormLevel::POLICY,
    );
    crate::decode(&crate::encode(&artifact).expect("encode trapping float artifact"))
        .unwrap_or_else(|err| panic!("a trapping<f32> annotation must validate: {err:?}"));
}

#[test]
fn duplicate_authored_type_site_fails_closed() {
    let mut artifact = authored_type_artifact();
    artifact
        .presentation
        .authored_types
        .push(artifact.presentation.authored_types[0].clone());
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("duplicate type sites must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::InvalidPresentation {
            issue: "duplicate_type_site",
            ..
        }
    ));
}

#[test]
fn unknown_authored_type_owner_fails_closed() {
    let mut artifact = authored_type_artifact();
    artifact.presentation.authored_types[0].site.owner =
        radix_hir::HirOwnerRef::Def(DefId(USER_DEF_ID_BASE + 999));
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("an unknown type-site owner must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::DanglingIndex { kind: "DefId", .. }
    ));
}

#[test]
fn nonexistent_type_slot_fails_closed() {
    let mut artifact = authored_type_artifact();
    artifact.presentation.authored_types[0].site.slot = radix_hir::HirTypeSlot::AliasBody;
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("a slot not legal for the owner must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::InvalidPresentation {
            issue: "unknown_type_slot",
            ..
        }
    ));
}

/// A by-label `iuncta` read must name a real node; a dangling `HirId` is a
/// producer defect and fails closed instead of silently keeping `[i]`.
#[test]
fn dangling_tuple_label_access_fails_closed() {
    let mut artifact = authored_type_artifact();
    artifact
        .presentation
        .tuple_label_accesses
        .push(radix_hir::HirTupleLabelAccess {
            expr_id: radix_hir::HirId(9_999),
            label: Symbol(1),
        });
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("a dangling label-access id must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "HirId", .. }
        ),
        "dangling label access must surface as a HirId DanglingIndex: {err:?}"
    );
}

#[test]
fn unknown_named_definition_fails_closed() {
    let mut artifact = authored_type_artifact();
    let record = &mut artifact.presentation.authored_types[2];
    let radix_hir::HirTypeFormHead::Named { def_id, .. } = &mut record.form.head else {
        panic!("fixture must have a named form");
    };
    *def_id = DefId(USER_DEF_ID_BASE + 999);
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("unknown named definition must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::DanglingIndex { kind: "DefId", .. }
    ));
}

#[test]
fn named_definition_kind_mismatch_fails_closed() {
    let mut artifact = authored_type_artifact();
    let record = &mut artifact.presentation.authored_types[2];
    let radix_hir::HirTypeFormHead::Named { def_id, .. } = &mut record.form.head else {
        panic!("fixture must have a named form");
    };
    *def_id = DefId(0);
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("a function definition cannot be a named type");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::InvalidPresentation {
            issue: "named_def_not_type",
            ..
        }
    ));
}

#[test]
fn unsupported_authored_level_fails_closed() {
    let mut artifact = authored_type_artifact();
    let radix_hir::HirTypeFormHead::Semantic(level) =
        &mut artifact.presentation.authored_types[0].form.head
    else {
        panic!("fixture must have a semantic form");
    };
    *level = TypeFormLevel(3);
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("unsupported level must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::InvalidPresentation {
            issue: "numeric_level_or_arity",
            ..
        }
    ));
}

#[test]
fn wrong_authored_arity_fails_closed() {
    let mut artifact = authored_type_artifact();
    artifact.presentation.authored_types[1].form.args.pop();
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("wrong aggregate arity must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::InvalidPresentation {
            issue: "aggregate_level_or_arity",
            ..
        }
    ));
}

#[test]
fn unresolved_authored_type_id_fails_closed() {
    let mut artifact = authored_type_artifact();
    artifact.presentation.authored_types[0].form.ty = TypeId(99_999);
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("unresolved authored TypeId must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::DanglingIndex { kind: "TypeId", .. }
    ));
}

#[test]
fn hole_at_non_hole_type_slot_fails_closed() {
    let mut artifact = authored_type_artifact();
    artifact.presentation.authored_types[0].form.head = radix_hir::HirTypeFormHead::Infer;
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("parameter hole must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::InvalidPresentation {
            issue: "hole_in_illegal_slot",
            ..
        }
    ));

    let mut artifact = authored_type_artifact();
    artifact.presentation.authored_types[1].form.head = radix_hir::HirTypeFormHead::Infer;
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("return hole must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::InvalidPresentation {
            issue: "hole_in_illegal_slot",
            ..
        }
    ));
}

/// A closure parameter written `_ m` records its authored hole at the
/// parameter's `ClosureParameter` slot; the compiler infers the parameter
/// type from the call and the root-witness check pins the recorded witness,
/// so the hole is legal there. A declared-parameter hole still fails closed
/// through [`hole_at_non_hole_type_slot_fails_closed`].
#[test]
fn closure_parameter_hole_type_slot_loads() {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let ty = types.primitive(Primitive::Numerus);
    artifact.types = types.snapshot();
    let param_def = DefId(USER_DEF_ID_BASE + 2);
    let closure = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Clausura(
            vec![HirParam {
                def_id: param_def,
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
        ),
        ty: None,
        span: Span::default(),
    };
    stub_function(&mut artifact).body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(3),
            kind: HirStatementKind::Expr(closure),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    artifact.presentation.authored_types = vec![radix_hir::HirAuthoredTypeUse {
        site: radix_hir::HirTypeSite {
            owner: radix_hir::HirOwnerRef::Def(param_def),
            slot: radix_hir::HirTypeSlot::ClosureParameter(0),
        },
        form: radix_hir::HirTypeForm {
            ty,
            head: radix_hir::HirTypeFormHead::Infer,
            args: Vec::new(),
            label: None,
        },
    }];
    let bytes = encode(&artifact).expect("encode artifact");
    decode(&bytes).expect("a closure-parameter hole is a legal authored hole");
}

#[test]
fn unresolved_named_path_symbol_fails_closed() {
    let mut artifact = authored_type_artifact();
    let radix_hir::HirTypeFormHead::Named { path, .. } =
        &mut artifact.presentation.authored_types[2].form.head
    else {
        panic!("fixture must have a named form");
    };
    path[0] = Symbol(99_999);
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("unresolved named path Symbol must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::DanglingIndex { kind: "Symbol", .. }
    ));
}

#[test]
fn unresolved_authored_size_index_fails_closed() {
    let mut artifact = authored_type_artifact();
    let radix_hir::HirTypeFormArg::Size(index) =
        &mut artifact.presentation.authored_types[1].form.args[1]
    else {
        panic!("fixture must have a size argument");
    };
    *index = radix_types::IndexId(99_999);
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("unresolved authored IndexId must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::DanglingIndex {
            kind: "IndexId",
            ..
        }
    ));
}

#[test]
fn authored_root_witness_mismatch_fails_closed() {
    let mut artifact = authored_type_artifact();
    artifact.presentation.authored_types[3].form.ty = TypeId(0);
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("root witness mismatch must fail closed");
    assert!(matches!(
        err,
        crate::error::HirArtifactError::InvalidPresentation {
            issue: "root_witness_mismatch",
            ..
        }
    ));
}

/// Shape obligations are validated through one shared table helper, so the
/// unit's own table gets the same boundary/kind checks as nested provider
/// tables: a call row carrying a function-boundary relation fails closed
/// before lowering consumes it.
#[test]
fn call_shape_relation_marked_function_boundary_fails_closed() {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let dim = types.intern_index(IndexExpr::Literal(2));
    artifact.types = types.snapshot();
    artifact.shape_obligations.calls = vec![crate::artifact::ShapeCallObligationsWire {
        call: HirId(0),
        relations: vec![crate::artifact::ShapeRelationWire {
            left: dim,
            right: dim,
            span: Span::default(),
            boundary: crate::artifact::ShapeRelationBoundaryWire::FunctionEntry,
        }],
    }];
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("a call row carrying a non-call relation must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::MalformedSnapshot {
                kind: "shape obligations",
                ..
            }
        ),
        "call row with a function-boundary relation must fail closed: {err:?}"
    );
}

/// Type-param validation is shared by every HIR owner that declares them; a
/// generic constraint naming an out-of-arena `TypeId` fails closed through
/// the shared DanglingIndex path.
#[test]
fn dangling_type_param_constraint_fails_closed() {
    let mut artifact = stub_artifact();
    let HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.type_params.push(radix_hir::HirTypeParam {
        def_id: DefId(USER_DEF_ID_BASE + 1),
        kind: radix_syntax::GenericParamKind::Typus,
        name: Symbol(0),
        span: Span::default(),
        constraint: radix_hir::HirTypeParamConstraint::OneOf(vec![TypeId(99_999)]),
        default: None,
    });
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("a dangling type-param constraint TypeId must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "TypeId", .. }
        ),
        "dangling constraint type must fail closed: {err:?}"
    );
}

/// A-59: the export-snapshot `Function` arm validated each generic
/// parameter's constraint and default but never its name, so a snapshot
/// function carrying a dangling generic-param `Symbol` loaded while the
/// same symbol on a struct method was rejected. The fix routes the name
/// through `check_symbol` like every neighbouring arm.
#[test]
fn function_export_dangling_generic_param_name_fails_closed() {
    use crate::artifact::{FileExportSnapshot, FileInterfaceSnapshot, ResolverSnapshot};
    use radix_types::{FuncSig, GenericParamDescriptor, GenericParamSort, TypeParamConstraint};

    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let ret = types.primitive(Primitive::Numerus);
    artifact.types = types.snapshot();
    artifact.resolver = ResolverSnapshot {
        namespace_exports: Vec::new(),
        file_interfaces: vec![(
            Symbol(0),
            FileInterfaceSnapshot {
                exports: vec![(
                    Symbol(1),
                    FileExportSnapshot::Function(FuncSig {
                        type_params: vec![GenericParamDescriptor {
                            name: Symbol(99_999),
                            sort: GenericParamSort::Typus,
                            constraint: TypeParamConstraint::Any,
                            default: None,
                        }],
                        params: Vec::new(),
                        ret,
                        err: None,
                        is_async: false,
                        is_generator: false,
                    }),
                )],
                unavailable: Vec::new(),
                const_values: Vec::new(),
            },
        )],
        used_namespaces: Vec::new(),
        imported_nominal_types: Vec::new(),
        ambiguous_imported_nominal_types: Vec::new(),
        conversion_rows: Vec::new(),
        module_bindings: Vec::new(),
    };

    let bytes = encode(&artifact).expect("encode artifact");
    let err = decode(&bytes)
        .expect_err("a dangling generic-param name on a function export must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "Symbol", .. }
        ),
        "dangling generic-param name must fail closed: {err:?}"
    );
}

/// The companion positive case: a function export whose generic parameter
/// names are interned symbols must keep loading.
#[test]
fn function_export_valid_generic_param_name_loads() {
    use crate::artifact::{FileExportSnapshot, FileInterfaceSnapshot, ResolverSnapshot};
    use radix_types::{FuncSig, GenericParamDescriptor, GenericParamSort, TypeParamConstraint};

    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let ret = types.primitive(Primitive::Numerus);
    artifact.types = types.snapshot();
    artifact.resolver = ResolverSnapshot {
        namespace_exports: Vec::new(),
        file_interfaces: vec![(
            Symbol(0),
            FileInterfaceSnapshot {
                exports: vec![(
                    Symbol(1),
                    FileExportSnapshot::Function(FuncSig {
                        type_params: vec![GenericParamDescriptor {
                            name: Symbol(1),
                            sort: GenericParamSort::Typus,
                            constraint: TypeParamConstraint::Any,
                            default: None,
                        }],
                        params: Vec::new(),
                        ret,
                        err: None,
                        is_async: false,
                        is_generator: false,
                    }),
                )],
                unavailable: Vec::new(),
                const_values: Vec::new(),
            },
        )],
        used_namespaces: Vec::new(),
        imported_nominal_types: Vec::new(),
        ambiguous_imported_nominal_types: Vec::new(),
        conversion_rows: Vec::new(),
        module_bindings: Vec::new(),
    };

    let bytes = encode(&artifact).expect("encode artifact");
    decode(&bytes).expect("a valid generic-param name must keep loading");
}

/// An inferred set-cell literal (`_ [ … ]`) is rewritten to a `Conversio`
/// whose node id stays in `presentation.inferred_constructor_ids`. Lowering
/// keeps the authored `_` record at that node's `AscriptionTarget` slot; the
/// validator's site inventory must recognize the site (the serialize-side
/// registry already keeps it). Without the id in the set the slot stays
/// unknown, so the artifact still fails closed.
#[test]
fn inferred_constructor_ascription_site_is_registered() {
    fn conversio_artifact() -> crate::artifact::HirArtifact {
        let mut artifact = stub_artifact();
        let mut types = TypeTable::new();
        let target = types.intern(Type::Primitive(Primitive::Numerus));
        artifact.types = types.snapshot();
        let conversio = HirExpression {
            id: HirId(1),
            kind: HirExpressionKind::Conversio {
                source: Box::new(lit(2, 0)),
                target: radix_hir::HirConversioTarget::Type(target),
                params: Vec::new(),
                recovery: None,
                route: None,
                implicit: None,
            },
            ty: None,
            span: Span::default(),
        };
        stub_function(&mut artifact).body = Some(HirBlock::simple(
            vec![HirStatement {
                id: HirId(3),
                kind: HirStatementKind::Expr(conversio),
                span: Span::default(),
            }],
            None,
            Span::default(),
        ));
        artifact.presentation.authored_types = vec![radix_hir::HirAuthoredTypeUse {
            site: radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Hir(HirId(1)),
                slot: radix_hir::HirTypeSlot::AscriptionTarget,
            },
            form: radix_hir::HirTypeForm {
                ty: target,
                head: radix_hir::HirTypeFormHead::Infer,
                args: Vec::new(),
                label: None,
            },
        }];
        artifact
    }

    // Id in the set: the site resolves. The record is an `Infer` head at a
    // non-hole slot, so it may still fail the separate hole-slot legality
    // check; it must not fail as an unknown site.
    let mut artifact = conversio_artifact();
    artifact.presentation.inferred_constructor_ids = vec![HirId(1)];
    match crate::decode(&crate::encode(&artifact).expect("encode")) {
        Ok(_) => {}
        Err(crate::error::HirArtifactError::InvalidPresentation { issue, .. }) => {
            assert_ne!(
                issue, "unknown_type_slot",
                "an inferred-constructor ascription site must be registered"
            );
        }
        Err(other) => panic!("registered site must not fail structurally: {other:?}"),
    }

    // Id absent from the set: the same record keeps the fail-closed unknown
    // slot report.
    let artifact = conversio_artifact();
    let err = crate::decode(&crate::encode(&artifact).expect("encode"))
        .expect_err("an unregistered AscriptionTarget site must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "unknown_type_slot",
                ..
            }
        ),
        "unregistered ascription site must fail as unknown_type_slot: {err:?}"
    );
}

#[test]
fn generic_alias_body_matches_binds_params_by_name() {
    let mut types = TypeTable::new();
    let param_k = types.intern(Type::Param(Symbol(1)));
    let param_v = types.intern(Type::Param(Symbol(2)));
    let int_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let string_ty = types.primitive(Primitive::Textus);
    let swapped_body = types.intern(Type::Map(param_v, param_k));
    let swapped_witness = types.intern(Type::Map(string_ty, int_ty));
    let misordered_witness = types.intern(Type::Map(int_ty, string_ty));
    let list_body = types.array(param_k);
    let list_int = types.array(int_ty);
    let list_string = types.array(string_ty);
    let snapshot = types.snapshot();

    let bindings = [(Symbol(1), int_ty), (Symbol(2), string_ty)];
    assert!(
        super::generic_alias_body_matches(&snapshot, &bindings, swapped_body, swapped_witness),
        "map<V, K> must bind K=int and V=string into map<string, int>"
    );
    assert!(
        !super::generic_alias_body_matches(&snapshot, &bindings, swapped_body, misordered_witness),
        "map<V, K> must not match map<int, string> for the same bindings"
    );

    let one_binding = [(Symbol(1), int_ty)];
    assert!(
        super::generic_alias_body_matches(&snapshot, &one_binding, list_body, list_int),
        "list<T> must match list<int> when T is bound to int"
    );
    assert!(
        !super::generic_alias_body_matches(&snapshot, &one_binding, list_body, list_string),
        "list<T> must not match list<string> when T is bound to int"
    );
}

#[test]
fn generic_alias_body_matches_fails_closed() {
    let mut types = TypeTable::new();
    let param_t = types.intern(Type::Param(Symbol(1)));
    let int_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let list_body = types.array(param_t);
    let list_int = types.array(int_ty);
    let body_alias = types.intern(Type::Alias(DefId(USER_DEF_ID_BASE + 1), list_body));
    let witness_alias = types.intern(Type::Alias(DefId(USER_DEF_ID_BASE + 2), list_int));
    let snapshot = types.snapshot();
    let bindings = [(Symbol(1), int_ty)];

    assert!(
        !super::generic_alias_body_matches(&snapshot, &bindings, list_body, int_ty),
        "list<T> must not match an unrelated primitive witness"
    );
    assert!(
        !super::generic_alias_body_matches(&snapshot, &bindings, body_alias, witness_alias),
        "aliases with different DefIds must not match"
    );
    assert!(
        !super::generic_alias_body_matches(&snapshot, &bindings, int_ty, param_t),
        "a Param on the witness side never binds"
    );
}

/// The collector keeps every declared typus parameter of an alias and drops
/// `magnitudo` size parameters, and non-alias items contribute nothing: a
/// generic alias body only ever binds `Type::Param` names.
#[test]
fn collect_generic_alias_params_maps_typus_only() {
    let mut artifact = stub_artifact();
    let alias_def = DefId(USER_DEF_ID_BASE + 1);
    artifact.hir.items.push(radix_hir::HirItem {
        annotations: Vec::new(),
        id: HirId(9),
        def_id: alias_def,
        kind: radix_hir::HirItemKind::TypeAlias(radix_hir::HirTypeAlias {
            name: Symbol(2),
            type_params: vec![
                radix_hir::HirTypeParam {
                    def_id: DefId(USER_DEF_ID_BASE + 2),
                    kind: radix_syntax::GenericParamKind::Typus,
                    name: Symbol(3),
                    span: Span::default(),
                    constraint: radix_hir::HirTypeParamConstraint::Any,
                    default: None,
                },
                radix_hir::HirTypeParam {
                    def_id: DefId(USER_DEF_ID_BASE + 3),
                    kind: radix_syntax::GenericParamKind::Magnitudo,
                    name: Symbol(4),
                    span: Span::default(),
                    constraint: radix_hir::HirTypeParamConstraint::Any,
                    default: None,
                },
            ],
            ty: TypeId(0),
        }),
        span: Span::default(),
    });

    let params = super::collect_generic_alias_params(&artifact);
    assert_eq!(
        params.get(&alias_def),
        Some(&vec![Symbol(3)]),
        "only the typus parameter must be collected"
    );
    assert_eq!(params.len(), 1, "only the alias item must contribute");
    assert!(
        !params.contains_key(&DefId(0)),
        "the function item contributes nothing"
    );
}

/// The witness helper binds each authored argument to the alias parameter it
/// names and only accepts when the body matches the site witness under those
/// bindings; a disagreeing binding or an undeclared alias fails closed.
#[test]
fn transparent_generic_alias_witness_binds_authored_args() {
    let mut types = TypeTable::new();
    let sym_t = Symbol(1);
    let param_t = types.intern(Type::Param(sym_t));
    let list_t = types.array(param_t);
    let pairs_def = DefId(USER_DEF_ID_BASE + 7);
    let _alias_body = types.intern(Type::Alias(pairs_def, list_t));
    let int_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let list_int = types.array(int_ty);
    let string_ty = types.primitive(Primitive::Textus);
    let _list_string = types.array(string_ty);
    let snapshot = types.snapshot();
    let sym_pairs = Symbol(2);
    let alias_params = handroll::FxHashMap::from_iter([(pairs_def, vec![sym_t])]);
    let named_head = radix_hir::HirTypeFormHead::Named {
        def_id: pairs_def,
        path: vec![sym_pairs],
    };
    let arg_form = |element: radix_types::TypeId| radix_hir::HirTypeForm {
        ty: element,
        head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::LONG),
        args: Vec::new(),
        label: None,
    };

    let int_form = radix_hir::HirTypeForm {
        ty: list_int,
        head: named_head.clone(),
        args: vec![radix_hir::HirTypeFormArg::Type(Box::new(arg_form(int_ty)))],
        label: None,
    };
    assert_eq!(
        super::transparent_generic_alias_witness(&snapshot, &alias_params, pairs_def, &int_form),
        Some(vec![radix_types::AppliedArg::Type(int_ty)]),
        "list<T> with T=int must witness its applied argument"
    );

    let string_form = radix_hir::HirTypeForm {
        ty: list_int,
        head: named_head.clone(),
        args: vec![radix_hir::HirTypeFormArg::Type(Box::new(arg_form(
            string_ty,
        )))],
        label: None,
    };
    assert_eq!(
        super::transparent_generic_alias_witness(&snapshot, &alias_params, pairs_def, &string_form),
        None,
        "a T=string binding must not match a list<int> site witness"
    );

    let undeclared_params = handroll::FxHashMap::default();
    assert_eq!(
        super::transparent_generic_alias_witness(
            &snapshot,
            &undeclared_params,
            pairs_def,
            &int_form
        ),
        None,
        "an alias the artifact never declared must fail closed"
    );
}

// ―─ Conversation constructor (`sermo`) authored-form pairing ─────────────

/// The compiler-owned conversation constructor these artifacts record as their
/// metadata. Analysis finalizes `sermo<O, R>` to this definition's bare
/// `Type::Struct` carrier, so the authored `sermo<O, R>` children have no
/// `Applied` witness argument list.
const CONVERSATION: DefId = DefId(USER_DEF_ID_BASE + 40);

/// A resolvable definition the artifact never records as a carrier type, used
/// to prove the metadata's carrier identity is checked.
const CARRIERLESS: DefId = DefId(USER_DEF_ID_BASE + 41);

/// Build an artifact whose single declared local carries one authored named
/// form headed by [`CONVERSATION`], with `metadata` as the artifact's recorded
/// conversation constructor. `build` supplies the site witness and the authored
/// children against the artifact's own type table.
fn conversation_artifact(
    metadata: Option<DefId>,
    build: impl FnOnce(&mut TypeTable) -> (Type, Vec<radix_hir::HirTypeFormArg>),
) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let (witness, args) = build(&mut types);
    let witness_ty = types.intern(witness);
    let local_def = DefId(USER_DEF_ID_BASE + 42);
    artifact.interner = vec!["main".to_owned(), "test".to_owned(), "sermo".to_owned()];
    stub_function(&mut artifact).body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Local(radix_hir::HirLocal {
                def_id: local_def,
                name: Symbol(0),
                ty: Some(witness_ty),
                init: None,
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    for (id, def_id) in [(2, CONVERSATION), (3, CARRIERLESS)] {
        artifact.hir.items.push(radix_hir::HirItem {
            annotations: Vec::new(),
            id: HirId(id),
            def_id,
            kind: HirItemKind::Struct(radix_hir::HirStruct {
                name: Symbol(2),
                type_params: Vec::new(),
                fields: Vec::new(),
                methods: Vec::new(),
                implements: Vec::new(),
            }),
            span: Span::default(),
        });
    }
    artifact.types = types.snapshot();
    artifact.conversation_constructor = metadata;
    artifact.presentation.authored_types = vec![radix_hir::HirAuthoredTypeUse {
        site: radix_hir::HirTypeSite {
            owner: radix_hir::HirOwnerRef::Def(local_def),
            slot: radix_hir::HirTypeSlot::Declared,
        },
        form: radix_hir::HirTypeForm {
            ty: witness_ty,
            head: radix_hir::HirTypeFormHead::Named {
                def_id: CONVERSATION,
                path: vec![Symbol(2)],
            },
            args,
            label: None,
        },
    }];
    artifact
}

/// One authored `Type` child witnessing `ty`, spelled as a semantic head.
fn typed_child(ty: TypeId) -> radix_hir::HirTypeFormArg {
    radix_hir::HirTypeFormArg::Type(Box::new(radix_hir::HirTypeForm {
        ty,
        head: radix_hir::HirTypeFormHead::Semantic(TypeFormLevel::LONG),
        args: Vec::new(),
        label: None,
    }))
}

/// Load one conversation artifact, surfacing the loader error.
fn load_conversation(
    artifact: crate::artifact::HirArtifact,
) -> Result<(), crate::error::HirArtifactError> {
    decode(&encode(&artifact).expect("encode conversation artifact")).map(|_| ())
}

#[test]
fn conversation_typed_form_pairs_the_bare_carrier() {
    // `sermo<none, string>`: the witness is the bare carrier struct while the
    // author spelled two type arguments. Only the recorded conversation
    // constructor may pair that way.
    let artifact = conversation_artifact(Some(CONVERSATION), |types| {
        let opener = types.primitive(Primitive::Nihil);
        let reply = types.primitive(Primitive::Textus);
        (
            Type::Struct(CONVERSATION),
            vec![typed_child(opener), typed_child(reply)],
        )
    });
    load_conversation(artifact).expect("a typed conversation form must validate");
}

#[test]
fn conversation_bare_form_pairs_the_bare_carrier() {
    // A loose `sermo` binding authors no argument at all; the bare carrier
    // witness is its exact pair.
    let artifact = conversation_artifact(Some(CONVERSATION), |_| {
        (Type::Struct(CONVERSATION), Vec::new())
    });
    load_conversation(artifact).expect("a bare conversation form must validate");
}

#[test]
fn conversation_form_with_wrong_arity_fails_closed() {
    for arity in [1usize, 3] {
        let artifact = conversation_artifact(Some(CONVERSATION), move |types| {
            let int = types.primitive(Primitive::Numerus);
            (Type::Struct(CONVERSATION), vec![typed_child(int); arity])
        });
        let err =
            load_conversation(artifact).expect_err("a wrong conversation arity must fail closed");
        assert!(
            matches!(
                err,
                crate::error::HirArtifactError::InvalidPresentation {
                    issue: "named_argument_arity",
                    ..
                }
            ),
            "a {arity}-argument conversation form must fail with named_argument_arity: {err:?}"
        );
    }
}

#[test]
fn conversation_form_with_non_type_child_fails_closed() {
    let artifact = conversation_artifact(Some(CONVERSATION), |types| {
        let int = types.primitive(Primitive::Numerus);
        let size = types.intern_index(IndexExpr::Literal(2));
        (
            Type::Struct(CONVERSATION),
            vec![typed_child(int), radix_hir::HirTypeFormArg::Size(size)],
        )
    });
    let err =
        load_conversation(artifact).expect_err("a non-type conversation argument must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "conversation_argument_kind",
                ..
            }
        ),
        "a size argument on a conversation form must fail with conversation_argument_kind: {err:?}"
    );
}

#[test]
fn conversation_form_with_dangling_child_fails_closed() {
    let artifact = conversation_artifact(Some(CONVERSATION), |types| {
        let int = types.primitive(Primitive::Numerus);
        (
            Type::Struct(CONVERSATION),
            vec![typed_child(int), typed_child(TypeId(u32::MAX))],
        )
    });
    let err =
        load_conversation(artifact).expect_err("a dangling conversation child must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "TypeId", .. }
        ),
        "a dangling conversation child type must fail with a TypeId DanglingIndex: {err:?}"
    );
}

#[test]
fn conversation_form_under_another_constructor_fails_closed() {
    // The bare carrier pairing is reserved for the recorded constructor: an
    // artifact that records a different (or no) constructor keeps the strict
    // argument-arity rule even though the witness is a bare struct.
    for metadata in [None, Some(CARRIERLESS)] {
        let artifact = conversation_artifact(metadata, |types| {
            let int = types.primitive(Primitive::Numerus);
            (
                Type::Struct(CONVERSATION),
                vec![typed_child(int), typed_child(int)],
            )
        });
        let err = load_conversation(artifact)
            .expect_err("an unrecorded conversation constructor must fail closed");
        assert!(
            matches!(
                err,
                crate::error::HirArtifactError::InvalidPresentation {
                    issue: "named_argument_arity",
                    ..
                }
            ),
            "a bare carrier under {metadata:?} must fail with named_argument_arity: {err:?}"
        );
    }
}

#[test]
fn conversation_form_with_a_non_carrier_witness_fails_closed() {
    // An alias witness names the recorded constructor but is not its bare
    // carrier struct, so the pairing must not admit it.
    let artifact = conversation_artifact(Some(CONVERSATION), |types| {
        let int = types.primitive(Primitive::Numerus);
        (
            Type::Alias(CONVERSATION, int),
            vec![typed_child(int), typed_child(int)],
        )
    });
    let err = load_conversation(artifact)
        .expect_err("a non-carrier conversation witness must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "named_argument_arity",
                ..
            }
        ),
        "an alias conversation witness must fail with named_argument_arity: {err:?}"
    );
}

#[test]
fn conversation_metadata_without_a_carrier_fails_closed() {
    // The recorded constructor must be the definition whose bare struct the
    // artifact's type table carries.
    let artifact = conversation_artifact(Some(CARRIERLESS), |_| {
        (Type::Struct(CONVERSATION), Vec::new())
    });
    let err = load_conversation(artifact)
        .expect_err("carrierless conversation metadata must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "conversation_constructor_not_carrier",
                ..
            }
        ),
        "recorded metadata without a carrier struct must fail with \
         conversation_constructor_not_carrier: {err:?}"
    );
}

#[test]
fn conversation_metadata_with_a_dangling_definition_fails_closed() {
    let unregistered = DefId(USER_DEF_ID_BASE + 43);
    let artifact = conversation_artifact(Some(unregistered), |_| {
        (Type::Struct(CONVERSATION), Vec::new())
    });
    let err =
        load_conversation(artifact).expect_err("dangling conversation metadata must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex { kind: "DefId", .. }
        ),
        "an unregistered conversation constructor must fail with a DefId DanglingIndex: {err:?}"
    );
}

#[test]
fn unrelated_named_type_with_extra_arguments_fails_closed() {
    // Ordinary named types keep the strict arity rule: a user struct's bare
    // witness never pairs with authored arguments, even when the artifact
    // records a conversation constructor.
    let mut artifact = conversation_artifact(Some(CONVERSATION), |types| {
        let int = types.primitive(Primitive::Numerus);
        (
            Type::Struct(CARRIERLESS),
            vec![typed_child(int), typed_child(int)],
        )
    });
    artifact.presentation.authored_types[0].form.head = radix_hir::HirTypeFormHead::Named {
        def_id: CARRIERLESS,
        path: vec![Symbol(2)],
    };
    let err =
        load_conversation(artifact).expect_err("extra arguments on a named type must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::InvalidPresentation {
                issue: "named_argument_arity",
                ..
            }
        ),
        "extra arguments on an unrelated named type must fail with named_argument_arity: {err:?}"
    );
}
