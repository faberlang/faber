//! Faber is one thread: a host call blocks the program until it returns.
//! Thread-cancel recovery is the bug we are not copying. The read runs on a
//! worker thread, the call waits at most `deadline_ms`, and a straggler is
//! never cancelled: it keeps running and lands in the previous slot when it
//! finishes.
//!
//! Serves Latin `machina:specimen`: one host snapshot. This cut reports
//! memory, per-core CPU and the process table. A deadline of 0 means never
//! wait: answer from the previous slot without spawning a read.

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
struct CoreTicks {
    name: String,
    busy_ns: u64,
    total_ns: u64,
}

/// One process row's absolute CPU and resident-memory figures.
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
struct IntervalCore {
    name: String,
    busy_ns: i64,
    total_ns: i64,
}

/// One process row's CPU and memory across the sampling interval.
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

/// Test-only stand-in for the host read, so a test can drive the reader
/// thread deterministically on any target.
#[cfg(test)]
static COLLECT_OVERRIDE: Mutex<
    Option<std::sync::Arc<dyn Fn() -> Result<Collected, LeafError> + Send + Sync>>,
> = Mutex::new(None);

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

#[cfg(test)]
pub(crate) fn set_collect_override(
    f: Option<std::sync::Arc<dyn Fn() -> Result<Collected, LeafError> + Send + Sync>>,
) {
    match COLLECT_OVERRIDE.lock() {
        Ok(mut guard) => *guard = f,
        Err(poisoned) => *poisoned.into_inner() = f,
    }
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
        let _sent = tx.send(read_host());
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
                    store_late(generation, late);
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
/// Callers have already released their guard (`store_and_render` and
/// `store_late` drop it at the end of their body), so this never nests.
fn store_if_newer(previous: Previous) {
    let mut guard = slot();
    if guard
        .as_ref()
        .is_none_or(|prev| prev.generation < previous.generation)
    {
        *guard = Some(previous);
    }
}

/// Store a late collection against the snapshot that is current when it lands.
/// A late read never had a slot of its own, so the stored snapshot is the only
/// interval baseline available; rendering against `None` would drop the CPU
/// signal from every replay even though the counters moved. A slot with a
/// newer generation rejects the straggler outright, and a slot sampled after
/// this late result is not an interval base (its counters are not a prefix).
fn store_late(generation: u64, collected: Collected) {
    let mut guard = slot();
    let accepted = guard
        .as_ref()
        .is_none_or(|prev| prev.generation < generation);
    if !accepted {
        return;
    }
    let baseline = guard
        .as_ref()
        .filter(|prev| prev.taken_at <= collected.taken_at);
    let rendered = render_intervals(baseline, &collected);
    *guard = Some(make_previous(generation, collected, Some(rendered)));
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
    debug_assert!(old.is_none_or(|previous| previous.taken_at <= fresh.taken_at));
    fresh
        .cores
        .iter()
        .map(|core| {
            let previous = old.and_then(|previous| {
                previous
                    .cores_ticks
                    .iter()
                    .find(|previous_core| previous_core.name == core.name)
            });
            let (busy_ns, total_ns) = previous.map_or((0, 0), |previous_core| {
                (
                    i64::try_from(core.busy_ns.saturating_sub(previous_core.busy_ns))
                        .unwrap_or(i64::MAX),
                    i64::try_from(core.total_ns.saturating_sub(previous_core.total_ns))
                        .unwrap_or(i64::MAX),
                )
            });
            IntervalCore {
                name: core.name.clone(),
                busy_ns,
                total_ns,
            }
        })
        .collect()
}

fn interval_procs(old: Option<&Previous>, fresh: &Collected) -> Vec<IntervalProc> {
    debug_assert!(old.is_none_or(|previous| previous.taken_at <= fresh.taken_at));
    let mut rows = fresh
        .procs
        .iter()
        .map(|proc_row| {
            let previous = old.and_then(|previous| {
                previous
                    .proc_ticks
                    .iter()
                    .find(|previous_proc| previous_proc.pid == proc_row.pid)
            });
            let cpu_ns = previous.map_or(0, |previous_proc| {
                i64::try_from(proc_row.cpu_ns)
                    .unwrap_or(i64::MAX)
                    .saturating_sub(i64::try_from(previous_proc.cpu_ns).unwrap_or(i64::MAX))
            });
            IntervalProc {
                pid: proc_row.pid,
                command: proc_row.command.clone(),
                cpu_ns,
                mem_bytes: i64::try_from(proc_row.mem_bytes).unwrap_or(i64::MAX),
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .cpu_ns
            .cmp(&left.cpu_ns)
            .then_with(|| left.pid.cmp(&right.pid))
    });
    rows.truncate(MAX_PROCS);
    rows
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

/// The reader thread's host read. Tests substitute `COLLECT_OVERRIDE`; every
/// other build reaches `collect()` unchanged.
fn read_host() -> Result<Collected, LeafError> {
    #[cfg(test)]
    {
        // WHY: the Arc is cloned out and the guard dropped at this `;` before
        // the closure runs. Holding the lock across a blocking read would
        // deadlock a second reader.
        let override_fn = match COLLECT_OVERRIDE.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(f) = override_fn {
            return f();
        }
    }
    collect()
}

#[cfg(target_os = "macos")]
fn collect() -> Result<Collected, LeafError> {
    let total_bytes = total_memory()?;
    let used_bytes = used_memory()?;
    Ok(Collected {
        taken_at: Instant::now(),
        cores: collect_cores()?,
        used_bytes,
        total_bytes,
        procs: collect_procs()?,
    })
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn mach_timebase() -> (u32, u32) {
    let mut info = std::mem::MaybeUninit::<libc::mach_timebase_info>::zeroed();
    // SAFETY: `mach_timebase_info` writes one timebase struct to `info`.
    let status = unsafe { libc::mach_timebase_info(info.as_mut_ptr()) };
    if status != 0 {
        return (1, 1);
    }
    // SAFETY: a successful call initialized the full timebase struct.
    let info = unsafe { info.assume_init() };
    if info.denom == 0 {
        (1, 1)
    } else {
        (info.numer, info.denom)
    }
}

#[cfg(target_os = "macos")]
fn scale_mach_ticks(ticks: u64, numer: u32, denom: u32) -> u64 {
    if denom == 0 {
        0
    } else {
        ticks * u64::from(numer) / u64::from(denom)
    }
}

#[cfg(target_os = "macos")]
fn collect_procs() -> Result<Vec<ProcTicks>, LeafError> {
    // `PROC_ALL_PIDS` is defined by libproc.h but is not exported by libc.
    const PROC_ALL_PIDS: u32 = 1;

    // SAFETY: a null buffer with size zero asks libproc for the required size.
    let initial_size = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    if initial_size < 0 {
        return Ok(Vec::new());
    }
    let pid_size = std::mem::size_of::<u32>();
    let initial_pids = usize::try_from(initial_size).unwrap_or(0) / pid_size;
    let mut pids = vec![0_u32; initial_pids.saturating_add(64)];
    let mut retries = 0;
    let pid_count = loop {
        let buffer_bytes = pids.len().saturating_mul(pid_size);
        let buffer_size = i32::try_from(buffer_bytes).unwrap_or(i32::MAX);
        // SAFETY: `pids` is writable for `buffer_size` bytes.
        let listed =
            unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, pids.as_mut_ptr().cast(), buffer_size) };
        if listed < 0 {
            return Ok(Vec::new());
        }
        let listed_bytes = usize::try_from(listed).unwrap_or(usize::MAX);
        if listed_bytes < usize::try_from(buffer_size).unwrap_or(0) {
            break (listed_bytes / pid_size).min(pids.len());
        }
        if retries == 3 {
            break pids.len();
        }
        retries += 1;
        let required_pids = listed_bytes / pid_size;
        pids.resize(required_pids.saturating_add(64), 0);
    };

    let (numer, denom) = mach_timebase();
    let info_size =
        i32::try_from(std::mem::size_of::<libc::proc_taskallinfo>()).unwrap_or(i32::MAX);
    let mut procs = Vec::with_capacity(pid_count);
    for &pid in &pids[..pid_count] {
        let mut info = std::mem::MaybeUninit::<libc::proc_taskallinfo>::zeroed();
        // SAFETY: `info` is writable for the exact task-all-info structure size.
        let read = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTASKALLINFO,
                0,
                info.as_mut_ptr().cast(),
                info_size,
            )
        };
        if read != info_size {
            continue;
        }
        // SAFETY: `proc_pidinfo` returned the exact structure size.
        let info = unsafe { info.assume_init() };
        let name = if info.pbsd.pbi_name[0] != 0 {
            &info.pbsd.pbi_name[..]
        } else {
            &info.pbsd.pbi_comm[..]
        };
        let command_bytes = name
            .iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| byte.to_ne_bytes()[0])
            .collect::<Vec<_>>();
        procs.push(ProcTicks {
            pid: i64::from(pid),
            command: String::from_utf8_lossy(&command_bytes).into_owned(),
            cpu_ns: scale_mach_ticks(
                info.ptinfo.pti_total_user + info.ptinfo.pti_total_system,
                numer,
                denom,
            ),
            mem_bytes: info.ptinfo.pti_resident_size,
        });
    }
    Ok(procs)
}

/// Convert host processor counters, whose unit is one scheduling tick, to ns.
#[cfg(target_os = "macos")]
fn tick_hz() -> u64 {
    // SAFETY: `sysconf` takes a scalar selector and writes no output buffer.
    // This is the sole source for the scheduling-tick scale of host counters.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz <= 0 {
        100
    } else {
        u64::try_from(hz).unwrap_or(100)
    }
}

#[cfg(target_os = "macos")]
fn ticks_to_ns(ticks: u32, hz: u64) -> u64 {
    // `checked_div` yields `None` exactly when `hz` is zero, so the zero-tick
    // scale falls through to the same `0` the explicit guard produced.
    u64::from(ticks)
        .checked_mul(1_000_000_000)
        .and_then(|scaled| scaled.checked_div(hz))
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn wide_ticks_to_ns(ticks: u64, hz: u64) -> u64 {
    if hz == 0 {
        0
    } else if let Ok(ticks) = u32::try_from(ticks) {
        ticks_to_ns(ticks, hz)
    } else {
        ticks.saturating_mul(1_000_000_000) / hz
    }
}

// WHY: libc deprecates `mach_host_self` in favour of the `mach2` crate; adding
// a dependency is outside this leaf's cut, and the call is the same Mach port.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn collect_cores() -> Result<Vec<CoreTicks>, LeafError> {
    let mut count: libc::natural_t = 0;
    let mut info: libc::processor_info_array_t = std::ptr::null_mut();
    let mut info_count: libc::mach_msg_type_number_t = 0;
    // SAFETY: `count` and `info_count` each point to one output integer of
    // their declared type, and `info` points to one output array pointer; on
    // success the kernel fills the array whose length is `info_count` words.
    let status = unsafe {
        libc::host_processor_info(
            libc::mach_host_self(),
            libc::PROCESSOR_CPU_LOAD_INFO,
            std::ptr::from_mut(&mut count),
            std::ptr::from_mut(&mut info),
            std::ptr::from_mut(&mut info_count),
        )
    };
    if status != libc::KERN_SUCCESS {
        if !info.is_null() {
            // SAFETY: if the failed call returned an allocated `info` array,
            // `info_count` gives its length in integer_t words.
            let _ = unsafe {
                libc::vm_deallocate(
                    libc::mach_task_self(),
                    info as libc::vm_address_t,
                    usize::try_from(info_count)
                        .unwrap_or(usize::MAX)
                        .saturating_mul(std::mem::size_of::<libc::integer_t>()),
                )
            };
        }
        return Err(LeafError::unavailable(
            ROUTE,
            format!("host_processor_info failed ({status})"),
        ));
    }

    let cores = (|| {
        let processor_count = usize::try_from(count).unwrap_or(usize::MAX);
        let info_words = usize::try_from(info_count).unwrap_or(usize::MAX);
        let words_per_record = std::mem::size_of::<libc::processor_cpu_load_info>()
            / std::mem::size_of::<libc::integer_t>();
        let Some(required_words) = processor_count.checked_mul(words_per_record) else {
            return Err(LeafError::unavailable(
                ROUTE,
                "host_processor_info returned an invalid CPU array",
            ));
        };
        if (processor_count > 0 && info.is_null()) || required_words > info_words {
            return Err(LeafError::unavailable(
                ROUTE,
                "host_processor_info returned an invalid CPU array",
            ));
        }

        let records: &[libc::processor_cpu_load_info] = if processor_count == 0 {
            &[]
        } else {
            // SAFETY: successful `host_processor_info` returned `count`
            // records, and the validated `info_count` covers their full size.
            unsafe { std::slice::from_raw_parts(info.cast(), processor_count) }
        };
        let hz = tick_hz();
        let user_index = usize::try_from(libc::CPU_STATE_USER).unwrap_or(0);
        let system_index = usize::try_from(libc::CPU_STATE_SYSTEM).unwrap_or(0);
        let idle_index = usize::try_from(libc::CPU_STATE_IDLE).unwrap_or(0);
        let nice_index = usize::try_from(libc::CPU_STATE_NICE).unwrap_or(0);
        Ok(records
            .iter()
            .enumerate()
            .map(|(index, record)| {
                let user = u64::from(record.cpu_ticks[user_index]);
                let system = u64::from(record.cpu_ticks[system_index]);
                let idle = u64::from(record.cpu_ticks[idle_index]);
                let nice = u64::from(record.cpu_ticks[nice_index]);
                let busy_ticks = user + system + nice;
                let total_ticks = busy_ticks + idle;
                CoreTicks {
                    name: format!("cpu{index}"),
                    busy_ns: wide_ticks_to_ns(busy_ticks, hz),
                    total_ns: wide_ticks_to_ns(total_ticks, hz),
                }
            })
            .collect())
    })();

    if !info.is_null() {
        // SAFETY: `info` is the kernel-allocated array returned above, and
        // `info_count` gives its length in integer_t words.
        let _ = unsafe {
            libc::vm_deallocate(
                libc::mach_task_self(),
                info as libc::vm_address_t,
                usize::try_from(info_count)
                    .unwrap_or(usize::MAX)
                    .saturating_mul(std::mem::size_of::<libc::integer_t>()),
            )
        };
    }
    cores
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
