//! Runtime contract types — re-exported from leaf `radix-runtime-contract`
//! (display, frame, instans, sparsa, tensor).

pub use radix_runtime_contract::{display, display_tokens, frame, instans, sparsa, tensor, trap};

use crate::locale::LocalePack;
use display_tokens::DisplayTokens;

/// Read the program-output tokens of `pack`.
///
/// `true`/`false`/`tuple` come from the `[keywords]` rows `verum`/`falsum`/
/// `iuncta`; `none` comes from the `[types]` row `nihil` (never the
/// `[keywords]` `nihil` value-literal alias). A missing row falls back to the
/// Latin word.
#[must_use]
pub fn display_tokens_from_pack(pack: &LocalePack) -> DisplayTokens {
    let latin = DisplayTokens::latin();
    let keyword = |key: &str, fallback: String| {
        pack.keywords
            .get(key)
            .map_or(fallback, |row| row.canonical.clone())
    };
    DisplayTokens {
        true_: keyword("verum", latin.true_),
        false_: keyword("falsum", latin.false_),
        none: pack
            .types
            .get("nihil")
            .map_or(latin.none, |row| row.canonical.clone()),
        tuple: keyword("iuncta", latin.tuple),
    }
}

#[cfg(test)]
#[path = "display_tokens_test.rs"]
mod display_tokens_tests;

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
