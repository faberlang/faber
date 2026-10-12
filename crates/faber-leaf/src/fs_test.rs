use super::{Arg, Reply, is_route, serve};
use crate::FsErrorKind;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn temp_path(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("faber-leaf-fs-{label}-{n}"))
}

fn serve_ok(route: &str, args: &[Arg]) -> Reply {
    serve(route, args)
        .expect("fs route")
        .unwrap_or_else(|err| panic!("{route} failed: {err}"))
}

#[test]
fn unknown_route_is_not_this_leaf() {
    assert!(!is_route("consolum:dic"));
    assert!(serve("consolum:dic", &[]).is_none());
}

#[test]
fn write_then_read_text_round_trips() {
    let path = temp_path("roundtrip");
    let path_text = path.to_string_lossy().into_owned();
    assert!(matches!(
        serve_ok(
            "solum:scribe",
            &[
                Arg::Str(path_text.clone()),
                Arg::Str("roundtrip-ok".to_owned()),
            ],
        ),
        Reply::Unit
    ));
    assert_eq!(
        serve_ok("solum:lege", &[Arg::Str(path_text)]),
        Reply::Str("roundtrip-ok".to_owned())
    );
    let _ = fs::remove_file(path);
}

#[test]
fn missing_file_is_not_found() {
    let error = serve(
        "solum:lege",
        &[Arg::Str("/no/such/faber-leaf-file".to_owned())],
    )
    .expect("fs route")
    .expect_err("missing file");
    assert_eq!(error.kind(), FsErrorKind::NotFound);
}

#[test]
fn temp_dir_is_non_empty() {
    match serve_ok("solum:temporarium", &[]) {
        Reply::Str(path) => assert!(!path.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn status_reports_byte_length() {
    let path = temp_path("status");
    let path_text = path.to_string_lossy().into_owned();
    fs::write(&path, "stat").expect("write");
    let Reply::Map(fields) = serve_ok("solum:status", &[Arg::Str(path_text)]) else {
        panic!("expected status map");
    };
    assert!(fields.contains(&("byte_length".to_owned(), Reply::Int(4))));
    assert!(fields.contains(&("is_directory".to_owned(), Reply::Bool(false))));
    let _ = fs::remove_file(path);
}

#[test]
fn extension_includes_the_dot() {
    assert_eq!(
        serve_ok("solum:suffixum", &[Arg::Str("a/b.fab".to_owned())]),
        Reply::Str(".fab".to_owned())
    );
}
