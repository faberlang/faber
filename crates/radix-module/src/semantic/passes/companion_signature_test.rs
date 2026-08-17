//! Coverage for companion-function signature synthesis.
//!
//! These tests prove that backward companions receive function types with the
//! correct arity (tensor params + residual + upstream), parameter structure,
//! and tuple return shape.

use crate::codegen::Target;
use crate::semantic::{self, PassConfig, SemanticError, SemanticErrorKind, Type};

fn analyze_result(source: &str) -> semantic::SemanticResult {
    let lex = crate::lexer::lex(source);
    let mut parse = crate::parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    semantic::analyze(&program, &config, &mut parse.interner)
}

fn analyze_errors(source: &str) -> Vec<SemanticError> {
    let lex = crate::lexer::lex(source);
    let mut parse = crate::parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    let result = semantic::analyze(&program, &config, &mut parse.interner);
    result.diagnostics
}

/// Simple primal with two tensor params to produce a 4-param companion.
const TWO_TENSOR_PRIMAL: &str = r#"
@ radix lane "air"
@ radix backward "mul_backward"
functio mul_pair(tf32[2,2] a, tf32[2,2] b) → tf32[2,2] {
    redde a.multiplica(b)
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
    assert_eq!(grad_count, 2, "companion should return 2 gradients, got {grad_count}");
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
    fixum numerus x ← 5
    fixum numerus result ← mul_backward(x)
    nota result
}
"#;
    let errors = analyze_errors(source);

    let has_wrong_arity = errors.iter().any(|e| {
        e.kind == SemanticErrorKind::WrongArity
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
    redde x.addita(y).media()
}

incipit {
    fixum fractus upstream ← 1.0
    fixum _ _ ← mean_backward(vacua, vacua, vacua, upstream)
}
"#;
    let errors = analyze_errors(source);

    // Companion name must resolve — no SEM001 for mean_backward.
    let has_sem001 = errors.iter().any(|e| {
        e.kind == SemanticErrorKind::UndefinedVariable
            && e.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "unknown_identifier")
            && e.args
                .iter()
                .any(|a| a.name == "name" && a.value == "mean_backward")
    });
    assert!(!has_sem001, "companion name should resolve without SEM001; errors: {errors:?}");

    // Correct arity: 4 args supplied, 4 params expected.
    let has_wrong_arity = errors
        .iter()
        .any(|e| e.kind == SemanticErrorKind::WrongArity);
    assert!(
        !has_wrong_arity,
        "companion call with correct arity (4 args) should not produce WrongArity; errors: {errors:?}"
    );

    // Companion has a Func type → callable.
    let has_not_callable = errors
        .iter()
        .any(|e| e.kind == SemanticErrorKind::NotCallable);
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
    redde x.addita(y).media()
}

functio nil() → vacuum {
    tacet
}

incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0]
    fixum tf32[] seed ← vacua
    fixum tf32[5] x ← seed.strue(flat, [5])
    fixum tf32[5] y ← seed.strue(flat, [5])
    fixum fractus upstream ← 1.0
    fixum _ grads ← mean_backward(x, y, nil(), upstream)
}
"#;
    let result = analyze_result(source);
    let errors = &result.diagnostics;
    assert!(
        result.success(),
        "companion call with correct tensor args + vacuum residual should fully typecheck; errors: {errors:?}"
    );
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
    fixum numerus result ← nonexistent_func()
    nota result
}
"#;
    let errors = analyze_errors(source);

    let has_sem001 = errors.iter().any(|e| {
        e.kind == SemanticErrorKind::UndefinedVariable
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
