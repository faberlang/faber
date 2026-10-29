//! al2: register-class `vector`/`matrix` and optional containers alias on an
//! unmarked bind, like every other aggregate (advisor ruling 2026-09-30,
//! Q1/Q2; R1 in `faber/docs/semantics-rulings.md` has no register-class
//! carve-out).
//!
//! The classification is `TypeTable::is_aliasing_container` (and its
//! `_through_option` peel), shared with the Rust alias-bind emitter. These
//! tests pin the semantic side: a write through an unmarked alias of a `de`
//! parameter is rejected, and reads stay legal.

fn latin_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn issues(body: &str) -> Vec<String> {
    let source = format!("incipit {{ }}\n{body}\n");
    let lex = radix_lexer::lex_with_locale_pack(&source, &latin_pack());
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
                .map_or_else(|| format!("{:?}", err.kind), |a| a.value.clone())
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
fn vector_alias_of_de_param_rejects_element_write() {
    assert_rejected(
        "functio f(de vector<f32, magnitudo 4> v) → vacuum { sit vector<f32, magnitudo 4> w ← v\n w[0] ← 9.0 }",
    );
}

#[test]
fn vector_alias_of_de_param_allows_reads() {
    assert_accepted(
        "functio f(de vector<f32, magnitudo 4> v) → fractus { sit vector<f32, magnitudo 4> w ← v\n redde w[0] }",
    );
}

#[test]
fn vector_alias_of_mutable_local_still_writes() {
    assert_accepted(
        "functio f() → vacuum { varia vector<f32, magnitudo 4> v ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, magnitudo 4>\n varia vector<f32, magnitudo 4> w ← v\n w[0] ← 9.0 }",
    );
}

#[test]
fn matrix_alias_of_de_param_allows_reads() {
    assert_accepted(
        "functio f(de matrix<f32, [2, 2]> m) → vacuum { sit matrix<f32, [2, 2]> n ← m }",
    );
}
