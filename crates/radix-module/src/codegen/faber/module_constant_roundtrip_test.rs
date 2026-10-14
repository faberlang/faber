//! Module-level constants survive Faber re-emission spelled `fixum` (st1 R15).
//!
//! A top-level `fixum T X = e` is the module constant, so it re-emits with the
//! pack's `fixum` spelling (en `const`) and the authored initializer as
//! written. The genus `generis` field keeps its modifier.

use crate::driver::reemit_faber;
use crate::codegen::Target;
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

const BODY: &str = "const int LIMIT = 4 * 1024\nconst _ INFERRED = 10\nconst string NAME = \"ab\" + \"cd\"\n\nclass Punctum {\n    const int x\n    static int Z = 3\n}\n\nmain {\n    print LIMIT\n}\n";

fn en_source() -> String {
    format!("+++\nlocale = \"en\"\n+++\n{BODY}")
}

#[test]
fn en_format_spells_module_constants_const_and_keeps_the_field_modifier() {
    let formatted = reemit(&en_source(), "en");
    assert!(formatted.contains("const int LIMIT = 4 * 1024\n"), "{formatted}");
    assert!(formatted.contains("const _ INFERRED = 10\n"), "{formatted}");
    assert!(formatted.contains("const string NAME = \"ab\" + \"cd\"\n"), "{formatted}");
    assert!(formatted.contains("    static int Z = 3\n"), "{formatted}");
    assert!(!formatted.contains("\nstatic "), "{formatted}");
}

#[test]
fn en_format_is_stable() {
    let once = reemit(&en_source(), "en");
    assert_eq!(reemit(&format!("+++\nlocale = \"en\"\n+++\n{once}"), "en"), once);
}

#[test]
fn en_to_la_prints_fixum_and_round_trips() {
    let la = reemit(&en_source(), "la");
    assert!(la.contains("fixum numerus LIMIT = 4 * 1024\n"), "{la}");
    assert!(la.contains("fixum _ INFERRED = 10\n"), "{la}");
    assert!(la.contains("    generis numerus Z = 3\n"), "{la}");
    assert_eq!(reemit(&la, "en"), reemit(&en_source(), "en"));
}
