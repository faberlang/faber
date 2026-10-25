//! The `¶` format-spec parser and renderers for the CLIF runtime.
//!
//! A verbatim copy of the parse and render halves of
//! `crates/radix-types/src/format_spec.rs` (the runtime staticlib cannot depend
//! on the compiler crates, as `regex_program.rs` is a copy for the same
//! reason). Validation against the subject type stays in the compiler; a spec
//! reaching this module has already passed it. The MIR runner executes the
//! reference renderer and the corpus golden pins the two together.

use super::magnus::{Magnus, Radix};

/// Why a spec was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatSpecError {
    /// The text does not follow the spec pattern.
    Malformed,
}

/// Alignment inside the minimum width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatAlign {
    /// `<`
    Left,
    /// `>`
    Right,
    /// `^` (odd padding goes to the right, like Rust and Python)
    Center,
}

/// Presentation kind for numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatKind {
    /// `x` lowercase hexadecimal.
    Hex,
    /// `b` binary.
    Binary,
    /// `o` octal.
    Octal,
    /// `e` scientific (`1.5e3`, Rust `LowerExp` shape).
    Scientific,
}

/// Named `instans` presets. No strftime-style patterns (D2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstansPreset {
    /// Full ISO 8601 / RFC 3339 UTC wire at the value's precision.
    Iso,
    /// `YYYY-MM-DD` (UTC).
    Date,
    /// `HH:MM:SS` (UTC).
    Time,
}

/// The layout form of a spec: every part is optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LayoutSpec {
    /// Fill character; only meaningful with an explicit align.
    pub(crate) fill: char,
    /// Explicit alignment (numbers default right, text left).
    pub(crate) align: Option<FormatAlign>,
    /// `+`: always show the sign.
    pub(crate) plus: bool,
    /// `0`: zero-pad after the sign.
    pub(crate) zero: bool,
    /// Minimum width in scalars (grows to fit, never truncates numbers).
    pub(crate) width: Option<usize>,
    /// `.N`: precision on numbers, maximum length on text.
    pub(crate) precision: Option<usize>,
    /// Number presentation kind.
    pub(crate) kind: Option<FormatKind>,
}

impl Default for LayoutSpec {
    fn default() -> Self {
        Self {
            fill: ' ',
            align: None,
            plus: false,
            zero: false,
            width: None,
            precision: None,
            kind: None,
        }
    }
}

/// A parsed `¶` spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatSpec {
    /// `[fill][align][sign][0][width][.precision][kind]`.
    Layout(LayoutSpec),
    /// `iso`, `date`, or `time` (instans only).
    Preset(InstansPreset),
}

fn align_of(c: char) -> Option<FormatAlign> {
    match c {
        '<' => Some(FormatAlign::Left),
        '>' => Some(FormatAlign::Right),
        '^' => Some(FormatAlign::Center),
        _ => None,
    }
}

fn digits(chars: &[char], at: &mut usize) -> Result<Option<usize>, FormatSpecError> {
    let start = *at;
    while chars.get(*at).is_some_and(char::is_ascii_digit) {
        *at += 1;
    }
    if *at == start {
        return Ok(None);
    }
    let text: String = chars[start..*at].iter().collect();
    text.parse::<u32>()
        .map(|n| Some(n as usize))
        .map_err(|_| FormatSpecError::Malformed)
}

/// Parse a spec's text.
///
/// # Errors
///
/// [`FormatSpecError::Malformed`] when the text does not follow the pattern.
pub(crate) fn parse_format_spec(text: &str) -> Result<FormatSpec, FormatSpecError> {
    match text {
        "iso" => return Ok(FormatSpec::Preset(InstansPreset::Iso)),
        "date" => return Ok(FormatSpec::Preset(InstansPreset::Date)),
        "time" => return Ok(FormatSpec::Preset(InstansPreset::Time)),
        _ => {}
    }
    let chars: Vec<char> = text.chars().collect();
    let mut spec = LayoutSpec::default();
    let mut at = 0;
    if let Some(align) = chars.get(1).copied().and_then(align_of) {
        spec.fill = chars[0];
        spec.align = Some(align);
        at = 2;
    } else if let Some(align) = chars.first().copied().and_then(align_of) {
        spec.align = Some(align);
        at = 1;
    }
    if chars.get(at) == Some(&'+') {
        spec.plus = true;
        at += 1;
    }
    if chars.get(at) == Some(&'0') {
        spec.zero = true;
        at += 1;
    }
    spec.width = digits(&chars, &mut at)?;
    if chars.get(at) == Some(&'.') {
        at += 1;
        spec.precision = Some(digits(&chars, &mut at)?.ok_or(FormatSpecError::Malformed)?);
    }
    spec.kind = match chars.get(at) {
        Some('x') => Some(FormatKind::Hex),
        Some('b') => Some(FormatKind::Binary),
        Some('o') => Some(FormatKind::Octal),
        Some('e') => Some(FormatKind::Scientific),
        _ => None,
    };
    if spec.kind.is_some() {
        at += 1;
    }
    if at != chars.len() {
        return Err(FormatSpecError::Malformed);
    }
    Ok(FormatSpec::Layout(spec))
}

/// Apply sign, zero-pad, fill and alignment to a rendered body.
///
/// `zero_ok` is false for non-finite floats, which pad with the fill instead.
fn finish(
    sign: &str,
    body: &str,
    spec: &LayoutSpec,
    default_align: FormatAlign,
    zero_ok: bool,
) -> String {
    let len = sign.chars().count() + body.chars().count();
    let width = spec.width.unwrap_or(0);
    if len >= width {
        return format!("{sign}{body}");
    }
    let pad = width - len;
    if spec.zero && zero_ok {
        return format!("{sign}{}{body}", "0".repeat(pad));
    }
    let fill = |n: usize| spec.fill.to_string().repeat(n);
    match spec.align.unwrap_or(default_align) {
        FormatAlign::Left => format!("{sign}{body}{}", fill(pad)),
        FormatAlign::Right => format!("{}{sign}{body}", fill(pad)),
        FormatAlign::Center => format!("{}{sign}{body}{}", fill(pad / 2), fill(pad - pad / 2)),
    }
}

fn sign_of(negative: bool, plus: bool) -> &'static str {
    if negative {
        "-"
    } else if plus {
        "+"
    } else {
        ""
    }
}

/// Render an integer (signed or unsigned values both fit `i128`).
///
/// Radix kinds print sign plus magnitude digits, never two's complement
/// (`-42 ¶ "x"` = `-2a`, D2.7). Precision pads with fractional zeros
/// (`42 ¶ ".2"` = `42.00`, D2.4). `e` renders through the float path.
pub(crate) fn render_integer(value: i128, spec: &LayoutSpec) -> String {
    let magnitude = value.unsigned_abs();
    let body = match spec.kind {
        Some(FormatKind::Hex) => format!("{magnitude:x}"),
        Some(FormatKind::Binary) => format!("{magnitude:b}"),
        Some(FormatKind::Octal) => format!("{magnitude:o}"),
        Some(FormatKind::Scientific) => {
            #[allow(clippy::cast_precision_loss)]
            let float = magnitude as f64;
            scientific(float, spec.precision)
        }
        None => match spec.precision {
            Some(p) if p > 0 => format!("{magnitude}.{}", "0".repeat(p)),
            _ => magnitude.to_string(),
        },
    };
    finish(
        sign_of(value < 0, spec.plus),
        &body,
        spec,
        FormatAlign::Right,
        true,
    )
}

/// Render an unbounded integer (`inf`, ruling 42): the same layout rules as
/// [`render_integer`] over a `Magnus` of any size. Radix kinds print sign plus
/// magnitude digits (`-42 ¶ "x"` = `-2a`), and `e` renders the exact digits
/// as a coefficient and exponent (a value the `f64` path can hold renders
/// exactly as [`render_integer`] would).
pub(crate) fn render_magnus(value: &Magnus, spec: &LayoutSpec) -> String {
    let magnitude = value.abs();
    let body = match spec.kind {
        Some(FormatKind::Hex) => magnitude.to_string_radix(Radix::Hex),
        Some(FormatKind::Binary) => magnitude.to_string_radix(Radix::Bin),
        Some(FormatKind::Octal) => magnitude.to_string_radix(Radix::Oct),
        Some(FormatKind::Scientific) => scientific_digits(&magnitude, spec.precision),
        None => {
            let digits = magnitude.to_string_radix(Radix::Dec);
            match spec.precision {
                Some(p) if p > 0 => format!("{digits}.{}", "0".repeat(p)),
                _ => digits,
            }
        }
    };
    finish(
        sign_of(value.is_negative(), spec.plus),
        &body,
        spec,
        FormatAlign::Right,
        true,
    )
}

/// `d.ddde<exp>` from the exact decimal digits of a non-negative integer.
fn scientific_digits(magnitude: &Magnus, precision: Option<usize>) -> String {
    let float = magnitude.to_f64();
    if float.is_finite() {
        return scientific(float, precision);
    }
    let digits = magnitude.to_string_radix(Radix::Dec);
    let exponent = digits.len() - 1;
    let coefficient = if let Some(p) = precision {
        // Truncate to `p` fractional digits (the value is far past f64, so
        // the digit string is the exact source of truth).
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

fn scientific(magnitude: f64, precision: Option<usize>) -> String {
    match precision {
        Some(p) => format!("{magnitude:.p$e}"),
        None => format!("{magnitude:e}"),
    }
}

/// Render a float. `display` renders a non-negative finite value with no
/// precision and no kind (the target's ordinary `fractus` display).
///
/// `NaN`, `∞`, `-∞` ignore precision and kind and never zero-pad (D2.7).
pub(crate) fn render_float(
    value: f64,
    spec: &LayoutSpec,
    display: impl FnOnce(f64) -> String,
) -> String {
    if value.is_nan() {
        return finish("", "NaN", spec, FormatAlign::Right, false);
    }
    let negative = value.is_sign_negative();
    let sign = sign_of(negative, spec.plus);
    let magnitude = value.abs();
    if magnitude.is_infinite() {
        return finish(sign, "∞", spec, FormatAlign::Right, false);
    }
    let body = match (spec.kind, spec.precision) {
        (Some(FormatKind::Scientific), precision) => scientific(magnitude, precision),
        (_, Some(p)) => format!("{magnitude:.p$}"),
        (_, None) => display(magnitude),
    };
    finish(sign, &body, spec, FormatAlign::Right, true)
}

/// Render an exact decimal `carrier × 10^-scale` (`d64`, F4 ruling).
///
/// A spec prints exactly what it says: `.N` rounds the exact digits half-even
/// to `N` fraction digits and pads with zeros when the value has fewer
/// (`12.5 ¶ ".2"` = `12.50`); with no precision the shortest form prints
/// (trailing fraction zeros dropped, `12.50` is `12.5`, `3.00` is `3`). Sign,
/// zero, fill, align and width behave as for floats. The sign is the value's
/// sign, so a negative value that rounds to zero prints `-0.0`, as a float
/// does. The kind is validated before rendering; scientific output is rendered
/// from the exact digits.
pub(crate) fn render_decimal(carrier: i128, scale: u32, spec: &LayoutSpec) -> String {
    let magnitude = carrier.unsigned_abs();
    let body = if spec.kind == Some(FormatKind::Scientific) {
        scientific_decimal(magnitude, scale, spec.precision)
    } else {
        let digits = scale as usize;
        let (whole, fraction) = match spec.precision {
            None => {
                let (whole, fraction) = split_fraction(magnitude, digits);
                (whole, fraction.trim_end_matches('0').to_owned())
            }
            Some(p) if p >= digits => {
                let (whole, mut fraction) = split_fraction(magnitude, digits);
                fraction.push_str(&"0".repeat(p - digits));
                (whole, fraction)
            }
            Some(p) => {
                let rounded = round_half_even_pow10(magnitude, digits - p);
                split_fraction(rounded, p)
            }
        };
        if fraction.is_empty() {
            whole
        } else {
            format!("{whole}.{fraction}")
        }
    };
    finish(
        sign_of(carrier < 0, spec.plus),
        &body,
        spec,
        FormatAlign::Right,
        true,
    )
}

/// The integer digits and the `digits` fraction digits of `magnitude × 10^-digits`.
fn split_fraction(magnitude: u128, digits: usize) -> (String, String) {
    let text = format!("{magnitude:0>width$}", width = digits + 1);
    let (whole, fraction) = text.split_at(text.len() - digits);
    (whole.to_owned(), fraction.to_owned())
}

/// `magnitude / 10^shift` rounded to the nearest integer, ties to even.
fn round_half_even_pow10(magnitude: u128, shift: usize) -> u128 {
    let Some(divisor) = u32::try_from(shift)
        .ok()
        .and_then(|shift| 10_u128.checked_pow(shift))
    else {
        // 10^shift exceeds every `u128`, so the quotient is zero and the
        // remainder is below half of the divisor.
        return 0;
    };
    let quotient = magnitude / divisor;
    let remainder = magnitude % divisor;
    let half = divisor / 2;
    if remainder > half || (remainder == half && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    }
}

/// Render a scaled decimal in scientific notation without converting to float.
fn scientific_decimal(magnitude: u128, mut scale: u32, precision: Option<usize>) -> String {
    if magnitude == 0 {
        return match precision {
            Some(p) if p > 0 => format!("0.{}e0", "0".repeat(p)),
            _ => "0e0".to_owned(),
        };
    }

    let mut normalized_magnitude = magnitude;
    let mut digits = magnitude.to_string();
    while scale > 0 && digits.ends_with('0') {
        digits.pop();
        normalized_magnitude /= 10;
        scale -= 1;
    }
    let mut exponent = digits.len() as i64 - 1 - i64::from(scale);

    if let Some(p) = precision {
        let significant_digits = p + 1;
        if digits.len() > significant_digits {
            let dropped = digits.len() - significant_digits;
            normalized_magnitude = round_half_even_pow10(normalized_magnitude, dropped);
            digits = normalized_magnitude.to_string();
            while digits.len() > significant_digits && digits.ends_with('0') {
                digits.pop();
                exponent += 1;
            }
        } else if digits.len() < significant_digits {
            digits.push_str(&"0".repeat(significant_digits - digits.len()));
        }

        let mut body = digits[..1].to_owned();
        if p > 0 {
            body.push('.');
            body.push_str(&digits[1..]);
        }
        format!("{body}e{exponent}")
    } else {
        let coefficient = digits.trim_end_matches('0');
        if coefficient.len() > 1 {
            format!("{}.{}e{exponent}", &coefficient[..1], &coefficient[1..])
        } else {
            format!("{coefficient}e{exponent}")
        }
    }
}

/// Render text: `.N` truncates to N scalars; width counts scalars (D2.7).
pub(crate) fn render_text(value: &str, spec: &LayoutSpec) -> String {
    let body: String = match spec.precision {
        Some(n) => value.chars().take(n).collect(),
        None => value.to_owned(),
    };
    finish("", &body, spec, FormatAlign::Left, false)
}

/// Derive a preset's text from the value's full ISO wire
/// (`[±]YYYY…-MM-DDTHH:MM:SS[.fff]Z`).
///
/// `date` is everything before `T`; `time` is the eight characters after it.
pub(crate) fn render_instans_preset(iso: &str, preset: InstansPreset) -> String {
    let (date, time) = iso.split_once('T').unwrap_or((iso, ""));
    match preset {
        InstansPreset::Iso => iso.to_owned(),
        InstansPreset::Date => date.to_owned(),
        InstansPreset::Time => time.chars().take(8).collect(),
    }
}

#[cfg(test)]
#[path = "format_spec_test.rs"]
mod format_spec_test;
