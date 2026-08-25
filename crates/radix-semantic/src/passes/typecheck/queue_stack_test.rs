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

#[test]
fn queue_and_stack_methods_typecheck_with_empty_pop_union() {
    crate::test_support::assert_accepts(
        r#"functio queue_ops(queue<numerus> q) → numerus ∪ nihil {
    varia queue<numerus> values ← q
    values.appende(1)
    fixum numerus ∪ nihil item ← values.decapita()
    fixum numerus size ← values.longitudo()
    redde item
}

functio stack_ops(stack<textus> s) → textus ∪ nihil {
    varia stack<textus> values ← s
    values.appende("top")
    fixum textus ∪ nihil item ← values.detrahe()
    fixum numerus size ← values.longitudo()
    nota size
    redde item
}"#,
    );
}

#[test]
fn bounded_queue_and_stack_push_use_alternate_exit_but_pop_stays_nullable() {
    for source in [
        r"functio queue_push(queue<numerus, 2> q) → vacuum {
    varia queue<numerus, 2> values ← q
    values.appende(1)
}",
        r##"functio stack_push(stack<textus, 2> s) → vacuum {
    varia stack<textus, 2> values ← s
    values.appende("top")
}"##,
    ] {
        assert_rejects_with_issue(source, "failable_call_requires_handler");
    }
    crate::test_support::assert_accepts(
        r"functio queue_pop(queue<numerus, 2> q) → numerus ∪ nihil {
    varia queue<numerus, 2> values ← q
    redde values.decapita()
}

functio stack_pop(stack<textus, 2> s) → textus ∪ nihil {
    varia stack<textus, 2> values ← s
    redde values.detrahe()
}",
    );
}

#[test]
fn queue_and_stack_for_from_bind_elements_in_their_access_orders() {
    crate::test_support::assert_accepts(
        r#"functio queue_iter(queue<numerus> q) → vacuum {
    itera ex q fixum item {
        nota item
    }
}

functio stack_iter(stack<textus, 2> s) → vacuum {
    itera ex s fixum item {
        nota item
    }
}"#,
    );
}

#[test]
fn queue_and_stack_empty_pop_recover_with_coalesce_without_a_handler() {
    crate::test_support::assert_accepts(
        r#"functio queue_recover(queue<numerus, 2> q) → numerus {
    redde q.decapita() vel 0
}

functio stack_recover(stack<textus, 2> s) → textus {
    redde s.detrahe() vel "empty"
}"#,
    );
}
