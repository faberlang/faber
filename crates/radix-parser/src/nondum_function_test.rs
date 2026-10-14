//! Parser contract for `@ nondum` free functions and genus methods.
//!
//! A top-level `functio` or a genus method annotated `@ nondum` may omit its
//! body: it is present in the interface and unavailable, and semantic analysis
//! refuses any use. Without the annotation a missing body stays
//! `FunctionBodyAbsent`, and only the annotated declaration (never a later
//! declaration or a nested function) gets the exemption.

use crate::test_support::{ParseExpectation, assert_parse_error_matches_la, parse_ok_la};
use crate::{ParseErrorKind, ParseResult};
use radix_syntax::{AnnotationKind, StmtKind};

fn only_function(result: &ParseResult, index: usize) -> &radix_syntax::FuncDecl {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[index].kind else {
        panic!("expected functio declaration at statement {index}");
    };
    func
}

#[test]
fn bodyless_nondum_sugar_function_parses() {
    let result =
        parse_ok_la("@ nondum \"pending host dispatch\"\nfunctio parse(textus s) → textus\n");
    let func = only_function(&result, 0);
    assert!(func.body.is_none());
    assert_eq!(func.params.len(), 1);
    assert!(func.ret.is_some());
}

#[test]
fn bodyless_nondum_function_keeps_its_annotation() {
    let result = parse_ok_la("@ nondum rs \"later\"\nfunctio parse(textus s) → textus\n");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements[0].annotations.len(), 1);
    let AnnotationKind::Statement(annotation) = &program.statements[0].annotations[0].kind else {
        panic!("expected statement annotation");
    };
    assert_eq!(result.interner.resolve(annotation.name.name), "nondum");
    assert_eq!(annotation.args.len(), 2);
}

#[test]
fn bodyless_nondum_braced_function_parses() {
    let result = parse_ok_la(
        "@ nondum { target = rs, ratio = \"later\" }\nfunctio parse(textus s) → textus\n",
    );
    assert!(only_function(&result, 0).body.is_none());
}

#[test]
fn bodyless_nondum_function_composes_with_other_annotations() {
    let result = parse_ok_la("@ publica\n@ nondum \"later\"\nfunctio parse(textus s) → textus\n");
    assert!(only_function(&result, 0).body.is_none());
}

#[test]
fn bodyless_nondum_function_with_error_channel_parses() {
    let result = parse_ok_la("@ nondum \"later\"\nfunctio parse(textus s) → textus ⇥ textus\n");
    let func = only_function(&result, 0);
    assert!(func.body.is_none());
    assert!(func.err.is_some());
}

#[test]
fn nondum_function_with_a_body_still_parses() {
    let result = parse_ok_la("@ nondum \"later\"\nfunctio parse(textus s) → textus { redde s }\n");
    assert!(only_function(&result, 0).body.is_some());
}

#[test]
fn consecutive_bodyless_nondum_functions_parse() {
    let result = parse_ok_la(
        "@ nondum \"a\"\nfunctio a() → numerus\n@ nondum \"b\"\nfunctio b() → numerus\n",
    );
    assert!(only_function(&result, 0).body.is_none());
    assert!(only_function(&result, 1).body.is_none());
}

#[test]
fn bodyless_function_without_nondum_is_function_body_absent() {
    assert_parse_error_matches_la(
        "functio parse(textus s) → textus\n",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}

#[test]
fn other_annotations_do_not_exempt_a_missing_body() {
    assert_parse_error_matches_la(
        "@ publica\nfunctio parse(textus s) → textus\n",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}

#[test]
fn nondum_exemption_does_not_leak_to_the_next_declaration() {
    assert_parse_error_matches_la(
        "@ nondum \"later\"\nfunctio a() → numerus\nfunctio b() → numerus\n",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}

fn only_method(result: &ParseResult, index: usize) -> &radix_syntax::FuncDecl {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus declaration");
    };
    let methods: Vec<_> = class
        .members
        .iter()
        .filter_map(|member| match &member.kind {
            radix_syntax::ClassMemberKind::Method(method) => Some(method),
            radix_syntax::ClassMemberKind::Field(_) => None,
        })
        .collect();
    methods[index]
}

#[test]
fn bodyless_nondum_genus_method_parses() {
    let result = parse_ok_la(
        "genus Animal {\n    @ nondum \"later\"\n    functio sonus() → textus\n    functio nomen() → textus { redde \"x\" }\n}\n",
    );
    assert!(only_method(&result, 0).body.is_none());
    assert!(only_method(&result, 1).body.is_some());
}

#[test]
fn bodyless_nondum_braced_genus_method_parses() {
    let result = parse_ok_la(
        "genus Animal {\n    @ nondum { target = rs, ratio = \"later\" }\n    functio sonus() → textus\n}\n",
    );
    assert!(only_method(&result, 0).body.is_none());
}

#[test]
fn bodyless_genus_method_without_nondum_is_function_body_absent() {
    assert_parse_error_matches_la(
        "genus Animal {\n    @ publica\n    functio sonus() → textus\n}\n",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}

#[test]
fn nondum_genus_method_exemption_does_not_leak_to_the_next_method() {
    assert_parse_error_matches_la(
        "genus Animal {\n    @ nondum \"later\"\n    functio a() → numerus\n    functio b() → numerus\n}\n",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}

#[test]
fn nondum_on_a_non_function_does_not_exempt_nested_functions() {
    assert_parse_error_matches_la(
        "@ nondum \"later\"\nincipit {\n    functio inner() → numerus\n}\n",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}
