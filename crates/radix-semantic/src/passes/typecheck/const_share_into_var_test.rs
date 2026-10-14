//! `const` is deep (need, rule 2): a plain `var T y ← x` that shares
//! a `const` or `ref` container into a writable name is SEM065
//! `const_shared_into_var`; `← copy x`, `← own x`, scalars and `var` sources
//! stay legal.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::SemanticErrorKind;
use crate::test_support;

const ISSUE: &str = "const_shared_into_var";
const MUTATING_RECEIVER_ISSUE: &str = "imperativus_operator_method_requires_varia_receiver";

#[test]
fn const_list_shared_into_var_is_rejected() {
    let source = "incipit {\n    fixum lista<numerus> xs ← [1]\n    varia lista<numerus> ys ← xs\n    ys.appende(2)\n    nota xs\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn const_scalar_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    fixum numerus a ← 1\n    varia numerus b ← a\n    b ← b + 1\n    nota b\n}",
    );
}

#[test]
fn const_list_copy_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    fixum lista<numerus> xs ← [1]\n    varia lista<numerus> ys ← copy xs\n    ys.appende(2)\n    nota xs\n    nota ys\n}",
    );
}

#[test]
fn var_list_shared_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia lista<numerus> xs ← [1]\n    varia lista<numerus> ys ← xs\n    ys.appende(2)\n    nota ys\n}",
    );
}

#[test]
fn module_constant_list_shared_into_var_is_rejected() {
    let source = "fixum lista<numerus> XS = [1, 2]\nincipit {\n    varia lista<numerus> ys ← XS\n    ys.appende(3)\n    nota XS\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn module_constant_list_copy_into_var_is_accepted() {
    test_support::assert_accepts(
        "fixum lista<numerus> XS = [1, 2]\nincipit {\n    varia lista<numerus> ys ← copy XS\n    ys.appende(3)\n    nota XS\n    nota ys\n}",
    );
}

#[test]
fn module_constant_scalar_into_var_is_accepted() {
    test_support::assert_accepts(
        "fixum numerus N = 2\nincipit {\n    varia numerus b ← N\n    b ← b + 1\n    nota b\n}",
    );
}

#[test]
fn const_genus_shared_into_var_is_rejected() {
    let source = "genus Custos {\n    varia lista<numerus> res\n}\nincipit {\n    fixum Custos c ← Custos { res = [1, 2] }\n    varia Custos d ← c\n    nota d\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn const_genus_field_shared_into_var_is_rejected() {
    let source = "genus Custos {\n    varia lista<numerus> res\n}\nincipit {\n    fixum Custos c ← Custos { res = [1, 2] }\n    varia lista<numerus> ys ← c.res\n    ys.appende(3)\n    nota c\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn module_constant_genus_field_shared_into_var_is_rejected() {
    let source = "genus Custos {\n    varia lista<numerus> res\n}\nfixum Custos C = Custos { res = [1, 2] }\nincipit {\n    varia lista<numerus> ys ← C.res\n    ys.appende(3)\n    nota C\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn call_result_of_const_list_shared_into_var_is_rejected() {
    let source = "functio pick(de lista<numerus> xs) → de lista<numerus> {\n    redde xs\n}\nincipit {\n    fixum lista<numerus> xs ← [1, 2]\n    varia lista<numerus> ys ← pick(xs)\n    ys.appende(3)\n    nota ys\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn const_nested_list_element_shared_into_var_is_rejected() {
    let source = "incipit {\n    fixum lista<lista<numerus>> xs ← [[1], [2]]\n    itera ex xs fixum r {\n        varia lista<numerus> ys ← r\n        ys.appende(3)\n        nota ys\n    }\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn plain_owned_call_result_into_var_is_accepted() {
    test_support::assert_accepts(
        "functio pick(lista<numerus> xs) → lista<numerus> {\n    redde xs\n}\nincipit {\n    fixum lista<numerus> xs ← [1, 2]\n    varia lista<numerus> ys ← pick(xs)\n    ys.appende(3)\n    nota ys\n}",
    );
}

#[test]
fn var_nested_list_element_shared_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia lista<lista<numerus>> xs ← [[1], [2]]\n    itera ex xs fixum r {\n        varia lista<numerus> ys ← r\n        nota ys\n    }\n}",
    );
}

#[test]
fn const_genus_copy_into_var_is_accepted() {
    test_support::assert_accepts(
        "genus Custos {\n    varia lista<numerus> res\n}\nincipit {\n    fixum Custos c ← Custos { res = [1, 2] }\n    varia Custos d ← copy c\n    nota d\n}",
    );
}

#[test]
fn const_list_own_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    fixum lista<numerus> xs ← [1]\n    varia lista<numerus> ys ← own xs\n    ys.appende(2)\n    nota ys\n}",
    );
}

#[test]
fn const_text_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    fixum textus s ← \"salve\"\n    varia textus t ← s\n    nota t\n}",
    );
}

#[test]
fn const_to_const_share_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    fixum lista<numerus> xs ← [1, 2]\n    fixum lista<numerus> ys ← xs\n    nota ys\n}",
    );
}

#[test]
fn de_param_genus_shared_into_var_is_rejected() {
    let source = "genus Punctum {\n    varia numerus x\n}\nfunctio prend(de Punctum q) → vacuum {\n    varia Punctum b ← q\n    nota b.x\n}\nincipit { }";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn de_param_list_shared_into_var_is_rejected() {
    let source = "functio prend(de lista<numerus> xs) → numerus {\n    varia lista<numerus> ys ← xs\n    redde ys.longitudo()\n}\nincipit { }";
    let result = test_support::analyze_source(source);
    let found = result
        .diagnostics
        .iter()
        .find(|err| {
            matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
                && test_support::issue(err) == Some(ISSUE)
        })
        .unwrap_or_else(|| panic!("expected SEM065 {ISSUE}, got {:?}", result.diagnostics));
    assert!(
        found
            .args
            .iter()
            .any(|arg| arg.name == "source" && arg.value == "parameter"),
        "expected source=parameter, got {:?}",
        found.args
    );
}

#[test]
fn unmarked_param_genus_shared_into_var_is_rejected() {
    let source = "genus Punctum {\n    varia numerus x\n}\nfunctio prend(Punctum q) → vacuum {\n    varia Punctum b ← q\n    nota b.x\n}\nincipit { }";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn de_param_const_alias_is_accepted() {
    test_support::assert_accepts(
        "genus Punctum {\n    varia numerus x\n}\nfunctio prend(de Punctum q) → numerus {\n    fixum Punctum b ← q\n    redde b.x\n}\nincipit { }",
    );
}

#[test]
fn de_param_copy_into_var_is_accepted() {
    test_support::assert_accepts(
        "genus Punctum {\n    varia numerus x\n}\nfunctio prend(de Punctum q) → numerus {\n    varia Punctum b ← copy q\n    redde b.x\n}\nincipit { }",
    );
}

#[test]
fn in_param_genus_shared_into_var_is_accepted() {
    test_support::assert_accepts(
        "genus Punctum {\n    varia numerus x\n}\nfunctio prend(in Punctum q) → numerus {\n    varia Punctum b ← q\n    redde b.x\n}\nincipit { }",
    );
}

#[test]
fn de_param_scalar_into_var_is_accepted() {
    test_support::assert_accepts(
        "functio prend(de numerus n) → numerus {\n    varia numerus m ← n\n    redde m\n}\nincipit { }",
    );
}

#[test]
fn parameter_source_arg_is_parameter() {
    let source = "functio prend(de lista<numerus> xs) → numerus {\n    varia lista<numerus> ys ← xs\n    redde ys.longitudo()\n}\nincipit { }";
    let result = test_support::analyze_source(source);
    let err = result
        .diagnostics
        .iter()
        .find(|err| {
            matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
                && test_support::issue(err) == Some(ISSUE)
        })
        .unwrap_or_else(|| panic!("expected SEM065 {ISSUE}, got {:?}", result.diagnostics));
    assert_eq!(
        err.args
            .iter()
            .find(|arg| arg.name == "source")
            .map(|arg| arg.value.as_str()),
        Some("parameter"),
        "expected source=parameter, got {:?}",
        err.args
    );
}

#[test]
fn const_local_source_arg_is_constant() {
    let source = "incipit {\n    fixum lista<numerus> xs ← [1]\n    varia lista<numerus> ys ← xs\n    ys.appende(2)\n    nota xs\n}";
    let result = test_support::analyze_source(source);
    let err = result
        .diagnostics
        .iter()
        .find(|err| {
            matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
                && test_support::issue(err) == Some(ISSUE)
        })
        .unwrap_or_else(|| panic!("expected SEM065 {ISSUE}, got {:?}", result.diagnostics));
    assert_eq!(
        err.args
            .iter()
            .find(|arg| arg.name == "source")
            .map(|arg| arg.value.as_str()),
        Some("constant"),
        "expected source=constant, got {:?}",
        err.args
    );
}

#[test]
fn itera_binder_source_over_parameter_is_parameter() {
    let source = "functio f(lista<lista<numerus>> xss) → numerus {\n    itera ex xss fixum r {\n        varia lista<numerus> y ← r\n        y.appende(1)\n    }\n    redde 0\n}\nincipit { }";
    let result = test_support::analyze_source(source);
    let found = result
        .diagnostics
        .iter()
        .find(|err| {
            matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
                && test_support::issue(err) == Some(ISSUE)
        })
        .unwrap_or_else(|| panic!("expected SEM065 {ISSUE}, got {:?}", result.diagnostics));
    let source_arg = found
        .args
        .iter()
        .find(|arg| arg.name == "source")
        .unwrap_or_else(|| panic!("expected source arg, got {:?}", found.args));
    assert_eq!(
        source_arg.value, "parameter",
        "itera binder over a read-only parameter is a parameter source: {:?}",
        found.args
    );
}

#[test]
fn itera_binder_source_over_const_local_is_constant() {
    let source = "incipit {\n    fixum lista<lista<numerus>> xs ← [[1], [2]]\n    itera ex xs fixum r {\n        varia lista<numerus> y ← r\n        y.appende(1)\n    }\n}";
    let result = test_support::analyze_source(source);
    let found = result
        .diagnostics
        .iter()
        .find(|err| {
            matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
                && test_support::issue(err) == Some(ISSUE)
        })
        .unwrap_or_else(|| panic!("expected SEM065 {ISSUE}, got {:?}", result.diagnostics));
    let source_arg = found
        .args
        .iter()
        .find(|arg| arg.name == "source")
        .unwrap_or_else(|| panic!("expected source arg, got {:?}", found.args));
    assert_eq!(
        source_arg.value, "constant",
        "itera binder over a const local is a constant source: {:?}",
        found.args
    );
}

/// Reassigning a `varia` local from an unmarked read-only parameter is not
/// guarded by SEM065: `check_local` runs `reject_read_only_share_into_var`
/// only at a declaration, and the assignment installs `ys` as an alias of the
/// read-only parameter. The shared alias is caught one step later, when a
/// write goes through it (`ys.appende(1)` here): SEM020 `ImmutableAssignment`
/// with the mutating-receiver issue. SEM065 is not the guard for reassignment,
/// so no second diagnostic is emitted at `ys ← xs`.
#[test]
fn reassign_from_parameter_is_rejected() {
    let source = "functio f(lista<numerus> xs) → numerus {\n    varia lista<numerus> ys ← []\n    ys ← xs\n    ys.appende(1)\n    redde ys.longitudo()\n}\nincipit { }";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ImmutableAssignment)
            && test_support::issue(err) == Some(MUTATING_RECEIVER_ISSUE)),
        "expected SEM020 {MUTATING_RECEIVER_ISSUE}, got {:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|err| matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)),
        "SEM065 is not the guard for reassignment, got {:?}",
        result.diagnostics
    );
}

/// `← copy xs` is the legal spelling: the copy is independent, so `ys` stays
/// a fresh writable local and `ys.appende(1)` is accepted.
#[test]
fn reassign_from_parameter_copy_is_accepted() {
    test_support::assert_accepts(
        "functio f(lista<numerus> xs) → numerus {\n    varia lista<numerus> ys ← []\n    ys ← copy xs\n    ys.appende(1)\n    redde ys.longitudo()\n}\nincipit { }",
    );
}

// ── Wrapped sources (7b471d14-7): a wrapper around the initializer must not
// hide a read-only root. `place_roots` walks every branch that can supply the
// value: both `Si` branch tails (the `c ✓ a ✗ b` conditional expression), both
// `vel` coalesce operands, the inner of a `∷` ascription, and a block tail.
//
// Parentheses lower through `ExprKind::Paren` to the inner expression, so
// `(xs)` is the plain `Path(xs)` node by the time typecheck sees it and is
// already covered by `place_root`; the test below pins that erasure.
//
// A bare source `Block` cannot be written in initializer position in the
// current surface (braces are JSON literals), so the `Block` arm is exercised
// through the conditional branch tails, which lower to blocks.

fn assert_wrapped_source_into_var_rejected(source: &str) {
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn wrapped_source_into_var_parenthesized_parameter_is_rejected() {
    assert_wrapped_source_into_var_rejected(
        "functio f(lista<numerus> xs) → numerus {\n    varia lista<numerus> ys ← (xs)\n    redde ys.longitudo()\n}\nincipit { }",
    );
}

#[test]
fn wrapped_source_into_var_coalesce_parameter_is_rejected() {
    assert_wrapped_source_into_var_rejected(
        "functio f(lista<numerus> xs) → numerus {\n    varia lista<numerus> ∪ nihil maybe ← nulla\n    varia lista<numerus> ys ← maybe vel xs\n    redde ys.longitudo()\n}\nincipit { }",
    );
}

#[test]
fn wrapped_source_into_var_ascription_parameter_is_rejected() {
    assert_wrapped_source_into_var_rejected(
        "functio f(lista<numerus> xs) → numerus {\n    varia lista<numerus> ys ← xs ∷ lista<numerus>\n    redde ys.longitudo()\n}\nincipit { }",
    );
}

#[test]
fn wrapped_source_into_var_conditional_parameter_is_rejected() {
    assert_wrapped_source_into_var_rejected(
        "functio f(lista<numerus> xs) → numerus {\n    fixum bivalens c ← verum\n    varia lista<numerus> ys ← c ✓ xs ✗ [2]\n    redde ys.longitudo()\n}\nincipit { }",
    );
}

#[test]
fn wrapped_source_into_var_all_fresh_is_accepted() {
    test_support::assert_accepts(
        "functio f(bivalens c) → numerus {\n    varia lista<numerus> ys ← c ✓ [1] ✗ [2]\n    redde ys.longitudo()\n}\nincipit { }",
    );
}

// ── Aggregates holding containers (7b471d14-8): a tuple or union that holds a
// container shares that container's state, so a plain `var T y ← x` from an
// unmarked parameter is SEM065 just like a bare container. A scalar-only
// aggregate still copies by value and stays legal.

fn assert_aggregate_into_var_rejected(source: &str) {
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn shareable_payload_tuple_with_list_into_var_is_rejected() {
    assert_aggregate_into_var_rejected(
        "functio prend(de iuncta<lista<numerus>, numerus> p) → numerus {\n    varia iuncta<lista<numerus>, numerus> q ← p\n    redde 0\n}\nincipit { }",
    );
}

#[test]
fn shareable_payload_union_with_list_into_var_is_rejected() {
    assert_aggregate_into_var_rejected(
        "functio prend(lista<numerus> ∪ numerus p) → numerus {\n    varia lista<numerus> ∪ numerus q ← p\n    redde 0\n}\nincipit { }",
    );
}

#[test]
fn shareable_payload_tuple_of_scalars_into_var_is_accepted() {
    test_support::assert_accepts(
        "functio prend(de iuncta<numerus, numerus> p) → numerus {\n    varia iuncta<numerus, numerus> q ← p\n    redde 0\n}\nincipit { }",
    );
}

#[test]
fn shareable_payload_union_of_scalars_into_var_is_accepted() {
    test_support::assert_accepts(
        "functio prend(numerus ∪ fractus p) → numerus {\n    varia numerus ∪ fractus q ← p\n    redde 0\n}\nincipit { }",
    );
}

#[test]
fn shareable_payload_generic_param_into_var_is_accepted() {
    test_support::assert_accepts(
        "functio prend<T>(T p) → numerus {\n    varia T q ← p\n    redde 0\n}\nincipit { }",
    );
}

// ── Method-call sources (7b471d14-9): a promoted collection method whose
// registry row is the `Values` view (`tabula.valores`) yields an array over
// the receiver's own values, so a binder over it aliases the receiver exactly
// like a binder over a place rooted in the receiver. A scalar-yielding method
// (`claves`), a scalar element, and a receiver that is not a read-only place
// all stay accepted.

fn assert_method_source_into_var_rejected(source: &str) {
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn itera_method_source_binder_valores_over_parameter_is_rejected() {
    assert_method_source_into_var_rejected(
        "functio f(tabula<textus, lista<numerus>> p) → numerus {\n    itera ex p.valores() fixum r {\n        varia lista<numerus> y ← r\n        y.appende(1)\n    }\n    redde 0\n}\nincipit { }",
    );
}

#[test]
fn itera_method_source_binder_valores_over_const_local_is_rejected() {
    assert_method_source_into_var_rejected(
        "incipit {\n    fixum tabula<textus, lista<numerus>> p ← vacua\n    itera ex p.valores() fixum r {\n        varia lista<numerus> y ← r\n        y.appende(1)\n    }\n}",
    );
}

#[test]
fn itera_method_source_binder_scalar_element_is_accepted() {
    test_support::assert_accepts(
        "functio f(tabula<textus, numerus> p) → numerus {\n    itera ex p.valores() fixum r {\n        varia numerus y ← r\n        y ← y + 1\n    }\n    redde 0\n}\nincipit { }",
    );
}

#[test]
fn itera_method_source_binder_claves_scalar_keys_is_accepted() {
    test_support::assert_accepts(
        "functio f(tabula<textus, lista<numerus>> p) → numerus {\n    itera ex p.claves() fixum k {\n        varia textus t ← k\n        nota t\n    }\n    redde 0\n}\nincipit { }",
    );
}

#[test]
fn itera_method_source_binder_fresh_local_is_accepted() {
    test_support::assert_accepts(
        "functio f() → numerus {\n    varia tabula<textus, lista<numerus>> p ← vacua\n    itera ex p.valores() fixum r {\n        varia lista<numerus> y ← r\n        y.appende(1)\n    }\n    redde 0\n}\nincipit { }",
    );
}

/// 7b471d14-9 follow-up: `is_receiver_values_view` is pack-first exactly like
/// `check_intrinsic_method_call`. Under a non-`la` pack the canonical Latin
/// spelling is not a legal method, so it must not count as the Values view;
/// only the active pack's surface spelling does.
fn analyze_en(source: &str) -> test_support::AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    test_support::analyze_source_with_config(source, config)
}

fn has_const_shared_into_var(result: &test_support::AnalyzedFixture) -> bool {
    result.diagnostics.iter().any(|err| {
        err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)
    })
}

#[test]
fn values_view_detection_is_pack_first() {
    let en_values = "fn f(map<string, list<int>> p) → int {\n    for from p.values() const r {\n        var list<int> y ← r\n        y.append(1)\n    }\n    return 0\n}\nmain { }";
    let en_valores = "fn f(map<string, list<int>> p) → int {\n    for from p.valores() const r {\n        var list<int> y ← r\n        y.append(1)\n    }\n    return 0\n}\nmain { }";

    assert!(
        has_const_shared_into_var(&analyze_en(en_values)),
        "the en surface `values` is the Values view, so a binder over it must stay SEM065 {ISSUE}"
    );
    assert!(
        !has_const_shared_into_var(&analyze_en(en_valores)),
        "canonical Latin `valores` is not a method under the en pack, so it must not count as the Values view"
    );
}

// ── `→ de T` call results beyond one free function with one `de` parameter
// (7b471d14-10): a method call's `de` result hands back the receiver's state,
// and a free function with several `de` parameters of the returned payload
// type may hand back any of them. Every candidate root the callee exposes is
// a possible source, so any read-only candidate makes the bind a shared
// read-only alias (SEM065). A plain `→ T` result stays a fresh value.

#[test]
fn de_result_alias_method_call_receiver_over_parameter_is_rejected() {
    let source = "genus Custos {\n    varia lista<numerus> res\n    functio sume() → de lista<numerus> {\n        redde ego.res\n    }\n}\nfunctio f(Custos p) → numerus {\n    varia lista<numerus> y ← p.sume()\n    redde y.longitudo()\n}\nincipit { }";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn de_result_alias_free_function_two_de_params_over_parameter_is_rejected() {
    let source = "functio pick(de lista<numerus> a, de lista<numerus> b) → de lista<numerus> {\n    redde a\n}\nfunctio f(lista<numerus> xs) → numerus {\n    fixum lista<numerus> clean ← [1, 2]\n    varia lista<numerus> y ← pick(xs, clean)\n    redde y.longitudo()\n}\nincipit { }";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn de_result_alias_free_function_two_de_params_over_fresh_locals_is_accepted() {
    test_support::assert_accepts(
        "functio pick(de lista<numerus> a, de lista<numerus> b) → de lista<numerus> {\n    redde a\n}\nfunctio f() → numerus {\n    varia lista<numerus> a ← [1]\n    varia lista<numerus> b ← [2]\n    varia lista<numerus> y ← pick(a, b)\n    redde y.longitudo()\n}\nincipit { }",
    );
}

#[test]
fn de_result_alias_method_call_over_fresh_receiver_is_accepted() {
    test_support::assert_accepts(
        "genus Custos {\n    varia lista<numerus> res\n    functio sume() → de lista<numerus> {\n        redde ego.res\n    }\n}\nfunctio f() → numerus {\n    varia Custos p ← Custos { res = [1, 2] }\n    varia lista<numerus> y ← p.sume()\n    redde y.longitudo()\n}\nincipit { }",
    );
}
