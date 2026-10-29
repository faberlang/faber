//! `rumpe` and `redde` that would end a whole `filum` loop from inside one
//! lane are refused by name; `perge`, loop-local `rumpe`, and `redde` in a
//! closure or a reduction term body stay legal.
//!
//! Latin fixtures use the test parser's explicit Latin surface.

use crate::test_support::analyze_source_la;

const BREAK: &str = "break_in_thread_body";
const RETURN: &str = "return_in_thread_body";

fn issues(source: &str) -> Vec<String> {
    analyze_source_la(source)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .filter_map(|d| d.issue().map(str::to_owned))
        .collect()
}

fn refused(source: &str, issue: &str) {
    let found = issues(source);
    assert!(!found.is_empty(), "unexpectedly accepted");
    assert!(
        found.iter().all(|found| found == issue),
        "expected only {issue}, got {found:?}"
    );
}

fn accepted(source: &str) {
    let result = analyze_source_la(source);
    assert!(result.success(), "{:?}", result.diagnostics);
}

/// A function whose `filum` body is `body`; `src` drives the loop and `out`
/// is written at the lane's own coordinate.
fn lane(body: &str) -> String {
    format!(
        r"@ nucleum
@ publica
functio k(in tensor<f32, [4]> out, de tensor<f32, [4]> src) → vacuum {{
    itera ex src apud [i] filum lane sit value {{
        {body}
    }}
}}
"
    )
}

#[test]
fn break_that_targets_the_thread_loop_is_refused() {
    refused(&lane("si value ≻ 1.0 { rumpe }\n out[i] ← value"), BREAK);
}

#[test]
fn return_from_the_function_inside_the_thread_loop_is_refused() {
    refused(&lane("si value ≻ 1.0 { redde }\n out[i] ← value"), RETURN);
}

#[test]
fn both_refusals_are_reported_with_their_own_names() {
    let found = issues(&lane(
        "si value ≻ 1.0 { rumpe }\n si value ≻ 2.0 { redde }\n out[i] ← value",
    ));
    assert!(found.iter().any(|found| found == BREAK), "{found:?}");
    assert!(found.iter().any(|found| found == RETURN), "{found:?}");
}

#[test]
fn continue_skips_one_lane_and_is_accepted() {
    accepted(&lane("si value ≻ 1.0 { perge }\n out[i] ← value"));
}

#[test]
fn break_and_continue_in_a_sequential_loop_inside_the_lane_are_accepted() {
    accepted(&lane(
        "varia i32 n ← 0
        dum n ≺ 4 {
            n ← n + 1
            si n ≡ 2 { perge }
            si n ≡ 3 { rumpe }
        }
        out[i] ← value",
    ));
}

#[test]
fn return_inside_a_sequential_loop_inside_the_lane_is_still_refused() {
    refused(
        &lane(
            "varia i32 n ← 0
        dum n ≺ 4 {
            n ← n + 1
            si n ≡ 2 { redde }
        }
        out[i] ← value",
        ),
        RETURN,
    );
}

#[test]
fn break_in_a_breakable_block_inside_the_lane_targets_that_block() {
    accepted(&lane(
        "fac {
            si value ≻ 1.0 { rumpe }
        }
        out[i] ← value",
    ));
}

#[test]
fn break_in_a_sequential_loop_around_the_thread_loop_is_accepted() {
    accepted(
        r"@ nucleum
@ publica
functio k(in tensor<f32, [4]> out, de tensor<f32, [4]> src) → vacuum {
    varia i32 round ← 0
    dum round ≺ 3 {
        round ← round + 1
        itera ex src apud [i] filum lane sit value {
            out[i] ← value
        }
        si round ≡ 2 { rumpe }
    }
}
",
    );
}

#[test]
fn break_that_targets_a_thread_loop_inside_a_sequential_loop_is_refused() {
    refused(
        r"@ nucleum
@ publica
functio k(in tensor<f32, [4]> out, de tensor<f32, [4]> src) → vacuum {
    varia i32 round ← 0
    dum round ≺ 3 {
        round ← round + 1
        itera ex src apud [i] filum lane sit value {
            si value ≻ 1.0 { rumpe }
            out[i] ← value
        }
    }
}
",
        BREAK,
    );
}

#[test]
fn break_that_targets_a_nested_thread_loop_is_refused() {
    refused(
        r"@ nucleum
@ publica
functio k(in tensor<f32, [2, 2]> out, de tensor<f32, [2]> rows, de tensor<f32, [2]> cols) → vacuum {
    itera ex rows apud [i] filum outer sit row {
        itera ex cols apud [j] filum inner sit col {
            si col ≻ 1.0 { rumpe }
            out[[i, j]] ← row + col
        }
    }
}
",
        BREAK,
    );
}

#[test]
fn thread_return_outside_a_lane_is_refused_by_issue_code() {
    refused(
        "functio f() → vacuum { redde filum }",
        "return_thread_outside_lane",
    );
}

#[test]
fn thread_return_with_a_value_is_refused_by_issue_code() {
    refused(&lane("redde filum 1"), "return_thread_with_value");
}

#[test]
fn thread_return_in_a_nested_closure_body_is_refused_by_issue_code() {
    // The independence proof also declines the closure on its own account;
    // the placement refusal must be among the reasons.
    let found = issues(&lane(
        "sit (f32) → vacuum f ← clausura f32 x → vacuum { redde filum }",
    ));
    assert!(
        found
            .iter()
            .any(|found| found == "return_thread_in_nested_body"),
        "{found:?}"
    );
}

#[test]
fn thread_return_in_a_closure_outside_any_lane_is_refused_as_outside() {
    refused(
        r"functio f() → vacuum {
    sit (f32) → vacuum g ← clausura f32 x → vacuum { redde filum }
}
",
        "return_thread_outside_lane",
    );
}

#[test]
fn thread_return_at_any_depth_of_its_lane_is_accepted_semantically() {
    accepted(&lane("varia i32 n ← 0\n dum n ≺ 4 { redde filum }"));
}

#[test]
fn return_inside_a_reduction_term_body_returns_the_term_value() {
    accepted(
        r"@ nucleum
functio k(tensor<f32, [32]> xs) → f32 {
    redde reducta via summa ex xs apud [i] filum f sit s { redde s }
}
",
    );
}

#[test]
fn return_inside_a_closure_in_the_lane_is_not_a_thread_return() {
    // The independence proof declines closures in a thread body on its own
    // account; the refusal under test must not be among the reasons.
    let found = issues(&lane(
        "sit (f32) → f32 twice ← f32 x ∴ x * 2.0\n out[i] ← twice(value)",
    ));
    assert!(
        found
            .iter()
            .any(|found| found == "filum_independence_unproven"),
        "{found:?}"
    );
    assert!(
        !found.iter().any(|found| found == RETURN || found == BREAK),
        "{found:?}"
    );
}

#[test]
fn raised_errors_stay_legal_in_the_lane() {
    accepted(
        r#"@ nucleum
@ publica
functio k(in tensor<f32, [4]> out, de tensor<f32, [4]> src) → vacuum ⇥ textus {
    itera ex src apud [i] filum lane sit value {
        si value ≻ 1.0 { iace "boom" }
        out[i] ← value
    }
}
"#,
    );
}
