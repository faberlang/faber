//! Package-lowering diagnostics keep the error's span, physical line, named
//! issue and secondary definition span; validation diagnostics keep the named
//! issue and never name a line past the end of the unit file.

use super::diag::{mir_error_diag, mir_validation_diag};
use radix_mir::MirValidationError;
use radix_mir_lower::lower::MirError;
use radix_module::lexer::Span;
use std::path::PathBuf;

fn error(span: Span, definition_span: Option<Span>) -> MirError {
    MirError {
        message: "prose that tests do not assert".to_owned(),
        issue: "nucleum_fragment_type".to_owned(),
        span,
        definition_span,
    }
}

fn arg<'a>(diagnostic: &'a radix_module::diagnostics::Diagnostic, name: &str) -> Option<&'a str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

#[test]
fn lowering_error_diagnostic_carries_file_line_issue_and_definition_span() {
    let source = "first line\nsecond line\nthird line\n";
    let path: PathBuf =
        std::env::temp_dir().join(format!("faber-diag-test-{}.fab", std::process::id()));
    std::fs::write(&path, source).expect("write fixture source");
    let call = Span::new(11, 17);
    let definition = Span::new(0, 5);
    let diagnostic = mir_error_diag(&path, error(call, Some(definition)));
    std::fs::remove_file(&path).expect("remove fixture source");

    assert_eq!(diagnostic.span, Some(call));
    assert_eq!(diagnostic.line, Some(2), "span offset 11 is on line 2");
    assert_eq!(diagnostic.file, path.display().to_string());
    assert_eq!(arg(&diagnostic, "issue"), Some("nucleum_fragment_type"));
    assert_eq!(arg(&diagnostic, "definition_start"), Some("0"));
    assert_eq!(arg(&diagnostic, "definition_end"), Some("5"));
}

#[test]
fn lowering_error_diagnostic_without_readable_source_keeps_the_span() {
    let path = PathBuf::from("/nonexistent-faber-diag-test/missing.fab");
    let call = Span::new(3, 9);
    let diagnostic = mir_error_diag(&path, error(call, None));
    assert_eq!(diagnostic.span, Some(call));
    assert_eq!(diagnostic.line, None);
    assert_eq!(arg(&diagnostic, "definition_start"), None);
}

#[test]
fn lowering_error_diagnostic_without_a_span_stays_spanless() {
    let diagnostic = mir_error_diag(&PathBuf::from("unit.fab"), error(Span::default(), None));
    assert_eq!(diagnostic.span, None, "a defaulted span is no location");
    assert_eq!(arg(&diagnostic, "issue"), Some("nucleum_fragment_type"));
}

fn validation_error(span: Span) -> MirValidationError {
    MirValidationError {
        message: "prose that tests do not assert".to_owned(),
        issue: "mir_validation_example".to_owned(),
        span,
    }
}

#[test]
fn validation_error_diagnostic_carries_the_named_issue() {
    let source = "first line\nsecond line\nthird line\n";
    let path: PathBuf = std::env::temp_dir().join(format!(
        "faber-diag-validation-test-{}.fab",
        std::process::id()
    ));
    std::fs::write(&path, source).expect("write fixture source");
    let diagnostic = mir_validation_diag(&path, validation_error(Span::new(11, 17)));
    std::fs::remove_file(&path).expect("remove fixture source");

    assert_eq!(arg(&diagnostic, "issue"), Some("mir_validation_example"));
    assert!(diagnostic.span.is_some());
    assert_eq!(diagnostic.line, Some(2), "span offset 11 is on line 2");
}

#[test]
fn validation_error_diagnostic_past_the_end_of_the_file_keeps_issue_without_span() {
    let source = "a\nb\nc\n";
    let path: PathBuf = std::env::temp_dir().join(format!(
        "faber-diag-validation-past-end-test-{}.fab",
        std::process::id()
    ));
    std::fs::write(&path, source).expect("write fixture source");
    let diagnostic = mir_validation_diag(&path, validation_error(Span::new(4000, 4010)));
    std::fs::remove_file(&path).expect("remove fixture source");

    assert_eq!(arg(&diagnostic, "issue"), Some("mir_validation_example"));
    assert_eq!(diagnostic.span, None);
    assert_eq!(diagnostic.line, None);
    assert_eq!(arg(&diagnostic, "span_start"), Some("4000"));
    assert_eq!(arg(&diagnostic, "span_end"), Some("4010"));
}
