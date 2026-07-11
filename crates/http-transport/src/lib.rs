//! G9 API1 HTTP transport: Tokio/hyper accept, full-body carriers, request-id
//! correlation, body limits, one-request-per-connection, and graceful shutdown.
//!
//! TARGET: Rust host binding/provider surface for the future public `http`
//! package. Lives in the HIR packet staging tree until the package scaffold
//! owns this code.
//!
//! WHY: API0 approved the unified `faber::frame` sermo substrate. API1 adds
//! socket accept and demux **without** a second queue model: each connection
//! is one request (v1 limit), correlated by request id on the host map and on
//! response headers. Multiplexed `subsermo(parent_id)` remains an additive
//! residual for multi-request connections.
//!
//! EDGE: one-request-per-connection is deliberate (`Connection: close`); keep-alive
//! and HTTP/2 are out of scope for this unit.

use bytes::Bytes;
use faber::frame;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::cmp::min;
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::timeout;

const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);

/// Default max body (1 MiB) for the first transport slice.
pub const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

/// Header carrying the transport-assigned request id on every response.
pub const REQUEST_ID_HEADER: &str = "x-faber-request-id";

/// Full-body HTTP request carrier.
#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub id: String,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

/// Full-body HTTP response carrier.
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

impl HttpResponse {
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
            body: Bytes::from(body.into()),
        }
    }

    pub fn empty(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Bytes::new(),
        }
    }
}

/// Transport limits and posture.
#[derive(Clone, Debug)]
pub struct TransportConfig {
    pub max_body_bytes: usize,
    /// Bound on body collect + handler for one request.
    pub request_timeout: Duration,
    /// Max concurrent in-flight connections (accept still queues OS-side).
    pub max_in_flight: usize,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            request_timeout: Duration::from_secs(30),
            max_in_flight: 64,
        }
    }
}

/// Host-side correlation ledger for in-flight request ids.
///
/// This is demux bookkeeping on the transport, not a second sermo frame queue.
#[derive(Default)]
pub struct CorrelationTable {
    inner: Mutex<HashMap<String, CorrelationEntry>>,
}

#[derive(Clone, Debug)]
pub struct CorrelationEntry {
    pub method: String,
    pub path: String,
    pub completed: bool,
    pub status: Option<u16>,
}

impl CorrelationTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin(&self, id: &str, method: &str, path: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.insert(
                id.to_owned(),
                CorrelationEntry {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    completed: false,
                    status: None,
                },
            );
        }
    }

    pub fn complete(&self, id: &str, status: u16) {
        if let Ok(mut map) = self.inner.lock() {
            if let Some(mut entry) = map.remove(id) {
                entry.completed = true;
                entry.status = Some(status);
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<CorrelationEntry> {
        self.inner.lock().ok().and_then(|map| map.get(id).cloned())
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

type BoxHandler =
    Arc<dyn Fn(HttpRequest) -> Pin<Box<dyn Future<Output = HttpResponse> + Send>> + Send + Sync>;

/// Running HTTP/1 accept loop with graceful cancel.
pub struct HttpTransport {
    local_addr: SocketAddr,
    config: TransportConfig,
    cancel: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    correlations: Arc<CorrelationTable>,
    in_flight: Arc<AtomicUsize>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl HttpTransport {
    /// Bind `127.0.0.1:0` (or `addr`) and start serving with `handler`.
    pub async fn serve<F, Fut>(
        addr: SocketAddr,
        config: TransportConfig,
        handler: F,
    ) -> Result<Self, TransportError>
    where
        F: Fn(HttpRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| TransportError::bind(e.to_string()))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| TransportError::bind(e.to_string()))?;

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_notify = Arc::new(Notify::new());
        let correlations = Arc::new(CorrelationTable::new());
        let in_flight = Arc::new(AtomicUsize::new(0));
        let slots = Arc::new(Semaphore::new(config.max_in_flight.max(1)));
        let connections = Arc::new(Mutex::new(Vec::new()));

        let handler: BoxHandler = Arc::new(move |req| Box::pin(handler(req)));
        let join = tokio::spawn(accept_loop(
            listener,
            config.clone(),
            handler,
            Arc::clone(&cancel),
            Arc::clone(&cancel_notify),
            Arc::clone(&correlations),
            Arc::clone(&in_flight),
            Arc::clone(&connections),
            slots,
        ));

        Ok(Self {
            local_addr,
            config,
            cancel,
            cancel_notify,
            correlations,
            in_flight,
            connections,
            join: Mutex::new(Some(join)),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn config(&self) -> &TransportConfig {
        &self.config
    }

    pub fn correlations(&self) -> &CorrelationTable {
        &self.correlations
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Signal accept loop to stop; in-flight handlers observe cancel mid-body when timed.
    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.cancel_notify.notify_waiters();
    }

    /// Shutdown and wait for the accept task to finish.
    pub async fn shutdown_and_join(self) {
        self.shutdown();
        let handle = self.join.lock().ok().and_then(|mut g| g.take());
        if let Some(handle) = handle {
            let _ = handle.await;
        }
        drain_connection_tasks(self.connections).await;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportError {
    pub issue: &'static str,
    pub message: String,
}

impl TransportError {
    fn bind(message: impl Into<String>) -> Self {
        Self {
            issue: "http_transport_bind_failed",
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.issue, self.message)
    }
}

impl std::error::Error for TransportError {}

#[allow(clippy::too_many_arguments)]
async fn accept_loop(
    listener: TcpListener,
    config: TransportConfig,
    handler: BoxHandler,
    cancel: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    correlations: Arc<CorrelationTable>,
    in_flight: Arc<AtomicUsize>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    slots: Arc<Semaphore>,
) {
    loop {
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        let accept = listener.accept();
        let cancelled = cancel_notify.notified();
        tokio::pin!(accept);
        tokio::pin!(cancelled);

        let (stream, _peer) = tokio::select! {
            ready = &mut accept => match ready {
                Ok(pair) => pair,
                Err(_) => {
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    continue;
                }
            },
            _ = &mut cancelled => break,
        };

        if cancel.load(Ordering::SeqCst) {
            break;
        }

        let permit = match slots.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                tokio::spawn(reject_busy_connection(
                    stream,
                    busy_rejection_timeout(config.request_timeout),
                ));
                continue;
            }
        };

        let config = config.clone();
        let handler = Arc::clone(&handler);
        let cancel = Arc::clone(&cancel);
        let correlations = Arc::clone(&correlations);
        let in_flight = Arc::clone(&in_flight);
        let request_timeout = config.request_timeout;

        in_flight.fetch_add(1, Ordering::SeqCst);
        let connection = tokio::spawn(async move {
            let _permit = permit;
            let io = TokioIo::new(stream);
            let service = service_fn(move |req| {
                let handler = Arc::clone(&handler);
                let config = config.clone();
                let cancel = Arc::clone(&cancel);
                let correlations = Arc::clone(&correlations);
                async move {
                    Ok::<_, Infallible>(
                        handle_connection(req, handler, config, cancel, correlations).await,
                    )
                }
            });

            // One request per connection: disable keep-alive.
            let conn = serve_http1(io, service, request_timeout);
            let _ = conn.await;
            in_flight.fetch_sub(1, Ordering::SeqCst);
        });
        if let Ok(mut tasks) = connections.lock() {
            tasks.push(connection);
        }
    }
}

async fn drain_connection_tasks(connections: Arc<Mutex<Vec<JoinHandle<()>>>>) {
    let tasks = connections
        .lock()
        .ok()
        .map(|mut tasks| std::mem::take(&mut *tasks))
        .unwrap_or_default();
    for mut task in tasks {
        if timeout(SHUTDOWN_DRAIN_TIMEOUT, &mut task).await.is_err() {
            task.abort();
            let _ = task.await;
        }
    }
}

async fn handle_connection(
    req: Request<Incoming>,
    handler: BoxHandler,
    config: TransportConfig,
    cancel: Arc<AtomicBool>,
    correlations: Arc<CorrelationTable>,
) -> Response<Full<Bytes>> {
    if cancel.load(Ordering::SeqCst) {
        return with_request_id(
            Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Full::new(Bytes::from_static(b"shutting down")))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::new()))),
            "shutdown",
        );
    }

    let request_id = frame::next_frame_id();
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_owned();
    let query = uri.query().map(str::to_owned);
    let headers = collect_headers(req.headers());

    correlations.begin(&request_id, method.as_str(), &path);

    let body_result = timeout(
        config.request_timeout,
        Limited::new(req.into_body(), config.max_body_bytes).collect(),
    )
    .await;

    let body = match body_result {
        Ok(Ok(collected)) => collected.to_bytes(),
        Ok(Err(_)) => {
            correlations.complete(&request_id, StatusCode::PAYLOAD_TOO_LARGE.as_u16());
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                &request_id,
                "body exceeds max_body_bytes",
            );
        }
        Err(_) => {
            correlations.complete(&request_id, StatusCode::REQUEST_TIMEOUT.as_u16());
            return error_response(
                StatusCode::REQUEST_TIMEOUT,
                &request_id,
                "body read timeout",
            );
        }
    };

    if cancel.load(Ordering::SeqCst) {
        correlations.complete(&request_id, StatusCode::SERVICE_UNAVAILABLE.as_u16());
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &request_id,
            "shutting down",
        );
    }

    let carrier = HttpRequest {
        id: request_id.clone(),
        method: method.as_str().to_owned(),
        path,
        query,
        headers,
        body,
    };

    let response = match timeout(config.request_timeout, handler(carrier)).await {
        Ok(resp) => resp,
        Err(_) => {
            correlations.complete(&request_id, StatusCode::GATEWAY_TIMEOUT.as_u16());
            return error_response(StatusCode::GATEWAY_TIMEOUT, &request_id, "handler timeout");
        }
    };

    if cancel.load(Ordering::SeqCst) {
        correlations.complete(&request_id, StatusCode::SERVICE_UNAVAILABLE.as_u16());
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &request_id,
            "shutting down",
        );
    }

    correlations.complete(&request_id, response.status);
    build_response(response, &request_id)
}

fn collect_headers(headers: &http::HeaderMap) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            out.push((name.as_str().to_owned(), v.to_owned()));
        }
    }
    out
}

fn build_response(response: HttpResponse, request_id: &str) -> Response<Full<Bytes>> {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = Response::builder()
        .status(status)
        .header(http::header::CONNECTION, "close")
        .header(REQUEST_ID_HEADER, request_id);

    for (name, value) in &response.headers {
        if name.eq_ignore_ascii_case(REQUEST_ID_HEADER) {
            continue;
        }
        if name.eq_ignore_ascii_case("connection") {
            continue;
        }
        builder = builder.header(name.as_str(), value.as_str());
    }

    builder.body(Full::new(response.body)).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header(REQUEST_ID_HEADER, request_id)
            .header(http::header::CONNECTION, "close")
            .body(Full::new(Bytes::from_static(b"response build failed")))
            .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
    })
}

fn error_response(status: StatusCode, request_id: &str, message: &str) -> Response<Full<Bytes>> {
    with_request_id(
        Response::builder()
            .status(status)
            .header(http::header::CONNECTION, "close")
            .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Full::new(Bytes::from(message.to_owned())))
            .unwrap_or_else(|_| Response::new(Full::new(Bytes::from(message.to_owned())))),
        request_id,
    )
}

fn with_request_id(mut response: Response<Full<Bytes>>, request_id: &str) -> Response<Full<Bytes>> {
    if let Ok(value) = http::HeaderValue::from_str(request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response.headers_mut().insert(
        http::header::CONNECTION,
        http::HeaderValue::from_static("close"),
    );
    response
}

fn serve_http1<S>(
    io: TokioIo<S>,
    service: impl hyper::service::Service<
        Request<Incoming>,
        Response = Response<Full<Bytes>>,
        Error = Infallible,
    >,
    request_timeout: Duration,
) -> impl Future<Output = Result<(), hyper::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = http1::Builder::new();
    builder.keep_alive(false);
    builder.timer(TokioTimer::new());
    builder.header_read_timeout(Some(request_timeout));
    builder.serve_connection(io, service)
}

fn busy_rejection_timeout(request_timeout: Duration) -> Duration {
    min(request_timeout, Duration::from_millis(250))
}

async fn reject_busy_connection(stream: tokio::net::TcpStream, timeout_after: Duration) {
    use tokio::io::AsyncWriteExt;

    let request_id = frame::next_frame_id();
    let response = format!(
        "HTTP/1.1 503 Service Unavailable\r\ncontent-type: text/plain; charset=utf-8\r\nconnection: close\r\n{REQUEST_ID_HEADER}: {request_id}\r\ncontent-length: 11\r\n\r\nserver busy"
    );
    let _ = timeout(timeout_after, async {
        let mut stream = stream;
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
    })
    .await;
}

#[cfg(test)]
#[path = "http_transport_test.rs"]
mod http_transport_test;
