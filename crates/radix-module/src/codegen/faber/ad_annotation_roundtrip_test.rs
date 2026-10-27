//! Reader-locale round trip for the compiler-owned `@ ad '<route>'` handler
//! annotation (en `@ call '<route>'`).

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
            .with_latin_locale()
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

#[test]
fn ad_annotation_round_trips_between_en_and_la() {
    let en = "+++\nlocale = \"en\"\n+++\n@ call 'orders:list'\nfn orders(int n) → int {\n    return n\n}\n";
    let la = reemit(en, "la");
    assert!(la.contains("@ ad 'orders:list'\nfunctio orders"), "la must spell @ ad:\n{la}");
    let back = reemit(&la, "en");
    assert!(
        back.contains("@ call 'orders:list'\nfn orders"),
        "en must spell @ call:\n{back}"
    );
    assert_eq!(back, reemit(en, "en"), "en→la→en must agree with en format");
    assert_eq!(la, reemit(&la, "la"), "la format must be a fixed point");
}
