//! GPD-5 emitter proofs: declared `=` defaults re-emit on generic parameters.

use crate::surface::{KeywordSurface, TypeBindingKind};
use crate::{FaberCodegen, empty_presentation};

use radix_hir::{
    HirBlock, HirFunction, HirId, HirItem, HirItemKind, HirModule, HirParam, HirParamMode,
    HirTypeParam, HirTypeParamConstraint,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_syntax::GenericParamKind;
use radix_types::{Primitive, SemanticGenericParamDefault, TypeTable};

struct TestSurface;

impl KeywordSurface for TestSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Magnitudo => "magnitudo",
            _ => "kw",
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Textus) => "textus",
            TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
            _ => "type",
        }
    }
}

fn span() -> Span {
    Span::default()
}

fn type_param(
    def_id: u32,
    name: &str,
    kind: GenericParamKind,
    default: Option<SemanticGenericParamDefault>,
    interner: &mut Interner,
) -> HirTypeParam {
    HirTypeParam {
        def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + def_id),
        kind,
        name: interner.intern(name),
        span: span(),
        constraint: HirTypeParamConstraint::Any,
        default,
    }
}

fn function(name: &str, type_params: Vec<HirTypeParam>, interner: &mut Interner) -> HirItem {
    let types = TypeTable::new();
    HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE),
        kind: HirItemKind::Function(Box::new(HirFunction {
            name: interner.intern(name),
            type_params,
            params: vec![HirParam {
                def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 2),
                name: interner.intern("x"),
                ty: types.primitive(Primitive::Numerus),
                mode: HirParamMode::Owned,
                optional: false,
                sponte: false,
                default: None,
                span: span(),
            }],
            cli_args: None,
            ret_ty: Some(types.primitive(Primitive::Numerus)),
            err_ty: None,
            body: Some(HirBlock {
                span: span(),
                breakable: None,
                end: None,
                statements: Vec::new(),
                expr: None,
            }),
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
    }
}

fn emit_items(items: Vec<HirItem>, interner: &Interner, types: &TypeTable) -> String {
    let hir = HirModule {
        items,
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let mut codegen = FaberCodegen::default();
    codegen
        .generate(&hir, &empty_presentation(), types, interner, &TestSurface)
        .expect("hand-built module must emit")
        .code
}

#[test]
fn declared_defaults_reemit_on_generic_params() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);

    let pick = function(
        "pick",
        vec![
            type_param(
                3,
                "T",
                GenericParamKind::Typus,
                Some(SemanticGenericParamDefault::Typus(textus)),
                &mut interner,
            ),
            type_param(
                4,
                "D",
                GenericParamKind::Magnitudo,
                Some(SemanticGenericParamDefault::Magnitudo(64)),
                &mut interner,
            ),
        ],
        &mut interner,
    );
    // Explicit leading param with a defaulted trailing suffix.
    let tag = function(
        "tag",
        vec![
            type_param(5, "M", GenericParamKind::Magnitudo, None, &mut interner),
            type_param(
                6,
                "N",
                GenericParamKind::Magnitudo,
                Some(SemanticGenericParamDefault::Magnitudo(2)),
                &mut interner,
            ),
        ],
        &mut interner,
    );
    // No defaults at all: emission must stay byte-identical to the old shape.
    let identitas = function(
        "identitas",
        vec![type_param(
            7,
            "T",
            GenericParamKind::Typus,
            None,
            &mut interner,
        )],
        &mut interner,
    );

    let code = emit_items(vec![pick, tag, identitas], &interner, &types);

    assert!(
        code.contains("functio pick<T = textus, magnitudo D = 64>("),
        "type and magnitudo defaults must re-emit, got:\n{code}"
    );
    assert!(
        code.contains("functio tag<magnitudo M, magnitudo N = 2>("),
        "mixed explicit/defaulted suffix must re-emit, got:\n{code}"
    );
    assert!(
        code.contains("functio identitas<T>("),
        "params without defaults must not grow `= `, got:\n{code}"
    );
}
