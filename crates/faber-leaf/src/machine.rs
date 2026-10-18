//! Faber is one thread: a host call blocks the program until it returns.
//! Thread-cancel recovery is the bug we are not copying. The read runs on a
//! worker thread, the call waits at most `deadline_ms`, and a straggler is
//! never cancelled: it keeps running and lands in the previous slot when it
//! finishes.
//!
//! Serves Latin `machina:specimen`: one host snapshot. This cut reports
//! memory; per-core CPU and the process table land in c1de1950-2 and -3.
//! A deadline of 0 means never wait: answer from the previous slot without
//! spawning a read.

#[cfg(test)]
#[path = "machine_test.rs"]
mod tests;

use crate::error::LeafError;
use crate::value::{Arg, Reply};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const ROUTE: &str = "machina:specimen";

/// Route names this leaf serves. Must match the `serve` match arms below.
const ROUTES: &[&str] = &["machina:specimen"];

/// Process-table cap for one snapshot; the sampler keeps the busiest rows.
pub const MAX_PROCS: usize = 64;

/// One core's absolute CPU tick counters from one host sample.
// TARGET: c1de1950-2 differences these into `IntervalCore` rows.
#[allow(dead_code)]
struct CoreTicks {
    name: String,
    busy_ns: u64,
    total_ns: u64,
}

/// One process row's absolute CPU and resident-memory figures.
// TARGET: c1de1950-3 differences these into `IntervalProc` rows.
#[allow(dead_code)]
struct ProcTicks {
    pid: i64,
    command: String,
    cpu_ns: u64,
    mem_bytes: u64,
}

/// One worker-thread host read, stamped when it completed.
pub(crate) struct Collected {
    taken_at: Instant,
    cores: Vec<CoreTicks>,
    used_bytes: u64,
    total_bytes: u64,
    procs: Vec<ProcTicks>,
}

/// One core's busy/total nanoseconds across the sampling interval.
// TARGET: c1de1950-2 constructs these.
#[allow(dead_code)]
struct IntervalCore {
    name: String,
    busy_ns: i64,
    total_ns: i64,
}

/// One process row's CPU and memory across the sampling interval.
// TARGET: c1de1950-3 constructs these.
#[allow(dead_code)]
struct IntervalProc {
    pid: i64,
    command: String,
    cpu_ns: i64,
    mem_bytes: i64,
}

/// The rendered snapshot a reply replays, in interval units.
struct Rendered {
    cores: Vec<IntervalCore>,
    used_bytes: i64,
    total_bytes: i64,
    procs: Vec<IntervalProc>,
}

/// The last completed sample: absolutes for differencing, render for replay.
// TARGET: c1de1950-2/-3 read the absolute tick vectors.
#[allow(dead_code)]
struct Previous {
    /// Issue order of the read that produced this snapshot. A straggler whose
    /// generation is older than the slot's is rejected on arrival.
    generation: u64,
    taken_at: Instant,
    cores_ticks: Vec<CoreTicks>,
    proc_ticks: Vec<ProcTicks>,
    rendered: Rendered,
}

static PREVIOUS: Mutex<Option<Previous>> = Mutex::new(None);

/// Monotonic issue counter for reads, so slot writes are ordered by issue and
/// not by `taken_at` (which is stamped when a read completes).
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn slot() -> MutexGuard<'static, Option<Previous>> {
    match PREVIOUS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Drop the previous sample. Test isolation and execution boundaries only.
pub fn reset_previous() {
    *slot() = None;
}

/// Whether `route` belongs to this leaf. Pure name lookup; samples nothing.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
}

/// Answer a `machina:*` route. `None` means the route is not this leaf.
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "machina:specimen" => sample(args),
        _ => return None,
    };
    Some(result)
}

fn sample(args: &[Arg]) -> Result<Reply, LeafError> {
    let deadline_ms = match args {
        [Arg::Int(ms)] => *ms,
        [] => return Err(LeafError::invalid_argument(ROUTE, "missing deadline_ms")),
        _ => {
            return Err(LeafError::invalid_argument(
                ROUTE,
                "deadline_ms must be int",
            ));
        }
    };
    if deadline_ms < 0 {
        return Err(LeafError::invalid_argument(
            ROUTE,
            "deadline_ms must be non-negative",
        ));
    }
    // WHY: 0 means "never wait" — no thread is spawned, so the reply is the
    // slot's contents by construction and deadline-0 stays deterministic.
    if deadline_ms == 0 {
        return Ok(previous_reply());
    }
    // WHY: the generation is taken before the read starts, so a straggler that
    // started first can never outrank a snapshot that started later.
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, rx) = std::sync::mpsc::channel::<Result<Collected, LeafError>>();
    std::thread::spawn(move || {
        // WHY: the receiver may already be gone (deadline expired); a failed
        // send drops the late value without cancelling the read.
        let _sent = tx.send(collect());
    });
    let wait = Duration::from_millis(u64::try_from(deadline_ms).unwrap_or(u64::MAX));
    match rx.recv_timeout(wait) {
        Ok(Ok(fresh)) => Ok(store_and_render(generation, fresh)),
        Ok(Err(error)) => Err(error),
        Err(_timeout) => {
            // The earlier read is never cancelled: it keeps running and owns
            // the receiver, so its late value becomes the next previous slot.
            std::thread::spawn(move || {
                if let Ok(Ok(late)) = rx.recv() {
                    store_if_newer(make_previous(generation, late, None));
                }
            });
            Ok(previous_reply())
        }
    }
}

fn store_and_render(generation: u64, fresh: Collected) -> Reply {
    let old = slot().take();
    let rendered = render_intervals(old.as_ref(), &fresh);
    let reply = snapshot_reply(&rendered, false, 0);
    store_if_newer(make_previous(generation, fresh, Some(rendered)));
    reply
}

/// Store `previous` only if it is newer than the slot's current snapshot.
/// Callers have already released their guard (`store_and_render` drops the
/// `slot()` guard of its `take()` at the semicolon), so this never nests.
fn store_if_newer(previous: Previous) {
    let mut guard = slot();
    if guard
        .as_ref()
        .is_none_or(|prev| prev.generation < previous.generation)
    {
        *guard = Some(previous);
    }
}

fn make_previous(generation: u64, collected: Collected, rendered: Option<Rendered>) -> Previous {
    let rendered = rendered.unwrap_or_else(|| render_intervals(None, &collected));
    Previous {
        generation,
        taken_at: collected.taken_at,
        cores_ticks: collected.cores,
        proc_ticks: collected.procs,
        rendered,
    }
}

#[cfg(test)]
pub(crate) fn collected_for_test(used_bytes: u64) -> Collected {
    Collected {
        taken_at: Instant::now(),
        cores: Vec::new(),
        used_bytes,
        total_bytes: 0,
        procs: Vec::new(),
    }
}

fn render_intervals(old: Option<&Previous>, fresh: &Collected) -> Rendered {
    Rendered {
        cores: interval_cores(old, fresh),
        used_bytes: i64::try_from(fresh.used_bytes).unwrap_or(i64::MAX),
        total_bytes: i64::try_from(fresh.total_bytes).unwrap_or(i64::MAX),
        procs: interval_procs(old, fresh),
    }
}

fn interval_cores(old: Option<&Previous>, fresh: &Collected) -> Vec<IntervalCore> {
    // TARGET: c1de1950-2 differences each core by name against the previous
    // slot's absolutes. This unit ships the slot and the rendered shape.
    debug_assert!(old.is_none_or(|previous| previous.taken_at <= fresh.taken_at));
    Vec::new()
}

fn interval_procs(old: Option<&Previous>, fresh: &Collected) -> Vec<IntervalProc> {
    // TARGET: c1de1950-3 differences the process table against the previous
    // slot's absolutes, capped at `MAX_PROCS`.
    debug_assert!(old.is_none_or(|previous| previous.taken_at <= fresh.taken_at));
    Vec::new()
}

fn previous_reply() -> Reply {
    match slot().as_ref() {
        Some(previous) => {
            let age_ms =
                i64::try_from(Instant::now().duration_since(previous.taken_at).as_millis())
                    .unwrap_or(i64::MAX);
            snapshot_reply(&previous.rendered, true, age_ms)
        }
        None => snapshot_reply(
            &Rendered {
                cores: Vec::new(),
                used_bytes: 0,
                total_bytes: 0,
                procs: Vec::new(),
            },
            true,
            0,
        ),
    }
}

fn snapshot_reply(rendered: &Rendered, stale: bool, age_ms: i64) -> Reply {
    Reply::Map(vec![
        ("stale".to_owned(), Reply::Bool(stale)),
        ("age_ms".to_owned(), Reply::Int(age_ms)),
        (
            "cores".to_owned(),
            Reply::List(rendered.cores.iter().map(core_reply).collect()),
        ),
        (
            "memory".to_owned(),
            Reply::Map(vec![
                ("used_bytes".to_owned(), Reply::Int(rendered.used_bytes)),
                ("total_bytes".to_owned(), Reply::Int(rendered.total_bytes)),
            ]),
        ),
        (
            "procs".to_owned(),
            Reply::List(rendered.procs.iter().map(proc_reply).collect()),
        ),
    ])
}

fn core_reply(core: &IntervalCore) -> Reply {
    Reply::Map(vec![
        ("name".to_owned(), Reply::Str(core.name.clone())),
        ("busy_ns".to_owned(), Reply::Int(core.busy_ns)),
        ("total_ns".to_owned(), Reply::Int(core.total_ns)),
    ])
}

fn proc_reply(proc_row: &IntervalProc) -> Reply {
    Reply::Map(vec![
        ("pid".to_owned(), Reply::Int(proc_row.pid)),
        ("command".to_owned(), Reply::Str(proc_row.command.clone())),
        ("cpu_ns".to_owned(), Reply::Int(proc_row.cpu_ns)),
        ("mem_bytes".to_owned(), Reply::Int(proc_row.mem_bytes)),
    ])
}

#[cfg(target_os = "macos")]
fn collect() -> Result<Collected, LeafError> {
    let total_bytes = total_memory()?;
    let used_bytes = used_memory()?;
    Ok(Collected {
        taken_at: Instant::now(),
        cores: Vec::new(),
        used_bytes,
        total_bytes,
        procs: Vec::new(),
    })
}

#[cfg(target_os = "macos")]
fn total_memory() -> Result<u64, LeafError> {
    let mut mib = [libc::CTL_HW, libc::HW_MEMSIZE];
    let mut total: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: `mib` names the HW_MEMSIZE query; the kernel writes one u64
    // into `total`, and `size` is that buffer's size in bytes.
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            std::ptr::from_mut(&mut total).cast(),
            std::ptr::from_mut(&mut size),
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return Err(LeafError::unavailable(
            ROUTE,
            format!(
                "sysctl HW_MEMSIZE failed: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    Ok(total)
}

// WHY: libc omits `host_page_size`; the Mach declaration is stable and the
// symbol lives in libSystem, which every macOS binary links.
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn host_page_size(
        host_priv: libc::host_t,
        out_page_size: *mut libc::vm_size_t,
    ) -> libc::kern_return_t;
}

// WHY: libc deprecates `mach_host_self` in favour of the `mach2` crate; adding
// a dependency is outside this leaf's cut, and the call is the same Mach port.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn used_memory() -> Result<u64, LeafError> {
    let mut stats = std::mem::MaybeUninit::<libc::vm_statistics64_data_t>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: `HOST_VM_INFO64` fixes the struct type; the kernel writes a
    // `vm_statistics64_data_t` into `stats` and its word count into `count`.
    let status = unsafe {
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            stats.as_mut_ptr().cast(),
            std::ptr::from_mut(&mut count),
        )
    };
    if status != libc::KERN_SUCCESS {
        return Err(LeafError::unavailable(
            ROUTE,
            format!("host_statistics64 failed ({status})"),
        ));
    }
    // SAFETY: the call above returned KERN_SUCCESS, so the whole
    // `vm_statistics64_data_t` was written.
    let stats = unsafe { stats.assume_init() };
    let mut page_size: libc::vm_size_t = 0;
    // SAFETY: `host_page_size` writes one `vm_size_t` into `page_size`.
    let status =
        unsafe { host_page_size(libc::mach_host_self(), std::ptr::from_mut(&mut page_size)) };
    if status != libc::KERN_SUCCESS {
        return Err(LeafError::unavailable(
            ROUTE,
            format!("host_page_size failed ({status})"),
        ));
    }
    // Used = wired (unswappable) + active + compressed. Inactive, free, and
    // speculative pages count as available, not used.
    let pages = u64::from(stats.active_count)
        + u64::from(stats.wire_count)
        + u64::from(stats.compressor_page_count);
    Ok(pages.saturating_mul(u64::try_from(page_size).unwrap_or(u64::MAX)))
}

#[cfg(not(target_os = "macos"))]
fn collect() -> Result<Collected, LeafError> {
    Err(LeafError::unavailable(
        ROUTE,
        "host snapshot leaf is macOS-only in this cut",
    ))
}
