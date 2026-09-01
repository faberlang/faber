//! Runner runtime value model and scalar/tensor operations.
//!
//! `Value` is the interpreter-side mirror of MIR operands during stepping.

#![allow(
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::missing_errors_doc,
    clippy::needless_lifetimes,
    clippy::needless_pass_by_value,
    clippy::map_unwrap_or
)]
// Semantic numeric conversions between host value kinds: map-key
// canonicalization, modular-word carriers, width-check i128 intermediates,
// masked shift amounts, and mixed-carrier numeric math all convert between
// i64/u64/f64 by design.
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "semantic numeric conversions between host value kinds in the runner value model"
)]

use super::RunnerError;
use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol};
use radix_mir::MirClosureEnvironmentId;
use radix_mir::{MirBinOp, MirFunctionId};
use radix_runtime_contract::display::{display_bivalens, display_fractus, display_text_payload};
use radix_runtime_contract::instans::Instans;
use radix_runtime_contract::{D32_SCALE, D64_SCALE};
use radix_types::NumericWidth;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::rc::Rc;

/// Runtime value carried by the MIR runner.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    /// Unsigned integer magnitude (`numerus<u64>`); never sign-encoded.
    UInt(u64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// One-byte `ascii<1>` (`'x'`, `s[i]`). Not a heap `String`.
    Ascii(u8),
    Nil,
    Unit,
    Octeti(Vec<u8>),
    Instans(Instans),
    Regex(String),
    Sermo(Rc<RefCell<SermoValue>>),
    Function(MirFunctionId),
    Closure(MirFunctionId, MirClosureEnvironmentId, Box<Value>),
    ClosureEnvironment(
        MirClosureEnvironmentId,
        Rc<RefCell<FxHashMap<Symbol, Value>>>,
    ),
    Array(Rc<RefCell<Vec<Value>>>),
    Tensor(Rc<RefCell<TensorValue>>),
    /// Register-class vector: lane-ordered cells matching `VectorLane(i)`.
    Vector(Rc<RefCell<Vec<Value>>>),
    /// Register-class matrix: row-major flat cells matching `MatrixCell { row, column }`.
    Matrix(Rc<RefCell<MatrixValue>>),
    Map(Rc<RefCell<FxHashMap<MapKey, Value>>>),
    Set(Rc<RefCell<FxHashSet<MapKey>>>),
    Record(Rc<RefCell<FxHashMap<Symbol, Value>>>),
    Struct(DefId, Rc<RefCell<FxHashMap<Symbol, Value>>>),
    EnumVariant(DefId, Rc<RefCell<FxHashMap<Symbol, Value>>>),
    Tuple(Vec<Value>),
    /// Opaque gradient-handle id for in-process gradient accumulation
    /// (`MirIntrinsic::Gradient`). Carries the handle-key into the gradient
    /// storage map on the runner; never a numeric value.
    GradientHandle(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TensorValue {
    pub data: Vec<Value>,
    pub shape: Vec<i64>,
}

/// Opaque gradient-handle storage for the in-process runner.
///
/// Clean-break internal contract (mirror of the host LLVM gradient ABI
/// `__faber_rt_v1_gradient_*`): one handle per trainable parameter, flat
/// element-ordered data, explicit shape, element-wise accumulate, zero fill.
/// Values are `Value` carriers instead of raw `f32` buffers so the runner
/// never touches the device ABI ownership surface.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientStorage {
    pub data: Vec<Value>,
    pub shape: Vec<i64>,
}

/// Small register matrix carried by the CPU runner (Stage matrix subset).
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixValue {
    pub rows: u8,
    pub columns: u8,
    /// Row-major length `rows * columns`.
    pub data: Vec<Value>,
}

impl MatrixValue {
    #[must_use]
    pub fn cell(&self, row: u8, column: u8) -> Option<&Value> {
        let index = usize::from(row) * usize::from(self.columns) + usize::from(column);
        self.data.get(index)
    }

    #[must_use]
    pub fn cell_mut(&mut self, row: u8, column: u8) -> Option<&mut Value> {
        let index = usize::from(row) * usize::from(self.columns) + usize::from(column);
        self.data.get_mut(index)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SermoValue {
    pub route: String,
    pub opener: Value,
}

/// Hashable key subset for `tabula` / `copia`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    /// Negative integers only; non-negative integers canonicalize to `UInt`.
    Int(i64),
    /// Non-negative integers of either carrier, plus all unsigned magnitudes:
    /// same-value keys collide, upper-half magnitudes never sign-encode.
    UInt(u64),
    Bool(bool),
    Str(String),
    Unit,
}

impl Value {
    pub fn as_bool(&self, span: Span) -> Result<bool, RunnerError> {
        match self {
            Value::Bool(value) => Ok(*value),
            _ => Err(RunnerError::internal(span, "expected bivalens operand")),
        }
    }

    pub fn as_index(&self, span: Span) -> Result<usize, RunnerError> {
        match self {
            Value::Int(value) => usize::try_from(*value)
                .map_err(|_| RunnerError::internal(span, "expected non-negative numerus index")),
            Value::UInt(value) => usize::try_from(*value)
                .map_err(|_| RunnerError::internal(span, "expected non-negative numerus index")),
            _ => Err(RunnerError::internal(
                span,
                "expected non-negative numerus index",
            )),
        }
    }

    pub fn display(&self, interner: &Interner, span: Span) -> Result<String, RunnerError> {
        self.display_typed(interner, span, None)
    }

    /// Width-aware display: f32-declared floats render at f32 precision
    /// (9007e12c3 carried the declared width into arithmetic; the display
    /// path follows), everything else renders as before.
    pub fn display_typed(
        &self,
        interner: &Interner,
        span: Span,
        float_width: Option<NumericWidth>,
    ) -> Result<String, RunnerError> {
        Ok(match self {
            Value::Int(v) => v.to_string(),
            Value::UInt(v) => v.to_string(),
            Value::Float(v) => match float_width {
                Some(width) if width.is_float() && width != NumericWidth::F64 => {
                    // f32 shortest repr, keeping the fractus `N.0` integral
                    // shape (`5`, not `5.0`, is not a Faber fractus text).
                    #[expect(clippy::cast_possible_truncation)]
                    let narrowed = format!("{}", *v as f32);
                    let integral = narrowed
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == '-' || c == '+');
                    if integral {
                        format!("{narrowed}.0")
                    } else {
                        narrowed
                    }
                }
                _ => display_fractus(*v),
            },
            Value::Bool(v) => display_bivalens(*v).to_owned(),
            Value::Str(v) => display_text_payload(v).to_owned(),
            Value::Ascii(b) => {
                let ch = char::from(*b);
                let mut buf = [0u8; 4];
                display_text_payload(ch.encode_utf8(&mut buf)).to_owned()
            }
            Value::Nil => "nihil".to_owned(),
            Value::Unit => String::new(),
            Value::Octeti(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            Value::Instans(value) => value.to_rfc3339(),
            Value::Regex(pattern) => pattern.clone(),
            Value::Sermo(sermo) => format!("sermo({})", sermo.borrow().route),
            Value::Function(id) => format!("function f{}", id.0),
            Value::Closure(id, environment, _) => {
                format!("closure f{} env{}", id.0, environment.0)
            }
            Value::ClosureEnvironment(id, _) => format!("closure env{}", id.0),
            Value::Array(items) => display_list(&items.borrow(), interner, span, float_width)?,
            Value::Tensor(tensor) => {
                display_list(&tensor.borrow().data, interner, span, float_width)?
            }
            Value::Vector(lanes) => display_list(&lanes.borrow(), interner, span, float_width)?,
            Value::Matrix(matrix) => display_matrix(&matrix.borrow(), interner, span, float_width)?,
            Value::Map(entries) => {
                display_tabula_map(&entries.borrow(), interner, span, float_width)?
            }
            Value::Struct(_, fields) => display_tabula_struct(fields, interner, span, float_width)?,
            Value::Set(_) | Value::Record(_) | Value::EnumVariant(_, _) | Value::Tuple(_) => {
                format!("{self:?}")
            }
            Value::GradientHandle(id) => format!("gradient-handle#{id}"),
        })
    }

    pub fn to_key(&self, span: Span) -> Result<MapKey, RunnerError> {
        match self {
            // Canonical numeric key: every non-negative integer maps to `UInt`
            // so the same value finds the same entry regardless of carrier.
            // Negative values keep the signed key; upper-half unsigned
            // magnitudes can never collide with signed keys.
            Value::Int(v) if *v >= 0 => Ok(MapKey::UInt(*v as u64)),
            Value::Int(v) => Ok(MapKey::Int(*v)),
            Value::UInt(v) => Ok(MapKey::UInt(*v)),
            Value::Bool(v) => Ok(MapKey::Bool(*v)),
            Value::Str(v) => Ok(MapKey::Str(v.clone())),
            Value::Ascii(b) => Ok(MapKey::Str(char::from(*b).to_string())),
            Value::Unit => Ok(MapKey::Unit),
            _ => Err(RunnerError::internal(
                span,
                "value is not a valid map/set key",
            )),
        }
    }

    #[must_use]
    pub fn is_empty_collection(&self) -> bool {
        match self {
            Value::Array(cell) => cell.borrow().is_empty(),
            Value::Tensor(cell) => cell.borrow().data.is_empty(),
            Value::Map(cell) => cell.borrow().is_empty(),
            Value::Set(cell) => cell.borrow().is_empty(),
            Value::Str(value) => value.is_empty(),
            Value::Ascii(_) => false,
            Value::Octeti(bytes) => bytes.is_empty(),
            _ => false,
        }
    }
}

fn display_list(
    items: &[Value],
    interner: &Interner,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let parts = items
        .iter()
        .map(|item| display_in_collection(item, interner, span, float_width))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("[{}]", parts.join(", ")))
}

/// Render a register matrix as nested rows `[[...],[...]]`, matching the
/// compiled host route (row-major cells split on the column count).
fn display_matrix(
    matrix: &MatrixValue,
    interner: &Interner,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let rows = usize::from(matrix.rows);
    let columns = usize::from(matrix.columns);
    let row_parts = (0..rows)
        .map(|row| {
            let start = row * columns;
            let end = start + columns;
            display_list(
                matrix.data.get(start..end).unwrap_or(&[]),
                interner,
                span,
                float_width,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("[{}]", row_parts.join(", ")))
}

fn display_in_collection(
    value: &Value,
    interner: &Interner,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    Ok(match value {
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Array(items) => display_list(&items.borrow(), interner, span, float_width)?,
        Value::Map(entries) => display_tabula_map(&entries.borrow(), interner, span, float_width)?,
        Value::Struct(_, fields) => display_tabula_struct(fields, interner, span, float_width)?,
        other => other.display_typed(interner, span, float_width)?,
    })
}

fn display_tabula_map(
    entries: &FxHashMap<MapKey, Value>,
    interner: &Interner,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let mut parts = Vec::new();
    for (key, value) in entries {
        let key_str = match key {
            MapKey::Str(s) => format!("\"{s}\""),
            MapKey::Int(v) => v.to_string(),
            MapKey::UInt(v) => v.to_string(),
            MapKey::Bool(v) => format!("{v}"),
            MapKey::Unit => "nihil".to_owned(),
        };
        let val_str = display_in_collection(value, interner, span, float_width)?;
        parts.push(format!("{key_str}: {val_str}"));
    }
    Ok(format!("{{{}}}", parts.join(", ")))
}

fn display_tabula_struct(
    fields: &RefCell<FxHashMap<Symbol, Value>>,
    interner: &Interner,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let mut parts = Vec::new();
    for (symbol, value) in fields.borrow().iter() {
        let name = interner.resolve(*symbol);
        let val_str = display_in_collection(value, interner, span, float_width)?;
        parts.push(format!("\"{name}\": {val_str}"));
    }
    Ok(format!("{{{}}}", parts.join(", ")))
}

pub fn int_bitwise_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    span: Span,
) -> Result<Value, RunnerError> {
    // Unsigned semantics whenever either side is `UInt`: a non-negative signed
    // operand widens exactly. A negative signed operand in an unsigned context
    // fails closed (typecheck rejects it upstream; carriers can still mix
    // through conversions, so the boundary must not reinterpret sign bits).
    let unsigned = match (&left, &right) {
        (Value::UInt(a), Value::UInt(b)) => Some((*a, *b)),
        (Value::UInt(a), Value::Int(b)) => u64::try_from(*b).ok().map(|b| (*a, b)),
        (Value::Int(a), Value::UInt(b)) => u64::try_from(*a).ok().map(|a| (a, *b)),
        _ => None,
    };
    if let Some((left, right)) = unsigned {
        let shift = (right & 0x3f) as u32;
        return Ok(Value::UInt(match op {
            MirBinOp::BitAnd => left & right,
            MirBinOp::BitOr => left | right,
            MirBinOp::BitXor => left ^ right,
            MirBinOp::Shl => left.wrapping_shl(shift),
            MirBinOp::Shr => left.wrapping_shr(shift),
            _ => return Err(RunnerError::internal(span, "invalid bitwise operator")),
        }));
    }
    let (Value::Int(left), Value::Int(right)) = (left, right) else {
        return Err(RunnerError::internal(
            span,
            "bitwise operand requires numerus",
        ));
    };
    let shift = |amount: i64| (amount & 0x3f) as u32;
    Ok(Value::Int(match op {
        MirBinOp::BitAnd => left & right,
        MirBinOp::BitOr => left | right,
        MirBinOp::BitXor => left ^ right,
        MirBinOp::Shl => left.wrapping_shl(shift(right)),
        MirBinOp::Shr => left.wrapping_shr(shift(right)),
        _ => return Err(RunnerError::internal(span, "invalid bitwise operator")),
    }))
}

/// Width authority for modular execution: values are reduced modulo 2^W.
/// The carrier is u64 internally; results surface as `Int` below u64 and as
/// `UInt` at u64 so upper-half magnitudes never sign-encode.
///
/// `Type::ModularWord` widths are unsigned by semantic admission, so
/// `unsigned_max` is total in practice; the fallback keeps the helper
/// panic-free without encoding that invariant as a panic budget cost.
pub(crate) fn modular_word_mask(width: NumericWidth) -> u64 {
    width.unsigned_max().unwrap_or(u64::MAX)
}

fn modular_word_bits(width: NumericWidth) -> u32 {
    // 2^W - 1 has exactly W significant bits.
    64 - modular_word_mask(width).leading_zeros()
}

pub(crate) fn modular_word_result(value: u64, width: NumericWidth) -> Value {
    if width == NumericWidth::U64 {
        Value::UInt(value)
    } else {
        Value::Int(value as i64)
    }
}

pub(crate) fn modular_operand(
    value: Value,
    span: Span,
    error: &'static str,
) -> Result<u64, RunnerError> {
    match value {
        Value::Int(value) if value >= 0 => {
            u64::try_from(value).map_err(|_| RunnerError::internal(span, error))
        }
        Value::UInt(value) => Ok(value),
        _ => Err(RunnerError::internal(span, error)),
    }
}

/// Execute one `modulus<W>` arithmetic operation. Conversion at this boundary
/// keeps modular wraparound separate from checked `numerus` math.
pub fn modular_word_numeric_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let left = modular_operand(left, span, "modulus operand mismatch")?;
    let right = modular_operand(right, span, "modulus operand mismatch")?;
    let mask = modular_word_mask(width);
    let value = match op {
        MirBinOp::Add => left.wrapping_add(right) & mask,
        MirBinOp::Sub => left.wrapping_sub(right) & mask,
        MirBinOp::Mul => left.wrapping_mul(right) & mask,
        MirBinOp::Div => left.checked_div(right).ok_or_else(|| {
            RunnerError::internal_with_issue(
                span,
                "modulus division failed",
                "modular_division_by_zero",
            )
        })?,
        MirBinOp::Mod => left.checked_rem(right).ok_or_else(|| {
            RunnerError::internal_with_issue(
                span,
                "modulus division failed",
                "modular_division_by_zero",
            )
        })?,
        _ => {
            return Err(RunnerError::internal(
                span,
                "invalid modulus arithmetic operator",
            ));
        }
    };
    Ok(modular_word_result(value, width))
}

pub fn modular_word_neg(
    value: Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let value = modular_operand(value, span, "modulus negation operand mismatch")?;
    Ok(modular_word_result(
        value.wrapping_neg() & modular_word_mask(width),
        width,
    ))
}

pub fn modular_word_bitnot(
    value: Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let value = modular_operand(value, span, "modulus bitnot operand mismatch")?;
    Ok(modular_word_result(
        !value & modular_word_mask(width),
        width,
    ))
}

pub fn modular_word_bitwise_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let left = modular_operand(left, span, "modulus bitwise operand mismatch")?;
    let right = modular_operand(right, span, "modulus bitwise operand mismatch")?;
    let mask = modular_word_mask(width);
    let shift = (right & u64::from(modular_word_bits(width) - 1)) as u32;
    let value = match op {
        MirBinOp::BitAnd => left & right,
        MirBinOp::BitOr => left | right,
        MirBinOp::BitXor => left ^ right,
        MirBinOp::Shl => left.wrapping_shl(shift) & mask,
        MirBinOp::Shr => (left & mask).wrapping_shr(shift),
        _ => {
            return Err(RunnerError::internal(
                span,
                "invalid modulus bitwise operator",
            ));
        }
    };
    Ok(modular_word_result(value, width))
}

pub fn modular_word_compare_values(
    left: Value,
    right: Value,
    op: MirBinOp,
    span: Span,
) -> Result<Value, RunnerError> {
    let left = modular_operand(left, span, "modulus comparison operand mismatch")?;
    let right = modular_operand(right, span, "modulus comparison operand mismatch")?;
    let ordering = left.cmp(&right);
    let result = match op {
        MirBinOp::Lt => ordering.is_lt(),
        MirBinOp::Gt => ordering.is_gt(),
        MirBinOp::LtEq => ordering.is_le(),
        MirBinOp::GtEq => ordering.is_ge(),
        _ => {
            return Err(RunnerError::internal(
                span,
                "invalid modulus comparison operator",
            ));
        }
    };
    Ok(Value::Bool(result))
}

// INVARIANT: the runner is Faber's target-neutral numeric authority. Integer
// overflow is therefore decided by the declared `numerus<W>` width, not by host
// i64 bounds. These helpers compute in i128 and range-check against `W` so a
// `numerus<u8>` add of 200 + 100 fails here exactly as `u8::checked_add` would
// fail in Rust codegen — independent of any target backend. Faber requires
// same-width operands, so the result width governs both sides.
//
// Scope: integer widths only. Float width fidelity (f32 vs f64 rounding) is a
// separate gap. The signed `(Int, Int)` carrier cannot encode `numerus<u64>`
// magnitudes above i64::MAX — those results fail closed here (`check_int_width`
// rejects them) instead of wrapping; the unsigned carrier path
// (`check_uint_width`, used for `Value::UInt` operands) keeps upper-half
// magnitudes exact.

/// Signed `[min, max]` of an integer width as i128, so every Faber integer width
/// — including `u64` — range-checks without host overflow.
#[must_use]
pub fn int_width_range(width: NumericWidth) -> (i128, i128) {
    match width {
        NumericWidth::I8 => (-128, 127),
        NumericWidth::I16 => (-32_768, 32_767),
        NumericWidth::I32 => (i32::MIN as i128, i32::MAX as i128),
        NumericWidth::I64 => (i64::MIN as i128, i64::MAX as i128),
        NumericWidth::U8 => (0, u8::MAX as i128),
        NumericWidth::U16 => (0, u16::MAX as i128),
        NumericWidth::U32 => (0, u32::MAX as i128),
        NumericWidth::U64 => (0, u64::MAX as i128),
        // Floats are not range-checked here; their width is a separate fidelity gap.
        NumericWidth::F16 | NumericWidth::BF16 | NumericWidth::F32 | NumericWidth::F64 => {
            (i64::MIN as i128, i64::MAX as i128)
        }
        // decimal-widths D1: scaled-integer carriers fit their integer domain
        // (d32 = i32×10⁻⁷, d64 = i64×10⁻⁹); D4 owns runner arithmetic.
        NumericWidth::D32 => (i32::MIN as i128, i32::MAX as i128),
        NumericWidth::D64 => (i64::MIN as i128, i64::MAX as i128),
    }
}

/// Decimal scale as an integer factor (`d32 = 10^7`, `d64 = 10^9`).
///
/// The decimal runner stores the scaled integer in the ordinary `Value::Int`
/// carrier. Keeping the factor here, next to the width bounds, makes every
/// arithmetic path use the same representation contract.
#[must_use]
pub(crate) fn decimal_scale_factor(width: NumericWidth) -> Option<i128> {
    match width {
        NumericWidth::D32 => Some(i128::from(D32_SCALE)),
        NumericWidth::D64 => Some(i128::from(D64_SCALE)),
        _ => None,
    }
}

/// Pure round-half-even quotient for values whose intermediate arithmetic has
/// already been checked. Rust integer division truncates toward zero, so the
/// remainder decides whether the quotient moves away from zero. A midpoint
/// moves only when the truncated quotient is odd.
#[must_use]
pub(crate) fn round_half_even_quotient(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator == 0 {
        return None;
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder == 0 {
        return Some(quotient);
    }

    let remainder_abs = remainder.unsigned_abs();
    let denominator_abs = denominator.unsigned_abs();
    let twice_remainder = remainder_abs.checked_mul(2)?;
    let rounds = twice_remainder > denominator_abs
        || (twice_remainder == denominator_abs && quotient % 2 != 0);
    if !rounds {
        return Some(quotient);
    }

    let step = if (numerator < 0) == (denominator < 0) {
        1
    } else {
        -1
    };
    quotient.checked_add(step)
}

/// Checked round-half-even division with the runner's standard arithmetic
/// diagnostics. Decimal parsing uses the pure quotient helper so it can
/// distinguish malformed text from a carrier overflow.
pub(crate) fn round_half_even_division(
    numerator: i128,
    denominator: i128,
    span: Span,
    error: &'static str,
) -> Result<i128, RunnerError> {
    round_half_even_quotient(numerator, denominator)
        .ok_or_else(|| RunnerError::internal(span, error))
}

/// Execute one decimal arithmetic operation over scaled `Value::Int` values.
/// Addition, subtraction, and remainder stay in carrier units. Multiplication
/// divides by the scale and division multiplies by the scale; both reductions
/// use checked i128 intermediates and round-half-even.
pub(crate) fn decimal_numeric_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let (Value::Int(left), Value::Int(right)) = (left, right) else {
        return Err(RunnerError::internal(
            span,
            "decimal operand requires scaled numerus",
        ));
    };
    let scale = decimal_scale_factor(width)
        .ok_or_else(|| RunnerError::internal(span, "decimal arithmetic requires decimal width"))?;
    let result = match op {
        MirBinOp::Add => (left as i128)
            .checked_add(right as i128)
            .ok_or_else(|| RunnerError::internal(span, "numerus overflow"))?,
        MirBinOp::Sub => (left as i128)
            .checked_sub(right as i128)
            .ok_or_else(|| RunnerError::internal(span, "numerus overflow"))?,
        MirBinOp::Mul => {
            let product = (left as i128)
                .checked_mul(right as i128)
                .ok_or_else(|| RunnerError::internal(span, "numerus overflow"))?;
            round_half_even_division(product, scale, span, "numerus overflow")?
        }
        MirBinOp::Div => {
            if right == 0 {
                return Err(RunnerError::internal(span, "numerus division failed"));
            }
            let numerator = (left as i128)
                .checked_mul(scale)
                .ok_or_else(|| RunnerError::internal(span, "numerus division failed"))?;
            round_half_even_division(numerator, right as i128, span, "numerus division failed")?
        }
        MirBinOp::Mod => {
            if right == 0 {
                return Err(RunnerError::internal(span, "numerus division failed"));
            }
            (left as i128) % (right as i128)
        }
        _ => {
            return Err(RunnerError::internal(
                span,
                "invalid decimal arithmetic operator",
            ));
        }
    };
    check_int_width(result, width, span, "numerus overflow").map(Value::Int)
}

fn check_int_width(
    result: i128,
    width: NumericWidth,
    span: Span,
    error: &'static str,
) -> Result<i64, RunnerError> {
    let (lo, hi) = int_width_range(width);
    if !(lo..=hi).contains(&result) {
        return Err(RunnerError::internal(span, error));
    }
    // The i64 carrier cannot sign-encode `numerus<u64>` magnitudes above
    // i64::MAX — a raw `as i64` cast wraps them negative. Fail closed instead,
    // matching the out-of-range discipline of every other width; genuine u64
    // arithmetic runs through the unsigned carrier path (`check_uint_width`),
    // which keeps upper-half magnitudes exact as `UInt`.
    let Ok(encoded) = i64::try_from(result) else {
        return Err(RunnerError::internal(span, error));
    };
    Ok(encoded)
}

pub fn width_checked_neg(v: i64, width: NumericWidth, span: Span) -> Result<i64, RunnerError> {
    check_int_width(-(v as i128), width, span, "numerus overflow")
}

pub fn width_checked_add(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    check_int_width(a as i128 + b as i128, width, span, "numerus overflow")
}

pub fn width_checked_sub(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    check_int_width(a as i128 - b as i128, width, span, "numerus overflow")
}

pub fn width_checked_mul(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    check_int_width(a as i128 * b as i128, width, span, "numerus overflow")
}

/// Division by zero and signed-min / -1 overflow both report `numerus division
/// failed`, matching the prior i64 `checked_div` error text.
pub fn width_checked_div(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    if b == 0 {
        return Err(RunnerError::internal(span, "numerus division failed"));
    }
    check_int_width(
        (a as i128) / (b as i128),
        width,
        span,
        "numerus division failed",
    )
}

pub fn width_checked_rem(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    if b == 0 {
        return Err(RunnerError::internal(span, "numerus division failed"));
    }
    check_int_width(
        (a as i128) % (b as i128),
        width,
        span,
        "numerus division failed",
    )
}

/// Unsigned counterparts of the width-checked ops above. They compute in u128
/// and range-check against the declared width so `numerus<u64>` upper-half
/// magnitudes stay exact — never sign-encoded into i64.
fn check_uint_width(
    result: u128,
    width: NumericWidth,
    span: Span,
    error: &'static str,
) -> Result<u64, RunnerError> {
    let (_, hi) = int_width_range(width);
    if result <= hi as u128 {
        Ok(result as u64)
    } else {
        Err(RunnerError::internal(span, error))
    }
}

/// Unsigned negation: only zero is representable; anything else overflows.
pub fn uint_width_checked_neg(
    v: u64,
    _width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    if v == 0 {
        Ok(0)
    } else {
        Err(RunnerError::internal(span, "numerus overflow"))
    }
}

pub fn uint_width_checked_add(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    check_uint_width(a as u128 + b as u128, width, span, "numerus overflow")
}

pub fn uint_width_checked_sub(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    let result = a
        .checked_sub(b)
        .ok_or_else(|| RunnerError::internal(span, "numerus overflow"))?;
    check_uint_width(result as u128, width, span, "numerus overflow")
}

pub fn uint_width_checked_mul(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    check_uint_width(a as u128 * b as u128, width, span, "numerus overflow")
}

pub fn uint_width_checked_div(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    if b == 0 {
        return Err(RunnerError::internal(span, "numerus division failed"));
    }
    check_uint_width((a / b) as u128, width, span, "numerus division failed")
}

pub fn uint_width_checked_rem(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    if b == 0 {
        return Err(RunnerError::internal(span, "numerus division failed"));
    }
    check_uint_width((a % b) as u128, width, span, "numerus division failed")
}

/// Round a scalar float to the runner's declared arithmetic width.
///
/// WHY: `Value::Float` is a single f64 carrier, but Faber scalar arithmetic
/// still executes at the declared width. The fuzzy-value path uses this same
/// f32-round-trip policy for every non-f64 width.
fn round_float(value: f64, width: NumericWidth) -> f64 {
    if matches!(width, NumericWidth::F64) {
        value
    } else {
        #[expect(clippy::cast_possible_truncation)]
        {
            value as f32 as f64
        }
    }
}

fn float_binop(
    left: f64,
    right: f64,
    width: NumericWidth,
    float_op: impl FnOnce(f64, f64) -> f64,
) -> f64 {
    let left = round_float(left, width);
    let right = round_float(right, width);
    round_float(float_op(left, right), width)
}

/// Numeric binary dispatch. The integer path delegates to a width-checked op so
/// overflow honors `numerus<W>`; scalar float operands and results are rounded
/// at the declared width before and after the operation.
pub fn numeric_binop(
    left: Value,
    right: Value,
    int_op: fn(i64, i64, NumericWidth, Span) -> Result<i64, RunnerError>,
    uint_op: fn(u64, u64, NumericWidth, Span) -> Result<u64, RunnerError>,
    float_op: impl FnOnce(f64, f64) -> f64,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => int_op(a, b, width, span).map(Value::Int),
        (Value::UInt(a), Value::UInt(b)) => uint_op(a, b, width, span).map(Value::UInt),
        // Mixed signed/unsigned integer operands are accepted only when the
        // signed side is non-negative and the result width is unsigned — there
        // is no lossy reinterpretation either way.
        (Value::UInt(a), Value::Int(b)) if width.is_unsigned_int() && b >= 0 => {
            uint_op(a, b as u64, width, span).map(Value::UInt)
        }
        (Value::Int(a), Value::UInt(b)) if width.is_unsigned_int() && a >= 0 => {
            uint_op(a as u64, b, width, span).map(Value::UInt)
        }
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_binop(a, b, width, float_op))),
        (Value::Int(a), Value::Float(b)) => {
            Ok(Value::Float(float_binop(a as f64, b, width, float_op)))
        }
        (Value::UInt(a), Value::Float(b)) => {
            Ok(Value::Float(float_binop(a as f64, b, width, float_op)))
        }
        (Value::Float(a), Value::Int(b)) => {
            Ok(Value::Float(float_binop(a, b as f64, width, float_op)))
        }
        (Value::Float(a), Value::UInt(b)) => {
            Ok(Value::Float(float_binop(a, b as f64, width, float_op)))
        }
        _ => Err(RunnerError::internal(span, "numeric operand mismatch")),
    }
}

pub fn add_values(
    left: Value,
    right: Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    if width.is_decimal() {
        return decimal_numeric_binop(left, right, MirBinOp::Add, width, span);
    }
    match (left, right) {
        (Value::Str(left), Value::Str(right)) => Ok(Value::Str(format!("{left}{right}"))),
        (left, right) => numeric_binop(
            left,
            right,
            width_checked_add,
            uint_width_checked_add,
            |a, b| a + b,
            width,
            span,
        ),
    }
}

pub fn compare_values(
    left: Value,
    right: Value,
    op: MirBinOp,
    span: Span,
) -> Result<Value, RunnerError> {
    let ordering = match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::UInt(a), Value::UInt(b)) => a.cmp(b),
        (Value::UInt(a), Value::Int(b)) => {
            if *b < 0 {
                std::cmp::Ordering::Greater
            } else {
                a.cmp(&(*b as u64))
            }
        }
        (Value::Int(a), Value::UInt(b)) => {
            if *a < 0 {
                std::cmp::Ordering::Less
            } else {
                (*a as u64).cmp(b)
            }
        }
        (Value::Float(a), Value::Float(b)) => a
            .partial_cmp(b)
            .ok_or_else(|| RunnerError::internal(span, "float compare"))?,
        // Mixed integer/float ordering joins at the float carrier — the same
        // promotion `numeric_binop` applies to `+ - * /`, so `a < b` accepts
        // exactly the operand mixes `a + b` accepts (join-cast ruling).
        (Value::Int(a), Value::Float(b)) => (*a as f64)
            .partial_cmp(b)
            .ok_or_else(|| RunnerError::internal(span, "float compare"))?,
        (Value::UInt(a), Value::Float(b)) => (*a as f64)
            .partial_cmp(b)
            .ok_or_else(|| RunnerError::internal(span, "float compare"))?,
        (Value::Float(a), Value::Int(b)) => a
            .partial_cmp(&(*b as f64))
            .ok_or_else(|| RunnerError::internal(span, "float compare"))?,
        (Value::Float(a), Value::UInt(b)) => a
            .partial_cmp(&(*b as f64))
            .ok_or_else(|| RunnerError::internal(span, "float compare"))?,
        (Value::Str(a), Value::Str(b)) => a.cmp(b),
        (Value::Ascii(a), Value::Ascii(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Instans(a), Value::Instans(b)) => a
            .partial_cmp_at_coarser(*b)
            .ok_or_else(|| RunnerError::internal(span, "instans compare"))?,
        _ => return Err(RunnerError::internal(span, "comparison type mismatch")),
    };
    let result = match op {
        MirBinOp::Lt => ordering.is_lt(),
        MirBinOp::Gt => ordering.is_gt(),
        MirBinOp::LtEq => ordering.is_le(),
        MirBinOp::GtEq => ordering.is_ge(),
        _ => return Err(RunnerError::internal(span, "invalid comparison operator")),
    };
    Ok(Value::Bool(result))
}

#[must_use]
pub fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Instans(a), Value::Instans(b)) => a
            .partial_cmp_at_coarser(*b)
            .is_some_and(|ord| ord == std::cmp::Ordering::Equal),
        // Mixed carriers compare by numeric value: a non-negative `Int` and a
        // `UInt` with the same magnitude are the same Faber integer. Equality
        // compares bit patterns per the modular contract; for non-negative
        // values the bit pattern is the magnitude.
        (Value::Int(a), Value::UInt(b)) => u64::try_from(*a).is_ok_and(|a| a == *b),
        (Value::UInt(a), Value::Int(b)) => u64::try_from(*b).is_ok_and(|b| *a == b),
        // `≅` promotes mixed integer/float operands to the joined float type
        // before exact comparison. Keep integer/unsigned comparison above
        // lossless; only the promoted pair uses the target f64 carrier, matching
        // generated Rust's numeric promotion. Fuzzy `≈` is a collection
        // Approximate call and does not use this exact-value helper.
        (Value::Int(a), Value::Float(b)) => (*a as f64)
            .partial_cmp(b)
            .is_some_and(std::cmp::Ordering::is_eq),
        (Value::Float(a), Value::Int(b)) => a
            .partial_cmp(&(*b as f64))
            .is_some_and(std::cmp::Ordering::is_eq),
        (Value::UInt(a), Value::Float(b)) => (*a as f64)
            .partial_cmp(b)
            .is_some_and(std::cmp::Ordering::is_eq),
        (Value::Float(a), Value::UInt(b)) => a
            .partial_cmp(&(*b as f64))
            .is_some_and(std::cmp::Ordering::is_eq),
        // Scalar `s[i]`/`accipe` yields the one-byte `ascii<1>` carrier while
        // literals arrive as heap textus; the declared intrinsic return is
        // Textus, so equality joins the carriers by character value (a single
        // ASCII byte). Multi-byte textus never equals an `ascii<1>`.
        (Value::Ascii(a), Value::Str(b)) => b.as_bytes() == [*a],
        (Value::Str(a), Value::Ascii(b)) => a.as_bytes() == [*b],
        _ => left == right,
    }
}

/// One-byte `ascii<1>` from a Unicode scalar. Non-ASCII fails closed.
pub fn ascii1_from_scalar(ch: char, span: Span) -> Result<Value, RunnerError> {
    if u32::from(ch) > 0x7f {
        return Err(RunnerError::internal(span, "textus index trap"));
    }
    Ok(Value::Ascii(ch as u8))
}

/// Scalar `[i]` on a textus/ascii payload as `ascii<1>`, not a heap `String`.
pub fn scalar_text_index(text: &str, index: usize, span: Span) -> Result<Value, RunnerError> {
    let ch = text
        .chars()
        .nth(index)
        .ok_or_else(|| RunnerError::internal(span, "text index out of bounds"))?;
    ascii1_from_scalar(ch, span)
}

pub fn format_template(
    template: &str,
    args: &[Value],
    interner: &Interner,
    span: Span,
) -> Result<String, RunnerError> {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    let mut next_arg = 0usize;
    while let Some(ch) = chars.next() {
        if ch != '§' {
            out.push(ch);
            continue;
        }
        let mut index = String::new();
        while chars.peek().is_some_and(char::is_ascii_digit) {
            let Some(digit) = chars.next() else {
                break;
            };
            index.push(digit);
        }
        let arg_index = if index.is_empty() {
            let current = next_arg;
            next_arg += 1;
            current
        } else {
            index.parse::<usize>().unwrap_or(usize::MAX)
        };
        let rendered = args
            .get(arg_index)
            .map(|arg| arg.display(interner, span))
            .transpose()?
            .unwrap_or_else(|| format!("§{index}"));
        out.push_str(&rendered);
    }
    Ok(out)
}
