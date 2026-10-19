use super::{Arg, Reply, is_route, serve};

fn field<'a>(fields: &'a [(String, Reply)], key: &str) -> &'a Reply {
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .expect("field")
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
    assert_eq!(field(fields, "cores"), &Reply::List(Vec::new()));
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
