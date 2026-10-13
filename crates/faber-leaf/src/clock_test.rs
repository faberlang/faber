use super::{Arg, Reply, is_route, serve};
use crate::LeafErrorKind;

#[test]
fn route_list_matches_serve() {
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("tempus:nope"));
    assert!(serve("tempus:nope", &[]).is_none());
}

#[test]
fn now_returns_a_positive_epoch_ns() {
    match serve("tempus:nunc", &[]).expect("route").expect("now") {
        Reply::Int(nanos) => assert!(nanos > 0),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn monotonic_does_not_decrease() {
    let first = match serve("tempus:monotonicum", &[]).expect("route").expect("a") {
        Reply::Int(n) => n,
        other => panic!("unexpected {other:?}"),
    };
    let second = match serve("tempus:monotonicum", &[]).expect("route").expect("b") {
        Reply::Int(n) => n,
        other => panic!("unexpected {other:?}"),
    };
    assert!(second >= first);
}

#[test]
fn active_is_non_negative() {
    match serve("tempus:activum", &[])
        .expect("route")
        .expect("active")
    {
        Reply::Int(n) => assert!(n >= 0),
        other => panic!("unexpected {other:?}"),
    }
}

fn active_nanos() -> i64 {
    match serve("tempus:activum", &[])
        .expect("route")
        .expect("active")
    {
        Reply::Int(n) => n,
        other => panic!("unexpected {other:?}"),
    }
}

/// A host that reports 0 forever passes the non-negative test; this one
/// fails it. Burns CPU until the process clock moves.
#[cfg(unix)]
#[test]
fn active_advances_under_cpu_work() {
    let start = active_nanos();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut acc = 0_u64;
    while active_nanos() <= start {
        assert!(
            std::time::Instant::now() < deadline,
            "process CPU time did not advance"
        );
        for i in 0..200_000_u64 {
            acc = std::hint::black_box(acc.wrapping_mul(31).wrapping_add(i));
        }
    }
    assert!(start > 0, "a running test process has used CPU time");
}

#[test]
fn sleep_rejects_negative_ms() {
    let error = serve("tempus:dormiet", &[Arg::Int(-1)])
        .expect("route")
        .expect_err("negative");
    assert_eq!(error.kind(), LeafErrorKind::InvalidArgument);
}

#[test]
fn sleep_zero_returns_immediately() {
    assert_eq!(
        serve("tempus:dormiet", &[Arg::Int(0)])
            .expect("route")
            .expect("zero"),
        Reply::Unit
    );
}

#[test]
fn sleep_interruptible_reports_cancelled() {
    let error = super::sleep_interruptible(&[Arg::Int(1_000)], &|| true).expect_err("cancelled");
    assert_eq!(error.kind(), LeafErrorKind::Cancelled);
}
