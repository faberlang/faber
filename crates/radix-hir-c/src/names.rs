//! C identifier policy: ASCII-only, case-sensitive uniqueness.
//!
//! User bindings arrive already prefixed by the shared policy
//! ([`radix_codegen_shared::names::prefixed_binding_name`]: `l_` for locals,
//! `p_` for parameters), as on the Rust, Go, TypeScript, Swift, Python and
//! Haskell targets. The prefix is what keeps a Faber name out of C's reserved
//! space: no C keyword, no reserved `_X` / `__x` identifier, and no object-like
//! macro of `<stdio.h>`, `<stdint.h>` or `<inttypes.h>` starts with `l_` or
//! `p_`, so a Faber local named `char`, `stdout` or `INT64_MAX` is `l_char`,
//! `l_stdout`, `l_INT64_MAX` and needs no keyword table.
//!
//! EDGE: POSIX reserves the `_t` suffix for type names its headers may add
//! (`l_size_t`). A block-scope variable cannot be broken by a later typedef of
//! that name, and no header defines an `l_`/`p_` type, so it is accepted.

use handroll::FxHashSet;
use radix_codegen_shared::sanitize_ident;

/// Claims unique identifiers within one C scope. C identifiers are
/// case-sensitive, so uniqueness is exact.
#[derive(Clone, Default)]
pub(crate) struct Namer {
    taken: FxHashSet<String>,
}

impl Namer {
    /// The scope of `main`.
    pub(crate) fn program_scope() -> Self {
        Self::default()
    }

    /// Claim a sanitized, unique spelling of a prefixed binding name. Two
    /// Faber names can sanitize to one C name (`l_café`, `l_cafè`), so a
    /// repeat takes a numeric suffix.
    pub(crate) fn claim(&mut self, prefixed: &str) -> String {
        let base = sanitize_ident(prefixed, "l_");
        let mut candidate = base.clone();
        let mut suffix = 1_u32;
        while self.taken.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        self.taken.insert(candidate.clone());
        candidate
    }
}
