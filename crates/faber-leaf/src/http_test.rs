//! HTTP/1.1 leaf: listen, accept, respond, client get/post.

use super::{is_route, reset_handles, serve, split_url};
use crate::value::{Arg, Reply};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn serve_ok(route: &str, args: &[Arg]) -> Reply {
    serve(route, args)
        .expect("http route")
        .unwrap_or_else(|err| panic!("{route}: {err}"))
}

#[test]
fn plaintext_get_round_trips() {
    let _guard = crate::test_support::handle_table_guard();
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
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    client
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("write timeout");
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
    let _guard = crate::test_support::handle_table_guard();
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
fn client_reports_a_closed_peer_on_its_own_route() {
    let _guard = crate::test_support::handle_table_guard();
    reset_handles();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request);
    });
    let url = format!("http://127.0.0.1:{port}/gone");
    let error = serve("http:get", &[Arg::Str(url)])
        .expect("route")
        .expect_err("peer closed");
    server.join().expect("server");
    assert!(error.to_string().starts_with("http:request:"), "{error}");
}

#[test]
fn oversize_body_is_refused() {
    let _guard = crate::test_support::handle_table_guard();
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
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    client
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("write timeout");
    client
        .write_all(b"POST /big HTTP/1.1\r\nHost: localhost\r\nContent-Length: 9\r\n\r\n123456789")
        .expect("write");
    let message = server.join().expect("server");
    assert!(message.contains("max_body_bytes"), "{message}");
}

#[test]
fn request_open_reads_chunked_body_incrementally() {
    let _guard = crate::test_support::handle_table_guard();
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
    let _guard = crate::test_support::handle_table_guard();
    reset_handles();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .expect("write timeout");
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
fn request_send_writes_request_and_pends() {
    let _guard = crate::test_support::handle_table_guard();
    reset_handles();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        read_request_head(&mut socket)
    });
    let url = format!("http://127.0.0.1:{port}/send");
    let id = match serve_ok(
        "http:request_send",
        &[
            Arg::Str("GET".to_owned()),
            Arg::Str(url),
            Arg::List(vec![Arg::Map(vec![
                ("name".to_owned(), "x-split".to_owned()),
                ("value".to_owned(), "yes".to_owned()),
            ])]),
            Arg::Str(String::new()),
        ],
    ) {
        Reply::Int(id) => id,
        other => panic!("request_send: {other:?}"),
    };
    let request = server.join().expect("server");
    assert!(request.starts_with("GET /send HTTP/1.1"), "{request}");
    assert!(request.contains("Host: 127.0.0.1:"), "{request}");
    assert!(request.contains("Connection: close"), "{request}");
    assert!(request.contains("x-split: yes"), "{request}");
    // A pending request is not a reader.
    let read = serve("http:read", &[Arg::Int(id)]).expect("route");
    assert!(read.is_err(), "{read:?}");
}

#[test]
fn request_head_completes_split_exchange() {
    let _guard = crate::test_support::handle_table_guard();
    reset_handles();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let request = read_request_head(&mut socket);
        let body = read_request_body(&mut socket, &request, 7);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nx-split: yes\r\n\r\nhello")
            .expect("write response");
        socket.flush().expect("flush response");
        (request, body)
    });
    let url = format!("http://127.0.0.1:{port}/split");
    let id = match serve_ok(
        "http:request_send",
        &[
            Arg::Str("POST".to_owned()),
            Arg::Str(url),
            Arg::List(Vec::new()),
            Arg::Str("payload".to_owned()),
        ],
    ) {
        Reply::Int(id) => id,
        other => panic!("request_send: {other:?}"),
    };
    let opened = serve_ok("http:request_head", &[Arg::Int(id)]);
    let (request, body) = server.join().expect("server");
    assert!(request.contains("Content-Length: 7"), "{request}");
    assert_eq!(body, b"payload");
    let Reply::Map(fields) = opened else {
        panic!("request_head: {opened:?}");
    };
    assert!(fields.contains(&("status".to_owned(), Reply::Int(200))));
    let headers = fields
        .iter()
        .find(|(key, _)| key == "headers")
        .map(|(_, value)| value)
        .expect("headers");
    let Reply::List(entries) = headers else {
        panic!("headers: {headers:?}");
    };
    assert!(entries.contains(&Reply::Map(vec![
        ("name".to_owned(), Reply::Str("x-split".to_owned())),
        ("value".to_owned(), Reply::Str("yes".to_owned())),
    ])));
    let reader = fields
        .iter()
        .find(|(key, _)| key == "reader")
        .and_then(|(_, value)| match value {
            Reply::Int(reader) => Some(*reader),
            _ => None,
        })
        .expect("reader");
    let chunk = serve_ok("http:read", &[Arg::Int(reader)]);
    let Reply::Map(chunk_fields) = chunk else {
        panic!("read: {chunk:?}");
    };
    let bytes = chunk_fields
        .iter()
        .find(|(key, _)| key == "bytes")
        .and_then(|(_, value)| match value {
            Reply::Bytes(bytes) => Some(bytes.clone()),
            Reply::Str(text) => Some(text.as_bytes().to_vec()),
            _ => None,
        })
        .expect("bytes");
    let done = chunk_fields
        .iter()
        .find(|(key, _)| key == "done")
        .and_then(|(_, value)| match value {
            Reply::Bool(done) => Some(*done),
            _ => None,
        })
        .expect("done");
    assert_eq!(bytes, b"hello");
    assert!(done);
}

/// Read raw bytes up to and including the request head terminator.
fn read_request_head(socket: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buf = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let n = socket.read(&mut buf).expect("read head");
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buf[..n]);
    }
    String::from_utf8_lossy(&request).into_owned()
}

/// Read the declared request body after `head`, including bytes already read.
fn read_request_body(socket: &mut TcpStream, head: &str, want: usize) -> Vec<u8> {
    let header_end = head.find("\r\n\r\n").map_or(head.len(), |index| index + 4);
    let mut body = head.as_bytes()[header_end..].to_vec();
    let mut buf = [0_u8; 1024];
    while body.len() < want {
        let n = socket.read(&mut buf).expect("read body");
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    body.truncate(want);
    body
}

#[test]
fn bracketed_ipv6_authority_keeps_host_and_port_apart() {
    let parts = split_url("http://[::1]:8080/path?q=1").expect("url");
    assert_eq!(parts.host, "[::1]");
    assert_eq!(parts.port, 8080);
    assert_eq!(parts.target, "/path?q=1");
    assert!(!parts.https);

    let literal = split_url("https://[::1]").expect("url");
    assert_eq!(literal.host, "[::1]");
    assert_eq!(literal.port, 443);
    assert_eq!(literal.target, "/");
    assert!(literal.https);

    let named = split_url("http://example.com:8000/p").expect("url");
    assert_eq!(named.host, "example.com");
    assert_eq!(named.port, 8000);

    let bare = split_url("http://example.com/p").expect("url");
    assert_eq!(bare.host, "example.com");
    assert_eq!(bare.port, 80);
}

#[test]
fn route_list_matches_serve() {
    let _guard = crate::test_support::handle_table_guard();
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("http:nope"));
    assert!(serve("http:nope", &[]).is_none());
    reset_handles();
}

fn hostile_chunk_case_server(side_body: &[u8]) -> String {
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
        let error = serve("http:accept", &[Arg::Int(listener)])
            .expect("route")
            .expect_err("hostile chunk size");
        let _ = serve("http:stop", &[Arg::Int(listener)]);
        error.to_string()
    });
    let mut client =
        TcpStream::connect(("127.0.0.1", u16::try_from(port).unwrap())).expect("connect");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    client
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("write timeout");
    client.write_all(side_body).expect("write");
    let message = server.join().expect("server");
    assert!(!message.is_empty());
    message
}

fn hostile_chunk_request(chunk_header: &str) -> String {
    let request = format!(
        "POST /chunked HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n{chunk_header}\r\n"
    );
    hostile_chunk_case_server(request.as_bytes())
}

#[test]
fn server_rejects_overflowing_chunk_size() {
    let _guard = crate::test_support::handle_table_guard();
    let message = hostile_chunk_request("ffffffffffffffff");
    assert!(
        message.contains("chunk") || message.contains("body"),
        "{message}"
    );
}

#[test]
fn server_rejects_huge_chunk_size_without_unbounded_buffering() {
    let _guard = crate::test_support::handle_table_guard();
    let message = hostile_chunk_request("7fffffffffffffff");
    assert!(
        message.contains("chunk") || message.contains("body"),
        "{message}"
    );
}

#[test]
fn server_rejects_non_hex_chunk_size() {
    let _guard = crate::test_support::handle_table_guard();
    let message = hostile_chunk_request("zzzz");
    assert!(message.contains("invalid chunk size"), "{message}");
}

#[test]
fn server_rejects_a_chunk_size_line_that_never_ends() {
    let _guard = crate::test_support::handle_table_guard();
    let mut request =
        b"POST /chunked HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    request.extend_from_slice(&[b'a'; 4096]);
    let message = hostile_chunk_case_server(&request);
    assert!(message.contains("chunk size line too long"), "{message}");
}

fn hostile_chunked_response(chunk_header: &str) -> String {
    let chunk_header = chunk_header.to_owned();
    reset_handles();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = thread::spawn(move || {
        let (mut conn, _) = listener.accept().expect("accept");
        conn.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        conn.set_write_timeout(Some(Duration::from_secs(5)))
            .expect("write timeout");
        let mut request = Vec::new();
        let _ = conn.read(&mut request);
        let response =
            format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{chunk_header}\r\n");
        let _ = conn.write_all(response.as_bytes());
        let _ = conn.flush();
        thread::sleep(std::time::Duration::from_millis(200));
    });
    let url = format!("http://127.0.0.1:{port}/hostile");
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
    let error = serve("http:read", &[Arg::Int(reader), Arg::Int(65_536)])
        .expect("route")
        .expect_err("hostile chunk size");
    server.join().expect("server");
    error.to_string()
}

#[test]
fn client_rejects_overflowing_chunk_size() {
    let _guard = crate::test_support::handle_table_guard();
    let message = hostile_chunked_response("ffffffffffffffff");
    assert!(!message.is_empty(), "{message}");
}

#[test]
fn client_rejects_huge_chunk_size() {
    let _guard = crate::test_support::handle_table_guard();
    let message = hostile_chunked_response("7fffffffffffffff");
    assert!(!message.is_empty(), "{message}");
}

#[test]
fn client_rejects_non_hex_chunk_size() {
    let _guard = crate::test_support::handle_table_guard();
    let message = hostile_chunked_response("zzzz");
    assert!(message.contains("invalid chunk size"), "{message}");
}

/// Serve one raw HTTP exchange: read the request head, then write `response`
/// followed by `body`. Returns the request head text so tests can assert on
/// what the client sent.
fn serve_raw_response(
    listener: TcpListener,
    response: String,
    body: Vec<u8>,
) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut head = Vec::new();
        let mut byte = [0_u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).expect("request head");
            head.push(byte[0]);
        }
        stream
            .write_all(response.as_bytes())
            .expect("write response");
        stream.write_all(&body).expect("write body");
        String::from_utf8_lossy(&head).into_owned()
    })
}

#[test]
fn fetch_follows_redirect_and_returns_binary_body() {
    let payload: Vec<u8> = (0_u8..=255).cycle().take(700).collect();
    let final_listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let final_port = final_listener.local_addr().expect("addr").port();
    let redirect_listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let redirect_port = redirect_listener.local_addr().expect("addr").port();

    let final_response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let final_server = serve_raw_response(final_listener, final_response, payload.clone());
    let redirect_server = serve_raw_response(
        redirect_listener,
        format!(
            "HTTP/1.1 302 Found\r\nLocation: http://localhost:{final_port}/asset\r\n\
             Content-Length: 0\r\nConnection: close\r\n\r\n"
        ),
        Vec::new(),
    );

    let fetched = super::fetch(
        &format!("http://127.0.0.1:{redirect_port}/asset"),
        &[("Authorization".to_owned(), "Bearer token".to_owned())],
        1 << 20,
    )
    .expect("fetch");

    let redirect_head = redirect_server.join().expect("redirect server");
    let final_head = final_server.join().expect("final server");
    assert_eq!(fetched.status, 200);
    assert_eq!(fetched.body, payload, "binary body must survive verbatim");
    assert!(
        redirect_head
            .to_ascii_lowercase()
            .contains("authorization:"),
        "first hop carries credentials: {redirect_head}"
    );
    assert!(
        !final_head.to_ascii_lowercase().contains("authorization:"),
        "cross-host redirect must drop credentials: {final_head}"
    );
}

#[test]
fn fetch_refuses_a_redirect_without_location() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = serve_raw_response(
        listener,
        "HTTP/1.1 302 Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
        Vec::new(),
    );
    let error = super::fetch(&format!("http://127.0.0.1:{port}/asset"), &[], 1 << 20)
        .expect_err("location-less redirect");
    server.join().expect("server");
    assert!(error.to_string().contains("Location"), "{error}");
}
