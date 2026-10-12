//! HTTP/1.1 leaf: listen, accept, respond, client get/post.

use super::{reset_handles, serve};
use crate::value::{Arg, Reply};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;

fn lock() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn serve_ok(route: &str, args: &[Arg]) -> Reply {
    serve(route, args)
        .expect("http route")
        .unwrap_or_else(|err| panic!("{route}: {err}"))
}

#[test]
fn plaintext_get_round_trips() {
    let _guard = lock();
    reset_handles();
    let listener = match serve_ok("http:listen", &[Arg::Int(0)]) {
        Reply::Int(id) => id,
        other => panic!("listen: {other:?}"),
    };
    let port = match serve_ok("http:port", &[Arg::Int(listener)]) {
        Reply::Int(port) => port,
        other => panic!("port: {other:?}"),
    };
    let server = thread::spawn(move || {
        let incoming = serve_ok("http:accept", &[Arg::Int(listener)]);
        let Reply::Map(fields) = incoming else {
            panic!("accept: {incoming:?}");
        };
        let id = fields
            .iter()
            .find(|(key, _)| key == "id")
            .and_then(|(_, value)| match value {
                Reply::Str(id) => Some(id.clone()),
                _ => None,
            })
            .expect("id");
        let method = fields
            .iter()
            .find(|(key, _)| key == "method")
            .and_then(|(_, value)| match value {
                Reply::Str(method) => Some(method.clone()),
                _ => None,
            })
            .expect("method");
        assert_eq!(method, "GET");
        serve_ok(
            "http:respond",
            &[
                Arg::Str(id),
                Arg::Int(200),
                Arg::List(Vec::new()),
                Arg::Str("ok GET /health".to_owned()),
            ],
        );
        serve_ok("http:stop", &[Arg::Int(listener)]);
    });
    let mut client = TcpStream::connect(("127.0.0.1", u16::try_from(port).unwrap()))
        .expect("connect");
    client
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("write");
    let mut reply = String::new();
    client.read_to_string(&mut reply).expect("read");
    server.join().expect("server");
    assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
    assert!(reply.ends_with("\r\n\r\nok GET /health"), "{reply}");
}

#[test]
fn client_posts_to_local_listener() {
    let _guard = lock();
    reset_handles();
    let listener = match serve_ok("http:listen", &[Arg::Int(0)]) {
        Reply::Int(id) => id,
        other => panic!("listen: {other:?}"),
    };
    let port = match serve_ok("http:port", &[Arg::Int(listener)]) {
        Reply::Int(port) => port,
        other => panic!("port: {other:?}"),
    };
    let server = thread::spawn(move || {
        let incoming = serve_ok("http:accept", &[Arg::Int(listener)]);
        let Reply::Map(fields) = incoming else {
            panic!("accept: {incoming:?}");
        };
        let id = fields
            .iter()
            .find(|(key, _)| key == "id")
            .and_then(|(_, value)| match value {
                Reply::Str(id) => Some(id.clone()),
                _ => None,
            })
            .expect("id");
        let body = fields
            .iter()
            .find(|(key, _)| key == "body")
            .and_then(|(_, value)| match value {
                Reply::Str(body) => Some(body.clone()),
                _ => None,
            })
            .expect("body");
        assert_eq!(body, "hello");
        serve_ok(
            "http:respond",
            &[
                Arg::Str(id),
                Arg::Int(200),
                Arg::List(Vec::new()),
                Arg::Str("posted".to_owned()),
            ],
        );
        serve_ok("http:stop", &[Arg::Int(listener)]);
    });
    let url = format!("http://127.0.0.1:{port}/post");
    let reply = serve_ok(
        "http:post",
        &[Arg::Str(url), Arg::Str("hello".to_owned())],
    );
    server.join().expect("server");
    let Reply::Map(fields) = reply else {
        panic!("post: {reply:?}");
    };
    assert!(fields.contains(&("status".to_owned(), Reply::Int(200))));
    assert!(fields.contains(&("body".to_owned(), Reply::Str("posted".to_owned()))));
}

#[test]
fn oversize_body_is_refused() {
    let _guard = lock();
    reset_handles();
    let listener = match serve_ok("http:listen", &[Arg::Int(0), Arg::Int(4)]) {
        Reply::Int(id) => id,
        other => panic!("listen: {other:?}"),
    };
    let port = match serve_ok("http:port", &[Arg::Int(listener)]) {
        Reply::Int(port) => port,
        other => panic!("port: {other:?}"),
    };
    let server = thread::spawn(move || {
        let error = serve("http:accept", &[Arg::Int(listener)])
            .expect("route")
            .expect_err("oversize body");
        let _ = serve("http:stop", &[Arg::Int(listener)]);
        error.to_string()
    });
    let mut client = TcpStream::connect(("127.0.0.1", u16::try_from(port).unwrap()))
        .expect("connect");
    client
        .write_all(b"POST /big HTTP/1.1\r\nHost: localhost\r\nContent-Length: 9\r\n\r\n123456789")
        .expect("write");
    let message = server.join().expect("server");
    assert!(
        message.contains("max_body_bytes"),
        "{message}"
    );
}
