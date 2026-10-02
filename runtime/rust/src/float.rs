//! Float helpers for generated Rust.
//!
//! The Rust target has no std half-float carrier, so `fractus<f16>` exists
//! only as a conversion-chain intermediate decoded to `f32`, the same
//! width-agnostic float-carrier model the MIR runner uses. `numerus<u16> ↦ f16
//! via Bits` therefore lowers through [`f16_bits_to_f32`] instead of a
//! nonexistent std `f16::from_bits`.

/// The `f32` an IEEE half-float bit pattern denotes: the standard
/// sign/exponent/mantissa decode, mirroring the MIR runner's `half_to_f32`.
/// Subnormals scale by 2^-24; the infinity/NaN row honors the sign bit so
/// `Bits` round trips keep ±Inf/NaN magnitudes.
// WHY: the casts are the semantic ops of the decode (a 10-bit mantissa and a 5-bit
// exponent widen into `f32` exactly), mirrored from the compiler's former prelude.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
#[must_use]
pub fn f16_bits_to_f32(bits: u16) -> f32 {
    let sign = u32::from((bits >> 15) & 1);
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    if exponent == 0 {
        if mantissa == 0 {
            return if sign == 1 { -0.0 } else { 0.0 };
        }
        return (mantissa as f32) * 2f32.powi(-24) * if sign == 1 { -1.0 } else { 1.0 };
    }
    if exponent == 0x1f {
        let magnitude = if mantissa == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        };
        return if sign == 1 { -magnitude } else { magnitude };
    }
    let value = (1.0f32 + mantissa as f32 / 1024.0f32) * 2f32.powi(exponent as i32 - 15);
    if sign == 1 { -value } else { value }
}

#[cfg(test)]
#[path = "float_test.rs"]
mod tests;
