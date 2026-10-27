//! Faber re-emit of the value conditional `c ✓ a ✗ b` (DECISIONS D4.5).
//!
//! The emitter writes the one sanctioned spelling in every reader locale and
//! never a chained (nested) conditional, since the parser rejects nesting
//! (D4.6) and emitted Faber must re-parse.

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;

fn reemit(source: &str, to: &str) -> String {
    let pack = LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(to))
        .unwrap_or_else(|err| panic!("installed {to} pack should load: {err:?}"));
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = reemit_faber(&session, &pack, "test.fab", source);
    assert!(
        result.success(),
        "source must re-emit to {to} and re-validate:\n{source}\n{:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    result
        .output
        .expect("successful re-emit must carry output")
        .code
}

const EN: &str = "+++\nlocale = \"en\"\n+++\nfn f(int n) → string {\n    return n ≥ 18 ✓ \"adult\" ✗ \"minor\"\n}\n";

#[test]
fn conditional_reemits_check_ballot_in_every_locale() {
    let la = reemit(EN, "la");
    assert!(la.contains("n ≥ 18 ✓ \"adult\" ✗ \"minor\""), "{la}");
    assert!(!la.contains("sic"), "{la}");
    let back = reemit(&la, "en");
    assert!(back.contains("n ≥ 18 ✓ \"adult\" ✗ \"minor\""), "{back}");
    assert!(!back.contains("yields"), "{back}");
    assert_eq!(back, reemit(EN, "en"));
}

fn reemit_issues(source: &str) -> Vec<String> {
    let pack = LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirFaber)
            .with_dev_locale_root()
            .with_locale_pack(crate::locale::latin_locale_pack()),
    );
    let result = reemit_faber(&session, &pack, "test.fab", source);
    result
        .diagnostics
        .iter()
        .filter_map(|d| d.issue().map(str::to_owned))
        .collect()
}

#[test]
fn en_yields_else_reports_migration_to_check_ballot() {
    let issues = reemit_issues(
        "+++\nlocale = \"en\"\n+++\nfn f(int n) → string {\n    return n ≥ 18 yields \"adult\" else \"minor\"\n}\n",
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue == "conditional_sic_removed"),
        "{issues:?}"
    );
}
