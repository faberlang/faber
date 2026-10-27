//! The no-locale default reader pack is `en`, unconditionally; a default pack
//! that cannot load is an error, never a Latin fallback.

use super::{AnalyzeRequest, Config, Session, default_code_pack};

/// The untagged English source: lexes only under the `en` pack.
const EN_SOURCE: &str = "main { print 1 }";

/// The untagged Latin source: lexes only under the Latin pack.
const LATIN_SOURCE: &str = "incipit { nota 1 }";

#[test]
fn default_code_pack_is_en() {
    let pack = default_code_pack(&Config::default()).expect("en pack loads from the dev locale root");
    assert_eq!(pack.metadata.id, "en");
}

#[test]
fn default_code_pack_without_locale_root_is_an_error() {
    let config = Config { locale_root: None, ..Config::default() };
    assert!(
        default_code_pack(&config).is_err(),
        "a default pack that cannot load must not fall back"
    );
}

#[test]
fn untagged_english_source_checks_without_any_switch() {
    let result = crate::services::check_diagnostics_for_source("main.fab", EN_SOURCE, false);
    assert!(result.success, "untagged en source must check: {:?}", result.diagnostics);
}

#[test]
fn untagged_latin_source_is_not_read_as_latin_by_default() {
    let result = crate::services::check_diagnostics_for_source("main.fab", LATIN_SOURCE, false);
    assert!(!result.success, "untagged Latin source must not check under the en default");
}

#[test]
fn config_with_latin_locale_checks_latin_source() {
    let session = Session::new(Config::default().with_latin_locale());
    crate::driver::analyze_source(&session, "latin.fab", LATIN_SOURCE, AnalyzeRequest::default())
        .expect("latin source must check under with_latin_locale");
}
