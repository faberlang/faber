//! A2-LIFT (gap G01): the built-in math functions are methods on a scalar
//! float, the same set and Latin names the tensor lift (K5) applies per
//! element. `absolutum` was already a numeric method; the other seven
//! (`radix`, `exponentia`, `logarithmus`, `logarithmus_decimalis`, `sinus`,
//! `cosinus`, `tangens`) are float-only rows, so an integer receiver has no
//! such method.

use radix_types::MirMathFunction;

fn accepts(source: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}\n{source}",
        result.diagnostics
    );
}

fn error_issues(source: &str) -> Vec<String> {
    crate::test_support::analyze_source(source)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .filter_map(|d| {
            d.args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

#[test]
fn every_scalar_function_keeps_a_float_receiver_type() {
    for function in MirMathFunction::SCALAR {
        let name = function.latin_name();
        for ty in ["f32", "f64"] {
            accepts(&format!(
                "functio lifted({ty} x) → {ty} {{\n    redde x.{name}()\n}}"
            ));
        }
    }
}

#[test]
fn a_scalar_function_result_composes_with_arithmetic_and_other_functions() {
    accepts("functio mixed(f32 x) → f32 {\n    redde x.radix() + x.sinus() * x.cosinus()\n}");
    accepts("functio chained(f64 x) → f64 {\n    redde x.exponentia().logarithmus()\n}");
    accepts("functio sized(f32 x) → f32 {\n    redde (x * x + 1.0).radix()\n}");
}

#[test]
fn the_result_is_the_receiver_width_never_narrowed() {
    // An f64 receiver gives an f64 result: it does not land in an f32 slot
    // without an explicit conversion. (An f32 result widens to f64 losslessly.)
    for function in MirMathFunction::SCALAR {
        let name = function.latin_name();
        let issues = error_issues(&format!(
            "functio narrow(f64 x) → f32 {{\n    redde x.{name}()\n}}"
        ));
        assert!(!issues.is_empty(), "{name}: an f64 result is not an f32");
        accepts(&format!(
            "functio widen(f32 x) → f64 {{\n    redde x.{name}()\n}}"
        ));
    }
}

#[test]
fn an_integer_receiver_has_no_float_only_math_method() {
    for function in MirMathFunction::SCALAR {
        let name = function.latin_name();
        for ty in ["i32", "u8", "numerus"] {
            let issues = error_issues(&format!(
                "functio lifted({ty} x) → f64 {{\n    redde x.{name}()\n}}"
            ));
            assert!(
                issues.iter().any(|issue| issue == "unknown_method"),
                "{ty}.{name}: {issues:?}"
            );
        }
    }
}

#[test]
fn a_scalar_function_takes_no_argument() {
    let issues = error_issues("functio lifted(f32 x) → f32 {\n    redde x.radix(2.0)\n}");
    assert!(
        issues.iter().any(|issue| issue == "unknown_method"),
        "{issues:?}"
    );
}

#[test]
fn scalar_absolutum_is_unchanged() {
    accepts("functio magnitude(f32 x) → f32 {\n    redde x.absolutum()\n}");
    accepts("functio magnitude(i8 x) → u8 {\n    redde x.absolutum()\n}");
}
