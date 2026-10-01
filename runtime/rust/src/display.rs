//! Shared scalar display helpers for generated Rust and interpreter paths.
//!
//! ## Contract authority
//!
//! [`FractusDisplay`], [`display_fractus`], [`display_bivalens`] and
//! [`display_text_payload`] mirror the definitions in
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

pub fn display_fractus<T: FractusDisplay>(value: T) -> String {
    if value.has_zero_fraction() {
        value.display_whole()
    } else {
        value.to_string()
    }
}

#[must_use]
pub fn display_bivalens(value: bool) -> &'static str {
    if value { "verum" } else { "falsum" }
}

#[must_use]
pub fn display_text_payload(value: &str) -> &str {
    value
}

pub fn display_valor(value: &Valor) -> String {
    match value {
        Valor::Nihil => "nihil".to_owned(),
        Valor::Bivalens(value) => display_bivalens(*value).to_owned(),
        Valor::Numerus(value) => value.to_string(),
        Valor::Magnus(value) => value.to_string(),
        Valor::Fractus(value) => display_fractus(*value),
        Valor::Textus(value) | Valor::Instans(value) => value.clone(),
        Valor::Octeti(bytes) => format!("<{} bytes>", bytes.len()),
        Valor::Lista(items) => {
            let inner = items
                .iter()
                .map(display_valor)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        Valor::Tabula(items) => {
            let inner = items
                .iter()
                .map(|(key, value)| format!("{key:?}: {}", display_valor(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

pub fn display_option<T: Display>(value: Option<&T>) -> String {
    value.map_or_else(|| "nihil".to_owned(), ToString::to_string)
}

pub fn display_option_bivalens(value: Option<bool>) -> &'static str {
    value.map_or("nihil", display_bivalens)
}

pub fn display_option_fractus<T: FractusDisplay>(value: Option<T>) -> String {
    value.map_or_else(|| "nihil".to_owned(), display_fractus)
}

pub fn display_option_vacuum<T>(value: Option<T>) -> &'static str {
    value.map_or("nihil", |_| "vacuum")
}
