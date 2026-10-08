use super::{local_port, serve};
use crate::{MapKey, Value};
use radix_lexer::Span;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::rc::Rc;
use std::thread::{self, JoinHandle};

fn list(items: Vec<Value>) -> Value {
    Value::Array(Rc::new(RefCell::new(items)))
}

fn route(name: &str, opener: &Value) -> Value {
    serve(name, opener, Span::new(0, 0))
        .unwrap_or_else(|| panic!("{name} should be served"))
        .unwrap_or_else(|error| panic!("{name} failed: {}", error.message))
}

fn route_error(name: &str, opener: &Value) -> String {
    serve(name, opener, Span::new(0, 0))
        .unwrap_or_else(|| panic!("{name} should be served"))
        .expect_err("route should fail")
        .message
}

fn field(carrier: &Value, name: &str) -> Value {
    let Value::Map(fields) = carrier else {
        panic!("carrier should be a map");
    };
    fields
        .borrow()
        .get(&MapKey::Str(name.to_owned()))
        .unwrap_or_else(|| panic!("carrier field {name}"))
        .clone()
}

fn text(value: &Value) -> String {
    let Value::Str(text) = value else {
        panic!("expected textus, got {value:?}");
    };
    text.clone()
}

fn header(name: &str, value: &str) -> Value {
    let mut fields = rustc_hash::FxHashMap::default();
    fields.insert(MapKey::Str("name".to_owned()), Value::Str(name.to_owned()));
    fields.insert(
        MapKey::Str("value".to_owned()),
        Value::Str(value.to_owned()),
    );
    Value::Map(Rc::new(RefCell::new(fields)))
}

fn listen(args: Vec<Value>) -> (Value, u16) {
    let handle = route("http:listen", &list(args));
    let Value::Int(id) = handle else {
        panic!("listen returns an int handle");
    };
    (handle, local_port(id).expect("listener port reads back"))
}

/// Connect, send `request`, and read the whole reply until the server closes.
fn client(port: u16, request: &'static [u8]) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream.write_all(request).expect("send request");
        let mut reply = Vec::new();
        drop(stream.read_to_end(&mut reply));
        reply
    })
}

#[test]
fn port_zero_binds_and_a_get_round_trips() {
    let (handle, port) = listen(vec![Value::Int(0)]);
    assert_ne!(port, 0, "port 0 binds an ephemeral port that reads back");
    let reply = client(
        port,
        b"GET /health?x=1 HTTP/1.1\r\nHost: localhost\r\nX-Probe: yes\r\n\r\n",
    );

    let incoming = route("http:accept", &handle);
    assert_eq!(text(&field(&incoming, "method")), "GET");
    assert_eq!(text(&field(&incoming, "path")), "/health?x=1");
    assert_eq!(text(&field(&incoming, "body")), "");
    assert!(matches!(field(&incoming, "connection"), Value::Int(_)));
    let Value::Array(headers) = field(&incoming, "headers") else {
        panic!("headers carrier is a list");
    };
    let names: Vec<String> = headers
        .borrow()
        .iter()
        .map(|row| text(&field(row, "name")))
        .collect();
    assert_eq!(names, ["Host", "X-Probe"]);

    route(
        "http:respond",
        &list(vec![
            field(&incoming, "id"),
            Value::Int(200),
            list(vec![header("Content-Type", "text/plain")]),
            Value::Str("ok".to_owned()),
        ]),
    );
    let reply = String::from_utf8(reply.join().expect("client")).expect("utf8 reply");
    assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
    assert!(reply.contains("content-length: 2\r\n"), "{reply}");
    assert!(reply.contains("Content-Type: text/plain\r\n"), "{reply}");
    assert!(reply.ends_with("\r\n\r\nok"), "{reply}");
    route("http:stop", &handle);
    assert!(local_port(1).is_none(), "stop releases the listener");
}

#[test]
fn a_post_body_reaches_the_carrier_as_text() {
    let (handle, port) = listen(vec![Value::Int(0)]);
    let reply = client(
        port,
        b"POST /chat HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\n\r\nhello",
    );
    let incoming = route("http:accept", &list(vec![handle.clone()]));
    assert_eq!(text(&field(&incoming, "body")), "hello");
    route(
        "http:respond",
        &list(vec![
            field(&incoming, "id"),
            Value::Int(404),
            list(vec![]),
            Value::Str(String::new()),
        ]),
    );
    let reply = String::from_utf8(reply.join().expect("client")).expect("utf8 reply");
    assert!(reply.starts_with("HTTP/1.1 404 Not Found\r\n"), "{reply}");
    route("http:stop", &handle);
}

#[test]
fn sse_events_are_chunk_framed_and_terminated() {
    let (handle, port) = listen(vec![Value::Int(0)]);
    let reply = client(port, b"GET /events HTTP/1.1\r\nHost: h\r\n\r\n");
    let incoming = route("http:accept", &handle);
    let writer = route(
        "http:respond_open",
        &list(vec![
            field(&incoming, "id"),
            Value::Int(200),
            list(vec![
                header("Content-Type", "text/event-stream"),
                header("Cache-Control", "no-cache"),
            ]),
        ]),
    );
    for event in ["data: one\n\n", "data: two\n\n"] {
        route(
            "http:respond_chunk",
            &list(vec![
                writer.clone(),
                Value::Octeti(event.as_bytes().to_vec()),
            ]),
        );
    }
    route(
        "http:respond_finish",
        &list(vec![writer, Value::Bool(false)]),
    );

    let reply = String::from_utf8(reply.join().expect("client")).expect("utf8 reply");
    assert!(
        reply.starts_with("HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n"),
        "{reply}"
    );
    assert!(
        reply.contains("Content-Type: text/event-stream\r\n"),
        "{reply}"
    );
    let (_, body) = reply.split_once("\r\n\r\n").expect("body after head");
    assert_eq!(
        body,
        "b\r\ndata: one\n\n\r\nb\r\ndata: two\n\n\r\n0\r\n\r\n"
    );
    route("http:stop", &handle);
}

#[test]
fn keep_alive_finish_parks_the_connection_for_the_next_request() {
    let (handle, port) = listen(vec![Value::Int(0)]);
    let reply = thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .write_all(b"GET /a HTTP/1.1\r\nHost: h\r\n\r\n")
            .expect("first request");
        let mut seen = Vec::new();
        let mut chunk = [0_u8; 256];
        while !seen.ends_with(b"0\r\n\r\n") {
            let count = stream.read(&mut chunk).expect("first reply");
            assert_ne!(count, 0, "server closed a keep-alive stream");
            seen.extend_from_slice(&chunk[..count]);
        }
        stream
            .write_all(b"GET /b HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
            .expect("second request");
        let mut rest = Vec::new();
        drop(stream.read_to_end(&mut rest));
        (seen, rest)
    });
    let first = route("http:accept", &handle);
    assert_eq!(text(&field(&first, "path")), "/a");
    let writer = route(
        "http:respond_open",
        &list(vec![field(&first, "id"), Value::Int(200), list(vec![])]),
    );
    route(
        "http:respond_chunk",
        &list(vec![writer.clone(), Value::Str("x".to_owned())]),
    );
    route(
        "http:respond_finish",
        &list(vec![writer, Value::Bool(true)]),
    );

    let second = route("http:accept", &handle);
    assert_eq!(text(&field(&second, "path")), "/b");
    assert_eq!(
        field(&second, "connection").clone_int(),
        field(&first, "connection").clone_int()
    );
    route(
        "http:respond",
        &list(vec![
            field(&second, "id"),
            Value::Int(200),
            list(vec![]),
            Value::Str("two".to_owned()),
        ]),
    );
    let (first_reply, second_reply) = reply.join().expect("client");
    assert!(String::from_utf8_lossy(&first_reply).ends_with("1\r\nx\r\n0\r\n\r\n"));
    assert!(String::from_utf8_lossy(&second_reply).ends_with("two"));
    route("http:stop", &handle);
}

#[test]
fn an_oversize_body_is_refused_by_the_limit() {
    let (handle, port) = listen(vec![Value::Int(0), Value::Int(4)]);
    let reply = client(
        port,
        b"POST /big HTTP/1.1\r\nHost: h\r\nContent-Length: 9\r\n\r\n123456789",
    );
    let message = route_error("http:accept", &handle);
    assert_eq!(message, "http:accept request body exceeds max_body_bytes 4");
    assert!(
        reply.join().expect("client").is_empty(),
        "no response is written for a refused body"
    );

    let (handle, port) = listen(vec![Value::Int(0), Value::Int(4)]);
    let reply = client(
        port,
        b"POST /ok HTTP/1.1\r\nHost: h\r\nContent-Length: 4\r\n\r\n1234",
    );
    let incoming = route("http:accept", &handle);
    assert_eq!(text(&field(&incoming, "body")), "1234");
    route(
        "http:respond",
        &list(vec![
            field(&incoming, "id"),
            Value::Int(200),
            list(vec![]),
            Value::Str(String::new()),
        ]),
    );
    drop(reply.join());
    route("http:stop", &handle);
}

#[test]
fn bind_host_and_argument_validation() {
    let (handle, _port) = listen(vec![Value::Int(0), Value::Str("127.0.0.1".to_owned())]);
    route("http:stop", &handle);
    let too_big = list(vec![Value::Int(0), Value::Int(17 * 1024 * 1024)]);
    assert_eq!(
        route_error("http:listen", &too_big),
        "max_body_bytes must be at most 16777216"
    );
    assert_eq!(
        route_error("http:listen", &list(vec![Value::Int(70_000)])),
        "http:listen port must be between 0 and 65535"
    );
    assert_eq!(
        route_error("http:stop", &Value::Int(999)),
        "http:stop unknown listener 999"
    );
    assert!(
        serve("http:nope", &Value::Unit, Span::new(0, 0)).is_none(),
        "unknown http routes stay unrouted"
    );
}

trait CloneInt {
    fn clone_int(&self) -> i64;
}

impl CloneInt for Value {
    fn clone_int(&self) -> i64 {
        let Value::Int(value) = self else {
            panic!("expected int");
        };
        *value
    }
}
