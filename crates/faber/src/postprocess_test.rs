use super::{parse_eslint_output, render_normal, write_temp_rust_project_in};

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

/// One LOCALE002 note per file, printed ahead of the errors it explains: a
/// second keyword-level note for the same file is dropped, other files keep
/// their own note. Asserts codes, files, and issue names — never message text.
#[test]
fn normalize_locale002_keeps_one_note_per_file_first() {
    use radix::module::diagnostics::Diagnostic;

    let note = |file: &str| {
        Diagnostic::warning("note")
            .with_code("LOCALE002")
            .with_arg("issue", "source_reads_as_other_locale")
            .with_file(file.to_owned())
    };
    let diagnostics = vec![
        Diagnostic::error("unknown identifier")
            .with_code("SEM001")
            .with_file("a.fab"),
        note("a.fab"),
        Diagnostic::error("undefined name")
            .with_code("SEM008")
            .with_file("a.fab"),
        note("a.fab"),
        note("b.fab"),
    ];

    let normalized = radix::services::normalize_locale002_notes(diagnostics);

    let locale002: Vec<&str> = normalized
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("LOCALE002"))
        .map(|diagnostic| diagnostic.file.as_str())
        .collect();
    assert_eq!(locale002, vec!["a.fab", "b.fab"]);
    assert_eq!(normalized[0].code.as_deref(), Some("LOCALE002"));
    assert_eq!(
        normalized[0].issue().as_deref(),
        Some("source_reads_as_other_locale")
    );
    assert!(
        normalized
            .iter()
            .take(2)
            .all(|diagnostic| !diagnostic.is_error()),
        "notes come first: {normalized:?}"
    );
    let errors: Vec<&str> = normalized
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect();
    assert_eq!(errors, vec!["SEM001", "SEM008"]);
}

/// A wrong-locale note for a file whose frontmatter declares a locale is
/// dropped: the note must not claim the source "has no declared locale" when
/// it does. Notes for files without a declared locale survive. Asserts codes
/// and issue names — never message text.
#[test]
fn drop_declared_locale002_notes_suppresses_only_declared_sources() {
    use radix::module::diagnostics::Diagnostic;

    let guard = tempfile::tempdir().expect("tempdir");
    let declared = guard.path().join("declared.fab");
    std::fs::write(&declared, "+++\nlocale = \"en\"\n+++\n\nmain { print 1 }\n").expect("write");
    let undeclared = guard.path().join("undeclared.fab");
    std::fs::write(&undeclared, "main { print 1 }\n").expect("write");

    let note_for = |file: &std::path::Path| {
        Diagnostic::warning("note")
            .with_code("LOCALE002")
            .with_arg("issue", "source_reads_as_other_locale")
            .with_file(file.display().to_string())
    };
    let diagnostics = vec![note_for(&declared), note_for(&undeclared)];

    let kept = super::drop_declared_locale002_notes(diagnostics);

    let files: Vec<&str> = kept
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("LOCALE002"))
        .map(|diagnostic| diagnostic.file.as_str())
        .collect();
    assert_eq!(files, vec![undeclared.display().to_string().as_str()]);
    assert_eq!(
        kept[0].issue().as_deref(),
        Some("source_reads_as_other_locale")
    );
}

fn en_pack() -> radix::module::locale::LocalePack {
    let path = radix::module::locale::workspace_locale_pack_path("en");
    radix::module::locale::LocalePack::from_toml_path(path).expect("load en pack")
}

fn llvm_host_link_failure(detail: &str) -> radix::module::diagnostics::Diagnostic {
    crate::package_diagnostic_error("clang link failed")
        .with_arg("issue", "llvm_host_link_failed")
        .with_arg("path", "out/bin")
        .with_arg("detail", detail)
}

/// A package refusal with a pack row reaches the user through the row: the
/// rendering differs from the compact identity and follows the `detail` arg.
#[test]
fn normal_mode_renders_the_pack_row_of_a_package_refusal() {
    let pack = en_pack();
    let diag = llvm_host_link_failure("ld: library not found");

    let rendered = render_normal(&diag, Some(&pack));

    assert_ne!(rendered, radix::module::diagnostics::render_plain(&diag));
    assert_ne!(
        rendered,
        render_normal(
            &llvm_host_link_failure("a different linker output"),
            Some(&pack)
        )
    );
}

#[test]
fn normal_mode_keeps_rowless_and_non_package_diagnostics_compact() {
    let pack = en_pack();
    let rowless =
        crate::package_diagnostic_error("raw prose").with_arg("issue", "no_such_issue_row");
    let other = radix::module::diagnostics::Diagnostic::error("raw prose")
        .with_code("SEM001")
        .with_arg("issue", "llvm_host_link_failed");

    for diag in [&rowless, &other] {
        assert_eq!(
            render_normal(diag, Some(&pack)),
            radix::module::diagnostics::render_plain(diag)
        );
    }
    // Without a pack the package refusal also stays compact.
    let diag = llvm_host_link_failure("x");
    assert_eq!(
        render_normal(&diag, None),
        radix::module::diagnostics::render_plain(&diag)
    );
}
