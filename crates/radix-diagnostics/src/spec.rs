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

/// Return the stable diagnostic spec for reader-locale spelling suggestions.
#[must_use]
pub fn locale_suggestion_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "LOCALE002",
        help: Some(
            "use the suggested pack spelling if you meant the keyword; the written spelling is a valid name",
        ),
    }
}

/// Return the stable diagnostic spec for an unknown or invalid reader locale.
#[must_use]
pub fn locale_unknown_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "LOCALE003",
        help: Some(
            "check the locale id is a valid reader locale directory name under the standard library reader path",
        ),
    }
}

/// Return the stable diagnostic spec for an identifier whose visible
/// characters span two or more Unicode scripts. Non-fatal by ruling: the
/// advisory observes look-alike risk without rejecting the spelling.
#[must_use]
pub fn mixed_script_identifier_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "LOCALE004",
        help: Some(
            "check for look-alike characters from another alphabet; the name is accepted as written",
        ),
    }
}

/// Return the stable diagnostic spec for package-layer refusals (`PKG001`).
///
/// The code is a catch-all: each refusal names itself by `issue` arg, and the
/// reader packs hold one issue row per issue that renders its cause. The base
/// row exists to hold those issue rows; the renderer never shows it.
#[must_use]
pub fn package_diagnostic_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "PKG001",
        help: None,
    }
}

/// Return the stable diagnostic spec for "serialized by the backend" advice
/// (`INFO001`, [`Severity::Info`](crate::Severity::Info)): a kernel a target
/// runs on one lane, on one partition, as one sequential call, or not at all.
/// Each case names itself by `issue` arg; `kernel` and `reason` are data.
/// Advice is never promoted to an error.
#[must_use]
pub fn serialized_by_backend_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "INFO001",
        help: None,
    }
}
