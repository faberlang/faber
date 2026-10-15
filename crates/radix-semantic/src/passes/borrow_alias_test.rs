//! tc1: aliases of read-only (`de`) places carry the read-only access.
//!
//! Unmarked binds alias (D12.1), so mutation through an alias of a `de`
//! parameter is rejected exactly like direct mutation, for plain genus binds
//! and contract-typed binds alike.

const PRELUDE: &str = r"
implendum Crescens {
    functio cresce(numerus per) → vacuum
    functio adde(in numerus k) → vacuum
    functio mensura() → numerus
}
genus Quadratum implet Crescens {
    varia numerus latus
    functio cresce(numerus per) → vacuum { ego.latus ← ego.latus + per }
    functio adde(in numerus k) → vacuum { ego.latus ← ego.latus + k }
    functio mensura() → numerus { redde ego.latus * ego.latus }
}
genus Punctum { varia numerus x }
incipit { }
";

fn latin_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn issues(body: &str) -> Vec<String> {
    let source = format!("{PRELUDE}\n{body}\n");
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
fn plain_genus_alias_of_de_param_rejects_field_write() {
    assert_rejected("functio f(de Punctum q) → vacuum { sit Punctum b ← q\n b.x ← 5 }");
}

#[test]
fn contract_alias_of_de_param_rejects_mutating_method() {
    assert_rejected("functio f(de Quadratum q) → vacuum { sit Crescens c ← q\n c.cresce(1) }");
}

#[test]
fn direct_mutation_of_de_param_still_rejected() {
    assert_rejected("functio f(de Quadratum q) → vacuum { q.cresce(1) }");
}

#[test]
fn contract_alias_of_de_param_allows_non_mutating_call() {
    assert_accepted(
        "functio f(de Quadratum q) → numerus { sit Crescens c ← q\n redde c.mensura() }",
    );
}

#[test]
fn fixum_contract_alias_of_de_param_rejects_mut_param_call() {
    assert_rejected(
        "functio f(de Quadratum q) → numerus { sit Crescens c ← q\n varia numerus k ← 1\n c.adde(k)\n redde 1 }",
    );
}

#[test]
fn alias_of_de_param_passed_to_mut_param_is_rejected() {
    let found = issues(
        "functio g(in Quadratum x) → vacuum { x.cresce(1) }\nfunctio f(de Quadratum q) → vacuum { sit Quadratum c ← q\n g(c) }",
    );
    assert!(
        found.iter().any(|i| i == "de_to_in_or_ex_position"),
        "{found:?}"
    );
}

#[test]
fn contract_alias_of_mutable_local_still_mutates() {
    assert_accepted(
        "functio f() → vacuum { varia Quadratum q ← Quadratum { latus = 1 }\n sit Crescens c ← q\n c.cresce(1) }",
    );
}

#[test]
fn lista_alias_of_de_param_rejects_mutating_method() {
    assert_rejected(
        "functio f(de lista<numerus> xs) → vacuum { sit lista<numerus> ys ← xs\n ys.appende(9) }",
    );
}

#[test]
fn lista_alias_of_de_param_rejects_element_write() {
    assert_rejected(
        "functio f(de lista<numerus> xs) → vacuum { sit lista<numerus> ys ← xs\n ys[0] ← 1 }",
    );
}

#[test]
fn tabula_alias_of_de_param_rejects_mutating_method() {
    assert_rejected(
        "functio f(de tabula<textus, numerus> m) → vacuum { sit tabula<textus, numerus> n ← m\n n.pone(\"a\", 1) }",
    );
}

#[test]
fn nested_collection_field_alias_of_de_param_rejects_mutating_method() {
    assert_rejected(
        "genus Cont { varia lista<numerus> items }\nfunctio f(de Cont q) → vacuum { sit lista<numerus> ls ← q.items\n ls.appende(1) }",
    );
}

#[test]
fn lista_alias_of_de_param_allows_reads() {
    assert_accepted(
        "functio f(de lista<numerus> xs) → numerus { sit lista<numerus> ys ← xs\n redde ys.longitudo() }",
    );
}

#[test]
fn lista_alias_of_mutable_local_still_mutates() {
    assert_accepted(
        "functio f() → vacuum { varia lista<numerus> xs ← [1]\n varia lista<numerus> ys ← xs\n ys.appende(9) }",
    );
}
