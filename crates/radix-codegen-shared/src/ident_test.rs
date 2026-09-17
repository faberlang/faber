use super::{output_resource_name, sanitize_ident};

#[test]
fn sanitize_ident_sanitizes_and_falls_back() {
    assert_eq!(sanitize_ident("add_f32", "x"), "add_f32");
    assert_eq!(sanitize_ident("2bad", "x"), "_2bad");
    assert_eq!(sanitize_ident("a-b.c", "x"), "a_b_c");
    assert_eq!(sanitize_ident("", "kernel"), "kernel");
}

#[test]
fn output_resource_name_uses_output_buffer_when_param_is_output() {
    assert_eq!(output_resource_name("output"), "output_buffer");
    assert_eq!(output_resource_name("data"), "output");
    assert_eq!(output_resource_name("input"), "output");
    assert_eq!(output_resource_name("x"), "output");
}
