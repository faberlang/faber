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
