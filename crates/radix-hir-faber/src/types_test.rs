use super::*;
use crate::{FaberCodegen, KeywordSurface, TypeBindingKind, empty_presentation};
use radix_hir::{
    DefId, HirAnnotation, HirBlock, HirCallArg, HirCape, HirCasuArm, HirConst, HirEnum,
    HirExpression, HirExpressionKind, HirFunction, HirId, HirImport, HirImportItem, HirInterface,
    HirInterfaceMethod, HirItem, HirItemKind, HirLiteral, HirLocal, HirModule, HirObjectField,
    HirObjectKey, HirOrdoEnum, HirOrdoMember, HirPattern, HirPresentation, HirSchema,
    HirSchemaColumn, HirStatement, HirStatementKind, HirStruct, HirTypeAlias, HirVariant,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_types::TypeTable;

/// Minimal Latin surface so the leaf crate can unit-test without the façade pack.
struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Incipit => "incipit",
            TokenKind::Incipiet => "incipiet",
            TokenKind::Argumenta => "argumenta",
            TokenKind::De => "de",
            TokenKind::In => "in",
            TokenKind::Own => "own",
            TokenKind::Copy => "copy",
            TokenKind::Functio => "functio",
            TokenKind::Redde => "redde",
            TokenKind::Secus => "secus",
            TokenKind::Si => "si",
            TokenKind::Sin => "sin",
            TokenKind::Ergo => "ergo",
            TokenKind::Fixum => "fixum",
            TokenKind::Cape => "cape",
            TokenKind::Fac => "fac",
            TokenKind::Dum => "dum",
            TokenKind::Discerne => "discerne",
            TokenKind::Omnia => "omnia",
            TokenKind::Casu => "casu",
            TokenKind::Ut => "ut",
            TokenKind::Verum => "verum",
            TokenKind::Falsum => "falsum",
            TokenKind::Importa => "importa",
            TokenKind::Ex => "ex",
            TokenKind::Publica => "publica",
            TokenKind::Privata => "privata",
            TokenKind::Sparge => "sparge",
            TokenKind::Lege => "lege",
            TokenKind::Rumpe => "rumpe",
            TokenKind::Vide => "vide",
            TokenKind::Mone => "mone",
            TokenKind::Nota => "nota",
            TokenKind::Adfirma => "adfirma",
            TokenKind::Requirit => "requirit",
            TokenKind::Reice => "reice",
            TokenKind::Mori => "mori",
            TokenKind::Iace => "iace",
            TokenKind::Finge => "finge",
            TokenKind::Cede => "cede",
            TokenKind::Tacet => "tacet",
            TokenKind::Perge => "perge",
            TokenKind::Custodi => "custodi",
            TokenKind::Varia => "varia",
            TokenKind::Itera => "itera",
            TokenKind::Nihil => "nihil",
            TokenKind::Non => "non",
            TokenKind::Est => "est",
            TokenKind::Sponte => "sponte",
            TokenKind::Vel => "vel",
            TokenKind::Scriptum => "scriptum",
            TokenKind::Proba => "proba",
            TokenKind::Omitte => "omitte",
            TokenKind::Futurum => "futurum",
            TokenKind::Solum => "solum",
            TokenKind::Tag => "tag",
            TokenKind::Temporis => "temporis",
            TokenKind::Metior => "metior",
            TokenKind::Repete => "repete",
            TokenKind::Fragilis => "fragilis",
            TokenKind::SolumIn => "solum_in",
            TokenKind::Magnitudo => "magnitudo",
            TokenKind::Per => "per",
            TokenKind::Ad => "ad",
            TokenKind::Genus => "genus",
            TokenKind::Discretio => "discretio",
            TokenKind::Implendum => "implendum",
            TokenKind::Typus => "typus",
            TokenKind::Implet => "implet",
            TokenKind::Generis => "generis",
            TokenKind::Ordo => "ordo",
            TokenKind::Schema => "schema",
            TokenKind::Columna => "columna",
            _ => "?",
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Textus) => "textus",
            TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
            TypeBindingKind::Primitive(Primitive::Fractus) => "fractus",
            TypeBindingKind::Primitive(Primitive::Bivalens) => "bivalens",
            TypeBindingKind::Primitive(Primitive::Nihil) => "nihil",
            TypeBindingKind::Primitive(Primitive::Vacuum) => "vacuum",
            TypeBindingKind::Primitive(Primitive::Valor) => "valor",
            TypeBindingKind::Primitive(Primitive::Ascii) => "ascii",
            TypeBindingKind::Primitive(Primitive::Octeti) => "octeti",
            TypeBindingKind::Primitive(Primitive::Instans) => "instans",
            TypeBindingKind::Collection(CollectionKind::Lista) => "lista",
            TypeBindingKind::Collection(CollectionKind::Tabula) => "tabula",
            TypeBindingKind::Collection(CollectionKind::Copia) => "copia",
            TypeBindingKind::Collection(CollectionKind::Promissum) => "promissum",
            TypeBindingKind::Collection(CollectionKind::Cursor) => "cursor",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor) => "tensor",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Vector) => "vector",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Matrix) => "matrix",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Sparsa) => "sparsa",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Atomic) => "atomic",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum) => "intervallum",
            TypeBindingKind::ModularWord => "modulus",
            TypeBindingKind::Ratio => "ratio",
            _ => "ignotum",
        }
    }
}

#[test]
fn type_to_faber_prints_imported_qualified_spelling() {
    // Regression: an imported nominal (`base.Vector3`) has no local def
    // name, so the emitter printed the `def_N` fallback. The qualified
    // spelling carried from the import contract must win.
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let interner = Interner::new();
    let surface = LatinSurface;

    let type_id = types.intern(Type::Struct(DefId(0x9000_0001)));
    let mut codegen = FaberCodegen::new(&types, &interner);
    codegen
        .imported_type_spellings
        .insert(type_id, "base.Vector3".to_owned());
    assert_eq!(
        codegen.type_to_faber(type_id, &names, &surface),
        "base.Vector3"
    );
}

#[test]
fn type_to_faber_renders_primitives_lista_and_option() {
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let interner = Interner::new();
    let surface = LatinSurface;

    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let lista = types.array(numerus);
    let option = types.option(textus);
    let i32_ty = types.sized_numeric(Primitive::Numerus, radix_types::NumericWidth::I32);
    let codegen = FaberCodegen::new(&types, &interner);
    assert_eq!(codegen.type_to_faber(textus, &names, &surface), "textus");
    assert_eq!(
        codegen.type_to_faber(lista, &names, &surface),
        "lista<numerus>"
    );
    assert_eq!(
        codegen.type_to_faber(option, &names, &surface),
        "textus ∪ nihil"
    );
    assert_eq!(codegen.type_to_faber(i32_ty, &names, &surface), "i32");
}

#[test]
fn type_to_faber_renders_intervallum_step() {
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let interner = Interner::new();
    let surface = LatinSurface;

    let numerus = types.primitive(Primitive::Numerus);
    let step_two = types.intern_index(radix_types::IndexExpr::Literal(2));
    let step_one = types.intern_index(radix_types::IndexExpr::Literal(1));
    let stepped = types.intervallum_with_step(numerus, step_two);
    let bare = types.intervallum(numerus);
    let explicit_one = types.intervallum_with_step(numerus, step_one);
    let codegen = FaberCodegen::new(&types, &interner);

    assert_eq!(
        codegen.type_to_faber(stepped, &names, &surface),
        "intervallum<numerus, 2>"
    );
    // Step 1 is canonical silence: bare and explicit `1` share one spelling.
    assert_eq!(
        codegen.type_to_faber(bare, &names, &surface),
        "intervallum<numerus>"
    );
    assert_eq!(
        codegen.type_to_faber(explicit_one, &names, &surface),
        "intervallum<numerus>"
    );
}

#[test]
fn type_to_faber_emits_fn_type_param_modes() {
    use radix_types::{FuncSig, ParamType, SemanticParamMode};
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let interner = Interner::new();
    let surface = LatinSurface;

    let textus = types.primitive(Primitive::Textus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let ref_textus = types.reference(SemanticMutability::Immutable, textus);

    // A type-position `de textus` param carries the marker on the type, so
    // the underlying `Type::Ref` prints it and no separate prefix is added.
    let sig = FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: ref_textus,
            mode: SemanticParamMode::Ref,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let func_ty = types.intern(radix_types::Type::Func(sig));

    // A mode that is not encoded on the type (e.g. `own` move) gets an
    // explicit type-position marker instead of being silently dropped.
    let owned_sig = FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: textus,
            mode: SemanticParamMode::Move,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let move_func_ty = types.intern(radix_types::Type::Func(owned_sig));
    let codegen = FaberCodegen::new(&types, &interner);
    assert_eq!(
        codegen.type_to_faber(func_ty, &names, &surface),
        "(de textus) → vacuum"
    );
    assert_eq!(
        codegen.type_to_faber(move_func_ty, &names, &surface),
        "(own textus) → vacuum"
    );
}

#[test]
fn flatten_option_strips_nested_wrappers() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let once = types.option(textus);
    let twice = types.option(once);
    let interner = Interner::new();
    let codegen = FaberCodegen::new(&types, &interner);
    assert_eq!(codegen.flatten_option(twice), textus);
    assert_eq!(codegen.flatten_option(textus), textus);
}

#[test]
fn emit_empty_hir_program_succeeds() {
    let hir = HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let types = TypeTable::new();
    let interner = Interner::new();
    let out = FaberCodegen::new(&types, &interner)
        .generate(&hir, &empty_presentation(), &LatinSurface)
        .expect("empty HIR must emit");
    assert!(
        out.code.is_empty(),
        "empty program should emit empty source, got {:?}",
        out.code
    );
}

// ── Tensor-family + vacua surface routing probe ───────────────────────

/// Probe surface that maps tensor-family and vacua identities to markers.
///
/// If the emitter printed hardcoded Latin, the markers would never appear.
struct ProbeSurface;

impl KeywordSurface for ProbeSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn contextual_keyword(&self, latin_key: &'static str) -> &str {
        match latin_key {
            "vacua" => "VACUA_MARKER",
            key => key,
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor) => "TENSOR_MARKER",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Vector) => "VECTOR_MARKER",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Matrix) => "MATRIX_MARKER",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Sparsa) => "SPARSA_MARKER",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Atomic) => "ATOMIC_MARKER",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum) => "INTERVALLUM_MARKER",
            TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
            _ => "ignotum",
        }
    }
}

#[test]
fn tensor_family_type_emit_routes_through_type_surface() {
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let interner = Interner::new();
    let surface = ProbeSurface;

    let i32_ty = types.sized_numeric(Primitive::Numerus, radix_types::NumericWidth::I32);
    let rank0 = types.rank0_shape();
    let lit4 = types.intern_index(radix_types::IndexExpr::Literal(4));
    let width4 = types.intern_index(radix_types::IndexExpr::Tuple(vec![lit4]));
    let lit2 = types.intern_index(radix_types::IndexExpr::Literal(2));
    let square = types.intern_index(radix_types::IndexExpr::Tuple(vec![lit2, lit2]));
    let tensor = types.tensor_with_shape(i32_ty, rank0);
    let vector = types.vector_with_width(i32_ty, width4);
    let matrix = types.matrix_with_shape(i32_ty, square);
    let sparsa = types.sparsa_with_shape(i32_ty, rank0);
    let atomic = types.atomic(i32_ty);
    let intervallum = types.intervallum(types.primitive(Primitive::Numerus));
    let codegen = FaberCodegen::new(&types, &interner);

    assert_eq!(
        codegen.type_to_faber(tensor, &names, &surface),
        "TENSOR_MARKER<i32, []>"
    );
    assert_eq!(
        codegen.type_to_faber(vector, &names, &surface),
        "VECTOR_MARKER<i32, [4]>"
    );
    assert_eq!(
        codegen.type_to_faber(matrix, &names, &surface),
        "MATRIX_MARKER<i32, [2, 2]>"
    );
    assert_eq!(
        codegen.type_to_faber(sparsa, &names, &surface),
        "SPARSA_MARKER<i32, []>"
    );
    assert_eq!(
        codegen.type_to_faber(atomic, &names, &surface),
        "ATOMIC_MARKER<i32>"
    );
    assert_eq!(
        codegen.type_to_faber(intervallum, &names, &surface),
        "INTERVALLUM_MARKER<numerus>"
    );
}

#[test]
fn vacua_emit_routes_through_contextual_keyword() {
    // A bare entry block containing only the empty-collection marker: the
    // emitted source must carry the probe marker, proving the Vacua arm
    // consults the surface instead of printing the literal "vacua".
    let interner = Interner::new();
    let hir = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(
            vec![HirStatement {
                id: HirId(1),
                kind: HirStatementKind::Expr(HirExpression {
                    id: HirId(2),
                    kind: HirExpressionKind::Vacua,
                    ty: None,
                    span: span(),
                }),
                span: span(),
            }],
            None,
            span(),
        )),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let types = TypeTable::new();
    let out = FaberCodegen::new(&types, &interner)
        .generate(&hir, &empty_presentation(), &ProbeSurface)
        .expect("vacua program must emit");
    assert!(
        out.code.contains("VACUA_MARKER"),
        "Vacua emit must route through contextual_keyword, got:\n{}",
        out.code
    );
    assert!(
        !out.code.contains("vacua"),
        "Vacua emit must not print the literal Latin spelling, got:\n{}",
        out.code
    );
}

#[test]
fn vacua_emit_uses_canonical_when_surface_spelling_is_bound() {
    // A collected binding that shares the marker surface does not divert
    // emission: the marker always prints the canonical surface. Re-parse
    // re-claims the spelling as the marker (null-like shadowing).
    let mut interner = Interner::new();
    let bound = interner.intern("VACUA_MARKER");
    let hir = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(
            vec![HirStatement {
                id: HirId(1),
                kind: HirStatementKind::Local(HirLocal {
                    def_id: DefId(1),
                    name: bound,
                    ty: None,
                    init: Some(HirExpression {
                        id: HirId(2),
                        kind: HirExpressionKind::Vacua,
                        ty: None,
                        span: span(),
                    }),
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: span(),
            }],
            None,
            span(),
        )),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let types = TypeTable::new();
    let out = FaberCodegen::new(&types, &interner)
        .generate(&hir, &empty_presentation(), &ProbeSurface)
        .expect("shadowed vacua program must emit");
    assert!(
        out.code.contains("← VACUA_MARKER"),
        "Vacua emit must stay canonical when a same-spelled binding exists, got:\n{}",
        out.code
    );
    assert!(
        !out.code.contains("VACUA_ALIAS"),
        "Vacua emit must not fall back to a pack alias, got:\n{}",
        out.code
    );
}

fn span() -> Span {
    Span::default()
}

fn generate_faber(program: &HirModule, codegen: &mut FaberCodegen<'_>) -> String {
    generate_faber_with_presentation(program, &empty_presentation(), codegen)
}

fn generate_faber_with_presentation(
    program: &HirModule,
    presentation: &HirPresentation,
    codegen: &mut FaberCodegen<'_>,
) -> String {
    codegen
        .generate(program, presentation, &LatinSurface)
        .expect("codegen")
        .code
}

fn module_with_item(item: HirItem) -> HirModule {
    HirModule {
        items: vec![item],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

fn assert_publica_precedes(code: &str, declaration: &str) {
    let annotation = code
        .lines()
        .position(|line| line.contains("publica"))
        .unwrap_or_else(|| panic!("item must re-emit its @ publica annotation:\n{code}"));
    let declaration = code
        .lines()
        .position(|line| line.contains(declaration))
        .unwrap_or_else(|| panic!("declaration `{declaration}` must be emitted:\n{code}"));
    assert!(
        annotation < declaration,
        "@ publica must precede `{declaration}`:\n{code}"
    );
}

#[test]
fn multi_import_emits_all_items() {
    let mut interner = Interner::new();
    let path_sym = interner.intern("pkg");
    let name_a = interner.intern("alpha");
    let name_b = interner.intern("beta");

    let types = TypeTable::new();

    let import = HirImport {
        path: path_sym,
        visibility: radix_syntax::Visibility::Publica,
        items: vec![
            HirImportItem {
                def_id: DefId(1),
                name: name_a,
                alias: None,
            },
            HirImportItem {
                def_id: DefId(2),
                name: name_b,
                alias: None,
            },
        ],
        selective: false,
        target: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Import(import),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("alpha") && code.contains("beta"),
        "multi-import must emit all items, got:\n{code}"
    );
}

#[test]
fn discerne_omnia_preserves_exhaustiveness() {
    let mut interner = Interner::new();
    let name_f = interner.intern("f");
    let name_x = interner.intern("x");

    let types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);

    let discerne = HirExpression {
        id: HirId(10),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![HirExpression {
                id: HirId(11),
                kind: HirExpressionKind::Path(DefId(1)),
                ty: Some(bivalens),
                span: span(),
            }],
            arms: vec![HirCasuArm {
                patterns: vec![HirPattern::Variant(
                    DefId(2),
                    vec![HirPattern::Binding(DefId(3), name_x)],
                )],
                body: HirExpression {
                    id: HirId(12),
                    kind: HirExpressionKind::Block(HirBlock {
                        statements: vec![],
                        expr: Some(Box::new(HirExpression {
                            id: HirId(13),
                            kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                            ty: Some(bivalens),
                            span: span(),
                        })),
                        span: span(),
                        ..Default::default()
                    }),
                    ty: Some(bivalens),
                    span: span(),
                },
                span: span(),
            }],
            exhaustive: true,
        },
        ty: Some(bivalens),
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: name_f,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(bivalens),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(20),
                kind: HirStatementKind::Expr(discerne),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("omnia"),
        "exhaustive discerne must emit `omnia` keyword, got:\n{code}"
    );
}

#[test]
fn si_with_cape_preserves_cape_keyword() {
    let mut interner = Interner::new();
    let name_f = interner.intern("f");
    let name_err = interner.intern("err");

    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let bivalens = types.primitive(Primitive::Bivalens);

    let then_block = HirBlock {
        statements: vec![HirStatement {
            id: HirId(20),
            kind: HirStatementKind::Redde(Some(HirExpression {
                id: HirId(21),
                kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("ok"))),
                ty: Some(textus),
                span: span(),
            })),
            span: span(),
        }],
        expr: None,
        span: span(),
        ..Default::default()
    };

    let else_block = HirBlock {
        statements: vec![HirStatement {
            id: HirId(30),
            kind: HirStatementKind::Redde(Some(HirExpression {
                id: HirId(31),
                kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("nope"))),
                ty: Some(textus),
                span: span(),
            })),
            span: span(),
        }],
        expr: None,
        span: span(),
        ..Default::default()
    };

    let catch = HirCape {
        binding_def_id: DefId(100),
        binding_name: name_err,
        binding_ty: Some(textus),
        body: HirBlock {
            statements: vec![HirStatement {
                id: HirId(40),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: HirId(41),
                    kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("caught"))),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        },
        span: span(),
    };

    let expr = HirExpression {
        id: HirId(10),
        kind: HirExpressionKind::Si {
            cond: Box::new(HirExpression {
                id: HirId(11),
                kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                ty: Some(bivalens),
                span: span(),
            }),
            then_block,
            then_catch: Some(Box::new(catch)),
            else_block: Some(else_block),
        },
        ty: Some(textus),
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: name_f,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(textus),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(22),
                kind: HirStatementKind::Expr(expr),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("cape"),
        "si with then_catch + else_block must emit `cape` keyword, got:\n{code}"
    );
    assert!(
        code.contains("err"),
        "cape binding name must appear in output, got:\n{code}"
    );
}

#[test]
fn si_cape_else_with_compact_else_preserves_cape() {
    // Si with then_catch AND an else_block that is sic/secus compatible
    // (single-expression return). The cape must still appear.
    let mut interner = Interner::new();
    let name_f = interner.intern("f");
    let name_err = interner.intern("err");

    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let bivalens = types.primitive(Primitive::Bivalens);

    let then_block = HirBlock {
        statements: vec![HirStatement {
            id: HirId(20),
            kind: HirStatementKind::Redde(Some(HirExpression {
                id: HirId(21),
                kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("ok"))),
                ty: Some(textus),
                span: span(),
            })),
            span: span(),
        }],
        expr: None,
        span: span(),
        ..Default::default()
    };

    let else_block = HirBlock {
        statements: vec![HirStatement {
            id: HirId(30),
            kind: HirStatementKind::Redde(Some(HirExpression {
                id: HirId(31),
                kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("nope"))),
                ty: Some(textus),
                span: span(),
            })),
            span: span(),
        }],
        expr: None,
        span: span(),
        ..Default::default()
    };

    let catch = HirCape {
        binding_def_id: DefId(100),
        binding_name: name_err,
        binding_ty: Some(textus),
        body: HirBlock {
            statements: vec![],
            expr: Some(Box::new(HirExpression {
                id: HirId(41),
                kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("caught"))),
                ty: Some(textus),
                span: span(),
            })),
            span: span(),
            ..Default::default()
        },
        span: span(),
    };

    let expr = HirExpression {
        id: HirId(10),
        kind: HirExpressionKind::Si {
            cond: Box::new(HirExpression {
                id: HirId(11),
                kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                ty: Some(bivalens),
                span: span(),
            }),
            then_block,
            then_catch: Some(Box::new(catch)),
            else_block: Some(else_block),
        },
        ty: Some(textus),
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: name_f,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(textus),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(22),
                kind: HirStatementKind::Expr(expr),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("cape"),
        "si with then_catch + else_block must emit `cape` keyword, got:\n{code}"
    );
    assert!(
        code.contains("caught"),
        "cape body value must appear, got:\n{code}"
    );
    assert!(
        code.contains("nope"),
        "else_block value must appear, got:\n{code}"
    );
    let cape_pos = code.find("cape").unwrap();
    let secus_pos = code.find("secus").unwrap();
    assert!(
        cape_pos < secus_pos,
        "cape must appear before secus, got cape at {cape_pos}, secus at {secus_pos}\n{code}"
    );
}

#[test]
fn fac_loop_detection_long_body() {
    // A `fac ... dum` where the body has multiple statements before the
    // final rumpe+not pattern. `as_fac_loop` must still recognize it.
    let mut interner = Interner::new();
    let name_f = interner.intern("f");

    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let bivalens = types.primitive(Primitive::Bivalens);
    let i32_ty = types.sized_numeric(Primitive::Numerus, radix_types::NumericWidth::I32);

    let rumpe_si = HirExpression {
        id: HirId(30),
        kind: HirExpressionKind::Si {
            cond: Box::new(HirExpression {
                id: HirId(31),
                kind: HirExpressionKind::Unary(
                    radix_hir::HirUnOp::Not,
                    Box::new(HirExpression {
                        id: HirId(32),
                        kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                        ty: Some(bivalens),
                        span: span(),
                    }),
                ),
                ty: Some(bivalens),
                span: span(),
            }),
            then_block: HirBlock {
                statements: vec![HirStatement {
                    id: HirId(33),
                    kind: HirStatementKind::Rumpe,
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            },
            then_catch: None,
            else_block: None,
        },
        ty: Some(bivalens),
        span: span(),
    };

    let loop_block = HirBlock {
        statements: vec![
            HirStatement {
                id: HirId(40),
                kind: HirStatementKind::Local(radix_hir::HirLocal {
                    def_id: DefId(50),
                    name: interner.intern("x"),
                    ty: Some(i32_ty),
                    mutable: true,
                    init: Some(HirExpression {
                        id: HirId(41),
                        kind: HirExpressionKind::Literal(HirLiteral::Int(0)),
                        ty: Some(numerus),
                        span: span(),
                    }),
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                    is_await: false,
                }),
                span: span(),
            },
            HirStatement {
                id: HirId(42),
                kind: HirStatementKind::Expr(rumpe_si),
                span: span(),
            },
        ],
        expr: None,
        span: span(),
        ..Default::default()
    };

    let expr = HirExpression {
        id: HirId(10),
        kind: HirExpressionKind::Loop(loop_block),
        ty: Some(numerus),
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: name_f,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(numerus),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(22),
                kind: HirStatementKind::Expr(expr),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("fac"),
        "multi-statement loop body must be recognized as fac, got:\n{code}"
    );
    assert!(
        code.contains("dum"),
        "loop must contain dum keyword, got:\n{code}"
    );
}

#[test]
fn record_type_preserves_field_names() {
    // Record types must emit field names, not just value type de-duplication.
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let mut interner = Interner::new();
    let surface = LatinSurface;

    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    let name_sym = interner.intern("name");
    let desc_sym = interner.intern("desc");
    let age_sym = interner.intern("age");
    let mut fields = FxHashMap::default();
    fields.insert(name_sym, textus);
    fields.insert(desc_sym, textus);
    fields.insert(age_sym, numerus);
    let record_id = types.intern(radix_types::Type::Record(fields));
    let codegen = FaberCodegen::new(&types, &interner);

    let result = codegen.type_to_faber(record_id, &names, &surface);
    assert_eq!(result, "ratio<age: numerus, desc: textus, name: textus>");
}

#[test]
fn record_type_renders_as_sorted_ratio() {
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let mut interner = Interner::new();
    let surface = LatinSurface;

    let g = interner.intern("g");
    let w = interner.intern("w");
    let f32_ty = types.sized_numeric(Primitive::Fractus, radix_types::NumericWidth::F32);
    let numerus = types.primitive(Primitive::Numerus);
    let record = types.record([(w, numerus), (g, f32_ty)]);
    let codegen = FaberCodegen::new(&types, &interner);

    assert_eq!(
        codegen.type_to_faber(record, &names, &surface),
        "ratio<g: f32, w: numerus>"
    );
}

/// English surface for the record head: the pack row is `[types] ratio =
/// "record"`; the Latin contextual-keyword path would still print `ratio`.
struct EnglishRecordSurface;

impl KeywordSurface for EnglishRecordSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Ratio => "record",
            TypeBindingKind::Primitive(Primitive::Numerus) => "int",
            TypeBindingKind::Primitive(Primitive::Fractus) => "float",
            _ => "ignotum",
        }
    }
}

#[test]
fn record_type_renders_the_surface_record_spelling_in_en() {
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let mut interner = Interner::new();
    let g = interner.intern("g");
    let w = interner.intern("w");
    let fractus = types.primitive(Primitive::Fractus);
    let numerus = types.primitive(Primitive::Numerus);
    let record = types.record([(w, numerus), (g, fractus)]);
    let codegen = FaberCodegen::new(&types, &interner);

    assert_eq!(
        codegen.type_to_faber(record, &names, &EnglishRecordSurface),
        "record<g: float, w: int>"
    );
    // The la surface (LatinSurface) still emits the Latin spelling.
    assert!(
        codegen
            .type_to_faber(record, &names, &LatinSurface)
            .starts_with("ratio<")
    );
}

#[test]
fn record_construction_emits_sorted_ratio_form_and_reparses() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let g = interner.intern("g");
    let w = interner.intern("w");
    let r = interner.intern("r");
    let f32_ty = types.sized_numeric(Primitive::Fractus, radix_types::NumericWidth::F32);
    let numerus = types.primitive(Primitive::Numerus);
    let record = types.record([(w, numerus), (g, f32_ty)]);

    let verte = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Verte {
            source: Box::new(HirExpression {
                id: HirId(3),
                kind: HirExpressionKind::Literal(HirLiteral::Nil),
                ty: None,
                span: span(),
            }),
            target: record,
            entries: Some(vec![
                HirObjectField {
                    key: HirObjectKey::Ident(w),
                    value: Some(HirExpression {
                        id: HirId(4),
                        kind: HirExpressionKind::Literal(HirLiteral::Int(2)),
                        ty: Some(numerus),
                        span: span(),
                    }),
                },
                HirObjectField {
                    key: HirObjectKey::Ident(g),
                    value: Some(HirExpression {
                        id: HirId(5),
                        kind: HirExpressionKind::Literal(HirLiteral::Float(1.0)),
                        ty: Some(f32_ty),
                        span: span(),
                    }),
                },
            ]),
        },
        ty: Some(record),
        span: span(),
    };

    let program = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(
            vec![HirStatement {
                id: HirId(1),
                kind: HirStatementKind::Local(HirLocal {
                    def_id: DefId(1),
                    name: r,
                    ty: Some(record),
                    init: Some(verte),
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: span(),
            }],
            None,
            span(),
        )),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("ratio<g: f32, w: numerus> r ← ratio<g: f32, w: numerus> {"),
        "ratio construction must emit the sorted typed constructor, got:\n{code}"
    );
    assert!(
        code.contains("g = 1.0") && code.contains("w = 2"),
        "construction fields must keep genus `field = value` form, got:\n{code}"
    );
    let g_at = code.find("g = 1.0").expect("g field");
    let w_at = code.find("w = 2").expect("w field");
    assert!(
        g_at < w_at,
        "construction fields must be sorted by label, got:\n{code}"
    );

    let parsed = radix_parser::parse(radix_lexer::lex(&code));
    assert!(
        parsed.success(),
        "emitted ratio construction must re-parse, got:\n{code}\nerrors: {:?}",
        parsed.errors
    );
}

#[test]
fn closure_params_preserve_sponte_and_defaults() {
    let mut interner = Interner::new();
    let name_f = interner.intern("f");
    let name_x = interner.intern("x");
    let name_y = interner.intern("y");

    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);

    let closure = HirExpression {
        id: HirId(10),
        kind: HirExpressionKind::Clausura(
            vec![
                radix_hir::HirParam {
                    def_id: DefId(1),
                    name: name_x,
                    ty: textus,
                    mode: radix_hir::HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: span(),
                },
                radix_hir::HirParam {
                    def_id: DefId(2),
                    name: name_y,
                    ty: textus,
                    mode: radix_hir::HirParamMode::Owned,
                    optional: true,
                    sponte: true,
                    default: Some(HirExpression {
                        id: HirId(20),
                        kind: HirExpressionKind::Literal(HirLiteral::String(
                            interner.intern("default"),
                        )),
                        ty: Some(textus),
                        span: span(),
                    }),
                    span: span(),
                },
            ],
            Some(textus),
            None,
            Box::new(HirExpression {
                id: HirId(30),
                kind: HirExpressionKind::Block(HirBlock {
                    statements: vec![],
                    expr: Some(Box::new(HirExpression {
                        id: HirId(31),
                        kind: HirExpressionKind::Path(DefId(1)),
                        ty: Some(textus),
                        span: span(),
                    })),
                    span: span(),
                    ..Default::default()
                }),
                ty: Some(textus),
                span: span(),
            }),
        ),
        ty: Some(textus),
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: name_f,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(textus),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(22),
                kind: HirStatementKind::Expr(closure),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("sponte"),
        "closure param with sponte must emit 'sponte', got:\n{code}"
    );
    assert!(
        code.contains("vel"),
        "closure param with default must emit 'vel', got:\n{code}"
    );
    assert!(
        code.contains("default"),
        "closure param default value must appear, got:\n{code}"
    );
}

// ── Emitter convert-defect regressions (VM-U3 / gradus S1) ──────────────

#[test]
fn import_privata_emits_no_visibility_keyword() {
    // Canonical import emission writes no visibility keyword for the
    // non-public tier; `faber convert` output must re-parse.
    let mut interner = Interner::new();
    let path_sym = interner.intern("gradus:dtype");
    let name = interner.intern("dtype");

    let types = TypeTable::new();

    let import = HirImport {
        path: path_sym,
        visibility: radix_syntax::Visibility::Privata,
        items: vec![HirImportItem {
            def_id: DefId(1),
            name,
            alias: None,
        }],
        selective: false,
        target: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Import(import),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert_eq!(
        code.trim(),
        r#"importa ex "gradus:dtype" dtype"#,
        "import must emit without a visibility keyword"
    );
}

/// Reader-locale surface that localizes the selective-import keywords so the
/// locale-preservation proof exercises real pack spellings, not Latin identity.
struct LocaleSurface;

impl KeywordSurface for LocaleSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Importa => "import",
            TokenKind::Ex => "from",
            TokenKind::Publica => "public",
            TokenKind::Fixum => "const",
            TokenKind::Ut => "as",
            _ => LatinSurface.keyword(kind),
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        LatinSurface.type_surface(kind)
    }
}

fn selective_import_program(interner: &mut Interner, publica: bool) -> HirModule {
    let path_sym = interner.intern("norma:consolum");
    let dic = interner.intern("dic");
    let output = interner.intern("output");
    let funde = interner.intern("funde");

    let import = HirImport {
        path: path_sym,
        visibility: if publica {
            radix_syntax::Visibility::Publica
        } else {
            radix_syntax::Visibility::Privata
        },
        items: vec![
            HirImportItem {
                def_id: DefId(1),
                name: dic,
                alias: Some(output),
            },
            HirImportItem {
                def_id: DefId(2),
                name: funde,
                alias: None,
            },
        ],
        selective: true,
        target: None,
    };

    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Import(import),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn selective_import_emits_ordered_fixum_member_list() {
    // EOT-2W: canonical selective re-emission keeps `fixum` and the ordered
    // member/alias list (`dic ut output, funde`), never the namespace item
    // shape and never the `* ut` wildcard spelling.
    let mut interner = Interner::new();
    let program = selective_import_program(&mut interner, false);
    let types = TypeTable::new();

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert_eq!(
        code.trim(),
        r#"importa ex "norma:consolum" fixum dic ut output, funde"#,
        "selective import must re-emit the ordered fixum member list"
    );
}

#[test]
fn selective_import_publica_emits_visibility_before_fixum() {
    let mut interner = Interner::new();
    let program = selective_import_program(&mut interner, true);
    let types = TypeTable::new();

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert_eq!(
        code.trim(),
        r#"importa ex "norma:consolum" publica fixum dic ut output, funde"#,
        "selective publica import keeps the visibility tier before `fixum`"
    );
}

#[test]
fn selective_import_localized_surface_projects_fixum_keyword() {
    // Locale-preserving re-emission: the `fixum` marker and the alias keyword
    // project through the reader surface, while the exported member spellings
    // and the caller-owned alias stay exactly as interned.
    let mut interner = Interner::new();
    let program = selective_import_program(&mut interner, false);
    let types = TypeTable::new();

    let code = FaberCodegen::new(&types, &interner)
        .generate(&program, &empty_presentation(), &LocaleSurface)
        .expect("codegen")
        .code;
    assert_eq!(
        code.trim(),
        r#"import from "norma:consolum" const dic as output, funde"#,
        "localized selective import must keep the locale keyword spellings"
    );
}

#[test]
fn selective_import_identity_alias_collapses_to_member_name() {
    // An explicit identity alias (`funde ut funde`) is semantically the bare
    // member binding; canonical emission collapses it instead of inventing a
    // redundant `ut` pair.
    let mut interner = Interner::new();
    let path_sym = interner.intern("pkg");
    let funde = interner.intern("funde");

    let import = HirImport {
        path: path_sym,
        visibility: radix_syntax::Visibility::Privata,
        items: vec![HirImportItem {
            def_id: DefId(1),
            name: funde,
            alias: Some(funde),
        }],
        selective: true,
        target: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Import(import),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let types = TypeTable::new();
    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert_eq!(
        code.trim(),
        r#"importa ex "pkg" fixum funde"#,
        "identity alias must collapse; `* ut` is the namespace wildcard shape"
    );
}

#[test]
fn union_last_variant_has_no_trailing_comma() {
    // Canonical union layout separates variants with a comma; the last
    // variant must not carry one (PARSE030 on re-parse of the converted
    // output under the source locale).
    let mut interner = Interner::new();
    let types = TypeTable::new();

    let enum_item = HirEnum {
        name: interner.intern("DType"),
        type_params: Vec::new(),
        commune_fields: Vec::new(),
        variants: vec![
            HirVariant {
                def_id: DefId(1),
                name: interner.intern("F32"),
                fields: vec![],
                span: span(),
            },
            HirVariant {
                def_id: DefId(2),
                name: interner.intern("F16"),
                fields: vec![],
                span: span(),
            },
        ],
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Enum(enum_item),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("F32,\n    F16\n"),
        "earlier variants keep the comma separator, last carries none:\n{code}"
    );
    assert!(
        !code.contains("F16,\n"),
        "last variant must not carry a trailing comma:\n{code}"
    );
}

#[test]
fn struct_publica_annotation_reemits() {
    // S1-U2 convert defect: the canonical emitter dropped `@ publica` on
    // `genus` declarations (dtype 16->14 publica decls through convert).
    // Item-level annotations must re-emit ahead of the declaration so the
    // converted module keeps its export tier.
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let publica = interner.intern("publica");

    let struct_item = HirItem {
        annotations: vec![HirAnnotation::with_fields(publica, Vec::new(), span())],
        id: HirId(0),
        def_id: DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::Struct(HirStruct {
            name: interner.intern("Artifactus"),
            type_params: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            implements: Vec::new(),
        }),
        span: span(),
    };

    let program = HirModule {
        items: vec![struct_item],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    let publica_line = code
        .lines()
        .find(|line| line.contains("publica"))
        .unwrap_or_else(|| panic!("struct must re-emit its @ publica annotation:\n{code}"));
    let struct_line = code
        .lines()
        .position(|line| line.contains("genus Artifactus"))
        .expect("struct declaration must be emitted");
    assert!(
        code.lines().position(|line| line == publica_line).unwrap() < struct_line,
        "@ publica must precede the struct declaration:\n{code}"
    );
}

#[test]
fn enum_publica_annotation_reemits() {
    // S1-U2 convert defect: the canonical emitter dropped `@ publica` on
    // `discretio` declarations. `HirEnum` now carries item annotations so
    // the converted module keeps its export tier.
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let publica = interner.intern("publica");

    let enum_item = HirItem {
        annotations: vec![HirAnnotation::with_fields(publica, Vec::new(), span())],
        id: HirId(0),
        def_id: DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::Enum(HirEnum {
            name: interner.intern("DType"),
            type_params: Vec::new(),
            commune_fields: Vec::new(),
            variants: vec![HirVariant {
                def_id: DefId(1),
                name: interner.intern("F32"),
                fields: vec![],
                span: span(),
            }],
        }),
        span: span(),
    };

    let program = HirModule {
        items: vec![enum_item],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    let publica_line = code
        .lines()
        .find(|line| line.contains("publica"))
        .unwrap_or_else(|| panic!("union must re-emit its @ publica annotation:\n{code}"));
    let discretio_line = code
        .lines()
        .position(|line| line.contains("discretio DType"))
        .expect("union declaration must be emitted");
    assert!(
        code.lines().position(|line| line == publica_line).unwrap() < discretio_line,
        "@ publica must precede the union declaration:\n{code}"
    );
}

#[test]
fn function_publica_annotation_reemits() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let item = HirItem {
        annotations: vec![HirAnnotation::with_fields(
            interner.intern("publica"),
            Vec::new(),
            span(),
        )],
        id: HirId(0),
        def_id: DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::Function(Box::new(HirFunction {
            name: interner.intern("saluta"),
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: Some(HirBlock::simple(Vec::new(), None, span())),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        })),
        span: span(),
    };

    let code = generate_faber(
        &module_with_item(item),
        &mut FaberCodegen::new(&types, &interner),
    );
    assert_publica_precedes(&code, "functio saluta");
}

#[test]
fn ordo_publica_annotation_reemits() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let item_def = DefId(radix_types::USER_DEF_ID_BASE);
    let variant_def = DefId(radix_types::USER_DEF_ID_BASE + 1);
    let item = HirItem {
        annotations: vec![HirAnnotation::with_fields(
            interner.intern("publica"),
            Vec::new(),
            span(),
        )],
        id: HirId(0),
        def_id: item_def,
        kind: HirItemKind::Enum(HirEnum {
            name: interner.intern("Color"),
            type_params: Vec::new(),
            commune_fields: Vec::new(),
            variants: vec![HirVariant {
                def_id: variant_def,
                name: interner.intern("Red"),
                fields: Vec::new(),
                span: span(),
            }],
        }),
        span: span(),
    };
    let mut presentation = empty_presentation();
    presentation.ordo_enums.push(HirOrdoEnum {
        def_id: item_def,
        members: vec![HirOrdoMember {
            def_id: variant_def,
            value: None,
        }],
    });

    let code = generate_faber_with_presentation(
        &module_with_item(item),
        &presentation,
        &mut FaberCodegen::new(&types, &interner),
    );
    assert_publica_precedes(&code, "ordo Color");
}

#[test]
fn interface_publica_annotation_reemits() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let item = HirItem {
        annotations: vec![HirAnnotation::with_fields(
            interner.intern("publica"),
            Vec::new(),
            span(),
        )],
        id: HirId(0),
        def_id: DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::Interface(HirInterface {
            name: interner.intern("Legibile"),
            type_params: Vec::new(),
            methods: vec![HirInterfaceMethod {
                def_id: DefId(radix_types::USER_DEF_ID_BASE + 1),
                availability: Vec::new(),
                name: interner.intern("lege"),
                params: Vec::new(),
                ret_ty: None,
                err_ty: None,
                posture: radix_syntax::CallablePosture::SyncFinite,
                span: span(),
            }],
        }),
        span: span(),
    };

    let code = generate_faber(
        &module_with_item(item),
        &mut FaberCodegen::new(&types, &interner),
    );
    assert_publica_precedes(&code, "implendum Legibile");
}

#[test]
fn type_alias_publica_annotation_reemits() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let item = HirItem {
        annotations: vec![HirAnnotation::with_fields(
            interner.intern("publica"),
            Vec::new(),
            span(),
        )],
        id: HirId(0),
        def_id: DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::TypeAlias(HirTypeAlias {
            name: interner.intern("Alias"),
            type_params: Vec::new(),
            ty: types.primitive(radix_types::Primitive::Numerus),
        }),
        span: span(),
    };

    let code = generate_faber(
        &module_with_item(item),
        &mut FaberCodegen::new(&types, &interner),
    );
    assert_publica_precedes(&code, "typus Alias");
}

#[test]
fn schema_publica_annotation_reemits() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let column = interner.intern("id");
    let item = HirItem {
        annotations: vec![HirAnnotation::with_fields(
            interner.intern("publica"),
            Vec::new(),
            span(),
        )],
        id: HirId(0),
        def_id: DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::Schema(HirSchema {
            name: interner.intern("Quaerenda"),
            columns: vec![HirSchemaColumn {
                name: column,
                source: column,
                ty: types.primitive(radix_types::Primitive::Numerus),
                nullable: false,
                order: 0,
            }],
        }),
        span: span(),
    };

    let code = generate_faber(
        &module_with_item(item),
        &mut FaberCodegen::new(&types, &interner),
    );
    assert_publica_precedes(&code, "schema Quaerenda");
}

#[test]
fn constant_publica_annotation_reemits() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let numerus = types.primitive(radix_types::Primitive::Numerus);
    let item = HirItem {
        annotations: vec![HirAnnotation::with_fields(
            interner.intern("publica"),
            Vec::new(),
            span(),
        )],
        id: HirId(0),
        def_id: DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::Constant(HirConst {
            name: interner.intern("VALOR"),
            ty: Some(numerus),
            value: HirExpression {
                id: HirId(1),
                kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                ty: Some(numerus),
                span: span(),
            },
            mutable: false,
            is_await: false,
            statica: None,
        }),
        span: span(),
    };

    let code = generate_faber(
        &module_with_item(item),
        &mut FaberCodegen::new(&types, &interner),
    );
    assert_publica_precedes(&code, "fixum numerus VALOR");
}

#[test]
fn import_publica_annotation_reemits() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let binding = interner.intern("console");
    let item = HirItem {
        annotations: vec![HirAnnotation::with_fields(
            interner.intern("publica"),
            Vec::new(),
            span(),
        )],
        id: HirId(0),
        def_id: DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::Import(HirImport {
            path: interner.intern("norma:console"),
            visibility: radix_syntax::Visibility::Privata,
            items: vec![HirImportItem {
                def_id: DefId(radix_types::USER_DEF_ID_BASE + 1),
                name: binding,
                alias: None,
            }],
            selective: false,
            target: None,
        }),
        span: span(),
    };

    let code = generate_faber(
        &module_with_item(item),
        &mut FaberCodegen::new(&types, &interner),
    );
    assert_publica_precedes(&code, "importa ex");
}

#[test]
fn imported_variant_pattern_uses_qualified_spelling() {
    // S1-U2 convert defect: a `discerne` arm matching an imported union
    // variant (`manifestum.Cognita`) re-emitted as the `def_N` fallback,
    // which the converted module re-parse rejected with SEM001. The
    // import contract's qualified spelling must win.
    let names = FxHashMap::default();
    let interner = Interner::new();
    let surface = LatinSurface;
    let types = TypeTable::new();
    let mut writer = radix_codegen_shared::CodeWriter::new();
    let variant_def = DefId(0x9000_0001);
    let mut codegen = FaberCodegen::new(&types, &interner);
    codegen
        .imported_def_spellings
        .insert(variant_def, "manifestum.Cognita".to_owned());
    codegen.write_pattern(
        &HirPattern::Variant(variant_def, Vec::new()),
        &names,
        &surface,
        &mut writer,
    );
    assert_eq!(
        writer.as_str(),
        "manifestum.Cognita",
        "imported variant pattern must use the qualified spelling, not def_N"
    );
}

#[test]
fn call_to_local_variant_reemits_finge_construction() {
    // `finge F32` lowers to `Call(Path(variant_def), [], [])`; the emitter
    // must recover the `finge` keyword so the emission re-parses.
    let mut interner = Interner::new();
    let types = TypeTable::new();

    let variant_def = DefId(7);
    let enum_item = HirEnum {
        name: interner.intern("DType"),
        type_params: Vec::new(),
        commune_fields: Vec::new(),
        variants: vec![HirVariant {
            def_id: variant_def,
            name: interner.intern("F32"),
            fields: vec![],
            span: span(),
        }],
    };

    let function = HirFunction {
        cli_args: None,
        name: interner.intern("f"),
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: None,
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(2),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: HirId(3),
                    kind: HirExpressionKind::Call(
                        Box::new(HirExpression {
                            id: HirId(4),
                            kind: HirExpressionKind::Path(variant_def),
                            ty: None,
                            span: span(),
                        }),
                        Vec::new(),
                        vec![],
                        None,
                    ),
                    ty: None,
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![
            HirItem {
                annotations: Vec::new(),
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Enum(enum_item),
                span: span(),
            },
            HirItem {
                annotations: Vec::new(),
                id: HirId(1),
                def_id: DefId(radix_types::USER_DEF_ID_BASE + 1),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            },
        ],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("redde finge F32"),
        "variant construction must re-emit the `finge` keyword:\n{code}"
    );
}

#[test]
fn finge_with_named_fields_reemits_braced_fields() {
    let mut interner = Interner::new();
    let types = TypeTable::new();

    let variant_def = DefId(7);
    let causa = interner.intern("causa");
    let enum_item = HirEnum {
        name: interner.intern("TensorError"),
        type_params: Vec::new(),
        commune_fields: Vec::new(),
        variants: vec![HirVariant {
            def_id: variant_def,
            name: interner.intern("FormaInvalida"),
            fields: vec![],
            span: span(),
        }],
    };

    let function = HirFunction {
        cli_args: None,
        name: interner.intern("f"),
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: None,
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(2),
                kind: HirStatementKind::Expr(HirExpression {
                    id: HirId(3),
                    kind: HirExpressionKind::Throw(Box::new(HirExpression {
                        id: HirId(30),
                        kind: HirExpressionKind::Call(
                            Box::new(HirExpression {
                                id: HirId(4),
                                kind: HirExpressionKind::Path(variant_def),
                                ty: None,
                                span: span(),
                            }),
                            Vec::new(),
                            vec![HirCallArg {
                                name: Some(causa),
                                spread: false,
                                marker: None,
                                expr: HirExpression {
                                    id: HirId(5),
                                    kind: HirExpressionKind::Path(DefId(9)),
                                    ty: None,
                                    span: span(),
                                },
                                span: span(),
                            }],
                            None,
                        ),
                        ty: None,
                        span: span(),
                    })),
                    ty: None,
                    span: span(),
                }),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![
            HirItem {
                annotations: Vec::new(),
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Enum(enum_item),
                span: span(),
            },
            HirItem {
                annotations: Vec::new(),
                id: HirId(1),
                def_id: DefId(radix_types::USER_DEF_ID_BASE + 1),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            },
        ],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("iace finge FormaInvalida { causa = def_9 }"),
        "named finge fields must re-emit in the braced form:\n{code}"
    );
}

/// Probe surface that localizes one intrinsic canonical name.
struct IntrinsicProbeSurface;

impl KeywordSurface for IntrinsicProbeSurface {
    fn keyword(&self, kind: TokenKind) -> &'static str {
        match kind {
            TokenKind::Finge => "variant",
            _ => LatinSurface.keyword(kind),
        }
    }

    fn intrinsic_surface(&self, canonical: &str) -> Option<String> {
        (canonical == "divide").then(|| "split".to_owned())
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &'static str {
        LatinSurface.type_surface(kind)
    }
}

#[test]
fn intrinsic_method_call_localizes_through_surface() {
    // HIR method symbols are canonical registry spellings; under a
    // non-`la` reader pack the canonical Latin name is a compile error on
    // re-parse. The emitter must project them through the target pack's
    // intrinsic surface rows — for genuine intrinsic-method receivers
    // (a collection, tensor, or primitive value); nominal and module-
    // import receivers keep the canonical symbol.
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let receiver_def = DefId(5);

    let body = HirBlock {
        statements: vec![HirStatement {
            id: HirId(2),
            kind: HirStatementKind::Expr(HirExpression {
                id: HirId(3),
                kind: HirExpressionKind::MethodCall(
                    Box::new(HirExpression {
                        id: HirId(4),
                        kind: HirExpressionKind::Path(receiver_def),
                        ty: Some(textus),
                        span: span(),
                    }),
                    interner.intern("divide"),
                    Vec::new(),
                    vec![HirCallArg {
                        name: None,
                        spread: false,
                        marker: None,
                        expr: HirExpression {
                            id: HirId(5),
                            kind: HirExpressionKind::Literal(HirLiteral::String(
                                interner.intern("/"),
                            )),
                            ty: None,
                            span: span(),
                        },
                        span: span(),
                    }],
                    None,
                ),
                ty: None,
                span: span(),
            }),
            span: span(),
        }],
        expr: None,
        span: span(),
        ..Default::default()
    };

    let function = HirFunction {
        cli_args: None,
        name: interner.intern("f"),
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: None,
        err_ty: None,
        body: Some(body),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = FaberCodegen::new(&types, &interner)
        .generate(&program, &empty_presentation(), &IntrinsicProbeSurface)
        .expect("codegen")
        .code;
    assert!(
        code.contains(".split("),
        "canonical intrinsic must localize through the surface:\n{code}"
    );
}

#[test]
fn cape_handler_keeps_leading_author_local() {
    // A `cape err { sit textus c ← …; … }` handler whose first authored
    // statement is a real local must keep it: the binding-local skip only
    // applies to the synthetic runtime-provided binding.
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);

    let err = interner.intern("err");
    let c = interner.intern("c");
    let catch = HirCape {
        binding_def_id: DefId(100),
        binding_name: err,
        binding_ty: Some(textus),
        body: HirBlock {
            statements: vec![
                HirStatement {
                    id: HirId(40),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id: DefId(101),
                        name: c,
                        ty: Some(textus),
                        init: Some(HirExpression {
                            id: HirId(41),
                            kind: HirExpressionKind::Literal(HirLiteral::String(
                                interner.intern("x"),
                            )),
                            ty: Some(textus),
                            span: span(),
                        }),
                        mutable: false,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span: span(),
                },
                HirStatement {
                    id: HirId(42),
                    kind: HirStatementKind::Redde(Some(HirExpression {
                        id: HirId(43),
                        kind: HirExpressionKind::Path(DefId(101)),
                        ty: Some(textus),
                        span: span(),
                    })),
                    span: span(),
                },
            ],
            expr: None,
            span: span(),
            ..Default::default()
        },
        span: span(),
    };

    let expr = HirExpression {
        id: HirId(10),
        kind: HirExpressionKind::Handled {
            body: HirBlock {
                statements: vec![],
                expr: Some(Box::new(HirExpression {
                    id: HirId(11),
                    kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("ok"))),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
                ..Default::default()
            },
            catch: Box::new(catch),
        },
        ty: Some(textus),
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: interner.intern("f"),
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: Some(textus),
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(20),
                kind: HirStatementKind::Expr(expr),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    let program = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains("cape err {") && code.contains("sit textus c ← \"x\""),
        "cape handler must keep its leading authored local:\n{code}"
    );
}

// ── ⇇ copy-into re-emission ─────────────────────────────────────────

fn copy_into_program(interner: &mut Interner) -> HirModule {
    let funde = interner.intern("funde");
    let alpha = interner.intern("alpha");
    let transfer = HirExpression {
        id: HirId(3),
        ty: None,
        kind: HirExpressionKind::CopyInto {
            target: Box::new(HirExpression {
                id: HirId(1),
                ty: None,
                kind: HirExpressionKind::Path(DefId(radix_types::USER_DEF_ID_BASE)),
                span: span(),
            }),
            value: Box::new(HirExpression {
                id: HirId(2),
                ty: None,
                kind: HirExpressionKind::Literal(HirLiteral::String(alpha)),
                span: span(),
            }),
        },
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: funde,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: None,
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(5),
                kind: HirStatementKind::Expr(transfer),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn copy_into_emits_glyph_canonical() {
    // Canonical re-emission keeps the U+21C7 glyph between the target and the
    // value, with no console or formatting vocabulary.
    let mut interner = Interner::new();
    let program = copy_into_program(&mut interner);
    let types = TypeTable::new();

    let code = generate_faber(&program, &mut FaberCodegen::new(&types, &interner));
    assert!(
        code.contains(r#"funde ⇇ "alpha""#) && !code.contains(r#""alpha" ⇇"#),
        "copy-into must re-emit one `target ⇇ value`, got:\n{code}"
    );
}

#[test]
fn copy_into_glyph_is_locale_invariant() {
    // `⇇` is an operator token, never a keyword: localized re-emission keeps
    // the glyph exactly while keywords project through the reader surface.
    let mut interner = Interner::new();
    let program = copy_into_program(&mut interner);
    let types = TypeTable::new();

    let code = FaberCodegen::new(&types, &interner)
        .generate(&program, &empty_presentation(), &LocaleSurface)
        .expect("codegen")
        .code;
    assert!(
        code.contains("⇇"),
        "localized copy-into output must keep the U+21C7 glyph, got:\n{code}"
    );

    // `⇜` is the same kind of token: sink-into keeps its glyph too.
    let mut interner = Interner::new();
    let program = sink_into_program(&mut interner);
    let code = FaberCodegen::new(&types, &interner)
        .generate(&program, &empty_presentation(), &LocaleSurface)
        .expect("codegen")
        .code;
    assert!(
        code.contains("⇜"),
        "localized sink-into output must keep the U+21DD glyph, got:\n{code}"
    );
}

// ── ⇜ sink-into re-emission ───────────────────────────────────────────

/// One function whose body is the operator form `s ⇜ "x"`. The function is
/// named for the sink so the re-emitted statement reads exactly as authored.
fn sink_into_program(interner: &mut Interner) -> HirModule {
    let s = interner.intern("s");
    let x = interner.intern("x");
    let transfer = HirExpression {
        id: HirId(3),
        ty: None,
        kind: HirExpressionKind::SinkInto {
            sink: Box::new(HirExpression {
                id: HirId(1),
                ty: None,
                kind: HirExpressionKind::Path(DefId(radix_types::USER_DEF_ID_BASE)),
                span: span(),
            }),
            item: Box::new(HirExpression {
                id: HirId(2),
                ty: None,
                kind: HirExpressionKind::Literal(HirLiteral::String(x)),
                span: span(),
            }),
        },
        span: span(),
    };

    let function = HirFunction {
        cli_args: None,
        name: s,
        type_params: Vec::new(),
        params: Vec::new(),
        ret_ty: None,
        err_ty: None,
        body: Some(HirBlock {
            statements: vec![HirStatement {
                id: HirId(5),
                kind: HirStatementKind::Expr(transfer),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };

    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Function(Box::new(function)),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn sink_into_round_trip() {
    // `s ⇜ "x"` re-emits as the same spaced statement under both reader
    // surfaces: the glyph is an operator token, so neither surface respells it
    // and the formatter keeps one space on each side.
    let mut interner = Interner::new();
    let program = sink_into_program(&mut interner);
    let types = TypeTable::new();

    let en = FaberCodegen::new(&types, &interner)
        .generate(&program, &empty_presentation(), &LocaleSurface)
        .expect("codegen")
        .code;
    let la = FaberCodegen::new(&types, &interner)
        .generate(&program, &empty_presentation(), &LatinSurface)
        .expect("codegen")
        .code;

    for (surface, code) in [("en", &en), ("la", &la)] {
        assert!(
            code.contains(r#"s ⇜ "x""#),
            "sink-into must re-emit one spaced `sink ⇜ item` on the {surface} surface, got:\n{code}"
        );
    }
    assert_eq!(en, la, "the glyph statement is locale-invariant");
}

// ── Bounded type + marker-hole surface routing ─────────────────────────

/// Reader-locale surface overriding a few type rows; the rest fall back Latin.
struct LocalizedTypeSurface;

impl KeywordSurface for LocalizedTypeSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        LatinSurface.keyword(kind)
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Collection(CollectionKind::Lista) => "list",
            TypeBindingKind::Primitive(Primitive::Textus) => "text",
            _ => LatinSurface.type_surface(kind),
        }
    }
}

#[test]
fn bounded_types_print_via_the_surface() {
    // The bounded buffer/collection heads are ordinary type identities: they
    // must localize through the reader surface like their unbounded twins
    // (`list<T>` / `text<N>`), not stay hardcoded Latin.
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let interner = Interner::new();
    let four = types.intern_index(radix_types::IndexExpr::Literal(4));
    let numerus = types.primitive(Primitive::Numerus);

    let cases = [
        (
            types.bounded_array(numerus, four),
            "list<numerus, magnitudo 4>",
        ),
        (types.bounded_textus(four), "text<magnitudo 4>"),
        (types.bounded_ascii(four), "ascii<magnitudo 4>"),
        (types.bounded_octeti(four), "list<b8, magnitudo 4>"),
    ];
    let latin_bounded_array = types.bounded_array(numerus, four);
    let latin_bounded_textus = types.bounded_textus(four);
    let latin_bounded_octeti = types.bounded_octeti(four);
    let codegen = FaberCodegen::new(&types, &interner);
    for (type_id, expected) in cases {
        assert_eq!(
            codegen.type_to_faber(type_id, &names, &LocalizedTypeSurface),
            expected
        );
    }

    // Latin surface keeps the canonical spellings.
    assert_eq!(
        codegen.type_to_faber(latin_bounded_array, &names, &LatinSurface),
        "lista<numerus, magnitudo 4>"
    );
    assert_eq!(
        codegen.type_to_faber(latin_bounded_textus, &names, &LatinSurface),
        "textus<magnitudo 4>"
    );
    assert_eq!(
        codegen.type_to_faber(latin_bounded_octeti, &names, &LatinSurface),
        "lista<b8, magnitudo 4>"
    );
}

#[test]
fn bare_b8_list_and_bounded_buffer_render_as_list_forms() {
    // Ruling d477c76d retires the `octeti`/`bytes` word: a raw-bit buffer is
    // a list of `b8`, bounded or not, and re-emits through the collection
    // surface.
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let interner = Interner::new();
    let four = types.intern_index(radix_types::IndexExpr::Literal(4));
    let b8 = types.intern(Type::Bits(BitsWidth::B8));
    let bare_list = types.array(b8);
    let bounded = types.bounded_octeti(four);
    let primitive_octeti = types.primitive(Primitive::Octeti);
    let codegen = FaberCodegen::new(&types, &interner);

    assert_eq!(
        codegen.type_to_faber(bare_list, &names, &LocalizedTypeSurface),
        "list<b8>"
    );
    assert_eq!(
        codegen.type_to_faber(bounded, &names, &LocalizedTypeSurface),
        "list<b8, magnitudo 4>"
    );
    assert_eq!(
        codegen.type_to_faber(primitive_octeti, &names, &LocalizedTypeSurface),
        "list<b8>"
    );
    assert_eq!(
        codegen.type_to_faber(bare_list, &names, &LatinSurface),
        "lista<b8>"
    );
    assert_eq!(
        codegen.type_to_faber(bounded, &names, &LatinSurface),
        "lista<b8, magnitudo 4>"
    );
}

#[test]
fn marker_hole_prints_its_family() {
    // A marker hole's family is identity (`instans<_>` is not `modulus<_>`);
    // re-emission must keep the family the semantic layer interned.
    let mut types = TypeTable::new();
    let names = FxHashMap::default();
    let interner = Interner::new();

    let holes = [
        (MarkerFamily::Modulus, "modulus<_>"),
        (MarkerFamily::Instans, "instans<_>"),
    ]
    .map(|(family, expected)| {
        (
            types.intern(radix_types::Type::MarkerHole {
                family,
                var: radix_types::InferVar(0),
            }),
            expected,
        )
    });
    let codegen = FaberCodegen::new(&types, &interner);
    for (hole, expected) in holes {
        assert_eq!(codegen.type_to_faber(hole, &names, &LatinSurface), expected);
    }
}

// ── erratum carrier surface routing probe ───────────────────────────

/// Probe surface that maps the `erratum` carrier identity to an English
/// marker. If the emitter printed the definition name, the marker would
/// never appear.
struct ErratumProbeSurface;

impl KeywordSurface for ErratumProbeSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Erratum => "error",
            TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
            _ => "ignotum",
        }
    }
}

#[test]
fn erratum_type_head_projects_through_the_pack_surface() {
    // `erratum` is a compiler-owned builtin genus, so its head must project
    // through the target pack (`error` in en) instead of re-emitting the
    // Latin definition name. A user genus that happens to be named
    // `erratum` keeps its own spelling.
    let mut types = TypeTable::new();
    let mut names = FxHashMap::default();
    let mut interner = Interner::new();
    let surface = ErratumProbeSurface;

    let numerus = types.primitive(Primitive::Numerus);
    let builtin_base = types.intern(Type::Struct(DefId(1)));
    let builtin = types.intern(Type::Applied(
        builtin_base,
        vec![radix_types::AppliedArg::Type(numerus)],
    ));
    names.insert(DefId(1), interner.intern("erratum"));

    let user_base = DefId(radix_types::USER_DEF_ID_BASE + 1);
    let user_head = types.intern(Type::Struct(user_base));
    let user = types.intern(Type::Applied(
        user_head,
        vec![radix_types::AppliedArg::Type(numerus)],
    ));
    names.insert(user_base, interner.intern("erratum"));

    let codegen = FaberCodegen::new(&types, &interner);
    assert_eq!(
        codegen.type_to_faber(builtin, &names, &surface),
        "error<numerus>"
    );
    assert_eq!(
        codegen.type_to_faber(user, &names, &surface),
        "erratum<numerus>"
    );
}
