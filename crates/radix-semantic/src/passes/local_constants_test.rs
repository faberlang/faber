//! Local compile-time constants, `fixum T x = e` (DECISIONS D5.10).
//!
//! Contract:
//! - the initializer is evaluated by the statics' evaluator: literals,
//!   arithmetic, other local constants, and `generis` statics all qualify;
//! - the value must fit the declared type whatever its overflow policy;
//! - a non-constant initializer is SEM060 `constant_initializer_not_constant`;
//! - `varia T x = e` is a parse error naming the `=` rule;
//! - a constant is an ordinary immutable local of type `T`, usable in
//!   arithmetic;
//! - `←` keeps its runtime meaning: `sit T x ← e` accepts a runtime value.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::{analyze_source_with_config, assert_accepts, assert_rejects_with_issue};
use radix_hir::HirStatementKind;
use radix_target::Target;

#[test]
fn constant_initializer_is_accepted() {
    assert_accepts("functio f() → numerus {\n    fixum numerus n = 4 * 8\n    redde n\n}\n");
}

#[test]
fn constant_is_recorded_in_the_presentation_sidecar() {
    let fixture = analyze_source_with_config(
        "functio f() → numerus {\n    fixum numerus n = 4\n    sit numerus m ← 5\n    redde n + m\n}\n",
        PassConfig::for_target(Target::HirRust),
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let radix_hir::HirItemKind::Function(func) = &hir.items[0].kind else {
        panic!("expected function");
    };
    let locals: Vec<_> = func
        .body
        .as_ref()
        .expect("body")
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => Some(local.def_id),
            _ => None,
        })
        .collect();
    let recorded = &fixture.result.presentation.compile_time_locals;
    assert_eq!(recorded.as_slice(), &[locals[0]]);
}

#[test]
fn constants_reference_other_constants_and_statics() {
    assert_accepts(
        "fixum numerus BASIS = 10\n\
         functio f() → numerus {\n    \
             fixum numerus a = BASIS * 2\n    \
             fixum numerus b = a + 1\n    \
             redde b\n}\n",
    );
}

#[test]
fn constant_is_usable_in_arithmetic() {
    assert_accepts(
        "functio f(numerus p) → numerus {\n    fixum numerus k = 3\n    redde p * k + k\n}\n",
    );
}

#[test]
fn inferred_slot_takes_the_constant_default() {
    assert_accepts("functio f() → numerus {\n    fixum _ n = 10\n    redde n\n}\n");
}

#[test]
fn constant_that_fits_its_type_is_accepted() {
    assert_accepts("functio f() → numerus {\n    fixum u8 d = 200\n    redde 0\n}\n");
}

#[test]
fn constant_that_does_not_fit_is_rejected() {
    assert_rejects_with_issue(
        "functio f() → numerus {\n    fixum u8 d = 300\n    redde 0\n}\n",
        "store_constant_out_of_range",
    );
}

#[test]
fn constant_must_fit_whatever_the_overflow_policy() {
    assert_rejects_with_issue(
        "functio f() → numerus {\n    fixum saturatus<u8> w = 300\n    redde 0\n}\n",
        "store_constant_out_of_range",
    );
    assert_rejects_with_issue(
        "functio f() → numerus {\n    fixum modulus<u8> w = 300\n    redde 0\n}\n",
        "store_constant_out_of_range",
    );
}

#[test]
fn computed_constant_that_does_not_fit_is_rejected() {
    assert_rejects_with_issue(
        "fixum u8 A = 200\n\
         functio f() → numerus {\n    fixum u8 d = A + 100\n    redde 0\n}\n",
        "constant_arithmetic_overflow",
    );
}

#[test]
fn division_by_zero_is_rejected() {
    assert_rejects_with_issue(
        "functio f() → numerus {\n    fixum numerus n = 4 / 0\n    redde n\n}\n",
        "constant_division_by_zero",
    );
}

#[test]
fn non_constant_initializer_is_rejected() {
    assert_rejects_with_issue(
        "functio f(numerus p) → numerus {\n    fixum numerus n = p + 1\n    redde n\n}\n",
        "constant_initializer_not_constant",
    );
}

#[test]
fn runtime_local_is_not_a_constant() {
    assert_rejects_with_issue(
        "functio f() → numerus {\n    sit numerus r ← 5\n    fixum numerus n = r + 1\n    redde n\n}\n",
        "constant_initializer_not_constant",
    );
}

#[test]
fn call_initializer_is_rejected() {
    assert_rejects_with_issue(
        "functio g() → numerus { redde 1 }\n\
         functio f() → numerus {\n    fixum numerus n = g()\n    redde n\n}\n",
        "constant_initializer_not_constant",
    );
}

#[test]
fn arrow_initializer_accepts_a_runtime_value() {
    assert_accepts("functio f(numerus p) → numerus {\n    sit numerus n ← p + 1\n    redde n\n}\n");
}

#[test]
fn varia_with_equals_is_rejected() {
    assert_rejects_with_issue(
        "functio f() → numerus {\n    varia numerus n = 4\n    redde n\n}\n",
        "varia_compile_time_initializer",
    );
}

#[test]
fn constant_is_not_assignable() {
    assert_rejects_with_issue(
        "functio f() → numerus {\n    fixum numerus n = 4\n    n ← 5\n    redde n\n}\n",
        "immutable_second_assignment",
    );
}

fn en_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config
}

#[test]
fn english_spelling_is_a_constant_too() {
    let ok = analyze_source_with_config(
        "fn f() → int {\n    const int n = 4 * 8\n    return n\n}\n",
        en_config(),
    );
    assert!(ok.is_ok(), "diagnostics: {:?}", ok.diagnostics);
    let bad = analyze_source_with_config(
        "fn f(int p) → int {\n    const int n = p + 1\n    return n\n}\n",
        en_config(),
    );
    assert!(
        bad.diagnostics
            .iter()
            .any(|err| err.issue() == Some("constant_initializer_not_constant")),
        "diagnostics: {:?}",
        bad.diagnostics
    );
}
