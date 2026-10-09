//! Language traps: the one place generated Rust fails.
//!
//! Generated code never writes `.expect(..)`, `.unwrap()` or `panic!(..)`
//! inline. A failing index, a missing key, a bad conversion or a violated
//! assertion goes through this module, so the failure text and the panic site
//! live in the runtime and the generated source reads as plain Faber.
//!
//! A trap is an ordinary unwinding panic. Hosts that isolate a program run
//! (`catch_unwind`) keep working; nothing here exits the process. Every public
//! function is `#[track_caller]`, so the reported location is the generated
//! call site, exactly as with `Option::expect`.
//!
//! The text of each [`Trap`] is the text the generated code used to pass to
//! `.expect(..)` / `panic!(..)`. Failure output is byte-identical:
//! `Option::expect(msg)` prints `msg`, `Result::expect(msg)` prints
//! `msg: {err:?}`, and [`some`] / [`ok`] reproduce those.

use core::fmt;

/// A closed set of language traps. `Display` is the exact failure text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trap {
    /// `lista` index out of range.
    ListaIndex,
    /// `textus` index out of range.
    TextusIndex,
    /// `ascii` index out of range.
    AsciiIndex,
    /// `octeti` index out of range.
    OctetiIndex,
    /// `tabula` lookup of an absent key.
    TabulaKey,
    /// A `nonnull` assertion met `none`.
    Nonnull,
    /// A range step that is not positive.
    RangeStep,
    /// `itera` over sources of unequal length.
    Lockstep,
    /// Elementwise `lista` operation over unequal lengths.
    ListaLength,
    /// A bounded `lista` literal larger than its capacity.
    Capacity,
    /// A failable call that failed where no recovery was declared.
    FailableCall,
    /// A conversion `from` to `to` that failed.
    Conversion(&'static str, &'static str),
    /// A conversion `from` to `to` whose value does not fit.
    OutOfRange(&'static str, &'static str),
    /// Any other fixed message, printed verbatim.
    Other(&'static str),
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trap::ListaIndex => f.write_str("lista index trap"),
            Trap::TextusIndex => f.write_str("textus index trap"),
            Trap::AsciiIndex => f.write_str("ascii index trap"),
            Trap::OctetiIndex => f.write_str("octeti index trap"),
            Trap::TabulaKey => f.write_str("tabula missing key"),
            Trap::Nonnull => f.write_str("nonnull assertion failed"),
            Trap::RangeStep => f.write_str("range step must be positive"),
            Trap::Lockstep => f.write_str("itera lockstep sources must have equal length"),
            Trap::ListaLength => f.write_str("lista elementwise length mismatch"),
            Trap::Capacity => f.write_str("bounded lista literal exceeds capacity"),
            Trap::FailableCall => f.write_str("failable call failed"),
            Trap::Conversion(from, to) => write!(f, "{from} to {to} conversion failed"),
            Trap::OutOfRange(from, to) => write!(f, "{from} to {to} conversion out of range"),
            Trap::Other(text) => f.write_str(text),
        }
    }
}

/// Fail with `msg`: the single `panic!` site for language traps.
///
/// # Panics
///
/// Always.
#[cold]
#[inline(never)]
#[track_caller]
pub fn fail(msg: impl fmt::Display) -> ! {
    panic!("{msg}")
}

/// `Option<T>` to `T`, trapping with `k` on `None`.
///
/// # Panics
///
/// With the text of `k` when `v` is `None`.
#[inline]
#[track_caller]
pub fn some<T>(v: Option<T>, k: Trap) -> T {
    match v {
        Some(value) => value,
        None => fail(k),
    }
}

/// `Result<T, E>` to `T`, trapping with `k` on `Err`. The failure text is
/// `Result::expect`'s: `"{k}: {err:?}"`.
///
/// # Panics
///
/// With `"{k}: {err:?}"` when `r` is `Err`.
#[inline]
#[track_caller]
pub fn ok<T, E: fmt::Debug>(r: Result<T, E>, k: Trap) -> T {
    match r {
        Ok(value) => value,
        Err(err) => fail(format_args!("{k}: {err:?}")),
    }
}

/// `Result<T, &'static str>` to `T`; the failure text is the `Err` text itself
/// (the tensor and sparsa constructors name their own failure).
///
/// # Panics
///
/// With the `Err` text when `r` is `Err`.
#[inline]
#[track_caller]
pub fn ok_static<T>(r: Result<T, &'static str>) -> T {
    match r {
        Ok(value) => value,
        Err(text) => fail(text),
    }
}

/// Trap with `k` unless `cond` holds.
///
/// # Panics
///
/// With the text of `k` when `cond` is false.
#[inline]
#[track_caller]
pub fn require(cond: bool, k: Trap) {
    if !cond {
        fail(k);
    }
}

/// A state the emitter proves impossible. `why` names the broken invariant.
///
/// # Panics
///
/// Always, with `internal error: entered unreachable code: {why}`.
#[cold]
#[inline(never)]
#[track_caller]
pub fn unreachable(why: &'static str) -> ! {
    fail(format_args!(
        "internal error: entered unreachable code: {why}"
    ))
}
