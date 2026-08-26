//! DB-U3: loop-pattern binders reuse binding-position HIR expansion.

use crate::test_support::{self, AnalyzedFixture};
use radix_hir::{HirBlock, HirExpression, HirExpressionKind, HirItemKind, HirStatementKind};

#[test]
fn rejects_de_var_reproduction_at_binding_site() {
    test_support::assert_rejects_with_issue(
        r"
functio bad(lista<numerus> ys) → vacuum {
    itera de ys varia y {
        y ← y + 5
    }
}
",
        "itera_de_requires_fixum",
    );
}

#[test]
fn accepts_de_const_bindings_for_array_index_and_map_key() {
    let _ = test_support::analyze_ok(
        r"
functio arrays(lista<numerus> ys) → vacuum {
    itera de ys fixum index {
        nota index
    }
}

functio maps(tabula<textus, numerus> values) → vacuum {
    itera de values fixum key {
        nota key
    }
}
",
    );
}

#[test]
fn accepts_ex_var_element_binding() {
    let _ = test_support::analyze_ok(
        r"
functio values(lista<numerus> ys) → vacuum {
    itera ex ys varia value {
        value ← value + 5
    }
}
",
    );
}

fn function_body(unit: &AnalyzedFixture) -> &HirBlock {
    unit.hir
        .as_ref()
        .expect("hir")
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Function(function) => function.body.as_ref(),
            _ => None,
        })
        .expect("function body")
}

fn expression_shape(expr: &HirExpression, unit: &AnalyzedFixture) -> String {
    match &expr.kind {
        HirExpressionKind::Path(_) => "path".to_owned(),
        HirExpressionKind::Literal(literal) => format!("literal:{literal:?}"),
        HirExpressionKind::Index(object, index) => {
            format!(
                "index({}, {})",
                expression_shape(object, unit),
                expression_shape(index, unit)
            )
        }
        HirExpressionKind::Field(object, name) => {
            format!(
                "field({}, {})",
                expression_shape(object, unit),
                unit.interner.resolve(*name)
            )
        }
        HirExpressionKind::Tuple(elements, _) => {
            let elements = elements
                .iter()
                .map(|element| expression_shape(element, unit))
                .collect::<Vec<_>>()
                .join(", ");
            format!("tuple({elements})")
        }
        other => format!("other:{other:?}"),
    }
}

fn local_shapes(block: &HirBlock, unit: &AnalyzedFixture) -> Vec<(bool, String)> {
    block
        .statements
        .iter()
        .filter_map(|statement| {
            let HirStatementKind::Local(local) = &statement.kind else {
                return None;
            };
            Some((
                local.mutable,
                local
                    .init
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), |init| expression_shape(init, unit)),
            ))
        })
        .collect()
}

fn loop_body(unit: &AnalyzedFixture) -> &HirBlock {
    let statement = function_body(unit)
        .statements
        .iter()
        .find(|statement| matches!(&statement.kind, HirStatementKind::Expr(_)))
        .expect("loop statement");
    let HirStatementKind::Expr(expr) = &statement.kind else {
        unreachable!("loop statement is an expression statement");
    };
    let HirExpressionKind::Itera(_, _, _, _, _, body) = &expr.kind else {
        panic!("expected itera HIR, got {:?}", expr.kind);
    };
    body
}

#[test]
fn array_loop_binder_matches_binding_position_reads() {
    let binding = test_support::analyze_ok(
        r"
functio bind(lista<numerus> pair) → vacuum {
    fixum [a, b] ← pair
    tacet
}
",
    );
    let looping = test_support::analyze_ok(
        r"
functio loopum(lista<lista<numerus>> pairs) → vacuum {
    itera ex pairs fixum [a, b] {
        tacet
    }
}
",
    );

    assert_eq!(
        local_shapes(function_body(&binding), &binding),
        local_shapes(loop_body(&looping), &looping),
        "array binder must lower to the same per-element index reads",
    );
}

#[test]
fn object_loop_binder_matches_binding_position_fields_and_aliases() {
    let binding = test_support::analyze_ok(
        r"
genus Point {
    numerus x = 0
    numerus y = 0
}

functio bind(Point point) → vacuum {
    fixum {x ut left, y} ← point
    tacet
}
",
    );
    let looping = test_support::analyze_ok(
        r"
genus Point {
    numerus x = 0
    numerus y = 0
}

functio loopum(lista<Point> points) → vacuum {
    itera ex points fixum {x ut left, y} {
        tacet
    }
}
",
    );

    assert_eq!(
        local_shapes(function_body(&binding), &binding),
        local_shapes(loop_body(&looping), &looping),
        "object binder must lower to the same per-element field reads",
    );
}

#[test]
fn labeled_tuple_loop_binder_binds_by_label() {
    let binding = test_support::analyze_ok(
        r"
functio bind(iuncta<gx: numerus, gw: numerus> pair) → vacuum {
    fixum {gx} ← pair
    tacet
}
",
    );
    let looping = test_support::analyze_ok(
        r"
functio loopum() → vacuum {
    fixum lista<iuncta<gx: numerus, gw: numerus>> xs ← [
        iuncta<numerus, numerus> [1, 2],
        iuncta<numerus, numerus> [3, 4]
    ]
    itera ex xs fixum {gx} {
        tacet
    }
}
",
    );

    assert_eq!(
        local_shapes(function_body(&binding), &binding),
        local_shapes(loop_body(&looping), &looping),
        "by-label binder over a labeled-tuple list must lower to the same \
         per-element index reads as binding position",
    );
}

#[test]
fn loop_pattern_failure_uses_binding_position_issue() {
    test_support::assert_rejects_with_issue(
        r"
functio bad(iuncta<numerus> pair) → vacuum {
    fixum [a, b] ← pair
    tacet
}
",
        "tuple_index_out_of_bounds",
    );
    test_support::assert_rejects_with_issue(
        r"
functio bad(lista<iuncta<numerus>> pairs) → vacuum {
    itera ex pairs fixum [a, b] {
        tacet
    }
}
",
        "tuple_index_out_of_bounds",
    );
}

#[test]
fn mutable_loop_pattern_locals_inherit_binding_mutability() {
    let looping = test_support::analyze_ok(
        r"
functio loopum(lista<lista<numerus>> pairs) → vacuum {
    itera ex pairs varia [_, head, ceteri tail] {
        tacet
    }
}
",
    );

    assert_eq!(
        local_shapes(loop_body(&looping), &looping),
        vec![
            (true, "index(path, literal:Int(1))".to_owned()),
            (true, "path".to_owned()),
        ],
        "array holes and rest must retain binding-form mutability",
    );
}
