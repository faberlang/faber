//! C identifier policy: ASCII-only, case-sensitive uniqueness, and no name the
//! implementation or the included headers reserve.

use radix_codegen_shared::sanitize_ident;
use rustc_hash::FxHashSet;

/// C23 keywords, including the alternate spellings C23 made keywords
/// (`bool`, `true`, `false`, `alignas`, ...). The underscore-capital forms are
/// also caught by the reserved-identifier rule; they are listed for clarity.
const KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "auto",
    "bool",
    "break",
    "case",
    "char",
    "const",
    "constexpr",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "false",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "nullptr",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_assert",
    "struct",
    "switch",
    "thread_local",
    "true",
    "typedef",
    "typeof",
    "typeof_unqual",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_BitInt",
    "_Bool",
    "_Complex",
    "_Decimal128",
    "_Decimal32",
    "_Decimal64",
    "_Generic",
    "_Imaginary",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
];

/// Names the emitted program uses or the included headers define as objects
/// or object-like macros. A local named `stdout` or `EOF` would be rewritten
/// by the preprocessor or shadow a name the program calls. Typedefs ending in
/// `_t` and the `<stdint.h>` / `<inttypes.h>` macro families are matched by
/// pattern in [`unreserve`].
const LIBRARY: &[&str] = &[
    "main",
    "printf",
    "NULL",
    "EOF",
    "BUFSIZ",
    "FILE",
    "FILENAME_MAX",
    "FOPEN_MAX",
    "L_ctermid",
    "L_tmpnam",
    "P_tmpdir",
    "SEEK_CUR",
    "SEEK_END",
    "SEEK_SET",
    "TMP_MAX",
    "stdin",
    "stdout",
    "stderr",
    "errno",
    "assert",
];

/// The prefix the emitter reserves for its own names.
const OWN_PREFIX: &str = "faber_";

/// C reserves `_` + capital and `__` in every scope.
fn reserved_underscore(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.first() == Some(&b'_')
        && bytes
            .get(1)
            .is_some_and(|b| *b == b'_' || b.is_ascii_uppercase())
}

/// `<stdint.h>` limit and constant macros (`INT64_MAX`, `UINT8_C`, ...) and
/// `<inttypes.h>` format macros (`PRId64`, `SCNx32`, ...).
fn reserved_macro_family(name: &str) -> bool {
    let upper = name
        .bytes()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    if upper && (name.starts_with("INT") || name.starts_with("UINT")) {
        return true;
    }
    (name.starts_with("PRI") || name.starts_with("SCN"))
        && name.as_bytes().get(3).is_some_and(u8::is_ascii_lowercase)
}

/// Rewrite a name that a reservation *pattern* matches into one no pattern
/// matches. A numeric suffix cannot do this (`INT64_MAX_1` is still a macro
/// spelling, `__x_1` still starts with `__`), so the rewrite is structural.
fn unreserve(name: String) -> String {
    let mut name = name;
    if reserved_underscore(&name) || reserved_macro_family(&name) {
        name = format!("v_{name}");
    }
    // POSIX reserves the `_t` suffix for types (`int64_t`, `size_t`, ...).
    if name.ends_with("_t") {
        name.push_str("_v");
    }
    name
}

/// True when a claim of `name` must take a numeric suffix: a keyword, or a
/// name the program or its headers use.
fn listed(name: &str) -> bool {
    KEYWORDS.contains(&name) || LIBRARY.contains(&name)
}

/// Claims unique identifiers within one C scope. C identifiers are
/// case-sensitive, so uniqueness is exact.
#[derive(Clone, Default)]
pub(crate) struct Namer {
    taken: FxHashSet<String>,
}

impl Namer {
    /// The scope of `main`: reserved names are checked on every claim.
    pub(crate) fn program_scope() -> Self {
        Self::default()
    }

    /// Claim a sanitized, unique spelling of `raw`. A name that starts with the
    /// emitter's own prefix is renamed so it cannot collide with a helper; a
    /// pattern-reserved name is rewritten; a listed or taken name takes a
    /// numeric suffix (which no reservation pattern matches).
    pub(crate) fn claim(&mut self, raw: &str) -> String {
        let mut base = unreserve(sanitize_ident(raw, "v"));
        if base.starts_with(OWN_PREFIX) {
            base = format!("v_{base}");
        }
        let mut candidate = base.clone();
        let mut suffix = 1_u32;
        while listed(&candidate) || self.taken.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        self.taken.insert(candidate.clone());
        candidate
    }
}
