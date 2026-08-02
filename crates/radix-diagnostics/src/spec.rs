//! Shared diagnostic catalog types and reader-locale fallback entries.
//!
//! Phase-specific catalogs (lex/parse/semantic kinds) stay in the `radix`
//! package until those error enums live in foundation crates.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Catalog entry attached to a phase-specific error kind.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DiagnosticSpec {
    /// Stable diagnostic code, such as `LEX001` or `SEM033`.
    pub code: &'static str,

    /// Generic remediation shown when a phase does not provide custom help.
    pub help: Option<&'static str>,
}

/// Return the stable diagnostic spec for visible reader-locale fallback.
#[must_use]
pub fn reader_locale_fallback_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "READER001",
        help: Some(
            "use the pack spelling or add the Latin spelling as an explicit alias if the bridge is intentional",
        ),
    }
}

/// Return the stable diagnostic spec for reader-locale spelling suggestions.
#[must_use]
pub fn reader_locale_suggestion_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "READER002",
        help: Some(
            "use the suggested pack spelling or add the spelling as an explicit alias if it is intentional",
        ),
    }
}

/// Return the stable diagnostic spec for an unknown or invalid reader locale.
#[must_use]
pub fn reader_locale_unknown_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "READER003",
        help: Some(
            "check the locale id is a valid reader locale directory name under the standard library reader path",
        ),
    }
}
