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
//! floats take everything except the radix kinds; `textus` takes fill, align,
//! width and `.N` (truncate to N scalars); `instans` takes only the named
//! presets `iso`, `date`, `time`.
//!
//! This module is the single parser and validator shared by typecheck and
//! every backend, plus the reference renderer the MIR runner executes. Target
//! emitters reproduce the same outputs; the corpus golden pins them together.

use crate::numeric_width::NumericWidth;
use crate::types::{Primitive, Type, TypeId, TypeTable};

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
    /// Decimal `numerus<d64>`: display policy is held (fork F4).
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
/// `Decimal` and `Unsupported` subjects are the caller's to reject first;
/// here they accept nothing.
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
        FormatSubject::Instans | FormatSubject::Decimal | FormatSubject::Unsupported => {
            Err(FormatSpecError::PresetRequired)
        }
    }
}

/// Classify a type as a `¶` subject.
#[must_use]
pub fn format_subject(types: &TypeTable, ty: TypeId) -> FormatSubject {
    let (_, payload) = types.peel_access_marker(ty);
    match types.get(payload) {
        Type::Alias(_, inner) | Type::Ref(_, inner) => format_subject(types, *inner),
        Type::Primitive(Primitive::Numerus) => FormatSubject::Signed,
        Type::SizedNumeric(Primitive::Numerus, width) => numerus_subject(*width),
        Type::ModularWord(width) => numerus_subject(*width),
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
