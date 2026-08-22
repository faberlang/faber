use super::*;
use radix_hir::HirModule;

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
