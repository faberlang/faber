//! The exit status a runner failure ends the process with: its sysexits class
//! from the shared trap table, selected by structure, never by message text.

use super::RunnerError;
use radix_lexer::Span;

fn leaf_failure(kind: &str) -> RunnerError {
    // A leaf failure crosses as a conversion-coded error carrying its kind.
    let mut error = RunnerError::conversion(Span::default(), "sermo reply conversion failed");
    error.args.push(("fault_kind".to_owned(), kind.to_owned()));
    error
}

#[test]
fn a_program_logic_failure_exits_70() {
    let error = RunnerError::internal(Span::default(), "text index out of bounds");
    assert_eq!(error.exit_status(), 70);
}

#[test]
fn the_message_wording_never_picks_the_class() {
    for text in ["bool conversion failed", "not found", "permission denied"] {
        assert_eq!(
            RunnerError::internal(Span::default(), text).exit_status(),
            70
        );
    }
}

#[test]
fn a_conversion_failure_exits_65() {
    let error = RunnerError::conversion(Span::default(), "text to bivalens conversion failed");
    assert_eq!(error.exit_status(), 65);
}

#[test]
fn an_unsupported_feature_exits_70() {
    assert_eq!(
        RunnerError::unsupported(Span::default(), "provider").exit_status(),
        70
    );
}

#[test]
fn a_leaf_failure_takes_the_class_of_its_io_fault_variant() {
    for (kind, class) in [
        ("NotFound", 66),
        ("Permission", 77),
        ("TimedOut", 75),
        ("ConnectionRefused", 69),
        ("AlreadyExists", 73),
        ("Io", 74),
        ("InvalidArgument", 70),
    ] {
        assert_eq!(leaf_failure(kind).exit_status(), class, "{kind}");
    }
}

#[test]
fn a_leaf_failure_of_an_unknown_variant_exits_70() {
    assert_eq!(leaf_failure("NoSuchVariant").exit_status(), 70);
}
