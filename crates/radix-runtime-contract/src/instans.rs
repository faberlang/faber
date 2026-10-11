//! Radix-owned `instans` runtime — absolute point-in-time with precision
//! contract.
//!
//! Mirrors `faber::instans`. Radix is the authority; the runtime copy
//! is expected to become a generated/committed artifact in a later phase. All
//! precision variants store i64 nanoseconds since Unix epoch; the precision
//! parameter declares how many trailing digits are meaningful
//! (significant-figures semantics), not a different storage width.
//!
//! Unlike the runtime original, this copy carries no `Valor` coupling: the
//! runner extracts instans values from concrete `Value` via [`Instans::from_rfc3339`]
//! and [`Instans::from_epoch_integer`].

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_MILLI: i64 = 1_000_000;
const NANOS_PER_MICRO: i64 = 1_000;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3_600;
const NANOS_PER_DAY: i64 = SECONDS_PER_HOUR * 24 * NANOS_PER_SECOND;

/// Declared precision (`praecisio`) for an [`Instans`] value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstansPraecisio {
    /// Bare `instans` — second granularity (Unix time default).
    Secunda,
    /// Millisecond granularity (`instans<ms>`).
    Millisecunda,
    /// Microsecond granularity (`instans<µs>`).
    Microsecunda,
    /// Nanosecond granularity (`instans<ns>`).
    Nanosecunda,
}

impl InstansPraecisio {
    /// Coarsest (`Secunda`) to finest (`Nanosecunda`) rank for cross-precision comparison.
    fn rank(self) -> u8 {
        match self {
            Self::Secunda => 0,
            Self::Millisecunda => 1,
            Self::Microsecunda => 2,
            Self::Nanosecunda => 3,
        }
    }

    /// Nanosecond modulus for this precision (finer bits are zeroed).
    #[must_use]
    pub fn modulus_nanos(self) -> i64 {
        match self {
            Self::Secunda => NANOS_PER_SECOND,
            Self::Millisecunda => NANOS_PER_MILLI,
            Self::Microsecunda => NANOS_PER_MICRO,
            Self::Nanosecunda => 1,
        }
    }

    /// Zero sub-precision bits per the contract.
    ///
    /// WHY: Euclidean bucket arithmetic — pre-epoch values must truncate toward
    /// earlier buckets, not toward zero (which would snap `-1ms` to epoch).
    #[must_use]
    pub fn truncate_nanos(self, nanos: i64) -> i64 {
        let modulus = self.modulus_nanos();
        nanos.div_euclid(modulus) * modulus
    }

    /// RFC3339 fractional-digit count for emit (no fabricated sub-precision digits).
    fn fraction_digits(self) -> usize {
        match self {
            Self::Secunda => 0,
            Self::Millisecunda => 3,
            Self::Microsecunda => 6,
            Self::Nanosecunda => 9,
        }
    }
}

/// Absolute instant stored as nanoseconds since Unix epoch.
///
/// [`PartialEq`] compares at the value's declared precision — two `instans<ms>`
/// operands are equal when their millisecond-truncated nanosecond forms match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instans {
    nanos: i64,
    praecisio: InstansPraecisio,
}

impl Instans {
    #[must_use]
    pub fn from_nanos(nanos: i64, praecisio: InstansPraecisio) -> Self {
        Self {
            nanos: praecisio.truncate_nanos(nanos),
            praecisio,
        }
    }

    #[must_use]
    pub fn from_epoch_seconds(seconds: i64, praecisio: InstansPraecisio) -> Self {
        Self::from_nanos(seconds.saturating_mul(NANOS_PER_SECOND), praecisio)
    }

    #[must_use]
    pub fn from_epoch_millis(millis: i64, praecisio: InstansPraecisio) -> Self {
        Self::from_nanos(millis.saturating_mul(NANOS_PER_MILLI), praecisio)
    }

    #[must_use]
    pub fn from_epoch_micros(micros: i64, praecisio: InstansPraecisio) -> Self {
        Self::from_nanos(micros.saturating_mul(NANOS_PER_MICRO), praecisio)
    }

    /// Parse an RFC3339 wire string into an instant at the requested precision.
    ///
    /// Replaces the runtime's `Instans::try_from_valor` for the
    /// `Valor::Instans(text) | Valor::Textus(text)` arms, without the `Valor`
    /// middleman. Returns `None` on any malformed input.
    #[must_use]
    pub fn from_rfc3339(text: &str, praecisio: InstansPraecisio) -> Option<Self> {
        let nanos = parse_rfc3339(text)?;
        Some(Self::from_nanos(nanos, praecisio))
    }

    /// Build an instant from an epoch integer interpreted in the requested
    /// precision's native units (seconds/ms/µs/ns).
    ///
    /// Replaces the runtime's `Instans::try_from_valor` for the `Valor::Numerus`
    /// arm.
    #[must_use]
    pub fn from_epoch_integer(epoch: i64, praecisio: InstansPraecisio) -> Self {
        match praecisio {
            InstansPraecisio::Secunda => Self::from_epoch_seconds(epoch, praecisio),
            InstansPraecisio::Millisecunda => Self::from_epoch_millis(epoch, praecisio),
            InstansPraecisio::Microsecunda => Self::from_epoch_micros(epoch, praecisio),
            InstansPraecisio::Nanosecunda => Self::from_nanos(epoch, praecisio),
        }
    }

    #[must_use]
    pub fn nanos(&self) -> i64 {
        self.nanos
    }

    #[must_use]
    pub fn praecisio(&self) -> InstansPraecisio {
        self.praecisio
    }

    /// Re-tag at a new precision, applying truncation when narrowing.
    #[must_use]
    pub fn ad_praecisionem(self, praecisio: InstansPraecisio) -> Self {
        Self::from_nanos(self.nanos, praecisio)
    }

    /// Compare two instants at the coarser of their declared precisions.
    #[must_use]
    pub fn partial_cmp_at_coarser(self, other: Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp_at_coarser(other))
    }

    fn cmp_at_coarser(self, other: Self) -> std::cmp::Ordering {
        let praecisio = coarser_praecisio(self.praecisio, other.praecisio);
        let lhs = praecisio.truncate_nanos(self.nanos);
        let rhs = praecisio.truncate_nanos(other.nanos);
        lhs.cmp(&rhs)
    }

    /// Emit RFC3339 UTC (`Z`) at the value's declared precision.
    #[must_use]
    pub fn to_rfc3339(self) -> String {
        format_rfc3339_utc(self.nanos, self.praecisio)
    }
}

impl PartialOrd for Instans {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Instans {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_at_coarser(*other)
    }
}

fn coarser_praecisio(a: InstansPraecisio, b: InstansPraecisio) -> InstansPraecisio {
    if a.rank() <= b.rank() { a } else { b }
}

/// Parse RFC3339 datetimes to nanoseconds since Unix epoch (UTC storage).
///
/// EDGE: accepts `Z` / `±00:00` and numeric offsets `±HH:MM` / `±HHMM`; rejects
/// missing offset and named zones.
fn parse_rfc3339(input: &str) -> Option<i64> {
    let (date, rest) = input.split_once('T').or_else(|| input.split_once('t'))?;
    let (year, month, day) = parse_date(date)?;
    let (hour, minute, second, fraction_nanos, offset_minutes) = parse_time_and_offset(rest)?;
    validate_civil_date(year, month, day)?;
    validate_time_of_day(hour, minute, second)?;

    let local_nanos = civil_datetime_nanos(year, month, day, hour, minute, second, fraction_nanos)?;
    let offset_nanos = nanos_from_offset_minutes(offset_minutes)?;
    local_nanos.checked_sub(offset_nanos)
}

fn civil_datetime_nanos(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    fraction_nanos: i64,
) -> Option<i64> {
    let day_nanos = days_from_civil(year, month, day).checked_mul(NANOS_PER_DAY)?;
    let seconds_of_day = i64::from(hour)
        .checked_mul(SECONDS_PER_HOUR)?
        .checked_add(i64::from(minute).checked_mul(SECONDS_PER_MINUTE)?)?
        .checked_add(i64::from(second))?;
    let time_nanos = seconds_of_day
        .checked_mul(NANOS_PER_SECOND)?
        .checked_add(fraction_nanos)?;
    day_nanos.checked_add(time_nanos)
}

fn nanos_from_offset_minutes(offset_minutes: i32) -> Option<i64> {
    i64::from(offset_minutes)
        .checked_mul(60)?
        .checked_mul(NANOS_PER_SECOND)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
fn format_rfc3339_utc(nanos: i64, praecisio: InstansPraecisio) -> String {
    use std::fmt::Write as _;

    let truncated = praecisio.truncate_nanos(nanos);
    let day_index = truncated.div_euclid(NANOS_PER_DAY);
    let nanos_of_day = truncated.rem_euclid(NANOS_PER_DAY);
    let (year, month, day) = civil_from_days(day_index);
    let secs_of_day = (nanos_of_day / NANOS_PER_SECOND) as u32;
    let nanos_remainder = (nanos_of_day % NANOS_PER_SECOND) as u32;
    let hour = secs_of_day / SECONDS_PER_HOUR as u32;
    let minute = (secs_of_day % SECONDS_PER_HOUR as u32) / SECONDS_PER_MINUTE as u32;
    let second = secs_of_day % SECONDS_PER_MINUTE as u32;

    let mut out = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    let digits = praecisio.fraction_digits();
    if digits > 0 {
        let fraction = nanos_remainder / 10u32.pow(9 - digits as u32);
        out.push('.');
        write!(out, "{fraction:0digits$}").ok();
    }
    out.push('Z');
    out
}

fn parse_date(date: &str) -> Option<(i32, u32, u32)> {
    if date.len() != 10 {
        return None;
    }
    let year: i32 = date.get(0..4)?.parse().ok()?;
    expect_char(date, 4, b'-')?;
    let month: u32 = date.get(5..7)?.parse().ok()?;
    expect_char(date, 7, b'-')?;
    let day: u32 = date.get(8..10)?.parse().ok()?;
    Some((year, month, day))
}

fn parse_time_and_offset(rest: &str) -> Option<(u32, u32, u32, i64, i32)> {
    if rest.len() < 8 {
        return None;
    }
    let hour: u32 = rest.get(0..2)?.parse().ok()?;
    expect_char(rest, 2, b':')?;
    let minute: u32 = rest.get(3..5)?.parse().ok()?;
    expect_char(rest, 5, b':')?;
    let second: u32 = rest.get(6..8)?.parse().ok()?;
    let mut cursor = 8usize;
    let fraction_nanos = if rest.as_bytes().get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while cursor < rest.len() && rest.as_bytes()[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if start == cursor {
            return None;
        }
        let digits = &rest[start..cursor];
        let padded = format!("{digits:0<9}");
        padded.get(0..9)?.parse().ok()?
    } else {
        0
    };
    let offset_minutes = parse_offset(&rest[cursor..])?;
    Some((hour, minute, second, fraction_nanos, offset_minutes))
}

/// Offset east of UTC in whole minutes (`+04:00` → `240`).
fn parse_offset(zone: &str) -> Option<i32> {
    if zone.is_empty() {
        return None;
    }
    match zone {
        "Z" | "z" | "+00:00" | "-00:00" => Some(0),
        _ => parse_numeric_offset(zone),
    }
}

#[allow(clippy::cast_possible_wrap)]
fn parse_numeric_offset(zone: &str) -> Option<i32> {
    let sign = match zone.as_bytes().first()? {
        b'+' => 1i32,
        b'-' => -1i32,
        _ => return None,
    };
    let rest = &zone[1..];
    let minutes = if let Some((hours, mins)) = rest.split_once(':') {
        offset_hhmm_minutes(hours.parse().ok()?, mins.parse().ok()?)?
    } else if rest.len() == 4 {
        offset_hhmm_minutes(rest.get(0..2)?.parse().ok()?, rest.get(2..4)?.parse().ok()?)?
    } else if rest.len() == 2 {
        offset_hhmm_minutes(rest.parse().ok()?, 0)?
    } else {
        return None;
    };
    Some(sign * minutes as i32)
}

fn offset_hhmm_minutes(hour: u32, minute: u32) -> Option<u32> {
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(hour * 60 + minute)
}

fn expect_char(input: &str, index: usize, expected: u8) -> Option<()> {
    match input.as_bytes().get(index) {
        Some(&ch) if ch == expected => Some(()),
        _ => None,
    }
}

fn validate_civil_date(year: i32, month: u32, day: u32) -> Option<()> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let max_day = days_in_month(year, month);
    if !(1..=max_day).contains(&day) {
        return None;
    }
    Some(())
}

fn validate_time_of_day(hour: u32, minute: u32, second: u32) -> Option<()> {
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut days = DAYS[(month - 1) as usize];
    if month == 2 && is_leap_year(year) {
        days += 1;
    }
    days
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days from 1970-01-01 to the given civil date (Howard Hinnant).
#[allow(clippy::cast_possible_wrap)]
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let mut y = year;
    y -= i32::from(month <= 2);
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let month_i = month as i32;
    let day_i = day as i32;
    let doy = (153 * (if month > 2 { month_i - 3 } else { month_i + 9 }) + 2) / 5 + day_i - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era) * 146_097 + i64::from(doe) - 719_468
}

/// Civil date from days since 1970-01-01 (Howard Hinnant).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let mut y = yoe as i32 + (era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    (y, m, d)
}
