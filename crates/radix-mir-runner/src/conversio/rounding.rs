//! The rounding clause of a float or decimal `↦` integer conversion
//! (need, ruling items 4 to 7): `x ↦ T via Floor|Ceil|HalfEven|Trunc`.
//!
//! The clause reaches MIR as an identifier symbol in `MirConversion.params`.
//! The mode picks how the fraction goes; the range and NaN check that makes the
//! conversion failable runs after it, on the rounded integer.

use super::{Runner, Symbol};
use crate::value::round_half_even_quotient;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum RoundingMode {
    /// Toward negative infinity.
    Floor,
    /// Toward positive infinity.
    Ceil,
    /// Nearest, ties to the even neighbour.
    HalfEven,
    /// Toward zero.
    Trunc,
}

impl RoundingMode {
    fn from_hint(name: &str) -> Option<Self> {
        match name {
            "Floor" => Some(Self::Floor),
            "Ceil" => Some(Self::Ceil),
            "HalfEven" => Some(Self::HalfEven),
            "Trunc" => Some(Self::Trunc),
            _ => None,
        }
    }

    /// Round a float to an integral float. NaN and infinities pass through
    /// unchanged, so the range check after it still rejects them.
    pub(crate) fn round_float(self, value: f64) -> f64 {
        match self {
            Self::Floor => value.floor(),
            Self::Ceil => value.ceil(),
            Self::HalfEven => value.round_ties_even(),
            Self::Trunc => value.trunc(),
        }
    }

    /// Round the exact quotient `numerator / denominator` to an integer.
    /// `None` for a zero denominator or an overflowing intermediate.
    pub(crate) fn round_quotient(self, numerator: i128, denominator: i128) -> Option<i128> {
        if denominator == 0 {
            return None;
        }
        match self {
            Self::HalfEven => round_half_even_quotient(numerator, denominator),
            Self::Trunc => numerator.checked_div(denominator),
            Self::Floor | Self::Ceil => {
                let quotient = numerator.checked_div(denominator)?;
                if numerator.checked_rem(denominator)? == 0 {
                    return Some(quotient);
                }
                // The truncated quotient sits toward zero; the exact value is
                // above it when the signs agree and below it when they differ.
                let exact_is_positive = (numerator < 0) == (denominator < 0);
                match (self, exact_is_positive) {
                    (Self::Ceil, true) => quotient.checked_add(1),
                    (Self::Floor, false) => quotient.checked_sub(1),
                    _ => Some(quotient),
                }
            }
        }
    }
}

impl<H: super::super::Host + ?Sized> Runner<'_, H> {
    /// The rounding named in a conversion's `via` clause, if any.
    pub(super) fn rounding_hint(&self, params: &[Symbol]) -> Option<RoundingMode> {
        params
            .iter()
            .find_map(|param| RoundingMode::from_hint(self.interner.resolve(*param)))
    }
}
