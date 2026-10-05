//! `Magnus`: a std-only, hand-rolled arbitrary-precision signed integer — the carrier of the
//! opt-in unbounded `inf` integer width (inf-delivery-spec rulings 36, 40, 44, 45).
//!
//! Representation: sign plus little-endian `u32` limbs, always normalized (no high zero limbs;
//! zero is the empty limb vector and never negative), so the derived `Eq`/`Hash` are exact.
//!
//! Semantics, all exact (no operation here ever wraps or saturates):
//! - `+ - *`, unary `-`: exact.
//! - `/` and `%` are **floor** division and the floor remainder (sign of the divisor,
//!   D11.6 rulings 14 and 29): `a == b*(a/b) + a%b`, `0 <= a%b < b` for `b > 0`.
//! - `potentia` is exact; a negative exponent is an error (ruling 28), `0^0 == 1`.
//! - `∧ ∨ ⊻ ¬` act on the infinite two's-complement extension (`¬x == -x - 1`).
//! - `⇐` (shift left) is `x * 2^n` with no cap; `⇒` (shift right) is the arithmetic shift
//!   `floor(x / 2^n)`.
//! - `to_f64` rounds to nearest, ties to even, and overflows to `±∞` exactly as IEEE 754
//!   conversion does (a magnitude of at least `2^1024 - 2^970` becomes infinity); `from_f64`
//!   truncates toward zero and rejects NaN and `±∞`.
//!
//! Resource contract (ruling 45): `Magnus` itself has no size bound; running out of memory is a
//! process abort like any other allocation failure. The `*_within` methods take a bit-length
//! ceiling and fail fast with [`MagnusError::CeilingExceeded`] *before* allocating, for callers
//! (the MIR runner's `RUNNER_INF_BIT_CEILING`) that want a fault instead of thrashing.
//!
//! Complexity, with `n` and `m` the operand limb counts: `+ - ⇐ ⇒` and bitwise ops and
//! comparison are O(n); `*` is schoolbook O(n·m) (no Karatsuba); `/ %` are Knuth Algorithm D,
//! O(n·m); `potentia` is O(M(n·e) · log e); decimal parse and render are O(n²); radix 2/8/16
//! render is O(n). Hashing is O(n).

use std::cmp::Ordering;
use std::fmt;

const LIMB_BITS: usize = 32;
const BASE: u64 = 1 << 32;

/// Why a `Magnus` operation failed. Every variant is a value-level failure the caller maps to
/// its own trap or `⇥`; none is a resource-exhaustion abort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagnusError {
    /// Floor division or remainder with a zero divisor.
    DivisionByZero,
    /// `potentia` with a negative exponent (ruling 28).
    NegativeExponent,
    /// A shift whose count is negative (ruling 10).
    NegativeShift,
    /// `from_f64` of NaN or `±∞`.
    NotFinite,
    /// Text is empty, is only a sign, or contains a character that is not a digit of the radix.
    Malformed,
    /// A `*_within` operation would exceed its bit-length ceiling.
    CeilingExceeded,
    /// The operation's size does not fit this machine's address space at all.
    Unrepresentable,
}

impl fmt::Display for MagnusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            MagnusError::DivisionByZero => "division by zero",
            MagnusError::NegativeExponent => "negative exponent",
            MagnusError::NegativeShift => "negative shift count",
            MagnusError::NotFinite => "NaN or infinity has no integer value",
            MagnusError::Malformed => "malformed integer text",
            MagnusError::CeilingExceeded => "integer size ceiling exceeded",
            MagnusError::Unrepresentable => "integer size is not representable",
        };
        f.write_str(text)
    }
}

impl std::error::Error for MagnusError {}

/// The text radices of `↦ textus` and `↦ ascii<N> via Hex/Bin/Oct` (ruling 40).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Radix {
    Bin,
    Oct,
    Dec,
    Hex,
}

impl Radix {
    fn base(self) -> u32 {
        match self {
            Radix::Bin => 2,
            Radix::Oct => 8,
            Radix::Dec => 10,
            Radix::Hex => 16,
        }
    }
}

/// Arbitrary-precision signed integer. See the module docs for semantics and complexity.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct Magnus {
    neg: bool,
    mag: Vec<u32>,
}

// ---------------------------------------------------------------------------------------------
// limb helpers (magnitudes are little-endian slices; results are trimmed)
// ---------------------------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)] // WHY: the mask makes the truncation exact.
fn lo(x: u64) -> u32 {
    (x & 0xFFFF_FFFF) as u32
}

fn hi(x: u64) -> u32 {
    lo(x >> 32)
}

fn trim(v: &mut Vec<u32>) {
    while v.last() == Some(&0) {
        v.pop();
    }
}

fn cmp_mag(a: &[u32], b: &[u32]) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| {
        a.iter()
            .rev()
            .zip(b.iter().rev())
            .map(|(x, y)| x.cmp(y))
            .find(|ord| ord.is_ne())
            .unwrap_or(Ordering::Equal)
    })
}

fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = Vec::with_capacity(long.len() + 1);
    let mut carry = 0_u64;
    for (i, &limb) in long.iter().enumerate() {
        let sum = u64::from(limb) + u64::from(short.get(i).copied().unwrap_or(0)) + carry;
        out.push(lo(sum));
        carry = sum >> 32;
    }
    if carry != 0 {
        out.push(lo(carry));
    }
    out
}

/// `a - b` for `a >= b`.
fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0_u64;
    for (i, &limb) in a.iter().enumerate() {
        let sub = u64::from(b.get(i).copied().unwrap_or(0)) + borrow;
        let cur = u64::from(limb);
        if cur >= sub {
            out.push(lo(cur - sub));
            borrow = 0;
        } else {
            out.push(lo(cur + BASE - sub));
            borrow = 1;
        }
    }
    trim(&mut out);
    out
}

fn mul_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0_u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0_u64;
        for (j, &y) in b.iter().enumerate() {
            let cur = u64::from(out[i + j]) + u64::from(x) * u64::from(y) + carry;
            out[i + j] = lo(cur);
            carry = hi(cur).into();
        }
        out[i + b.len()] = lo(carry);
    }
    trim(&mut out);
    out
}

/// `v = v * m + a` in place.
fn mul_small_add(v: &mut Vec<u32>, m: u32, a: u32) {
    let mut carry = u64::from(a);
    for limb in v.iter_mut() {
        let cur = u64::from(*limb) * u64::from(m) + carry;
        *limb = lo(cur);
        carry = cur >> 32;
    }
    if carry != 0 {
        v.push(lo(carry));
    }
}

/// Divide in place by a non-zero small divisor; returns the remainder.
fn divmod_small_in_place(v: &mut Vec<u32>, d: u32) -> u32 {
    let mut rem = 0_u64;
    for limb in v.iter_mut().rev() {
        let cur = (rem << 32) | u64::from(*limb);
        *limb = lo(cur / u64::from(d));
        rem = cur % u64::from(d);
    }
    trim(v);
    lo(rem)
}

/// Knuth TAOCP 4.3.1 Algorithm D. Requires `v.len() >= 2`, `u.len() >= v.len()`, top limb of
/// `v` non-zero. Returns `(quotient, remainder)`, trimmed.
#[allow(clippy::many_single_char_names)] // WHY: Knuth's u, v, q, n, m, s, j notation.
fn divmod_knuth(u: &[u32], v: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let n = v.len();
    let m = u.len() - n;
    let s = v[n - 1].leading_zeros();
    // D1: normalize so the divisor's top bit is set.
    let vn = shl_small(v, s, false);
    let mut un = shl_small(u, s, true);
    let v_top = u64::from(vn[n - 1]);
    let v_second = u64::from(vn[n - 2]);
    let mut q = vec![0_u32; m + 1];
    for j in (0..=m).rev() {
        // D3: estimate the quotient digit from the top two limbs, then correct it with the
        // third (at most two decrements).
        let num = (u64::from(un[j + n]) << 32) | u64::from(un[j + n - 1]);
        let mut qhat = num / v_top;
        let mut rhat = num % v_top;
        while qhat >= BASE || qhat * v_second > ((rhat << 32) | u64::from(un[j + n - 2])) {
            qhat -= 1;
            rhat += v_top;
            if rhat >= BASE {
                break;
            }
        }
        // D4: multiply and subtract qhat * v from the current window of un.
        let mut mul_carry = 0_u64;
        let mut borrow = 0_u64;
        for i in 0..n {
            let p = qhat * u64::from(vn[i]) + mul_carry;
            mul_carry = p >> 32;
            let sub = (p & 0xFFFF_FFFF) + borrow;
            let cur = u64::from(un[i + j]);
            if cur >= sub {
                un[i + j] = lo(cur - sub);
                borrow = 0;
            } else {
                un[i + j] = lo(cur + BASE - sub);
                borrow = 1;
            }
        }
        let sub = mul_carry + borrow;
        let cur = u64::from(un[j + n]);
        let went_negative = cur < sub;
        un[j + n] = if went_negative {
            lo(cur + BASE - sub)
        } else {
            lo(cur - sub)
        };
        // D5/D6: qhat was one too large (probability ~2/BASE): add the divisor back.
        if went_negative {
            qhat -= 1;
            let mut carry = 0_u64;
            for i in 0..n {
                let t = u64::from(un[i + j]) + u64::from(vn[i]) + carry;
                un[i + j] = lo(t);
                carry = t >> 32;
            }
            un[j + n] = lo(u64::from(un[j + n]) + carry);
        }
        q[j] = lo(qhat);
    }
    // D8: denormalize the remainder.
    un.truncate(n);
    let mut r = shr_small(&un, s);
    trim(&mut q);
    trim(&mut r);
    (q, r)
}

/// Left shift by `bits < 32`; `grow` appends the carry-out limb.
fn shl_small(a: &[u32], bits: u32, grow: bool) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len() + 1);
    if bits == 0 {
        out.extend_from_slice(a);
        if grow {
            out.push(0);
        }
        return out;
    }
    let mut carry = 0_u32;
    for &limb in a {
        out.push((limb << bits) | carry);
        carry = limb >> (32 - bits);
    }
    if grow {
        out.push(carry);
    }
    out
}

/// Right shift by `bits < 32`.
fn shr_small(a: &[u32], bits: u32) -> Vec<u32> {
    if bits == 0 {
        return a.to_vec();
    }
    let mut out = vec![0_u32; a.len()];
    for i in 0..a.len() {
        let next = a.get(i + 1).copied().unwrap_or(0);
        out[i] = (a[i] >> bits) | (next << (32 - bits));
    }
    out
}

/// `(quotient, remainder)` of magnitudes; `v` must be non-empty.
fn divmod_mag(u: &[u32], v: &[u32]) -> (Vec<u32>, Vec<u32>) {
    if cmp_mag(u, v) == Ordering::Less {
        return (Vec::new(), u.to_vec());
    }
    if v.len() == 1 {
        let mut q = u.to_vec();
        let r = divmod_small_in_place(&mut q, v[0]);
        let mut rem = vec![r];
        trim(&mut rem);
        return (q, rem);
    }
    divmod_knuth(u, v)
}

fn shl_mag(a: &[u32], n: usize) -> Vec<u32> {
    if a.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0_u32; n / LIMB_BITS];
    #[allow(clippy::cast_possible_truncation)] // WHY: `n % 32 < 32`.
    let bits = (n % LIMB_BITS) as u32;
    out.extend(shl_small(a, bits, true));
    trim(&mut out);
    out
}

fn shr_mag(a: &[u32], n: usize) -> Vec<u32> {
    let limbs = n / LIMB_BITS;
    if limbs >= a.len() {
        return Vec::new();
    }
    #[allow(clippy::cast_possible_truncation)] // WHY: `n % 32 < 32`.
    let bits = (n % LIMB_BITS) as u32;
    let mut out = shr_small(&a[limbs..], bits);
    trim(&mut out);
    out
}

fn bit_len_mag(a: &[u32]) -> u64 {
    match a.last() {
        None => 0,
        Some(top) => {
            let full = (a.len() - 1) as u64 * 32;
            full + u64::from(32 - top.leading_zeros())
        }
    }
}

fn get_bit(a: &[u32], index: usize) -> bool {
    a.get(index / LIMB_BITS)
        .is_some_and(|limb| (limb >> (index % LIMB_BITS)) & 1 == 1)
}

/// Number of trailing zero bits of a non-zero magnitude.
fn trailing_zeros_mag(a: &[u32]) -> u64 {
    let mut zeros = 0_u64;
    for &limb in a {
        if limb == 0 {
            zeros += 32;
        } else {
            return zeros + u64::from(limb.trailing_zeros());
        }
    }
    zeros
}

fn negate_twos(v: &mut [u32]) {
    let mut carry = true;
    for limb in v.iter_mut() {
        let (value, overflow) = (!*limb).overflowing_add(u32::from(carry));
        *limb = value;
        carry = overflow;
    }
}

fn u128_limbs(mut x: u128) -> Vec<u32> {
    let mut v = Vec::with_capacity(4);
    while x != 0 {
        v.push(lo(u64::try_from(x & 0xFFFF_FFFF).unwrap_or(0)));
        x >>= 32;
    }
    v
}

// ---------------------------------------------------------------------------------------------
// construction
// ---------------------------------------------------------------------------------------------

impl Magnus {
    fn from_parts(neg: bool, mut mag: Vec<u32>) -> Self {
        trim(&mut mag);
        let neg = neg && !mag.is_empty();
        Magnus { neg, mag }
    }

    /// Zero. O(1).
    #[must_use]
    pub fn zero() -> Self {
        Magnus::default()
    }

    /// One. O(1).
    #[must_use]
    pub fn one() -> Self {
        Magnus::from(1_u64)
    }

    /// Build from an exact `i64`. O(1).
    #[must_use]
    pub fn from_i64(value: i64) -> Self {
        Magnus::from_parts(value < 0, u128_limbs(u128::from(value.unsigned_abs())))
    }

    /// Build from an exact `u64`. O(1).
    #[must_use]
    pub fn from_u64(value: u64) -> Self {
        Magnus::from_parts(false, u128_limbs(u128::from(value)))
    }

    /// Build from an exact `i128`. O(1).
    #[must_use]
    pub fn from_i128(value: i128) -> Self {
        Magnus::from_parts(value < 0, u128_limbs(value.unsigned_abs()))
    }

    /// Build from an exact `u128`. O(1).
    #[must_use]
    pub fn from_u128(value: u128) -> Self {
        Magnus::from_parts(false, u128_limbs(value))
    }

    /// Truncate toward zero (`-2.9` gives `-2`, `-0.5` gives `0`). NaN and `±∞` fail with
    /// [`MagnusError::NotFinite`]. O(size of the result).
    ///
    /// # Errors
    /// [`MagnusError::NotFinite`] for NaN and infinities.
    pub fn from_f64(value: f64) -> Result<Self, MagnusError> {
        if !value.is_finite() {
            return Err(MagnusError::NotFinite);
        }
        let bits = value.to_bits();
        let negative = bits >> 63 == 1;
        let exponent_field = (bits >> 52) & 0x7FF;
        if exponent_field == 0 {
            return Ok(Magnus::zero()); // zero and subnormals truncate to 0
        }
        let mantissa = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
        let shift = i64::try_from(exponent_field).unwrap_or(0) - 1023 - 52;
        let base = Magnus::from_u64(mantissa);
        let magnitude = if shift >= 0 {
            // `shift <= 971`, so the shift is always representable.
            base.shl(u64::try_from(shift).unwrap_or(0))
                .unwrap_or_default()
        } else {
            base.shr(shift.unsigned_abs())
        };
        Ok(if negative { -magnitude } else { magnitude })
    }

    /// Parse optional `+`/`-` then digits of `radix` (either case; no prefix, no separators,
    /// any length). Same acceptance as `i64::from_str_radix`, unbounded. O(n²) for decimal.
    ///
    /// # Errors
    /// [`MagnusError::Malformed`] for empty text, a bare sign, or a non-digit.
    pub fn parse_radix(text: &str, radix: Radix) -> Result<Self, MagnusError> {
        let (neg, digits) = match text.as_bytes().first() {
            Some(b'-') => (true, &text.as_bytes()[1..]),
            Some(b'+') => (false, &text.as_bytes()[1..]),
            _ => (false, text.as_bytes()),
        };
        if digits.is_empty() {
            return Err(MagnusError::Malformed);
        }
        let base = radix.base();
        // Digits consumed per limb multiply: the largest k with base^k <= u32::MAX.
        let mut per_chunk = 1_usize;
        let mut chunk_mult = u64::from(base);
        while u32::try_from(chunk_mult * u64::from(base)).is_ok() {
            chunk_mult *= u64::from(base);
            per_chunk += 1;
        }
        let mut mag: Vec<u32> = Vec::new();
        for chunk in digits.chunks(per_chunk) {
            let mut value = 0_u32;
            let mut mult = 1_u32;
            for &byte in chunk {
                let digit = char::from(byte)
                    .to_digit(base)
                    .ok_or(MagnusError::Malformed)?;
                value = value * base + digit;
                mult *= base;
            }
            mul_small_add(&mut mag, mult, value);
        }
        Ok(Magnus::from_parts(neg, mag))
    }

    /// Parse decimal text; see [`Magnus::parse_radix`].
    ///
    /// # Errors
    /// [`MagnusError::Malformed`].
    pub fn parse_decimal(text: &str) -> Result<Self, MagnusError> {
        Magnus::parse_radix(text, Radix::Dec)
    }

    /// Render in `radix`: lowercase digits, no prefix, a leading `-` for negatives (the exact
    /// inverse of [`Magnus::parse_radix`]). O(n²) for decimal, O(n) for radix 2/8/16.
    #[must_use]
    pub fn to_string_radix(&self, radix: Radix) -> String {
        let digits = self.digits_radix(radix);
        if self.neg {
            format!("-{digits}")
        } else {
            digits
        }
    }

    fn digits_radix(&self, radix: Radix) -> String {
        if self.mag.is_empty() {
            return "0".to_owned();
        }
        if radix == Radix::Dec {
            // Peel nine decimal digits at a time.
            let mut rest = self.mag.clone();
            let mut chunks: Vec<u32> = Vec::new();
            while !rest.is_empty() {
                chunks.push(divmod_small_in_place(&mut rest, 1_000_000_000));
            }
            let mut out = String::new();
            for (i, chunk) in chunks.iter().rev().enumerate() {
                let digits = chunk.to_string();
                if i > 0 {
                    out.push_str(&"0".repeat(9 - digits.len()));
                }
                out.push_str(&digits);
            }
            return out;
        }
        let bits_per_digit = match radix {
            Radix::Bin => 1,
            Radix::Oct => 3,
            _ => 4,
        };
        let total_bits = usize::try_from(bit_len_mag(&self.mag)).unwrap_or(0);
        let digit_count = total_bits.div_ceil(bits_per_digit);
        let mut out = String::with_capacity(digit_count);
        for d in (0..digit_count).rev() {
            let mut value = 0_u32;
            for b in (0..bits_per_digit).rev() {
                value = (value << 1) | u32::from(get_bit(&self.mag, d * bits_per_digit + b));
            }
            out.push(char::from_digit(value, 16).unwrap_or('0'));
        }
        out
    }
}

impl From<i64> for Magnus {
    fn from(value: i64) -> Self {
        Magnus::from_i64(value)
    }
}
impl From<u64> for Magnus {
    fn from(value: u64) -> Self {
        Magnus::from_u64(value)
    }
}
impl From<i128> for Magnus {
    fn from(value: i128) -> Self {
        Magnus::from_i128(value)
    }
}
impl From<u128> for Magnus {
    fn from(value: u128) -> Self {
        Magnus::from_u128(value)
    }
}
impl From<i32> for Magnus {
    fn from(value: i32) -> Self {
        Magnus::from_i64(i64::from(value))
    }
}
impl From<u32> for Magnus {
    fn from(value: u32) -> Self {
        Magnus::from_u64(u64::from(value))
    }
}

impl std::str::FromStr for Magnus {
    type Err = MagnusError;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Magnus::parse_decimal(text)
    }
}

impl fmt::Display for Magnus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad_integral(!self.neg, "", &self.digits_radix(Radix::Dec))
    }
}

impl fmt::Debug for Magnus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Magnus({self})")
    }
}

// ---------------------------------------------------------------------------------------------
// inspection and narrowing
// ---------------------------------------------------------------------------------------------

impl Magnus {
    /// True for zero. O(1).
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.mag.is_empty()
    }

    /// True for values below zero. O(1).
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.neg
    }

    /// True for odd values (floor semantics: `-3` is odd). O(1).
    #[must_use]
    pub fn is_odd(&self) -> bool {
        self.mag.first().is_some_and(|limb| limb & 1 == 1)
    }

    /// `-1`, `0` or `1`. O(1).
    #[must_use]
    pub fn signum(&self) -> i8 {
        if self.mag.is_empty() {
            0
        } else if self.neg {
            -1
        } else {
            1
        }
    }

    /// Bits in the magnitude: `0` for zero, `k + 1` for `2^k`; `-2^k` also reports `k + 1`.
    /// O(1).
    #[must_use]
    pub fn bit_len(&self) -> u64 {
        bit_len_mag(&self.mag)
    }

    /// True when `bit_len()` is above `ceiling_bits` (ruling 45 post-check for `+ -`). O(1).
    #[must_use]
    pub fn exceeds_ceiling(&self, ceiling_bits: u64) -> bool {
        self.bit_len() > ceiling_bits
    }

    /// Absolute value. O(n).
    #[must_use]
    pub fn abs(&self) -> Self {
        Magnus {
            neg: false,
            mag: self.mag.clone(),
        }
    }

    fn mag_u128(&self) -> Option<u128> {
        if self.mag.len() > 4 {
            return None;
        }
        Some(
            self.mag
                .iter()
                .rev()
                .fold(0_u128, |acc, &limb| (acc << 32) | u128::from(limb)),
        )
    }

    /// Exact `u128`, or `None` when out of range. O(1).
    #[must_use]
    pub fn to_u128(&self) -> Option<u128> {
        if self.neg { None } else { self.mag_u128() }
    }

    /// Exact `i128`, or `None` when out of range. O(1).
    #[must_use]
    pub fn to_i128(&self) -> Option<i128> {
        let magnitude = self.mag_u128()?;
        if self.neg {
            if magnitude <= 1_u128 << 127 {
                Some(0_i128.wrapping_sub_unsigned(magnitude))
            } else {
                None
            }
        } else {
            i128::try_from(magnitude).ok()
        }
    }

    /// Exact `u64`, or `None` when out of range. O(1).
    #[must_use]
    pub fn to_u64(&self) -> Option<u64> {
        self.to_u128().and_then(|v| u64::try_from(v).ok())
    }

    /// Exact `i64`, or `None` when out of range. O(1).
    #[must_use]
    pub fn to_i64(&self) -> Option<i64> {
        self.to_i128().and_then(|v| i64::try_from(v).ok())
    }

    /// The low 128 bits of the infinite two's-complement form (`x mod 2^128`), the basis of
    /// `↦ wrapping<W>` (ruling 24): truncate the result to `W` bits. O(1).
    #[must_use]
    pub fn low_u128_wrapping(&self) -> u128 {
        let mut low = 0_u128;
        for &limb in self.mag.iter().take(4).rev() {
            low = (low << 32) | u128::from(limb);
        }
        if self.neg { low.wrapping_neg() } else { low }
    }

    /// Nearest `f64`, ties to even; `±∞` when the magnitude rounds to at least `2^1024`
    /// (ruling 40: the IEEE default, never a trap). O(n).
    #[must_use]
    pub fn to_f64(&self) -> f64 {
        let len = self.bit_len();
        let magnitude = if len <= 64 {
            // `u64 as f64` rounds to nearest, ties to even.
            #[allow(clippy::cast_precision_loss)]
            let value = self
                .mag_u128()
                .map_or(0.0, |m| u64::try_from(m).unwrap_or(0) as f64);
            value
        } else if len > 1024 {
            f64::INFINITY
        } else {
            let drop = usize::try_from(len - 64).unwrap_or(0);
            let top_limbs = shr_mag(&self.mag, drop);
            let mut top = (u64::from(top_limbs.get(1).copied().unwrap_or(0)) << 32)
                | u64::from(top_limbs.first().copied().unwrap_or(0));
            // Sticky bit: anything dropped below the 64 kept bits forces the tie-break to
            // round up; the kept top bit guarantees bit 0 lies below the rounding position.
            if trailing_zeros_mag(&self.mag) < len - 64 {
                top |= 1;
            }
            #[allow(clippy::cast_precision_loss)]
            let scaled = top as f64;
            // `len <= 1024`, so the scale is a finite power of two; the product is exact
            // unless rounding carried to 2^1024, which correctly becomes infinity.
            scaled * f64::from_bits((1023 + (len - 64)) << 52)
        };
        if self.neg { -magnitude } else { magnitude }
    }

    /// Minimal two's-complement bytes, most significant first (Java `toByteArray`: zero is
    /// `[0]`; a positive value whose top bit is set gets a leading `0`). O(n).
    #[must_use]
    pub fn to_bytes_be(&self) -> Vec<u8> {
        let mut bytes = self.to_bytes_le();
        bytes.reverse();
        bytes
    }

    /// Minimal two's-complement bytes, least significant first; see [`Magnus::to_bytes_be`].
    /// O(n).
    #[must_use]
    pub fn to_bytes_le(&self) -> Vec<u8> {
        let limbs = self.to_twos(self.mag.len() + 1);
        let mut bytes: Vec<u8> = limbs.iter().flat_map(|limb| limb.to_le_bytes()).collect();
        while bytes.len() > 1 {
            let top = bytes[bytes.len() - 1];
            let next_high_bit = bytes[bytes.len() - 2] & 0x80 != 0;
            let redundant = (top == 0x00 && !next_high_bit) || (top == 0xFF && next_high_bit);
            if !redundant {
                break;
            }
            bytes.pop();
        }
        bytes
    }

    /// Decode two's-complement bytes (any length, most significant first); the exact inverse
    /// of [`Magnus::to_bytes_be`]. Empty input is zero. O(n).
    #[must_use]
    pub fn from_bytes_be(bytes: &[u8]) -> Self {
        let mut le = bytes.to_vec();
        le.reverse();
        Magnus::from_bytes_le(&le)
    }

    /// Decode two's-complement bytes (any length, least significant first). O(n).
    #[must_use]
    pub fn from_bytes_le(bytes: &[u8]) -> Self {
        let Some(&top) = bytes.last() else {
            return Magnus::zero();
        };
        let fill = if top & 0x80 != 0 { 0xFF } else { 0x00 };
        let mut padded = bytes.to_vec();
        while !padded.len().is_multiple_of(4) {
            padded.push(fill);
        }
        let (chunks, _) = padded.as_chunks::<4>();
        let limbs = chunks.iter().map(|c| u32::from_le_bytes(*c)).collect();
        Magnus::from_twos(limbs)
    }

    /// Two's-complement limbs of exactly `len` limbs (`len > mag.len()`).
    fn to_twos(&self, len: usize) -> Vec<u32> {
        let mut v = self.mag.clone();
        v.resize(len, 0);
        if self.neg {
            negate_twos(&mut v);
        }
        v
    }

    /// Inverse of `to_twos`: the top bit of the top limb is the sign.
    fn from_twos(mut limbs: Vec<u32>) -> Self {
        let neg = limbs.last().is_some_and(|top| top >> 31 == 1);
        if neg {
            negate_twos(&mut limbs);
        }
        Magnus::from_parts(neg, limbs)
    }
}

// ---------------------------------------------------------------------------------------------
// arithmetic
// ---------------------------------------------------------------------------------------------

impl Magnus {
    fn add_ref(&self, rhs: &Magnus) -> Magnus {
        if self.neg == rhs.neg {
            return Magnus::from_parts(self.neg, add_mag(&self.mag, &rhs.mag));
        }
        match cmp_mag(&self.mag, &rhs.mag) {
            Ordering::Equal => Magnus::zero(),
            Ordering::Greater => Magnus::from_parts(self.neg, sub_mag(&self.mag, &rhs.mag)),
            Ordering::Less => Magnus::from_parts(rhs.neg, sub_mag(&rhs.mag, &self.mag)),
        }
    }

    fn neg_ref(&self) -> Magnus {
        Magnus::from_parts(!self.neg, self.mag.clone())
    }

    fn mul_ref(&self, rhs: &Magnus) -> Magnus {
        Magnus::from_parts(self.neg != rhs.neg, mul_mag(&self.mag, &rhs.mag))
    }

    /// `self * rhs`, failing before any allocation when even the smallest possible product
    /// would be longer than `ceiling_bits` (ruling 45), and after when it is. O(n·m).
    ///
    /// # Errors
    /// [`MagnusError::CeilingExceeded`].
    pub fn mul_within(&self, rhs: &Magnus, ceiling_bits: u64) -> Result<Magnus, MagnusError> {
        if !self.is_zero() && !rhs.is_zero() {
            let least = self
                .bit_len()
                .saturating_add(rhs.bit_len())
                .saturating_sub(1);
            if least > ceiling_bits {
                return Err(MagnusError::CeilingExceeded);
            }
        }
        let product = self.mul_ref(rhs);
        if product.exceeds_ceiling(ceiling_bits) {
            return Err(MagnusError::CeilingExceeded);
        }
        Ok(product)
    }

    /// Floor quotient and floor remainder together: `self == rhs * q + r` and `r` takes the
    /// sign of `rhs` (or is zero). Knuth Algorithm D, O(n·m).
    ///
    /// # Errors
    /// [`MagnusError::DivisionByZero`].
    pub fn div_mod_floor(&self, rhs: &Magnus) -> Result<(Magnus, Magnus), MagnusError> {
        if rhs.is_zero() {
            return Err(MagnusError::DivisionByZero);
        }
        let (q, r) = divmod_mag(&self.mag, &rhs.mag);
        let same_sign = self.neg == rhs.neg;
        if r.is_empty() {
            return Ok((Magnus::from_parts(!same_sign, q), Magnus::zero()));
        }
        if same_sign {
            return Ok((Magnus::from_parts(false, q), Magnus::from_parts(rhs.neg, r)));
        }
        // Opposite signs with a remainder: truncation rounded toward zero, floor goes one
        // further down, and the remainder moves to the far side of zero.
        let q = add_mag(&q, &[1]);
        let r = sub_mag(&rhs.mag, &r);
        Ok((Magnus::from_parts(true, q), Magnus::from_parts(rhs.neg, r)))
    }

    /// Floor quotient (`/`). O(n·m).
    ///
    /// # Errors
    /// [`MagnusError::DivisionByZero`].
    pub fn div_floor(&self, rhs: &Magnus) -> Result<Magnus, MagnusError> {
        self.div_mod_floor(rhs).map(|(q, _)| q)
    }

    /// Floor remainder (`%`), sign of the divisor. O(n·m).
    ///
    /// # Errors
    /// [`MagnusError::DivisionByZero`].
    pub fn mod_floor(&self, rhs: &Magnus) -> Result<Magnus, MagnusError> {
        self.div_mod_floor(rhs).map(|(_, r)| r)
    }

    /// `potentia`: exact `self^exp` by left-to-right squaring, O(M(n·e) · log e). `0^0 == 1`.
    ///
    /// # Errors
    /// [`MagnusError::NegativeExponent`] for `exp < 0` (ruling 28);
    /// [`MagnusError::Unrepresentable`] when `|self| >= 2` and `exp` does not fit a `u64`.
    pub fn potentia(&self, exp: &Magnus) -> Result<Magnus, MagnusError> {
        self.potentia_within(exp, u64::MAX)
    }

    /// `potentia` with a result bit-length ceiling (ruling 45): fails before computing when
    /// the smallest possible result already exceeds `ceiling_bits`.
    ///
    /// # Errors
    /// As [`Magnus::potentia`], plus [`MagnusError::CeilingExceeded`].
    pub fn potentia_within(&self, exp: &Magnus, ceiling_bits: u64) -> Result<Magnus, MagnusError> {
        if exp.neg {
            return Err(MagnusError::NegativeExponent);
        }
        if exp.is_zero() {
            return Ok(Magnus::one());
        }
        let small_base = self.is_zero() || self.mag == [1];
        let Some(count) = exp.to_u64() else {
            // The exponent exceeds 2^64: only 0, 1 and -1 have a representable power.
            if !small_base {
                return Err(MagnusError::Unrepresentable);
            }
            return Ok(if self.neg && exp.is_odd() {
                self.clone()
            } else {
                self.abs()
            });
        };
        if small_base {
            return Ok(if self.neg && count % 2 == 1 {
                self.clone()
            } else {
                self.abs()
            });
        }
        // |self| >= 2: the result has at least (bit_len - 1) * count + 1 bits.
        let least = (self.bit_len() - 1).saturating_mul(count).saturating_add(1);
        if least > ceiling_bits {
            return Err(MagnusError::CeilingExceeded);
        }
        let mut result = Magnus::one();
        for shift in (0..64 - count.leading_zeros()).rev() {
            result = result.mul_ref(&result);
            if (count >> shift) & 1 == 1 {
                result = result.mul_ref(self);
            }
        }
        if result.exceeds_ceiling(ceiling_bits) {
            return Err(MagnusError::CeilingExceeded);
        }
        Ok(result)
    }

    /// `self ⇐ count`: `self * 2^count`, uncapped. O(n + count/32).
    ///
    /// # Errors
    /// [`MagnusError::Unrepresentable`] when the result could not be addressed on this
    /// machine (a non-zero value shifted by more than `usize::MAX` bits).
    pub fn shl(&self, count: u64) -> Result<Magnus, MagnusError> {
        if self.is_zero() {
            return Ok(Magnus::zero());
        }
        let count = usize::try_from(count).map_err(|_| MagnusError::Unrepresentable)?;
        if count
            .checked_add(self.mag.len() * LIMB_BITS + LIMB_BITS)
            .is_none()
        {
            return Err(MagnusError::Unrepresentable);
        }
        Ok(Magnus::from_parts(self.neg, shl_mag(&self.mag, count)))
    }

    /// `shl` with a result bit-length ceiling (ruling 45), checked before allocating. O(n).
    ///
    /// # Errors
    /// [`MagnusError::CeilingExceeded`], [`MagnusError::Unrepresentable`].
    pub fn shl_within(&self, count: u64, ceiling_bits: u64) -> Result<Magnus, MagnusError> {
        if !self.is_zero() && self.bit_len().saturating_add(count) > ceiling_bits {
            return Err(MagnusError::CeilingExceeded);
        }
        self.shl(count)
    }

    /// `self ⇒ count`: the arithmetic shift `floor(self / 2^count)`; counts of any size are
    /// fine (the result saturates at `0` or `-1`). O(n).
    #[must_use]
    pub fn shr(&self, count: u64) -> Magnus {
        // A count beyond the address space is beyond every limb too.
        let count = usize::try_from(count).unwrap_or(usize::MAX);
        if !self.neg {
            return Magnus::from_parts(false, shr_mag(&self.mag, count));
        }
        // floor(-m / 2^n) == -(((m - 1) >> n) + 1)
        let shifted = shr_mag(&sub_mag(&self.mag, &[1]), count);
        Magnus::from_parts(true, add_mag(&shifted, &[1]))
    }

    /// Shift left by a `Magnus` count.
    ///
    /// # Errors
    /// [`MagnusError::NegativeShift`], and as [`Magnus::shl`]; a count above `u64` with a
    /// non-zero value is [`MagnusError::Unrepresentable`].
    pub fn shl_by(&self, count: &Magnus) -> Result<Magnus, MagnusError> {
        if count.neg {
            return Err(MagnusError::NegativeShift);
        }
        if self.is_zero() {
            return Ok(Magnus::zero());
        }
        self.shl(count.to_u64().ok_or(MagnusError::Unrepresentable)?)
    }

    /// Shift right by a `Magnus` count.
    ///
    /// # Errors
    /// [`MagnusError::NegativeShift`].
    pub fn shr_by(&self, count: &Magnus) -> Result<Magnus, MagnusError> {
        if count.neg {
            return Err(MagnusError::NegativeShift);
        }
        Ok(self.shr(count.to_u64().unwrap_or(u64::MAX)))
    }

    fn bitwise(&self, rhs: &Magnus, op: fn(u32, u32) -> u32) -> Magnus {
        let len = self.mag.len().max(rhs.mag.len()) + 1;
        let a = self.to_twos(len);
        let b = rhs.to_twos(len);
        Magnus::from_twos(a.iter().zip(&b).map(|(&x, &y)| op(x, y)).collect())
    }

    /// Bitwise AND on infinite two's complement. O(n).
    #[must_use]
    pub fn bit_and(&self, rhs: &Magnus) -> Magnus {
        self.bitwise(rhs, |x, y| x & y)
    }

    /// Bitwise OR on infinite two's complement. O(n).
    #[must_use]
    pub fn bit_or(&self, rhs: &Magnus) -> Magnus {
        self.bitwise(rhs, |x, y| x | y)
    }

    /// Bitwise XOR on infinite two's complement. O(n).
    #[must_use]
    pub fn bit_xor(&self, rhs: &Magnus) -> Magnus {
        self.bitwise(rhs, |x, y| x ^ y)
    }

    /// Bitwise NOT: `-self - 1`. O(n).
    #[must_use]
    pub fn bit_not(&self) -> Magnus {
        self.neg_ref().add_ref(&Magnus::from_parts(true, vec![1]))
    }
}

macro_rules! forward_binop {
    ($trait:ident, $method:ident, |$a:ident, $b:ident| $body:expr) => {
        impl std::ops::$trait<&Magnus> for &Magnus {
            type Output = Magnus;
            fn $method(self, rhs: &Magnus) -> Magnus {
                let ($a, $b) = (self, rhs);
                $body
            }
        }
        impl std::ops::$trait<Magnus> for Magnus {
            type Output = Magnus;
            fn $method(self, rhs: Magnus) -> Magnus {
                std::ops::$trait::$method(&self, &rhs)
            }
        }
        impl std::ops::$trait<&Magnus> for Magnus {
            type Output = Magnus;
            fn $method(self, rhs: &Magnus) -> Magnus {
                std::ops::$trait::$method(&self, rhs)
            }
        }
        impl std::ops::$trait<Magnus> for &Magnus {
            type Output = Magnus;
            fn $method(self, rhs: Magnus) -> Magnus {
                std::ops::$trait::$method(self, &rhs)
            }
        }
    };
}

// `+` and `-` are O(n); `*` is O(n·m) and unbounded (use `mul_within` under a ceiling).
forward_binop!(Add, add, |a, b| a.add_ref(b));
forward_binop!(Sub, sub, |a, b| a.add_ref(&b.neg_ref()));
forward_binop!(Mul, mul, |a, b| a.mul_ref(b));

impl std::ops::Neg for &Magnus {
    type Output = Magnus;
    fn neg(self) -> Magnus {
        self.neg_ref()
    }
}

impl std::ops::Neg for Magnus {
    type Output = Magnus;
    fn neg(self) -> Magnus {
        Magnus::from_parts(!self.neg, self.mag)
    }
}

impl Ord for Magnus {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.neg, other.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => cmp_mag(&self.mag, &other.mag),
            (true, true) => cmp_mag(&other.mag, &self.mag),
        }
    }
}

impl PartialOrd for Magnus {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
#[path = "magnus_test.rs"]
mod tests;
