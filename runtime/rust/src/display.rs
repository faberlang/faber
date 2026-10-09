//! Shared scalar display helpers for generated Rust and interpreter paths.
//!
//! ## Contract authority
//!
//! [`FractusDisplay`], [`fractus`], [`bivalens`] and
//! [`text_payload`] mirror the definitions in
//! `radix-runtime-contract/src/display.rs`.  The contract is the canonical
//! authority; the runtime copy must stay in sync.
//!
//! Unlike `tensor`/`sparsa`, items cannot be `pub use`-re-exported from the
//! contract because `FractusDisplay` has orphan-rule-bound `impl`s for `f32`
//! and `f64` that prevent importing the trait from another crate.

use crate::Valor;
use std::fmt::Display;

pub trait FractusDisplay: Copy + Display {
    fn has_zero_fraction(self) -> bool;

    /// The text of a value with a zero fraction. An `f64` prints its exact
    /// decimal expansion with one fraction digit; an `f32` prints its shortest
    /// round-trip digits plus `.0`, never the exact binary expansion (the MIR
    /// runner's `display_float_at_width` is the oracle for both).
    fn display_whole(self) -> String;
}

impl FractusDisplay for f32 {
    fn has_zero_fraction(self) -> bool {
        self.fract() == 0.0
    }

    fn display_whole(self) -> String {
        format!("{self}.0")
    }
}

impl FractusDisplay for f64 {
    fn has_zero_fraction(self) -> bool {
        self.fract() == 0.0
    }

    fn display_whole(self) -> String {
        format!("{self:.1}")
    }
}

pub fn fractus<T: FractusDisplay>(value: T) -> String {
    if value.has_zero_fraction() {
        value.display_whole()
    } else {
        value.to_string()
    }
}

/// The output words of one locale: what a program prints for a `bivalens`, the
/// null value and a tuple head. The print locale is the code locale, so
/// English code prints `true`/`false`/`none`/`tuple` and Latin code prints
/// `verum`/`falsum`/`nihil`/`iuncta`. Every plain function below prints
/// [`DisplayTokens::LATIN`]; the `*_with` variants take the module's tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayTokens {
    pub true_: &'static str,
    pub false_: &'static str,
    pub none: &'static str,
    pub tuple: &'static str,
}

impl DisplayTokens {
    /// The Latin tokens (`verum`, `falsum`, `nihil`, `iuncta`).
    pub const LATIN: Self = Self {
        true_: "verum",
        false_: "falsum",
        none: "nihil",
        tuple: "iuncta",
    };
}

#[must_use]
pub fn bivalens(value: bool) -> &'static str {
    bivalens_with(value, &DisplayTokens::LATIN)
}

#[must_use]
pub fn bivalens_with(value: bool, tokens: &DisplayTokens) -> &'static str {
    if value { tokens.true_ } else { tokens.false_ }
}

#[must_use]
pub fn text_payload(value: &str) -> &str {
    value
}

pub fn valor(value: &Valor) -> String {
    valor_with(value, &DisplayTokens::LATIN)
}

pub fn valor_with(value: &Valor, tokens: &DisplayTokens) -> String {
    match value {
        Valor::Nihil => tokens.none.to_owned(),
        Valor::Bivalens(value) => bivalens_with(*value, tokens).to_owned(),
        Valor::Numerus(value) => value.to_string(),
        Valor::Magnus(value) => value.to_string(),
        Valor::Fractus(value) => fractus(*value),
        Valor::Textus(value) | Valor::Instans(value) => value.clone(),
        Valor::Octeti(bytes) => format!("<{} bytes>", bytes.len()),
        Valor::Lista(items) => {
            let inner = items
                .iter()
                .map(|item| valor_with(item, tokens))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        Valor::Tabula(items) => {
            let inner = items
                .iter()
                .map(|(key, value)| format!("{key:?}: {}", valor_with(value, tokens)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

pub fn option<T: Display>(value: Option<&T>) -> String {
    option_with(value, &DisplayTokens::LATIN)
}

pub fn option_with<T: Display>(value: Option<&T>, tokens: &DisplayTokens) -> String {
    value.map_or_else(|| tokens.none.to_owned(), ToString::to_string)
}

pub fn option_bivalens(value: Option<bool>) -> &'static str {
    option_bivalens_with(value, &DisplayTokens::LATIN)
}

pub fn option_bivalens_with(value: Option<bool>, tokens: &DisplayTokens) -> &'static str {
    value.map_or(tokens.none, |value| bivalens_with(value, tokens))
}

pub fn option_fractus<T: FractusDisplay>(value: Option<T>) -> String {
    option_fractus_with(value, &DisplayTokens::LATIN)
}

pub fn option_fractus_with<T: FractusDisplay>(value: Option<T>, tokens: &DisplayTokens) -> String {
    value.map_or_else(|| tokens.none.to_owned(), fractus)
}

pub fn option_vacuum<T>(value: Option<T>) -> &'static str {
    option_vacuum_with(value, &DisplayTokens::LATIN)
}

pub fn option_vacuum_with<T>(value: Option<T>, tokens: &DisplayTokens) -> &'static str {
    value.map_or(tokens.none, |_| "vacuum")
}
