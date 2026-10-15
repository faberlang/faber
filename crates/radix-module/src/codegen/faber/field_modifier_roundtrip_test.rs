//! Genus field modifiers survive Faber re-emission (DECISIONS D16.1).
//!
//! `sit` / `varia` / `fixum` are recorded on the field and re-emitted in
//! the target locale (`const` / `var` / `static` in en). An unmarked field
//! is a parse error (D5c), so every re-emitted field carries its modifier.

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

const BODY: &str = "class Punctum {\n    let int x\n    mut int y\n    const int Z = 3\n}\n\nmain {\n    let Punctum p ← Punctum { x = 1, y = 2 }\n    print p.x\n}\n";

#[test]
fn en_format_keeps_each_field_modifier() {
    let en = format!("+++\nlocale = \"en\"\n+++\n{BODY}");
    let formatted = reemit(&en, "en");
    assert!(formatted.contains("    let int x\n"), "{formatted}");
    assert!(formatted.contains("    mut int y\n"), "{formatted}");
    assert!(formatted.contains("    const int Z = 3\n"), "{formatted}");
}

#[test]
fn en_to_la_round_trip_keeps_field_modifiers() {
    let en = format!("+++\nlocale = \"en\"\n+++\n{BODY}");
    let la = reemit(&en, "la");
    assert!(la.contains("    sit numerus x\n"), "{la}");
    assert!(la.contains("    varia numerus y\n"), "{la}");
    assert!(la.contains("    fixum numerus Z = 3\n"), "{la}");
    assert_eq!(reemit(&la, "en"), reemit(&en, "en"));
}
