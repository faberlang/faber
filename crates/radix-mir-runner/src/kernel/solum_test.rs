use super::read_byte_range;
use radix_lexer::Span;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_path(suffix: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should follow the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "radix-solum-read-range-{}-{nonce}-{suffix}",
        std::process::id()
    ))
}

#[test]
fn range_read_returns_exact_bytes_and_a_short_read_at_eof() {
    let path = unique_path("artifact");
    fs::write(&path, b"abcdefgh").expect("write range fixture");

    let path_text = path.to_string_lossy();
    let first = read_byte_range(&path_text, 2, 3, Span::new(0, 0)).expect("first range");
    let second = read_byte_range(&path_text, 6, 4, Span::new(0, 0)).expect("second range");
    assert_eq!(first, b"cde");
    assert_eq!(second, b"gh");

    fs::remove_file(path).expect("remove range fixture");
}
