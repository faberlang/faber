//! Reader-locale formatter round trip for `return thread` / `redde filum`.

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;

fn pack(locale: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

fn source(locale: &str, body: &str) -> String {
    format!("+++\nlocale = \"{locale}\"\n+++\n{body}")
}

fn reemit(source: &str, target: &str) -> String {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = reemit_faber(&session, &pack(target), "return-thread.fab", source);
    assert!(
        result.success(),
        "source must re-emit to {target}: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue(), diagnostic.args.clone()))
            .collect::<Vec<_>>()
    );
    result.output.expect("successful re-emission output").code
}

#[test]
fn thread_return_round_trips_in_the_source_locale() {
    let english_body = "@ kernel\nfn lane_exit(tensor<f32, [4]> values) → void {\n    for from values at [i] thread lane let value {\n        return thread\n    }\n}\n";
    let english = source("en", english_body);
    let formatted_english = reemit(&english, "en");
    assert!(formatted_english.contains("return thread"), "{formatted_english}");

    let latin = reemit(&source("en", &formatted_english), "la");
    assert!(latin.contains("redde filum"), "{latin}");
    let formatted_latin = reemit(&source("la", &latin), "la");
    assert!(formatted_latin.contains("redde filum"), "{formatted_latin}");
    let back_to_english = reemit(&source("la", &formatted_latin), "en");
    assert_eq!(
        formatted_english, back_to_english,
        "en → la → en should preserve the source spelling and canonical formatting"
    );
}
