//! Blocking clock leaf. Serves Latin `tempus:*` routes.
//!
//! Civil datetime is pure Faber. `interval` stays N4b (cursor return).
//! `tempus:activum` is process CPU time in nanoseconds: on Unix hosts
//! `clock_gettime(CLOCK_PROCESS_CPUTIME_ID)`. A host without that clock
//! returns 0; that is not a wall-clock substitute.

#[cfg(test)]
#[path = "clock_test.rs"]
mod tests;

use crate::error::LeafError;
use crate::value::{Arg, Reply};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static MONOTONIC_ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Route names this leaf serves. Must match the `serve` match arms below.
pub(crate) const ROUTES: &[&str] = &[
    "tempus:nunc",
    "tempus:monotonicum",
    "tempus:activum",
    "tempus:dormiet",
];

/// Whether `route` belongs to this leaf. Pure name lookup; reads no clock.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
}

/// Answer a `tempus:*` route. `None` means the route is not this leaf.
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "tempus:nunc" => now(args),
        "tempus:monotonicum" => monotonic(args),
        "tempus:activum" => active(args),
        "tempus:dormiet" => sleep(args, &|| false),
        _ => return None,
    };
    Some(result)
}

/// One-shot sleep that can abort when `is_cancelled` becomes true.
pub fn sleep_interruptible(
    args: &[Arg],
    is_cancelled: &dyn Fn() -> bool,
) -> Result<Reply, LeafError> {
    sleep(args, is_cancelled)
}

fn expect_int(
    route: &'static str,
    args: &[Arg],
    index: usize,
    name: &str,
) -> Result<i64, LeafError> {
    match args.get(index) {
        Some(Arg::Int(value)) => Ok(*value),
        Some(_) => Err(LeafError::invalid_argument(
            route,
            format!("{name} must be numerus"),
        )),
        None => Err(LeafError::invalid_argument(
            route,
            format!("missing {name}"),
        )),
    }
}

fn empty_ok(route: &'static str, args: &[Arg]) -> Result<(), LeafError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(LeafError::invalid_argument(route, "unexpected arguments"))
    }
}

fn now(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "tempus:nunc";
    empty_ok(ROUTE, args)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    Ok(Reply::Int(nanos))
}

fn monotonic(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "tempus:monotonicum";
    empty_ok(ROUTE, args)?;
    let origin = MONOTONIC_ORIGIN.get_or_init(Instant::now);
    let nanos = i64::try_from(origin.elapsed().as_nanos()).unwrap_or(i64::MAX);
    Ok(Reply::Int(nanos))
}

fn active(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "tempus:activum";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Int(process_cpu_nanos()))
}

fn process_cpu_nanos() -> i64 {
    #[cfg(unix)]
    {
        unix_process_cpu_nanos().unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        0
    }
}

// `time_t` and `c_long` are 32-bit on some Unix hosts, so the widening is
// real there and a no-op on 64-bit ones.
#[cfg(unix)]
#[allow(clippy::useless_conversion)]
fn unix_process_cpu_nanos() -> Option<i64> {
    let mut spec = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `spec` is a valid, writable `timespec` for the whole call, and
    // `clock_gettime` writes nothing else.
    let status = unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &raw mut spec) };
    if status != 0 {
        return None;
    }
    let seconds = i64::from(spec.tv_sec);
    let nanos = i64::from(spec.tv_nsec);
    Some(seconds.saturating_mul(1_000_000_000).saturating_add(nanos))
}

fn sleep(args: &[Arg], is_cancelled: &dyn Fn() -> bool) -> Result<Reply, LeafError> {
    const ROUTE: &str = "tempus:dormiet";
    let ms = expect_int(ROUTE, args, 0, "ms")?;
    if ms < 0 {
        return Err(LeafError::invalid_argument(
            ROUTE,
            "ms must be non-negative",
        ));
    }
    if ms == 0 {
        return Ok(Reply::Unit);
    }
    let deadline = Instant::now() + Duration::from_millis(u64::try_from(ms).unwrap_or(u64::MAX));
    while Instant::now() < deadline {
        if is_cancelled() {
            return Err(LeafError::cancelled(ROUTE));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        thread::sleep(remaining.min(Duration::from_millis(5)));
    }
    if is_cancelled() {
        return Err(LeafError::cancelled(ROUTE));
    }
    Ok(Reply::Unit)
}
