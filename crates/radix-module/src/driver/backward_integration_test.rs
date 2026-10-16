//! Driver-level backward generation integration tests.
//! Verifies multi-function, zero-function, mixed-annotation, MIR structural, and unique function IDs.

use super::compile;
use crate::codegen::Target;
use crate::driver::ModuleCompileRequest;
use crate::test_support::test_session_la;

// ── Test 1: Two backward-annotated functions ──

#[test]
fn air_lane_backward_handles_two_backward_functions() {
    // P1: Iterate over multiple @ radix backward annotations in one source.
    // Each annotated function must produce its own backward companion.
    let source = r#"
@ radix lane "air"
@ radix backward "loss_a_backward"
functio loss_a(tf32[5] x, tf32[5] y) → fractus {
    redde (x + y).media()
}

@ radix lane "air"
@ radix backward "loss_b_backward"
functio loss_b(tf32[5] a, tf32[5] b) → fractus {
    redde (a ⊙ b).media()
}
"#;

    let result = compile(
        &test_session_la(Target::MirLlvm),
        "backward.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "two backward functions should compile successfully:\n{:?}",
        result.diagnostics
    );

    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!(
            "expected llvm-text output with two backward companions, got: {:?}",
            result.output.as_ref().map(|_| "non-llvm-text output"),
        );
    };

    // Both companion names must appear in the LLVM output.
    assert!(
        output.code.contains("loss_a_backward"),
        "expected companion 'loss_a_backward' in llvm-text output:\n{}",
        output.code,
    );
    assert!(
        output.code.contains("loss_b_backward"),
        "expected companion 'loss_b_backward' in llvm-text output:\n{}",
        output.code,
    );

    // Both primal function names must still appear (backward is additive).
    assert!(
        output.code.contains("loss_a"),
        "expected primal 'loss_a' in llvm-text output:\n{}",
        output.code,
    );
    assert!(
        output.code.contains("loss_b"),
        "expected primal 'loss_b' in llvm-text output:\n{}",
        output.code,
    );
}

// ── Test 2: No backward annotation ──

#[test]
fn air_lane_program_without_backward_is_noop() {
    // An AIR-lane function without @ radix backward should compile
    // successfully but produce no companion function.
    let source = r#"
@ radix lane "air"
functio forward(tf32[5] x, tf32[5] y) → tf32[5] {
    redde x + y
}
"#;

    let result = compile(
        &test_session_la(Target::MirLlvm),
        "backward.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "AIR function without backward annotation should compile:\n{:?}",
        result.diagnostics
    );

    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!(
            "expected llvm-text output, got: {:?}",
            result.output.as_ref().map(|_| "non-llvm-text output"),
        );
    };

    // The primal function must be in the output.
    assert!(
        output.code.contains("forward"),
        "expected primal 'forward' in llvm-text output:\n{}",
        output.code,
    );

    // No companion with the backward-annotation naming convention should appear.
    // Without @ radix backward, no companion function is generated.
    let backward_hint = "forward_backward";
    assert!(
        !output.code.contains(backward_hint),
        "unexpected companion '{backward_hint}' should not appear without @ radix backward:\n{}",
        output.code,
    );
}

// ── Test 3: Mixed backward and non-backward ──

#[test]
fn air_lane_mixed_backward_and_non_backward() {
    // When one function has @ radix backward and another does not, only
    // the annotated function's companion should appear in the output.
    let source = r#"
@ radix lane "air"
@ radix backward "diff_backward"
functio diff(tf32[5] x, tf32[5] y) → fractus {
    redde (x + y).media()
}

@ radix lane "air"
functio plain(tf32[5] x) → tf32[5] {
    redde x
}
"#;

    let result = compile(
        &test_session_la(Target::MirLlvm),
        "backward.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "mixed backward/non-backward should compile:\n{:?}",
        result.diagnostics
    );

    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!(
            "expected llvm-text output, got: {:?}",
            result.output.as_ref().map(|_| "non-llvm-text output"),
        );
    };

    // The annotated companion name must appear.
    assert!(
        output.code.contains("diff_backward"),
        "expected companion 'diff_backward' in llvm-text output:\n{}",
        output.code,
    );

    // A companion for the non-backward function must NOT appear.
    let unexpected = "plain_backward";
    assert!(
        !output.code.contains(unexpected),
        "unexpected companion '{unexpected}' for function without @ radix backward:\n{}",
        output.code,
    );

    // Both primal functions must still be present.
    assert!(
        output.code.contains("diff"),
        "expected primal 'diff' in llvm-text output:\n{}",
        output.code,
    );
    assert!(
        output.code.contains("plain"),
        "expected primal 'plain' in llvm-text output:\n{}",
        output.code,
    );
}

// ── Test 4: Companion has unique function identity ──

#[test]
fn air_lane_backward_companion_has_unique_function_id() {
    // The companion function must have a distinct identity from the primal.
    // In the LLVM text output, this means both the primal and companion
    // appear as separate function definitions with distinct names.
    let source = r#"
@ radix lane "air"
@ radix backward "mean_backward"
functio mean(tf32[5] x, tf32[5] y) → fractus {
    redde (x + y).media()
}
"#;

    let result = compile(
        &test_session_la(Target::MirLlvm),
        "backward.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(result.success(), "backward function should compile:\n{:?}", result.diagnostics);

    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!(
            "expected llvm-text output, got: {:?}",
            result.output.as_ref().map(|_| "non-llvm-text output"),
        );
    };

    // Split the LLVM output at function definition boundaries.
    // Each `define @<name>(...)` marker starts a new function.
    let function_start = "define ";
    let function_definitions: Vec<&str> = output.code.split(function_start).collect();

    // Find the primal by matching its function definition.
    let primal_found = function_definitions
        .iter()
        .any(|section| section.starts_with("@mean(") || section.contains("@mean("));
    assert!(
        primal_found,
        "primal function 'mean' should appear as a distinct definition:\n{}",
        output.code,
    );

    // Find the companion by matching its function definition.
    let companion_found = function_definitions
        .iter()
        .any(|section| section.starts_with("@mean_backward(") || section.contains("@mean_backward("));
    assert!(
        companion_found,
        "companion 'mean_backward' should appear as a distinct definition:\n{}",
        output.code,
    );

    // Both names appear in the output (basic name check).
    assert!(output.code.contains("mean"), "expected primal name 'mean' in llvm-text output");
    assert!(
        output.code.contains("mean_backward"),
        "expected companion name 'mean_backward' in llvm-text output"
    );
}

// ── Test 5: Multi-op companion preserves bindings ──

#[test]
fn air_lane_backward_companion_preserves_bindings_in_mir() {
    // A backward-annotated function with multiple operations (add, multiply,
    // mean) should produce a companion with multiple gradient computations.
    // Compare against a single-op companion to verify substantive growth.
    let source = r#"
@ radix lane "air"
@ radix backward "single_backward"
functio single(tf32[5] x, tf32[5] y) → tf32[5] {
    redde x + y
}

@ radix lane "air"
@ radix backward "multi_backward"
functio multi(tf32[5] x, tf32[5] y) → fractus {
    redde ((x + y) ⊙ x).media()
}
"#;

    let result = compile(
        &test_session_la(Target::MirLlvm),
        "backward.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(result.success(), "multi-op backward should compile:\n{:?}", result.diagnostics);

    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!(
            "expected llvm-text output, got: {:?}",
            result.output.as_ref().map(|_| "non-llvm-text output"),
        );
    };

    // Both companion names must be present.
    assert!(
        output.code.contains("single_backward"),
        "expected companion 'single_backward' in llvm-text output:\n{}",
        output.code,
    );
    assert!(
        output.code.contains("multi_backward"),
        "expected companion 'multi_backward' in llvm-text output:\n{}",
        output.code,
    );

    // Split the LLVM output into function sections bounded by `define @`.
    // Each section starts with `@func_name(...) {` and runs until the next
    // `define @` or end of file.
    let sections: Vec<&str> = output.code.split("define ").collect();

    // Find the companion function sections.
    let single_section = sections
        .iter()
        .find(|s| s.contains("single_backward"))
        .expect("single_backward companion section not found");
    let multi_section = sections
        .iter()
        .find(|s| s.contains("multi_backward"))
        .expect("multi_backward companion section not found");

    // The multi-op companion (add → multiply → mean) must be substantially
    // larger than the single-op companion (add only) because it replays
    // more forward operations and computes more gradient paths.
    let multi_len = multi_section.len();
    let single_len = single_section.len();
    assert!(
        multi_len > single_len * 2,
        "multi-op companion ({multi_len} bytes) should be > 2x single-op companion ({single_len} bytes):\n\
         single section:\n{single_section}\n\n\
         multi section:\n{multi_section}",
    );
}

// -- Test 6: Companion call in incipit compiles through driver —

/// Prove that a backward-annotated AIR companion is source-callable:
/// the companion name resolves, FuncSig arity is validated, the
/// callee is recognized as callable at the typecheck level, and the
/// empty residual is source-visible as vacuum (Primitive::Vacuum).
///
/// The incipit constructs tensor values and calls the companion with
/// full arity (tensor params + `nil()` for vacuum residual +
/// upstream gradient). The empty residual is now Primitive::Vacuum in
/// the companion ABI, matching the Faber source type — full typecheck
/// succeeds.
///
/// Companion ABI:
///   `companion(tensor_params..., residual: vacuum, upstream: R) -> (grad_T1, ..., grad_TN)`
#[test]
fn companion_name_call_completes_through_driver() {
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

    let result = compile(
        &test_session_la(Target::MirLlvm),
        "backward.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "companion call with correct tensor args + vacuum residual should fully compile; diagnostics: {:?}",
        result.diagnostics
    );

    // Both primal and companion names must appear in LLVM output.
    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!(
            "expected llvm-text output with companion call, got: {:?}",
            result.output.as_ref().map(|_| "non-llvm-text output"),
        );
    };

    assert!(output.code.contains("mean"), "expected primal name 'mean' in llvm-text output");
    assert!(
        output.code.contains("mean_backward"),
        "expected companion name 'mean_backward' in llvm-text output"
    );
}

// ── Test 7: Interprocedural backward through driver ──

/// Prove that the production driver path
/// (`lower_mir_with_context_for_target` → `apply_air_backward_bundle` →
/// `generate_companions_bottom_up`) works end-to-end for an
/// interprocedural Call DAG: an annotated caller calls an unannotated
/// pure-AIR callee.  Both companions must appear in the LLVM-text
/// output.
///
/// Annotated companion (`caller_backward`) appears by user-declared
/// name.  Unannotated callee companions receive a `__backward_callee_N`
/// name from `generate_companions_bottom_up` interned through the
/// lowering interner; the test verifies structural presence by checking
/// that the LLVM text contains at least four distinct `define`'d
/// functions (caller, callee, caller_backward, callee companion)
/// rather than matching a specific string.
///
/// # Fixed
///
/// Originally blocked by per-function TypeId mismatch in gradient tuple
/// lookup.  Fixed by pre-interning gradient-tuple types in
/// `prepare_air_backward_bundle` and adding `Type::Tuple` support to
/// MIR field projection validation.
#[test]
fn interprocedural_call_emits_both_companions_through_driver() {
    let source = r#"
@ radix lane "air"
@ radix backward "caller_backward"
functio caller(tf32[5] x, tf32[5] w) → tf32[5] {
    redde x + callee(x, w)
}

@ radix lane "air"
functio callee(tf32[5] a, tf32[5] b) → tf32[5] {
    redde a + b
}
"#;

    let result = compile(
        &test_session_la(Target::MirLlvm),
        "backward.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "interprocedural backward should compile successfully:\n{:?}",
        result.diagnostics
    );

    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!(
            "expected llvm-text output with interprocedural companions, got: {:?}",
            result.output.as_ref().map(|_| "non-llvm-text output"),
        );
    };

    // Annotated companion must appear by its user-declared name.
    assert!(
        output.code.contains("caller_backward"),
        "expected annotated companion 'caller_backward' in llvm-text output:\n{}",
        output.code,
    );

    // Both primal functions must be present.
    assert!(
        output.code.contains("caller"),
        "expected primal 'caller' in llvm-text output:\n{}",
        output.code,
    );
    assert!(
        output.code.contains("callee"),
        "expected primal 'callee' in llvm-text output:\n{}",
        output.code,
    );

    // Unannotated callee companion: the name is `__backward_callee_N`
    // interned through the lowering interner (not a user-visible string).
    // Verify structural presence by counting `define ` function definitions.
    // We expect at least 4: caller, callee, caller_backward, and the callee
    // companion.
    let define_count = output.code.matches("define ").count();
    assert!(
        define_count >= 4,
        "expected at least 4 function definitions (caller, callee, caller_backward, \
         callee companion) but found {define_count}:\n{}",
        output.code,
    );
}
