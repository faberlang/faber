use super::{Arg, Reply, is_route, serve};

fn field<'a>(fields: &'a [(String, Reply)], key: &str) -> &'a Reply {
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .expect("field")
}

fn core_rows(reply: &Reply) -> Vec<(String, i64, i64)> {
    let Reply::Map(fields) = reply else {
        panic!("expected snapshot map");
    };
    let Reply::List(rows) = field(fields, "cores") else {
        panic!("expected cores list");
    };
    rows.iter()
        .map(|row| {
            let Reply::Map(fields) = row else {
                panic!("expected core map");
            };
            let Reply::Str(name) = field(fields, "name") else {
                panic!("expected core name");
            };
            let Reply::Int(busy_ns) = field(fields, "busy_ns") else {
                panic!("expected busy_ns");
            };
            let Reply::Int(total_ns) = field(fields, "total_ns") else {
                panic!("expected total_ns");
            };
            (name.clone(), *busy_ns, *total_ns)
        })
        .collect()
}

fn proc_rows(reply: &Reply) -> Vec<(i64, String, i64, i64)> {
    let Reply::Map(fields) = reply else {
        panic!("expected snapshot map");
    };
    let Reply::List(rows) = field(fields, "procs") else {
        panic!("expected procs list");
    };
    rows.iter()
        .map(|row| {
            let Reply::Map(fields) = row else {
                panic!("expected process map");
            };
            let Reply::Int(pid) = field(fields, "pid") else {
                panic!("expected process pid");
            };
            let Reply::Str(command) = field(fields, "command") else {
                panic!("expected process command");
            };
            let Reply::Int(cpu_ns) = field(fields, "cpu_ns") else {
                panic!("expected process cpu_ns");
            };
            let Reply::Int(mem_bytes) = field(fields, "mem_bytes") else {
                panic!("expected process mem_bytes");
            };
            (*pid, command.clone(), *cpu_ns, *mem_bytes)
        })
        .collect()
}

fn proc_ticks(pid: i64, cpu_ns: u64, mem_bytes: u64) -> super::ProcTicks {
    super::ProcTicks {
        pid,
        command: format!("proc{pid}"),
        cpu_ns,
        mem_bytes,
    }
}

fn core_states(name: &str, user: u32, system: u32, idle: u32, nice: u32) -> super::CoreTicks {
    super::CoreTicks {
        name: name.to_owned(),
        user_ticks: user,
        system_ticks: system,
        idle_ticks: idle,
        nice_ticks: nice,
        tick_hz: 1_000_000_000,
    }
}

fn core_ticks(name: &str, busy: u32, total: u32) -> super::CoreTicks {
    core_states(name, busy, 0, total - busy, 0)
}

fn collected_procs(procs: Vec<super::ProcTicks>) -> super::Collected {
    super::Collected {
        taken_at: std::time::Instant::now(),
        cores: Vec::new(),
        used_bytes: 0,
        total_bytes: 0,
        procs,
    }
}

#[test]
fn route_list_matches_serve() {
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("machina:nope"));
    assert!(serve("machina:nope", &[]).is_none());
}

#[test]
fn specimen_rejects_bad_deadlines() {
    assert!(serve("machina:specimen", &[]).is_some_and(|reply| reply.is_err()));
    assert!(
        serve("machina:specimen", &[Arg::Str("x".to_owned())]).is_some_and(|reply| reply.is_err())
    );
    assert!(serve("machina:specimen", &[Arg::Int(-1)]).is_some_and(|reply| reply.is_err()));
}

#[test]
fn deadline_zero_without_previous_is_stale_and_empty() {
    let _guard = crate::test_support::handle_table_guard();
    super::reset_previous();
    let reply = serve("machina:specimen", &[Arg::Int(0)])
        .expect("route")
        .expect("specimen");
    assert_eq!(
        reply,
        Reply::Map(vec![
            ("stale".to_owned(), Reply::Bool(true)),
            ("age_ms".to_owned(), Reply::Int(0)),
            ("cores".to_owned(), Reply::List(Vec::new()),),
            (
                "memory".to_owned(),
                Reply::Map(vec![
                    ("used_bytes".to_owned(), Reply::Int(0)),
                    ("total_bytes".to_owned(), Reply::Int(0)),
                ]),
            ),
            ("procs".to_owned(), Reply::List(Vec::new()),),
        ])
    );
}

#[cfg(target_os = "macos")]
#[test]
fn fresh_snapshot_reports_memory() {
    let _guard = crate::test_support::handle_table_guard();
    super::reset_previous();
    let reply = serve("machina:specimen", &[Arg::Int(5000)])
        .expect("route")
        .expect("specimen");
    let Reply::Map(fields) = &reply else {
        panic!("expected map, got {reply:?}");
    };
    assert_eq!(field(fields, "stale"), &Reply::Bool(false));
    assert_eq!(field(fields, "age_ms"), &Reply::Int(0));
    let cores = core_rows(&reply);
    assert!(!cores.is_empty());
    assert!(
        cores
            .iter()
            .all(|(_, busy_ns, total_ns)| total_ns >= busy_ns)
    );
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    let Reply::Int(total) = field(memory, "total_bytes") else {
        panic!("expected total_bytes int");
    };
    let Reply::Int(used) = field(memory, "used_bytes") else {
        panic!("expected used_bytes int");
    };
    assert!(*total > 0, "host total memory must be positive");
    assert!(*used <= *total, "used memory cannot exceed total");
}

#[test]
fn interval_cores_cpu_state_rollover_preserves_interval() {
    let old = super::make_previous(
        1,
        super::Collected {
            taken_at: std::time::Instant::now(),
            cores: vec![core_states("cpu0", 100, 10, u32::MAX - 5, 20)],
            used_bytes: 0,
            total_bytes: 0,
            procs: Vec::new(),
        },
        None,
    );
    let fresh = super::Collected {
        taken_at: old.taken_at + std::time::Duration::from_nanos(1),
        cores: vec![core_states("cpu0", 105, 12, 2, 21)],
        used_bytes: 0,
        total_bytes: 0,
        procs: Vec::new(),
    };

    let rows = super::interval_cores(Some(&old), &fresh);
    assert_eq!((rows[0].busy_ns, rows[0].total_ns), (8, 16));
    assert!(rows[0].total_ns >= rows[0].busy_ns);
}

#[test]
fn interval_cores_differences_by_name() {
    let old = super::make_previous(
        1,
        super::Collected {
            taken_at: std::time::Instant::now(),
            cores: vec![
                core_ticks("cpu0", 100, 400),
                core_ticks("cpu1", 50, 200),
                core_ticks("cpu9", 1, 2),
            ],
            used_bytes: 0,
            total_bytes: 0,
            procs: Vec::new(),
        },
        None,
    );
    let fresh = super::Collected {
        taken_at: old.taken_at + std::time::Duration::from_nanos(1),
        cores: vec![core_ticks("cpu1", 80, 260), core_ticks("cpu0", 160, 500)],
        used_bytes: 0,
        total_bytes: 0,
        procs: Vec::new(),
    };

    let rows = super::interval_cores(Some(&old), &fresh);
    assert_eq!(
        rows.iter()
            .map(|row| (row.name.as_str(), row.busy_ns, row.total_ns))
            .collect::<Vec<_>>(),
        vec![("cpu1", 30, 60), ("cpu0", 60, 100)]
    );
}

#[test]
fn interval_cores_first_snapshot_is_zero() {
    let fresh = super::Collected {
        taken_at: std::time::Instant::now(),
        cores: vec![core_ticks("cpu1", 80, 260), core_ticks("cpu0", 160, 500)],
        used_bytes: 0,
        total_bytes: 0,
        procs: Vec::new(),
    };

    let rows = super::interval_cores(None, &fresh);
    assert_eq!(
        rows.iter()
            .map(|row| (row.name.as_str(), row.busy_ns, row.total_ns))
            .collect::<Vec<_>>(),
        vec![("cpu1", 0, 0), ("cpu0", 0, 0)]
    );
}

#[test]
fn interval_procs_differences_by_pid() {
    let old = super::make_previous(
        1,
        collected_procs(vec![proc_ticks(1, 0, 100), proc_ticks(2, 500, 200)]),
        None,
    );
    let fresh = collected_procs(vec![
        proc_ticks(2, 900, 220),
        proc_ticks(1, 300, 150),
        proc_ticks(3, 7, 50),
    ]);

    let rows = super::interval_procs(Some(&old), &fresh);
    assert_eq!(
        rows.iter()
            .map(|row| (row.pid, row.cpu_ns, row.mem_bytes))
            .collect::<Vec<_>>(),
        vec![(2, 400, 220), (1, 300, 150), (3, 0, 50)]
    );
}

#[test]
fn interval_procs_subtracts_unsigned_counters_before_conversion() {
    let above_i64_max = u64::try_from(i64::MAX).expect("i64 max") + 1;
    let old = super::make_previous(
        1,
        collected_procs(vec![
            proc_ticks(1, 500, 0),
            proc_ticks(2, above_i64_max + 100, 0),
        ]),
        None,
    );
    let fresh = collected_procs(vec![
        proc_ticks(1, 100, 0),
        proc_ticks(2, above_i64_max + 500, 0),
    ]);

    let rows = super::interval_procs(Some(&old), &fresh);
    assert_eq!(
        rows.iter()
            .map(|row| (row.pid, row.cpu_ns))
            .collect::<Vec<_>>(),
        vec![(2, 400), (1, 0)]
    );
}

#[test]
fn interval_procs_drops_exited_pid() {
    let old = super::make_previous(
        1,
        collected_procs(vec![proc_ticks(1, 100, 100), proc_ticks(2, 200, 200)]),
        None,
    );
    let fresh = collected_procs(vec![proc_ticks(1, 150, 120)]);

    let rows = super::interval_procs(Some(&old), &fresh);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].pid, rows[0].cpu_ns, rows[0].mem_bytes),
        (1, 50, 120)
    );
}

#[test]
fn interval_procs_caps_at_max_procs() {
    let old = super::make_previous(
        1,
        collected_procs((1_i64..=80).map(|pid| proc_ticks(pid, 0, 0)).collect()),
        None,
    );
    let fresh = collected_procs(
        (1_i64..=80)
            .map(|pid| proc_ticks(pid, u64::try_from(pid * 1000).expect("cpu counter"), 1))
            .collect(),
    );

    let rows = super::interval_procs(Some(&old), &fresh);
    assert_eq!(rows.len(), super::MAX_PROCS);
    assert_eq!((rows[0].pid, rows[0].cpu_ns), (80, 80_000));
    assert_eq!(
        (
            rows[super::MAX_PROCS - 1].pid,
            rows[super::MAX_PROCS - 1].cpu_ns
        ),
        (17, 17_000)
    );
}

#[test]
fn interval_procs_first_snapshot_is_zero_cpu() {
    let fresh = collected_procs(vec![
        proc_ticks(3, 700, 30),
        proc_ticks(1, 100, 10),
        proc_ticks(2, 400, 20),
    ]);

    let rows = super::interval_procs(None, &fresh);
    assert_eq!(
        rows.iter()
            .map(|row| (row.pid, row.cpu_ns, row.mem_bytes))
            .collect::<Vec<_>>(),
        vec![(1, 0, 10), (2, 0, 20), (3, 0, 30)]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn scale_mach_ticks() {
    assert_eq!(super::scale_mach_ticks(1000, 1, 1), 1000);
    assert_eq!(super::scale_mach_ticks(1000, 125, 3), 41666);
    assert_eq!(super::scale_mach_ticks(5, 1, 0), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn ticks_to_ns() {
    assert_eq!(super::ticks_to_ns(100, 100), 1_000_000_000);
    assert_eq!(super::ticks_to_ns(0, 100), 0);
    assert_eq!(super::ticks_to_ns(5, 0), 0);
    // Four wrapped CPU-state counters can sum past `u32`; the scale still holds.
    assert_eq!(
        super::ticks_to_ns(u64::from(u32::MAX) + 1, 1_000_000_000),
        4_294_967_296
    );
}

#[cfg(target_os = "macos")]
#[test]
fn collect_sees_own_process() {
    let collected = super::collect().expect("host collection");
    let own_pid = i64::from(std::process::id());
    let own_process = collected
        .procs
        .iter()
        .find(|proc_row| proc_row.pid == own_pid)
        .expect("own process row");
    assert!(!own_process.command.is_empty());
    assert!(own_process.mem_bytes > 0);
}

#[cfg(target_os = "macos")]
#[test]
fn fresh_snapshot_reports_processes() {
    let _guard = crate::test_support::handle_table_guard();
    super::reset_previous();
    let _warm = serve("machina:specimen", &[Arg::Int(5000)])
        .expect("route")
        .expect("specimen");
    let fresh = serve("machina:specimen", &[Arg::Int(5000)])
        .expect("route")
        .expect("specimen");
    let replay = serve("machina:specimen", &[Arg::Int(0)])
        .expect("route")
        .expect("specimen");

    let fresh_rows = proc_rows(&fresh);
    let replay_rows = proc_rows(&replay);
    assert!(!fresh_rows.is_empty());
    assert!(!replay_rows.is_empty());
    assert!(fresh_rows.len() <= super::MAX_PROCS);
    assert!(replay_rows.len() <= super::MAX_PROCS);
    assert_eq!(
        fresh_rows.iter().map(|row| row.0).collect::<Vec<_>>(),
        replay_rows.iter().map(|row| row.0).collect::<Vec<_>>()
    );
    for rows in [&fresh_rows, &replay_rows] {
        assert!(rows.iter().all(|row| !row.1.is_empty()));
        assert!(rows.windows(2).all(|pair| pair[0].2 >= pair[1].2));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn collect_reports_every_core() {
    let collected = super::collect().expect("host collection");
    assert!(!collected.cores.is_empty());
    for (index, core) in collected.cores.iter().enumerate() {
        assert_eq!(core.name, format!("cpu{index}"));
        let busy_ticks =
            u64::from(core.user_ticks) + u64::from(core.system_ticks) + u64::from(core.nice_ticks);
        let total_ticks = busy_ticks + u64::from(core.idle_ticks);
        assert!(total_ticks >= busy_ticks);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn fresh_snapshot_reports_per_core_cpu() {
    let _guard = crate::test_support::handle_table_guard();
    super::reset_previous();
    let first = serve("machina:specimen", &[Arg::Int(5000)])
        .expect("route")
        .expect("specimen");
    let second = serve("machina:specimen", &[Arg::Int(5000)])
        .expect("route")
        .expect("specimen");
    let replay = serve("machina:specimen", &[Arg::Int(0)])
        .expect("route")
        .expect("specimen");

    let first_rows = core_rows(&first);
    let second_rows = core_rows(&second);
    let replay_rows = core_rows(&replay);
    assert!(!first_rows.is_empty());
    assert_eq!(second_rows.len(), first_rows.len());
    assert_eq!(replay_rows.len(), first_rows.len());
    let names = |rows: &[(String, i64, i64)]| {
        rows.iter()
            .map(|(name, _, _)| name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&second_rows), names(&first_rows));
    assert_eq!(names(&replay_rows), names(&first_rows));
    for rows in [&first_rows, &second_rows, &replay_rows] {
        assert!(
            rows.iter()
                .all(|(_, busy_ns, total_ns)| total_ns >= busy_ns && *busy_ns >= 0)
        );
    }
}

#[test]
fn out_of_order_guard_rejects_older_generation() {
    let _guard = crate::test_support::handle_table_guard();
    super::reset_previous();
    super::store_if_newer(super::make_previous(
        2,
        super::collected_for_test(222),
        None,
    ));
    // The late straggler of an earlier read: it started first, so its
    // generation is behind the slot's even though its `taken_at` is newer.
    super::store_if_newer(super::make_previous(
        1,
        super::collected_for_test(111),
        None,
    ));
    let reply = serve("machina:specimen", &[Arg::Int(0)])
        .expect("route")
        .expect("specimen");
    let Reply::Map(fields) = &reply else {
        panic!("expected map, got {reply:?}");
    };
    assert_eq!(field(fields, "stale"), &Reply::Bool(true));
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    assert_eq!(field(memory, "used_bytes"), &Reply::Int(222));

    // A newer generation is still admitted.
    super::store_if_newer(super::make_previous(
        3,
        super::collected_for_test(333),
        None,
    ));
    let reply = serve("machina:specimen", &[Arg::Int(0)])
        .expect("route")
        .expect("specimen");
    let Reply::Map(fields) = &reply else {
        panic!("expected map, got {reply:?}");
    };
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    assert_eq!(field(memory, "used_bytes"), &Reply::Int(333));
}

// WHY: a late read lands with no slot of its own, so its intervals must be
// measured against the snapshot already in the slot; rendering against `None`
// would report zero CPU even though the counters moved. The poll waits for the
// late snapshot to become visible instead of sleeping a fixed time, so the
// assertions never depend on scheduler timing.
#[test]
fn late_result_renders_intervals_against_previous_snapshot() {
    let _guard = crate::test_support::handle_table_guard();
    super::reset_previous();
    super::store_if_newer(super::make_previous(
        0,
        super::Collected {
            taken_at: std::time::Instant::now(),
            cores: vec![core_ticks("cpu0", 100, 400), core_ticks("cpu1", 50, 200)],
            used_bytes: 111,
            total_bytes: 1000,
            procs: vec![proc_ticks(7, 1000, 500), proc_ticks(8, 2000, 600)],
        },
        None,
    ));

    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    // WHY: `Receiver` is `Send` but not `Sync`, and the override is shared; the
    // lock is only ever taken by the one read that blocks.
    let release_rx = std::sync::Mutex::new(release_rx);
    super::set_collect_override(Some(std::sync::Arc::new(move || {
        release_rx
            .lock()
            .expect("release lock")
            .recv()
            .expect("release reader");
        Ok(super::Collected {
            taken_at: std::time::Instant::now(),
            cores: vec![core_ticks("cpu0", 160, 500), core_ticks("cpu1", 80, 260)],
            used_bytes: 222,
            total_bytes: 1000,
            procs: vec![
                proc_ticks(7, 1400, 520),
                proc_ticks(8, 2300, 610),
                proc_ticks(9, 99, 70),
            ],
        })
    })));

    // The read times out while the override blocks, so no slot exists for it
    // yet; the seeded snapshot is the only baseline it can difference against.
    let timed_out = serve("machina:specimen", &[Arg::Int(1)])
        .expect("route")
        .expect("specimen");
    release_tx.send(()).expect("release reader");

    let mut late = None;
    for _ in 0..200 {
        let reply = serve("machina:specimen", &[Arg::Int(0)])
            .expect("route")
            .expect("specimen");
        let Reply::Map(fields) = &reply else {
            panic!("expected map, got {reply:?}");
        };
        let Reply::Map(memory) = field(fields, "memory") else {
            panic!("expected memory map");
        };
        if field(memory, "used_bytes") == &Reply::Int(222) {
            late = Some(reply);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // Clear the override before asserting so a panic cannot leak it.
    super::set_collect_override(None);
    let late = late.expect("late snapshot lands in the slot");

    let Reply::Map(fields) = &timed_out else {
        panic!("expected map, got {timed_out:?}");
    };
    assert_eq!(field(fields, "stale"), &Reply::Bool(true));
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    assert_eq!(field(memory, "used_bytes"), &Reply::Int(111));

    assert_eq!(
        core_rows(&late),
        vec![("cpu0".to_owned(), 60, 100), ("cpu1".to_owned(), 30, 60),]
    );
    assert_eq!(
        proc_rows(&late),
        vec![
            (7, "proc7".to_owned(), 400, 520),
            (8, "proc8".to_owned(), 300, 610),
            (9, "proc9".to_owned(), 0, 70),
        ]
    );

    // A straggler that is not newer than the stored one is still rejected, so
    // rendering against the baseline does not loosen the generation ordering.
    super::store_late(1, super::collected_for_test(333));
    let reply = serve("machina:specimen", &[Arg::Int(0)])
        .expect("route")
        .expect("specimen");
    let Reply::Map(fields) = &reply else {
        panic!("expected map, got {reply:?}");
    };
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    assert_eq!(field(memory, "used_bytes"), &Reply::Int(222));
}

// WHY: the final assertion is timing-independent on correct code --
// `store_if_newer` can only ever leave 222 in the slot, because the timeout
// watcher's generation (1) is behind the stored one (2). The bounded wait
// exists only so a regression (an unconditional slot write) fires reliably
// instead of slipping through the race window.
#[test]
fn late_watcher_result_does_not_overwrite_newer_success() {
    let _guard = crate::test_support::handle_table_guard();
    super::reset_previous();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    // WHY: `Receiver` is `Send` but not `Sync`, and the override is shared; the
    // lock is only ever taken by read #0, the one call that blocks.
    let release_rx = std::sync::Mutex::new(release_rx);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&calls);
    super::set_collect_override(Some(std::sync::Arc::new(move || {
        if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            let released = release_rx.lock().expect("release lock");
            let _ = released.recv(); // block until the test releases us
            Ok(super::collected_for_test(111))
        } else {
            Ok(super::collected_for_test(222))
        }
    })));

    // (1) Read #0 is still blocked, so this call times out with an empty slot.
    let timed_out = serve("machina:specimen", &[Arg::Int(1)])
        .expect("route")
        .expect("specimen");

    // (2) Read #1 (generation 2) lands: the slot now holds 222.
    let fresh = serve("machina:specimen", &[Arg::Int(5000)])
        .expect("route")
        .expect("specimen");

    // (3) Release read #0; its late 111 snapshot reaches the timeout watcher
    // of phase 1, which holds the older generation 1.
    release_tx.send(()).expect("release reader");

    // (4) Give a broken guard its chance to fire, then read the slot without
    // spawning a read (deadline 0).
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let after_late_arrival = serve("machina:specimen", &[Arg::Int(0)])
        .expect("route")
        .expect("specimen");

    // Clear the override before asserting so a panic cannot leak it.
    super::set_collect_override(None);

    let Reply::Map(fields) = &timed_out else {
        panic!("expected map, got {timed_out:?}");
    };
    assert_eq!(field(fields, "stale"), &Reply::Bool(true));
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    assert_eq!(field(memory, "used_bytes"), &Reply::Int(0));

    let Reply::Map(fields) = &fresh else {
        panic!("expected map, got {fresh:?}");
    };
    assert_eq!(field(fields, "stale"), &Reply::Bool(false));
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    assert_eq!(field(memory, "used_bytes"), &Reply::Int(222));

    let Reply::Map(fields) = &after_late_arrival else {
        panic!("expected map, got {after_late_arrival:?}");
    };
    assert_eq!(field(fields, "stale"), &Reply::Bool(true));
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    assert_eq!(field(memory, "used_bytes"), &Reply::Int(222));
}

#[test]
fn collect_override_feeds_reply() {
    let _guard = crate::test_support::handle_table_guard();
    super::reset_previous();
    super::set_collect_override(Some(std::sync::Arc::new(|| {
        Ok(super::collected_for_test(777))
    })));
    let reply = serve("machina:specimen", &[Arg::Int(5000)])
        .expect("route")
        .expect("specimen");
    super::set_collect_override(None);
    let Reply::Map(fields) = &reply else {
        panic!("expected map, got {reply:?}");
    };
    assert_eq!(field(fields, "stale"), &Reply::Bool(false));
    let Reply::Map(memory) = field(fields, "memory") else {
        panic!("expected memory map");
    };
    assert_eq!(field(memory, "used_bytes"), &Reply::Int(777));
}
