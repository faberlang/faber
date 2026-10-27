//! Verification test for backward-generation emission.
//!
//! Confirms that a tensor AIR function annotated with `@ radix backward`
//! produces a backward companion function in emitted LLVM text, with the
//! expected pointer-typed parameters (unlike the scalar skip test which asserts
//! the companion is absent).

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

/// Tensor AIR functions annotated with `@ radix backward` must emit the
/// backward companion in compiled LLVM text output.
#[test]
fn tensor_air_emits_backward() {
    let source = r#"
@ radix lane "air"
@ radix backward "multiplica_backward"
functio multiplica(tf32[2] a, tf32[2] b) → tf32[2] {
    redde a ⊙ b
}
"#;

    let result = compile(&llvm_session(), "multiplica.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "tensor AIR backward compilation should succeed:\n{:?}",
        result.diagnostics,
    );

    // The backward companion MUST be emitted for tensor AIR functions.
    if let Some(radix_module::Output::LlvmText(output)) = result.output {
        // Assert the complete definition so a correctly named function with a
        // malformed (for example, void-typed) parameter does not satisfy the test.
        assert!(
            output
                .code
                .contains("define internal { ptr, ptr } @multiplica_backward(ptr %l0, ptr %l1, ptr %l3)"),
            "backward companion should have the expected LLVM signature:\n{}",
            output.code,
        );
    } else {
        panic!("expected llvm-text output, got: {:?}", result.output.as_ref().map(|_| "..."));
    }
}
