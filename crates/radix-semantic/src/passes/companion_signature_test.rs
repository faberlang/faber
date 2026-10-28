//! Coverage for companion-function signature synthesis.
//!
//! These tests prove that backward companions receive function types with the
//! correct arity (tensor params + residual + upstream), parameter structure,
//! and tuple return shape.

use crate::{PassConfig, SemanticError, SemanticErrorKind, Type};
use radix_target::Target;

fn latin_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn analyze_result(source: &str) -> crate::SemanticResult {
    let lex = radix_lexer::lex_with_locale_pack(source, &latin_pack());
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    crate::analyze(&program, &config, &mut parse.interner)
}

fn analyze_errors(source: &str) -> Vec<SemanticError> {
    let lex = radix_lexer::lex_with_locale_pack(source, &latin_pack());
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    let result = crate::analyze(&program, &config, &mut parse.interner);
    result.diagnostics
}

/// Structured-fact matcher: error kind plus machine `issue` argument.
fn has_issue(errors: &[SemanticError], kind: SemanticErrorKind, issue: &str) -> bool {
    errors
        .iter()
        .any(|e| e.kind == kind && e.args.iter().any(|a| a.name == "issue" && a.value == issue))
}

/// Simple primal with two tensor params to produce a 4-param companion.
const TWO_TENSOR_PRIMAL: &str = r#"
@ radix lane "air"
@ radix backward "mul_backward"
functio mul_pair(tf32[2,2] a, tf32[2,2] b) → tf32[2,2] {
    redde a ⊙ b
}
"#;

/// Primal with one tensor param and one non-tensor param to produce a 3-param companion.
const ONE_TENSOR_PRIMAL: &str = r#"
@ radix lane "air"
@ radix backward "f_backward"
functio f(tf32[4] x, numerus n) → tf32[4] {
    redde x
}
"#;

#[test]
fn companion_symbol_has_func_type() {
    let result = analyze_result(TWO_TENSOR_PRIMAL);

    let companion_def_id = result
        .radix_lanes
        .iter_backward()
        .next()
        .map(|(_, bw)| bw.companion_def_id)
        .expect("companion should be in radix_lanes");

    let symbol = result
        .resolver
        .get_symbol(companion_def_id)
        .expect("companion symbol should exist in resolver");

    let ty_id = symbol.ty.expect("companion symbol should have a type");
    let ty = result.types.get(ty_id);
    assert!(
        matches!(ty, Type::Func(_)),
        "companion symbol ty should be Type::Func, got: {ty:?}"
    );
}

#[test]
fn companion_func_sig_has_correct_arity() {
    let result = analyze_result(TWO_TENSOR_PRIMAL);

    let companion_def_id = result
        .radix_lanes
        .iter_backward()
        .next()
        .map(|(_, bw)| bw.companion_def_id)
        .expect("companion should be in radix_lanes");

    let symbol = result
        .resolver
        .get_symbol(companion_def_id)
        .expect("companion symbol should exist in resolver");

    let ty_id = symbol.ty.expect("companion symbol should have a type");
    let ty = result.types.get(ty_id);
    let Type::Func(sig) = ty else {
        panic!("expected Func type, got {ty:?}");
    };

    // 2 tensor params + residual + upstream = 4 params.
    assert_eq!(
        sig.params.len(),
        4,
        "companion signature should have 4 params (2 tensors + residual + upstream), got {}",
        sig.params.len()
    );

    // Return type should be a tuple of 2 gradient types.
    let ret_ty = result.types.get(sig.ret);
    let grad_count = match ret_ty {
        Type::Tuple(elements) => elements.len(),
        _ => panic!("expected tuple return, got {ret_ty:?}"),
    };
    assert_eq!(
        grad_count, 2,
        "companion should return 2 gradients, got {grad_count}"
    );
}

#[test]
fn call_with_wrong_arity_typecheck_errors() {
    let source = r#"
@ radix lane "air"
@ radix backward "mul_backward"
functio mul_pair(numerus a, numerus b) → numerus {
    redde a + b
}

incipit {
    sit numerus x ← 5
    sit numerus result ← mul_backward(x)
    nota result
}
"#;
    let errors = analyze_errors(source);

    let has_wrong_arity = errors.iter().any(|e| {
        e.kind == SemanticErrorKind::ArityMismatch
            && e.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "wrong_number_of_arguments")
    });
    assert!(
        has_wrong_arity,
        "call with wrong arity should produce WrongArity error; errors: {errors:?}"
    );
}

#[test]
fn companion_call_with_correct_arity_typechecks() {
    // Correct-arity companion call must not produce SEM001, WrongArity,
    // or NotCallable on the companion name. Argument type mismatches from
    // using `vacua` as tensor args are unrelated noise and are tolerated.
    // The residual param is now vacuum (Primitive::Vacuum), so vacua as
    // the 3rd arg matches — only tensor-arg types remain mismatched.
    let source = r#"
@ radix lane "air"
@ radix backward "mean_backward"
functio mean(tf32[5] x, tf32[5] y) → fractus {
    redde (x + y).media()
}

incipit {
    sit fractus upstream ← 1.0
    sit _ ← mean_backward(vacua, vacua, vacua, upstream)
}
"#;
    let errors = analyze_errors(source);

    // Companion name must resolve — no SEM001 for mean_backward.
    let has_sem001 = errors.iter().any(|e| {
        e.kind == SemanticErrorKind::VariableUndefined
            && e.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "unknown_identifier")
            && e.args
                .iter()
                .any(|a| a.name == "name" && a.value == "mean_backward")
    });
    assert!(
        !has_sem001,
        "companion name should resolve without SEM001; errors: {errors:?}"
    );

    // Correct arity: 4 args supplied, 4 params expected.
    let has_wrong_arity = errors
        .iter()
        .any(|e| e.kind == SemanticErrorKind::ArityMismatch);
    assert!(
        !has_wrong_arity,
        "companion call with correct arity (4 args) should not produce WrongArity; errors: {errors:?}"
    );

    // Companion has a Func type → callable.
    let has_not_callable = errors
        .iter()
        .any(|e| e.kind == SemanticErrorKind::NonCallable);
    assert!(
        !has_not_callable,
        "companion with Func type should not produce NotCallable; errors: {errors:?}"
    );
}

/// With correctly typed tensor args and a vacuum residual, the companion call
/// must fully typecheck with zero errors.
#[test]
fn companion_call_with_correctly_typed_args_typechecks_fully() {
    let source = r#"
@ radix lane "air"
@ radix backward "mean_backward"
functio mean(tf32[5] x, tf32[5] y) → fractus {
    redde (x + y).media()
}

functio nil() → vacuum {
    tacet
}

incipit {
    sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0]
    sit tf32[5] x ← flat ↦ tf32[5]
    sit tf32[5] y ← flat ↦ tf32[5]
    sit fractus upstream ← 1.0
    sit _ grads ← mean_backward(x, y, nil(), upstream)
}
"#;
    let result = analyze_result(source);
    let errors = &result.diagnostics;
    assert!(
        result.is_ok(),
        "companion call with correct tensor args + vacuum residual should fully typecheck; errors: {errors:?}"
    );
}

const GENERIC_TWO_TENSOR_PRIMAL: &str = r#"
@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss<magnitudo R, magnitudo C>(
    tensor<f32, [R, C]> x, tensor<f32, [R, C]> w
) → f32 {
    sit tensor<f32, [R, C]> product ← x ⊙ w
    redde product.media()
}
"#;

fn generic_companion_probe_source(call: &str, declarations: &str) -> String {
    format!(
        "{GENERIC_TWO_TENSOR_PRIMAL}\nfunctio nil() → vacuum {{\n    tacet\n}}\n\n{declarations}\n\nincipit {{\n    sit tensor<f32, [2, 2]> x ← vacua\n    sit tensor<f32, [2, 2]> w ← vacua\n    sit f32 upstream ← 1.0\n    {call}\n}}"
    )
}

/// P1: an implicit companion call must substitute both tensor members of its
/// generated tuple return before a concrete downstream consumer sees them.
#[test]
fn generic_companion_implicit_call_substitutes_tuple_return_shapes() {
    let source = generic_companion_probe_source(
        "sit [gx, gw] ← loss_backward(x, w, nil(), upstream)\n    sit f32 consumed ← consume(x, gw)\n    nota consumed",
        r"functio consume(tensor<f32, [2, 2]> gx, tensor<f32, [2, 2]> gw) → f32 {
    redde 1.0
}",
    );
    let result = analyze_result(&source);
    let errors = &result.diagnostics;
    assert!(
        result.is_ok(),
        "implicit generic companion call should substitute tuple-return shapes; errors: {:?}",
        result.diagnostics
    );
    // Variable-to-concrete proof: `gw` reaches the concrete [2, 2] consumer.
    // Without tuple-return substitution the baseline reds here are exactly
    // incompatible_tensor_index + argument_type_mismatch at the consumer.
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::TypeMismatch,
            "incompatible_tensor_index"
        ),
        "consumer must see concrete [2, 2] shapes; errors: {errors:?}"
    );
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::TypeMismatch,
            "argument_type_mismatch"
        ),
        "consumer call must typecheck against substituted shapes; errors: {errors:?}"
    );
}

/// P2: explicit shape arguments use the propagated companion type-parameter
/// list, while still binding the shape symbols positionally.
#[test]
fn generic_companion_explicit_shape_args_bind_both_dimensions() {
    let source = generic_companion_probe_source(
        "sit _ grads ← loss_backward<magnitudo 2, magnitudo 2>(x, w, nil(), upstream)",
        "",
    );
    let result = analyze_result(&source);
    let errors = &result.diagnostics;
    assert!(
        result.is_ok(),
        "explicit generic companion shape args should bind both dimensions; errors: {:?}",
        result.diagnostics
    );
    // The explicit list is counted against the propagated 2-param list, so the
    // baseline P2 red (wrong_number_of_type_arguments) must be absent.
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::ArityMismatch,
            "wrong_number_of_type_arguments"
        ),
        "explicit <2, 2> must be accepted against the real type-param count; errors: {errors:?}"
    );
}

/// P3: a concrete tuple result annotation must unify through the generated
/// companion return rather than leaving its tensor shapes symbolic.
#[test]
fn generic_companion_result_tuple_annotation_unifies_shapes() {
    let source = generic_companion_probe_source(
        "sit iuncta<tensor<f32, [2, 2]>, tensor<f32, [2, 2]>> grads ← loss_backward(x, w, nil(), upstream)",
        "",
    );
    let result = analyze_result(&source);
    let errors = &result.diagnostics;
    assert!(
        result.is_ok(),
        "concrete result tuple annotation should unify through generic companion; errors: {:?}",
        result.diagnostics
    );
    // Baseline P3 reds are incompatible_tensor_index + the annotation
    // mismatch; both must be absent once the tuple return substitutes.
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::TypeMismatch,
            "incompatible_tensor_index"
        ),
        "annotated tuple members must unify at concrete shapes; errors: {errors:?}"
    );
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::TypeMismatch,
            "initializer_annotation_mismatch"
        ),
        "initializer must match the concrete tuple annotation; errors: {errors:?}"
    );
}

/// Row (d): the `gradients_simple_loss` variable-to-variable precedent (gradus
/// `src/gradient.fab:242-247`). A generic consumer annotates the companion
/// tuple in its own size vars, so the companion's symbolic members substitute
/// variable-to-variable instead of to concrete dimensions.
#[test]
fn generic_companion_variable_to_variable_consumer_stays_green() {
    let source = format!(
        "{GENERIC_TWO_TENSOR_PRIMAL}\nfunctio nil() → vacuum {{\n    tacet\n}}\n\nfunctio grad_wrap<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x, tensor<f32, [M, N]> w, f32 upstream) → f32 {{\n    sit iuncta<tensor<f32, [M, N]>, tensor<f32, [M, N]>> grads ← loss_backward(x, w, nil(), upstream)\n    sit _ gw ← grads[1]\n    redde upstream + gw.media()\n}}\n\nincipit {{\n    sit tensor<f32, [2, 2]> x ← vacua\n    sit tensor<f32, [2, 2]> w ← vacua\n    sit f32 upstream ← 1.0\n    sit f32 packed ← grad_wrap(x, w, upstream)\n    nota packed\n}}"
    );
    let result = analyze_result(&source);
    let errors = &result.diagnostics;
    assert!(
        result.is_ok(),
        "generic consumer binding companion shapes to its own size vars should typecheck; errors: {errors:?}"
    );
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::TypeMismatch,
            "incompatible_tensor_index"
        ),
        "variable-to-variable tuple annotation must unify; errors: {errors:?}"
    );
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::TypeMismatch,
            "initializer_annotation_mismatch"
        ),
        "variable-to-variable tuple annotation must match its initializer; errors: {errors:?}"
    );
}

/// Row (e): frozen-form regression. A concrete-primal companion keeps its
/// pre-existing behavior: implicit call + concrete downstream consumer, fully
/// green with no shape facts in play.
#[test]
fn concrete_primal_companion_frozen_form_regression() {
    let source = r#"
@ radix lane "air"
@ radix backward "loss2_backward"
functio concrete_loss(tensor<f32, [2, 2]> x, tensor<f32, [2, 2]> w) → f32 {
    sit tensor<f32, [2, 2]> product ← x ⊙ w
    redde product.media()
}

functio nil() → vacuum {
    tacet
}

functio consume(tensor<f32, [2, 2]> gx, tensor<f32, [2, 2]> gw) → f32 {
    redde 1.0
}

incipit {
    sit tensor<f32, [2, 2]> x ← vacua
    sit tensor<f32, [2, 2]> w ← vacua
    sit f32 upstream ← 1.0
    sit [gx, gw] ← loss2_backward(x, w, nil(), upstream)
    sit f32 consumed ← consume(gx, gw)
    nota consumed
}"#;
    let result = analyze_result(source);
    let errors = &result.diagnostics;
    assert!(
        result.is_ok(),
        "concrete-primal companion call + consumer should stay fully green; errors: {errors:?}"
    );
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::TypeMismatch,
            "incompatible_tensor_index"
        ),
        "frozen form must not produce shape/index facts; errors: {errors:?}"
    );
    assert!(
        !has_issue(
            errors,
            SemanticErrorKind::ArityMismatch,
            "wrong_number_of_type_arguments"
        ),
        "frozen form must not produce type-argument facts; errors: {errors:?}"
    );
}

/// Row (f): explicit type args at the wrong arity still reject with
/// `wrong_number_of_type_arguments` — now counted against the real propagated
/// companion type-param list (2), per CTP-1.
#[test]
fn generic_companion_wrong_arity_type_args_still_rejected() {
    for call in [
        "sit _ grads ← loss_backward<magnitudo 2>(x, w, nil(), upstream)",
        "sit _ grads ← loss_backward<magnitudo 2, magnitudo 2, magnitudo 4>(x, w, nil(), upstream)",
    ] {
        let source = generic_companion_probe_source(call, "");
        let errors = analyze_errors(&source);
        assert!(
            has_issue(
                &errors,
                SemanticErrorKind::ArityMismatch,
                "wrong_number_of_type_arguments"
            ),
            "wrong-arity explicit type args must reject with wrong_number_of_type_arguments; call: {call}; errors: {errors:?}"
        );
    }
}

#[test]
fn unknown_name_still_sem001() {
    let source = r#"
@ radix lane "air"
@ radix backward "mul_backward"
functio mul_pair(numerus a, numerus b) → numerus {
    redde a + b
}

incipit {
    sit numerus result ← nonexistent_func()
    nota result
}
"#;
    let errors = analyze_errors(source);

    let has_sem001 = errors.iter().any(|e| {
        e.kind == SemanticErrorKind::VariableUndefined
            && e.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "unknown_identifier")
    });
    assert!(has_sem001, "truly unknown names must still produce SEM001");
}

#[test]
fn non_air_backward_no_phantom_companion() {
    let source = r#"
@ radix lane "mir"
@ radix backward "mir_backward"
functio f(numerus a) → numerus {
    redde a
}
"#;
    let result = analyze_result(source);

    // Should be no backward entries for non-AIR functions.
    assert_eq!(
        result.radix_lanes.iter_backward().count(),
        0,
        "non-air backward should not be in radix_lanes"
    );
}

#[test]
fn single_tensor_param_companion_has_three_params() {
    let result = analyze_result(ONE_TENSOR_PRIMAL);

    let companion_def_id = result
        .radix_lanes
        .iter_backward()
        .next()
        .map(|(_, bw)| bw.companion_def_id)
        .expect("companion should be in radix_lanes");

    let symbol = result
        .resolver
        .get_symbol(companion_def_id)
        .expect("companion symbol should exist in resolver");

    let ty_id = symbol.ty.expect("companion symbol should have a type");
    let Type::Func(sig) = result.types.get(ty_id) else {
        panic!("expected Func type");
    };

    assert_eq!(
        sig.params.len(),
        3,
        "one-tensor primal: companion should have 3 params (1 tensor + residual + upstream), got {}",
        sig.params.len()
    );
}

#[test]
fn companion_def_id_stays_stable() {
    let result = analyze_result(TWO_TENSOR_PRIMAL);
    // HIR should be present (no hard errors).
    let _hir = result.hir.expect("HIR should be present");

    let (_, bw) = result
        .radix_lanes
        .iter_backward()
        .next()
        .expect("companion should be in radix_lanes");

    // The companion DefId should have the 0x8000_0000 high-bit scheme.
    assert_eq!(
        bw.companion_def_id.0 & 0x8000_0000,
        0x8000_0000,
        "companion DefId should use 0x8000_0000 high-bit scheme"
    );

    // The companion symbol in the resolver must reference the same DefId.
    let symbol = result
        .resolver
        .get_symbol(bw.companion_def_id)
        .expect("companion symbol should exist in resolver");
    assert_eq!(symbol.def_id, bw.companion_def_id);
    assert_eq!(symbol.name, bw.companion_name);
}

#[test]
fn companion_no_hir_stub_in_items() {
    // Verify that no synthetic HIR items with the companion DefId are present.
    // The signature is injected directly into typecheck's function table.
    let result = analyze_result(TWO_TENSOR_PRIMAL);
    let hir = result.hir.expect("HIR should be present");

    let companion_def_id = result
        .radix_lanes
        .iter_backward()
        .next()
        .map(|(_, bw)| bw.companion_def_id)
        .expect("companion should be in radix_lanes");

    // The companion should NOT appear as a HIR item (signature is registered
    // directly in typecheck, not via HIR stub).
    let companion_in_hir = hir.items.iter().any(|item| item.def_id == companion_def_id);
    assert!(
        !companion_in_hir,
        "companion should NOT have a HIR item (signature is injected directly into typecheck)"
    );
}
