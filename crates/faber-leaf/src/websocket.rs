//! Blocking WebSocket leaf. Serves `websocket:*` routes.
//!
//! tungstenite is the framing stack (A2 / Q4). TLS reuses the HTTP rustls
//! client path. No async runtime.

#[cfg(test)]
#[path = "websocket_test.rs"]
mod tests;

use crate::entropy::fill_random;
use crate::error::LeafError;
use crate::http::{self, HttpStream};
use crate::value::{Arg, Reply};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::protocol::{Role, WebSocket};
use tungstenite::{Message, client::IntoClientRequest};

static NEXT_ID: AtomicI64 = AtomicI64::new(1);
static HANDLES: Mutex<Option<HashMap<i64, Socket>>> = Mutex::new(None);

enum Socket {
    Ready(Box<WebSocket<HttpStream>>),
    ClientPending(Box<HttpStream>, String),
}

fn with_handles<T>(f: impl FnOnce(&mut HashMap<i64, Socket>) -> T) -> T {
    let mut guard = match HANDLES.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(guard.get_or_insert_with(HashMap::new))
}

fn insert(socket: WebSocket<HttpStream>) -> i64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with_handles(|map| {
        map.insert(id, Socket::Ready(Box::new(socket)));
    });
    id
}

/// Drop every WebSocket so a later program does not inherit peers.
pub fn reset_handles() {
    with_handles(HashMap::clear);
}

/// Route names this leaf serves. Must match the `serve` match arms below.
const ROUTES: &[&str] = &[
    "websocket:connect",
    "websocket:connect_open",
    "websocket:accept",
    "websocket:send",
    "websocket:send_bytes",
    "websocket:receive",
    "websocket:close",
];

/// Whether `route` belongs to this leaf. Pure name lookup; opens no peer.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
}

/// Answer a `websocket:*` route. `None` means the route is not this leaf.
#[must_use]
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "websocket:connect" => connect(args),
        "websocket:connect_open" => connect_open(args),
        "websocket:accept" => accept(args),
        "websocket:send" => send_text(args),
        "websocket:send_bytes" => send_bytes(args),
        "websocket:receive" => receive(args),
        "websocket:close" => close(args),
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

fn connect(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "websocket:connect";
    let url = expect_str(ROUTE, args, 0, "url")?;
    let ca_path = match args.get(1) {
        Some(Arg::Str(path)) => Some(path.as_str()),
        _ => None,
    };
    let (secure, host, port, target) = split_ws_url(url)?;
    let addr = format!("{host}:{port}");
    let tcp = TcpStream::connect(&addr).map_err(|err| LeafError::from_io(ROUTE, &addr, &err))?;
    tcp.set_read_timeout(Some(Duration::from_secs(60)))
        .and_then(|()| tcp.set_write_timeout(Some(Duration::from_secs(60))))
        .map_err(|err| LeafError::from_io(ROUTE, &addr, &err))?;
    let stream = if secure {
        wrap_tls(tcp, &host, ca_path)?
    } else {
        HttpStream::Plain(tcp)
    };
    let handshake = format!("ws://{host}:{port}{target}");
    let request = handshake
        .into_client_request()
        .map_err(|err| LeafError::invalid_argument(ROUTE, format!("websocket request: {err}")))?;
    let (socket, _) = tungstenite::client(request, stream)
        .map_err(|err| LeafError::unavailable(ROUTE, format!("websocket handshake: {err}")))?;
    Ok(Reply::Int(insert(socket)))
}

fn connect_open(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "websocket:connect_open";
    let url = expect_str(ROUTE, args, 0, "url")?;
    let ca_path = match args.get(1) {
        Some(Arg::Str(path)) => Some(path.as_str()),
        _ => None,
    };
    let (secure, host, port, target) = split_ws_url(url)?;
    let addr = format!("{host}:{port}");
    let tcp = TcpStream::connect(&addr).map_err(|err| LeafError::from_io(ROUTE, &addr, &err))?;
    tcp.set_read_timeout(Some(Duration::from_secs(60)))
        .and_then(|()| tcp.set_write_timeout(Some(Duration::from_secs(60))))
        .map_err(|err| LeafError::from_io(ROUTE, &addr, &err))?;
    let mut stream = if secure {
        wrap_tls(tcp, &host, ca_path)?
    } else {
        HttpStream::Plain(tcp)
    };
    let mut random = [0_u8; 16];
    fill_random(&mut random);
    let key = base64(&random);
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|err| LeafError::from_io(ROUTE, &addr, &err))?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with_handles(|map| {
        map.insert(id, Socket::ClientPending(Box::new(stream), key));
    });
    Ok(Reply::Int(id))
}

fn accept(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "websocket:accept";
    let request_id = expect_str(ROUTE, args, 0, "id")?;
    let (mut stream, headers) = http::take_upgrade(request_id)?;
    let key = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-key"))
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| LeafError::invalid_argument(ROUTE, "missing Sec-WebSocket-Key"))?;
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let reply = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream
        .write_all(reply.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))?;
    let socket = WebSocket::from_raw_socket(stream, Role::Server, None);
    Ok(Reply::Int(insert(socket)))
}

fn send_text(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "websocket:send";
    let id = expect_int(ROUTE, args, 0, "id")?;
    let text = expect_str(ROUTE, args, 1, "text")?.to_owned();
    with_socket(id, ROUTE, |socket| {
        socket
            .send(Message::Text(text.into()))
            .map_err(|err| LeafError::from_io(ROUTE, "", &std::io::Error::other(err)))
    })
    .map(|()| Reply::Unit)
}

fn send_bytes(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "websocket:send_bytes";
    let id = expect_int(ROUTE, args, 0, "id")?;
    let data = match args.get(1) {
        Some(Arg::Bytes(bytes)) => bytes.clone(),
        Some(Arg::Str(text)) => text.as_bytes().to_vec(),
        _ => {
            return Err(LeafError::invalid_argument(ROUTE, "missing data"));
        }
    };
    with_socket(id, ROUTE, |socket| {
        socket
            .send(Message::Binary(data.into()))
            .map_err(|err| LeafError::from_io(ROUTE, "", &std::io::Error::other(err)))
    })
    .map(|()| Reply::Unit)
}

fn receive(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "websocket:receive";
    let id = expect_int(ROUTE, args, 0, "id")?;
    with_socket(id, ROUTE, |socket| match socket.read() {
        Ok(Message::Text(text)) => Ok(Reply::Map(vec![
            ("text".to_owned(), Reply::Str(text.to_string())),
            ("done".to_owned(), Reply::Bool(false)),
        ])),
        Ok(Message::Binary(bytes)) => Ok(Reply::Map(vec![
            ("text".to_owned(), Reply::Str(String::new())),
            ("bytes".to_owned(), Reply::Bytes(bytes.to_vec())),
            ("done".to_owned(), Reply::Bool(false)),
        ])),
        Ok(Message::Close(_)) | Err(tungstenite::Error::AlreadyClosed) => {
            Ok(empty_text_frame(true))
        }
        Ok(Message::Ping(payload)) => {
            socket
                .send(Message::Pong(payload))
                .map_err(|err| LeafError::from_io(ROUTE, "", &std::io::Error::other(err)))?;
            Ok(empty_text_frame(false))
        }
        Ok(_) => Ok(empty_text_frame(false)),
        Err(err) => Err(LeafError::from_io(ROUTE, "", &std::io::Error::other(err))),
    })
}

/// A frame with no text payload; `done` marks a peer close.
fn empty_text_frame(done: bool) -> Reply {
    Reply::Map(vec![
        ("text".to_owned(), Reply::Str(String::new())),
        ("done".to_owned(), Reply::Bool(done)),
    ])
}

fn close(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "websocket:close";
    let id = expect_int(ROUTE, args, 0, "id")?;
    with_handles(|map| match map.remove(&id) {
        Some(Socket::Ready(mut socket)) => {
            // WHY: close is best-effort teardown; the socket is discarded and
            // a close-frame error is not actionable.
            drop(socket.close(None));
        }
        Some(Socket::ClientPending(stream, key)) => drop((stream, key)),
        None => {}
    });
    Ok(Reply::Unit)
}

fn with_socket<T>(
    id: i64,
    route: &'static str,
    f: impl FnOnce(&mut WebSocket<HttpStream>) -> Result<T, LeafError>,
) -> Result<T, LeafError> {
    let mut socket = with_handles(|map| match map.remove(&id) {
        Some(Socket::Ready(socket)) => Ok(socket),
        Some(Socket::ClientPending(stream, key)) => {
            map.insert(id, Socket::ClientPending(stream, key));
            Err(LeafError::invalid_argument(route, "handshake not finished"))
        }
        None => Err(LeafError::closed(route, "socket closed")),
    })?;
    let result = f(&mut socket);
    with_handles(|map| {
        map.insert(id, Socket::Ready(socket));
    });
    result
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied();
        let third = chunk.get(2).copied();
        encoded.push(char::from(ALPHABET[usize::from(first >> 2)]));
        encoded.push(char::from(
            ALPHABET[usize::from(((first & 0x03) << 4) | (second.unwrap_or(0) >> 4))],
        ));
        if let Some(second) = second {
            encoded.push(char::from(
                ALPHABET[usize::from(((second & 0x0f) << 2) | (third.unwrap_or(0) >> 6))],
            ));
        } else {
            encoded.push('=');
        }
        if let Some(third) = third {
            encoded.push(char::from(ALPHABET[usize::from(third & 0x3f)]));
        } else {
            encoded.push('=');
        }
    }
    encoded
}

fn split_ws_url(url: &str) -> Result<(bool, String, u16, String), LeafError> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| {
        LeafError::invalid_argument("websocket:connect", format!("invalid URL `{url}`"))
    })?;
    let secure = match scheme {
        "wss" => true,
        "ws" => false,
        _ => {
            return Err(LeafError::invalid_argument(
                "websocket:connect",
                format!("unsupported scheme `{scheme}`"),
            ));
        }
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    // WHY: an IPv6 literal carries colons of its own, so the port separator is
    // only a colon after the closing bracket.
    let port_colon = match authority.rfind(']') {
        Some(close) => authority[close..].find(':').map(|offset| close + offset),
        None => authority.rfind(':'),
    };
    let split = port_colon.map(|index| (&authority[..index], &authority[index + 1..]));
    let (host, port) = match split {
        Some((host, port)) if !host.is_empty() => {
            let parsed = port.parse::<u16>().map_err(|_| {
                LeafError::invalid_argument("websocket:connect", format!("invalid port `{port}`"))
            })?;
            (host.to_owned(), parsed)
        }
        _ => (authority.to_owned(), if secure { 443 } else { 80 }),
    };
    Ok((secure, host, port, path.to_owned()))
}

fn wrap_tls(stream: TcpStream, host: &str, ca_path: Option<&str>) -> Result<HttpStream, LeafError> {
    // WHY: installing the process-wide crypto provider is idempotent; an error
    // only means another caller installed one first.
    drop(rustls::crypto::ring::default_provider().install_default());
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = ca_path {
        let bytes = std::fs::read(path)
            .map_err(|err| LeafError::from_io("websocket:connect", path, &err))?;
        for cert in rustls_pemfile::certs(&mut bytes.as_slice()) {
            let cert = cert.map_err(|err| {
                LeafError::invalid_argument("websocket:connect", format!("ca pem: {err}"))
            })?;
            roots.add(cert).map_err(|err| {
                LeafError::invalid_argument("websocket:connect", format!("ca: {err}"))
            })?;
        }
    }
    let config = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );
    // WHY: the URL host keeps its brackets for `addr` and the handshake URL, but a
    // TLS server name is the bare IP literal.
    let bare_host = host.trim_start_matches('[').trim_end_matches(']');
    let name = ServerName::try_from(bare_host.to_owned()).map_err(|_| {
        LeafError::invalid_argument(
            "websocket:connect",
            format!("invalid TLS server name `{host}`"),
        )
    })?;
    let conn = ClientConnection::new(config, name)
        .map_err(|err| LeafError::unavailable("websocket:connect", format!("tls client: {err}")))?;
    Ok(HttpStream::TlsClient(rustls::StreamOwned::new(
        conn, stream,
    )))
}
