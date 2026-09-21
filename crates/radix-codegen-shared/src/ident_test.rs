use super::{
    SCALAR_LOOP_CANDIDATES, SCALAR_LOOP_FALLBACK, VALUE_JOIN_CANDIDATES, VALUE_JOIN_FALLBACK,
    output_resource_name, sanitize_ident, sanitize_rust_module_ident, unclaimed_name,
};

#[test]
fn sanitize_ident_sanitizes_and_falls_back() {
    assert_eq!(sanitize_ident("add_f32", "x"), "add_f32");
    assert_eq!(sanitize_ident("2bad", "x"), "_2bad");
    assert_eq!(sanitize_ident("a-b.c", "x"), "a_b_c");
    assert_eq!(sanitize_ident("", "kernel"), "kernel");
}

// ── S-B2 shared module-naming policy ─────────────────────────────────────

/// Hostile segment text (raw import-path payloads) maps to identifier-only
/// output; the same policy derives `pub mod` names and use-line segments.
#[test]
fn sanitize_rust_module_ident_maps_hostile_segments_to_identifier_only() {
    assert_eq!(sanitize_rust_module_ident("echo"), "echo");
    assert_eq!(sanitize_rust_module_ident("faber-hir-v1"), "faber_hir_v1");
    // S-B2 fixture shape: quote, semicolon, and space must never survive —
    // pre-policy this text would land raw inside `use crate::…;`.
    assert_eq!(sanitize_rust_module_ident("x\"; static I"), "x___static_I");
    assert_eq!(sanitize_rust_module_ident("123pkg"), "m_123pkg");
    assert_eq!(sanitize_rust_module_ident("---"), "m");
    assert_eq!(sanitize_rust_module_ident(""), "m");
}

#[test]
fn output_resource_name_uses_output_buffer_when_param_is_output() {
    assert_eq!(output_resource_name("output"), "output_buffer");
    assert_eq!(output_resource_name("data"), "output");
    assert_eq!(output_resource_name("input"), "output");
    assert_eq!(output_resource_name("x"), "output");
}

// ── S06-1 shared candidate-claim law ──────────────────────────────────────

/// The claim law: the first candidate that shadows none of the kernel
/// parameter, output buffer, or index expression.
#[test]
fn unclaimed_name_returns_first_unclaimed_candidate() {
    assert_eq!(
        unclaimed_name(
            VALUE_JOIN_CANDIDATES,
            "x",
            "output",
            "id",
            VALUE_JOIN_FALLBACK
        ),
        "faber_joined"
    );
    assert_eq!(
        unclaimed_name(
            SCALAR_LOOP_CANDIDATES,
            "x",
            "output",
            "id",
            SCALAR_LOOP_FALLBACK
        ),
        "faber_loop"
    );
}

#[test]
fn unclaimed_name_skips_candidates_claimed_by_bindings() {
    // The parameter claims `faber_joined`, the output buffer claims
    // `faber_joined_value`, the index claims nothing → third candidate.
    assert_eq!(
        unclaimed_name(
            VALUE_JOIN_CANDIDATES,
            "faber_joined",
            "faber_joined_value",
            "id",
            VALUE_JOIN_FALLBACK
        ),
        "faber_joined_scalar"
    );
    // Any binding shape can claim a candidate; here the index does.
    assert_eq!(
        unclaimed_name(
            SCALAR_LOOP_CANDIDATES,
            "x",
            "output",
            "faber_loop",
            SCALAR_LOOP_FALLBACK
        ),
        "faber_loop_value"
    );
}

#[test]
fn unclaimed_name_falls_back_when_every_candidate_is_claimed() {
    assert_eq!(
        unclaimed_name(
            VALUE_JOIN_CANDIDATES,
            "faber_joined",
            "faber_joined_value",
            "faber_joined_scalar",
            VALUE_JOIN_FALLBACK
        ),
        "faber_joined_result"
    );
    assert_eq!(
        unclaimed_name(
            SCALAR_LOOP_CANDIDATES,
            "faber_loop",
            "faber_loop_value",
            "faber_loop_scalar",
            SCALAR_LOOP_FALLBACK
        ),
        "faber_loop_result"
    );
}

/// The shared lists are pinned to the literals both emitters hardcoded, so
/// moving the law into this crate cannot move emitted text (B4B-7
/// invariant: byte-for-byte preservation).
#[test]
fn candidate_lists_pin_the_emitter_literals() {
    assert_eq!(
        VALUE_JOIN_CANDIDATES,
        ["faber_joined", "faber_joined_value", "faber_joined_scalar"].as_slice()
    );
    assert_eq!(VALUE_JOIN_FALLBACK, "faber_joined_result");
    assert_eq!(
        SCALAR_LOOP_CANDIDATES,
        ["faber_loop", "faber_loop_value", "faber_loop_scalar"].as_slice()
    );
    assert_eq!(SCALAR_LOOP_FALLBACK, "faber_loop_result");
}
