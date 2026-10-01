//! SFR-1 (summa-filum-reduction) pins: the `.summa()` tensor receiver stays
//! an ordinary intrinsic method. Anchored at `examples/training/device-summa*`
//! (both exemplars call `a.summa()` / `medius.summa()` as a plain reduction
//! method). SFR-2+ must not move `summa` out of this ordinary-method row when
//! the head-position `summa ex` spelling is admitted.

use super::air_tensor_intrinsics::is_allowed_tensor_intrinsic;
use crate::test_support::analyze_source;
use radix_hir::{HirExpressionKind, HirItemKind, HirStatementKind};

#[test]
fn summa_receiver_remains_ordinary_tensor_intrinsic() {
    assert!(
        is_allowed_tensor_intrinsic("summa"),
        "`.summa()` receiver (examples/training/device-summa*) must stay an \
         admitted ordinary tensor intrinsic"
    );
}

#[test]
fn summa_expression_lowers_to_hir_and_admits_sequential() {
    // SFR-3 replaces the SFR-2 fail-closed gate with the real admission laws:
    // the sequential (filum-omitted) spelling over a tensor now typechecks.
    let result = analyze_source(
        "functio t(tensor<f32, [8]> a) → vacuum { summa ex a apud [i] fixum s { redde s } }",
    );
    assert!(
        result.is_ok(),
        "sequential summa should admit, diagnostics: {:?}",
        result.diagnostics
    );
    let hir = result
        .hir
        .as_ref()
        .expect("summa source should lower to HIR");
    let function = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Function(function) if result.interner.resolve(function.name) == "t" => {
                Some(function)
            }
            _ => None,
        })
        .expect("summa function");
    let body = function.body.as_ref().expect("function body");
    let HirStatementKind::Expr(expression) = &body.statements[0].kind else {
        panic!("expected summa expression statement");
    };
    let HirExpressionKind::Summa {
        source,
        coordinates,
        filum,
        binding,
        body: term_body,
        ..
    } = &expression.kind
    else {
        panic!("expected HIR summa node");
    };
    assert!(matches!(source.kind, HirExpressionKind::Path(_)));
    assert_eq!(coordinates.len(), 1);
    assert!(filum.is_none(), "sequential spelling has no filum binder");
    assert_eq!(result.interner.resolve(binding.1), "s");
    assert_eq!(term_body.statements.len(), 1);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.issue() != Some("summa_expression_unsupported")),
        "the SFR-2 fail-closed gate must not fire after SFR-3 admission"
    );
}
