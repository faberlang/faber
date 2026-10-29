//! BB-6R oracle: bounded `octeti<N>` identity, D7 bridges, O(1) byte index.

use super::test_support::{
    analyze_and_get_diagnostics_la, compile_and_get_result_la, compile_for_target_and_get_result_la,
};
use crate::codegen::Target;

fn rust_code(result: crate::CompileResult) -> String {
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };
    output.code
}

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_octeti_is_not_bare_octeti_and_keeps_n_in_emit() {
    let result = compile_and_get_result_la(
        r"functio probe(lista<b8, magnitudo 8> bounded, lista<b8> unbounded) → vacuum {
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

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_octeti_scalar_index_is_o1_live_byte() {
    let result = compile_and_get_result_la(
        r"functio probe(lista<b8, magnitudo 8> xs) → u8 {
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

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_octeti_conversions_follow_d7() {
    let diagnostics = analyze_and_get_diagnostics_la(
        r"typus OctetiOcto = lista<b8, magnitudo 8>

functio bridges(lista<b8, magnitudo 8> bounded, lista<b8> unbounded) → vacuum {
    sit lista<b8> widened ← bounded ↦ lista<b8>
    sit OctetiOcto narrowed ← unbounded ↦ OctetiOcto
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

    let implicit = analyze_and_get_diagnostics_la(
        r"functio assign(lista<b8> unbounded) → vacuum {
    sit lista<b8, magnitudo 8> bounded ← unbounded
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

    let result = compile_and_get_result_la(
        r"functio bridges(lista<b8, magnitudo 8> bounded, lista<b8> unbounded) → vacuum {
    sit lista<b8> widened ← bounded ↦ lista<b8>
    sit lista<b8, magnitudo 8> narrowed ← unbounded ↦ lista<b8, magnitudo 8>
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
        code.contains("try_from_octeti") && code.contains("list<b8> to bounded list<b8> conversion failed"),
        "unbounded → bounded must fail closed when len > N, got: {code}"
    );
}

/// TypeScript carries bounded octeti as the plain byte array with the capacity
/// erased at the type and checked at conversions.
#[cfg(feature = "hir-ts")]
#[test]
fn bounded_octeti_is_carried_on_typescript() {
    let source = r"functio probe(lista<b8, magnitudo 8> xs) → u8 {
    redde xs[0]
}

incipit {
    nota 0
}";
    let result = compile_for_target_and_get_result_la(source, Target::HirTypeScript);
    assert!(
        result.success(),
        "TypeScript must carry bounded octeti: {:?}",
        result.diagnostics
    );
    let emitted = match &result.output {
        Some(crate::Output::TypeScript(output)) => output.code.as_str(),
        _ => "",
    };
    assert!(
        emitted.contains("p_xs: Array<number>"),
        "bounded octeti must be the byte array carrier: {emitted}"
    );
}

/// Go carries bounded octeti as a plain byte slice with the capacity erased at
/// the type and checked at conversions (need, sb425): it no longer
/// declines with `go_type_unsupported`.
#[cfg(feature = "hir-go")]
#[test]
fn bounded_octeti_is_carried_on_go() {
    let source = r"functio probe(lista<b8, magnitudo 8> xs) → u8 {
    redde xs[0]
}

incipit {
    nota 0
}";
    let result = compile_for_target_and_get_result_la(source, Target::HirGo);
    assert!(result.success(), "Go must carry bounded octeti: {:?}", result.diagnostics);
    let code = match &result.output {
        Some(crate::Output::Go(output)) => output.code.as_str(),
        _ => "",
    };
    assert!(
        code.contains("func probe(p_xs []byte)"),
        "bounded octeti must be a plain Go byte slice parameter, got: {code}"
    );
}

/// An octeti to tensor conversion whose extent is a size parameter that no
/// value binding supplies on TypeScript is declined by name, never emitted
/// with a scalar shape. A tensor parameter whose axis names the size is the
/// witness the entry scan binds it from, so that form emits.
#[cfg(feature = "hir-ts")]
#[test]
fn octeti_to_tensor_with_an_unbound_size_extent_is_declined_on_typescript() {
    // No tensor parameter witnesses `N`, so no value binding supplies its
    // extent: the octeti-to-tensor conversion must decline by name. (A
    // witnessing axis binds the size and the conversion emits; that case is
    // covered by the emission tests.)
    let source = r"functio decode<magnitudo N>(lista<b8> raw) → tensor<f32, [N]> ⇥ textus {
    redde raw ↦ tensor<f32, [N]> via Le
}

incipit {
    nota 0
}";
    let result = compile_for_target_and_get_result_la(source, Target::HirTypeScript);
    assert!(!result.success(), "an unbound extent must not emit");
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.is_error() && diagnostic.issue() == Some("ts_list_b8_to_tensor_shape_unresolved")
        }),
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-ts")]
#[test]
fn octeti_to_tensor_with_a_witnessed_size_extent_emits_on_typescript() {
    let source = r"functio decode<magnitudo N>(tensor<f32, [N]> proto, lista<b8> raw) → tensor<f32, [N]> ⇥ textus {
    redde raw ↦ tensor<f32, [N]> via Le
}

incipit {
    nota 0
}";
    let result = compile_for_target_and_get_result_la(source, Target::HirTypeScript);
    assert!(
        result.success(),
        "a size parameter bound from a tensor parameter axis must emit: {:?}",
        result.diagnostics
    );
}
