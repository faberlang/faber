//! The typed zero of a type is written `Type {}`; the bare form is retired.

use super::{ParseResult, parse};
use radix_lexer::lex;
use radix_syntax::{ExprKind, StmtKind};

fn parse_source(source: &str) -> ParseResult {
    parse(lex(source))
}

fn issues(result: &ParseResult) -> Vec<String> {
    result
        .errors
        .iter()
        .flat_map(|error| error.args.iter())
        .filter(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .collect()
}

fn init_is_construct(source: &str) -> bool {
    let result = parse_source(source);
    assert!(result.success(), "{source}: {:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected a declaration: {source}");
    };
    matches!(
        decl.init.as_ref().map(|e| &e.kind),
        Some(ExprKind::Construct(_))
    )
}

#[test]
fn sugar_zero_requires_empty_braces() {
    assert!(init_is_construct("sit tf32[2, 3] a ← tf32[2, 3] {}"));
    assert!(init_is_construct("sit vf32[4] v ← vf32[4] {}"));
    assert!(init_is_construct("sit mf32[2, 2] m ← mf32[2, 2] {}"));
}

#[test]
fn long_form_zero_is_a_value_expression() {
    assert!(init_is_construct(
        "sit tensor<f32, [2, 3]> a ← tensor<f32, [2, 3]> {}"
    ));
    assert!(init_is_construct(
        "sit vector<f32, magnitudo 4> v ← vector<f32, magnitudo 4> {}"
    ));
    assert!(init_is_construct(
        "sit matrix<f32, [2, 2]> m ← matrix<f32, [2, 2]> {}"
    ));
    // A vector-element tensor.
    assert!(init_is_construct(
        "sit tensor<vector<f32, magnitudo 4>, [8]> a ← tensor<vector<f32, magnitudo 4>, [8]> {}"
    ));
}

#[test]
fn zero_is_an_ordinary_receiver() {
    let result = parse_source("sit tf32[4] a ← tf32[4] {}.reple(1.5)");
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn bare_form_is_retired_with_a_named_diagnostic() {
    for source in [
        "sit tf32[2] a ← tf32[2]",
        "sit tensor<f32, [2]> a ← tensor<f32, [2]>",
        "sit vector<f32, magnitudo 4> v ← vector<f32, magnitudo 4>",
    ] {
        let result = parse_source(source);
        assert!(
            issues(&result).contains(&"constructor_requires_empty_braces".to_owned()),
            "{source}: {:?}",
            result.errors
        );
    }
}

#[test]
fn bracket_suffix_is_not_a_zero() {
    let result = parse_source("sit tf32[2] a ← tf32[2] []");
    assert!(!result.success(), "`Type []` must be refused");
    assert!(
        issues(&result).contains(&"constructor_requires_empty_braces".to_owned()),
        "{:?}",
        result.errors
    );
}

#[test]
fn collection_type_zero_is_refused_by_name() {
    let result = parse_source("sit _ a ← lista<f32, magnitudo 4> {}");
    assert!(
        issues(&result).contains(&"constructor_type_not_claimed".to_owned()),
        "{:?}",
        result.errors
    );
}

#[test]
fn comparison_with_a_type_word_name_still_parses() {
    let result = parse_source("sit int vector ← 5\nsit bivalens b ← vector ≺ 7");
    assert!(result.success(), "{:?}", result.errors);
    let result = parse_source("sit int tensor ← 5\nsit bivalens b ← tensor ≺ 7");
    assert!(result.success(), "{:?}", result.errors);
    // The retired ASCII form still recovers as a comparison, reporting only itself.
    let result = parse_source("sit int vector ← 5\nsit bivalens b ← vector < 7");
    assert_eq!(issues(&result), vec!["comparison_ascii_retired".to_owned()]);
}
