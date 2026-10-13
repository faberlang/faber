//! N5 proof: `faber-leaf` serves the `http:*` raw routes that
//! `norma:http/server` calls (`listen`, `accept`, `respond`, `respond_open`,
//! `respond_chunk`, `respond_finish`, `stop`) on the MIR runner. The programs
//! use the exact `call 'http:*'` shapes and carrier handling of
//! `norma/src/http/server.fab` (`incoming_of`, `reply`, `open_sse`,
//! `send_event`, `finish_stream`) and a `std::thread` client plays the peer.
//! The `norma:http/server` import itself is proved end to end by running
//! `faber run` on a package (library linking lives in the faber crate).

use super::{BufferHost, RunSourceError, RunnerError, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

fn run_host(name: &str, source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    match run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        name,
        source,
        host,
    ) {
        Ok(()) => Ok(()),
        Err(RunSourceError::Stepper(errors)) => Err(errors),
        Err(error) => panic!("{name} should reach the MIR runner: {error:?}"),
    }
}

fn program(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n")
}

/// Reserve a loopback port the program can bind (the donor tests' pattern).
fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("reserve loopback port")
        .local_addr()
        .expect("reserved address")
        .port()
}

fn connect(port: u16) -> TcpStream {
    let give_up = Instant::now() + Duration::from_secs(10);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => return stream,
            Err(error) if Instant::now() >= give_up => panic!("connect {port}: {error}"),
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// Send `request` once the program listens; read the reply until close.
fn client(port: u16, request: &'static [u8]) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut stream = connect(port);
        stream.write_all(request).expect("send request");
        let mut reply = Vec::new();
        drop(stream.read_to_end(&mut reply));
        String::from_utf8_lossy(&reply).into_owned()
    })
}

/// The listen/accept prelude: `bind`, then `incoming_of` on the raw carrier.
fn serve_prelude(listen_args: &str) -> String {
    format!(
        "  const list<value> opts ← [{listen_args}]\n  \
         const int handle ← call 'http:listen' (opts) ↦ int\n  \
         const value raw ← call 'http:accept' (handle) ↦ value\n  \
         const map<string, value> fields ↤ raw\n  \
         const string id ← fields[\"id\"] ↦ string\n  \
         const string method ← fields[\"method\"] ↦ string\n  \
         const string path ← fields[\"path\"] ↦ string\n  \
         const int connection ← fields[\"connection\"] ↦ int\n"
    )
}

#[test]
fn a_thread_client_gets_200_from_health() {
    let port = free_port();
    let body = format!(
        "{}  const list<value> headers ← []\n  \
         const list<value> args ← [id ↦ value, 200 ↦ value, headers ↦ value, \"ok \" + method + \" \" + path ↦ value]\n  \
         const void r1 ← call 'http:respond' (args) ↦ void\n  \
         const void r2 ← call 'http:stop' (handle) ↦ void\n  \
         print connection",
        serve_prelude(&format!("{port} ↦ value"))
    );
    let reply = client(port, b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let mut host = BufferHost::default();
    run_host("http-health.fab", &program(&body), &mut host)
        .unwrap_or_else(|errors| panic!("health program should run: {errors:?}"));
    let reply = reply.join().expect("client");
    assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
    assert!(reply.ends_with("\r\n\r\nok GET /health"), "{reply}");
    // WHY: the leaf numbers connections from a process-global counter, so the
    // printed id depends on which sibling test accepted first.
    let [connection] = host.stdout_lines.as_slice() else {
        panic!("expected one printed connection id: {:?}", host.stdout_lines);
    };
    assert!(
        connection.parse::<i64>().is_ok_and(|id| id >= 1),
        "connection id must be a positive integer: {connection}"
    );
}

#[test]
fn sse_events_are_chunk_framed_through_open_chunk_finish() {
    let port = free_port();
    let body = format!(
        "{}  const list<value> headers ← []\n  \
         const list<value> open ← [id ↦ value, 200 ↦ value, headers ↦ value]\n  \
         const int writer ← call 'http:respond_open' (open) ↦ int\n  \
         const list<value> first ← [writer ↦ value, \"data: one\\n\\n\" ↦ bytes ↦ value]\n  \
         const void r3 ← call 'http:respond_chunk' (first) ↦ void\n  \
         const list<value> second ← [writer ↦ value, \"data: two\\n\\n\" ↦ bytes ↦ value]\n  \
         const void r4 ← call 'http:respond_chunk' (second) ↦ void\n  \
         const list<value> done ← [writer ↦ value, false ↦ value]\n  \
         const void r5 ← call 'http:respond_finish' (done) ↦ void\n  \
         const void r6 ← call 'http:stop' (handle) ↦ void",
        serve_prelude(&format!("{port} ↦ value"))
    );
    let reply = client(port, b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let mut host = BufferHost::default();
    run_host("http-sse.fab", &program(&body), &mut host)
        .unwrap_or_else(|errors| panic!("sse program should run: {errors:?}"));
    let reply = reply.join().expect("client");
    assert!(
        reply.starts_with("HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n"),
        "{reply}"
    );
    let (_, chunks) = reply.split_once("\r\n\r\n").expect("body after head");
    assert_eq!(chunks, "b\r\ndata: one\n\n\r\nb\r\ndata: two\n\n\r\n0\r\n\r\n");
}

#[test]
fn bind_limited_refuses_an_oversize_body() {
    const POST: &[u8] = b"POST /big HTTP/1.1\r\nHost: localhost\r\nContent-Length: 9\r\n\r\n123456789";
    let port = free_port();
    let body = format!(
        "{}  print \"unreachable\"",
        serve_prelude(&format!("{port} ↦ value, 4 ↦ value"))
    );
    let reply = client(port, POST);
    let mut host = BufferHost::default();
    let errors =
        run_host("http-limit.fab", &program(&body), &mut host).expect_err("accept must refuse the oversize body");
    assert_eq!(errors[0].issue, "runner_sermo_reply_conversion_failed", "{errors:?}");
    assert!(host.stdout_lines.is_empty());
    assert_eq!(reply.join().expect("client"), "", "a refused body gets no response");

    // WHY: the runner error carries only the leaf's frame text, not its kind or
    // the limit, so the ceiling is proved the cause by contrast: the same
    // request is served once the ceiling fits the body.
    let port = free_port();
    let body = format!(
        "{}  const list<value> headers ← []\n  \
         const list<value> args ← [id ↦ value, 200 ↦ value, headers ↦ value, \"fits\" ↦ value]\n  \
         const void r1 ← call 'http:respond' (args) ↦ void\n  \
         const void r2 ← call 'http:stop' (handle) ↦ void",
        serve_prelude(&format!("{port} ↦ value, 16 ↦ value"))
    );
    let reply = client(port, POST);
    let mut host = BufferHost::default();
    run_host("http-limit-fits.fab", &program(&body), &mut host)
        .unwrap_or_else(|errors| panic!("a body within the ceiling should be served: {errors:?}"));
    let reply = reply.join().expect("client");
    assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
    assert!(reply.ends_with("\r\n\r\nfits"), "{reply}");
}
