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
fn interval_cores_differences_by_name() {
    let old = super::make_previous(
        1,
        super::Collected {
            taken_at: std::time::Instant::now(),
            cores: vec![
                super::CoreTicks {
                    name: "cpu0".to_owned(),
                    busy_ns: 100,
                    total_ns: 400,
                },
                super::CoreTicks {
                    name: "cpu1".to_owned(),
                    busy_ns: 50,
                    total_ns: 200,
                },
                super::CoreTicks {
                    name: "cpu9".to_owned(),
                    busy_ns: 1,
                    total_ns: 2,
                },
            ],
            used_bytes: 0,
            total_bytes: 0,
            procs: Vec::new(),
        },
        None,
    );
    let fresh = super::Collected {
        taken_at: old.taken_at + std::time::Duration::from_nanos(1),
        cores: vec![
            super::CoreTicks {
                name: "cpu1".to_owned(),
                busy_ns: 80,
                total_ns: 260,
            },
            super::CoreTicks {
                name: "cpu0".to_owned(),
                busy_ns: 160,
                total_ns: 500,
            },
        ],
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
        cores: vec![
            super::CoreTicks {
                name: "cpu1".to_owned(),
                busy_ns: 80,
                total_ns: 260,
            },
            super::CoreTicks {
                name: "cpu0".to_owned(),
                busy_ns: 160,
                total_ns: 500,
            },
        ],
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

#[cfg(target_os = "macos")]
#[test]
fn ticks_to_ns() {
    assert_eq!(super::ticks_to_ns(100, 100), 1_000_000_000);
    assert_eq!(super::ticks_to_ns(0, 100), 0);
    assert_eq!(super::ticks_to_ns(5, 0), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn collect_reports_every_core() {
    let collected = super::collect().expect("host collection");
    assert!(!collected.cores.is_empty());
    for (index, core) in collected.cores.iter().enumerate() {
        assert_eq!(core.name, format!("cpu{index}"));
        assert!(core.total_ns >= core.busy_ns);
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
