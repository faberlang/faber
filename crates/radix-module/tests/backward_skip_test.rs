//! Verification tests for backward-generation skip predicates.
//!
//! These guard against regressions in the scalar AIR backward skip (Finding 2):
//! functions with non-tensor params should not attempt backward generation.
//!
//! The Transpose/Fill skip (Finding 3) is tested via unit tests in
//! `driver::backward_skip_unit_tests` because those methods are rejected by
//! the AIR lane's semantic analysis before reaching `detect_backward_op`.

// Lane-gated: exercises the AIR backward lane (`device`) emitting LLVM text
// (`mir-llvm`); a core-only build excludes both.
#![cfg(all(feature = "device", feature = "mir-llvm"))]

use radix_module::codegen::Target;
use radix_module::driver::{Config, ModuleCompileRequest, Session, compile};

fn llvm_session() -> Session {
    Session::new(
        Config::new()
            .with_latin_locale()
            .with_target(Target::MirLlvm),
    )
}

/// Scalar AIR functions should not trigger backward generation even when
/// annotated with `@ radix backward`.  Compilation must succeed (no crash,
/// no spurious Mul-default diagnostics), and the backward companion name
/// must not appear in the emitted LLVM text.
#[test]
fn scalar_air_skips_backward_generation() {
    let source = r#"
@ radix lane "air"
@ radix backward "scalar_backward"
functio add_numeri(i32 a, i32 b) → numerus {
    redde a + b
}
"#;

    let result = compile(&llvm_session(), "scalar.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "scalar AIR backward compilation should succeed:\n{:?}",
        result.diagnostics,
    );

    // The backward companion must NOT be emitted — it was skipped.
    if let Some(radix_module::Output::LlvmText(output)) = result.output {
        assert!(
            !output.code.contains("scalar_backward"),
            "scalar backward companion should not be emitted:\n{}",
            output.code,
        );
    } else {
        panic!("expected llvm-text output, got: {:?}", result.output.as_ref().map(|_| "..."));
    }
}
