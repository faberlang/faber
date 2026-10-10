//! BB-6R oracle: bounded `octeti<N>` identity, D7 bridges, O(1) byte index.

use super::test_support::{analyze_and_get_diagnostics, compile_and_get_result, compile_for_target_and_get_result};
use crate::codegen::Target;

fn rust_code(result: crate::CompileResult) -> String {
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };
    output.code
}

#[test]
fn bounded_octeti_is_not_bare_octeti_and_keeps_n_in_emit() {
    let result = compile_and_get_result(
        r"functio probe(octeti<8> bounded, octeti unbounded) → vacuum {
    nota bounded
    nota unbounded
}

incipit {
    nota 0
}",
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = rust_code(result);
    assert!(
        code.contains("faber::OctetiN<8>"),
        "bounded octeti must retain N in Rust type, got: {code}"
    );
    assert!(code.contains("Vec<u8>"), "bare octeti emit must remain Vec<u8>, got: {code}");
    assert!(
        !code.contains("/* unsupported bounded buffer */"),
        "bounded octeti must not drop N in emit, got: {code}"
    );
}

#[test]
fn bounded_octeti_scalar_index_is_o1_live_byte() {
    let result = compile_and_get_result(
        r"functio probe(octeti<8> xs) → u8 {
    redde xs[0]
}

incipit {
    nota 0
}",
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = rust_code(result);
    assert!(
        code.contains("faber::OctetiN<8>"),
        "bounded octeti index must keep N, got: {code}"
    );
    assert!(
        code.contains(".get(0usize)") && code.contains("faber::Trap::OctetiIndex"),
        "bounded octeti scalar index must use the carrier O(1) prefix get/trap, got: {code}"
    );
    assert!(
        !code.contains(".copied()"),
        "OctetiN::get already returns the live byte; spare slots must not be copied, got: {code}"
    );
}

#[test]
fn bounded_octeti_conversions_follow_d7() {
    let diagnostics = analyze_and_get_diagnostics(
        r"typus OctetiOcto = octeti<8>

functio bridges(octeti<8> bounded, octeti unbounded) → vacuum {
    fixum octeti widened ← bounded ↦ octeti
    fixum OctetiOcto narrowed ← unbounded ↦ OctetiOcto
    nota widened
    nota narrowed
}

incipit {
    nota 0
}",
    );
    assert!(
        !diagnostics.iter().any(crate::Diagnostic::is_error),
        "diagnostics: {diagnostics:?}"
    );

    let implicit = analyze_and_get_diagnostics(
        r"functio assign(octeti unbounded) → vacuum {
    fixum octeti<8> bounded ← unbounded
    nota bounded
}

incipit {
    nota 0
}",
    );
    assert!(
        implicit.iter().any(crate::Diagnostic::is_error),
        "octeti<8> ≠ octeti: implicit assignment must fail, got: {implicit:?}"
    );

    let result = compile_and_get_result(
        r"functio bridges(octeti<8> bounded, octeti unbounded) → vacuum {
    fixum octeti widened ← bounded ↦ octeti
    fixum octeti<8> narrowed ← unbounded ↦ octeti<8>
    nota widened
    nota narrowed
}

incipit {
    nota 0
}",
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = rust_code(result);
    assert!(
        code.contains(".to_octeti()"),
        "bounded → unbounded must copy the valid prefix, got: {code}"
    );
    assert!(
        code.contains("try_from_octeti") && code.contains("octeti to bounded octeti conversion failed"),
        "unbounded → bounded must fail closed when len > N, got: {code}"
    );
}

#[test]
fn bounded_octeti_rejects_non_rust_targets_fail_closed() {
    let source = r"functio probe(octeti<8> xs) → u8 {
    redde xs[0]
}

incipit {
    nota 0
}";
    for target in [Target::HirTypeScript] {
        let result = compile_for_target_and_get_result(source, target);
        assert!(!result.success(), "target {target:?} must reject bounded octeti");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.is_error() && diagnostic.issue().is_some() }),
            "target {target:?} must carry a structured error: {:?}",
            result.diagnostics
        );
        let emitted = match &result.output {
            Some(crate::Output::TypeScript(output)) => output.code.as_str(),
            _ => "",
        };
        assert!(
            !emitted.contains("Uint8Array"),
            "target {target:?} must not approximate octeti<N> as Uint8Array: {emitted}"
        );
    }
}

/// Go carries bounded octeti as a plain byte slice with the capacity erased at
/// the type and checked at conversions (need, sb425): it no longer
/// declines with `go_type_unsupported`.
#[test]
fn bounded_octeti_is_carried_on_go() {
    let source = r"functio probe(octeti<8> xs) → u8 {
    redde xs[0]
}

incipit {
    nota 0
}";
    let result = compile_for_target_and_get_result(source, Target::HirGo);
    assert!(result.success(), "Go must carry bounded octeti: {:?}", result.diagnostics);
    let code = match &result.output {
        Some(crate::Output::Go(output)) => output.code.as_str(),
        _ => "",
    };
    assert!(
        code.contains("func probe(xs []byte)"),
        "bounded octeti must be a plain Go byte slice parameter, got: {code}"
    );
}
