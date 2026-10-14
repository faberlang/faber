//! LOCALE002 is an advisory for a guessed reading only.
//!
//! A reader pack resolved from a source declaration, a package manifest, or
//! `--locale` is a caller decision, so a failed analysis under it must not
//! second-guess the reading with `source_reads_as_other_locale`. The advisory
//! belongs to the product-default reading that nothing declared.

use radix_module::driver::{AnalyzeRequest, Config, Session, analyze_source};
use radix_module::locale::{LocalePack, workspace_locale_pack_path};

/// Installed `en` reader pack from the workspace locale tree.
fn en_pack() -> LocalePack {
    LocalePack::from_toml_path(workspace_locale_pack_path("en")).expect("installed en pack should load")
}

/// Latin keywords read under the `en` pack: analysis fails, so the driver's
/// failed-analysis path would otherwise append the wrong-locale advisory. The
/// block stays empty so the lexer's per-spelling near-miss suggestions (a
/// separate LOCALE002 family) do not fire; this isolates the reading note.
const LATIN_SOURCE: &str = "incipit { }";

/// Diagnostics from a failed analysis under `config`. The source must fail
/// under the `en` reading, which is what makes the advisory path run.
fn diagnostics_under(config: Config) -> Vec<radix_module::Diagnostic> {
    let session = Session::new(config);
    match analyze_source(&session, "explicit-pack.fab", LATIN_SOURCE, AnalyzeRequest::default()) {
        Ok(_) => panic!("latin surface must not analyze under the en pack"),
        Err(diagnostics) => diagnostics,
    }
}

fn locale002_diagnostics(diagnostics: &[radix_module::Diagnostic]) -> Vec<&radix_module::Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == Some("LOCALE002"))
        .collect()
}

fn has_issue(diagnostic: &radix_module::Diagnostic, issue: &str) -> bool {
    diagnostic
        .args
        .iter()
        .any(|arg| arg.name == "issue" && arg.value == issue)
}

#[test]
fn explicit_resolved_pack_draws_no_locale002() {
    let diagnostics = diagnostics_under(Config::default().with_resolved_locale_pack(en_pack()));
    let notes = locale002_diagnostics(&diagnostics);
    assert!(
        notes.is_empty(),
        "an explicitly resolved pack is never a guess: {diagnostics:?}"
    );
}

#[test]
fn implicit_pack_still_draws_locale002() {
    let diagnostics = diagnostics_under(Config::default().with_locale_pack(en_pack()));
    let notes = locale002_diagnostics(&diagnostics);
    assert!(
        notes
            .iter()
            .any(|note| has_issue(note, "source_reads_as_other_locale")),
        "the undeclared default reading stays second-guessed: {diagnostics:?}"
    );
}
