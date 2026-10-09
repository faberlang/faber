//! Per-locale program-output tokens: the words a program prints for a
//! `bivalens`, the null value, and a tuple head.
//!
//! The print locale is the code locale, so English code prints
//! `true`/`false`/`none`/`tuple` and Latin code prints
//! `verum`/`falsum`/`nihil`/`iuncta`. This crate has no locale-pack
//! dependency; `radix-module` builds a value from a pack row set.

/// The four output tokens of one locale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayTokens {
    /// Printed for a true `bivalens`.
    pub true_: String,
    /// Printed for a false `bivalens`.
    pub false_: String,
    /// Printed for the null value.
    pub none: String,
    /// Printed as the head of a tuple.
    pub tuple: String,
}

impl DisplayTokens {
    /// The Latin tokens (`verum`, `falsum`, `nihil`, `iuncta`).
    #[must_use]
    pub fn latin() -> Self {
        Self {
            true_: "verum".to_owned(),
            false_: "falsum".to_owned(),
            none: "nihil".to_owned(),
            tuple: "iuncta".to_owned(),
        }
    }
}

#[cfg(test)]
#[path = "display_tokens_test.rs"]
mod tests;
