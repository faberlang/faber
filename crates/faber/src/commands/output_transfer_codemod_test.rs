//! Focused proof for the output-transfer codemod command.

use super::{DeclineReason, codemod_single_doc};
use clap::Parser;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str, source: &str) -> (PathBuf, String) {
    let path = std::env::temp_dir().join(format!("faber-output-transfer-codemod-{name}.fab"));
    let _ = fs::remove_file(&path);
    fs::write(&path, source).expect("write codemod fixture");
    (path, source.to_owned())
}

#[test]
fn accepts_latin_multivalue_statement_and_emits_one_transfer_header() {
    let (path, _) = fixture(
        "latin-multi",
        r#"+++
locale = "la"
+++
incipit {
    nota 7, "salve"
}
"#,
    );
    let report = codemod_single_doc(&path, &fs::read_to_string(&path).expect("read fixture"))
        .expect("codemod accepted source");
    let _ = fs::remove_file(&path);

    assert_eq!(report.rewritten_statements, 1);
    assert!(
        report.declines.is_empty(),
        "unexpected declines: {:?}",
        report.declines
    );
    assert_eq!(report.source.matches("norma:console").count(), 1);
    assert!(
        report
            .source
            .contains("importa ex \"norma:console\" fixum dic ut output")
    );
    assert!(report.source.contains("output ⇇ \"§ §\\n\"(7, \"salve\")"));
    assert!(report.source.contains("⇇"));
    assert!(!report.source.contains("nota 7, \"salve\""));
}

#[test]
fn declines_non_normal_output_shapes_with_structured_reasons_and_reemits_them() {
    let source = r#"+++
locale = "la"
+++
incipit {
    mone "warning"
    vide "debug"
}
"#;
    let (path, original) = fixture("latin-declines", source);
    let report = codemod_single_doc(&path, &original).expect("declined source still re-emits");
    let _ = fs::remove_file(&path);

    assert_eq!(report.rewritten_statements, 0);
    assert_eq!(report.declines.len(), 2);
    assert_eq!(report.source, original);
    assert_decline(&report.declines[0], "unsupported_output_kind");
    assert_decline(&report.declines[1], "unsupported_output_kind");
    assert!(report.source.contains("mone \"warning\""));
    assert!(report.source.contains("vide \"debug\""));
}

#[test]
fn declines_unsupported_locale_through_the_structured_header_decline() {
    // The None arm of the surface-header let-else: an explicit unknown reader
    // locale yields no safe header, so the rewrite declines instead of
    // panicking. Proven at the rewrite seam; the command layer surfaces this
    // locale error before the decline print.
    let source = r#"+++
locale = "fr"
+++
incipit {
    nota "salve"
}
"#;
    let (rewritten, count, declines) = super::rewrite_source(source).expect("source lexes");
    assert_eq!(count, 0);
    assert_eq!(declines.len(), 1);
    assert_decline(&declines[0], "unsupported_locale");
    assert_eq!(rewritten, source);
}

#[test]
fn cli_exposes_the_dedicated_output_transfer_codemod_command() {
    let cli = crate::cli::Cli::try_parse_from([
        "faber",
        "output-transfer-codemod",
        "--stdout",
        "main.fab",
    ])
    .expect("parse output-transfer-codemod");
    let Some(crate::cli::Command::OutputTransferCodemod(args)) = cli.command else {
        panic!("expected output-transfer-codemod command");
    };
    assert!(args.stdout);
    assert_eq!(args.paths, vec![PathBuf::from("main.fab")]);
}

fn assert_decline(reason: &DeclineReason, code: &str) {
    assert_eq!(reason.code, code);
    assert!(reason.end >= reason.start);
}

fn rust_emit(name: &str, source: &str) -> String {
    let result =
        radix::services::compile_cli_source(name, source, radix::module::codegen::Target::HirRust);
    assert!(
        result.success(),
        "native rust emit failed for {name}: {:?}",
        result.diagnostics
    );
    radix::services::output_code(result.output.expect("rust output"))
}

fn rust_format_literal(code: &str, macro_name: &str) -> String {
    let needle = format!("{macro_name}!(\"");
    let start = code
        .find(&needle)
        .unwrap_or_else(|| panic!("missing {macro_name}! format string in:\n{code}"))
        + needle.len();
    let bytes = code.as_bytes();
    let mut end = start;
    while end < bytes.len() {
        if bytes[end] == b'"' && (end == start || bytes[end - 1] != b'\\') {
            break;
        }
        end += 1;
    }
    code[start..end].to_owned()
}

fn decode_format_source(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[test]
fn eot5b_representative_pre_post_native_output_is_byte_identical() {
    // Representative pre/post through the EOT-5A command. Native identity is
    // the shared hir-rust format table (rust_scribe_format /
    // rust_format_template / generate_format_arg_expr): scribe println! plus
    // macro newline vs write_partial of a constructor with embedded `\n`.
    // Compared as bytes. Not a corpus-wide migration claim.
    let pre = r#"+++
locale = "la"
+++
incipit {
    nota 7, "salve"
}
"#;
    let (path, _) = fixture("eot5b-pre", pre);
    let report = codemod_single_doc(&path, &fs::read_to_string(&path).expect("read fixture"))
        .expect("codemod accepted representative source");
    let _ = fs::remove_file(&path);

    assert_eq!(report.rewritten_statements, 1);
    assert!(
        report.source.contains("output ⇇ \"§ §\\n\"(7, \"salve\")"),
        "admitted rewrite must keep sequential holes, space separators, and the embedded newline:\n{}",
        report.source
    );

    let pre_rust = rust_emit("eot5b-pre.fab", pre);
    let post_rust = rust_emit("eot5b-post.fab", &report.source);
    let pre_fmt = rust_format_literal(&pre_rust, "println");
    let post_fmt = rust_format_literal(&post_rust, "format");
    assert_eq!(
        pre_fmt.as_bytes(),
        post_fmt.trim_end_matches("\\n").as_bytes(),
        "format-choice table must pick the same slots; diverging {{}}/{{:?}} fails the lemma\npre={pre_fmt:?}\npost={post_fmt:?}"
    );
    assert!(
        post_fmt.ends_with("\\n"),
        "template path must embed the trailing newline: {post_fmt:?}"
    );

    let mut pre_native = decode_format_source(&pre_fmt);
    pre_native.push('\n');
    let post_native = decode_format_source(&post_fmt);
    assert_eq!(
        pre_native.as_bytes(),
        post_native.as_bytes(),
        "EOT-5B compares native output as bytes, not normalized text\npre={pre_native:?}\npost={post_native:?}"
    );
    assert!(
        pre_native.ends_with('\n') && post_native.ends_with('\n'),
        "trailing newline is a native byte, not optional whitespace"
    );
    assert!(
        !pre_rust.contains("format!(") && post_rust.contains("format!("),
        "pre stays on println!; post uses the template path"
    );
}

const DRIFT_SOURCE: &str = "+++\nlocale = \"la\"\n+++\nincipit {\n    nota \"salve\"\n}\n";

fn run_codemod(args: &[&str]) -> std::process::Output {
    let mut full = vec!["output-transfer-codemod"];
    full.extend_from_slice(args);
    super::super::format_test::run_faber_capture(&full, None)
}

/// Write the codemod's own output and return it, so a second run is a no-op.
fn already_codemodded(dir: &std::path::Path) -> (PathBuf, String) {
    let path = dir.join("done.fab");
    let report = codemod_single_doc(&path, DRIFT_SOURCE).expect("codemod accepted source");
    fs::write(&path, &report.source).expect("write codemodded fixture");
    (path, report.source)
}

#[test]
fn cli_output_transfer_codemod_default_mode_is_report_only() {
    let cli = crate::cli::Cli::try_parse_from(["faber", "output-transfer-codemod", "main.fab"])
        .expect("parse output-transfer-codemod");
    let Some(crate::cli::Command::OutputTransferCodemod(args)) = cli.command else {
        panic!("expected output-transfer-codemod command");
    };
    assert!(!args.write && !args.check && !args.stdout && !args.stdin);
}

#[test]
fn cli_output_transfer_codemod_modes_conflict() {
    for pair in [
        ["--write", "--check"],
        ["--write", "--stdout"],
        ["--check", "--stdout"],
        ["--stdin", "--write"],
        ["--stdin", "--stdout"],
    ] {
        let result =
            crate::cli::Cli::try_parse_from(["faber", "output-transfer-codemod", pair[0], pair[1]]);
        assert!(result.is_err(), "{pair:?} must conflict");
    }
}

#[test]
fn output_transfer_codemod_default_writes_nothing_and_exits_1_on_drift() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("drift.fab");
    fs::write(&path, DRIFT_SOURCE).expect("write fixture");
    let (_, _) = already_codemodded(dir.path());
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
fn output_transfer_codemod_default_exits_0_when_nothing_would_change() {
    let dir = tempfile::tempdir().expect("create scratch");
    let (path, source) = already_codemodded(dir.path());
    let output = run_codemod(&[path.to_str().expect("utf8 path")]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "1 files already codemodded"
    );
    assert_eq!(fs::read_to_string(&path).expect("read back"), source);
}

#[test]
fn output_transfer_codemod_write_changes_only_files_that_drift() {
    let dir = tempfile::tempdir().expect("create scratch");
    let drift = dir.path().join("drift.fab");
    fs::write(&drift, DRIFT_SOURCE).expect("write fixture");
    let (clean, clean_source) = already_codemodded(dir.path());
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
    assert_ne!(
        fs::read_to_string(&drift).expect("read drift"),
        DRIFT_SOURCE
    );
    assert!(
        fs::read_to_string(&drift)
            .expect("read drift")
            .contains("norma:console")
    );
    assert_eq!(
        fs::read_to_string(&clean).expect("read clean"),
        clean_source
    );
    assert_eq!(
        fs::metadata(&clean)
            .and_then(|meta| meta.modified())
            .expect("clean mtime"),
        clean_mtime,
        "an unchanged file must not be touched"
    );
}

#[test]
fn output_transfer_codemod_counts_refused_files_and_never_writes_them() {
    let dir = tempfile::tempdir().expect("create scratch");
    let lossy = dir.path().join("lossy.fab");
    fs::write(&lossy, super::super::format_test::HOOK_SOURCE).expect("write lossy fixture");
    let drift = dir.path().join("drift.fab");
    fs::write(&drift, DRIFT_SOURCE).expect("write fixture");
    let root = dir.path().to_str().expect("utf8 path");

    let report = run_codemod(&[root]);
    let report_out = String::from_utf8_lossy(&report.stdout);
    assert_eq!(report.status.code(), Some(1), "{report_out}");
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
    assert_eq!(write.status.code(), Some(1), "{write_out}");
    assert_eq!(
        write_out.lines().last(),
        Some("codemodded 1 of 2 files, 1 refused (nothing written)")
    );
    assert_eq!(
        fs::read_to_string(&lossy).expect("read lossy"),
        super::super::format_test::HOOK_SOURCE,
        "a refused file is never written"
    );
}

#[test]
fn output_transfer_codemod_stdout_prints_one_document_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("drift.fab");
    fs::write(&path, DRIFT_SOURCE).expect("write fixture");
    let output = run_codemod(&["--stdout", path.to_str().expect("utf8 path")]);
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("norma:console"));
    assert_eq!(fs::read_to_string(&path).expect("read back"), DRIFT_SOURCE);
}
