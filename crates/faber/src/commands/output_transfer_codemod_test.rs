//! Focused proof for the output-transfer codemod command.

use super::{codemod_single_doc, DeclineReason};
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
    assert_eq!(report.source.matches("norma:consolum").count(), 1);
    assert!(report
        .source
        .contains("importa ex \"norma:consolum\" fixum dic ut output"));
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
    assert!(!reason.message.is_empty());
}
