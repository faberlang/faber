//! BB-5R oracle: bounded `textus<N>` identity, scalar access, and D7 bridges.

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
fn bounded_textus_scalar_index_emits_o1_ascii_one() {
    let result = compile_and_get_result_la(
        r"functio probe(textus<32> xs) → ascii<1> {
    redde xs[0]
}

incipit {
    nota 0
}",
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = rust_code(result);
    assert!(
        code.contains("faber::TextusN<32>"),
        "bounded textus must retain N in Rust type, got: {code}"
    );
    assert!(
        code.contains(".get(0usize)") && code.contains("textus index trap"),
        "bounded textus scalar index must use the carrier O(1) get/trap path, got: {code}"
    );
    assert!(
        !code.contains(".chars().nth"),
        "bounded textus scalar index must not walk chars, got: {code}"
    );
}

#[test]
fn bounded_textus_conversions_follow_d7() {
    let diagnostics = analyze_and_get_diagnostics_la(
        r"typus TextusOcto = textus<8>

functio bridges(textus<8> bounded, textus unbounded) → vacuum {
    fixum textus widened ← bounded ↦ textus
    fixum TextusOcto narrowed ← unbounded ↦ TextusOcto
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
}

#[test]
fn bounded_textus_rejects_non_rust_targets_fail_closed() {
    let source = r"functio probe(textus<32> xs) → ascii<1> {
    redde xs[0]
}

incipit {
    nota 0
}";
    for target in [Target::HirTypeScript] {
        let result = compile_for_target_and_get_result_la(source, target);
        assert!(!result.success(), "target {target:?} must reject bounded textus");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.is_error() && diagnostic.issue().is_some() }),
            "target {target:?} must carry a structured error: {:?}",
            result.diagnostics
        );
    }
}

/// Go carries bounded text as plain Go text with the capacity erased at the
/// type and checked at conversions (need, sb425): it no longer
/// declines with `go_type_unsupported`.
#[cfg(feature = "hir-go")]
#[test]
fn bounded_textus_is_carried_on_go() {
    let source = r"functio probe(textus<32> xs) → ascii<1> {
    redde xs[0]
}

incipit {
    nota 0
}";
    let result = compile_for_target_and_get_result_la(source, Target::HirGo);
    assert!(result.success(), "Go must carry bounded textus: {:?}", result.diagnostics);
    let code = match &result.output {
        Some(crate::Output::Go(output)) => output.code.as_str(),
        _ => "",
    };
    assert!(
        code.contains("func probe(p_xs string)"),
        "bounded textus must be a plain Go string parameter, got: {code}"
    );
}
