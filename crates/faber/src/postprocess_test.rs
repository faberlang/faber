use super::{parse_eslint_output, write_temp_rust_project_in};

/// A preplanted directory or symlink at a candidate temp-root name must be
/// neither written through nor clobbered: the lint temp root is created
/// exclusively under a nonce name.
#[test]
fn lint_temp_root_is_exclusive_and_ignores_planted_entries() {
    let guard = tempfile::tempdir().expect("tempdir");
    let parent = guard.path();
    let sentinel = parent.join("sentinel");
    std::fs::create_dir(&sentinel).expect("sentinel dir");
    let planted_dir = parent.join("planted-dir");
    std::fs::create_dir(&planted_dir).expect("planted dir");
    #[cfg(unix)]
    let planted_symlink = parent.join("planted-symlink");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&sentinel, &planted_symlink).expect("symlink");

    let code = "fn main() {}\n";
    let (temp_dir, main_rs) = write_temp_rust_project_in(parent, code).expect("temp rust project");

    let meta = std::fs::symlink_metadata(&temp_dir).expect("root metadata");
    assert!(meta.file_type().is_dir(), "temp root must be a directory");
    assert!(
        !meta.file_type().is_symlink(),
        "temp root must not be a symlink"
    );
    assert!(temp_dir.starts_with(parent));
    assert_ne!(temp_dir, planted_dir);
    #[cfg(unix)]
    assert_ne!(
        std::fs::canonicalize(&temp_dir).expect("canonical root"),
        std::fs::canonicalize(&sentinel).expect("canonical sentinel"),
        "temp root must not resolve into the symlink target"
    );

    assert_eq!(main_rs, temp_dir.join("src").join("main.rs"));
    assert_eq!(std::fs::read_to_string(&main_rs).expect("main.rs"), code);
    assert!(temp_dir.join("Cargo.toml").is_file());

    assert!(
        std::fs::read_dir(&sentinel)
            .expect("sentinel")
            .next()
            .is_none(),
        "nothing may be written through the planted symlink"
    );
    assert!(
        std::fs::read_dir(&planted_dir)
            .expect("planted dir")
            .next()
            .is_none(),
        "the planted directory must not be clobbered or written into"
    );

    assert!(
        temp_dir
            .file_name()
            .expect("root name")
            .to_string_lossy()
            .starts_with(".faber-lint.tmp-"),
        "temp root must use the exclusive nonce scheme, not a predictable name"
    );
}

#[test]
fn write_text_artifact_creates_parent_directories_and_writes_contents() {
    let guard = tempfile::tempdir().expect("tempdir");
    let path = guard.path().join("nested/deeper/out.rs");

    super::write_text_artifact(&path, "fn main() {}\n");

    assert_eq!(
        std::fs::read_to_string(&path).expect("read artifact"),
        "fn main() {}\n"
    );
}

#[test]
fn eslint_output_uses_fixed_source_when_present() {
    let output = r#"[{"filePath":"main.ts","output":"const value = 1;\n"}]"#;

    let fixed = parse_eslint_output("const value=1;\n", output).expect("valid ESLint output");

    assert_eq!(fixed, "const value = 1;\n");
}

#[test]
fn eslint_output_preserves_source_when_no_fix_was_needed() {
    let source = "const value = 1;\n";
    let output = r#"[{"filePath":"main.ts","messages":[]}]"#;

    let fixed = parse_eslint_output(source, output).expect("valid ESLint output");

    assert_eq!(fixed, source);
}

#[test]
fn eslint_output_rejects_unusable_json() {
    let invalid = parse_eslint_output("source", "not json").expect_err("invalid JSON");
    assert!(invalid.contains("invalid JSON"), "{invalid}");

    let wrong_shape = parse_eslint_output("source", "{}").expect_err("wrong JSON shape");
    assert!(wrong_shape.contains("must be an array"), "{wrong_shape}");

    let wrong_output_type =
        parse_eslint_output("source", r#"[{"filePath":"main.ts","output":null}]"#)
            .expect_err("non-string fixed output");
    assert!(
        wrong_output_type.contains("output field was not a string"),
        "{wrong_output_type}"
    );
}
