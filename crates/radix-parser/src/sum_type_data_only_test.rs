//! Parser contract for DECISIONS D9.1: `ordo` and `discretio` are data only.
//!
//! A function declared inside a sum type, or an `implet` clause on its header,
//! reports one pointed `DeclarationInvalid` diagnostic (`sum_type_function` /
//! `sum_type_implements`) instead of a generic separator or expression error.

use crate::ParseErrorKind;
use crate::test_support::{has_arg, parse_ok, parse_program};

fn assert_single_issue(source: &str, issue: &str) {
    let result = parse_program(source);
    assert_eq!(
        result.errors.len(),
        1,
        "expected exactly one pointed error, got {:?}",
        result.errors
    );
    let error = &result.errors[0];
    assert_eq!(error.kind, ParseErrorKind::DeclarationInvalid);
    assert!(has_arg(error, "issue", issue), "got {error:?}");
}

#[test]
fn function_inside_ordo_is_sum_type_function() {
    assert_single_issue(
        "ordo Color {\n    rubrum,\n    functio f() → numerus { redde 1 }\n}\n",
        "sum_type_function",
    );
    assert_single_issue(
        "ordo Color {\n    functio f() → numerus { redde 1 }\n    rubrum\n}\n",
        "sum_type_function",
    );
    assert_single_issue(
        "ordo Color {\n    rubrum\n    functio f() → numerus { redde 1 }\n}\n",
        "sum_type_function",
    );
    assert_single_issue(
        "discretio Figura {\n    Circulus { fractus r }\n    functio area() → fractus { redde 0.0 }\n}\n",
        "sum_type_function",
    );
}

#[test]
fn function_inside_discretio_is_sum_type_function() {
    assert_single_issue(
        "discretio Figura {\n    Circulus { fractus r },\n    functio area() → fractus { redde 0.0 }\n}\n",
        "sum_type_function",
    );
    assert_single_issue(
        "discretio Figura {\n    functio area() → fractus { redde 0.0 }\n    Circulus { fractus r }\n}\n",
        "sum_type_function",
    );
}

#[test]
fn annotated_function_inside_discretio_is_sum_type_function() {
    assert_single_issue(
        "discretio Figura {\n    @ publica\n    functio area() → fractus { redde 0.0 }\n    Circulus { fractus r }\n}\n",
        "sum_type_function",
    );
}

#[test]
fn implet_on_sum_type_header_is_sum_type_implements() {
    assert_single_issue(
        "ordo Color implet Nominatum { rubrum }\n",
        "sum_type_implements",
    );
    assert_single_issue(
        "discretio Figura implet Mensurabile { Circulus }\n",
        "sum_type_implements",
    );
    assert_single_issue(
        "discretio Capsa<T> implet Mensurabile { Plena { T valor } }\n",
        "sum_type_implements",
    );
}

#[test]
fn recovery_resumes_after_the_rejected_member() {
    let result = parse_program(
        "ordo Color {\n    rubrum,\n    functio f() → numerus { redde 1 }\n}\nfunctio g() → numerus { redde 2 }\n",
    );
    assert_eq!(result.errors.len(), 1, "got {:?}", result.errors);
    let program = result.program.expect("program");
    assert_eq!(program.statements.len(), 2);
}

#[test]
fn plain_sum_types_still_parse() {
    parse_ok("ordo Color { rubrum, viridis }\n");
    parse_ok(
        "discretio Figura {\n    Circulus { fractus r },\n    Quadratum { fractus latus }\n}\n",
    );
}
