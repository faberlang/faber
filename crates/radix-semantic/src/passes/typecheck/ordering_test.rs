//! D1.4 ordering contract: the `@ radix contract "ordering"` mark, built-in
//! conformance, glyph dispatch through `compare`, and tuple order.
//!
//! Fixtures declare the marked contract locally (the shape norma ships) and
//! use latin spellings, because the radix unit-test harness compiles bare
//! source without the en-locale lex pack.

use radix_hir::{HirBinOp, HirExpressionKind, HirStatementKind};

use crate::test_support;

const ORDERABLE: &str = "\
@ radix contract \"ordering\"
implendum Orderable<T> {
    functio compare(T other) → numerus
}

genus Persona implet Orderable<Persona> {
    varia numerus anni

    functio compare(Persona other) → numerus { redde ego.anni - other.anni }
}

genus Lapis {
    varia numerus massa
}

functio maior<T implet Orderable<T>>(T a, T b) → T {
    si a ≻ b { redde a }
    redde b
}

";

fn with_orderable(body: &str) -> String {
    format!("{ORDERABLE}{body}")
}

#[test]
fn genus_glyphs_order_through_compare() {
    test_support::assert_accepts(&with_orderable(
        "incipit {\n\
         sit Persona a ← Persona { anni = 3 }\n\
         sit Persona b ← Persona { anni = 5 }\n\
         sit bivalens x ← a ≺ b\n\
         sit bivalens y ← a ≥ b\n\
         }\n",
    ));
}

#[test]
fn genus_glyph_is_rewritten_to_a_compare_call() {
    let result = test_support::analyze_ok(&with_orderable(
        "incipit {\n\
         sit Persona a ← Persona { anni = 3 }\n\
         sit Persona b ← Persona { anni = 5 }\n\
         sit bivalens x ← a ≺ b\n\
         }\n",
    ));
    let hir = result.result.hir.as_ref().expect("hir");
    let entry = hir.entry.as_ref().expect("entry");
    let rewritten = entry.statements.iter().find_map(|stmt| match &stmt.kind {
        HirStatementKind::Local(local) => match &local.init.as_ref()?.kind {
            HirExpressionKind::Binary(HirBinOp::Lt, lhs, rhs) => Some((lhs.clone(), rhs.clone())),
            _ => None,
        },
        _ => None,
    });
    let (lhs, rhs) = rewritten.expect("a ≺ b stays a comparison");
    let HirExpressionKind::MethodCall(_, method, _, args, _) = &lhs.kind else {
        panic!("lhs should be the compare call, got {:?}", lhs.kind);
    };
    assert_eq!(result.interner.resolve(*method), "compare");
    assert_eq!(args.len(), 1);
    assert!(matches!(
        rhs.kind,
        HirExpressionKind::Literal(radix_hir::HirLiteral::Int(0))
    ));
}

#[test]
fn genus_without_the_contract_has_no_glyphs() {
    let errors = test_support::compile_and_get_diagnostics(&with_orderable(
        "incipit {\n\
         sit Lapis a ← Lapis { massa = 3 }\n\
         sit Lapis b ← Lapis { massa = 5 }\n\
         sit bivalens x ← a ≺ b\n\
         }\n",
    ));
    assert!(
        !errors.is_empty(),
        "ordering a genus without Orderable must fail"
    );
}

#[test]
fn bounded_parameter_glyphs_are_admitted() {
    test_support::assert_accepts(ORDERABLE);
}

#[test]
fn unbounded_parameter_has_no_glyphs() {
    let errors = test_support::compile_and_get_diagnostics(
        "functio maior<T>(T a, T b) → T {\n    si a ≻ b { redde a }\n    redde b\n}\n",
    );
    assert!(!errors.is_empty(), "an unbounded T must not order");
}

#[test]
fn builtins_satisfy_the_bound_without_implet() {
    test_support::assert_accepts(&with_orderable(
        "incipit {\n\
         sit numerus n ← maior(3, 9)\n\
         sit fractus f ← maior(1.5, 2.5)\n\
         sit textus t ← maior(\"pirum\", \"malum\")\n\
         sit Persona p ← maior(Persona { anni = 3 }, Persona { anni = 5 })\n\
         }\n",
    ));
}

#[test]
fn genus_without_implet_does_not_satisfy_the_bound() {
    test_support::assert_rejects_with_issue(
        &with_orderable(
            "incipit {\n\
             sit Lapis l ← maior(Lapis { massa = 3 }, Lapis { massa = 5 })\n\
             }\n",
        ),
        "generic_type_arg_bound",
    );
}

#[test]
fn bivalens_does_not_conform() {
    test_support::assert_rejects_with_issue(
        &with_orderable("incipit {\n sit bivalens b ← maior(verum, falsum)\n}\n"),
        "generic_type_arg_bound",
    );
}

#[test]
fn tuples_order_lexicographically_and_satisfy_the_bound() {
    test_support::assert_accepts(&with_orderable(
        "incipit {\n\
         sit bivalens x ← iuncta<numerus, textus>[1, \"b\"] ≺ iuncta<numerus, textus>[1, \"c\"]\n\
         sit iuncta<numerus, textus> m ← maior(iuncta<numerus, textus>[1, \"b\"], iuncta<numerus, textus>[2, \"a\"])\n\
         }\n",
    ));
}

#[test]
fn tuple_with_a_non_orderable_element_does_not_order() {
    let errors = test_support::compile_and_get_diagnostics(&with_orderable(
        "incipit {\n\
         sit bivalens x ← iuncta<numerus, bivalens>[1, verum] ≺ iuncta<numerus, bivalens>[1, falsum]\n\
         }\n",
    ));
    assert!(
        !errors.is_empty(),
        "a tuple with a bivalens element must not order"
    );
}

#[test]
fn contract_ordered_operands_must_share_a_type() {
    test_support::assert_rejects_with_issue(
        &with_orderable(
            "incipit {\n\
             sit bivalens x ← iuncta<numerus, textus>[1, \"b\"] ≺ iuncta<textus, numerus>[\"b\", 1]\n\
             }\n",
        ),
        "ordering_operands_differ",
    );
}

#[test]
fn ordering_mark_requires_the_compare_shape() {
    test_support::assert_rejects_with_issue(
        "@ radix contract \"ordering\"\nimplendum Orderable<T> {\n    functio compare(T other) → bivalens\n}\n",
        "radix_contract_ordering_shape",
    );
}

#[test]
fn unknown_contract_role_is_rejected() {
    test_support::assert_analysis_rejects_with_issue(
        "@ radix contract \"equality\"\nimplendum Aequabile<T> {\n    functio equals(T other) → bivalens\n}\n",
        "radix_contract_unknown_role",
    );
}

#[test]
fn contract_role_is_only_valid_on_an_implendum() {
    test_support::assert_analysis_rejects_with_issue(
        "@ radix contract \"ordering\"\ngenus Lapis {\n    varia numerus massa\n}\n",
        "radix_contract_placement",
    );
}
