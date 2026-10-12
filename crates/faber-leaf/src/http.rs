//! Blocking HTTP/1.1 client and server. Serves `http:*` routes.
//!
//! rustls is TLS (A2 / Q4). httparse is the request/response parser.
//! No async runtime. Handles live here; Norma carries only ids.

#[cfg(test)]
#[path = "http_test.rs"]
mod tests;

use crate::error::LeafError;
use crate::value::{Arg, Reply};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const DEFAULT_MAX_BODY: usize = 1_048_576;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

static NEXT_ID: AtomicI64 = AtomicI64::new(1);
static NEXT_CONN: AtomicI64 = AtomicI64::new(1);
static HANDLES: Mutex<Option<HashMap<i64, Handle>>> = Mutex::new(None);
static REQUESTS: Mutex<Option<HashMap<String, RequestSlot>>> = Mutex::new(None);

enum Handle {
    Agent {
        timeout: Duration,
        ca_path: Option<String>,
    },
    Listener {
        listener: TcpListener,
        max_body: usize,
        tls: Option<Arc<ServerConfig>>,
    },
    Reader {
        stream: IoStream,
        leftover: Vec<u8>,
        remaining: Option<u64>,
        chunked: bool,
        done: bool,
    },
    Writer {
        stream: IoStream,
    },
}

struct RequestSlot {
    stream: IoStream,
    headers: Vec<(String, String)>,
}

pub(crate) enum IoStream {
    Plain(TcpStream),
    TlsClient(rustls::StreamOwned<ClientConnection, TcpStream>),
    TlsServer(rustls::StreamOwned<ServerConnection, TcpStream>),
}

impl Read for IoStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf),
            Self::TlsClient(stream) => stream.read(buf),
            Self::TlsServer(stream) => stream.read(buf),
        }
    }
}

impl Write for IoStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buf),
            Self::TlsClient(stream) => stream.write(buf),
            Self::TlsServer(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::TlsClient(stream) => stream.flush(),
            Self::TlsServer(stream) => stream.flush(),
        }
    }
}

fn with_handles<T>(f: impl FnOnce(&mut HashMap<i64, Handle>) -> T) -> T {
    let mut guard = match HANDLES.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(guard.get_or_insert_with(HashMap::new))
}

fn with_requests<T>(f: impl FnOnce(&mut HashMap<String, RequestSlot>) -> T) -> T {
    let mut guard = match REQUESTS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(guard.get_or_insert_with(HashMap::new))
}

fn insert_handle(handle: Handle) -> i64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with_handles(|map| {
        map.insert(id, handle);
    });
    id
}

/// Drop listeners, agents, and pending HTTP state.
pub fn reset_handles() {
    with_handles(HashMap::clear);
    with_requests(HashMap::clear);
}

/// Take a pending request so the WebSocket leaf can upgrade it.
pub(crate) fn take_upgrade(
    request_id: &str,
) -> Result<(IoStream, Vec<(String, String)>), LeafError> {
    with_requests(|map| match map.remove(request_id) {
        Some(slot) => Ok((slot.stream, slot.headers)),
        None => Err(LeafError::closed("websocket:accept", "request closed")),
    })
}

/// Route names this leaf serves. Must match the `serve` match arms below.
const ROUTES: &[&str] = &[
    "http:agent",
    "http:get",
    "http:delete",
    "http:post",
    "http:put",
    "http:patch",
    "http:request",
    "http:request_open",
    "http:read",
    "http:listen",
    "http:listen_tls",
    "http:accept",
    "http:respond",
    "http:respond_open",
    "http:respond_chunk",
    "http:respond_finish",
    "http:stop",
    "http:port",
];

/// Whether `route` belongs to this leaf. Pure name lookup; opens no handle.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
}

/// Answer an `http:*` route. `None` means the route is not this leaf.
#[must_use]
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "http:agent" => Ok(agent(args)),
        "http:get" => client_call("GET", args, false),
        "http:delete" => client_call("DELETE", args, false),
        "http:post" => client_call("POST", args, true),
        "http:put" => client_call("PUT", args, true),
        "http:patch" => client_call("PATCH", args, true),
        "http:request" => generic(args, false),
        "http:request_open" => generic(args, true),
        "http:read" => read_chunk(args),
        "http:listen" => listen(args, false),
        "http:listen_tls" => listen(args, true),
        "http:accept" => accept(args),
        "http:respond" => respond(args),
        "http:respond_open" => respond_open(args),
        "http:respond_chunk" => respond_chunk(args),
        "http:respond_finish" => respond_finish(args),
        "http:stop" => stop(args),
        "http:port" => port(args),
        _ => return None,
    };
    Some(result)
}

fn expect_int(
    route: &'static str,
    args: &[Arg],
    index: usize,
    name: &str,
) -> Result<i64, LeafError> {
    match args.get(index) {
        Some(Arg::Int(value)) => Ok(*value),
        Some(_) => Err(LeafError::invalid_argument(
            route,
            format!("{name} must be numerus"),
        )),
        None => Err(LeafError::invalid_argument(
            route,
            format!("missing {name}"),
        )),
    }
}

fn expect_str<'a>(
    route: &'static str,
    args: &'a [Arg],
    index: usize,
    name: &str,
) -> Result<&'a str, LeafError> {
    match args.get(index) {
        Some(Arg::Str(value)) => Ok(value),
        Some(_) => Err(LeafError::invalid_argument(
            route,
            format!("{name} must be textus"),
        )),
        None => Err(LeafError::invalid_argument(
            route,
            format!("missing {name}"),
        )),
    }
}

fn headers_from_arg(arg: &Arg) -> Result<Vec<(String, String)>, LeafError> {
    match arg {
        Arg::Nil | Arg::Strs(_) => Ok(Vec::new()),
        Arg::List(items) => {
            let mut headers = Vec::new();
            for item in items {
                let Arg::Map(fields) = item else {
                    continue;
                };
                let name = fields
                    .iter()
                    .find(|(key, _)| key == "name")
                    .map(|(_, value)| value.clone());
                let value = fields
                    .iter()
                    .find(|(key, _)| key == "value")
                    .map(|(_, value)| value.clone());
                if let (Some(name), Some(value)) = (name, value) {
                    headers.push((name, value));
                }
            }
            Ok(headers)
        }
        Arg::Map(fields) => Ok(fields.clone()),
        _ => Err(LeafError::invalid_argument(
            "http:request",
            "headers must be a list of maps",
        )),
    }
}

fn body_from_arg(arg: &Arg) -> Vec<u8> {
    match arg {
        Arg::Str(text) => text.as_bytes().to_vec(),
        Arg::Bytes(bytes) => bytes.clone(),
        _ => Vec::new(),
    }
}

fn headers_reply(headers: &[(String, String)]) -> Reply {
    Reply::List(
        headers
            .iter()
            .map(|(name, value)| {
                Reply::Map(vec![
                    ("name".to_owned(), Reply::Str(name.clone())),
                    ("value".to_owned(), Reply::Str(value.clone())),
                ])
            })
            .collect(),
    )
}

fn agent(args: &[Arg]) -> Reply {
    let timeout = match args.first() {
        Some(Arg::Int(ms)) if *ms > 0 => {
            Duration::from_millis(u64::try_from(*ms).unwrap_or(60_000))
        }
        _ => DEFAULT_TIMEOUT,
    };
    let ca_path = match args.get(1) {
        Some(Arg::Str(path)) => Some(path.clone()),
        _ => None,
    };
    Reply::Int(insert_handle(Handle::Agent { timeout, ca_path }))
}

fn lookup_agent(id: i64) -> (Duration, Option<String>) {
    with_handles(|map| match map.get(&id) {
        Some(Handle::Agent { timeout, ca_path }) => (*timeout, ca_path.clone()),
        _ => (DEFAULT_TIMEOUT, None),
    })
}

struct ClientOpener {
    agent: Option<i64>,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn client_call(method: &str, args: &[Arg], has_body: bool) -> Result<Reply, LeafError> {
    let opener = parse_client_args(method, args, has_body)?;
    exchange(&opener, false)
}

fn generic(args: &[Arg], open: bool) -> Result<Reply, LeafError> {
    let (offset, agent) = match args.first() {
        Some(Arg::Int(id)) => (1, Some(*id)),
        _ => (0, None),
    };
    let method = expect_str("http:request", args, offset, "method")?;
    let url = expect_str("http:request", args, offset + 1, "url")?;
    let headers = match args.get(offset + 2) {
        Some(arg) => headers_from_arg(arg)?,
        None => Vec::new(),
    };
    let body = args.get(offset + 3).map(body_from_arg).unwrap_or_default();
    exchange(
        &ClientOpener {
            agent,
            method: method.to_owned(),
            url: url.to_owned(),
            headers,
            body,
        },
        open,
    )
}

fn parse_client_args(
    method: &str,
    args: &[Arg],
    has_body: bool,
) -> Result<ClientOpener, LeafError> {
    let (offset, agent) = match args.first() {
        Some(Arg::Int(id)) => (1, Some(*id)),
        _ => (0, None),
    };
    let url = expect_str("http:request", args, offset, "url")?.to_owned();
    if has_body {
        return match args.get(offset + 1) {
            Some(Arg::List(_) | Arg::Map(_) | Arg::Strs(_)) => Ok(ClientOpener {
                agent,
                method: method.to_owned(),
                url,
                headers: headers_from_arg(&args[offset + 1])?,
                body: args.get(offset + 2).map(body_from_arg).unwrap_or_default(),
            }),
            Some(other) => Ok(ClientOpener {
                agent,
                method: method.to_owned(),
                url,
                headers: Vec::new(),
                body: body_from_arg(other),
            }),
            None => Ok(ClientOpener {
                agent,
                method: method.to_owned(),
                url,
                headers: Vec::new(),
                body: Vec::new(),
            }),
        };
    }
    let headers = match args.get(offset + 1) {
        Some(arg) => headers_from_arg(arg)?,
        None => Vec::new(),
    };
    Ok(ClientOpener {
        agent,
        method: method.to_owned(),
        url,
        headers,
        body: Vec::new(),
    })
}

struct UrlParts {
    https: bool,
    host: String,
    port: u16,
    target: String,
}

fn split_url(url: &str) -> Result<UrlParts, LeafError> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| {
        LeafError::invalid_argument("http:request", format!("invalid URL `{url}`"))
    })?;
    let https = match scheme {
        "https" => true,
        "http" => false,
        _ => {
            return Err(LeafError::invalid_argument(
                "http:request",
                format!("unsupported scheme `{scheme}`"),
            ));
        }
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !host.ends_with(']') => {
            let parsed = port.parse::<u16>().map_err(|_| {
                LeafError::invalid_argument("http:request", format!("invalid port `{port}`"))
            })?;
            (host.to_owned(), parsed)
        }
        _ => (authority.to_owned(), if https { 443 } else { 80 }),
    };
    let target = if path.is_empty() { "/" } else { path }.to_owned();
    Ok(UrlParts {
        https,
        host,
        port,
        target,
    })
}

fn exchange(opener: &ClientOpener, open: bool) -> Result<Reply, LeafError> {
    let (timeout, ca_path) = opener.agent.map_or((DEFAULT_TIMEOUT, None), lookup_agent);
    let url = split_url(&opener.url)?;
    let addr = format!("{}:{}", url.host, url.port);
    let tcp =
        TcpStream::connect(&addr).map_err(|err| LeafError::from_io("http:request", &addr, &err))?;
    configure_stream(&tcp, timeout, "http:request")?;
    let mut stream = if url.https {
        wrap_tls_client(tcp, &url.host, ca_path.as_deref())?
    } else {
        IoStream::Plain(tcp)
    };
    let host_header = if (url.https && url.port == 443) || (!url.https && url.port == 80) {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        opener.method, url.target, host_header
    );
    if !opener.body.is_empty() {
        request.push_str("Content-Length: ");
        request.push_str(&opener.body.len().to_string());
        request.push_str("\r\n");
    }
    for (name, value) in &opener.headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .and_then(|()| {
            if opener.body.is_empty() {
                Ok(())
            } else {
                stream.write_all(&opener.body)
            }
        })
        .and_then(|()| stream.flush())
        .map_err(|err| LeafError::from_io("http:request", &addr, &err))?;
    let parsed = read_http_message(&mut stream, DEFAULT_MAX_BODY, true, !open)?;
    if open {
        let remaining = parsed.content_length;
        let id = insert_handle(Handle::Reader {
            leftover: parsed.body,
            remaining,
            chunked: parsed.chunked,
            done: false,
            stream,
        });
        return Ok(Reply::Map(vec![
            ("reader".to_owned(), Reply::Int(id)),
            ("status".to_owned(), Reply::Int(i64::from(parsed.status))),
            ("headers".to_owned(), headers_reply(&parsed.headers)),
        ]));
    }
    Ok(Reply::Map(vec![
        ("status".to_owned(), Reply::Int(i64::from(parsed.status))),
        ("headers".to_owned(), headers_reply(&parsed.headers)),
        (
            "body".to_owned(),
            Reply::Str(String::from_utf8_lossy(&parsed.body).into_owned()),
        ),
    ]))
}

fn read_chunk(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "http:read";
    let id = expect_int(ROUTE, args, 0, "reader")?;
    let max = match args.get(1) {
        Some(Arg::Int(value)) if *value > 0 => usize::try_from(*value).unwrap_or(65_536),
        _ => 65_536,
    };
    let handle = with_handles(|map| match map.remove(&id) {
        Some(handle @ Handle::Reader { .. }) => Ok(handle),
        Some(other) => {
            map.insert(id, other);
            Err(LeafError::invalid_argument(ROUTE, "id is not a reader"))
        }
        None => Err(LeafError::closed(ROUTE, "reader closed")),
    })?;
    let Handle::Reader {
        mut stream,
        mut leftover,
        mut remaining,
        chunked,
        mut done,
    } = handle
    else {
        return Err(LeafError::invalid_argument(ROUTE, "id is not a reader"));
    };
    if done {
        with_handles(|map| {
            map.insert(
                id,
                Handle::Reader {
                    stream,
                    leftover,
                    remaining,
                    chunked,
                    done,
                },
            );
        });
        return Ok(Reply::Map(vec![
            ("bytes".to_owned(), Reply::Bytes(Vec::new())),
            ("done".to_owned(), Reply::Bool(true)),
        ]));
    }
    let mut out = Vec::new();
    if !chunked {
        take_leftover(&mut leftover, remaining.as_mut(), max, &mut out);
    }
    if out.is_empty() {
        fill_reader(
            &mut stream,
            &mut leftover,
            remaining.as_mut(),
            chunked,
            &mut done,
            max,
            &mut out,
        )?;
    }
    if remaining == Some(0) && leftover.is_empty() {
        done = true;
    }
    with_handles(|map| {
        map.insert(
            id,
            Handle::Reader {
                stream,
                leftover,
                remaining,
                chunked,
                done,
            },
        );
    });
    Ok(Reply::Map(vec![
        ("bytes".to_owned(), Reply::Bytes(out)),
        ("done".to_owned(), Reply::Bool(done)),
    ]))
}

fn take_leftover(
    leftover: &mut Vec<u8>,
    remaining: Option<&mut u64>,
    max: usize,
    out: &mut Vec<u8>,
) {
    if leftover.is_empty() {
        return;
    }
    let take = leftover.len().min(max);
    out.extend_from_slice(&leftover[..take]);
    leftover.drain(..take);
    if let Some(left) = remaining {
        *left = left.saturating_sub(take as u64);
    }
}

fn fill_reader(
    stream: &mut IoStream,
    leftover: &mut Vec<u8>,
    mut remaining: Option<&mut u64>,
    chunked: bool,
    done: &mut bool,
    max: usize,
    out: &mut Vec<u8>,
) -> Result<(), LeafError> {
    while out.len() < max && !*done {
        if chunked {
            match read_next_chunk(stream, leftover, max) {
                Ok(Some(chunk)) if chunk.is_empty() => *done = true,
                Ok(Some(chunk)) => out.extend(chunk),
                Ok(None) => *done = true,
                Err(err) if is_peer_closed(&err) => *done = true,
                Err(err) => return Err(err),
            }
            continue;
        }
        if leftover.is_empty() && remaining.as_deref() != Some(&0) {
            let want = remaining
                .as_deref()
                .map_or(max - out.len(), |left| {
                    usize::try_from(*left).unwrap_or(max - out.len())
                })
                .min(max - out.len());
            let mut buf = vec![0_u8; want.max(1)];
            match stream.read(&mut buf) {
                Ok(0) => *done = true,
                Ok(n) => {
                    leftover.extend_from_slice(&buf[..n]);
                    if let Some(left) = remaining.as_deref_mut() {
                        *left = left.saturating_sub(n as u64);
                    }
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::UnexpectedEof
                        || err.kind() == std::io::ErrorKind::ConnectionReset
                        || err.kind() == std::io::ErrorKind::BrokenPipe =>
                {
                    *done = true;
                }
                Err(err) => return Err(LeafError::from_io("http:read", "", &err)),
            }
            continue;
        }
        if leftover.is_empty() {
            if remaining.as_deref() == Some(&0) {
                *done = true;
            }
            break;
        }
        take_leftover(leftover, remaining.as_deref_mut(), max - out.len(), out);
    }
    Ok(())
}

fn is_peer_closed(err: &LeafError) -> bool {
    let text = err.to_string();
    text.contains("UnexpectedEof")
        || text.contains("Connection reset")
        || text.contains("Broken pipe")
        || text.contains("peer closed")
}

fn read_byte(stream: &mut IoStream, pending: &mut Vec<u8>) -> Result<Option<u8>, LeafError> {
    if !pending.is_empty() {
        return Ok(Some(pending.remove(0)));
    }
    let mut byte = [0_u8; 1];
    match stream.read(&mut byte) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(byte[0])),
        Err(err)
            if err.kind() == std::io::ErrorKind::UnexpectedEof
                || err.kind() == std::io::ErrorKind::ConnectionReset
                || err.kind() == std::io::ErrorKind::BrokenPipe =>
        {
            Ok(None)
        }
        Err(err) => Err(LeafError::from_io("http:read", "", &err)),
    }
}

fn read_exact_pending(
    stream: &mut IoStream,
    pending: &mut Vec<u8>,
    want: usize,
) -> Result<Vec<u8>, LeafError> {
    // Never reserve from an untrusted size; grow as bytes actually arrive.
    let mut out = Vec::with_capacity(want.min(8192));
    while out.len() < want {
        if !pending.is_empty() {
            let take = pending.len().min(want - out.len());
            out.extend_from_slice(&pending[..take]);
            pending.drain(..take);
            continue;
        }
        let mut buf = vec![0_u8; (want - out.len()).min(8192)];
        match stream.read(&mut buf) {
            Ok(0) => {
                return Err(LeafError::closed("http:read", "truncated chunk"));
            }
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(err) => return Err(LeafError::from_io("http:read", "", &err)),
        }
    }
    Ok(out)
}

fn read_next_chunk(
    stream: &mut IoStream,
    pending: &mut Vec<u8>,
    max: usize,
) -> Result<Option<Vec<u8>>, LeafError> {
    let mut line = Vec::new();
    loop {
        match read_byte(stream, pending)? {
            None if line.is_empty() => return Ok(None),
            None => return Err(LeafError::closed("http:read", "truncated chunk size")),
            Some(byte) => {
                line.push(byte);
                if line.ends_with(b"\r\n") {
                    break;
                }
                if line.len() > 64 {
                    return Err(LeafError::invalid_argument(
                        "http:read",
                        "chunk size line too long",
                    ));
                }
            }
        }
    }
    let hex = std::str::from_utf8(&line[..line.len() - 2])
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    let size = usize::from_str_radix(hex, 16).map_err(|_| {
        LeafError::invalid_argument("http:read", format!("invalid chunk size `{hex}`"))
    })?;
    if size > max {
        return Err(LeafError::invalid_argument(
            "http:read",
            "chunk size exceeds read limit",
        ));
    }
    if size == 0 {
        let _ = read_exact_pending(stream, pending, 2);
        return Ok(Some(Vec::new()));
    }
    let mut data = read_exact_pending(stream, pending, size + 2)?;
    data.truncate(size);
    Ok(Some(data))
}

fn listen(args: &[Arg], tls: bool) -> Result<Reply, LeafError> {
    let route = if tls {
        "http:listen_tls"
    } else {
        "http:listen"
    };
    let port = expect_int(route, args, 0, "port")?;
    if !(0..=65_535).contains(&port) {
        return Err(LeafError::invalid_argument(route, "port out of range"));
    }
    let mut host = "0.0.0.0";
    let mut max_body = DEFAULT_MAX_BODY;
    let mut cert = None;
    let mut key = None;
    if tls {
        cert = Some(expect_str(route, args, 1, "cert")?);
        key = Some(expect_str(route, args, 2, "key")?);
        if let Some(Arg::Str(value)) = args.get(3) {
            host = value;
        }
        if let Some(Arg::Int(value)) = args.get(4) {
            max_body = usize::try_from(*value).unwrap_or(DEFAULT_MAX_BODY);
        }
    } else {
        match args.get(1) {
            Some(Arg::Str(value)) => host = value,
            Some(Arg::Int(value)) => {
                max_body = usize::try_from(*value).unwrap_or(DEFAULT_MAX_BODY);
            }
            _ => {}
        }
        match args.get(2) {
            Some(Arg::Str(value)) => host = value,
            Some(Arg::Int(value)) => {
                max_body = usize::try_from(*value).unwrap_or(DEFAULT_MAX_BODY);
            }
            _ => {}
        }
    }
    let listener = TcpListener::bind((host, u16::try_from(port).unwrap_or(0)))
        .map_err(|err| LeafError::from_io(route, host, &err))?;
    listener
        .set_nonblocking(false)
        .map_err(|err| LeafError::from_io(route, host, &err))?;
    let tls = match (cert, key) {
        (Some(cert), Some(key)) => Some(server_config(cert, key)?),
        _ => None,
    };
    Ok(Reply::Int(insert_handle(Handle::Listener {
        listener,
        max_body,
        tls,
    })))
}

fn accept(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "http:accept";
    let id = expect_int(ROUTE, args, 0, "handle")?;
    let pending = with_handles(|map| match map.get(&id) {
        Some(Handle::Listener {
            listener,
            max_body,
            tls,
        }) => listener
            .try_clone()
            .map(|cloned| (cloned, *max_body, tls.clone()))
            .map_err(|err| LeafError::from_io(ROUTE, "", &err)),
        Some(_) => Err(LeafError::invalid_argument(ROUTE, "id is not a listener")),
        None => Err(LeafError::closed(ROUTE, "listener closed")),
    })?;
    let (listener, max_body, tls) = pending;
    let (tcp, _) = listener
        .accept()
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))?;
    configure_stream(&tcp, DEFAULT_TIMEOUT, ROUTE)?;
    let mut stream = match tls {
        Some(config) => {
            let conn = ServerConnection::new(config).map_err(|err| {
                LeafError::unavailable(ROUTE, format!("tls server handshake: {err}"))
            })?;
            IoStream::TlsServer(rustls::StreamOwned::new(conn, tcp))
        }
        None => IoStream::Plain(tcp),
    };
    let parsed = read_http_message(&mut stream, max_body, false, true)?;
    let connection = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let request_id = connection.to_string();
    with_requests(|map| {
        map.insert(
            request_id.clone(),
            RequestSlot {
                stream,
                headers: parsed.headers.clone(),
            },
        );
    });
    Ok(Reply::Map(vec![
        ("id".to_owned(), Reply::Str(request_id)),
        ("connection".to_owned(), Reply::Int(connection)),
        ("method".to_owned(), Reply::Str(parsed.method)),
        ("path".to_owned(), Reply::Str(parsed.target)),
        ("headers".to_owned(), headers_reply(&parsed.headers)),
        (
            "body".to_owned(),
            Reply::Str(String::from_utf8_lossy(&parsed.body).into_owned()),
        ),
    ]))
}

fn take_request(id: &str, route: &'static str) -> Result<RequestSlot, LeafError> {
    with_requests(|map| match map.remove(id) {
        Some(slot) => Ok(slot),
        None => Err(LeafError::closed(route, "request closed")),
    })
}

fn respond(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "http:respond";
    let id = expect_str(ROUTE, args, 0, "id")?;
    let status = expect_int(ROUTE, args, 1, "status")?;
    let headers = match args.get(2) {
        Some(arg) => headers_from_arg(arg)?,
        None => Vec::new(),
    };
    let body = args.get(3).map(body_from_arg).unwrap_or_default();
    let mut slot = take_request(id, ROUTE)?;
    write_response(&mut slot.stream, status, &headers, Some(&body), false)?;
    Ok(Reply::Unit)
}

fn respond_open(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "http:respond_open";
    let id = expect_str(ROUTE, args, 0, "id")?;
    let status = expect_int(ROUTE, args, 1, "status")?;
    let headers = match args.get(2) {
        Some(arg) => headers_from_arg(arg)?,
        None => Vec::new(),
    };
    let slot = take_request(id, ROUTE)?;
    let mut stream = slot.stream;
    write_response(&mut stream, status, &headers, None, true)?;
    Ok(Reply::Int(insert_handle(Handle::Writer { stream })))
}

fn respond_chunk(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "http:respond_chunk";
    let id = expect_int(ROUTE, args, 0, "writer")?;
    let data = args.get(1).map(body_from_arg).unwrap_or_default();
    let mut handle = take_writer(id, ROUTE)?;
    let result = write_chunk(&mut handle, &data);
    put_writer(id, handle);
    result.map(|()| Reply::Unit)
}

fn respond_finish(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "http:respond_finish";
    let id = expect_int(ROUTE, args, 0, "writer")?;
    let mut handle = take_writer(id, ROUTE)?;
    write_chunk(&mut handle, &[])?;
    handle
        .flush()
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))?;
    drop(handle);
    Ok(Reply::Unit)
}

fn take_writer(id: i64, route: &'static str) -> Result<IoStream, LeafError> {
    with_handles(|map| match map.remove(&id) {
        Some(Handle::Writer { stream }) => Ok(stream),
        Some(other) => {
            map.insert(id, other);
            Err(LeafError::invalid_argument(route, "id is not a writer"))
        }
        None => Err(LeafError::closed(route, "writer closed")),
    })
}

fn put_writer(id: i64, stream: IoStream) {
    with_handles(|map| {
        map.insert(id, Handle::Writer { stream });
    });
}

fn write_chunk(stream: &mut IoStream, data: &[u8]) -> Result<(), LeafError> {
    let header = format!("{:x}\r\n", data.len());
    stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(data))
        .and_then(|()| stream.write_all(b"\r\n"))
        .and_then(|()| stream.flush())
        .map_err(|err| LeafError::from_io("http:respond_chunk", "", &err))
}

fn write_response(
    stream: &mut IoStream,
    status: i64,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    chunked: bool,
) -> Result<(), LeafError> {
    let code = u16::try_from(status).unwrap_or(200);
    let mut head = format!("HTTP/1.1 {code} {}\r\n", reason(code));
    if chunked {
        head.push_str("transfer-encoding: chunked\r\n");
    } else if let Some(body) = body {
        head.push_str("Content-Length: ");
        head.push_str(&body.len().to_string());
        head.push_str("\r\n");
    }
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .and_then(|()| match body {
            Some(body) if !chunked => stream.write_all(body),
            _ => Ok(()),
        })
        .and_then(|()| stream.flush())
        .map_err(|err| LeafError::from_io("http:respond", "", &err))
}

fn stop(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "http:stop";
    let id = expect_int(ROUTE, args, 0, "handle")?;
    with_handles(|map| {
        map.remove(&id);
    });
    Ok(Reply::Unit)
}

fn port(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "http:port";
    let id = expect_int(ROUTE, args, 0, "handle")?;
    with_handles(|map| match map.get(&id) {
        Some(Handle::Listener { listener, .. }) => listener
            .local_addr()
            .map(|addr| Reply::Int(i64::from(addr.port())))
            .map_err(|err| LeafError::from_io(ROUTE, "", &err)),
        Some(_) => Err(LeafError::invalid_argument(ROUTE, "id is not a listener")),
        None => Err(LeafError::closed(ROUTE, "listener closed")),
    })
}

struct ParsedMessage {
    status: u16,
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    content_length: Option<u64>,
    chunked: bool,
}

fn read_http_message(
    stream: &mut IoStream,
    max_body: usize,
    response: bool,
    drain_body: bool,
) -> Result<ParsedMessage, LeafError> {
    let mut buf = Vec::new();
    let mut scratch = [0_u8; 4096];
    let header_end = loop {
        let n = stream
            .read(&mut scratch)
            .map_err(|err| LeafError::from_io("http:accept", "", &err))?;
        if n == 0 {
            return Err(LeafError::closed("http:accept", "peer closed"));
        }
        buf.extend_from_slice(&scratch[..n]);
        if let Some(index) = find_header_end(&buf) {
            break index;
        }
        if buf.len() > 64 * 1024 {
            return Err(LeafError::invalid_argument(
                "http:accept",
                "headers too large",
            ));
        }
    };
    let header_bytes = &buf[..header_end];
    let leftover = buf[header_end..].to_vec();
    let mut parsed_headers = [httparse::EMPTY_HEADER; 64];
    let (status, method, target, headers) = if response {
        let mut resp = httparse::Response::new(&mut parsed_headers);
        match resp.parse(header_bytes) {
            Ok(httparse::Status::Complete(_)) => {}
            _ => {
                return Err(LeafError::invalid_argument(
                    "http:request",
                    "malformed response",
                ));
            }
        }
        let status = resp.code.unwrap_or(0);
        (
            status,
            String::new(),
            String::new(),
            collect_headers(resp.headers),
        )
    } else {
        let mut req = httparse::Request::new(&mut parsed_headers);
        match req.parse(header_bytes) {
            Ok(httparse::Status::Complete(_)) => {}
            _ => {
                return Err(LeafError::invalid_argument(
                    "http:accept",
                    "malformed request",
                ));
            }
        }
        (
            0,
            req.method.unwrap_or("").to_owned(),
            req.path.unwrap_or("/").to_owned(),
            collect_headers(req.headers),
        )
    };
    let chunked = headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
    });
    let content_length = headers.iter().find_map(|(name, value)| {
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.parse::<u64>().ok())
            .flatten()
    });
    if let Some(length) = content_length
        && usize::try_from(length).unwrap_or(usize::MAX) > max_body
    {
        return Err(LeafError::invalid_argument(
            "http:accept",
            "request body exceeds max_body_bytes",
        ));
    }
    let body = if !drain_body {
        leftover
    } else if chunked {
        read_chunked_body(stream, leftover, max_body)?
    } else if content_length.is_some() || response {
        read_length_body(stream, leftover, content_length, max_body)?
    } else {
        leftover
    };
    Ok(ParsedMessage {
        status,
        method,
        target,
        headers,
        body,
        content_length,
        chunked,
    })
}

fn collect_headers(headers: &[httparse::Header<'_>]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|header| {
            (
                header.name.to_owned(),
                String::from_utf8_lossy(header.value).into_owned(),
            )
        })
        .collect()
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn read_length_body(
    stream: &mut IoStream,
    leftover: Vec<u8>,
    length: Option<u64>,
    max_body: usize,
) -> Result<Vec<u8>, LeafError> {
    let mut body = leftover;
    let already = body.len();
    if let Some(length) = length {
        let target = usize::try_from(length).unwrap_or(max_body);
        if already >= target {
            body.truncate(target);
            return Ok(body);
        }
        body.resize(target, 0);
        stream
            .read_exact(&mut body[already..])
            .map_err(|err| LeafError::from_io("http:accept", "", &err))?;
        return Ok(body);
    }
    let mut scratch = [0_u8; 4096];
    loop {
        let n = stream
            .read(&mut scratch)
            .map_err(|err| LeafError::from_io("http:accept", "", &err))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&scratch[..n]);
        if body.len() > max_body {
            return Err(LeafError::invalid_argument(
                "http:accept",
                "request body exceeds max_body_bytes",
            ));
        }
    }
    Ok(body)
}

fn read_chunked_body(
    stream: &mut IoStream,
    leftover: Vec<u8>,
    max_body: usize,
) -> Result<Vec<u8>, LeafError> {
    let mut pending = leftover;
    let mut body = Vec::new();
    loop {
        if !pending.contains(&b'\n') {
            let mut scratch = [0_u8; 1024];
            let n = stream
                .read(&mut scratch)
                .map_err(|err| LeafError::from_io("http:accept", "", &err))?;
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&scratch[..n]);
            continue;
        }
        let Some(end) = pending.windows(2).position(|window| window == b"\r\n") else {
            let mut scratch = [0_u8; 1024];
            let n = stream
                .read(&mut scratch)
                .map_err(|err| LeafError::from_io("http:accept", "", &err))?;
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&scratch[..n]);
            continue;
        };
        let hex = std::str::from_utf8(&pending[..end]).unwrap_or("");
        let size = usize::from_str_radix(hex.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| LeafError::invalid_argument("http:accept", "invalid chunk size"))?;
        if size > max_body || body.len().saturating_add(size) > max_body {
            return Err(LeafError::invalid_argument(
                "http:accept",
                "request body exceeds max_body_bytes",
            ));
        }
        pending.drain(..end + 2);
        if size == 0 {
            break;
        }
        let want = size
            .checked_add(2)
            .ok_or_else(|| LeafError::invalid_argument("http:accept", "invalid chunk size"))?;
        while pending.len() < want {
            let mut scratch = [0_u8; 4096];
            let n = stream
                .read(&mut scratch)
                .map_err(|err| LeafError::from_io("http:accept", "", &err))?;
            if n == 0 {
                return Err(LeafError::closed("http:accept", "truncated chunk"));
            }
            pending.extend_from_slice(&scratch[..n]);
        }
        body.extend_from_slice(&pending[..size]);
        pending.drain(..size + 2);
        if body.len() > max_body {
            return Err(LeafError::invalid_argument(
                "http:accept",
                "request body exceeds max_body_bytes",
            ));
        }
    }
    Ok(body)
}

fn configure_stream(
    stream: &TcpStream,
    timeout: Duration,
    route: &'static str,
) -> Result<(), LeafError> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| LeafError::from_io(route, "", &err))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| LeafError::from_io(route, "", &err))
}

fn install_ring() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn wrap_tls_client(
    stream: TcpStream,
    host: &str,
    ca_path: Option<&str>,
) -> Result<IoStream, LeafError> {
    install_ring();
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = ca_path {
        let bytes =
            std::fs::read(path).map_err(|err| LeafError::from_io("http:request", path, &err))?;
        for cert in rustls_pemfile::certs(&mut bytes.as_slice()) {
            let cert = cert.map_err(|err| {
                LeafError::invalid_argument("http:request", format!("ca pem: {err}"))
            })?;
            roots
                .add(cert)
                .map_err(|err| LeafError::invalid_argument("http:request", format!("ca: {err}")))?;
        }
    }
    let config = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );
    let name = ServerName::try_from(host.to_owned()).map_err(|_| {
        LeafError::invalid_argument("http:request", format!("invalid TLS server name `{host}`"))
    })?;
    let conn = ClientConnection::new(config, name)
        .map_err(|err| LeafError::unavailable("http:request", format!("tls client: {err}")))?;
    Ok(IoStream::TlsClient(rustls::StreamOwned::new(conn, stream)))
}

fn server_config(cert_path: &str, key_path: &str) -> Result<Arc<ServerConfig>, LeafError> {
    const ROUTE: &str = "http:listen_tls";
    install_ring();
    let cert_bytes =
        std::fs::read(cert_path).map_err(|err| LeafError::from_io(ROUTE, cert_path, &err))?;
    let key_bytes =
        std::fs::read(key_path).map_err(|err| LeafError::from_io(ROUTE, key_path, &err))?;
    let certs = rustls_pemfile::certs(&mut cert_bytes.as_slice())
        .collect::<Result<Vec<CertificateDer<'static>>, _>>()
        .map_err(|err| LeafError::invalid_argument(ROUTE, format!("cert pem: {err}")))?;
    let key = rustls_pemfile::private_key(&mut key_bytes.as_slice())
        .map_err(|err| LeafError::invalid_argument(ROUTE, format!("key pem: {err}")))?
        .ok_or_else(|| LeafError::invalid_argument(ROUTE, "key pem is empty"))?;
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map(Arc::new)
        .map_err(|err| LeafError::unavailable(ROUTE, format!("tls server config: {err}")))
}

fn reason(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "",
    }
}

impl Drop for IoStream {
    fn drop(&mut self) {
        if let Self::Plain(stream) = self {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

/// Shared stream type for the WebSocket upgrade path.
pub(crate) use IoStream as HttpStream;
