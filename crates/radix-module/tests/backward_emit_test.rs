//! Verification test for backward-generation emission.
//!
//! Confirms that a tensor AIR function annotated with `@ radix backward`
//! DOES produce a backward companion function in emitted output (unlike
//! the scalar skip test which asserts the companion is absent), and that
//! the emitted LLVM IR assembles with `llvm-as` — a name-only assertion
//! passes on IR that cannot be assembled (regression: the companion used to
//! carry a `void`-typed parameter).

// Lane-gated: exercises the AIR backward lane (`device`) emitting LLVM text
// (`mir-llvm`); a core-only build excludes both.
#![cfg(all(feature = "device", feature = "mir-llvm"))]

use radix_module::codegen::Target;
use radix_module::driver::{compile, Config, Session, ModuleCompileRequest};

fn llvm_session() -> Session {
    Session::new(Config::new().with_target(Target::MirLlvm))
}

/// Assemble emitted LLVM text with `llvm-as`: try the PATH binary first,
/// then the Homebrew LLVM install (`/opt/homebrew/opt/llvm/bin/llvm-as`).
/// Skips gracefully when no `llvm-as` is available on the machine. The
/// emitted IR must be valid — a backward companion that only spells the
/// right symbol name is not enough.
fn assert_llvm_as_assembles(code: &str, label: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let candidates = ["llvm-as".to_owned(), "/opt/homebrew/opt/llvm/bin/llvm-as".to_owned()];
    let Some(binary) = candidates.iter().find(|binary| {
        Command::new(binary)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }) else {
        eprintln!("{label}: llvm-as not available; skipping assembly check");
        return;
    };

    let mut child = Command::new(binary)
        .arg("-o")
        .arg("/dev/null")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{label}: spawn {binary}: {error}"));
    child
        .stdin
        .take()
        .expect("llvm-as stdin")
        .write_all(code.as_bytes())
        .expect("write llvm ir");
    let output = child.wait_with_output().expect("wait llvm-as");
    assert!(
        output.status.success(),
        "{label}: emitted LLVM IR does not assemble:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Tensor AIR functions annotated with `@ radix backward` must emit the
/// backward companion in compiled LLVM text output.
#[test]
fn tensor_air_emits_backward() {
    let source = r#"
@ radix lane "air"
@ radix backward "multiplica_backward"
functio multiplica(tf32[2] a, tf32[2] b) → tf32[2] {
    redde a.multiplica(b)
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
        assert!(
            output.code.contains("multiplica_backward"),
            "backward companion should be emitted for tensor AIR function:\n{}",
            output.code,
        );
        // And the whole module must be valid LLVM — `llvm-as` exit 0.
        assert_llvm_as_assembles(&output.code, "multiplica-backward");
    } else {
        panic!("expected llvm-text output, got: {:?}", result.output.as_ref().map(|_| "..."));
    }
}
