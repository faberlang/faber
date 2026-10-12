use super::{Arg, Reply, serve};
use crate::LeafErrorKind;

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
