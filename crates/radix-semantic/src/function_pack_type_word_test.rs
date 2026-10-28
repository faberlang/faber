//! A function may share a reader-pack type spelling.
//!
//! Pack type words (`tensor`, `list`, `int`, ...) are contextual, not
//! reserved: `fn tensor(...)` declares a value that calls resolve to, the type
//! spelling stays usable in type position, and only a user-declared *type* of
//! the same name is a duplicate. The rule is the same under every pack.

use crate::locale::LocalePack;
use crate::{PassConfig, test_support};
use radix_target::Target;

fn analyze_with(id: &str, source: &str) -> test_support::AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
            .expect("installed pack should load"),
    );
    test_support::analyze_source_with_config(source, config)
}

fn has_issue(result: &test_support::AnalyzedFixture, issue: &str) -> bool {
    result.diagnostics.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)
    })
}

const EN_FN: &str = r"
fn tensor(int x) → int {
    return x + 1
}

fn dim(tensor<f32, [2]> t) → int {
    return 2
}

fn caller() → int {
    return tensor(3)
}
";

const LA_FN: &str = r"
functio tensor(numerus x) → numerus {
    redde x + 1
}

functio dim(tensor<f32, [2]> t) → numerus {
    redde 2
}

functio caller() → numerus {
    redde tensor(3)
}
";

#[test]
fn en_function_may_share_a_pack_type_word() {
    let result = analyze_with("en", EN_FN);
    assert!(!has_issue(&result, "duplicate_definition"));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn la_function_may_share_a_pack_type_word() {
    let result = analyze_with("la", LA_FN);
    assert!(!has_issue(&result, "duplicate_definition"));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn user_type_of_a_pack_type_word_is_still_a_duplicate() {
    let result = analyze_with("en", "type tensor = int\n");
    assert!(has_issue(&result, "duplicate_definition"));
}

#[test]
fn function_and_user_type_of_one_name_are_still_a_duplicate() {
    let result = analyze_with(
        "en",
        "type thing = int\nfn thing(int x) → int {\n    return x\n}\n",
    );
    assert!(has_issue(&result, "duplicate_definition"));
}
