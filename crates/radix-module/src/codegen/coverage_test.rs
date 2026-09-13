//! HIR coverage classification tests: each codegen target reports whether it can
//! emit a given HIR + type table, and which capability gaps block emission with
//! stable slugs (e.g. matrix, f16, modular words, `ad`).

use super::*;
use crate::hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirId, HirItem, HirItemKind, HirModule,
    HirStatement, HirStatementKind,
};
use crate::lexer::{Interner, Span};
use crate::semantic::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};

/// Minimal HIR program with no user-defined types.
fn empty_program() -> HirModule {
    HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn faber_classifier_always_capable() {
    let hir = empty_program();
    let types = TypeTable::default();
    let result = classify_hir_coverage(HirCoverageTarget::Faber, &hir, &types);
    assert!(result.is_capable());
}

#[test]
fn typescript_classifier_always_capable() {
    let hir = empty_program();
    let types = TypeTable::default();
    let result = classify_hir_coverage(HirCoverageTarget::TypeScript, &hir, &types);
    assert!(result.is_capable());
}

#[test]
fn go_rejects_modular_words_with_stable_slug() {
    let hir = empty_program();
    let mut types = TypeTable::default();
    types.intern(Type::ModularWord(NumericWidth::U32));

    let result = classify_hir_coverage(HirCoverageTarget::Go, &hir, &types);
    assert!(!result.is_capable(), "Go unexpectedly accepted modular words");
    assert!(
        result
            .gaps()
            .iter()
            .any(|gap| gap.slug() == "go_target_modular_word_unsupported"),
        "expected go_target_modular_word_unsupported, got {:?}",
        result.gaps()
    );
}

// TS4-1: the TypeScript emitter supports `modulus<W>` with per-width wrapping,
// so the coverage classifier no longer lists modular words as a TS rejection.
#[test]
fn typescript_classifier_accepts_modular_words() {
    let hir = empty_program();
    let mut types = TypeTable::default();
    types.intern(Type::ModularWord(NumericWidth::U32));

    let result = classify_hir_coverage(HirCoverageTarget::TypeScript, &hir, &types);
    assert!(
        result.is_capable(),
        "TypeScript must accept modular words after TS4-1, got {:?}",
        result.gaps()
    );
}

#[test]
fn rust_classifier_capable_on_empty() {
    let hir = empty_program();
    let types = TypeTable::default();
    let result = classify_hir_coverage(HirCoverageTarget::Rust, &hir, &types);
    assert!(result.is_capable());
}

#[test]
fn go_classifier_capable_on_empty() {
    let hir = empty_program();
    let types = TypeTable::default();
    let result = classify_hir_coverage(HirCoverageTarget::Go, &hir, &types);
    assert!(result.is_capable());
}

#[test]
fn hir_coverage_target_name_rust() {
    assert_eq!(HirCoverageTarget::Rust.name(), "rust");
}

#[test]
fn hir_coverage_target_name_go() {
    assert_eq!(HirCoverageTarget::Go.name(), "go");
}

#[test]
fn hir_coverage_target_name_typescript() {
    assert_eq!(HirCoverageTarget::TypeScript.name(), "ts");
}

#[test]
fn hir_coverage_target_name_faber() {
    assert_eq!(HirCoverageTarget::Faber.name(), "faber");
}

#[test]
fn from_target_maps_application_targets() {
    assert_eq!(from_target(crate::codegen::Target::HirRust), Some(HirCoverageTarget::Rust));
    assert_eq!(from_target(crate::codegen::Target::HirGo), Some(HirCoverageTarget::Go));
    assert_eq!(
        from_target(crate::codegen::Target::HirTypeScript),
        Some(HirCoverageTarget::TypeScript)
    );
    assert_eq!(from_target(crate::codegen::Target::HirFaber), Some(HirCoverageTarget::Faber));
}

#[test]
fn from_target_excludes_mir_targets() {
    assert_eq!(from_target(crate::codegen::Target::MirWasm), None);
    assert_eq!(from_target(crate::codegen::Target::MirLlvm), None);
    assert_eq!(from_target(crate::codegen::Target::MirMetal), None);
    assert_eq!(from_target(crate::codegen::Target::MirWgsl), None);
    assert_eq!(from_target(crate::codegen::Target::MirSexp), None);
}

#[test]
fn all_targets_listed() {
    assert_eq!(HirCoverageTarget::ALL.len(), 4);
}

#[test]
fn rust_accepts_matrix() {
    // Rust codegen emits matrix operations (see matrix_test.rs) and
    // neither reject_rust_unsupported_types nor classify_rust rejects
    // matrix types. The classifier must report Capable.
    let hir = empty_program();
    let mut types = TypeTable::default();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim = types.intern_index(IndexExpr::Literal(2));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim, dim]));
    let _matrix = types.matrix_with_shape(elem, shape);
    let result = classify_hir_coverage(HirCoverageTarget::Rust, &hir, &types);
    assert!(result.is_capable(), "Rust target must accept matrix types");
}

#[test]
fn rust_rejects_f16_with_stable_slug() {
    let hir = empty_program();
    let mut types = TypeTable::default();
    let _ = types.sized_numeric(Primitive::Fractus, NumericWidth::F16);
    let result = classify_hir_coverage(HirCoverageTarget::Rust, &hir, &types);
    assert!(!result.is_capable());
    let slugs: Vec<_> = result
        .gaps()
        .iter()
        .map(radix_mir::CapabilityGap::slug)
        .collect();
    assert!(
        slugs.contains(&"rust_target_fractus_f16_unsupported"),
        "expected f16 gap slug, got {slugs:?}"
    );
}

#[test]
fn go_rejects_ad_with_stable_slug() {
    let mut interner = Interner::new();
    let name = interner.intern("main");
    let route = interner.intern("runtime:test");
    let ad_expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Ad { route, opener: None },
        ty: None,
        span: Span::default(),
    };
    let body = HirBlock {
        statements: vec![HirStatement { id: HirId(2), kind: HirStatementKind::Expr(ad_expr), span: Span::default() }],
        expr: None,
        span: Span::default(),
        breakable: None,
        end: None,
    };
    let hir = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(3),
            def_id: DefId(1),
            kind: HirItemKind::Function(Box::new(HirFunction {
                name,
                type_params: Vec::new(),
                params: Vec::new(),
                cli_args: None,
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
            })),
            span: Span::default(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let types = TypeTable::default();
    let result = classify_hir_coverage(HirCoverageTarget::Go, &hir, &types);
    assert!(!result.is_capable());
    let slugs: Vec<_> = result
        .gaps()
        .iter()
        .map(radix_mir::CapabilityGap::slug)
        .collect();
    assert!(
        slugs.contains(&"go_target_ad_unsupported"),
        "expected ad gap slug, got {slugs:?}"
    );
}
