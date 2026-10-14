//! Build-time file embeds: `const T X = embed "path"` / la `fixum T X = insere
//! "path"` (DECISIONS D8.10).
//!
//! Reading a neighbouring file at build time is a compile-time intrinsic, not
//! I/O. The driver owns the file system: before semantic analysis it resolves
//! every authored `insere` path package-relative and records the outcome here,
//! keyed by the authored path spelling. Typecheck types each embed from its
//! slot, then the `insere_fold` pass folds it to a literal of that type:
//! `textus` requires valid UTF-8, `octeti` takes the raw bytes. Diagnostics
//! stay in the semantic catalog (SEM061) so every rejection has one stable
//! identity.

use std::collections::BTreeMap;

/// Why the driver could not supply an embedded file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsereRejection {
    /// The path is absolute; embeds are package-relative only.
    AbsolutePath,
    /// The path leaves the package root (through `..` or a symlink).
    EscapesPackage,
    /// No file exists at the resolved path.
    Missing,
    /// The file exists but could not be read (a directory, permissions, …).
    Unreadable,
    /// The source has no file location (stdin, in-memory analysis), so there
    /// is no package root to resolve against.
    NoSourceRoot,
}

impl InsereRejection {
    /// Stable diagnostic issue identity for this rejection.
    #[must_use]
    pub const fn issue(self) -> &'static str {
        match self {
            Self::AbsolutePath => "insere_path_absolute",
            Self::EscapesPackage => "insere_path_escapes_package",
            Self::Missing => "insere_file_missing",
            Self::Unreadable => "insere_file_unreadable",
            Self::NoSourceRoot => "insere_no_source_root",
        }
    }
}

/// The driver's answer for one authored embed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsereSource {
    Bytes(Vec<u8>),
    Rejected(InsereRejection),
}

/// Embedded files for one analysis unit, keyed by authored path spelling.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InsereFiles {
    entries: BTreeMap<String, InsereSource>,
}

impl InsereFiles {
    /// An empty table (no driver file context).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Record the resolution for one authored path.
    pub fn insert(&mut self, path: impl Into<String>, source: InsereSource) {
        self.entries.insert(path.into(), source);
    }

    /// The resolution for an authored path. An unresolved path means the
    /// analysis ran without a driver file context.
    #[must_use]
    pub fn get(&self, path: &str) -> &InsereSource {
        const UNRESOLVED: InsereSource = InsereSource::Rejected(InsereRejection::NoSourceRoot);
        self.entries.get(path).unwrap_or(&UNRESOLVED)
    }
}

/// Render embedded UTF-8 text as a `textus` literal payload.
///
/// The interner keeps a text literal's source escapes verbatim and every
/// backend decodes them (see `radix_codegen_shared::unescape_faber_text`), so
/// embedded text is escaped into that same form rather than stored raw.
#[must_use]
pub fn textus_literal_payload(text: &str) -> String {
    let mut payload = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => payload.push_str("\\\\"),
            '"' => payload.push_str("\\\""),
            '\n' => payload.push_str("\\n"),
            '\r' => payload.push_str("\\r"),
            '\t' => payload.push_str("\\t"),
            '\0' => payload.push_str("\\u{0}"),
            other => payload.push(other),
        }
    }
    payload
}

/// Render embedded bytes as an `octeti` literal payload (contiguous hex).
#[must_use]
pub fn octeti_literal_payload(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut payload = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        payload.push(char::from(HEX[usize::from(byte >> 4)]));
        payload.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    payload
}
