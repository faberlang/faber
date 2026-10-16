//! K5 (gpu-reset rule 15): the built-in scalar math functions lift over a
//! tensor; a user function does not.
//!
//! Registry-driven: the lifted set is `MirMathFunction::ALL` by canonical Latin
//! name. Element typing follows the scalar function: `absolutum` takes any
//! numeric element and range-types an integer element to the smallest holder
//! of `|x|`; every other member takes float elements only.

use radix_diagnostics::DiagnosticArg;
use radix_types::MirMathFunction;

const ISSUE_FLOAT_ELEMENT: &str = "tensor_math_float_element_required";
const ISSUE_NOT_LIFTED: &str = "tensor_function_not_lifted";

fn accepts(source: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}",
        result.diagnostics
    );
}

fn errors_of(source: &str) -> Vec<Vec<DiagnosticArg>> {
    crate::test_support::analyze_source(source)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.args.clone())
        .collect()
}

fn rejects_with_issue(source: &str, issue: &str) {
    let issue_arg = DiagnosticArg::new("issue", issue);
    let errors = errors_of(source);
    assert!(
        errors.iter().any(|args| args.contains(&issue_arg)),
        "expected issue `{issue}`, got: {errors:?}"
    );
}

#[test]
fn every_lifted_function_preserves_a_float_tensor_type() {
    for function in MirMathFunction::ALL {
        let name = function.latin_name();
        accepts(&format!(
            "functio lifted(tensor<f32, [2, 3]> x) → tensor<f32, [2, 3]> {{\n    redde x.{name}()\n}}"
        ));
        accepts(&format!(
            "functio lifted(tensor<f64, [4]> x) → tensor<f64, [4]> {{\n    redde x.{name}()\n}}"
        ));
    }
}

#[test]
fn float_only_functions_reject_integer_elements() {
    for function in MirMathFunction::ALL.into_iter().filter(|f| f.float_only()) {
        let name = function.latin_name();
        rejects_with_issue(
            &format!(
                "functio lifted(tensor<i32, [3]> x) → tensor<i32, [3]> {{\n    redde x.{name}()\n}}"
            ),
            ISSUE_FLOAT_ELEMENT,
        );
    }
}

#[test]
fn absolutum_takes_integer_elements_and_range_types_them() {
    // Scalar `absolutum` of an `i8` is a `u8` (the range rule); the tensor
    // lift is "the scalar operation per element", so the element follows.
    accepts("functio lifted(tensor<i8, [3]> x) → tensor<u8, [3]> {\n    redde x.absolutum()\n}");
    accepts("functio lifted(tensor<u8, [3]> x) → tensor<u8, [3]> {\n    redde x.absolutum()\n}");
    // Under ruling Q-X3 (2026-10-15, need) the i8 return slot
    // accepts the tensor<u8,[3]> absolutum as a checked u8→i8 element
    // narrowing (run-time trap for |x| > 127).
    accepts("functio lifted(tensor<i8, [3]> x) → tensor<i8, [3]> {\n    redde x.absolutum()\n}");
    // Q-X3 narrows only same-family integer elements, so a float-element
    // return slot still errors.
    assert!(
        !errors_of(
            "functio lifted(tensor<i8, [3]> x) → tensor<f32, [3]> {\n    redde x.absolutum()\n}"
        )
        .is_empty(),
        "an i8 tensor's absolutum is not a float tensor"
    );
}

#[test]
fn math_functions_reject_non_numeric_elements() {
    rejects_with_issue(
        "functio lifted(tensor<textus, [2]> x) → tensor<textus, [2]> {\n    redde x.absolutum()\n}",
        "tensor_arithmetic_numeric_element_required",
    );
}

#[test]
fn a_user_function_is_not_lifted_over_a_tensor() {
    let source = "functio duplum(f32 x) → f32 {\n    redde x * 2.0\n}\n\
                  functio lifted(tensor<f32, [3]> t) → tensor<f32, [3]> {\n    redde duplum(t)\n}";
    rejects_with_issue(source, ISSUE_NOT_LIFTED);
    let errors = errors_of(source);
    assert_eq!(
        errors.len(),
        1,
        "the lifting attempt is one diagnostic, not a cascade: {errors:?}"
    );
}

#[test]
fn a_scalar_call_on_a_scalar_is_untouched() {
    accepts(
        "functio duplum(f32 x) → f32 {\n    redde x * 2.0\n}\n\
         functio scalar(f32 y) → f32 {\n    redde duplum(y)\n}",
    );
}
