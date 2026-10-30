//! al2: an unmarked bind of an optional container aliases the container place
//! through the option (advisor ruling 2026-09-30 Q2).
//!
//! The classification is `TypeTable::binds_by_sharing` (and its
//! `_through_option` peel), shared with the Rust alias-bind emitter. These
//! tests pin the semantic side: a write through an unmarked alias of a `de`
//! parameter is rejected, and reads stay legal.
//!
//! Register-class `vector`/`matrix` values are silent values: `b ← a` copies
//! (operator ruling 7b25889f R1, which retires the 2026-09-30 Q1 aliasing
//! posture for them), so a lane write through an unmarked bind of a `de`
//! vector parameter writes the copy and is legal.

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
fn vector_copy_of_de_param_allows_element_write() {
    // R1: `w ← v` copies, so the lane write reaches `w` and never `v`.
    assert_accepted(
        "functio f(de vector<f32, magnitudo 4> v) → vacuum { varia vector<f32, magnitudo 4> w ← v\n w[0] ← 9.0 }",
    );
}

#[test]
fn vector_lane_write_into_a_de_param_is_still_rejected() {
    let found = issues("functio f(de vector<f32, magnitudo 4> v) → vacuum { v[0] ← 9.0 }");
    assert!(
        !found.is_empty(),
        "a lane write into a `de` parameter must be refused"
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
