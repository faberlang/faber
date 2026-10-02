//! The `¶` format-operator renderers for generated Rust.
//!
//! `value ¶ "spec"` lowers to one call into this module with the spec decomposed
//! into a [`Fmt`] literal. Rust's `format!` cannot express the Faber rules
//! directly: integer precision pads (`42.00`), radix kinds print sign plus
//! magnitude (`-2a`), non-finite floats print `NaN`/`∞`/`-∞`, and the default
//! float body is the Faber `fractus` display. These helpers reproduce the
//! reference renderer in `radix_types::format_spec`; the corpus golden pins the
//! two together.
//!
//! Generated code calls these as free functions, for example
//! `faber::format::int(i128::from(n), faber::Fmt { .. })`.

// WHY: the bodies are the verbatim `¶` helpers the compiler used to emit into
// every generated file; the `u128 -> f64` cast is the scientific rendering of a
// magnitude on purpose.
#![allow(clippy::cast_precision_loss)]

use crate::{Magnus, Radix};

/// A decomposed `¶` layout spec: `fill` and `align` (`<`, `^` or `>`), the
/// `plus` and `zero` flags, the minimum `width` (0 for none), the optional
/// `precision`, and the `kind` (`x`, `b`, `o`, `e`, or a space for the default).
#[derive(Clone, Copy)]
pub struct Fmt {
    pub fill: char,
    pub align: char,
    pub plus: bool,
    pub zero: bool,
    pub width: usize,
    pub precision: Option<usize>,
    pub kind: char,
}

/// Lay `sign` and `body` out to the spec's width: zero padding goes between the
/// sign and the body, fill padding follows the alignment.
#[must_use]
pub fn pad(sign: &str, body: &str, spec: Fmt, zero: bool) -> String {
    let len = sign.chars().count() + body.chars().count();
    if len >= spec.width {
        return format!("{sign}{body}");
    }
    let pad = spec.width - len;
    if zero {
        return format!("{sign}{}{body}", "0".repeat(pad));
    }
    let fill = |n: usize| spec.fill.to_string().repeat(n);
    match spec.align {
        '<' => format!("{sign}{body}{}", fill(pad)),
        '^' => format!("{}{sign}{body}{}", fill(pad / 2), fill(pad - pad / 2)),
        _ => format!("{}{sign}{body}", fill(pad)),
    }
}

/// The scientific body of a magnitude (`1.5e3`), with the spec's precision when
/// it has one.
#[must_use]
pub fn sci(magnitude: f64, precision: Option<usize>) -> String {
    match precision {
        Some(p) => format!("{magnitude:.p$e}"),
        None => format!("{magnitude:e}"),
    }
}

/// An integer through the spec: radix kinds and scientific print sign plus
/// magnitude, an integer precision pads with zero decimals (`42.00`).
#[must_use]
pub fn int(value: i128, spec: Fmt) -> String {
    let magnitude = value.unsigned_abs();
    let body = match spec.kind {
        'x' => format!("{magnitude:x}"),
        'b' => format!("{magnitude:b}"),
        'o' => format!("{magnitude:o}"),
        'e' => sci(magnitude as f64, spec.precision),
        _ => match spec.precision {
            Some(p) if p > 0 => format!("{magnitude}.{}", "0".repeat(p)),
            _ => magnitude.to_string(),
        },
    };
    let sign = if value < 0 {
        "-"
    } else if spec.plus {
        "+"
    } else {
        ""
    };
    pad(sign, &body, spec, spec.zero)
}

/// A float through the spec; `display` is the Faber `fractus` display of the
/// magnitude, the body when the spec has no precision and no scientific kind.
/// `NaN` and the infinities never zero-pad.
#[must_use]
pub fn float(value: f64, display: &str, spec: Fmt) -> String {
    if value.is_nan() {
        return pad("", "NaN", spec, false);
    }
    let sign = if value.is_sign_negative() {
        "-"
    } else if spec.plus {
        "+"
    } else {
        ""
    };
    let magnitude = value.abs();
    if magnitude.is_infinite() {
        return pad(sign, "∞", spec, false);
    }
    let body = match (spec.kind, spec.precision) {
        ('e', precision) => sci(magnitude, precision),
        (_, Some(p)) => format!("{magnitude:.p$}"),
        (_, None) => display.to_owned(),
    };
    pad(sign, &body, spec, spec.zero)
}

/// Text through the spec: a precision truncates to that many characters.
#[must_use]
pub fn text(value: &str, spec: Fmt) -> String {
    let body: String = match spec.precision {
        Some(n) => value.chars().take(n).collect(),
        None => value.to_owned(),
    };
    pad("", &body, spec, false)
}

/// An instant's RFC 3339 text through a preset: `d` the date, `t` the time to
/// whole seconds, anything else the full ISO text.
#[must_use]
pub fn instans(iso: String, preset: char) -> String {
    match preset {
        'd' => iso.split('T').next().unwrap_or_default().to_owned(),
        't' => iso
            .split('T')
            .nth(1)
            .unwrap_or_default()
            .chars()
            .take(8)
            .collect(),
        _ => iso,
    }
}

/// The scientific body of an unbounded integer's magnitude: an exact digit
/// rendering when it is beyond `f64`, otherwise the same body as [`sci`].
#[must_use]
pub fn magnus_sci(magnitude: &Magnus, precision: Option<usize>) -> String {
    let float = magnitude.to_f64();
    if float.is_finite() {
        return sci(float, precision);
    }
    let digits = magnitude.to_string_radix(Radix::Dec);
    let exponent = digits.len() - 1;
    let coefficient = if let Some(p) = precision {
        let take = (p + 1).min(digits.len());
        let mut shown = digits[..take].to_owned();
        shown.push_str(&"0".repeat(p + 1 - take));
        if p > 0 {
            shown.insert(1, '.');
        }
        shown
    } else {
        let trimmed = digits.trim_end_matches('0');
        let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
        if trimmed.len() > 1 {
            format!("{}.{}", &trimmed[..1], &trimmed[1..])
        } else {
            trimmed.to_owned()
        }
    };
    format!("{coefficient}e{exponent}")
}

/// An unbounded integer (`inf`) through the spec: the same layout rules over
/// [`Magnus`] (`radix_types::format_spec::render_magnus` is the reference).
#[must_use]
pub fn magnus(value: &Magnus, spec: Fmt) -> String {
    let magnitude = value.abs();
    let body = match spec.kind {
        'x' => magnitude.to_string_radix(Radix::Hex),
        'b' => magnitude.to_string_radix(Radix::Bin),
        'o' => magnitude.to_string_radix(Radix::Oct),
        'e' => magnus_sci(&magnitude, spec.precision),
        _ => {
            let digits = magnitude.to_string_radix(Radix::Dec);
            match spec.precision {
                Some(p) if p > 0 => format!("{digits}.{}", "0".repeat(p)),
                _ => digits,
            }
        }
    };
    let sign = if value.is_negative() {
        "-"
    } else if spec.plus {
        "+"
    } else {
        ""
    };
    pad(sign, &body, spec, spec.zero)
}

#[cfg(test)]
#[path = "format_test.rs"]
mod tests;
