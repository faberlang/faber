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
    let unit = analyze_ok("functio f(queue<numerus> q, stack<textus, magnitudo 4> s) → vacuum {}");
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
        r#"functio queue_ops(varia queue<numerus> q) → numerus ∪ nihil {
    varia queue<numerus> values ← q
    values.appende(1)
    sit numerus ∪ nihil item ← values.decapita()
    sit numerus size ← values.longitudo()
    redde item
}

functio stack_ops(varia stack<textus> s) → textus ∪ nihil {
    varia stack<textus> values ← s
    values.appende("top")
    sit textus ∪ nihil item ← values.detrahe()
    sit numerus size ← values.longitudo()
    nota size
    redde item
}"#,
    );
}

#[test]
fn bounded_queue_and_stack_push_use_alternate_exit_but_pop_stays_nullable() {
    for source in [
        r"functio queue_push(varia queue<numerus, magnitudo 2> q) → vacuum {
    varia queue<numerus, magnitudo 2> values ← q
    values.appende(1)
}",
        r#"functio stack_push(varia stack<textus, magnitudo 2> s) → vacuum {
    varia stack<textus, magnitudo 2> values ← s
    values.appende("top")
}"#,
    ] {
        assert_rejects_with_issue(source, "failable_call_requires_handler");
    }
    crate::test_support::assert_accepts(
        r"functio queue_pop(varia queue<numerus, magnitudo 2> q) → numerus ∪ nihil {
    varia queue<numerus, magnitudo 2> values ← q
    redde values.decapita()
}

functio stack_pop(varia stack<textus, magnitudo 2> s) → textus ∪ nihil {
    varia stack<textus, magnitudo 2> values ← s
    redde values.detrahe()
}",
    );
}

#[test]
fn queue_and_stack_for_from_bind_elements_in_their_access_orders() {
    crate::test_support::assert_accepts(
        r"functio queue_iter(queue<numerus> q) → vacuum {
    itera ex q sit item {
        nota item
    }
}

functio stack_iter(stack<textus, magnitudo 2> s) → vacuum {
    itera ex s sit item {
        nota item
    }
}",
    );
}

#[test]
fn queue_and_stack_empty_pop_recover_with_coalesce_without_a_handler() {
    crate::test_support::assert_accepts(
        r#"functio queue_recover(varia queue<numerus, magnitudo 2> q) → numerus {
    redde q.decapita() vel 0
}

functio stack_recover(varia stack<textus, magnitudo 2> s) → textus {
    redde s.detrahe() vel "empty"
}"#,
    );
}

#[test]
fn queue_stack_arithmetic_accepts_all_ops_and_broadcasts_both_sides() {
    crate::test_support::assert_accepts(
        r"functio queue_add(queue<numerus> a, queue<numerus> b) → queue<numerus> {
    redde a + b
}

functio queue_sub(queue<numerus> a, numerus scalar) → queue<numerus> {
    redde a - scalar
}

functio queue_scale(queue<numerus> a, numerus scalar) → queue<numerus> {
    redde a * scalar
}

functio stack_div(numerus scalar, stack<numerus> b) → stack<numerus> {
    redde scalar / b
}

functio stack_hadamard(stack<numerus> a, stack<numerus> b) → stack<numerus> {
    redde a ⊙ b
}

functio queue_hadamard_div(queue<numerus> a, queue<numerus> b) → queue<numerus> {
    redde a ⊘ b
}",
    );
}

#[test]
fn queue_stack_pair_multiply_and_divide_require_the_glyphs() {
    for (source, issue) in [
        (
            "functio f(queue<numerus> a, queue<numerus> b) → queue<numerus> { redde a * b }",
            "queue_stack_multiply_requires_glyph",
        ),
        (
            "functio f(queue<numerus> a, queue<numerus> b) → queue<numerus> { redde a / b }",
            "queue_stack_divide_requires_glyph",
        ),
        (
            "functio f(stack<numerus> a, stack<numerus> b) → stack<numerus> { redde a * b }",
            "queue_stack_multiply_requires_glyph",
        ),
        (
            "functio f(stack<numerus> a, stack<numerus> b) → stack<numerus> { redde a / b }",
            "queue_stack_divide_requires_glyph",
        ),
        (
            "functio f(queue<numerus, magnitudo 4> a, queue<numerus, magnitudo 2> b) → queue<numerus, magnitudo 2> { redde a * b }",
            "queue_stack_multiply_requires_glyph",
        ),
        (
            "functio f(stack<numerus, magnitudo 4> a, stack<numerus, magnitudo 4> b) → stack<numerus, magnitudo 4> { redde a / b }",
            "queue_stack_divide_requires_glyph",
        ),
    ] {
        crate::test_support::assert_rejects_with_issue(source, issue);
    }
}

#[test]
fn queue_stack_hadamard_keeps_kind_and_minimizes_bounded_capacity() {
    let unit = crate::test_support::analyze_ok(
        r"functio queue_bounded(queue<numerus, magnitudo 8> a, queue<numerus, magnitudo 3> b) → queue<numerus, magnitudo 3> {
    redde a ⊙ b
}

functio stack_bounded(stack<numerus, magnitudo 2> a, stack<numerus, magnitudo 7> b) → stack<numerus, magnitudo 2> {
    redde a ⊘ b
}",
    );
    let hir = unit.hir.as_ref().expect("HIR");
    let bounded_queue = match &hir.items[0].kind {
        radix_hir::HirItemKind::Function(function) => function.ret_ty.expect("queue return type"),
        other => panic!("expected queue function, got {other:?}"),
    };
    let bounded_stack = match &hir.items[1].kind {
        radix_hir::HirItemKind::Function(function) => function.ret_ty.expect("stack return type"),
        other => panic!("expected stack function, got {other:?}"),
    };
    assert!(matches!(
        unit.types.get(bounded_queue),
        Type::BoundedQueue { .. }
    ));
    assert!(matches!(
        unit.types.get(bounded_stack),
        Type::BoundedStack { .. }
    ));
}

#[test]
fn queue_stack_hadamard_rejects_mixed_kinds_scalars_and_non_numeric_elements() {
    for (source, issue) in [
        (
            "functio f(queue<numerus> q, stack<numerus> s) → queue<numerus> { redde q ⊙ s }",
            "glyph_operands_unsupported",
        ),
        (
            "functio f(queue<numerus> q) → queue<numerus> { redde q ⊙ 2 }",
            "glyph_operands_unsupported",
        ),
        (
            "functio f(queue<textus> q, queue<textus> r) → queue<textus> { redde q ⊙ r }",
            "glyph_op_numeric_element_required",
        ),
    ] {
        crate::test_support::assert_rejects_with_issue(source, issue);
    }
}

#[test]
fn queue_stack_arithmetic_preserves_kind_and_minimizes_bounded_capacity() {
    let unit = crate::test_support::analyze_ok(
        r"functio queue_bounded(queue<numerus, magnitudo 8> a, queue<numerus, magnitudo 3> b) → queue<numerus, magnitudo 3> {
    redde a + b
}

functio stack_bounded(stack<numerus, magnitudo 2> a, stack<numerus, magnitudo 7> b) → stack<numerus, magnitudo 2> {
    redde a ⊙ b
}",
    );
    let hir = unit.hir.as_ref().expect("HIR");
    let bounded_queue = match &hir.items[0].kind {
        radix_hir::HirItemKind::Function(function) => function.ret_ty.expect("queue return type"),
        other => panic!("expected queue function, got {other:?}"),
    };
    let bounded_stack = match &hir.items[1].kind {
        radix_hir::HirItemKind::Function(function) => function.ret_ty.expect("stack return type"),
        other => panic!("expected stack function, got {other:?}"),
    };
    assert!(matches!(
        unit.types.get(bounded_queue),
        Type::BoundedQueue { capacity, .. }
            if matches!(unit.types.get_index(*capacity), IndexExpr::Literal(3))
    ));
    assert!(matches!(
        unit.types.get(bounded_stack),
        Type::BoundedStack { capacity, .. }
            if matches!(unit.types.get_index(*capacity), IndexExpr::Literal(2))
    ));
}

#[test]
fn queue_stack_arithmetic_rejects_mixed_kinds_and_non_numeric_elements() {
    for source in [
        "functio mixed(queue<numerus> q, stack<numerus> s) → queue<numerus> { redde q + s }",
        "functio text_queue(queue<textus> q, queue<textus> r) → queue<textus> { redde q + r }",
    ] {
        crate::test_support::assert_rejects_with_issue(source, "numeric_queue_stack_operator_law");
    }
}

#[test]
fn queue_stack_arithmetic_diagnostic_is_localized_in_en_and_la() {
    let diagnostic = radix_diagnostics::Diagnostic::error("SEM011")
        .with_code("SEM011")
        .with_arg("issue", "numeric_queue_stack_operator_law")
        .with_arg("collection", "queue")
        .with_arg("operator", "+");
    for (locale, expected) in [
        (
            "en",
            "numeric queue operator + is elementwise; scalars broadcast on either side, queues must have equal length; mixed queue/stack operands are invalid; use ↦ for explicit width conversion",
        ),
        (
            "la",
            "operator queue numerica + per elementa agit; scalaria utrimque propagantur, queue parem longitudinem requirit; operanda queue/stack mixta invalida sunt; utere ↦ ad latitudinem convertendam",
        ),
    ] {
        let path = crate::locale::workspace_locale_pack_path(locale);
        let pack = crate::locale::LocalePack::from_toml_path(path).expect("locale pack");
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .expect("diagnostic template")
            .expect("SEM011 row");
        assert_eq!(rendered.message, expected, "locale {locale}");
    }
}
