use super::{MAX_RANGE_READ_BYTES, octeti_bytes, read_byte_range};
use crate::Value;
use radix_lexer::Span;
use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
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

#[test]
fn range_read_of_a_large_span_is_one_exact_read() {
    const LEN: u64 = 70 * 1024 * 1024;
    let path = unique_path("large");
    let file = fs::File::create(&path).expect("create large fixture");
    file.set_len(LEN).expect("size large fixture");
    drop(file);
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("reopen large fixture");
        for (offset, byte) in [(0, 0xA1_u8), (LEN / 2, 0xB2), (LEN - 1, 0xC3)] {
            file.seek(SeekFrom::Start(offset)).expect("seek fixture");
            file.write_all(&[byte]).expect("mark fixture");
        }
    }

    let bytes = read_byte_range(&path.to_string_lossy(), 0, LEN as i64, Span::new(0, 0))
        .expect("a 70 MiB range reads in one call");
    assert_eq!(bytes.len() as u64, LEN);
    assert_eq!(bytes[0], 0xA1);
    assert_eq!(bytes[(LEN / 2) as usize], 0xB2);
    assert_eq!(bytes[(LEN - 1) as usize], 0xC3);
    assert_eq!(bytes[1], 0);

    fs::remove_file(path).expect("remove large fixture");
}

#[test]
fn range_read_over_the_ceiling_is_refused_with_the_named_error() {
    let path = unique_path("ceiling");
    fs::write(&path, b"abcdefgh").expect("write ceiling fixture");
    let path_text = path.to_string_lossy();

    let at_ceiling = read_byte_range(&path_text, 0, MAX_RANGE_READ_BYTES as i64, Span::new(0, 0))
        .expect("a request at the ceiling is admitted");
    assert_eq!(at_ceiling, b"abcdefgh");

    let over = read_byte_range(
        &path_text,
        0,
        MAX_RANGE_READ_BYTES as i64 + 1,
        Span::new(0, 0),
    )
    .expect_err("a request above the ceiling is refused");
    assert_eq!(over.code, crate::RUNNER_CODE_INTERNAL);
    assert_eq!(
        over.issue,
        format!("runner_solum_read_range_longitudo_must_be_at_most_{MAX_RANGE_READ_BYTES}_bytes")
    );

    fs::remove_file(path).expect("remove ceiling fixture");
}

#[test]
fn range_read_refuses_negative_start_and_length() {
    let path = unique_path("negative");
    fs::write(&path, b"abcdefgh").expect("write negative fixture");
    let path_text = path.to_string_lossy();

    let start = read_byte_range(&path_text, -1, 1, Span::new(0, 0))
        .expect_err("a negative start is refused");
    assert_eq!(start.code, crate::RUNNER_CODE_INTERNAL);
    assert_eq!(
        start.issue,
        "runner_solum_read_range_initium_must_be_non_negative"
    );
    let length = read_byte_range(&path_text, 0, -1, Span::new(0, 0))
        .expect_err("a negative length is refused");
    assert_eq!(length.code, crate::RUNNER_CODE_INTERNAL);
    assert_eq!(
        length.issue,
        "runner_solum_read_range_longitudo_must_be_non_negative"
    );

    fs::remove_file(path).expect("remove negative fixture");
}

#[test]
fn octeti_bytes_accepts_the_byte_value_and_a_b8_list() {
    let list = |items: Vec<Value>| Value::Array(Rc::new(RefCell::new(items)));
    assert_eq!(octeti_bytes(&Value::Octeti(vec![1, 2])), Some(vec![1, 2]));
    assert_eq!(
        octeti_bytes(&list(vec![Value::UInt(0x41), Value::UInt(255)])),
        Some(vec![0x41, 255])
    );
    assert_eq!(octeti_bytes(&list(vec![Value::UInt(256)])), None);
    assert_eq!(octeti_bytes(&list(vec![Value::Bool(true)])), None);
    assert_eq!(octeti_bytes(&Value::Unit), None);
}
