//! C1 pins for queue and stack declaration surfaces.
//!
//! Runtime receiver behavior, including the settled bounded-push `⇥` exit and
//! empty-pop `T ∪ nihil` result, belongs to C2/C4. These tests only prove that
//! the type declarations lower to distinct unbounded/bounded semantic kinds.

use crate::test_support::{analyze_ok, assert_rejects_with_issue};
use crate::{IndexExpr, Type};
use radix_hir::HirItemKind;

#[test]
fn queue_and_stack_declarations_lower_to_distinct_kinds() {
    let unit = analyze_ok("functio f(queue<numerus> q, stack<textus, 4> s) → vacuum {}");
    let hir = unit.hir.as_ref().expect("HIR");
    let item = hir.items.first().expect("function item");
    let HirItemKind::Function(function) = &item.kind else {
        panic!("expected function item, got {:?}", item.kind);
    };
    assert_eq!(function.params.len(), 2);

    assert!(matches!(
        unit.types.get(function.params[0].ty),
        Type::Queue(element)
            if unit.types.equals(*element, unit.types.primitive(crate::Primitive::Numerus))
    ));
    assert!(matches!(
        unit.types.get(function.params[1].ty),
        Type::BoundedStack { element, capacity }
            if unit.types.equals(*element, unit.types.primitive(crate::Primitive::Textus))
                && matches!(unit.types.get_index(*capacity), IndexExpr::Literal(4))
    ));
}

#[test]
fn queue_and_stack_accept_unbounded_or_bounded_arity_only() {
    assert_rejects_with_issue("typus BadQueue = queue", "queue_arity");
    assert_rejects_with_issue("typus BadStack = stack", "stack_arity");
}
