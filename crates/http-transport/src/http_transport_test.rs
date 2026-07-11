use super::*;
use http_body_util::{BodyExt, Full};
use hyper::{Request, StatusCode};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::sleep;

fn loopback() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}

fn client() -> Client<HttpConnector, Full<Bytes>> {
    Client::builder(TokioExecutor::new()).build_http()
}

async fn get_text(uri: &str, body: &str) -> (StatusCode, String, Option<String>) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "text/plain")
        .body(Full::new(Bytes::from(body.to_owned())))
        .expect("request");
    let resp = client().request(req).await.expect("client request");
    let status = resp.status();
    let id = resp
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let collected = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&collected).into_owned(), id)
}

#[tokio::test]
async fn concurrent_correlation_two_slow_requests() {
    let delay = Duration::from_millis(120);
    let transport = HttpTransport::serve(
        loopback(),
        TransportConfig {
            request_timeout: Duration::from_secs(5),
            ..TransportConfig::default()
        },
        move |req| {
            let delay = delay;
            async move {
                sleep(delay).await;
                let body = format!("{}:{}", req.id, String::from_utf8_lossy(&req.body));
                HttpResponse::text(200, body)
            }
        },
    )
    .await
    .expect("bind");

    let base = format!("http://{}", transport.local_addr());
    let started = Instant::now();
    let a = tokio::spawn({
        let base = base.clone();
        async move { get_text(&format!("{base}/a"), "alpha").await }
    });
    let b = tokio::spawn({
        let base = base.clone();
        async move { get_text(&format!("{base}/b"), "beta").await }
    });

    let (sa, ba, ida) = a.await.expect("join a");
    let (sb, bb, idb) = b.await.expect("join b");
    let elapsed = started.elapsed();

    assert_eq!(sa, StatusCode::OK);
    assert_eq!(sb, StatusCode::OK);
    let ida = ida.expect("id a");
    let idb = idb.expect("id b");
    assert_ne!(ida, idb, "request ids must differ");
    assert_eq!(ba, format!("{ida}:alpha"));
    assert_eq!(bb, format!("{idb}:beta"));
    assert!(
        elapsed < delay * 2 - Duration::from_millis(20),
        "expected overlap; elapsed={elapsed:?} delay={delay:?}"
    );

    assert!(transport.correlations().get(&idb).is_none());
    assert!(transport.correlations().get(&ida).is_none());
    assert_eq!(transport.correlations().len(), 0);

    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn malformed_request_is_rejected_without_hang() {
    let transport =
        HttpTransport::serve(loopback(), TransportConfig::default(), |_req| async move {
            HttpResponse::text(200, "ok")
        })
        .await
        .expect("bind");

    let mut stream = TcpStream::connect(transport.local_addr())
        .await
        .expect("connect");
    // Not valid HTTP/1.1.
    stream
        .write_all(b"NOT-HTTP-AT-ALL\r\n\r\n")
        .await
        .expect("write");
    let mut buf = vec![0u8; 256];
    let n = timeout(Duration::from_secs(2), stream.read(&mut buf))
        .await
        .expect("read timeout wrapper")
        .expect("read");
    // Hyper may close without a response body; either clean close or 4xx is fine.
    // What must not happen is a hang past the timeout above.
    let _ = n;
    drop(stream);

    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn body_over_limit_returns_413() {
    let transport = HttpTransport::serve(
        loopback(),
        TransportConfig {
            max_body_bytes: 32,
            ..TransportConfig::default()
        },
        |_req| async move { HttpResponse::text(200, "should-not-run") },
    )
    .await
    .expect("bind");

    let uri = format!("http://{}/big", transport.local_addr());
    let body = "x".repeat(64);
    let (status, _text, id) = get_text(&uri, &body).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(id.is_some());

    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn handler_timeout_returns_504() {
    let transport = HttpTransport::serve(
        loopback(),
        TransportConfig {
            request_timeout: Duration::from_millis(50),
            ..TransportConfig::default()
        },
        |_req| async move {
            sleep(Duration::from_millis(400)).await;
            HttpResponse::text(200, "late")
        },
    )
    .await
    .expect("bind");

    let uri = format!("http://{}/slow", transport.local_addr());
    let (status, _text, id) = get_text(&uri, "hi").await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert!(id.is_some());

    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn shutdown_stops_accept_and_drains() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_h = Arc::clone(&hits);
    let transport = HttpTransport::serve(
        loopback(),
        TransportConfig {
            request_timeout: Duration::from_secs(2),
            ..TransportConfig::default()
        },
        move |_req| {
            hits_h.fetch_add(1, Ordering::SeqCst);
            async move { HttpResponse::text(200, "ok") }
        },
    )
    .await
    .expect("bind");

    let uri = format!("http://{}/once", transport.local_addr());
    let (status, body, _) = get_text(&uri, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "ok");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    transport.shutdown();
    // Accept loop should stop; further connects may fail or get 503.
    sleep(Duration::from_millis(50)).await;

    let second = client()
        .request(
            Request::builder()
                .method("GET")
                .uri(&uri)
                .body(Full::new(Bytes::new()))
                .expect("req"),
        )
        .await;
    match second {
        Ok(resp) => {
            assert!(
                resp.status() == StatusCode::SERVICE_UNAVAILABLE || resp.status().is_server_error(),
                "unexpected status {}",
                resp.status()
            );
        }
        Err(_) => {
            // Connection refused / reset after accept stopped is also clean.
        }
    }

    assert!(transport.is_cancelled());
    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn cancel_during_inflight_handler_surfaces_unavailable_or_client_error() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let gate_h = Arc::clone(&gate);
    let transport = HttpTransport::serve(
        loopback(),
        TransportConfig {
            request_timeout: Duration::from_secs(5),
            ..TransportConfig::default()
        },
        move |_req| {
            let gate = Arc::clone(&gate_h);
            async move {
                gate.notified().await;
                HttpResponse::text(200, "should-rarely-finish")
            }
        },
    )
    .await
    .expect("bind");

    let uri = format!("http://{}/hold", transport.local_addr());
    let client_task = tokio::spawn(async move { get_text(&uri, "hold").await });

    // Let the request enter the handler.
    sleep(Duration::from_millis(40)).await;
    assert!(transport.in_flight() >= 1 || !transport.correlations().is_empty());

    transport.shutdown();
    // Unblock handler after cancel so connection can complete with cancel check.
    gate.notify_waiters();

    let result = timeout(Duration::from_secs(3), client_task).await;
    match result {
        Ok(Ok((status, _, id))) => {
            // Handler may finish 200 if it raced past cancel, or 503 if cancel won.
            assert!(
                status == StatusCode::OK
                    || status == StatusCode::SERVICE_UNAVAILABLE
                    || status.is_server_error(),
                "status={status}"
            );
            let _ = id;
        }
        Ok(Err(_)) => {}
        Err(_) => panic!("client hung after shutdown"),
    }

    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn saturated_transport_returns_503_without_running_handler() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let gate_h = Arc::clone(&gate);
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_h = Arc::clone(&hits);
    let transport = HttpTransport::serve(
        loopback(),
        TransportConfig {
            request_timeout: Duration::from_secs(5),
            max_in_flight: 1,
            ..TransportConfig::default()
        },
        move |_req| {
            let gate = Arc::clone(&gate_h);
            hits_h.fetch_add(1, Ordering::SeqCst);
            async move {
                gate.notified().await;
                HttpResponse::text(200, "ok")
            }
        },
    )
    .await
    .expect("bind");

    let base = format!("http://{}", transport.local_addr());
    let first = tokio::spawn({
        let base = base.clone();
        async move { get_text(&format!("{base}/hold"), "").await }
    });
    sleep(Duration::from_millis(40)).await;

    let (status, body, id) = get_text(&format!("{base}/busy"), "").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, "server busy");
    assert!(id.is_some());
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "saturated request must not run handler"
    );

    gate.notify_waiters();
    let (first_status, first_body, _) = first.await.expect("join first");
    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(first_body, "ok");

    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn request_ids_come_from_frame_substrate() {
    // next_frame_id is the shared runtime id source (API0 substrate continuity).
    let a = frame::next_frame_id();
    let b = frame::next_frame_id();
    assert_ne!(a, b);
    assert!(!a.is_empty());
}

#[tokio::test]
async fn completed_requests_do_not_accumulate_in_correlation_table() {
    let transport =
        HttpTransport::serve(loopback(), TransportConfig::default(), |_req| async move {
            HttpResponse::text(200, "ok")
        })
        .await
        .expect("bind");

    let uri = format!("http://{}/steady", transport.local_addr());
    for _ in 0..16 {
        let (status, body, _) = get_text(&uri, "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ok");
        assert_eq!(transport.correlations().len(), 0);
    }

    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn slow_headers_and_saturation_are_time_bounded() {
    let transport = HttpTransport::serve(
        loopback(),
        TransportConfig {
            request_timeout: Duration::from_millis(100),
            max_in_flight: 1,
            ..TransportConfig::default()
        },
        |_req| async move { HttpResponse::text(200, "ok") },
    )
    .await
    .expect("bind");

    let mut first = TcpStream::connect(transport.local_addr())
        .await
        .expect("connect first");
    first
        .write_all(b"POST /hold HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n")
        .await
        .expect("write partial headers");

    sleep(Duration::from_millis(20)).await;

    let started = Instant::now();
    let mut second = TcpStream::connect(transport.local_addr())
        .await
        .expect("connect second");
    let mut buf = vec![0u8; 512];
    let n = timeout(Duration::from_secs(1), second.read(&mut buf))
        .await
        .expect("busy rejection timeout")
        .expect("busy rejection read");
    let busy_reply = String::from_utf8_lossy(&buf[..n]);
    assert!(
        busy_reply.contains("503 Service Unavailable") || n == 0,
        "expected immediate 503 or close, got: {busy_reply:?}"
    );
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "busy rejection should be bounded"
    );

    sleep(Duration::from_millis(160)).await;
    assert_eq!(transport.in_flight(), 0, "slow header slot should time out");

    transport.shutdown_and_join().await;
}

#[tokio::test]
async fn shutdown_and_join_drains_or_aborts_stalled_body_connections() {
    let transport =
        HttpTransport::serve(loopback(), TransportConfig::default(), |_req| async move {
            HttpResponse::text(200, "ok")
        })
        .await
        .expect("bind");

    let mut stream = TcpStream::connect(transport.local_addr())
        .await
        .expect("connect");
    stream
        .write_all(b"POST /hold HTTP/1.1\r\nHost: localhost\r\nContent-Length: 8\r\n\r\n")
        .await
        .expect("write partial body");

    sleep(Duration::from_millis(40)).await;
    assert_eq!(transport.in_flight(), 1, "partial body should hold one slot");

    let started = Instant::now();
    timeout(Duration::from_secs(2), transport.shutdown_and_join())
        .await
        .expect("shutdown join timeout");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "shutdown should not wait indefinitely on stalled body"
    );

    let mut buf = [0u8; 1];
    let read = timeout(Duration::from_secs(1), stream.read(&mut buf))
        .await
        .expect("read timeout")
        .expect("read");
    assert_eq!(read, 0, "connection should be closed after shutdown");
}
