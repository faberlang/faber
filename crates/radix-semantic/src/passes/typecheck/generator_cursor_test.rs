//! A6c: a `fiunt` call is typed `cursor<T>`, not `lista<T>`.

use crate::PassConfig;
use radix_target::Target;

fn errors(source: &str) -> usize {
    let lex = radix_lexer::lex(source);
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::HirRust);
    let result = crate::analyze(&program, &config, &mut parse.interner);
    result
        .diagnostics
        .iter()
        .filter(|diagnostic| !matches!(diagnostic.kind, crate::SemanticErrorKind::Warning(_)))
        .count()
}

const GEN: &str = "functio gen() fiunt → numerus {\n    cede 1\n}\n";

fn program(body: &str) -> String {
    format!("{GEN}\nincipit {{\n    {body}\n}}\n")
}

#[test]
fn generator_call_binds_to_cursor() {
    assert_eq!(errors(&program("fixum cursor<numerus> c ← gen()")), 0);
}

#[test]
fn generator_call_is_not_a_lista() {
    assert!(errors(&program("fixum lista<numerus> xs ← gen()")) > 0);
}

#[test]
fn generator_call_collects_to_lista_by_conversion() {
    assert_eq!(
        errors(&program("fixum lista<numerus> xs ← gen() ↦ lista<numerus>")),
        0
    );
}
