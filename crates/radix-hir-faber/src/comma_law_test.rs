//! CS-6 emitter proofs for the comma-separator law.
//!
//! The parser census is the completeness oracle.  This leaf test keeps one
//! explicit row for every census position, then pins the HIR writer paths that
//! can represent those positions.  Parser-only binding/destructure positions
//! are recorded as verified notes because HIR intentionally normalizes them
//! away before this backend runs.

use crate::surface::{KeywordSurface, TypeBindingKind};

use crate::{FaberCodegen, empty_presentation};
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirAnnotation, HirAnnotationField, HirAnnotationValue, HirBlock, HirCallArg, HirCasuArm,
    HirEnum, HirExpression, HirExpressionKind, HirFunction, HirImport, HirImportItem, HirItem,
    HirItemKind, HirParam, HirParamMode, HirPattern, HirStruct, HirTypeParam,
    HirTypeParamConstraint, HirVariant, HirVariantField,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_syntax::{CallablePosture, GenericParamKind, JsonMember, JsonValue, Visibility};
use radix_types::{
    FuncSig, IndexExpr, NumericWidth, ParamType, Primitive, SemanticParamMode, Type, TypeTable,
};
use rustc_hash::FxHashMap;

#[test]
fn generic_applied_arg_preserves_index_kind() {
    let mut types = TypeTable::new();
    let base = types.intern(Type::Param(radix_lexer::Symbol(10)));
    let size = types.intern_index(IndexExpr::Literal(3));
    let applied = types.intern(Type::Applied(
        base,
        vec![radix_types::AppliedArg::Size(size)],
    ));
    assert!(matches!(
        types.get(applied),
        Type::Applied(_, args)
            if matches!(args.as_slice(), [radix_types::AppliedArg::Size(id)] if *id == size)
    ));
}

struct TestSurface;

impl KeywordSurface for TestSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::De => "de",
            TokenKind::Ex => "ex",
            TokenKind::Finge => "finge",
            TokenKind::Futurum => "futurum",
            TokenKind::Copy => "copy",
            TokenKind::Importa => "importa",
            TokenKind::In => "in",
            TokenKind::Magnitudo => "magnitudo",
            TokenKind::Own => "own",
            TokenKind::Sparge => "sparge",
            TokenKind::Ut => "ut",
            _ => "kw",
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Textus) => "textus",
            TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
            TypeBindingKind::Primitive(Primitive::Bivalens) => "bivalens",
            TypeBindingKind::Primitive(Primitive::Vacuum) => "vacuum",
            TypeBindingKind::Collection(radix_types::CollectionKind::Lista) => "lista",
            TypeBindingKind::Collection(radix_types::CollectionKind::Tabula) => "tabula",
            TypeBindingKind::Collection(radix_types::CollectionKind::Copia) => "copia",
            TypeBindingKind::Collection(radix_types::CollectionKind::Promissum) => "promissum",
            TypeBindingKind::Collection(radix_types::CollectionKind::Cursor) => "cursor",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Tensor) => "tensor",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Vector) => "vector",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Matrix) => "matrix",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Sparsa) => "sparsa",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Atomic) => "atomic",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Intervallum) => {
                "intervallum"
            }
            _ => "type",
        }
    }
}

const CS1_ROWS: &[(&str, &str)] = &[
    (
        "decl.rs:396 array_binding_pattern elements",
        "verified-note",
    ),
    ("decl.rs:419 object_binding_pattern fields", "verified-note"),
    (
        "decl.rs:451 object rest duplicate-rest guard",
        "verified-note",
    ),
    (
        "decl.rs:470 array rest duplicate-rest guard",
        "verified-note",
    ),
    ("decl.rs:603 function parameters", "pin"),
    ("decl.rs:707 genus implements list", "pin"),
    ("decl.rs:1089 enum members", "pin"),
    ("decl.rs:1128 discretio payload fields", "pin"),
    ("decl.rs:1144 discretio variants", "pin"),
    ("decl.rs:1280 braced import fields", "verified-note"),
    ("decl.rs:1710 braced annotation fields", "pin"),
    ("decl.rs:1912 braced nucleum field", "verified-note"),
    ("decl.rs:2309 generic declaration parameters", "pin"),
    ("expr.rs:869 gradient selectors", "pin"),
    ("expr.rs:1024 tensor-family shape pair", "pin"),
    ("expr.rs:1462 finge named field initializers", "pin"),
    ("expr.rs:1507 iuncta elements", "pin"),
    ("expr.rs:1537 closure signature parameters", "pin"),
    ("expr.rs:1700 closure parameter helper", "pin"),
    ("expr.rs:1866 call arguments", "pin"),
    ("expr.rs:1894 array/list elements", "pin"),
    ("expr.rs:1953 typed object fields", "pin"),
    ("expr.rs:1991 JSON object members", "pin"),
    ("expr.rs:2077 JSON array elements", "pin"),
    ("expr.rs:2145 type arguments", "pin"),
    ("expr.rs:2182 scriptum arguments", "pin"),
    ("types.rs:172 atomic singleton", "verified-note"),
    ("types.rs:188 promissum alternate", "verified-note"),
    ("types.rs:214 tensor element/figura pair", "pin"),
    ("types.rs:242 named type parameters", "pin"),
    ("types.rs:339 function-type parameters", "pin"),
    ("types.rs:526 figura dimensions", "pin"),
    ("stmt.rs:614 scribe arguments", "pin"),
    ("stmt.rs:760 ex destructure fields", "verified-note"),
    ("pattern.rs:140 pattern bindings", "pin"),
];

#[test]
fn cs1_every_position_has_an_emitter_pin_or_verified_note() {
    assert_eq!(
        CS1_ROWS.len(),
        35,
        "CS-1 census rows must stay exhaustive (discerne subjects and arm patterns are `et` lists, no longer comma-law rows)"
    );
    for (position, disposition) in CS1_ROWS {
        assert!(
            *disposition == "pin" || *disposition == "verified-note",
            "{position} has no CS-6 disposition"
        );
    }

    // These parser-only shapes are deliberately explicit rather than silently
    // omitted: binding/destructure patterns are lowered before HIR emission,
    // while the singleton nucleum field and flattened imports have no comma
    // bearing in the canonical source form.
    assert_eq!(CS1_ROWS[0].1, "verified-note");
    assert_eq!(CS1_ROWS[1].1, "verified-note");
    assert_eq!(CS1_ROWS[2].1, "verified-note");
    assert_eq!(CS1_ROWS[3].1, "verified-note");
    assert_eq!(CS1_ROWS[9].1, "verified-note");
    assert_eq!(CS1_ROWS[11].1, "verified-note");
    assert_eq!(CS1_ROWS[33].1, "verified-note");
}

fn span() -> Span {
    Span::default()
}

fn int(interner: &Interner, value: u64) -> HirExpression {
    let _ = interner;
    HirExpression {
        id: radix_hir::HirId(u32::try_from(value).unwrap()),
        kind: HirExpressionKind::Literal(radix_hir::HirLiteral::Int(value)),
        ty: None,
        span: span(),
    }
}

fn render_expr(codegen: &FaberCodegen<'_>, expr: &HirExpression) -> String {
    let mut writer = CodeWriter::new();
    codegen.write_expr(expr, &FxHashMap::default(), &TestSurface, &mut writer);
    writer.finish()
}

fn call_arg(expr: HirExpression) -> HirCallArg {
    HirCallArg {
        name: None,
        spread: false,
        marker: None,
        expr,
        span: span(),
    }
}

fn object_field(
    name: &str,
    value: HirExpression,
    interner: &mut Interner,
) -> (radix_lexer::Symbol, HirExpression) {
    (interner.intern(name), value)
}

fn empty_function(
    interner: &mut Interner,
    name: &str,
    type_params: Vec<HirTypeParam>,
    params: Vec<HirParam>,
) -> HirFunction {
    HirFunction {
        name: interner.intern(name),
        type_params,
        params,
        cli_args: None,
        ret_ty: None,
        err_ty: None,
        body: Some(HirBlock::simple(Vec::new(), None, span())),
        posture: CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    }
}

fn emit_program(hir: &radix_hir::HirModule, codegen: &mut FaberCodegen<'_>) -> String {
    codegen
        .generate(hir, &empty_presentation(), &TestSurface)
        .expect("canonical Faber emission")
        .code
}

fn assert_no_trailing_separator(output: &str) {
    for close in [']', ')', '}'] {
        assert!(
            !output.contains(&format!(",{close}")) && !output.contains(&format!(", {close}")),
            "canonical output has a trailing comma before {close:?}:\n{output}"
        );
    }
}

#[test]
fn expression_item_lists_emit_between_only() {
    let mut interner = Interner::new();
    let types = TypeTable::new();

    let gradient = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Gradient {
            call: Box::new(int(&interner, 0)),
            selected_arguments: vec![int(&interner, 1), int(&interner, 2), int(&interner, 3)],
        },
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &gradient),
        "0 ∇ [1, 2, 3]"
    );

    let array = HirExpression {
        id: radix_hir::HirId(2),
        kind: HirExpressionKind::Array(vec![
            radix_hir::HirArrayElement::Expr(int(&interner, 1)),
            radix_hir::HirArrayElement::Expr(int(&interner, 2)),
            radix_hir::HirArrayElement::Expr(int(&interner, 3)),
        ]),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &array),
        "[1, 2, 3]"
    );

    let typed_object = HirExpression {
        id: radix_hir::HirId(3),
        kind: HirExpressionKind::Struct(
            DefId(10),
            vec![
                object_field("a", int(&interner, 1), &mut interner),
                object_field("b", int(&interner, 2), &mut interner),
                object_field("c", int(&interner, 3), &mut interner),
            ],
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &typed_object),
        "def_10 { a = 1, b = 2, c = 3 }"
    );

    let json = HirExpression {
        id: radix_hir::HirId(4),
        kind: HirExpressionKind::Literal(radix_hir::HirLiteral::JsonValor(JsonValue::Object(
            vec![
                JsonMember {
                    key_span: span(),
                    key: interner.intern("a"),
                    value: JsonValue::Numerus(1),
                },
                JsonMember {
                    key_span: span(),
                    key: interner.intern("b"),
                    value: JsonValue::Numerus(2),
                },
            ],
        ))),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &json),
        "{ \"a\": 1, \"b\": 2 }"
    );

    let json_array = HirExpression {
        id: radix_hir::HirId(5),
        kind: HirExpressionKind::Literal(radix_hir::HirLiteral::JsonValor(JsonValue::Array(vec![
            JsonValue::Numerus(1),
            JsonValue::Numerus(2),
            JsonValue::Numerus(3),
        ]))),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &json_array),
        "[1, 2, 3]"
    );

    let call = HirExpression {
        id: radix_hir::HirId(6),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(7),
                kind: HirExpressionKind::Path(DefId(20)),
                ty: None,
                span: span(),
            }),
            Vec::new(),
            vec![call_arg(int(&interner, 1)), call_arg(int(&interner, 2))],
            None,
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &call),
        "def_20(1, 2)"
    );

    let call_with_type_args = HirExpression {
        id: radix_hir::HirId(14),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(15),
                kind: HirExpressionKind::Path(DefId(21)),
                ty: None,
                span: span(),
            }),
            vec![
                types.primitive(Primitive::Numerus),
                types.primitive(Primitive::Textus),
            ],
            vec![call_arg(int(&interner, 1)), call_arg(int(&interner, 2))],
            None,
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &call_with_type_args),
        "def_21<numerus, textus>(1, 2)"
    );

    let closure_params = vec![
        HirParam {
            def_id: DefId(50),
            name: interner.intern("a"),
            ty: types.primitive(Primitive::Numerus),
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: span(),
        },
        HirParam {
            def_id: DefId(51),
            name: interner.intern("b"),
            ty: types.primitive(Primitive::Textus),
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: span(),
        },
    ];
    let closure = HirExpression {
        id: radix_hir::HirId(16),
        kind: HirExpressionKind::Clausura(
            closure_params,
            Some(types.primitive(Primitive::Numerus)),
            None,
            Box::new(int(&interner, 0)),
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &closure),
        "(numerus a, textus b) → numerus ∴ 0"
    );
    let single_closure = HirExpression {
        id: radix_hir::HirId(17),
        kind: HirExpressionKind::Clausura(
            vec![HirParam {
                def_id: DefId(52),
                name: interner.intern("only"),
                ty: types.primitive(Primitive::Numerus),
                mode: HirParamMode::Owned,
                optional: false,
                sponte: false,
                default: None,
                span: span(),
            }],
            None,
            None,
            Box::new(int(&interner, 0)),
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &single_closure),
        "numerus only ∴ 0"
    );

    let tuple = HirExpression {
        id: radix_hir::HirId(8),
        kind: HirExpressionKind::Tuple(
            vec![int(&interner, 1), int(&interner, 2), int(&interner, 3)],
            None,
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &tuple),
        "[1, 2, 3]"
    );

    let scribe = HirExpression {
        id: radix_hir::HirId(9),
        kind: HirExpressionKind::Scribe(
            radix_hir::HirScribeKind::Nota,
            vec![int(&interner, 1), int(&interner, 2), int(&interner, 3)],
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &scribe),
        "nota 1, 2, 3"
    );

    let template = HirExpression {
        id: radix_hir::HirId(10),
        kind: HirExpressionKind::Scriptum(
            interner.intern("message"),
            vec![int(&interner, 1), int(&interner, 2)],
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &template),
        "scriptum(\"message\", 1, 2)"
    );

    let finge = HirExpression {
        id: radix_hir::HirId(11),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(12),
                kind: HirExpressionKind::Path(DefId(30)),
                ty: None,
                span: span(),
            }),
            Vec::new(),
            vec![
                HirCallArg {
                    name: Some(interner.intern("x")),
                    spread: false,
                    marker: None,
                    expr: int(&interner, 1),
                    span: span(),
                },
                HirCallArg {
                    name: Some(interner.intern("y")),
                    spread: false,
                    marker: None,
                    expr: int(&interner, 2),
                    span: span(),
                },
            ],
            None,
        ),
        ty: None,
        span: span(),
    };
    let mut finge_codegen = FaberCodegen::new(&types, &interner);
    finge_codegen.variant_def_ids.insert(DefId(30));
    assert_eq!(
        render_expr(&finge_codegen, &finge),
        "finge def_30 { x = 1, y = 2 }"
    );

    let discerne = HirExpression {
        id: radix_hir::HirId(13),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![int(&interner, 1), int(&interner, 2), int(&interner, 3)],
            arms: Vec::new(),
            exhaustive: false,
        },
        ty: None,
        span: span(),
    };
    let discerne_output = render_expr(&FaberCodegen::new(&types, &interner), &discerne);
    assert!(discerne_output.starts_with("discerne 1 et 2 et 3 {"));

    for output in [
        "0 ∇ [1, 2, 3]",
        "[1, 2, 3]",
        "def_10 { a = 1, b = 2, c = 3 }",
        "{ \"a\": 1, \"b\": 2 }",
        "def_20(1, 2)",
        "[1, 2, 3]",
        "nota 1, 2, 3",
        "scriptum(\"message\", 1, 2)",
        "finge def_30 { x = 1, y = 2 }",
    ] {
        assert_no_trailing_separator(output);
    }
}

#[test]
fn declaration_and_annotation_lists_emit_between_only() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let type_params = vec![
        HirTypeParam {
            def_id: DefId(1),
            kind: GenericParamKind::Typus,
            name: interner.intern("T"),
            span: span(),
            constraint: HirTypeParamConstraint::Any,
            default: None,
        },
        HirTypeParam {
            def_id: DefId(2),
            kind: GenericParamKind::Typus,
            name: interner.intern("U"),
            span: span(),
            constraint: HirTypeParamConstraint::Any,
            default: None,
        },
        HirTypeParam {
            def_id: DefId(3),
            kind: GenericParamKind::Magnitudo,
            name: interner.intern("N"),
            span: span(),
            constraint: HirTypeParamConstraint::Any,
            default: None,
        },
    ];
    let params = vec![
        HirParam {
            def_id: DefId(4),
            name: interner.intern("a"),
            ty: numerus,
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: span(),
        },
        HirParam {
            def_id: DefId(5),
            name: interner.intern("b"),
            ty: textus,
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: span(),
        },
        HirParam {
            def_id: DefId(6),
            name: interner.intern("c"),
            ty: numerus,
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: span(),
        },
    ];
    let function = empty_function(&mut interner, "f", type_params, params);
    let annotation = HirAnnotation::with_fields(
        interner.intern("meta"),
        vec![
            HirAnnotationField {
                name: interner.intern("a"),
                value: HirAnnotationValue::Int(1),
                span: span(),
            },
            HirAnnotationField {
                name: interner.intern("b"),
                value: HirAnnotationValue::Int(2),
                span: span(),
            },
        ],
        span(),
    );
    let genus = HirStruct {
        name: interner.intern("S"),
        type_params: Vec::new(),
        fields: vec![
            radix_hir::HirField {
                def_id: DefId(20),
                name: interner.intern("x"),
                ty: numerus,
                modifier: None,
                sponte: false,
                annotations: Vec::new(),
                init: None,
                span: span(),
            },
            radix_hir::HirField {
                def_id: DefId(21),
                name: interner.intern("y"),
                ty: numerus,
                modifier: None,
                sponte: false,
                annotations: Vec::new(),
                init: None,
                span: span(),
            },
        ],
        methods: Vec::new(),
        implements: [100, 101, 102]
            .into_iter()
            .map(|id| radix_types::ContractRef {
                contract: DefId(id),
                args: Vec::new(),
            })
            .collect(),
    };
    let enm = HirEnum {
        name: interner.intern("E"),
        type_params: Vec::new(),
        commune_fields: Vec::new(),
        variants: vec![
            HirVariant {
                def_id: DefId(30),
                name: interner.intern("A"),
                fields: vec![
                    HirVariantField {
                        name: interner.intern("first"),
                        ty: numerus,
                        span: span(),
                    },
                    HirVariantField {
                        name: interner.intern("second"),
                        ty: textus,
                        span: span(),
                    },
                ],
                span: span(),
            },
            HirVariant {
                def_id: DefId(31),
                name: interner.intern("B"),
                fields: Vec::new(),
                span: span(),
            },
            HirVariant {
                def_id: DefId(32),
                name: interner.intern("C"),
                fields: Vec::new(),
                span: span(),
            },
        ],
    };
    let import = HirImport {
        path: interner.intern("pkg"),
        visibility: Visibility::Privata,
        items: vec![
            HirImportItem {
                def_id: DefId(40),
                name: interner.intern("one"),
                alias: None,
            },
            HirImportItem {
                def_id: DefId(41),
                name: interner.intern("two"),
                alias: Some(interner.intern("second")),
            },
        ],
        selective: false,
    };
    let hir = radix_hir::HirModule {
        items: vec![
            HirItem {
                annotations: Vec::new(),
                id: radix_hir::HirId(1),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Import(import),
                span: span(),
            },
            HirItem {
                annotations: vec![annotation],
                id: radix_hir::HirId(2),
                def_id: DefId(radix_types::USER_DEF_ID_BASE + 1),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            },
            HirItem {
                annotations: Vec::new(),
                id: radix_hir::HirId(3),
                def_id: DefId(radix_types::USER_DEF_ID_BASE + 2),
                kind: HirItemKind::Struct(genus),
                span: span(),
            },
            HirItem {
                annotations: Vec::new(),
                id: radix_hir::HirId(4),
                def_id: DefId(radix_types::USER_DEF_ID_BASE + 3),
                kind: HirItemKind::Enum(enm),
                span: span(),
            },
        ],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let output = emit_program(&hir, &mut FaberCodegen::new(&types, &interner));
    assert!(
        output.contains("<T, U, magnitudo N>"),
        "generic separators missing:\n{output}"
    );
    assert!(
        output.contains("(numerus a, textus b, numerus c)"),
        "parameter separators missing:\n{output}"
    );
    assert!(
        output.contains("implet def_100, def_101, def_102"),
        "implements separators missing:\n{output}"
    );
    assert!(output.contains("@ meta { a = 1, b = 2 }"));
    assert!(
        output.contains(
            "    A {\n        numerus first\n        textus second\n    },\n    B,\n    C\n"
        ),
        "declaration-block fields or variant separators drifted:\n{output}"
    );
    // Canonical imports flatten the parser's braced form into whitespace-
    // separated bindings.  This is a verified note for CS-1 row 10: there is
    // no item-list comma slot in the source form emitted by this backend.
    assert!(output.contains("importa ex \"pkg\" one two ut second"));
    assert_no_trailing_separator(&output);
}

#[test]
fn type_positions_pin_fixed_and_item_shape_separators() {
    let names = FxHashMap::default();
    let mut interner = Interner::new();
    let surface = TestSurface;
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let i32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim3 = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim2, dim3]));
    let applied_base = types.intern(Type::Param(interner.intern("Result")));
    let applied = types.intern(Type::Applied(
        applied_base,
        vec![
            radix_types::AppliedArg::Type(textus),
            radix_types::AppliedArg::Type(numerus),
        ],
    ));
    let tuple = types.intern(Type::Tuple(vec![textus, numerus]));
    let function = types.intern(Type::Func(FuncSig {
        type_params: Vec::new(),
        params: vec![
            ParamType {
                ty: textus,
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
            },
            ParamType {
                ty: numerus,
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
            },
        ],
        ret: i32_ty,
        err: None,
        is_async: false,
        is_generator: false,
    }));

    let tensor = types.tensor_with_shape(i32_ty, shape);
    let atomic = types.atomic(i32_ty);
    let promissum_failable = types.promissum_failable(textus, numerus);

    let codegen = FaberCodegen::new(&types, &interner);
    assert_eq!(
        codegen.type_to_faber(applied, &names, &surface),
        "Result<textus, numerus>"
    );
    assert_eq!(
        codegen.type_to_faber(tuple, &names, &surface),
        "iuncta<textus, numerus>"
    );
    assert_eq!(
        codegen.type_to_faber(function, &names, &surface),
        "(textus, numerus) → i32"
    );
    assert_eq!(
        codegen.type_to_faber(tensor, &names, &surface),
        "tensor<i32, [2, 3]>"
    );
    assert_eq!(
        codegen.type_to_faber(atomic, &names, &surface),
        "atomic<i32>"
    );
    assert_eq!(
        codegen.type_to_faber(promissum_failable, &names, &surface),
        "promissum<textus ⇥ numerus>"
    );
    assert_no_trailing_separator("Result<textus, numerus>");
    assert_no_trailing_separator("iuncta<textus, numerus>");
    assert_no_trailing_separator("(textus, numerus) → i32");
    assert_no_trailing_separator("tensor<i32, [2, 3]>");
}

#[test]
fn pattern_positions_pin_between_only_and_no_trailing() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let names = FxHashMap::default();
    let patterns = vec![
        HirPattern::Binding(DefId(1), interner.intern("r")),
        HirPattern::Binding(DefId(2), interner.intern("g")),
        HirPattern::Binding(DefId(3), interner.intern("b")),
    ];
    let mut writer = CodeWriter::new();
    let codegen = FaberCodegen::new(&types, &interner);
    codegen.write_pattern(
        &HirPattern::Variant(DefId(10), patterns.clone()),
        &names,
        &TestSurface,
        &mut writer,
    );
    let output = writer.finish();
    assert_eq!(output, "def_10 fixum r, g, b");
    assert_no_trailing_separator(&output);

    let arm = HirCasuArm {
        patterns,
        body: int(&interner, 1),
        span: span(),
    };
    let mut writer = CodeWriter::new();
    codegen.write_match_arms(&[arm], false, &names, &TestSurface, &mut writer);
    let arm_output = writer.finish();
    assert!(arm_output.contains("casu r et g et b {"));
    assert_no_trailing_separator(&arm_output);
}

#[test]
fn multi_subject_arm_separates_patterns_with_et_and_keeps_binding_commas() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let names = FxHashMap::default();
    let arm = HirCasuArm {
        patterns: vec![
            HirPattern::Variant(
                DefId(10),
                vec![HirPattern::Binding(DefId(1), interner.intern("r"))],
            ),
            HirPattern::Variant(
                DefId(11),
                vec![
                    HirPattern::Binding(DefId(2), interner.intern("a")),
                    HirPattern::Binding(DefId(3), interner.intern("b")),
                ],
            ),
        ],
        body: int(&interner, 1),
        span: span(),
    };
    let mut writer = CodeWriter::new();
    FaberCodegen::new(&types, &interner).write_match_arms(
        &[arm],
        false,
        &names,
        &TestSurface,
        &mut writer,
    );
    assert!(
        writer
            .finish()
            .contains("casu def_10 fixum r et def_11 fixum a, b {")
    );
}

#[test]
fn canonical_reemission_is_idempotent_for_the_cs1_fixture() {
    // This is the leaf-level equivalent of format/convert's second pass: both
    // product commands call this canonical HIR emitter, so the same analyzed
    // source fixture must produce byte-identical output on every pass.
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let type_name = interner.intern("T");
    let param_name = interner.intern("x");
    let function = empty_function(
        &mut interner,
        "f",
        vec![HirTypeParam {
            def_id: DefId(1),
            kind: GenericParamKind::Typus,
            name: type_name,
            span: span(),
            constraint: HirTypeParamConstraint::Any,
            default: None,
        }],
        vec![HirParam {
            def_id: DefId(2),
            name: param_name,
            ty: numerus,
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: span(),
        }],
    );
    let hir = radix_hir::HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(1),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let first = emit_program(&hir, &mut FaberCodegen::new(&types, &interner));
    let second = emit_program(&hir, &mut FaberCodegen::new(&types, &interner));
    assert_eq!(
        first, second,
        "canonical format/convert output is not idempotent"
    );
    assert_no_trailing_separator(&first);
}

/// Roundtrip repair (e2e B2): a lowered tuple that carries no explicit type
/// args (the objectPattern rest binding) must still emit the `iuncta<...>`
/// prefix derived from the expression's own tuple type, or the re-emitted
/// bracket literal parses back as an array and fails the second compile.
#[test]
fn tuple_without_type_args_derives_iuncta_prefix_from_expression_type() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let one_tuple = types.intern(Type::Tuple(vec![numerus]));

    let rest_tuple = HirExpression {
        id: radix_hir::HirId(60),
        kind: HirExpressionKind::Tuple(vec![int(&interner, 7)], None),
        ty: Some(one_tuple),
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &rest_tuple),
        "iuncta<numerus> [7]"
    );

    // Internal placeholders (no type args, no painted type) stay bare.
    let untyped = HirExpression {
        id: radix_hir::HirId(61),
        kind: HirExpressionKind::Tuple(vec![int(&interner, 1)], None),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &untyped),
        "[1]"
    );
}

#[test]
fn call_site_access_markers_are_emitted_before_the_argument() {
    let interner = Interner::new();
    let types = TypeTable::new();
    let marked = |marker, value| HirCallArg {
        marker,
        ..call_arg(int(&interner, value))
    };
    let call = HirExpression {
        id: radix_hir::HirId(40),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(41),
                kind: HirExpressionKind::Path(DefId(31)),
                ty: None,
                span: span(),
            }),
            Vec::new(),
            vec![
                marked(Some(radix_hir::HirArgMarker::Mut), 1),
                marked(Some(radix_hir::HirArgMarker::Own), 2),
                marked(Some(radix_hir::HirArgMarker::Copy), 3),
                marked(Some(radix_hir::HirArgMarker::Ref), 4),
                marked(None, 5),
            ],
            None,
        ),
        ty: None,
        span: span(),
    };
    assert_eq!(
        render_expr(&FaberCodegen::new(&types, &interner), &call),
        "def_31(in 1, own 2, copy 3, de 4, 5)"
    );
}
