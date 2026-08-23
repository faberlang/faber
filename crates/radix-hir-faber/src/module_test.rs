use super::*;
use crate::surface::TypeBindingKind;
use radix_hir::{HirBlock, HirExpression, HirId, HirLiteral, HirStatement, HirStatementKind};
use radix_lexer::Span;

struct TestSurface;

impl KeywordSurface for TestSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "keyword"
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &'static str {
        match kind {
            // Explicit arms for the tensor family so the new kind is not
            // silently swallowed; this stub emits "type" for every kind.
            #[allow(clippy::match_same_arms)]
            TypeBindingKind::TensorFamily(
                radix_types::TensorFamilyKind::Tensor
                | radix_types::TensorFamilyKind::Vector
                | radix_types::TensorFamilyKind::Matrix
                | radix_types::TensorFamilyKind::Sparsa
                | radix_types::TensorFamilyKind::Atomic
                | radix_types::TensorFamilyKind::Intervallum,
            ) => "type",
            _ => "type",
        }
    }
}

fn literal_int(id: u32, value: u64, span: Span) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: None,
        span,
    }
}

#[test]
fn emit_rejects_legacy_context_dependent_cede_hir() {
    let cede_span = Span { start: 10, end: 20 };
    let hir = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(
            vec![HirStatement {
                id: HirId(1),
                kind: HirStatementKind::Expr(HirExpression {
                    id: HirId(2),
                    kind: HirExpressionKind::Cede(Box::new(literal_int(
                        3,
                        1,
                        Span { start: 15, end: 16 },
                    ))),
                    ty: None,
                    span: cede_span,
                }),
                span: cede_span,
            }],
            None,
            Span { start: 0, end: 24 },
        )),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let types = TypeTable::new();
    let interner = Interner::new();

    let err = emit(
        &hir,
        &empty_presentation(),
        &types,
        &interner,
        &TestSurface,
        &FxHashMap::default(),
        &FxHashMap::default(),
    )
    .expect_err("legacy Cede HIR must fail closed before Faber emission");

    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "legacy_cede_reached_faber_codegen"),
        "missing legacy cede issue arg: {:?}",
        err.args
    );
}
