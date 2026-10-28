//! The `faber field-modifier-codemod` seam resolves each file's reader pack.

use super::codemod_field_modifiers;
use clap::Parser;
use std::fs;
use std::path::Path;

#[test]
fn en_frontmatter_file_gets_var() {
    let source = "+++\nlocale = \"en\"\n+++\nclass Punctum {\n    int x\n}\n";
    let result =
        codemod_field_modifiers(Path::new("punctum.fab"), source).expect("codemod accepted");
    assert_eq!(result.inserted, 1);
    assert_eq!(
        result.source,
        "+++\nlocale = \"en\"\n+++\nclass Punctum {\n    mut int x\n}\n"
    );
}

#[test]
fn la_frontmatter_file_gets_varia() {
    let source = "+++\nlocale = \"la\"\n+++\ngenus Punctum {\n    numerus x\n}\n";
    let result =
        codemod_field_modifiers(Path::new("punctum.fab"), source).expect("codemod accepted");
    assert_eq!(result.inserted, 1);
    assert!(
        result.source.contains("    varia numerus x\n"),
        "{}",
        result.source
    );
}

const DRIFT_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nclass Punctum {\n    int x\n}\n";
const DONE_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nclass Punctum {\n    mut int x\n}\n";

fn run_codemod(args: &[&str]) -> std::process::Output {
    let mut full = vec!["field-modifier-codemod"];
    full.extend_from_slice(args);
    super::super::format_test::run_faber_capture(&full, None)
}

#[test]
fn cli_field_modifier_codemod_default_mode_is_report_only() {
    let cli = crate::cli::Cli::try_parse_from(["faber", "field-modifier-codemod", "main.fab"])
        .expect("parse field-modifier-codemod");
    let Some(crate::cli::Command::FieldModifierCodemod(args)) = cli.command else {
        panic!("expected field-modifier-codemod command");
    };
    assert!(!args.write && !args.check && !args.stdout);
}

#[test]
fn cli_field_modifier_codemod_modes_conflict() {
    for pair in [
        ["--write", "--check"],
        ["--write", "--stdout"],
        ["--check", "--stdout"],
    ] {
        let result =
            crate::cli::Cli::try_parse_from(["faber", "field-modifier-codemod", pair[0], pair[1]]);
        assert!(result.is_err(), "{pair:?} must conflict");
    }
}

#[test]
fn field_modifier_codemod_default_writes_nothing_and_exits_1_on_drift() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("drift.fab");
    fs::write(&path, DRIFT_SOURCE).expect("write fixture");
    fs::write(dir.path().join("done.fab"), DONE_SOURCE).expect("write done fixture");
    let root = dir.path().to_str().expect("utf8 path");

    for args in [vec![root], vec!["--check", root]] {
        let output = run_codemod(&args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {stdout}");
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 2, "{args:?}: {stdout}");
        assert!(lines[0].starts_with("would codemod "), "{stdout}");
        assert!(lines[0].ends_with("drift.fab"), "{stdout}");
        assert!(
            lines[1].starts_with("1 of 2 files would be codemodded; rerun with --write to apply"),
            "{stdout}"
        );
    }
    assert_eq!(fs::read_to_string(&path).expect("read back"), DRIFT_SOURCE);
}

#[test]
fn field_modifier_codemod_default_exits_0_when_nothing_would_change() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("done.fab");
    fs::write(&path, DONE_SOURCE).expect("write fixture");
    let output = run_codemod(&[path.to_str().expect("utf8 path")]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "1 files already codemodded"
    );
    assert_eq!(fs::read_to_string(&path).expect("read back"), DONE_SOURCE);
}

#[test]
fn field_modifier_codemod_write_changes_only_files_that_drift() {
    let dir = tempfile::tempdir().expect("create scratch");
    let drift = dir.path().join("drift.fab");
    fs::write(&drift, DRIFT_SOURCE).expect("write fixture");
    let clean = dir.path().join("done.fab");
    fs::write(&clean, DONE_SOURCE).expect("write clean fixture");
    let clean_mtime = fs::metadata(&clean)
        .and_then(|meta| meta.modified())
        .expect("clean mtime");

    let output = run_codemod(&["--write", dir.path().to_str().expect("utf8 path")]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(lines[0].starts_with("codemodded ") && lines[0].ends_with("drift.fab"));
    assert_eq!(lines[1], "codemodded 1 of 2 files");
    assert_eq!(fs::read_to_string(&drift).expect("read drift"), DONE_SOURCE);
    assert_eq!(fs::read_to_string(&clean).expect("read clean"), DONE_SOURCE);
    assert_eq!(
        fs::metadata(&clean)
            .and_then(|meta| meta.modified())
            .expect("clean mtime"),
        clean_mtime,
        "an unchanged file must not be touched"
    );
}

#[test]
fn field_modifier_codemod_counts_refused_files_and_never_writes_them() {
    let dir = tempfile::tempdir().expect("create scratch");
    // A lexically broken file the span rewriter refuses.
    let broken_source = "+++\nlocale = \"en\"\n+++\nclass Punctum {\n    \"unterminated\n";
    let broken = dir.path().join("broken.fab");
    fs::write(&broken, broken_source).expect("write broken fixture");
    let drift = dir.path().join("drift.fab");
    fs::write(&drift, DRIFT_SOURCE).expect("write fixture");
    let root = dir.path().to_str().expect("utf8 path");

    let report = run_codemod(&[root]);
    let report_out = String::from_utf8_lossy(&report.stdout);
    // A refused file is a source that does not lex or parse: a data error.
    assert_eq!(report.status.code(), Some(65), "{report_out}");
    assert!(
        report_out
            .lines()
            .last()
            .is_some_and(|line| line.ends_with(", 1 refused (nothing written)")),
        "{report_out}"
    );
    assert_eq!(
        fs::read_to_string(&drift).expect("read drift"),
        DRIFT_SOURCE
    );

    let write = run_codemod(&["--write", root]);
    let write_out = String::from_utf8_lossy(&write.stdout);
    assert_eq!(write.status.code(), Some(65), "{write_out}");
    assert_eq!(
        write_out.lines().last(),
        Some("codemodded 1 of 2 files, 1 refused (nothing written)")
    );
    assert_eq!(
        fs::read_to_string(&broken).expect("read broken"),
        broken_source,
        "a refused file is never written"
    );
}

#[test]
fn field_modifier_codemod_stdout_prints_one_document_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("drift.fab");
    fs::write(&path, DRIFT_SOURCE).expect("write fixture");
    let output = run_codemod(&["--stdout", path.to_str().expect("utf8 path")]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), DONE_SOURCE);
    assert_eq!(fs::read_to_string(&path).expect("read back"), DRIFT_SOURCE);
}
