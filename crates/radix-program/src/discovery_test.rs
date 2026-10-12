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

#[test]
fn sanitize_crate_name_replaces_unicode_letters_with_separators() {
    assert_eq!(sanitize_crate_name("café"), "caf");
}

#[test]
fn sanitize_crate_name_lowercases_ascii_and_replaces_unicode_letters() {
    assert_eq!(sanitize_crate_name("CAFÉ"), "caf");
}

#[test]
fn sanitize_crate_name_replaces_unicode_symbols() {
    assert_eq!(sanitize_crate_name("hello·world"), "hello-world");
}

#[test]
fn sanitize_crate_name_dots_only_falls_back_to_package() {
    assert_eq!(sanitize_crate_name("..."), "package");
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

#[test]
fn missing_package_input_names_path_base_and_manifest_artifact() {
    let base = Path::new("/tmp/faber-check-discoverability");
    let input = base.join("missing-package");

    let diagnostic = discover_package_with_base(&input, base).expect_err("missing package");

    assert_eq!(diagnostic.code, Some("PKG001"));
    assert_eq!(diagnostic.issue(), Some("package_path_missing"));
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "path")
            .map(|arg| arg.value.as_str()),
        Some(input.to_str().expect("UTF-8 input path"))
    );
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "base")
            .map(|arg| arg.value.as_str()),
        Some(base.to_str().expect("UTF-8 base path"))
    );
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "artifact")
            .map(|arg| arg.value.as_str()),
        Some("manifest")
    );
}

fn compiler_manifest_with_tables(tables: &str) -> FaberManifest {
    toml::from_str(&format!(
        "[package]\nname = \"demo\"\n\n[paths]\nentry = \"main.fab\"\n{tables}"
    ))
    .expect("manifest parses")
}

#[test]
fn compiler_manifest_rejects_unknown_target_table() {
    let manifest = compiler_manifest_with_tables("\n[target.wgsl]\nbindings = \"b.toml\"\n");
    let err = validate_compiler_manifest(&manifest, Path::new("faber.toml"))
        .expect_err("an unknown [target.<key>] must fail discovery validation");
    assert_eq!(err.issue(), Some("package_target_table_unknown"));
}

#[test]
fn compiler_manifest_accepts_target_spelling_table() {
    let manifest = compiler_manifest_with_tables("\n[target.wgsl-text]\nbindings = \"b.toml\"\n");
    validate_compiler_manifest(&manifest, Path::new("faber.toml"))
        .expect("a target-spelling table passes discovery validation");
}

fn manifest_temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "faber-discovery-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).expect("mkdir");
    path
}

#[test]
fn discover_package_reads_manifest_backed_source_root_and_entry() {
    let root = manifest_temp_dir("manifest");
    std::fs::write(
        root.join("faber.toml"),
        "[package]\nname = \"demo\"\n\n[paths]\nentry = \"main.fab\"\n",
    )
    .expect("write manifest");

    let spec = discover_package(&root).expect("discover manifest-backed package");

    assert!(spec.manifest_backed);
    assert_eq!(spec.package_root, root);
    assert_eq!(spec.source_root, root.join("src"));
    assert_eq!(spec.entry, root.join("src").join("main.fab"));
    assert!(spec.templates.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}
