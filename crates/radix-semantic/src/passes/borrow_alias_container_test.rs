//! al1: mutation through an unmarked alias of a `de` parameter is rejected for
//! every aliasing container carrier, not only `lista`/`tabula`.
//!
//! The classification is `TypeTable::is_aliasing_container`, shared with the
//! Rust alias-bind emitter; these tests pin the semantic side (R1: unmarked
//! binds alias, the access marker decides).

fn issues(body: &str) -> Vec<String> {
    let source = format!("incipit {{ }}\n{body}\n");
    let lex = radix_lexer::lex(&source);
    let mut parsed = radix_parser::parse(lex);
    assert!(parsed.success(), "parse: {:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    let config = crate::PassConfig::base();
    let result = crate::analyze(&program, &config, &mut parsed.interner);
    result
        .diagnostics
        .iter()
        .filter(|err| err.is_error())
        .map(|err| {
            err.args
                .iter()
                .find(|a| a.name == "issue")
                .map(|a| a.value.to_string())
                .unwrap_or_else(|| format!("{:?}", err.kind))
        })
        .collect()
}

fn assert_rejected(body: &str) {
    let found = issues(body);
    assert!(
        found.iter().any(|i| i == "assign_to_de_parameter"),
        "{found:?}"
    );
}

fn assert_accepted(body: &str) {
    let found = issues(body);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn copia_alias_of_de_param_rejects_mutating_method() {
    assert_rejected(
        "functio f(de copia<numerus> s) → vacuum { fixum copia<numerus> t ← s\n t.adde(1) }",
    );
}

#[test]
fn queue_alias_of_de_param_rejects_mutating_method() {
    assert_rejected(
        "functio f(de queue<numerus> q) → vacuum { fixum queue<numerus> r ← q\n r.appende(1) }",
    );
}

#[test]
fn stack_alias_of_de_param_rejects_mutating_method() {
    assert_rejected(
        "functio f(de stack<numerus> k) → vacuum { fixum stack<numerus> m ← k\n m.appende(1) }",
    );
}

#[test]
fn sparsa_alias_of_de_param_rejects_mutating_method() {
    assert_rejected(
        "functio f(de sparsa<f32, [2, 3]> p) → vacuum { fixum sparsa<f32, [2, 3]> r ← p\n r.ponde([0, 1], 4.0) }",
    );
}

#[test]
fn tensor_alias_of_de_param_rejects_element_write() {
    assert_rejected(
        "functio f(de tensor<f64, [3]> t) → vacuum { fixum tensor<f64, [3]> u ← t\n u[0] ← 1.0 }",
    );
}

#[test]
fn copia_alias_of_mutable_local_still_mutates() {
    assert_accepted(
        "functio f() → vacuum { varia copia<numerus> s ← vacua\n varia copia<numerus> t ← s\n t.adde(1) }",
    );
}

#[test]
fn copia_alias_of_de_param_allows_reads() {
    assert_accepted(
        "functio f(de copia<numerus> s) → bivalens { fixum copia<numerus> t ← s\n redde t.habet(1) }",
    );
}
