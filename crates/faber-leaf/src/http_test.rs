//! HTTP/1.1 leaf: listen, accept, respond, client get/post.

use super::{is_route, reset_handles, serve};
use crate::value::{Arg, Reply};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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
    let mut client =
        TcpStream::connect(("127.0.0.1", u16::try_from(port).unwrap())).expect("connect");
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
    let reply = serve_ok("http:post", &[Arg::Str(url), Arg::Str("hello".to_owned())]);
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
    let mut client =
        TcpStream::connect(("127.0.0.1", u16::try_from(port).unwrap())).expect("connect");
    client
        .write_all(b"POST /big HTTP/1.1\r\nHost: localhost\r\nContent-Length: 9\r\n\r\n123456789")
        .expect("write");
    let message = server.join().expect("server");
    assert!(message.contains("max_body_bytes"), "{message}");
}

#[test]
fn request_open_reads_chunked_body_incrementally() {
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
        let writer = match serve_ok(
            "http:respond_open",
            &[
                Arg::Str(id),
                Arg::Int(200),
                Arg::List(vec![Arg::Map(vec![
                    ("name".to_owned(), "Content-Type".to_owned()),
                    ("value".to_owned(), "text/event-stream".to_owned()),
                ])]),
            ],
        ) {
            Reply::Int(writer) => writer,
            other => panic!("respond_open: {other:?}"),
        };
        serve_ok(
            "http:respond_chunk",
            &[Arg::Int(writer), Arg::Str("one".to_owned())],
        );
        serve_ok(
            "http:respond_chunk",
            &[Arg::Int(writer), Arg::Str("two".to_owned())],
        );
        serve_ok("http:respond_finish", &[Arg::Int(writer), Arg::Bool(false)]);
        serve_ok("http:stop", &[Arg::Int(listener)]);
    });
    let url = format!("http://127.0.0.1:{port}/sse");
    let opened = serve_ok(
        "http:request_open",
        &[
            Arg::Str("GET".to_owned()),
            Arg::Str(url),
            Arg::List(Vec::new()),
            Arg::Str(String::new()),
        ],
    );
    let Reply::Map(fields) = opened else {
        panic!("open: {opened:?}");
    };
    let reader = fields
        .iter()
        .find(|(key, _)| key == "reader")
        .and_then(|(_, value)| match value {
            Reply::Int(reader) => Some(*reader),
            _ => None,
        })
        .expect("reader");
    let mut body = Vec::new();
    loop {
        let chunk = serve_ok("http:read", &[Arg::Int(reader)]);
        let Reply::Map(fields) = chunk else {
            panic!("read: {chunk:?}");
        };
        let bytes = fields
            .iter()
            .find(|(key, _)| key == "bytes")
            .and_then(|(_, value)| match value {
                Reply::Bytes(bytes) => Some(bytes.clone()),
                Reply::Str(text) => Some(text.as_bytes().to_vec()),
                _ => None,
            })
            .expect("bytes");
        let done = fields
            .iter()
            .find(|(key, _)| key == "done")
            .and_then(|(_, value)| match value {
                Reply::Bool(done) => Some(*done),
                _ => None,
            })
            .expect("done");
        body.extend(bytes);
        if done {
            break;
        }
    }
    server.join().expect("server");
    assert_eq!(String::from_utf8_lossy(&body), "onetwo");
}

#[test]
fn request_open_reassembles_content_length_body_across_segments() {
    let _guard = lock();
    reset_handles();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n")
            .expect("head");
        socket.flush().expect("flush head");
        thread::sleep(std::time::Duration::from_millis(100));
        socket.write_all(b"12345").expect("first half");
        socket.flush().expect("flush first");
        thread::sleep(std::time::Duration::from_millis(100));
        socket.write_all(b"67890").expect("second half");
        socket.flush().expect("flush second");
    });
    let url = format!("http://127.0.0.1:{port}/body");
    let opened = serve_ok(
        "http:request_open",
        &[
            Arg::Str("GET".to_owned()),
            Arg::Str(url),
            Arg::List(Vec::new()),
            Arg::Str(String::new()),
        ],
    );
    let Reply::Map(fields) = opened else {
        panic!("open: {opened:?}");
    };
    let reader = fields
        .iter()
        .find(|(key, _)| key == "reader")
        .and_then(|(_, value)| match value {
            Reply::Int(reader) => Some(*reader),
            _ => None,
        })
        .expect("reader");
    let mut body = Vec::new();
    loop {
        let chunk = serve_ok("http:read", &[Arg::Int(reader)]);
        let Reply::Map(fields) = chunk else {
            panic!("read: {chunk:?}");
        };
        let bytes = fields
            .iter()
            .find(|(key, _)| key == "bytes")
            .and_then(|(_, value)| match value {
                Reply::Bytes(bytes) => Some(bytes.clone()),
                Reply::Str(text) => Some(text.as_bytes().to_vec()),
                _ => None,
            })
            .expect("bytes");
        let done = fields
            .iter()
            .find(|(key, _)| key == "done")
            .and_then(|(_, value)| match value {
                Reply::Bool(done) => Some(*done),
                _ => None,
            })
            .expect("done");
        body.extend(bytes);
        if done {
            break;
        }
    }
    server.join().expect("server");
    assert_eq!(body, b"1234567890");
}

#[test]
fn route_list_matches_serve() {
    let _guard = lock();
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("http:nope"));
    assert!(serve("http:nope", &[]).is_none());
    reset_handles();
}
