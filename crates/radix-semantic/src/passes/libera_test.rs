//! Focused tests for the base-lane closure modifier contracts (CL-4, KCM-03).
//!
//! The `libera` cases pin the capture-free contract: a marked capturing
//! closure fails with a structured issue naming/spanning the capture; a marked
//! closure referencing module-level items passes; an unmarked capturing
//! closure is unchanged. The `nucleum` cases pin the kernel-closure contract:
//! capture freedom, body forms, and the local-const/IIFE escape law.

use crate::passes::libera::{
    ISSUE_LIBERA_CAPTURE, ISSUE_NUCLEUM_CAPTURE, ISSUE_NUCLEUM_EFFECT, ISSUE_NUCLEUM_ESCAPE,
};
use crate::test_support::{analyze_source, issue};

fn find_error_by_issue<'a>(
    diagnostics: &'a [crate::SemanticError],
    wanted: &str,
) -> Option<&'a crate::SemanticError> {
    diagnostics
        .iter()
        .filter(|error| error.is_error())
        .find(|error| issue(error) == Some(wanted))
}

fn capture_arg<'a>(error: &'a crate::SemanticError, name: &str) -> Option<&'a str> {
    error
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn arg_is(error: &crate::SemanticError, name: &str, expected: &str) -> bool {
    capture_arg(error, name) == Some(expected)
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
    let source = r"
incipit {
    fixum numerus seed ← 2
    fixum _ closure ← (numerus value) libera ∴ value + seed
}
";
    let result = analyze_source(source);
    let error = find_error_by_issue(&result.diagnostics, ISSUE_LIBERA_CAPTURE)
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
        r"
functio take(numerus seed) → vacuum {
    fixum _ closure ← (numerus value) libera ∴ value + seed
    tacet
}
",
    );
    let error = find_error_by_issue(&result.diagnostics, ISSUE_LIBERA_CAPTURE)
        .unwrap_or_else(|| panic!("expected libera_capture, got: {:?}", result.diagnostics));
    assert_eq!(capture_arg(error, "capture"), Some("seed"));
}

#[test]
fn accepts_libera_closure_reference_to_module_level_function() {
    let result = analyze_source(
        r"
functio module_value() → numerus {
    redde 1
}
incipit {
    fixum _ closure ← (numerus value) libera ∴ value + module_value()
}
",
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
        r"
discretio Colorum { Rubrum }
incipit {
    fixum _ closure ← (numerus value) libera ∴ Colorum.Rubrum
}
",
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
        r"
incipit {
    fixum numerus seed ← 2
    fixum _ closure ← (numerus value) ∴ value + seed
}
",
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
        r"
incipit {
    fixum _ closure ← clausura numerus value libera → numerus {
        fixum numerus doubled ← value * 2
        redde doubled
    }
}
",
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
        r"
incipit {
    fixum numerus seed ← 2
    fixum _ closure ← (numerus value) libera ∴ (numerus inner) ∴ inner + seed
}
",
    );
    let error = find_error_by_issue(&result.diagnostics, ISSUE_LIBERA_CAPTURE)
        .unwrap_or_else(|| panic!("expected libera_capture, got: {:?}", result.diagnostics));
    assert_eq!(capture_arg(error, "capture"), Some("seed"));
}

#[test]
fn accepts_libera_closure_whose_nested_closure_captures_only_its_params() {
    let result = analyze_source(
        r"
incipit {
    fixum _ closure ← (numerus value) libera ∴ (numerus inner) ∴ inner + value
}
",
    );
    assert!(
        result.is_ok(),
        "nested closure references bound by the marked closure are not captures: {:?}",
        result.diagnostics
    );
}

// =============================================================================
// KCM-03 — nucleum kernel-closure contract: capture, body forms, escape
// =============================================================================

#[test]
fn rejects_nucleum_closure_capturing_an_enclosing_local() {
    let source = r"
incipit {
    fixum numerus seed ← 2
    fixum _ kernel ← (numerus value) nucleum ∴ value + seed
}
";
    let result = analyze_source(source);
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_CAPTURE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_capture, got: {:?}",
                result.diagnostics
            )
        });
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
fn rejects_nucleum_closure_capturing_an_enclosing_parameter() {
    let result = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum _ kernel ← (numerus value) nucleum ∴ value + seed
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_CAPTURE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_capture, got: {:?}",
                result.diagnostics
            )
        });
    assert_eq!(capture_arg(error, "capture"), Some("seed"));
}

#[test]
fn accepts_nucleum_closure_reference_to_module_level_kernel() {
    let result = analyze_source(
        r"
@ nucleum { }
functio device_leaf(numerus x) → numerus {
    redde x + 1
}
incipit {
    fixum _ kernel ← (numerus value) nucleum → numerus ∴ device_leaf(value)
}
",
    );
    assert!(
        result.is_ok(),
        "module-level kernel calls are statically resolved identities, not captures: {:?}",
        result.diagnostics
    );
}

#[test]
fn accepts_nucleum_closure_with_enclosing_type_and_size_parameters() {
    let result = analyze_source(
        r"
functio shaped<T, magnitudo N>(tensor<T, [N]> input) → vacuum {
    fixum _ kernel ← (tensor<T, [N]> value) nucleum → tensor<T, [N]> ∴ value
    tacet
}
",
    );
    assert!(
        result.is_ok(),
        "enclosing type/size parameters are not value captures: {:?}",
        result.diagnostics
    );
}

#[test]
fn accepts_nucleum_closure_local_call_and_same_expression_iife() {
    let result = analyze_source(
        r"
incipit {
    fixum _ double ← (numerus value) nucleum → numerus ∴ value * 2
    fixum numerus four ← double(2)
    fixum numerus six ← ((numerus value) nucleum → numerus ∴ value * 3)(2)
    tacet
}
",
    );
    assert!(
        result.is_ok(),
        "local const call and IIFE are the two legal kernel-closure uses: {:?}",
        result.diagnostics
    );
}

#[test]
fn iife_nucleum_closure_is_checked_under_the_full_contract() {
    let result = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum numerus r ← ((numerus value) nucleum → numerus ∴ value + seed)(1)
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_CAPTURE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_capture, got: {:?}",
                result.diagnostics
            )
        });
    assert_eq!(capture_arg(error, "capture"), Some("seed"));

    let effect = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum numerus r ← ((numerus value) nucleum → numerus ∴ (numerus inner) ∴ inner)(1)
    tacet
}
",
    );
    let error =
        find_error_by_issue(&effect.diagnostics, ISSUE_NUCLEUM_EFFECT).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_effect, got: {:?}",
                effect.diagnostics
            )
        });
    assert!(arg_is(error, "reason", "dynamic_value"));
}

#[test]
fn rejects_nucleum_closure_with_signature_error_channel() {
    let result = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum _ kernel ← (numerus value) nucleum → numerus ⇥ textus ∴ value
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_EFFECT).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_effect, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "reason", "alternate_exit"));
    assert!(arg_is(error, "operation", "error_channel"));
}

#[test]
fn rejects_nucleum_closure_body_throw() {
    let result = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum _ kernel ← clausura numerus value nucleum → numerus {
        iace value
        redde value
    }
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_EFFECT).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_effect, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "reason", "throw"));
    assert!(arg_is(error, "operation", "iace"));
}

#[test]
fn rejects_nucleum_closure_body_require_guard() {
    let result = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum _ kernel ← clausura numerus value nucleum → numerus {
        requirit value ≺ 10 iace value
        redde value
    }
    tacet
}
",
    );
    // `requirit` lowers to a guard around `iace`; the kernel identity fires.
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_EFFECT).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_effect, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "reason", "throw"));
}

#[test]
fn rejects_nucleum_closure_body_cape_recovery() {
    let result = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum _ kernel ← clausura numerus value nucleum → numerus {
        fac {
            fixum numerus probe ← value
        } cape err {
            tacet
        }
        redde value
    }
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_EFFECT).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_effect, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "reason", "cape"));
    assert!(arg_is(error, "operation", "cape"));
}

#[test]
fn rejects_nucleum_closure_body_scribe() {
    let result = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum _ kernel ← clausura numerus value nucleum → numerus {
        nota value
        redde value
    }
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_EFFECT).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_effect, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "reason", "host_effect"));
    assert!(arg_is(error, "operation", "scribe"));
}

#[test]
fn rejects_nucleum_closure_body_dynamic_value() {
    let result = analyze_source(
        r"
functio take(numerus seed) → vacuum {
    fixum _ kernel ← (numerus value) nucleum ∴ (numerus inner) ∴ inner
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_EFFECT).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_effect, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "reason", "dynamic_value"));
    assert!(arg_is(error, "operation", "clausura"));
}

#[test]
fn rejects_nucleum_closure_field_escape() {
    let result = analyze_source(
        r"
genus Capsula {
    numerus val
}
functio take() → vacuum {
    fixum _ kernel ← (numerus value) nucleum ∴ value
    fixum Capsula boxed ← Capsula { val = kernel }
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_ESCAPE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_escape, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "context", "field"));
}

#[test]
fn rejects_nucleum_closure_direct_field_escape() {
    let result = analyze_source(
        r"
genus Capsula {
    numerus val
}
functio take() → vacuum {
    fixum Capsula boxed ← Capsula { val = (numerus value) nucleum ∴ value }
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_ESCAPE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_escape, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "context", "field"));
}

#[test]
fn rejects_nucleum_closure_list_escape() {
    let result = analyze_source(
        r"
functio take() → vacuum {
    fixum _ kernel ← (numerus value) nucleum ∴ value
    fixum lista<numerus> items ← [kernel, kernel]
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_ESCAPE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_escape, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "context", "list"));
}

#[test]
fn rejects_nucleum_closure_return_escape() {
    let result = analyze_source(
        r"
functio take() → vacuum {
    fixum _ kernel ← (numerus value) nucleum ∴ value
    redde kernel
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_ESCAPE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_escape, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "context", "return"));
}

#[test]
fn rejects_nucleum_closure_argument_escape() {
    let result = analyze_source(
        r"
functio apply(numerus x) → numerus {
    redde x
}
functio take() → vacuum {
    fixum _ kernel ← (numerus value) nucleum ∴ value
    fixum numerus r ← apply(kernel)
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_ESCAPE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_escape, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "context", "ordinary_argument"));
}

#[test]
fn rejects_nucleum_closure_varia_binding() {
    let result = analyze_source(
        r"
functio take() → vacuum {
    varia _ kernel ← (numerus value) nucleum ∴ value
    tacet
}
",
    );
    let error =
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_ESCAPE).unwrap_or_else(|| {
            panic!(
                "expected nucleum_closure_escape, got: {:?}",
                result.diagnostics
            )
        });
    assert!(arg_is(error, "context", "varia_binding"));
}

#[test]
fn nucleum_contract_leaves_unmarked_capturing_closure_unchanged() {
    let result = analyze_source(
        r"
incipit {
    fixum numerus seed ← 2
    fixum _ plain ← (numerus value) ∴ value + seed
    fixum _ freed ← (numerus value) libera ∴ value + seed
}
",
    );
    // The plain closure captures legally; only the libera marker is checked.
    let libera_error =
        find_error_by_issue(&result.diagnostics, ISSUE_LIBERA_CAPTURE).expect("libera capture");
    assert_eq!(capture_arg(libera_error, "capture"), Some("seed"));
    assert!(
        find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_CAPTURE).is_none()
            && find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_EFFECT).is_none()
            && find_error_by_issue(&result.diagnostics, ISSUE_NUCLEUM_ESCAPE).is_none(),
        "no nucleum identity may fire without the marker: {:?}",
        result.diagnostics
    );
}
