//! Tests for the temporary `RADIX_DEFAULT_CODE_LOCALE` staging switch and the
//! test-support Latin session builder.
//!
//! Need: every route reads `en` when nothing declares a locale; until
//! the final unit flips that default in production, the switch proves both
//! states. These tests are the only writers of the process-wide variable in
//! this file, so a process-wide mutex serializes them.

use super::{AnalyzeRequest, Config, Session};

/// The untagged English source: lexes only under the `en` pack.
const EN_SOURCE: &str = "main { print 1 }";

/// The untagged Latin source: lexes only under the Latin pack.
const LATIN_SOURCE: &str = "incipit { nota 1 }";

static CODE_LOCALE_SWITCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds the switch mutex and clears `RADIX_DEFAULT_CODE_LOCALE` on drop, so a
/// panic inside a guarded test cannot leak the variable to the next test.
struct CodeLocaleSwitch {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Drop for CodeLocaleSwitch {
    fn drop(&mut self) {
        // SAFETY: `_lock` is held for the whole guard lifetime, so no other
        // test in this file can be mutating the variable concurrently.
        unsafe { std::env::remove_var("RADIX_DEFAULT_CODE_LOCALE") };
    }
}

fn set_code_locale_switch(value: Option<&str>) -> CodeLocaleSwitch {
    let lock = CODE_LOCALE_SWITCH_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: the returned guard holds the process-wide lock until after the
    // variable is cleared in its `Drop`.
    unsafe {
        match value {
            Some(value) => std::env::set_var("RADIX_DEFAULT_CODE_LOCALE", value),
            None => std::env::remove_var("RADIX_DEFAULT_CODE_LOCALE"),
        }
    }
    CodeLocaleSwitch { _lock: lock }
}

#[test]
fn default_code_pack_en_when_switch_set() {
    {
        let _switch = set_code_locale_switch(Some("en"));
        let result = crate::services::check_diagnostics_for_source("main.fab", EN_SOURCE, false);
        assert!(result.success, "en source must check under the switch: {:?}", result.diagnostics);
    }
    {
        let _switch = set_code_locale_switch(None);
        let result = crate::services::check_diagnostics_for_source("main.fab", EN_SOURCE, false);
        assert!(
            !result.success,
            "en source must fail with the switch unset (pack-less Latin reading): {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn config_with_latin_locale_checks_latin_source() {
    let session = Session::new(Config::default().with_latin_locale());
    crate::driver::analyze_source(&session, "latin.fab", LATIN_SOURCE, AnalyzeRequest::default())
        .expect("latin source must check under with_latin_locale");
}
