use super::{Arg, Reply, is_route, reset_handles, serve};
use std::thread;

#[test]
fn route_list_matches_serve() {
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("caelum:nope"));
    assert!(serve("caelum:nope", &[]).is_none());
    reset_handles();
}

fn serve_ok(route: &str, args: &[Arg]) -> Reply {
    serve(route, args)
        .expect("tcp route")
        .unwrap_or_else(|err| panic!("{route} failed: {err}"))
}

#[test]
fn unknown_route_is_not_this_leaf() {
    assert!(serve("solum:lege", &[]).is_none());
}

#[test]
fn plaintext_echo_round_trips() {
    reset_handles();
    let listener = match serve_ok("caelum:ausculta", &[Arg::Int(0)]) {
        Reply::Int(id) => id,
        other => panic!("unexpected {other:?}"),
    };
    let port = match serve_ok("caelum:portus", &[Arg::Int(listener)]) {
        Reply::Int(port) => port,
        other => panic!("unexpected {other:?}"),
    };
    let server = thread::spawn(move || {
        let client = match serve_ok("caelum:accipe", &[Arg::Int(listener)]) {
            Reply::Int(id) => id,
            other => panic!("unexpected {other:?}"),
        };
        let got = match serve_ok("caelum:hauri", &[Arg::Int(client), Arg::Int(5)]) {
            Reply::Bytes(bytes) => bytes,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(got, b"hello");
        assert!(matches!(
            serve_ok(
                "caelum:funde",
                &[Arg::Int(client), Arg::Bytes(b"world".to_vec())],
            ),
            Reply::Unit
        ));
        assert!(matches!(
            serve_ok("caelum:claude", &[Arg::Int(client)]),
            Reply::Unit
        ));
    });
    let socket = match serve_ok(
        "caelum:necte",
        &[
            Arg::Str("127.0.0.1".to_owned()),
            Arg::Int(port),
            Arg::Bool(false),
        ],
    ) {
        Reply::Int(id) => id,
        other => panic!("unexpected {other:?}"),
    };
    assert!(matches!(
        serve_ok(
            "caelum:funde",
            &[Arg::Int(socket), Arg::Bytes(b"hello".to_vec())],
        ),
        Reply::Unit
    ));
    let got = match serve_ok("caelum:hauri", &[Arg::Int(socket), Arg::Int(5)]) {
        Reply::Bytes(bytes) => bytes,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(got, b"world");
    assert!(matches!(
        serve_ok("caelum:claude", &[Arg::Int(socket)]),
        Reply::Unit
    ));
    server.join().expect("server");
    reset_handles();
}
