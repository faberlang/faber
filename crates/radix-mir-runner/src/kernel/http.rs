//! HTTP/1.1 listener routes for `norma:http/server` (`http:*` raw routes).
//!
//! The runner is single-threaded, so the listener, connection, request and
//! writer tables live in one thread-local [`State`]: `http:accept` blocks the
//! runner thread (polling nonblocking sockets) until a request is ready, and a
//! keep-alive connection is parked `Idle` between requests and re-polled by the
//! next `accept`. Source is copied from `hosts/crates/http` (hosts is retiring;
//! nothing here depends on it), minus the cross-thread machinery (`Arc`,
//! `Mutex`, cancellation, the second `listen(2)` backlog call).
//!
//! | route                 | opener                                              | reply   |
//! | --------------------- | --------------------------------------------------- | ------- |
//! | `http:listen`         | `[port]`, `[port, host]`, `[port, max_body]`, `[port, max_body, host]`, `[.., header_body_ms, idle_ms]` | handle |
//! | `http:accept`         | `handle` or `[handle]`                              | request carrier |
//! | `http:respond`        | `[request_id, status, headers, body]`               | none    |
//! | `http:respond_open`   | `[request_id, status, headers]`                     | writer  |
//! | `http:respond_chunk`  | `[writer, bytes]`                                   | none    |
//! | `http:respond_finish` | `[writer, keep_alive]`                              | none    |
//! | `http:stop`           | `handle` or `[handle]`                              | none    |
//!
//! The request carrier is a `map<textus, valor>` with `id`, `method`, `path`,
//! `headers` (a list of `{name, value}` maps), `body` and `connection`. The
//! body is a `textus` when it is valid UTF-8 (the only form `norma:http/server`
//! `body_text` decodes) and `octeti` otherwise. Outbound bodies and chunks may
//! be `octeti`, `bytes` (a list of byte numeri, what norma's `↦ bytes`
//! produces) or `textus`.

use super::super::value::MapKey;
use super::super::{RunnerError, Value};
use radix_lexer::Span;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;
const DEFAULT_BIND_HOST: &str = "127.0.0.1";
const MAX_CONFIGURED_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// The window an incomplete request header/body read may span.
const DEFAULT_HEADER_BODY_DEADLINE: Duration = Duration::from_secs(5);
/// The window a parked idle keep-alive connection may rest between requests.
const DEFAULT_IDLE_DEADLINE: Duration = Duration::from_secs(75);
const MAX_CONFIGURED_DEADLINE_MS: u64 = 86_400_000;
const WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const STREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_ID_PREFIX: &str = "http-";
const LAST_CHUNK: &[u8] = b"0\r\n\r\n";

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::new());
}

/// Drop every listener and connection and restart the id counters.
pub(crate) fn reset() {
    STATE.with(|state| *state.borrow_mut() = State::new());
}

/// Answer an `http:*` route, or `None` when the route is not served here.
pub(crate) fn serve(route: &str, opener: &Value, span: Span) -> Option<Result<Value, RunnerError>> {
    let verb = route.strip_prefix("http:")?;
    if !matches!(
        verb,
        "listen"
            | "accept"
            | "respond"
            | "respond_open"
            | "respond_chunk"
            | "respond_finish"
            | "stop"
    ) {
        return None;
    }
    let args = opener_args(opener);
    let result = STATE
        .with(|state| dispatch(&mut state.borrow_mut(), route, &args))
        .map_err(|message| RunnerError::internal(span, message));
    Some(result)
}

/// The bound port of a listener, for in-process readback.
#[cfg(test)]
pub(crate) fn local_port(handle: i64) -> Option<u16> {
    STATE.with(|state| {
        let state = state.borrow();
        let listener = state.listeners.get(&handle)?;
        listener.socket.local_addr().ok().map(|addr| addr.port())
    })
}

type Fault = String;

fn fault(message: impl Into<String>) -> Fault {
    message.into()
}

fn opener_args(opener: &Value) -> Vec<Value> {
    match opener {
        Value::Array(items) => items.borrow().clone(),
        other => vec![other.clone()],
    }
}

fn dispatch(state: &mut State, route: &str, args: &[Value]) -> Result<Value, Fault> {
    match route {
        "http:listen" => state.listen(args),
        "http:accept" => state.accept(int_arg(args.first(), "listener")?),
        "http:respond" => state.respond(args),
        "http:respond_open" => state.respond_open(args),
        "http:respond_chunk" => state.respond_chunk(args),
        "http:respond_finish" => state.respond_finish(args),
        _ => state.stop(int_arg(args.first(), "listener")?),
    }
}

struct Listener {
    socket: TcpListener,
    max_body_bytes: usize,
    deadlines: Deadlines,
}

/// One deadline mechanism, two magnitudes: the short `header_body` window
/// bounds an incomplete request read; the generous `idle` window bounds a
/// parked keep-alive connection between requests.
#[derive(Clone, Copy)]
struct Deadlines {
    header_body: Duration,
    idle: Duration,
}

struct Conn {
    listener: i64,
    stream: TcpStream,
    leftover: Vec<u8>,
    phase: Phase,
    idle_since: Instant,
    read_started: Option<Instant>,
}

enum Phase {
    Idle,
    Pending { request_id: String },
    Streaming { request_id: String, writer: i64 },
}

struct Parts {
    id: String,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct ParsedHeaders {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    content_length: usize,
}

enum Poll {
    Ready(Parts),
    Idle,
    Closed,
    Expired,
}

struct State {
    listeners: HashMap<i64, Listener>,
    conns: HashMap<i64, Conn>,
    requests: HashMap<String, i64>,
    writers: HashMap<i64, i64>,
    next_listener: i64,
    next_request: u64,
    next_conn: i64,
    next_writer: i64,
}

impl State {
    fn new() -> Self {
        Self {
            listeners: HashMap::new(),
            conns: HashMap::new(),
            requests: HashMap::new(),
            writers: HashMap::new(),
            next_listener: 1,
            next_request: 1,
            next_conn: 1,
            next_writer: 1,
        }
    }

    fn listen(&mut self, values: &[Value]) -> Result<Value, Fault> {
        if !(1..=5).contains(&values.len()) {
            return Err(fault(
                "http:listen requires [port], [port, max_body_bytes], [port, max_body_bytes, bind_host], [port, max_body_bytes, bind_host, header_body_deadline_ms], or [port, max_body_bytes, bind_host, header_body_deadline_ms, idle_deadline_ms]",
            ));
        }
        let port = u16::try_from(int_value(&values[0], "port")?)
            .map_err(|_| fault("http:listen port must be between 0 and 65535"))?;
        let (max_body_bytes, bind_host, deadlines) = match values.len() {
            1 => (
                DEFAULT_MAX_BODY_BYTES,
                DEFAULT_BIND_HOST.to_owned(),
                default_deadlines(),
            ),
            2 => match &values[1] {
                Value::Str(host) => (
                    DEFAULT_MAX_BODY_BYTES,
                    parse_bind_host(host)?,
                    default_deadlines(),
                ),
                other => (
                    bounded_body_size(int_value(other, "max_body_bytes")?)?,
                    DEFAULT_BIND_HOST.to_owned(),
                    default_deadlines(),
                ),
            },
            3 => (
                bounded_body_size(int_value(&values[1], "max_body_bytes")?)?,
                parse_bind_host(&text_value(&values[2], "bind_host")?)?,
                default_deadlines(),
            ),
            _ => (
                bounded_body_size(int_value(&values[1], "max_body_bytes")?)?,
                parse_bind_host(&text_value(&values[2], "bind_host")?)?,
                Deadlines {
                    header_body: deadline_arg(
                        values.get(3),
                        "header_body_deadline_ms",
                        DEFAULT_HEADER_BODY_DEADLINE,
                    )?,
                    idle: deadline_arg(values.get(4), "idle_deadline_ms", DEFAULT_IDLE_DEADLINE)?,
                },
            ),
        };
        let socket = TcpListener::bind((bind_host.as_str(), port))
            .map_err(|error| fault(format!("http:listen bind failed: {error}")))?;
        socket
            .set_nonblocking(true)
            .map_err(|error| fault(format!("http:listen setup failed: {error}")))?;
        let handle = self.next_listener;
        self.next_listener += 1;
        self.listeners.insert(
            handle,
            Listener {
                socket,
                max_body_bytes,
                deadlines,
            },
        );
        Ok(Value::Int(handle))
    }

    fn accept(&mut self, handle: i64) -> Result<Value, Fault> {
        if !self.listeners.contains_key(&handle) {
            return Err(fault(format!("http:accept unknown listener {handle}")));
        }
        loop {
            if let Some(carrier) = self.poll_idle(handle)? {
                return Ok(carrier);
            }
            let accepted = match self.listeners.get(&handle) {
                Some(listener) => listener.socket.accept(),
                None => return Err(fault(format!("http:accept unknown listener {handle}"))),
            };
            match accepted {
                Ok((stream, _peer)) => {
                    // A deadline-reaped admission returns None; the loop goes
                    // back to serving other clients.
                    if let Some(carrier) = self.admit(handle, stream)? {
                        return Ok(carrier);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(POLL_INTERVAL);
                }
                Err(error) => return Err(fault(format!("http:accept failed: {error}"))),
            }
        }
    }

    fn admit(&mut self, handle: i64, stream: TcpStream) -> Result<Option<Value>, Fault> {
        let (max_body_bytes, deadlines) = match self.listeners.get(&handle) {
            Some(listener) => (listener.max_body_bytes, listener.deadlines),
            None => return Err(fault(format!("http:accept unknown listener {handle}"))),
        };
        let mut conn = Conn {
            listener: handle,
            stream,
            leftover: Vec::new(),
            phase: Phase::Idle,
            idle_since: Instant::now(),
            read_started: None,
        };
        let Some(parts) = self.read_request(&mut conn, max_body_bytes, deadlines)? else {
            close_socket(&mut conn.stream);
            return Ok(None);
        };
        let conn_id = self.next_conn;
        self.next_conn += 1;
        conn.phase = Phase::Pending {
            request_id: parts.id.clone(),
        };
        self.requests.insert(parts.id.clone(), conn_id);
        self.conns.insert(conn_id, conn);
        Ok(Some(request_carrier(&parts, conn_id)))
    }

    fn read_request(
        &mut self,
        conn: &mut Conn,
        max_body_bytes: usize,
        deadlines: Deadlines,
    ) -> Result<Option<Parts>, Fault> {
        let deadline = Instant::now() + deadlines.header_body;
        loop {
            match poll_request(
                conn,
                max_body_bytes,
                &mut self.next_request,
                true,
                Some(deadline),
            )? {
                Poll::Ready(parts) => return Ok(Some(parts)),
                Poll::Expired => return Ok(None),
                Poll::Idle => thread::sleep(POLL_INTERVAL),
                Poll::Closed => return Err(fault("http:accept request ended before headers")),
            }
        }
    }

    /// Poll the parked keep-alive connections of one listener for a next
    /// request; reap those whose deadline closed.
    fn poll_idle(&mut self, handle: i64) -> Result<Option<Value>, Fault> {
        let Some(listener) = self.listeners.get(&handle) else {
            return Err(fault(format!("http:accept unknown listener {handle}")));
        };
        let (max_body_bytes, deadlines) = (listener.max_body_bytes, listener.deadlines);
        let mut idle: Vec<i64> = self
            .conns
            .iter()
            .filter(|(_, conn)| conn.listener == handle && matches!(conn.phase, Phase::Idle))
            .map(|(id, _)| *id)
            .collect();
        idle.sort_unstable();
        let mut closed = Vec::new();
        for conn_id in idle {
            let Some(conn) = self.conns.get_mut(&conn_id) else {
                continue;
            };
            let polled = poll_request(conn, max_body_bytes, &mut self.next_request, false, None);
            let outcome = match polled {
                Ok(outcome) => outcome,
                Err(error) => {
                    // A malformed pipelined request would fail every later
                    // accept: drop the connection, then surface the error.
                    self.drop_conn(conn_id);
                    return Err(error);
                }
            };
            let expired =
                matches!(outcome, Poll::Idle | Poll::Closed) && deadlines_expired(&deadlines, conn);
            match outcome {
                Poll::Ready(parts) => {
                    conn.phase = Phase::Pending {
                        request_id: parts.id.clone(),
                    };
                    self.requests.insert(parts.id.clone(), conn_id);
                    return Ok(Some(request_carrier(&parts, conn_id)));
                }
                Poll::Closed => closed.push(conn_id),
                Poll::Idle if expired => closed.push(conn_id),
                Poll::Idle | Poll::Expired => {}
            }
        }
        for conn_id in closed {
            self.drop_conn(conn_id);
        }
        Ok(None)
    }

    fn respond(&mut self, values: &[Value]) -> Result<Value, Fault> {
        let [request_id, status, headers, body] = values else {
            return Err(fault(
                "http:respond requires [request_id, status, headers, body]",
            ));
        };
        let request_id = text_value(request_id, "request_id")?;
        let status = status_value(status, "http:respond")?;
        let headers = response_headers(headers)?;
        let body = bytes_value(body, "body")?;
        let conn_id = self.take_request(&request_id)?;
        let Some(mut conn) = self.conns.remove(&conn_id) else {
            return Err(fault(format!("http unknown connection {conn_id}")));
        };
        match &conn.phase {
            Phase::Pending {
                request_id: pending,
            } if pending == &request_id => {}
            _ => return Err(fault(format!("http:respond unknown request {request_id}"))),
        }
        conn.phase = Phase::Idle;
        let written = write_blocking(
            &mut conn.stream,
            &format_response(status, &request_id, &headers, &body),
        );
        close_socket(&mut conn.stream);
        written?;
        Ok(Value::Unit)
    }

    fn respond_open(&mut self, values: &[Value]) -> Result<Value, Fault> {
        let [request_id, status, headers] = values else {
            return Err(fault(
                "http:respond_open requires [request_id, status, headers]",
            ));
        };
        let request_id = text_value(request_id, "request_id")?;
        let status = status_value(status, "http:respond_open")?;
        let headers = response_headers(headers)?;
        let conn_id = self.take_request(&request_id)?;
        let writer = self.next_writer;
        let Some(conn) = self.conns.get_mut(&conn_id) else {
            return Err(fault(format!("http unknown connection {conn_id}")));
        };
        match &conn.phase {
            Phase::Pending {
                request_id: pending,
            } if pending == &request_id => {}
            _ => {
                self.requests.insert(request_id.clone(), conn_id);
                return Err(fault(format!(
                    "http:respond_open unknown request {request_id}"
                )));
            }
        }
        let head = format_chunked_open(status, &request_id, &headers);
        if let Err(error) = write_blocking(&mut conn.stream, &head) {
            self.drop_conn(conn_id);
            return Err(error);
        }
        conn.phase = Phase::Streaming { request_id, writer };
        self.next_writer += 1;
        self.writers.insert(writer, conn_id);
        Ok(Value::Int(writer))
    }

    fn respond_chunk(&mut self, values: &[Value]) -> Result<Value, Fault> {
        let [writer, data] = values else {
            return Err(fault("http:respond_chunk requires [writer, bytes]"));
        };
        let writer = int_value(writer, "writer")?;
        let bytes = bytes_value(data, "bytes")?;
        if bytes.is_empty() {
            return Err(fault("http:respond_chunk bytes must not be empty"));
        }
        let Some(&conn_id) = self.writers.get(&writer) else {
            return Err(fault(format!("http:respond_chunk unknown writer {writer}")));
        };
        let Some(conn) = self.conns.get_mut(&conn_id) else {
            return Err(fault(format!("http unknown connection {conn_id}")));
        };
        match conn.phase {
            Phase::Streaming { writer: open, .. } if open == writer => {}
            _ => return Err(fault(format!("http:respond_chunk unknown writer {writer}"))),
        }
        write_backpressure(&mut conn.stream, &format_chunk(&bytes))?;
        Ok(Value::Unit)
    }

    fn respond_finish(&mut self, values: &[Value]) -> Result<Value, Fault> {
        let [writer, keep_alive] = values else {
            return Err(fault("http:respond_finish requires [writer, keep_alive]"));
        };
        let writer = int_value(writer, "writer")?;
        let Value::Bool(keep_alive) = keep_alive else {
            return Err(fault("keep_alive must be bivalens"));
        };
        let Some(conn_id) = self.writers.remove(&writer) else {
            return Err(fault(format!(
                "http:respond_finish unknown writer {writer}"
            )));
        };
        let Some(conn) = self.conns.get_mut(&conn_id) else {
            return Err(fault(format!("http unknown connection {conn_id}")));
        };
        match conn.phase {
            Phase::Streaming { writer: open, .. } if open == writer => {}
            _ => {
                return Err(fault(format!(
                    "http:respond_finish unknown writer {writer}"
                )));
            }
        }
        write_backpressure(&mut conn.stream, LAST_CHUNK)?;
        if *keep_alive {
            conn.phase = Phase::Idle;
            conn.idle_since = Instant::now();
            conn.stream
                .set_nonblocking(true)
                .map_err(|error| fault(format!("http:respond_finish setup failed: {error}")))?;
        } else {
            self.drop_conn(conn_id);
        }
        Ok(Value::Unit)
    }

    fn stop(&mut self, handle: i64) -> Result<Value, Fault> {
        if self.listeners.remove(&handle).is_none() {
            return Err(fault(format!("http:stop unknown listener {handle}")));
        }
        let ids: Vec<i64> = self
            .conns
            .iter()
            .filter(|(_, conn)| conn.listener == handle)
            .map(|(id, _)| *id)
            .collect();
        for conn_id in ids {
            self.drop_conn(conn_id);
        }
        Ok(Value::Unit)
    }

    fn take_request(&mut self, request_id: &str) -> Result<i64, Fault> {
        self.requests
            .remove(request_id)
            .ok_or_else(|| fault(format!("http unknown request {request_id}")))
    }

    /// Close a connection and forget its request/writer index entries.
    fn drop_conn(&mut self, conn_id: i64) {
        let Some(mut conn) = self.conns.remove(&conn_id) else {
            return;
        };
        close_socket(&mut conn.stream);
        match conn.phase {
            Phase::Pending { request_id } => {
                self.requests.remove(&request_id);
            }
            Phase::Streaming { request_id, writer } => {
                self.requests.remove(&request_id);
                self.writers.remove(&writer);
            }
            Phase::Idle => {}
        }
    }
}

fn default_deadlines() -> Deadlines {
    Deadlines {
        header_body: DEFAULT_HEADER_BODY_DEADLINE,
        idle: DEFAULT_IDLE_DEADLINE,
    }
}

/// Whether the connection's current window closed: the short header/body
/// window while a partial request is in flight, otherwise the idle window.
fn deadlines_expired(deadlines: &Deadlines, conn: &Conn) -> bool {
    let now = Instant::now();
    match conn.read_started {
        Some(started) => now >= started + deadlines.header_body,
        None => now >= conn.idle_since + deadlines.idle,
    }
}

fn poll_request(
    conn: &mut Conn,
    max_body_bytes: usize,
    next_request: &mut u64,
    block: bool,
    deadline: Option<Instant>,
) -> Result<Poll, Fault> {
    conn.stream
        .set_nonblocking(true)
        .map_err(|error| fault(format!("http:accept setup failed: {error}")))?;
    loop {
        if let Some(end) = find_header_end(&conn.leftover) {
            let ParsedHeaders {
                method,
                path,
                headers,
                content_length,
            } = parse_headers(&conn.leftover[..end])?;
            if content_length > max_body_bytes {
                return Err(fault(format!(
                    "http:accept request body exceeds max_body_bytes {max_body_bytes}"
                )));
            }
            let body_start = end + 4;
            let needed = body_start + content_length;
            if conn.leftover.len() >= needed {
                let body = conn.leftover[body_start..needed].to_vec();
                conn.leftover.drain(..needed);
                conn.idle_since = Instant::now();
                conn.read_started = (!conn.leftover.is_empty()).then(Instant::now);
                let id = format!("{REQUEST_ID_PREFIX}{next_request}");
                *next_request += 1;
                return Ok(Poll::Ready(Parts {
                    id,
                    method,
                    path,
                    headers,
                    body,
                }));
            }
        } else if conn.leftover.len() > MAX_HEADER_BYTES {
            return Err(fault("http:accept request headers exceed limit"));
        }
        let mut chunk = [0_u8; 4096];
        match conn.stream.read(&mut chunk) {
            Ok(0) => {
                if conn.leftover.is_empty() {
                    return Ok(Poll::Closed);
                }
                return Err(fault("http:accept request ended before headers"));
            }
            Ok(count) => {
                if conn.read_started.is_none() {
                    conn.read_started = Some(Instant::now());
                }
                conn.leftover.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if block {
                    if deadline.is_some_and(|at| Instant::now() >= at) {
                        return Ok(Poll::Expired);
                    }
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }
                return Ok(Poll::Idle);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(fault(format!("http:accept read failed: {error}"))),
        }
    }
}

fn parse_headers(bytes: &[u8]) -> Result<ParsedHeaders, Fault> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| fault("http:accept headers are not UTF-8"))?;
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| fault("http:accept request line is missing"))?;
    let mut request_parts = request_line.split_ascii_whitespace();
    let method = request_parts
        .next()
        .filter(|method| !method.is_empty() && method.bytes().all(is_token_byte))
        .ok_or_else(|| fault("http:accept request method is malformed"))?;
    let path = request_parts
        .next()
        .filter(|path| !path.is_empty() && !path.contains(['\r', '\n']))
        .ok_or_else(|| fault("http:accept request path is malformed"))?;
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(fault("http:accept requires HTTP/1.1"));
    }
    let mut headers = Vec::new();
    let mut content_length = None;
    let mut has_host = false;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| fault("http:accept header is malformed"))?;
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || !name.bytes().all(is_token_byte) || !is_valid_header_value(value) {
            return Err(fault("http:accept header is malformed"));
        }
        if name.eq_ignore_ascii_case("host") {
            has_host = true;
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(fault("http:accept transfer-encoding is unsupported"));
        }
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| fault("http:accept content-length is malformed"))?;
            if content_length.replace(parsed).is_some() {
                return Err(fault("http:accept repeats content-length"));
            }
        }
        headers.push((name.to_owned(), value.to_owned()));
    }
    if !has_host {
        return Err(fault("http:accept HTTP/1.1 request requires a Host header"));
    }
    Ok(ParsedHeaders {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        content_length: content_length.unwrap_or(0),
    })
}

fn map_value(fields: Vec<(&str, Value)>) -> Value {
    let mut map = FxHashMap::default();
    for (name, value) in fields {
        map.insert(MapKey::Str(name.to_owned()), value);
    }
    Value::Map(Rc::new(RefCell::new(map)))
}

fn request_carrier(parts: &Parts, conn_id: i64) -> Value {
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| {
            map_value(vec![
                ("name", Value::Str(name.clone())),
                ("value", Value::Str(value.clone())),
            ])
        })
        .collect();
    let body = match String::from_utf8(parts.body.clone()) {
        Ok(text) => Value::Str(text),
        Err(_) => Value::Octeti(parts.body.clone()),
    };
    map_value(vec![
        ("id", Value::Str(parts.id.clone())),
        ("method", Value::Str(parts.method.clone())),
        ("path", Value::Str(parts.path.clone())),
        ("headers", Value::Array(Rc::new(RefCell::new(headers)))),
        ("body", body),
        ("connection", Value::Int(conn_id)),
    ])
}

fn response_headers(value: &Value) -> Result<Vec<(String, String)>, Fault> {
    let Value::Array(items) = value else {
        return Err(fault("http:respond headers must be a list"));
    };
    items.borrow().iter().map(header_pair).collect()
}

fn header_pair(value: &Value) -> Result<(String, String), Fault> {
    let Value::Map(fields) = value else {
        return Err(fault("HTTP headers must be tables with name and value"));
    };
    let fields = fields.borrow();
    let text = |name: &str| match fields.get(&MapKey::Str(name.to_owned())) {
        Some(Value::Str(text)) => Ok(text.clone()),
        Some(_) => Err(fault(format!("{name} must be textus"))),
        None => Err(fault(format!("HTTP header field `{name}` is missing"))),
    };
    let (name, value) = (text("name")?, text("value")?);
    if name.is_empty() || !name.bytes().all(is_token_byte) || !is_valid_header_value(&value) {
        return Err(fault("HTTP header name or value is malformed"));
    }
    if name.eq_ignore_ascii_case("connection")
        || name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("transfer-encoding")
    {
        return Err(fault(
            "HTTP connection, content-length, and transfer-encoding headers are provider-owned",
        ));
    }
    Ok((name, value))
}

fn format_response(
    status: u16,
    request_id: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status} {}\r\nconnection: close\r\nx-faber-request-id: {request_id}\r\ncontent-length: {}\r\n",
        reason_phrase(status),
        body.len()
    )
    .into_bytes();
    append_headers(&mut response, headers);
    response.extend_from_slice(body);
    response
}

fn format_chunked_open(status: u16, request_id: &str, headers: &[(String, String)]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status} {}\r\ntransfer-encoding: chunked\r\nx-faber-request-id: {request_id}\r\n",
        reason_phrase(status),
    )
    .into_bytes();
    append_headers(&mut response, headers);
    response
}

fn append_headers(response: &mut Vec<u8>, headers: &[(String, String)]) {
    for (name, value) in headers {
        response.extend_from_slice(name.as_bytes());
        response.extend_from_slice(b": ");
        response.extend_from_slice(value.as_bytes());
        response.extend_from_slice(b"\r\n");
    }
    response.extend_from_slice(b"\r\n");
}

fn format_chunk(data: &[u8]) -> Vec<u8> {
    let mut frame = format!("{:x}\r\n", data.len()).into_bytes();
    frame.extend_from_slice(data);
    frame.extend_from_slice(b"\r\n");
    frame
}

/// Best-effort close of both halves of a connection socket.
fn close_socket(stream: &mut TcpStream) {
    drop(stream.shutdown(Shutdown::Both));
}

fn write_blocking(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), Fault> {
    stream
        .set_nonblocking(false)
        .map_err(|error| fault(format!("http write setup failed: {error}")))?;
    stream
        .set_write_timeout(Some(WRITE_TIMEOUT))
        .map_err(|error| fault(format!("http write timeout setup failed: {error}")))?;
    stream
        .write_all(bytes)
        .map_err(|error| fault(format!("http write failed: {error}")))
}

/// Write all of `bytes` to a nonblocking stream, waiting out a stalled peer
/// for up to [`STREAM_WRITE_TIMEOUT`] (backpressure).
fn write_backpressure(stream: &mut TcpStream, mut bytes: &[u8]) -> Result<(), Fault> {
    stream
        .set_nonblocking(true)
        .map_err(|error| fault(format!("http stream write setup failed: {error}")))?;
    let deadline = Instant::now() + STREAM_WRITE_TIMEOUT;
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(fault("http write timed out under backpressure"));
        }
        match stream.write(bytes) {
            Ok(0) => return Err(fault("http write closed")),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL_INTERVAL),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(fault(format!("http write failed: {error}"))),
        }
    }
    Ok(())
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "HTTP Status",
    }
}

fn int_arg(value: Option<&Value>, name: &str) -> Result<i64, Fault> {
    match value {
        Some(value) => int_value(value, name),
        None => Err(fault(format!("missing positional argument 0 ({name})"))),
    }
}

fn int_value(value: &Value, name: &str) -> Result<i64, Fault> {
    match value {
        Value::Int(value) => Ok(*value),
        Value::UInt(value) => {
            i64::try_from(*value).map_err(|_| fault(format!("{name} is out of range")))
        }
        _ => Err(fault(format!("{name} must be numerus"))),
    }
}

fn text_value(value: &Value, name: &str) -> Result<String, Fault> {
    match value {
        Value::Str(text) => Ok(text.clone()),
        _ => Err(fault(format!("{name} must be textus"))),
    }
}

fn bytes_value(value: &Value, name: &str) -> Result<Vec<u8>, Fault> {
    match value {
        Value::Octeti(bytes) => Ok(bytes.clone()),
        Value::Str(text) => Ok(text.as_bytes().to_vec()),
        // `bytes` (the norma carrier type) is a list of byte-valued numeri.
        Value::Array(items) => items
            .borrow()
            .iter()
            .map(|item| match item {
                Value::Int(byte) => u8::try_from(*byte).ok(),
                Value::UInt(byte) => u8::try_from(*byte).ok(),
                _ => None,
            })
            .collect::<Option<Vec<u8>>>()
            .ok_or_else(|| fault(format!("{name} must hold byte values 0..=255"))),
        _ => Err(fault(format!("{name} must be octeti, bytes or textus"))),
    }
}

fn status_value(value: &Value, route: &str) -> Result<u16, Fault> {
    let status = int_value(value, "status")?;
    u16::try_from(status)
        .ok()
        .filter(|status| (100..=599).contains(status))
        .ok_or_else(|| fault(format!("{route} status must be between 100 and 599")))
}

fn bounded_body_size(value: i64) -> Result<usize, Fault> {
    if value <= 0 {
        return Err(fault("max_body_bytes must be positive"));
    }
    let value = usize::try_from(value).map_err(|_| fault("max_body_bytes is too large"))?;
    if value > MAX_CONFIGURED_BODY_BYTES {
        return Err(fault(format!(
            "max_body_bytes must be at most {MAX_CONFIGURED_BODY_BYTES}"
        )));
    }
    Ok(value)
}

fn deadline_arg(value: Option<&Value>, name: &str, default: Duration) -> Result<Duration, Fault> {
    let Some(value) = value else {
        return Ok(default);
    };
    let millis = u64::try_from(int_value(value, name)?)
        .ok()
        .filter(|millis| *millis > 0)
        .ok_or_else(|| fault(format!("http:listen {name} must be positive")))?;
    if millis > MAX_CONFIGURED_DEADLINE_MS {
        return Err(fault(format!(
            "http:listen {name} must be at most {MAX_CONFIGURED_DEADLINE_MS} milliseconds"
        )));
    }
    Ok(Duration::from_millis(millis))
}

fn parse_bind_host(host: &str) -> Result<String, Fault> {
    if host.is_empty() || host.contains(['\0', '\r', '\n']) {
        return Err(fault("http:listen bind_host is malformed"));
    }
    Ok(host.to_owned())
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn is_valid_header_value(value: &str) -> bool {
    !value
        .bytes()
        .any(|byte| (byte < 0x20 && byte != b'\t') || byte == 0x7f)
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

#[cfg(test)]
#[path = "http_test.rs"]
mod tests;
