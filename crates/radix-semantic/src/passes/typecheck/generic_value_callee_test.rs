//! GAPS #33: a value callee's function-pointer type borrows the enclosing
//! function's rigid type parameters.
//!
//! Inside `fn ap<T, U>((T) → U f, list<T> xs)`, `f`'s signature declares no
//! type parameters of its own; its `T` and `U` are `ap`'s rigid params. The
//! call-site substitution must keep them rigid instead of minting fresh
//! inference holes that nothing can anchor.
//!
//! The fixtures are English-surface Faber, so they lex through the `en` reader
//! pack rather than the registry-direct Latin surface [`analyze_source`] uses.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config};
use radix_target::Target;

/// The installed English reader pack.
fn en_pack() -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load")
}

/// Analyze English-surface source with the Rust-shaped pass config.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(en_pack());
    analyze_source_with_config(source, config)
}

/// Diagnostic issue names present in a fixture, never message text.
fn issue_names(fixture: &AnalyzedFixture) -> Vec<String> {
    fixture
        .diagnostics
        .iter()
        .filter(|diag| diag.is_error())
        .filter_map(|diag| {
            diag.args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

/// GAPS #33: `out.append(f(x))` must typecheck; the borrowed `U` stays rigid.
#[test]
fn value_callee_result_in_argument_position() {
    let fixture = analyze_en(
        r#"fn ap<T, U>((T) → U f, list<T> xs) → list<U> {
    mut list<U> out ← empty
    for from xs let x {
        out.append(f(x))
    }
    return out
}

main {
    print ap(int x ∴ x + 1, [4, 5])
}
"#,
    );
    let issues = issue_names(&fixture);
    assert!(
        !issues
            .iter()
            .any(|issue| issue == "cannot_infer_expression_type"),
        "value callee call left an unanchorable type hole: {issues:?}"
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
}

/// Control: a genuinely generic named-function call still solves its param from
/// the argument type.
#[test]
fn generic_named_call_still_infers_from_arguments() {
    let fixture = analyze_en(
        r#"fn id<T>(T x) → T {
    return x
}

main {
    let int y ← id(3)
    print y
}
"#,
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
}
