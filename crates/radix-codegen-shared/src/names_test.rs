use super::*;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirFunction, HirId, HirItem, HirItemKind,
    HirIteraMode, HirLocal, HirModule, HirParam, HirParamMode, HirStatement, HirStatementKind,
};
use radix_lexer::Span;
use radix_types::TypeId;
use rustc_hash::FxHashSet;

// PRP-025: resolve_def must return Err for unknown DefId, not a sentinel.
#[test]
fn resolve_def_fails_for_missing() {
    let interner = Interner::new();
    let hir = HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let catalog = NameCatalog::new(&hir, &interner);
    let result = catalog.resolve_def(DefId(999));
    assert!(result.is_err(), "unknown DefId must return Err");
    let err = result.unwrap_err();
    assert_eq!(err.def_id, DefId(999));
    assert!(
        MissingDefError::MARKER.contains("unresolved"),
        "marker must be visibly non-identifier-like"
    );
}

/// Empty HIR program produces empty catalog and `resolve_def` fails.
#[test]
fn empty_catalog_fails_resolve() {
    let interner = Interner::new();
    let hir = HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let catalog = NameCatalog::new(&hir, &interner);
    assert!(!catalog.catalog_contains_def(DefId(0)));
    assert!(catalog.resolve_def(DefId(0)).is_err());
}

/// Symbol resolution through the interner round-trips correctly.
#[test]
fn resolve_symbol_roundtrip() {
    let mut interner = Interner::new();
    let hir = HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let sym = interner.intern("hello");
    let catalog = NameCatalog::new(&hir, &interner);
    assert_eq!(catalog.resolve_symbol(sym), "hello");
}

fn span() -> Span {
    Span::new(0, 1)
}

fn expr(kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(0),
        kind,
        ty: None,
        span: span(),
    }
}

fn local_stmt(def: u32, name: Symbol) -> HirStatement {
    HirStatement {
        id: HirId(def),
        kind: HirStatementKind::Local(HirLocal {
            def_id: DefId(def),
            name,
            ty: None,
            init: None,
            mutable: false,
            is_await: false,
            runtime_provided: false,
            await_store: None,
            size_decl: false,
            size_static: false,
        }),
        span: span(),
    }
}

fn function_item(
    def: u32,
    name: Symbol,
    params: Vec<HirParam>,
    body: Vec<HirStatement>,
) -> HirItem {
    HirItem {
        id: HirId(def),
        def_id: DefId(def),
        annotations: Vec::new(),
        kind: HirItemKind::Function(Box::new(HirFunction {
            name,
            type_params: Vec::new(),
            params,
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: Some(HirBlock::simple(body, None, span())),
            posture: Default::default(),
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

/// Parameters and locals are classified; function names are declarations.
#[test]
fn binding_kind_classifies_params_locals_and_declarations() {
    let mut interner = Interner::new();
    let param_name = interner.intern("amount");
    let local_name = interner.intern("total");
    let loop_name = interner.intern("item");
    let outer_name = interner.intern("outer");
    let nested_name = interner.intern("nested");

    let param = HirParam {
        def_id: DefId(10),
        name: param_name,
        ty: TypeId(0),
        mode: HirParamMode::Owned,
        optional: false,
        sponte: false,
        default: None,
        span: span(),
    };
    let loop_expr = expr(HirExpressionKind::Itera {
        mode: HirIteraMode::Ex,
        coordinates: Vec::new(),
        filum: None,
        binding: DefId(12),
        binding_name: loop_name,
        iter: Box::new(expr(HirExpressionKind::Vacua)),
        body: HirBlock::simple(Vec::new(), None, span()),
        mutable: false,
    });
    let body = vec![
        local_stmt(11, local_name),
        HirStatement {
            id: HirId(2),
            kind: HirStatementKind::Expr(loop_expr),
            span: span(),
        },
    ];
    let hir = HirModule {
        items: vec![
            function_item(1, outer_name, vec![param], body),
            function_item(2, nested_name, Vec::new(), Vec::new()),
        ],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let catalog = NameCatalog::new(&hir, &interner);

    assert_eq!(catalog.binding_kind(DefId(10)), Some(BindingKind::Param));
    assert_eq!(catalog.binding_kind(DefId(11)), Some(BindingKind::Local));
    assert_eq!(catalog.binding_kind(DefId(12)), Some(BindingKind::Local));
    assert_eq!(
        catalog.binding_kind(DefId(1)),
        None,
        "a function name is not a binding"
    );
    assert_eq!(
        catalog.binding_kind(DefId(2)),
        None,
        "a nested function name is not a binding"
    );
    assert_eq!(catalog.resolve_def(DefId(10)).ok(), Some("amount"));
}

/// Every binding is emitted with its kind's fixed prefix and original name.
#[test]
fn prefixed_binding_name_adds_kind_prefix() {
    assert_eq!(PARAM_PREFIX, "p_");
    assert_eq!(LOCAL_PREFIX, "l_");
    assert_eq!(
        prefixed_binding_name(BindingKind::Param, "float"),
        "p_float"
    );
    assert_eq!(
        prefixed_binding_name(BindingKind::Local, "float"),
        "l_float"
    );
    assert_eq!(
        prefixed_binding_name(BindingKind::Param, "p_float"),
        "p_p_float"
    );
}

/// Prefixing never merges two different binding spellings.
#[test]
fn prefixed_binding_names_are_injective() {
    let names = ["a", "ab", "b", "p_", "l_a", "float"];
    let mut seen = FxHashSet::default();
    for kind in [BindingKind::Param, BindingKind::Local] {
        for name in names {
            assert!(
                seen.insert(prefixed_binding_name(kind, name)),
                "collision for {kind:?} {name}"
            );
        }
    }
    assert_eq!(seen.len(), names.len() * 2);
}
