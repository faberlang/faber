use super::{module_segments_for_file, sanitize_rust_module_ident};
use std::path::Path;

#[test]
fn sanitize_maps_worktree_hyphen_slug_to_valid_ident() {
    assert_eq!(sanitize_rust_module_ident("faber-hir-v1"), "faber_hir_v1");
    assert_eq!(
        sanitize_rust_module_ident("factory/faber-hir-v1"),
        "factory_faber_hir_v1"
    );
    assert_eq!(sanitize_rust_module_ident("echo"), "echo");
    assert_eq!(sanitize_rust_module_ident("123pkg"), "m_123pkg");
    assert_eq!(sanitize_rust_module_ident("---"), "m");
    assert_eq!(sanitize_rust_module_ident(""), "m");
    assert_eq!(sanitize_rust_module_ident("gnu"), "gnu");
}

#[test]
fn module_segments_sanitize_outside_source_root_absolute_hyphen_path() {
    // Outside-package shared files (e.g. coreutils common/) fall back to the
    // absolute path when strip_prefix fails. Hyphenated worktree slugs must still
    // emit legal Rust module segments.
    let source_root = Path::new(
        "/Users/ianzepp/work/faberlang/worktrees/faber-hir-v1/examples/coreutils/packages/echo",
    );
    let file = Path::new(
        "/Users/ianzepp/work/faberlang/worktrees/faber-hir-v1/examples/coreutils/common/gnu/format.fab",
    );
    let segments = module_segments_for_file(source_root, file, None);
    assert!(
        segments
            .iter()
            .all(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')),
        "every segment must be a Rust-safe ident, got {segments:?}"
    );
    assert!(
        segments.iter().any(|s| s == "faber_hir_v1"),
        "expected sanitized worktree slug in {segments:?}"
    );
    assert!(
        !segments.iter().any(|s| s.contains('-')),
        "no raw hyphens allowed in {segments:?}"
    );
    assert_eq!(segments.last().map(String::as_str), Some("format"));
}
