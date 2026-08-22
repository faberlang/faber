//! Focused tests for the base-lane `libera` capture-free contract (CL-4).
//!
//! Three cases pin the contract: a marked capturing closure fails with a
//! structured issue naming/spanning the capture; a marked closure referencing
//! module-level items passes; an unmarked capturing closure is unchanged.

use crate::passes::libera::ISSUE_LIBERA_CAPTURE;
use crate::test_support::{analyze_source, issue};

fn find_capture_error(diagnostics: &[crate::SemanticError]) -> Option<&crate::SemanticError> {
    diagnostics
        .iter()
        .filter(|error| error.is_error())
        .find(|error| issue(error) == Some(ISSUE_LIBERA_CAPTURE))
}

fn capture_arg<'a>(error: &'a crate::SemanticError, name: &str) -> Option<&'a str> {
    error
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// Byte offset of the last occurrence of `needle` in `source`.
fn last_offset(source: &str, needle: &str) -> u32 {
    let offset = source
        .rfind(needle)
        .unwrap_or_else(|| panic!("`{needle}` must appear in the fixture"));
    u32::try_from(offset).expect("fixture byte offset fits u32")
}

#[test]
fn rejects_libera_closure_capturing_an_enclosing_local() {
    let source = r#"
incipit {
    fixum numerus seed ← 2
    fixum _ closure ← (numerus value) libera ∴ value + seed
}
"#;
    let result = analyze_source(source);
    let error = find_capture_error(&result.diagnostics)
        .unwrap_or_else(|| panic!("expected libera_capture, got: {:?}", result.diagnostics));
    assert_eq!(capture_arg(error, "capture"), Some("seed"));
    assert_eq!(capture_arg(error, "capture_name"), Some("seed"));
    let seed_offset = last_offset(source, "seed");
    assert!(
        error.span.start <= seed_offset && seed_offset < error.span.end,
        "diagnostic must span the captured `seed` use; span={:?}, offset={seed_offset}",
        error.span
    );
}

#[test]
fn rejects_libera_closure_capturing_an_enclosing_parameter() {
    let result = analyze_source(
        r#"
functio take(numerus seed) → vacuum {
    fixum _ closure ← (numerus value) libera ∴ value + seed
    tacet
}
"#,
    );
    let error = find_capture_error(&result.diagnostics)
        .unwrap_or_else(|| panic!("expected libera_capture, got: {:?}", result.diagnostics));
    assert_eq!(capture_arg(error, "capture"), Some("seed"));
}

#[test]
fn accepts_libera_closure_reference_to_module_level_function() {
    let result = analyze_source(
        r#"
functio module_value() → numerus {
    redde 1
}
incipit {
    fixum _ closure ← (numerus value) libera ∴ value + module_value()
}
"#,
    );
    assert!(
        result.is_ok(),
        "a libera closure may reference module-level items: {:?}",
        result.diagnostics
    );
}

#[test]
fn accepts_libera_closure_reference_to_enum_variant() {
    let result = analyze_source(
        r#"
discretio Colorum { Rubrum }
incipit {
    fixum _ closure ← (numerus value) libera ∴ Colorum.Rubrum
}
"#,
    );
    assert!(
        result.is_ok(),
        "enum variant paths are statically resolved, not captures: {:?}",
        result.diagnostics
    );
}

#[test]
fn unmarked_capturing_closure_is_unchanged() {
    let result = analyze_source(
        r#"
incipit {
    fixum numerus seed ← 2
    fixum _ closure ← (numerus value) ∴ value + seed
}
"#,
    );
    assert!(
        result.is_ok(),
        "an unmarked closure may capture enclosing locals: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|error| issue(error) != Some(ISSUE_LIBERA_CAPTURE)),
        "no libera_capture may fire without the marker: {:?}",
        result.diagnostics
    );
}

#[test]
fn accepts_libera_closure_with_body_local_binding() {
    let result = analyze_source(
        r#"
incipit {
    fixum _ closure ← clausura numerus value libera → numerus {
        fixum numerus doubled ← value * 2
        redde doubled
    }
}
"#,
    );
    assert!(
        result.is_ok(),
        "bindings introduced inside the marked closure are not captures: {:?}",
        result.diagnostics
    );
}

#[test]
fn rejects_libera_closure_transitively_capturing_through_nested_closure() {
    let result = analyze_source(
        r#"
incipit {
    fixum numerus seed ← 2
    fixum _ closure ← (numerus value) libera ∴ (numerus inner) ∴ inner + seed
}
"#,
    );
    let error = find_capture_error(&result.diagnostics)
        .unwrap_or_else(|| panic!("expected libera_capture, got: {:?}", result.diagnostics));
    assert_eq!(capture_arg(error, "capture"), Some("seed"));
}

#[test]
fn accepts_libera_closure_whose_nested_closure_captures_only_its_params() {
    let result = analyze_source(
        r#"
incipit {
    fixum _ closure ← (numerus value) libera ∴ (numerus inner) ∴ inner + value
}
"#,
    );
    assert!(
        result.is_ok(),
        "nested closure references bound by the marked closure are not captures: {:?}",
        result.diagnostics
    );
}
