//! Blocking WebSocket leaf. Serves `websocket:*` routes.
//!
//! tungstenite is the framing stack (A2 / Q4). TLS reuses the HTTP rustls
//! client path. No async runtime.

#[cfg(test)]
#[path = "websocket_test.rs"]
mod tests;

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

struct Socket {
    socket: WebSocket<HttpStream>,
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
        map.insert(id, Socket { socket });
    });
    id
}

/// Drop every WebSocket so a later program does not inherit peers.
pub fn reset_handles() {
    with_handles(HashMap::clear);
}

/// Whether `route` belongs to this leaf.
#[must_use]
pub fn is_route(route: &str) -> bool {
    serve(route, &[]).is_some()
}

/// Answer a `websocket:*` route. `None` means the route is not this leaf.
#[must_use]
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "websocket:connect" => connect(args),
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
    let request = handshake.into_client_request().map_err(|err| {
        LeafError::invalid_argument(ROUTE, format!("websocket request: {err}"))
    })?;
    let (socket, _) = tungstenite::client(request, stream)
        .map_err(|err| LeafError::unavailable(ROUTE, format!("websocket handshake: {err}")))?;
    Ok(Reply::Int(insert(socket)))
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
        Ok(Message::Close(_)) | Err(tungstenite::Error::AlreadyClosed) => Ok(Reply::Map(vec![
            ("text".to_owned(), Reply::Str(String::new())),
            ("done".to_owned(), Reply::Bool(true)),
        ])),
        Ok(Message::Ping(payload)) => {
            let _ = socket.send(Message::Pong(payload));
            Ok(Reply::Map(vec![
                ("text".to_owned(), Reply::Str(String::new())),
                ("done".to_owned(), Reply::Bool(false)),
            ]))
        }
        Ok(_) => Ok(Reply::Map(vec![
            ("text".to_owned(), Reply::Str(String::new())),
            ("done".to_owned(), Reply::Bool(false)),
        ])),
        Err(err) => Err(LeafError::from_io(
            ROUTE,
            "",
            &std::io::Error::other(err),
        )),
    })
}

fn close(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "websocket:close";
    let id = expect_int(ROUTE, args, 0, "id")?;
    with_handles(|map| {
        if let Some(mut socket) = map.remove(&id) {
            let _ = socket.socket.close(None);
        }
    });
    Ok(Reply::Unit)
}

fn with_socket<T>(
    id: i64,
    route: &'static str,
    f: impl FnOnce(&mut WebSocket<HttpStream>) -> Result<T, LeafError>,
) -> Result<T, LeafError> {
    let mut socket = with_handles(|map| match map.remove(&id) {
        Some(socket) => Ok(socket),
        None => Err(LeafError::closed(route, "socket closed")),
    })?;
    let result = f(&mut socket.socket);
    with_handles(|map| {
        map.insert(id, socket);
    });
    result
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
    let (host, port) = match authority.rsplit_once(':') {
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

fn wrap_tls(
    stream: TcpStream,
    host: &str,
    ca_path: Option<&str>,
) -> Result<HttpStream, LeafError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = ca_path {
        let bytes =
            std::fs::read(path).map_err(|err| LeafError::from_io("websocket:connect", path, &err))?;
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
    let name = ServerName::try_from(host.to_owned()).map_err(|_| {
        LeafError::invalid_argument(
            "websocket:connect",
            format!("invalid TLS server name `{host}`"),
        )
    })?;
    let conn = ClientConnection::new(config, name).map_err(|err| {
        LeafError::unavailable("websocket:connect", format!("tls client: {err}"))
    })?;
    Ok(HttpStream::TlsClient(rustls::StreamOwned::new(conn, stream)))
}
