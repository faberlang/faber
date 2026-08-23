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
fn summa_expression_lowers_to_hir_then_declines_with_catalog_issue() {
    let result = analyze_source(
        "functio t(numerus a) → numerus { summa ex a apud [i] filum f fixum s { s } }",
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
    } = &expression.kind
    else {
        panic!("expected HIR summa node");
    };
    assert!(matches!(source.kind, HirExpressionKind::Path(_)));
    assert_eq!(coordinates.len(), 1);
    assert!(filum.is_some());
    assert_eq!(result.interner.resolve(binding.1), "s");
    assert_eq!(term_body.statements.len(), 1);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        crate::semantic_spec(diagnostic.kind).code == "SEM008"
            && diagnostic.issue() == Some("summa_expression_unsupported")
    }));
}
