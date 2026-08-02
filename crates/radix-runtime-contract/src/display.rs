//! Radix-owned scalar display helpers shared by the interpreter (and, as text,
//! by generated Rust).
//!
//! Mirrors the subset of `faber-runtime::display` used at Radix build time.
//! Radix is the authority; the runtime copy is expected to become a
//! generated/committed artifact in a later phase.

use std::fmt::Display;

/// Float display contact: values with a zero fractional part render with one
/// trailing digit (`5.0`), others render via their native `to_string`.
pub trait FractusDisplay: Copy + Display {
    /// Whether the fractional part is exactly zero.
    fn has_zero_fraction(self) -> bool;
}

impl FractusDisplay for f32 {
    fn has_zero_fraction(self) -> bool {
        self.fract() == 0.0
    }
}

impl FractusDisplay for f64 {
    fn has_zero_fraction(self) -> bool {
        self.fract() == 0.0
    }
}

/// Render a `fractus` with the Faber display contract.
pub fn display_fractus<T: FractusDisplay>(value: T) -> String {
    if value.has_zero_fraction() {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

/// Render a `bivalens` as Faber boolean text (`verum` / `falsum`).
#[must_use]
pub fn display_bivalens(value: bool) -> &'static str {
    if value {
        "verum"
    } else {
        "falsum"
    }
}

/// Pass through a `textus` payload (hook point kept for parity with the runtime
/// display contract).
#[must_use]
pub fn display_text_payload(value: &str) -> &str {
    value
}
