//! Radix-owned scalar display helpers shared by the interpreter (and, as text,
//! by generated Rust).
//!
//! Mirrors the subset of `faber::display` used at Radix build time.
//! Radix is the authority; the runtime copy is expected to become a
//! generated/committed artifact in a later phase.

use std::fmt::Display;

/// Float display contact: values with a zero fractional part render with one
/// trailing digit (`5.0`), others render via their native `to_string`.
pub trait FractusDisplay: Copy + Display {
    /// Whether the fractional part is exactly zero.
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

/// Render a `fractus` with the Faber display contract.
pub fn display_fractus<T: FractusDisplay>(value: T) -> String {
    if value.has_zero_fraction() {
        value.display_whole()
    } else {
        value.to_string()
    }
}

/// Render a `bivalens` as Faber boolean text (`verum` / `falsum`).
#[must_use]
pub fn display_bivalens(value: bool) -> &'static str {
    if value { "verum" } else { "falsum" }
}

/// Pass through a `textus` payload (hook point kept for parity with the runtime
/// display contract).
#[must_use]
pub fn display_text_payload(value: &str) -> &str {
    value
}

#[cfg(test)]
#[path = "display_test.rs"]
mod tests;
