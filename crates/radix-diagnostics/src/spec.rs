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

/// Return the stable diagnostic spec for an unresolved keyword used in a
/// grammar position: a spelling the reader pack does not declare as a keyword
/// but that appears where a keyword is required.
#[must_use]
pub fn unresolved_keyword_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "UNRES001",
        help: Some("use a keyword spelling declared in the reader pack"),
    }
}

/// Return the stable diagnostic spec for an unresolved intrinsic called on a
/// receiver family: a spelling the reader pack does not declare for the
/// receiver's intrinsic surface.
#[must_use]
pub fn unresolved_intrinsic_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "UNRES002",
        help: Some("use an intrinsic spelling declared in the reader pack for the receiver family"),
    }
}

/// Return the stable diagnostic spec for an unresolved library member for an
/// identity: a spelling the reader pack does not declare for the library
/// member surface.
#[must_use]
pub fn unresolved_library_member_spec() -> DiagnosticSpec {
    DiagnosticSpec {
        code: "UNRES003",
        help: Some("use a member spelling declared in the reader pack for the library identity"),
    }
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
