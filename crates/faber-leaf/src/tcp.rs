//! Blocking TCP/TLS leaf. Serves Latin `caelum:*` routes.
//!
//! Handles live in this crate. Norma `Socket` / `Listener` carry only the id.
//! rustls is the TLS stack (A2 / Q4). No async runtime.

#[cfg(test)]
#[path = "tcp_test.rs"]
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

static NEXT_ID: AtomicI64 = AtomicI64::new(1);
static HANDLES: Mutex<Option<HashMap<i64, Handle>>> = Mutex::new(None);

enum Handle {
    Listener(TcpListener),
    TlsListener {
        listener: TcpListener,
        config: Arc<ServerConfig>,
    },
    Stream(TcpStream),
    TlsClient(rustls::StreamOwned<ClientConnection, TcpStream>),
    TlsServer(rustls::StreamOwned<ServerConnection, TcpStream>),
}

fn with_handles<T>(f: impl FnOnce(&mut HashMap<i64, Handle>) -> T) -> T {
    let mut guard = match HANDLES.lock() {
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

/// Drop every socket so a later program does not inherit listeners.
pub fn reset_handles() {
    with_handles(HashMap::clear);
}

/// Route names this leaf serves. Must match the `serve` match arms below.
const ROUTES: &[&str] = &[
    "caelum:ausculta",
    "caelum:auscultabit",
    "caelum:necte",
    "caelum:nectet",
    "caelum:accipe",
    "caelum:accipiet",
    "caelum:hauri",
    "caelum:hauriet",
    "caelum:funde",
    "caelum:fundet",
    "caelum:claude",
    "caelum:portus",
    "caelum:remotus",
    "caelum:locus",
];

/// Whether `route` belongs to this leaf. Pure name lookup; opens no socket.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
}

/// Answer a `caelum:*` route. `None` means the route is not this leaf.
#[must_use]
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "caelum:ausculta" | "caelum:auscultabit" => listen(args),
        "caelum:necte" | "caelum:nectet" => connect(args),
        "caelum:accipe" | "caelum:accipiet" => accept(args),
        "caelum:hauri" | "caelum:hauriet" => read_bytes(args),
        "caelum:funde" | "caelum:fundet" => write_bytes(args),
        "caelum:claude" => close(args),
        "caelum:portus" => port(args),
        "caelum:remotus" => remote(args),
        "caelum:locus" => local(args),
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

fn optional_str(args: &[Arg], index: usize) -> Option<&str> {
    match args.get(index) {
        Some(Arg::Str(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn optional_bool(args: &[Arg], index: usize) -> bool {
    match args.get(index) {
        Some(Arg::Bool(value)) => *value,
        Some(Arg::Int(value)) => *value != 0,
        _ => false,
    }
}

fn listen(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "caelum:ausculta";
    let port = expect_int(ROUTE, args, 0, "port")?;
    if !(0..=65_535).contains(&port) {
        return Err(LeafError::invalid_argument(ROUTE, "port out of range"));
    }
    let listener = TcpListener::bind(("0.0.0.0", u16::try_from(port).unwrap_or(0)))
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))?;
    listener
        .set_nonblocking(false)
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))?;
    let cert = optional_str(args, 1);
    let key = optional_str(args, 2);
    let handle = match (cert, key) {
        (Some(cert), Some(key)) => Handle::TlsListener {
            listener,
            config: server_config(cert, key)?,
        },
        (None, None) => Handle::Listener(listener),
        _ => {
            return Err(LeafError::invalid_argument(
                ROUTE,
                "cert and key must both be present for TLS",
            ));
        }
    };
    Ok(Reply::Int(insert_handle(handle)))
}

fn connect(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "caelum:necte";
    let host = expect_str(ROUTE, args, 0, "host")?;
    let port = expect_int(ROUTE, args, 1, "port")?;
    if !(1..=65_535).contains(&port) {
        return Err(LeafError::invalid_argument(ROUTE, "port out of range"));
    }
    let secure = optional_bool(args, 2);
    let addr = format!("{host}:{port}");
    let stream = TcpStream::connect(&addr).map_err(|err| LeafError::from_io(ROUTE, &addr, &err))?;
    configure_stream(&stream, ROUTE)?;
    if !secure {
        return Ok(Reply::Int(insert_handle(Handle::Stream(stream))));
    }
    let config = client_config();
    let name = ServerName::try_from(host.to_owned()).map_err(|_| {
        LeafError::invalid_argument(ROUTE, format!("invalid TLS server name `{host}`"))
    })?;
    let conn = ClientConnection::new(config, name)
        .map_err(|err| LeafError::unavailable(ROUTE, format!("tls client handshake: {err}")))?;
    Ok(Reply::Int(insert_handle(Handle::TlsClient(
        rustls::StreamOwned::new(conn, stream),
    ))))
}

fn accept(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "caelum:accipe";
    let id = expect_int(ROUTE, args, 0, "id")?;
    let pending = with_handles(|map| match map.get(&id) {
        Some(Handle::Listener(listener)) => listener
            .try_clone()
            .map(PendingAccept::Plain)
            .map_err(|err| LeafError::from_io(ROUTE, "", &err)),
        Some(Handle::TlsListener { listener, config }) => listener
            .try_clone()
            .map(|cloned| PendingAccept::Tls {
                listener: cloned,
                config: Arc::clone(config),
            })
            .map_err(|err| LeafError::from_io(ROUTE, "", &err)),
        Some(_) => Err(LeafError::invalid_argument(ROUTE, "id is not a listener")),
        None => Err(LeafError::closed(ROUTE, "listener closed")),
    })?;
    let child = match pending {
        PendingAccept::Plain(listener) => {
            let (stream, _) = listener
                .accept()
                .map_err(|err| LeafError::from_io(ROUTE, "", &err))?;
            configure_stream(&stream, ROUTE)?;
            Handle::Stream(stream)
        }
        PendingAccept::Tls { listener, config } => {
            let (stream, _) = listener
                .accept()
                .map_err(|err| LeafError::from_io(ROUTE, "", &err))?;
            configure_stream(&stream, ROUTE)?;
            let conn = ServerConnection::new(config).map_err(|err| {
                LeafError::unavailable(ROUTE, format!("tls server handshake: {err}"))
            })?;
            Handle::TlsServer(rustls::StreamOwned::new(conn, stream))
        }
    };
    Ok(Reply::Int(insert_handle(child)))
}

enum PendingAccept {
    Plain(TcpListener),
    Tls {
        listener: TcpListener,
        config: Arc<ServerConfig>,
    },
}

fn read_bytes(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "caelum:hauri";
    let id = expect_int(ROUTE, args, 0, "id")?;
    let limit = match args.get(1) {
        Some(Arg::Int(value)) if *value > 0 => usize::try_from(*value).unwrap_or(usize::MAX),
        _ => 65_536,
    };
    let mut handle = take_stream(id, ROUTE)?;
    let mut buf = vec![0_u8; limit];
    let read = match &mut handle {
        Handle::Stream(stream) => stream.read(&mut buf),
        Handle::TlsClient(stream) => stream.read(&mut buf),
        Handle::TlsServer(stream) => stream.read(&mut buf),
        Handle::Listener(_) | Handle::TlsListener { .. } => {
            put_handle(id, handle);
            return Err(LeafError::invalid_argument(ROUTE, "id is a listener"));
        }
    };
    let n = match read {
        Ok(n) => n,
        Err(err) => {
            put_handle(id, handle);
            return Err(LeafError::from_io(ROUTE, "", &err));
        }
    };
    put_handle(id, handle);
    buf.truncate(n);
    Ok(Reply::Bytes(buf))
}

fn take_stream(id: i64, route: &'static str) -> Result<Handle, LeafError> {
    with_handles(|map| match map.remove(&id) {
        Some(handle @ (Handle::Stream(_) | Handle::TlsClient(_) | Handle::TlsServer(_))) => {
            Ok(handle)
        }
        Some(handle) => {
            map.insert(id, handle);
            Err(LeafError::invalid_argument(route, "id is a listener"))
        }
        None => Err(LeafError::closed(route, "socket closed")),
    })
}

fn put_handle(id: i64, handle: Handle) {
    with_handles(|map| {
        map.insert(id, handle);
    });
}

fn write_bytes(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "caelum:funde";
    let id = expect_int(ROUTE, args, 0, "id")?;
    let data = match args.get(1) {
        Some(Arg::Bytes(value)) => value.clone(),
        Some(_) => {
            return Err(LeafError::invalid_argument(ROUTE, "data must be octeti"));
        }
        None => return Err(LeafError::invalid_argument(ROUTE, "missing data")),
    };
    let mut handle = take_stream(id, ROUTE)?;
    let result = match &mut handle {
        Handle::Stream(stream) => stream.write_all(&data).and_then(|()| stream.flush()),
        Handle::TlsClient(stream) => stream.write_all(&data).and_then(|()| stream.flush()),
        Handle::TlsServer(stream) => stream.write_all(&data).and_then(|()| stream.flush()),
        Handle::Listener(_) | Handle::TlsListener { .. } => {
            put_handle(id, handle);
            return Err(LeafError::invalid_argument(ROUTE, "id is a listener"));
        }
    };
    put_handle(id, handle);
    result
        .map(|()| Reply::Unit)
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))
}

fn close(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "caelum:claude";
    let id = expect_int(ROUTE, args, 0, "id")?;
    with_handles(|map| match map.remove(&id) {
        Some(Handle::Stream(stream)) => {
            // WHY: close is best-effort teardown; the stream is discarded and
            // a shutdown error is not actionable.
            drop(stream.shutdown(Shutdown::Both));
        }
        Some(Handle::TlsClient(mut stream)) => {
            // WHY: close is best-effort teardown; a failed final flush on a
            // discarded TLS stream is not actionable.
            drop(stream.flush());
        }
        Some(Handle::TlsServer(mut stream)) => {
            // WHY: close is best-effort teardown; a failed final flush on a
            // discarded TLS stream is not actionable.
            drop(stream.flush());
        }
        Some(Handle::Listener(_) | Handle::TlsListener { .. }) | None => {}
    });
    Ok(Reply::Unit)
}

fn port(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "caelum:portus";
    let id = expect_int(ROUTE, args, 0, "id")?;
    with_handles(|map| match map.get(&id) {
        Some(Handle::Listener(listener) | Handle::TlsListener { listener, .. }) => listener
            .local_addr()
            .map(|addr| Reply::Int(i64::from(addr.port())))
            .map_err(|err| LeafError::from_io(ROUTE, "", &err)),
        Some(_) => Err(LeafError::invalid_argument(ROUTE, "id is not a listener")),
        None => Err(LeafError::closed(ROUTE, "listener closed")),
    })
}

fn remote(args: &[Arg]) -> Result<Reply, LeafError> {
    endpoint_of(args, "caelum:remotus", true)
}

fn local(args: &[Arg]) -> Result<Reply, LeafError> {
    endpoint_of(args, "caelum:locus", false)
}

fn endpoint_of(args: &[Arg], route: &'static str, remote: bool) -> Result<Reply, LeafError> {
    let id = expect_int(route, args, 0, "id")?;
    with_handles(|map| {
        let addr = match map.get(&id) {
            Some(Handle::Stream(stream)) => pick_addr(stream, remote),
            Some(Handle::TlsClient(stream)) => pick_addr(&stream.sock, remote),
            Some(Handle::TlsServer(stream)) => pick_addr(&stream.sock, remote),
            Some(_) => return Err(LeafError::invalid_argument(route, "id is a listener")),
            None => return Err(LeafError::closed(route, "socket closed")),
        }
        .map_err(|err| LeafError::from_io(route, "", &err))?;
        Ok(Reply::Map(vec![
            ("host".to_owned(), Reply::Str(addr.ip().to_string())),
            ("port".to_owned(), Reply::Int(i64::from(addr.port()))),
        ]))
    })
}

fn pick_addr(stream: &TcpStream, remote: bool) -> std::io::Result<std::net::SocketAddr> {
    if remote {
        stream.peer_addr()
    } else {
        stream.local_addr()
    }
}

fn configure_stream(stream: &TcpStream, route: &'static str) -> Result<(), LeafError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|err| LeafError::from_io(route, "", &err))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(60)))
        .map_err(|err| LeafError::from_io(route, "", &err))
}

fn install_ring() {
    // WHY: installing the process-wide crypto provider is idempotent; an error
    // only means another caller installed one first.
    drop(rustls::crypto::ring::default_provider().install_default());
}

fn client_config() -> Arc<ClientConfig> {
    install_ring();
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn server_config(cert_path: &str, key_path: &str) -> Result<Arc<ServerConfig>, LeafError> {
    const ROUTE: &str = "caelum:ausculta";
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
