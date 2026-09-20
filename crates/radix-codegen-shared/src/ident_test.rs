use super::{output_resource_name, sanitize_ident, sanitize_rust_module_ident};

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
