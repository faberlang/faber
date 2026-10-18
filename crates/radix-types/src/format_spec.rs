//! The `¶` format-spec vocabulary (D2.1–D2.5, D2.7).
//!
//! `value ¶ "spec"` renders a built-in value as `textus`. The spec is a
//! string literal (D2.2) following one fixed pattern for every type:
//!
//! ```text
//! [fill][align][sign][0][width][.precision][kind]
//! ```
//!
//! Each subject accepts only the parts that make sense for it: integers take
//! every part (precision pads, `42 ¶ ".2"` = `42.00`; `x b o e` kinds);
//! floats take everything except the radix kinds; `d64` decimals take the
//! float parts except the radix kinds (`e` is admitted, operator ruling
//! 2026-10-17; exact digits, `.N` rounds half-even);
//! `textus` takes fill, align,
//! width and `.N` (truncate to N scalars); `instans` takes only the named
//! presets `iso`, `date`, `time`.
//!
//! This module is the single parser and validator shared by typecheck and
//! every backend, plus the reference renderer the MIR runner executes. Target
//! emitters reproduce the same outputs; the corpus golden pins them together.

use crate::numeric_width::NumericWidth;
use crate::types::{Primitive, Type, TypeId, TypeTable};
use handroll::{Magnus, Radix};

/// Alignment inside the minimum width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatAlign {
    /// `<`
    Left,
    /// `>`
    Right,
    /// `^` (odd padding goes to the right, like Rust and Python)
    Center,
}

/// Presentation kind for numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    /// `x` lowercase hexadecimal.
    Hex,
    /// `b` binary.
    Binary,
    /// `o` octal.
    Octal,
    /// `e` scientific (`1.5e3`, Rust `LowerExp` shape).
    Scientific,
}

impl FormatKind {
    /// Radix of the integer digit kinds; `None` for scientific.
    #[must_use]
    pub fn radix(self) -> Option<u32> {
        match self {
            Self::Hex => Some(16),
            Self::Binary => Some(2),
            Self::Octal => Some(8),
            Self::Scientific => None,
        }
    }
}

/// Named `instans` presets. No strftime-style patterns (D2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstansPreset {
    /// Full ISO 8601 / RFC 3339 UTC wire at the value's precision.
    Iso,
    /// `YYYY-MM-DD` (UTC).
    Date,
    /// `HH:MM:SS` (UTC).
    Time,
}

/// The layout form of a spec: every part is optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutSpec {
    /// Fill character; only meaningful with an explicit align.
    pub fill: char,
    /// Explicit alignment (numbers default right, text left).
    pub align: Option<FormatAlign>,
    /// `+`: always show the sign.
    pub plus: bool,
    /// `0`: zero-pad after the sign.
    pub zero: bool,
    /// Minimum width in scalars (grows to fit, never truncates numbers).
    pub width: Option<usize>,
    /// `.N`: precision on numbers, maximum length on text.
    pub precision: Option<usize>,
    /// Number presentation kind.
    pub kind: Option<FormatKind>,
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
pub enum FormatSpec {
    /// `[fill][align][sign][0][width][.precision][kind]`.
    Layout(LayoutSpec),
    /// `iso`, `date`, or `time` (instans only).
    Preset(InstansPreset),
}

/// What the left side of `¶` is, as far as formatting cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatSubject {
    /// Signed integers (`numerus`, `numerus<i8…i64>`).
    Signed,
    /// Unsigned integers (`numerus<u8…u64>`, `modulus<W>`).
    Unsigned,
    /// `fractus` of any width.
    Float,
    /// `textus`.
    Text,
    /// `instans` of any precision.
    Instans,
    /// Decimal `numerus<d64>`: exact digits, precision rounds half-even (F4).
    Decimal,
    /// Anything else; user types format through an ordinary function.
    Unsupported,
}

impl FormatSubject {
    /// Whether the subject is an integer.
    #[must_use]
    pub fn is_integer(self) -> bool {
        matches!(self, Self::Signed | Self::Unsigned)
    }
}

/// One named part of the spec pattern, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatSpecPart {
    Fill,
    Align,
    Sign,
    Zero,
    Width,
    Precision,
    Kind,
}

impl FormatSpecPart {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Align => "align",
            Self::Sign => "sign",
            Self::Zero => "zero",
            Self::Width => "width",
            Self::Precision => "precision",
            Self::Kind => "kind",
        }
    }
}

/// Why a spec was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatSpecError {
    /// The text does not follow the spec pattern.
    Malformed,
    /// The part is legal in the pattern but not for this subject.
    PartNotAllowed(FormatSpecPart),
    /// `instans` accepts only the named presets.
    PresetRequired,
    /// A named preset on a non-`instans` subject.
    PresetNotAllowed,
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

/// Parse a spec's text. Validation against the subject is [`validate_format_spec`].
///
/// # Errors
///
/// [`FormatSpecError::Malformed`] when the text does not follow the pattern.
pub fn parse_format_spec(text: &str) -> Result<FormatSpec, FormatSpecError> {
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

/// Check a parsed spec against its subject (pass 2, D2.2).
///
/// `Unsupported` subjects are the caller's to reject first; here they accept
/// nothing.
///
/// # Errors
///
/// The first part that does not fit the subject.
pub fn validate_format_spec(
    spec: &FormatSpec,
    subject: FormatSubject,
) -> Result<(), FormatSpecError> {
    let layout = match (spec, subject) {
        (FormatSpec::Preset(_), FormatSubject::Instans) => return Ok(()),
        (FormatSpec::Preset(_), _) => return Err(FormatSpecError::PresetNotAllowed),
        (FormatSpec::Layout(_), FormatSubject::Instans) => {
            return Err(FormatSpecError::PresetRequired);
        }
        (FormatSpec::Layout(layout), _) => layout,
    };
    let reject = |part| Err(FormatSpecError::PartNotAllowed(part));
    // `0` pads between the sign and the digits; an explicit fill/align would
    // say something different, so the two never combine.
    if layout.zero && layout.align.is_some() {
        return reject(FormatSpecPart::Zero);
    }
    match subject {
        FormatSubject::Signed | FormatSubject::Unsigned => {
            if layout.precision.is_some() && layout.kind.and_then(FormatKind::radix).is_some() {
                return reject(FormatSpecPart::Precision);
            }
            Ok(())
        }
        FormatSubject::Float => {
            if layout.kind.and_then(FormatKind::radix).is_some() {
                return reject(FormatSpecPart::Kind);
            }
            Ok(())
        }
        FormatSubject::Text => {
            if layout.plus {
                return reject(FormatSpecPart::Sign);
            }
            if layout.zero {
                return reject(FormatSpecPart::Zero);
            }
            if layout.kind.is_some() {
                return reject(FormatSpecPart::Kind);
            }
            Ok(())
        }
        FormatSubject::Decimal => {
            // Radix kinds have no meaning for decimal fraction digits; scientific
            // notation is admitted.
            if layout.kind.and_then(FormatKind::radix).is_some() {
                return reject(FormatSpecPart::Kind);
            }
            Ok(())
        }
        FormatSubject::Instans | FormatSubject::Unsupported => Err(FormatSpecError::PresetRequired),
    }
}

/// Classify a type as a `¶` subject.
#[must_use]
pub fn format_subject(types: &TypeTable, ty: TypeId) -> FormatSubject {
    let (_, payload) = types.peel_access_marker(ty);
    match types.get(payload) {
        Type::Alias(_, inner) | Type::Ref(_, inner) => format_subject(types, *inner),
        Type::Primitive(Primitive::Numerus) => FormatSubject::Signed,
        Type::SizedNumeric(Primitive::Numerus, width) | Type::ModularWord(width) => {
            numerus_subject(*width)
        }
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
            FormatSubject::Float
        }
        Type::Primitive(Primitive::Textus) => FormatSubject::Text,
        Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => FormatSubject::Instans,
        _ => FormatSubject::Unsupported,
    }
}

fn numerus_subject(width: NumericWidth) -> FormatSubject {
    if width.is_decimal() {
        FormatSubject::Decimal
    } else if width.is_unbounded() {
        // Ruling 42: `inf` takes the signed integer specs exactly as `int`
        // does (radix kinds, width, fill, sign, precision); it is a signed
        // subject whose value renders through `render_magnus`.
        FormatSubject::Signed
    } else if width.is_unsigned_int() {
        FormatSubject::Unsigned
    } else {
        FormatSubject::Signed
    }
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
#[must_use]
pub fn render_integer(value: i128, spec: &LayoutSpec) -> String {
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
#[must_use]
pub fn render_magnus(value: &Magnus, spec: &LayoutSpec) -> String {
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
#[must_use]
pub fn render_float(value: f64, spec: &LayoutSpec, display: impl FnOnce(f64) -> String) -> String {
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
#[must_use]
pub fn render_decimal(carrier: i128, scale: u32, spec: &LayoutSpec) -> String {
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
    let mut exponent = digits.len() as i64 - 1 - scale as i64;

    match precision {
        Some(p) => {
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
        }
        None => {
            let coefficient = digits.trim_end_matches('0');
            if coefficient.len() > 1 {
                format!("{}.{}e{exponent}", &coefficient[..1], &coefficient[1..])
            } else {
                format!("{coefficient}e{exponent}")
            }
        }
    }
}

/// Render text: `.N` truncates to N scalars; width counts scalars (D2.7).
#[must_use]
pub fn render_text(value: &str, spec: &LayoutSpec) -> String {
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
#[must_use]
pub fn render_instans_preset(iso: &str, preset: InstansPreset) -> String {
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
