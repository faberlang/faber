use super::*;

// ── sanitize_crate_name ───────────────────────────────────────────────────

#[test]
fn sanitize_crate_name_passes_through_simple_name() {
    assert_eq!(sanitize_crate_name("hello"), "hello");
}

#[test]
fn sanitize_crate_name_lowercases() {
    assert_eq!(sanitize_crate_name("HelloWorld"), "helloworld");
}

#[test]
fn sanitize_crate_name_preserves_hyphens_and_underscores() {
    assert_eq!(sanitize_crate_name("my-crate_name"), "my-crate_name");
}

#[test]
fn sanitize_crate_name_replaces_special_chars_with_hyphen() {
    assert_eq!(sanitize_crate_name("hello.world!"), "hello-world");
}

#[test]
fn sanitize_crate_name_trims_leading_and_trailing_separators() {
    assert_eq!(sanitize_crate_name("--hello--"), "hello");
}

#[test]
fn sanitize_crate_name_falls_back_to_package_when_all_removed() {
    assert_eq!(sanitize_crate_name("---"), "package");
    assert_eq!(sanitize_crate_name("!!!"), "package");
}

#[test]
fn sanitize_crate_name_handles_empty_string() {
    assert_eq!(sanitize_crate_name(""), "package");
}

#[test]
fn sanitize_crate_name_handles_whitespace_only() {
    assert_eq!(sanitize_crate_name("   "), "package");
}

#[test]
fn sanitize_crate_name_prefixes_with_p_when_starts_with_digit() {
    assert_eq!(sanitize_crate_name("123abc"), "p-123abc");
}

#[test]
fn sanitize_crate_name_prefixes_with_p_when_sanitized_result_starts_with_digit() {
    assert_eq!(sanitize_crate_name("_1name"), "p-1name");
}

#[test]
fn sanitize_crate_name_mixed_characters_and_digits() {
    assert_eq!(sanitize_crate_name("Foo_Bar-42"), "foo_bar-42");
}

#[test]
fn sanitize_crate_name_complex_with_special_characters() {
    assert_eq!(
        sanitize_crate_name("My@Awesome#Package$1.0"),
        "my-awesome-package-1-0"
    );
}

// ── BuildLayout::from_package_root ────────────────────────────────────────

#[test]
fn build_layout_from_package_root_sets_debug_binary() {
    let layout = BuildLayout::from_package_root("/tmp/my-pkg", "my-package");
    assert_eq!(
        layout.debug_binary,
        Path::new("/tmp/my-pkg/target/debug/my-package")
    );
}

#[test]
fn build_layout_from_package_root_sets_release_binary() {
    let layout = BuildLayout::from_package_root("/tmp/my-pkg", "my-package");
    assert_eq!(
        layout.release_binary,
        Path::new("/tmp/my-pkg/target/release/my-package")
    );
}

#[test]
fn build_layout_from_package_root_sets_generated_crate_paths() {
    let layout = BuildLayout::from_package_root("/tmp/my-pkg", "my-package");
    assert_eq!(
        layout.generated_crate_root,
        Path::new("/tmp/my-pkg/target/faber")
    );
    assert_eq!(
        layout.generated_cargo_manifest,
        Path::new("/tmp/my-pkg/target/faber/Cargo.toml")
    );
    assert_eq!(
        layout.generated_rust_entry,
        Path::new("/tmp/my-pkg/target/faber/src/main.rs")
    );
}

#[test]
fn build_layout_from_package_root_sets_manifest_path() {
    let layout = BuildLayout::from_package_root("/tmp/my-pkg", "my-pkg");
    assert_eq!(layout.manifest_path, Path::new("/tmp/my-pkg/faber.toml"));
}

#[test]
fn build_layout_from_package_root_normalizes_path() {
    let layout = BuildLayout::from_package_root("/tmp/./my-pkg/../my-pkg", "name");
    assert_eq!(layout.package_root, Path::new("/tmp/my-pkg"));
}

#[test]
fn build_layout_binary_name_returns_sanitized_name() {
    let layout = BuildLayout::from_package_root("/tmp/pkg", "My-Package");
    assert_eq!(layout.binary_name(), "my-package");
}
