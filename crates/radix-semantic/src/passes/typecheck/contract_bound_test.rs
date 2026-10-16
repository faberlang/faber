//! D1.1/D1.2 contract bounds and generic `implet` (operator default F1:
//! contracts are generic and a bound is written in full, `T implet C<T>`).
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use radix_diagnostics::DiagnosticArg;

use crate::test_support;

const CONTRACT: &str = "\
implendum Comparabile<T> {
    functio confer(T alius) → numerus
}

genus Arca implet Comparabile<Arca> {
    varia numerus pondus

    functio confer(Arca alius) → numerus { redde ego.pondus - alius.pondus }
}

genus Saccus implet Comparabile<Saccus> {
    varia numerus massa

    functio confer(Saccus alius) → numerus { redde ego.massa - alius.massa }
}

genus Lapis {
    varia numerus massa
}

functio maior<T implet Comparabile<T>>(T a, T b) → T {
    si a.confer(b) ≻ 0 { redde a }
    redde b
}

";

fn with_contract(body: &str) -> String {
    format!("{CONTRACT}{body}")
}

#[test]
fn bounded_function_calls_contract_member_on_type_parameter() {
    test_support::assert_accepts(CONTRACT);
}

#[test]
fn unbounded_type_parameter_has_no_contract_members() {
    test_support::assert_rejects_with_issue(
        "implendum Comparabile<T> {\n    functio confer(T alius) → numerus\n}\n\n\
         functio maior<T>(T a, T b) → numerus { redde a.confer(b) }\n",
        "unknown_method",
    );
}

#[test]
fn member_call_through_bound_checks_substituted_argument_type() {
    test_support::assert_rejects_with_issue(
        "implendum Comparabile<T> {\n    functio confer(T alius) → numerus\n}\n\n\
         functio f<T implet Comparabile<T>>(T a) → numerus { redde a.confer(\"x\") }\n",
        "expression_type_mismatch",
    );
}

#[test]
fn conforming_types_instantiate_the_bound() {
    test_support::assert_accepts(&with_contract(
        "incipit {\n    sit Arca x ← maior(Arca { pondus = 3 }, Arca { pondus = 5 })\n    \
         sit Saccus y ← maior(Saccus { massa = 2 }, Saccus { massa = 1 })\n    \
         nota x.pondus\n    nota y.massa\n}\n",
    ));
}

#[test]
fn non_conforming_type_names_type_and_contract() {
    test_support::assert_rejects_with_args(
        &with_contract(
            "incipit {\n    sit Lapis z ← maior(Lapis { massa = 1 }, Lapis { massa = 2 })\n    \
             nota z.massa\n}\n",
        ),
        &[
            DiagnosticArg::new("issue", "generic_type_arg_bound"),
            DiagnosticArg::new("rejected", "Lapis"),
            DiagnosticArg::new("param", "T"),
            DiagnosticArg::new("contract", "Comparabile<Lapis>"),
        ],
    );
}

#[test]
fn contract_with_other_arguments_does_not_satisfy_the_bound() {
    test_support::assert_rejects_with_issue(
        &with_contract(
            "genus Mixtum implet Comparabile<Arca> {\n    varia numerus m\n\n    \
             functio confer(Arca alius) → numerus { redde 0 }\n}\n\n\
             incipit {\n    sit Mixtum z ← maior(Mixtum { m = 1 }, Mixtum { m = 2 })\n    \
             nota z.m\n}\n",
        ),
        "generic_type_arg_bound",
    );
}

#[test]
fn bounded_parameter_satisfies_the_same_bound_at_a_nested_call() {
    test_support::assert_accepts(&with_contract(
        "functio maximus<U implet Comparabile<U>>(U a, U b, U c) → U {\n    \
         redde maior(maior(a, b), c)\n}\n",
    ));
}

#[test]
fn unbounded_parameter_does_not_satisfy_a_bound_at_a_nested_call() {
    test_support::assert_rejects_with_issue(
        &with_contract("functio maximus<U>(U a, U b) → U {\n    redde maior(a, b)\n}\n"),
        "generic_type_arg_bound",
    );
}

#[test]
fn generic_contract_requires_its_type_arguments() {
    test_support::assert_rejects_with_args(
        "implendum Comparabile<T> {\n    functio confer(T alius) → numerus\n}\n\n\
         genus Arca implet Comparabile {\n    varia numerus pondus\n\n    \
         functio confer(Arca alius) → numerus { redde 0 }\n}\n",
        &[
            DiagnosticArg::new("issue", "implet_contract_arity"),
            DiagnosticArg::new("contract", "Comparabile"),
            DiagnosticArg::new("expected", "1"),
            DiagnosticArg::new("found", "0"),
        ],
    );
}

#[test]
fn bound_without_type_arguments_is_rejected() {
    test_support::assert_rejects_with_issue(
        "implendum Comparabile<T> {\n    functio confer(T alius) → numerus\n}\n\n\
         functio f<T implet Comparabile>(T a) → T { redde a }\n",
        "implet_contract_arity",
    );
}

#[test]
fn bound_must_name_an_implendum() {
    test_support::assert_rejects_with_issue(
        "genus Arca {\n    varia numerus pondus\n}\n\nfunctio f<T implet Arca>(T a) → T { redde a }\n",
        "implet_requires_implendum",
    );
}

#[test]
fn bound_on_type_declaration_parameter_is_rejected() {
    test_support::assert_rejects_with_issue(
        "implendum Comparabile<T> {\n    functio confer(T alius) → numerus\n}\n\n\
         genus Capsa<T implet Comparabile<T>> {\n    varia T res\n}\n",
        "implet_bound_on_type_declaration",
    );
}

#[test]
fn several_bounds_each_contribute_members() {
    test_support::assert_accepts(
        "implendum Comparabile<T> {\n    functio confer(T alius) → numerus\n}\n\n\
         implendum Nominatum {\n    functio nomen() → textus\n}\n\n\
         functio f<T implet Comparabile<T> ∩ Nominatum>(T a, T b) → textus {\n    \
         si a.confer(b) ≻ 0 { redde a.nomen() }\n    redde b.nomen()\n}\n",
    );
}

#[test]
fn implet_missing_method_names_genus_contract_method() {
    test_support::assert_rejects_with_args(
        "implendum Salutans {\n    functio saluta() → textus\n    functio vale() → textus\n}\n\n\
         genus Persona implet Salutans {\n    varia textus nomen\n\n    \
         functio saluta() → textus { redde ego.nomen }\n}\n\nincipit { }\n",
        &[
            DiagnosticArg::new("issue", "implet_missing_method"),
            DiagnosticArg::new("genus", "Persona"),
            DiagnosticArg::new("contract", "Salutans"),
            DiagnosticArg::new("method", "vale"),
        ],
    );
}

#[test]
fn implet_method_signature_mismatch_reports_expected_and_found() {
    test_support::assert_rejects_with_issue(
        "implendum Legens {\n    functio lege() → numerus\n}\n\n\
         genus Charta implet Legens {\n    functio lege() → textus { redde \"charta\" }\n}\n\n\
         incipit { }\n",
        "implet_method_signature_mismatch",
    );
}
