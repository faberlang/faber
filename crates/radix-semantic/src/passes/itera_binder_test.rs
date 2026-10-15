//! DB-U3: loop-pattern binders reuse binding-position HIR expansion.
//! NR-2: the Cartesian `itera ab` range product lowers to its own HIR node
//! with ordered ranges/binders and dedicated SEM010 product laws.

use crate::test_support::{self, AnalyzedFixture};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirItemKind, HirIteraMode, HirLiteral,
    HirStatementKind,
};

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
    itera de ys sit index {
        nota index
    }
}

functio maps(tabula<textus, numerus> values) → vacuum {
    itera de values sit key {
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
        HirExpressionKind::MethodCall(receiver, name, _, args, _) => {
            let args = args
                .iter()
                .map(|arg| expression_shape(&arg.expr, unit))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "call({}.{}, {args})",
                expression_shape(receiver, unit),
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
    let HirExpressionKind::Itera { body, .. } = &expr.kind else {
        panic!("expected itera HIR, got {:?}", expr.kind);
    };
    body
}

#[test]
fn array_loop_binder_matches_binding_position_reads() {
    let binding = test_support::analyze_ok(
        r"
functio bind(lista<numerus> pair) → vacuum {
    sit [a, b] ← pair
    tacet
}
",
    );
    let looping = test_support::analyze_ok(
        r"
functio loopum(lista<lista<numerus>> pairs) → vacuum {
    itera ex pairs sit [a, b] {
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
    varia numerus x = 0
    varia numerus y = 0
}

functio bind(Point point) → vacuum {
    sit {x ut left, y} ← point
    tacet
}
",
    );
    let looping = test_support::analyze_ok(
        r"
genus Point {
    varia numerus x = 0
    varia numerus y = 0
}

functio loopum(lista<Point> points) → vacuum {
    itera ex points sit {x ut left, y} {
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
    sit {gx} ← pair
    tacet
}
",
    );
    let looping = test_support::analyze_ok(
        r"
functio loopum() → vacuum {
    sit lista<iuncta<gx: numerus, gw: numerus>> xs ← [
        iuncta<numerus, numerus> [1, 2],
        iuncta<numerus, numerus> [3, 4]
    ]
    itera ex xs sit {gx} {
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
    sit [a, b] ← pair
    tacet
}
",
        "tuple_index_out_of_bounds",
    );
    test_support::assert_rejects_with_issue(
        r"
functio bad(lista<iuncta<numerus>> pairs) → vacuum {
    itera ex pairs sit [a, b] {
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
            // D14.1: the rest binds the tail after both authored positions.
            (true, "call(path.omissa, literal:Int(2))".to_owned()),
        ],
        "array holes and rest must retain binding-form mutability",
    );
}

// ---------------------------------------------------------------------------
// NR-2: Cartesian `itera ab` range product (ordered ranges, ordered binders)
// ---------------------------------------------------------------------------

/// Distinct literal range starts prove the range vector keeps source order.
fn intervallum_start(expr: &HirExpression) -> Option<u64> {
    let HirExpressionKind::Intervallum { start, .. } = &expr.kind else {
        return None;
    };
    let HirExpressionKind::Literal(HirLiteral::Int(value)) = &start.kind else {
        return None;
    };
    Some(*value)
}

fn range_product<'a>(
    unit: &'a AnalyzedFixture,
) -> (
    &'a Vec<HirExpression>,
    &'a Vec<(radix_hir::DefId, radix_lexer::Symbol)>,
    &'a HirBlock,
) {
    let body = function_body(unit);
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!(
            "expected expression statement, got: {:?}",
            body.statements[0]
        );
    };
    let HirExpressionKind::IteraRange {
        ranges,
        bindings,
        body,
        ..
    } = &expr.kind
    else {
        panic!("expected IteraRange, got: {:?}", expr.kind);
    };
    (ranges, bindings, body)
}

fn binder_names(
    bindings: &[(radix_hir::DefId, radix_lexer::Symbol)],
    unit: &AnalyzedFixture,
) -> Vec<String> {
    bindings
        .iter()
        .map(|(_, name)| unit.interner.resolve(*name).to_owned())
        .collect()
}

#[test]
fn n2_range_product_keeps_range_and_binder_order() {
    let unit = test_support::analyze_ok(
        r"
functio grid(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 1‥n sit i, j {
        nota i
    }
}
",
    );
    let (ranges, bindings, body) = range_product(&unit);
    assert_eq!(ranges.len(), 2, "N=2 product carries two ranges");
    assert_eq!(
        intervallum_start(&ranges[0]),
        Some(0),
        "first range is leftmost-outermost"
    );
    assert_eq!(intervallum_start(&ranges[1]), Some(1));
    assert_eq!(binder_names(bindings, &unit), ["i", "j"]);
    assert_eq!(
        body.statements.len(),
        1,
        "body lowers into the product node"
    );
}

#[test]
fn n3_range_product_keeps_range_and_binder_order() {
    let unit = test_support::analyze_ok(
        r"
functio cube(numerus m, numerus n, numerus k) → vacuum {
    itera ab 0‥m, 1‥n, 2‥k sit i, j, p {
        nota i
    }
}
",
    );
    let (ranges, bindings, _) = range_product(&unit);
    assert_eq!(ranges.len(), 3);
    assert_eq!(
        ranges.iter().map(intervallum_start).collect::<Vec<_>>(),
        [Some(0), Some(1), Some(2)],
        "range order is source order (leftmost-outermost)"
    );
    assert_eq!(binder_names(bindings, &unit), ["i", "j", "p"]);
}

#[test]
fn single_range_ab_stays_on_shared_itera_node() {
    let unit = test_support::analyze_ok(
        r"
functio line(numerus m) → vacuum {
    itera ab 0‥m sit i {
        nota i
    }
}
",
    );
    let body = function_body(&unit);
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected expression statement");
    };
    let HirExpressionKind::Itera { mode, iter, .. } = &expr.kind else {
        panic!(
            "N=1 ab must stay HirExpressionKind::Itera, got: {:?}",
            expr.kind
        );
    };
    assert_eq!(*mode, HirIteraMode::Ab);
    assert!(
        matches!(iter.kind, HirExpressionKind::Intervallum { .. }),
        "N=1 keeps its single range source"
    );
}

#[test]
fn range_product_rejects_source_binder_count_mismatch() {
    test_support::assert_rejects_with_issue(
        r"
functio short(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n sit i {
        nota i
    }
}
",
        "itera_range_source_binder_arity_mismatch",
    );
    let diagnostics = test_support::analyze_and_get_diagnostics(
        r"
functio short(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n sit i {
        nota i
    }
}
",
    );
    let arity = diagnostics
        .iter()
        .find(|err| {
            err.args.iter().any(|arg| {
                arg.name == "issue" && arg.value == "itera_range_source_binder_arity_mismatch"
            })
        })
        .expect("structured arity issue");
    assert!(
        arity
            .args
            .iter()
            .any(|arg| arg.name == "expected" && arg.value == "2")
            && arity
                .args
                .iter()
                .any(|arg| arg.name == "actual" && arg.value == "1"),
        "arity issue carries the counts: {:?}",
        arity.args
    );
}

#[test]
fn range_product_rejects_extent_binder_shadowing() {
    test_support::assert_rejects_with_issue(
        r"
functio shadow(numerus m, numerus k) → vacuum {
    itera ab 0‥m, 0‥k sit i, k {
        nota i
    }
}
",
        "itera_range_extent_binder_shadowing",
    );
}

#[test]
fn range_product_rejects_mixed_range_member() {
    test_support::assert_rejects_with_issue(
        r"
functio mixed(numerus m, numerus n) → vacuum {
    itera ab 0‥m, n sit i, j {
        nota i
    }
}
",
        "itera_range_mixed_source",
    );
}

#[test]
fn range_product_rejects_apud_clause_through_requires_ex() {
    // The product node carries no coordinates; `apud` requires `ex`
    // iteration (same law family as the single-source form).
    test_support::assert_rejects_with_issue(
        r"
functio coord(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n apud [c] sit i, j {
        nota i
    }
}
",
        "itera_apud_requires_ex",
    );
}

#[test]
fn range_product_never_reaches_lockstep_checker() {
    // The zip form keeps its own identity on the same count-mismatch shape:
    // `ex` stays lockstep, `ab` stays product — the routes never cross.
    test_support::assert_rejects_with_issue(
        r"
functio zip(lista<numerus> xs, lista<numerus> ys) → vacuum {
    itera ex xs, ys sit x {
        nota x
    }
}
",
        "itera_lockstep_source_binder_arity_mismatch",
    );
    let unit = test_support::analyze_ok(
        r"
functio zip(lista<numerus> xs, lista<numerus> ys) → vacuum {
    itera ex xs, ys sit x, y {
        nota x
    }
}
",
    );
    let body = function_body(&unit);
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected expression statement");
    };
    assert!(
        matches!(expr.kind, HirExpressionKind::IteraLockstep { .. }),
        "ex series keeps IteraLockstep, got: {:?}",
        expr.kind
    );
}

#[test]
fn visitor_walks_product_ranges_binders_and_body_in_order() {
    let unit = test_support::analyze_ok(
        r"
functio grid(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 1‥n sit i, j {
        nota i
    }
}
",
    );
    let body = function_body(&unit);
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected expression statement");
    };
    let mut trace = ProductWalkTrace {
        interner: Some(&unit.interner),
        events: Vec::new(),
    };
    trace.visit_expr(expr);
    assert_eq!(
        trace.events,
        ["range:0", "range:1", "def:i", "def:j", "block",],
        "ranges walk before binder defs (evaluation order), then the body"
    );
}

#[derive(Default)]
struct ProductWalkTrace<'a> {
    interner: Option<&'a radix_lexer::Interner>,
    events: Vec<&'static str>,
}

impl HirVisitor for ProductWalkTrace<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Intervallum { start, .. } = &expr.kind {
            if let HirExpressionKind::Literal(HirLiteral::Int(value)) = &start.kind {
                self.events.push(match *value {
                    0 => "range:0",
                    1 => "range:1",
                    2 => "range:2",
                    _ => "range:?",
                });
            }
        }
        walk_expr(self, expr);
    }

    fn visit_def(&mut self, _def_id: radix_hir::DefId, name: radix_lexer::Symbol) {
        let spelling = self
            .interner
            .map(|interner| interner.resolve(name))
            .unwrap_or("?");
        self.events.push(match spelling {
            "i" => "def:i",
            "j" => "def:j",
            "p" => "def:p",
            _ => "def:?",
        });
    }

    fn visit_block(&mut self, block: &HirBlock) {
        self.events.push("block");
        radix_hir::visit::walk_block(self, block);
    }
}

#[test]
fn accepts_filum_lane_binder_on_single_source_itera() {
    // BUG-4D4D0AE9 (K2): the lane binder lowers as declared metadata on the
    // single-source `Itera` node; the runner keeps plain-loop semantics.
    let _ = test_support::analyze_ok(
        r"
functio walk(lista<numerus> ys) → vacuum {
    itera ex ys apud [i] filum f sit y {
        nota i, y
    }
}
",
    );
}

#[test]
fn declines_filum_clause_on_lockstep_series() {
    test_support::assert_rejects_with_issue(
        r"
functio zip_walk(lista<numerus> ks, lista<numerus> vs) → vacuum {
    itera ex ks, vs apud [i] filum f sit k, v {
        nota k, v
    }
}
",
        "itera_filum_requires_single_source",
    );
}

#[test]
fn declines_filum_clause_on_range_product() {
    test_support::assert_rejects_with_issue(
        r"
functio grid_walk(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n filum f sit r, c {
        nota r, c
    }
}
",
        "itera_filum_requires_single_source",
    );
}
