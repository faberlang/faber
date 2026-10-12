use super::{Arg, Reply, is_route, reset_seed, serve};
use crate::LeafErrorKind;

#[test]
fn route_list_matches_serve() {
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
    let error = serve("aleator:sortire", &[Arg::Int(5), Arg::Int(1)])
        .expect("route")
        .expect_err("inverted");
    assert_eq!(error.kind(), LeafErrorKind::InvalidArgument);
}

#[test]
fn octetos_rejects_negative_n() {
    let error = serve("aleator:octetos", &[Arg::Int(-1)])
        .expect("route")
        .expect_err("negative");
    assert_eq!(error.kind(), LeafErrorKind::InvalidArgument);
}
