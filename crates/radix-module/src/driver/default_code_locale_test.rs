//! Test-support checks for the temporary `RADIX_DEFAULT_CODE_LOCALE` staging
//! switch and the Latin session builder.
//!
//! Need: every route reads `en` when nothing declares a locale; until
//! the final unit flips that default in production, the switch proves both
//! states. The switch test writes the process-wide variable, so it lives in
//! its own test binary (`tests/default_locale_switch.rs`) where it cannot race
//! the pack-less lexing reads performed by this shared library test binary.
//! This file keeps only the env-free Latin-session case.

use super::{AnalyzeRequest, Config, Session};

/// The untagged Latin source: lexes only under the Latin pack.
const LATIN_SOURCE: &str = "incipit { nota 1 }";

#[test]
fn config_with_latin_locale_checks_latin_source() {
    let session = Session::new(Config::default().with_latin_locale());
    crate::driver::analyze_source(&session, "latin.fab", LATIN_SOURCE, AnalyzeRequest::default())
        .expect("latin source must check under with_latin_locale");
}
