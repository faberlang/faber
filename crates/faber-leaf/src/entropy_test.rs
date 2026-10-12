use super::{Arg, Reply, is_route, reset_seed, serve};
use crate::LeafErrorKind;

fn lock() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn seed(value: i64) {
    serve("aleator:semina", &[Arg::Int(value)])
        .expect("route")
        .expect("seed");
}

fn draw_pair() -> (Reply, Reply) {
    let first = serve("aleator:sortire", &[Arg::Int(0), Arg::Int(i64::MAX)])
        .expect("route")
        .expect("first");
    let second = serve("aleator:sortire", &[Arg::Int(0), Arg::Int(i64::MAX)])
        .expect("route")
        .expect("second");
    (first, second)
}

#[test]
fn route_list_matches_serve() {
    let _guard = lock();
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("aleator:nope"));
    assert!(serve("aleator:nope", &[]).is_none());
    reset_seed();
}

#[test]
fn seed_makes_sortire_deterministic() {
    let _guard = lock();
    reset_seed();
    serve("aleator:semina", &[Arg::Int(42)])
        .expect("route")
        .expect("seed");
    let first = serve("aleator:sortire", &[Arg::Int(1), Arg::Int(100)])
        .expect("route")
        .expect("first");
    serve("aleator:semina", &[Arg::Int(42)])
        .expect("route")
        .expect("seed again");
    let second = serve("aleator:sortire", &[Arg::Int(1), Arg::Int(100)])
        .expect("route")
        .expect("second");
    assert_eq!(first, second);
    reset_seed();
}

#[test]
fn fractum_is_in_unit_interval() {
    let _guard = lock();
    match serve("aleator:fractum", &[])
        .expect("route")
        .expect("float")
    {
        Reply::Float(value) => {
            assert!(value >= 0.0);
            assert!(value < 1.0);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn octetos_has_requested_length() {
    let _guard = lock();
    match serve("aleator:octetos", &[Arg::Int(4)])
        .expect("route")
        .expect("bytes")
    {
        Reply::Bytes(bytes) => assert_eq!(bytes.len(), 4),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn uuid_is_version_4() {
    let _guard = lock();
    match serve("aleator:uuid", &[]).expect("route").expect("uuid") {
        Reply::Str(uuid) => {
            assert_eq!(uuid.len(), 36);
            assert_eq!(uuid.as_bytes()[14], b'4');
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn sortire_rejects_inverted_range() {
    let _guard = lock();
    let error = serve("aleator:sortire", &[Arg::Int(5), Arg::Int(1)])
        .expect("route")
        .expect_err("inverted");
    assert_eq!(error.kind(), LeafErrorKind::InvalidArgument);
}

#[test]
fn octetos_rejects_negative_n() {
    let _guard = lock();
    let error = serve("aleator:octetos", &[Arg::Int(-1)])
        .expect("route")
        .expect_err("negative");
    assert_eq!(error.kind(), LeafErrorKind::InvalidArgument);
}

#[test]
fn semina_non_positive_unseeds() {
    let _guard = lock();

    // seed(1) repeated reproduces the same two draws.
    seed(1);
    let seeded = draw_pair();
    seed(1);
    let seeded_again = draw_pair();
    assert_eq!(seeded, seeded_again, "seed(1) must be reproducible");

    // seed(0) must return draws to OS entropy: reseeding 0 does not
    // reproduce the same pair, and the pair is not the seed-0 stream.
    seed(0);
    let unseeded = draw_pair();
    seed(0);
    let unseeded_again = draw_pair();
    assert_ne!(unseeded, seeded, "seed(0) must not seed with 0");
    assert_ne!(unseeded, unseeded_again, "seed(0) must unseed");

    // Negative seeds also unseed.
    seed(-1);
    let negative = draw_pair();
    assert_ne!(negative, seeded, "seed(-1) must not seed");

    reset_seed();
}
