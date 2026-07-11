use super::*;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[test]
fn response_strings_escape_json_metacharacters() {
    assert_eq!(json_string("a\"b\\c\n"), r#""a\"b\\c\n""#);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn api5_real_loopback_service_proves_product_outcomes() {
    let service = ServiceFixture::serve().await.expect("serve fixture");
    let addr = service.local_addr();

    let shown = request(
        addr,
        "GET /api/items/42?verbose=true HTTP/1.1\r\nHost: test\r\nX-Client: proof\r\n\r\n",
    )
    .await;
    assert_response(
        &shown,
        200,
        r#"{"id":"42","verbose":"true","client":"proof"}"#,
    );
    assert!(shown.contains("x-faber-middleware: request-id"), "{shown}");

    let body = r#"{"name":"Ada"}"#;
    let created = request(
        addr,
        &format!(
            "POST /api/items HTTP/1.1\r\nHost: test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
    .await;
    assert_response(&created, 201, r#"{"name":"Ada"}"#);

    let bad = request(
        addr,
        "POST /api/items HTTP/1.1\r\nHost: test\r\nContent-Length: 2\r\n\r\n[]",
    )
    .await;
    assert_response(&bad, 400, "invalid_json_object");

    let failed = request(addr, "GET /api/fail HTTP/1.1\r\nHost: test\r\n\r\n").await;
    assert_response(&failed, 500, r#"{"error":true,"issue":"fixture_failure"}"#);

    let missing = request(addr, "GET /absent HTTP/1.1\r\nHost: test\r\n\r\n").await;
    assert_response(&missing, 404, "route_not_found");

    let started = Instant::now();
    let (first, second) = tokio::join!(
        request(addr, "GET /api/slow HTTP/1.1\r\nHost: test\r\n\r\n"),
        request(addr, "GET /api/slow HTTP/1.1\r\nHost: test\r\n\r\n")
    );
    assert_response(&first, 200, "count");
    assert_response(&second, 200, "count");
    assert!(
        started.elapsed() < Duration::from_millis(220),
        "slow requests ran sequentially: {:?}",
        started.elapsed()
    );
    assert_eq!(service.counter().expect("counter"), 2);

    service.shutdown().await;
}

async fn request(addr: SocketAddr, wire: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    stream
        .write_all(wire.as_bytes())
        .await
        .expect("write request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read response");
    String::from_utf8(response).expect("utf8 response")
}

fn assert_response(response: &str, status: u16, body_fragment: &str) {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {status} ")),
        "unexpected response: {response}"
    );
    assert!(
        response.contains("content-type: application/json"),
        "missing json content type: {response}"
    );
    assert!(
        response.contains(body_fragment),
        "unexpected body: {response}"
    );
    assert!(
        response.contains("x-faber-request-id:"),
        "missing request correlation: {response}"
    );
}
