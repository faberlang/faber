//! Tests for source-file loading, frontmatter parsing, and offset/line utilities.
//!
//! Contract: `SourceFile` preserves raw content while exposing a body that excludes frontmatter.

use super::{SourceFile, SourceLoadError, source_load_diagnostic};
use crate::diagnostics::DiagnosticPhase;
use std::path::PathBuf;

#[test]
fn new_uses_filename_and_preserves_content() {
    let source = SourceFile::new(PathBuf::from("/tmp/demo.fab"), "a\nb\n".to_owned()).expect("source");

    assert_eq!(source.name, "demo.fab");
    assert_eq!(source.content, "a\nb\n");
    assert_eq!(source.raw_content, "a\nb\n");
    assert_eq!(source.body_byte_offset, 0);
    assert!(source.frontmatter.is_none());
}

#[test]
fn inline_sets_name_and_empty_path() {
    let source = SourceFile::inline("<stdin>", "incipit {}".to_owned()).expect("source");

    assert_eq!(source.name, "<stdin>");
    assert!(source.path.as_os_str().is_empty());
}

#[test]
fn peels_frontmatter_into_body_and_metadata() {
    let raw = "+++\nsectio = \"smoke\"\n+++\nfunctio ping() → vacuum {}\n";
    let source = SourceFile::inline("demo.fab", raw.to_owned()).expect("source");

    assert_eq!(source.content, "functio ping() → vacuum {}\n");
    assert_eq!(source.raw_content, raw);
    assert_eq!(source.frontmatter.as_ref().and_then(|fm| fm.sectio()), Some("smoke"));
    assert_eq!(&source.raw_content[source.body_byte_offset as usize..], source.content.as_str());
}

#[test]
fn frontmatter_only_file_has_empty_body() {
    let raw = "+++\nsectio = \"meta\"\n+++\n";
    let source = SourceFile::inline("meta.fab", raw.to_owned()).expect("source");

    assert_eq!(source.content, "");
    assert_eq!(source.frontmatter.as_ref().and_then(|fm| fm.sectio()), Some("meta"));
}

#[test]
fn source_load_diagnostic_uses_parse052_contract() {
    let err = SourceLoadError::Toml("demo.fab: expected string".to_owned());
    let diag = source_load_diagnostic("demo.fab", &err);

    assert_eq!(diag.code, Some("PARSE052"));
    assert_eq!(diag.phase, DiagnosticPhase::Parse);
    assert_eq!(diag.file, "demo.fab");
}

#[test]
fn invalid_frontmatter_toml_is_rejected() {
    let raw = "+++\nsectio = \n+++\n";
    let err = SourceFile::inline("bad.fab", raw.to_owned()).expect_err("invalid toml");
    assert!(matches!(err, SourceLoadError::Toml(_)));
}

#[test]
fn offset_to_line_col_maps_offsets_across_lines() {
    let source = SourceFile::inline("x", "ab\ncd\nef".to_owned()).expect("source");

    assert_eq!(source.offset_to_line_col(0), (1, 1));
    assert_eq!(source.offset_to_line_col(2), (1, 3));
    assert_eq!(source.offset_to_line_col(3), (2, 1));
    assert_eq!(source.offset_to_line_col(6), (3, 1));
}

#[test]
fn line_content_returns_trimmed_lines_and_none_out_of_range() {
    let source = SourceFile::inline("x", "prima\nsecunda\ntertia".to_owned()).expect("source");

    assert_eq!(source.line_content(1), Some("prima"));
    assert_eq!(source.line_content(2), Some("secunda"));
    assert_eq!(source.line_content(3), Some("tertia"));
    assert_eq!(source.line_content(4), None);
    assert_eq!(source.line_content(0), None);
}

// =============================================================================
// EOT-1 — driver red driver and source baseline
// =============================================================================

#[test]
fn eot_driver_acceptance_red_driver_for_selective_import_and_transfer() {
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let result = crate::driver::analyze_source(
        &session,
        "eot-1.fab",
        r#"importa ex "norma:consolum" fixum dic ut output
incipit { output ⇇ "salve\\n" }"#,
        crate::driver::AnalyzeRequest::default(),
    );
    assert!(
        result.is_ok(),
        "EOT-1 driver red fixture must reach analyzable HIR; current diagnostics={:?}; later issue names are `selective_import_missing_member`, `selective_import_duplicate_alias`, and transfer sink/payload issues",
        result.as_ref().err()
    );
}

#[test]
fn eot_driver_legacy_source_baseline_preserves_output_and_input_forms() {
    let source = r#"incipit {
  nota "legacy text"
  mone "legacy warning"
  vide "legacy debug"
  scribe "legacy compatibility"
  sit _ input ← lege
  nota input
}"#;
    let file = SourceFile::inline("eot-1-legacy.fab", source.to_owned()).expect("source");
    assert_eq!(file.content, source);
    assert_eq!(file.raw_content, source);
    assert_eq!(file.body_byte_offset, 0);
}
