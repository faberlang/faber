//! Blocking clock leaf. Serves Latin `tempus:*` routes.
//!
//! Civil datetime is pure Faber. `interval` stays N4b (cursor return).
//! `tempus:activum` is process CPU time on Linux (`/proc/self/stat` utime+stime
//! at 100 Hz → nanoseconds). Other hosts return 0; that is not a wall-clock
//! substitute.

#[cfg(test)]
#[path = "clock_test.rs"]
mod tests;

use crate::error::LeafError;
use crate::value::{Arg, Reply};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Assumed Linux `CLK_TCK` when converting `/proc/self/stat` ticks to ns.
#[cfg(target_os = "linux")]
const LINUX_CLK_TCK: i64 = 100;

static MONOTONIC_ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Route names this leaf serves. Must match the `serve` match arms below.
const ROUTES: &[&str] = &[
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
    #[cfg(target_os = "linux")]
    {
        linux_process_cpu_nanos().unwrap_or(0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

#[cfg(target_os = "linux")]
fn linux_process_cpu_nanos() -> Option<i64> {
    let text = std::fs::read_to_string("/proc/self/stat").ok()?;
    let after = text.rsplit_once(')')?.1;
    let mut fields = after.split_whitespace();
    // After `)`: ppid … minflt cminflt majflt cmajflt utime stime
    for _ in 0..11 {
        fields.next()?;
    }
    let utime = fields.next()?.parse::<i64>().ok()?;
    let stime = fields.next()?.parse::<i64>().ok()?;
    Some(utime.saturating_add(stime).saturating_mul(1_000_000_000) / LINUX_CLK_TCK)
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
