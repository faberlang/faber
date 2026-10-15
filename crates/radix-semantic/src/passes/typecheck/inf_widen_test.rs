//! inf-wn: typecheck wraps a bounded integer that lands in an `inf` slot in an
//! explicit `HirExpressionKind::Widen` node (inf-delivery ruling 40, row 1).
//!
//! The wrapped operand keeps its REAL bounded type (it is no longer repainted
//! with the slot's `inf`), and the node itself is typed `inf`, so a backend
//! converts from the operand's type without a side table. Untyped constants
//! join by exact value and take no node; a value already typed `inf` takes none.

use super::test_support;
use crate::types::Type;
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{HirExpression, HirExpressionKind};
use radix_types::NumericWidth::{self, I8, I64, Inf, U8, U64};
use radix_types::TypeTable;
use radix_types::numeric_width::numerus_integer_width;

/// One `Widen` node: its operand's own width and the node's width.
#[derive(Debug, PartialEq, Eq)]
struct Widened {
    operand_width: Option<NumericWidth>,
    node_width: Option<NumericWidth>,
    /// The operand's expression shape, for the site assertions.
    operand: &'static str,
}

struct Widens<'a> {
    types: &'a TypeTable,
    found: Vec<Widened>,
    /// Every `Widen` the walk met had `from == operand.ty` and `to == node.ty`.
    consistent: bool,
}

fn shape(expr: &HirExpression) -> &'static str {
    match &expr.kind {
        HirExpressionKind::Path(_) => "path",
        HirExpressionKind::Call(..) => "call",
        HirExpressionKind::MethodCall(..) => "method",
        HirExpressionKind::Binary(..) => "binary",
        HirExpressionKind::Field(..) => "field",
        HirExpressionKind::Index(..) => "index",
        _ => "other",
    }
}

impl HirVisitor for Widens<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Widen { operand, from, to } = &expr.kind {
            self.consistent &= operand.ty == Some(*from) && expr.ty == Some(*to);
            let width = |ty| numerus_integer_width(self.types, ty);
            self.found.push(Widened {
                operand_width: operand.ty.and_then(width),
                node_width: expr.ty.and_then(width),
                operand: shape(operand),
            });
        }
        walk_expr(self, expr);
    }
}

fn widens_of(source: &str) -> Vec<Widened> {
    let fixture = test_support::analyze_ok_la(source);
    let hir = fixture.hir.as_ref().expect("hir");
    let mut visitor = Widens {
        types: &fixture.types,
        found: Vec::new(),
        consistent: true,
    };
    walk_program(&mut visitor, hir);
    assert!(visitor.consistent, "a Widen disagrees with its own types");
    visitor.found
}

fn widened(operand_width: NumericWidth, operand: &'static str) -> Widened {
    Widened {
        operand_width: Some(operand_width),
        node_width: Some(Inf),
        operand,
    }
}

// ---------------------------------------------------------------------------
// Every site kind inserts the node
// ---------------------------------------------------------------------------

#[test]
fn a_declaration_initializer_is_widened() {
    let source = "incipit {\nsit u64 u ← 5\nsit inf w ← u\nnota w\n}\n";
    assert_eq!(widens_of(source), vec![widened(U64, "path")]);
}

#[test]
fn an_assignment_is_widened() {
    let source = "incipit {\nsit u8 u ← 5\nvaria inf w ← 0\nw ← u\nnota w\n}\n";
    assert_eq!(widens_of(source), vec![widened(U8, "path")]);
}

#[test]
fn a_call_argument_is_widened() {
    let source = "functio take(inf x) → inf {\nredde x\n}\n\
        incipit {\nsit i8 s ← -5\nnota take(s)\n}\n";
    assert_eq!(widens_of(source), vec![widened(I8, "path")]);
}

#[test]
fn a_return_is_widened() {
    let source = "functio f(u64 u) → inf {\nredde u\n}\n";
    assert_eq!(widens_of(source), vec![widened(U64, "path")]);
}

#[test]
fn a_field_initializer_is_widened() {
    let source = "genus Capsa {\nsit inf a\n}\n\
        incipit {\nsit u64 u ← 5\nsit Capsa c ← Capsa { a = u }\nnota c.a\n}\n";
    assert_eq!(widens_of(source), vec![widened(U64, "path")]);
}

#[test]
fn array_elements_are_widened_one_by_one() {
    let source = "incipit {\nsit inf x ← 3\nsit u8 b ← 1\n\
        sit lista<inf> xs ← [b, x, b]\nnota xs\n}\n";
    assert_eq!(
        widens_of(source),
        vec![widened(U8, "path"), widened(U8, "path")]
    );
}

#[test]
fn a_call_result_is_widened_from_its_declared_return() {
    let source = "functio small() → u8 {\nredde 7\n}\n\
        incipit {\nsit inf w ← small()\nnota w\n}\n";
    assert_eq!(widens_of(source), vec![widened(U8, "call")]);
}

#[test]
fn a_method_call_result_is_widened() {
    let source = "incipit {\nsit lista<numerus> xs ← [1, 2]\n\
        sit inf n ← xs.longitudo()\nnota n\n}\n";
    let found = widens_of(source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].operand, "method");
    assert_eq!(found[0].node_width, Some(Inf));
    assert_ne!(found[0].operand_width, Some(Inf));
}

#[test]
fn a_field_read_and_an_index_read_are_widened() {
    let source = "genus Capsa {\nsit u64 a\n}\n\
        incipit {\nsit Capsa c ← Capsa { a = 5 }\nsit lista<u8> xs ← [1, 2]\n\
        sit inf p ← c.a\nsit inf q ← xs[0]\nnota p\nnota q\n}\n";
    assert_eq!(
        widens_of(source),
        vec![widened(U64, "field"), widened(U8, "index")]
    );
}

#[test]
fn arithmetic_over_bounded_operands_widens_its_own_result() {
    // Ruling 35: the cap stands, so `a * a` is an `i64` and the store into
    // the `inf` slot widens that result; the operands keep their types.
    let source = "functio f(i64 a) → inf {\nsit inf d ← a * a\nredde d\n}\n";
    assert_eq!(widens_of(source), vec![widened(I64, "binary")]);
}

// ---------------------------------------------------------------------------
// Negatives: no node when nothing widens
// ---------------------------------------------------------------------------

#[test]
fn a_value_already_typed_inf_takes_no_node() {
    let source = "functio id(inf a) → inf {\nsit inf b ← a\nredde b + a\n}\n\
        incipit {\nsit inf x ← 3\nsit lista<inf> xs ← [x, x]\nnota id(x)\nnota xs\n}\n";
    assert!(widens_of(source).is_empty());
}

#[test]
fn an_untyped_constant_takes_no_node() {
    let source = "incipit {\nsit inf a ← 5\nsit inf b ← -5\nsit inf c ← 2 ⇐ 70\n\
        sit lista<inf> xs ← [1, 2]\nnota a\nnota b\nnota c\nnota xs\n}\n";
    assert!(widens_of(source).is_empty());
}

#[test]
fn a_bounded_value_into_a_bounded_or_open_slot_takes_no_node() {
    let source = "incipit {\nsit u8 b ← 1\nsit u64 w ← b\nsit r ← b\n\
        nota w\nnota r\n}\n";
    assert!(widens_of(source).is_empty());
}

#[test]
fn an_inf_operand_joins_a_bounded_one_with_each_keeping_its_own_type() {
    // `x + b` is typed `inf` by the join; the bounded operand is not stamped,
    // so no store happens and no node is needed.
    let source = "incipit {\nsit inf x ← 3\nsit u8 b ← 1\nsit r ← x + b\nnota r\n}\n";
    assert!(widens_of(source).is_empty());
}

#[test]
fn the_operand_keeps_its_declared_type_not_the_slot() {
    let fixture =
        test_support::analyze_ok_la("incipit {\nsit u64 u ← 5\nsit inf w ← u\nnota w\n}\n");
    let hir = fixture.hir.as_ref().expect("hir");
    let mut seen = None;
    struct Probe<'a>(&'a TypeTable, &'a mut Option<Type>);
    impl HirVisitor for Probe<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Widen { operand, .. } = &expr.kind {
                *self.1 = operand.ty.map(|ty| self.0.get(ty).clone());
            }
            walk_expr(self, expr);
        }
    }
    walk_program(&mut Probe(&fixture.types, &mut seen), hir);
    assert!(
        matches!(seen, Some(Type::SizedNumeric(_, U64))),
        "operand type: {seen:?}"
    );
}
