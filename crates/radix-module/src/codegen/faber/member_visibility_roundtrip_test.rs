//! Genus member visibility survives Faber re-emission (DECISIONS D5.3).
//!
//! `@ privata` / `@ interna` on a field or method is a checked visibility, so
//! format and convert must keep it on every member in the target locale.

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;

fn pack(locale: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

fn reemit(source: &str, to: &str) -> String {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = reemit_faber(&session, &pack(to), "test.fab", source);
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

const BODY: &str = "class Arca {\n    @ private\n    mut int secretum\n    @ internal\n    mut int domesticum\n\n    fn lege() → int {\n        return self.occulta()\n    }\n\n    @ private\n    fn occulta() → int {\n        return self.secretum\n    }\n}\n\nmain {\n    let Arca a ← Arca { secretum = 1, domesticum = 2 }\n    print a.lege()\n}\n";

#[test]
fn en_format_keeps_member_visibility() {
    let en = format!("+++\nlocale = \"en\"\n+++\n{BODY}");
    let formatted = reemit(&en, "en");
    assert!(formatted.contains("    @ private\n    mut int secretum\n"), "{formatted}");
    assert!(formatted.contains("    @ internal\n    mut int domesticum\n"), "{formatted}");
    assert!(formatted.contains("    @ private\n    fn occulta()"), "{formatted}");
}

#[test]
fn en_to_la_round_trip_keeps_member_visibility() {
    let en = format!("+++\nlocale = \"en\"\n+++\n{BODY}");
    let la = reemit(&en, "la");
    assert!(la.contains("    @ privata\n    varia numerus secretum\n"), "{la}");
    assert!(la.contains("    @ interna\n    varia numerus domesticum\n"), "{la}");
    assert_eq!(reemit(&la, "en"), reemit(&en, "en"));
}
