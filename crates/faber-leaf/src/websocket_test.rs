//! WebSocket leaf: HTTP upgrade, connect, echo.

use super::{empty_text_frame, is_route, reset_handles, serve, split_ws_url, wrap_tls};
use crate::http;
use crate::value::{Arg, Reply};
use std::thread;

fn serve_ok(route: &str, args: &[Arg]) -> Reply {
    serve(route, args)
        .expect("websocket route")
        .unwrap_or_else(|err| panic!("{route}: {err}"))
}

#[test]
fn empty_text_frame_carries_text_and_done() {
    assert_eq!(
        empty_text_frame(false),
        Reply::Map(vec![
            ("text".to_owned(), Reply::Str(String::new())),
            ("done".to_owned(), Reply::Bool(false)),
        ])
    );
    assert_eq!(
        empty_text_frame(true),
        Reply::Map(vec![
            ("text".to_owned(), Reply::Str(String::new())),
            ("done".to_owned(), Reply::Bool(true)),
        ])
    );
}

fn http_ok(route: &str, args: &[Arg]) -> Reply {
    http::serve(route, args)
        .expect("http route")
        .unwrap_or_else(|err| panic!("{route}: {err}"))
}

#[test]
fn echo_round_trips() {
    let _guard = crate::test_support::handle_table_guard();
    reset_handles();
    http::reset_handles();
    let listener = match http_ok("http:listen", &[Arg::Int(0)]) {
        Reply::Int(id) => id,
        other => panic!("listen: {other:?}"),
    };
    let port = match http_ok("http:port", &[Arg::Int(listener)]) {
        Reply::Int(port) => port,
        other => panic!("port: {other:?}"),
    };
    let server = thread::spawn(move || {
        let incoming = http_ok("http:accept", &[Arg::Int(listener)]);
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
        let socket = match serve_ok("websocket:accept", &[Arg::Str(id)]) {
            Reply::Int(id) => id,
            other => panic!("accept ws: {other:?}"),
        };
        let got = serve_ok("websocket:receive", &[Arg::Int(socket)]);
        let Reply::Map(fields) = got else {
            panic!("receive: {got:?}");
        };
        let text = fields
            .iter()
            .find(|(key, _)| key == "text")
            .and_then(|(_, value)| match value {
                Reply::Str(text) => Some(text.clone()),
                _ => None,
            })
            .expect("text");
        serve_ok("websocket:send", &[Arg::Int(socket), Arg::Str(text)]);
        serve_ok("websocket:close", &[Arg::Int(socket)]);
        http_ok("http:stop", &[Arg::Int(listener)]);
    });
    let url = format!("ws://127.0.0.1:{port}/ws");
    let client = match serve_ok("websocket:connect", &[Arg::Str(url)]) {
        Reply::Int(id) => id,
        other => panic!("connect: {other:?}"),
    };
    serve_ok(
        "websocket:send",
        &[Arg::Int(client), Arg::Str("ping".to_owned())],
    );
    let got = serve_ok("websocket:receive", &[Arg::Int(client)]);
    serve_ok("websocket:close", &[Arg::Int(client)]);
    server.join().expect("server");
    let Reply::Map(fields) = got else {
        panic!("client receive: {got:?}");
    };
    assert!(fields.contains(&("text".to_owned(), Reply::Str("ping".to_owned()))));
}

#[test]
fn route_list_matches_serve() {
    let _guard = crate::test_support::handle_table_guard();
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("websocket:nope"));
    assert!(serve("websocket:nope", &[]).is_none());
}

#[test]
fn split_ws_url_ipv6_literal_without_port_uses_default_port() {
    let (secure, host, port, target) = split_ws_url("ws://[::1]").unwrap();
    assert!(!secure);
    assert_eq!(host, "[::1]");
    assert_eq!(port, 80);
    assert_eq!(target, "/");
}

#[test]
fn split_ws_url_ipv6_literal_with_port_and_path() {
    let (secure, host, port, target) = split_ws_url("ws://[::1]:8080/chat?room=1").unwrap();
    assert!(!secure);
    assert_eq!(host, "[::1]");
    assert_eq!(port, 8080);
    assert_eq!(target, "/chat?room=1");
}

#[test]
fn split_ws_url_wss_ipv6_literal_defaults_to_443() {
    let (secure, host, port, target) = split_ws_url("wss://[2001:db8::1]/").unwrap();
    assert!(secure);
    assert_eq!(host, "[2001:db8::1]");
    assert_eq!(port, 443);
    assert_eq!(target, "/");
}

#[test]
fn split_ws_url_ipv6_literal_tls_server_name_drops_brackets() {
    let _guard = crate::test_support::handle_table_guard();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    assert!(wrap_tls(stream, "[::1]", None).is_ok());
}
