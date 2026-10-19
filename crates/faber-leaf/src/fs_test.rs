use super::{Arg, Reply, is_route, serve};
use crate::FsErrorKind;
use std::fs;
use std::path::{Path, PathBuf};
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
fn read_lines_splits_on_newlines() {
    let path = temp_path("lines");
    let path_text = path.to_string_lossy().into_owned();
    fs::write(&path, "one\ntwo\nthree\n").expect("write");
    assert_eq!(
        serve_ok("solum:carpe", &[Arg::Str(path_text)]),
        Reply::Strs(vec!["one".to_owned(), "two".to_owned(), "three".to_owned()])
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
fn temp_dir_is_canonical_without_trailing_separator() {
    let Reply::Str(path) = serve_ok("solum:temporarium", &[]) else {
        panic!("expected string reply");
    };
    assert!(
        path == "/" || !path.ends_with('/'),
        "trailing slash: {path}"
    );
    if let Ok(canonical) = fs::canonicalize(std::env::temp_dir()) {
        assert_eq!(path, canonical.to_string_lossy());
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

#[test]
fn route_list_matches_serve() {
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("solum:nope"));
    assert!(serve("solum:nope", &[]).is_none());
}

#[test]
fn conde_replaces_text_and_bytes_completely() {
    let dir = temp_path("conde");
    fs::create_dir_all(&dir).expect("create dir");
    let path = dir.join("target.bin");
    let path_text = path.to_string_lossy().into_owned();
    assert!(matches!(
        serve_ok(
            "solum:funde",
            &[Arg::Str(path_text.clone()), Arg::Bytes(b"old".to_vec())],
        ),
        Reply::Unit
    ));
    assert_eq!(fs::read(&path).expect("read"), b"old".as_slice());
    assert!(matches!(
        serve_ok(
            "solum:conde",
            &[
                Arg::Str(path_text.clone()),
                Arg::Str("new-complete".to_owned()),
            ],
        ),
        Reply::Unit
    ));
    assert_eq!(
        fs::read(&path).expect("read"),
        b"new-complete".as_slice(),
        "text replace must not leave old bytes"
    );
    assert!(matches!(
        serve_ok(
            "solum:conde",
            &[Arg::Str(path_text), Arg::Bytes(vec![0x00, 0xff, 0x10]),],
        ),
        Reply::Unit
    ));
    assert_eq!(
        fs::read(&path).expect("read"),
        vec![0x00, 0xff, 0x10],
        "bytes replace must be exact, non-UTF-8 included"
    );
    let names: Vec<_> = fs::read_dir(&dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(
        names,
        vec![std::ffi::OsString::from("target.bin")],
        "no conde temporary may survive"
    );
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn conde_failure_leaves_old_file_intact() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_path("conde-failure");
    fs::create_dir_all(&dir).expect("create dir");
    let target = dir.join("target.bin");
    fs::write(&target, b"old-complete").expect("write old");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).expect("read-only dir");

    let error = serve(
        "solum:conde",
        &[
            Arg::Str(target.to_string_lossy().into_owned()),
            Arg::Str("new".to_owned()),
        ],
    )
    .expect("fs route")
    .expect_err("a read-only directory must refuse the temporary");
    assert_eq!(error.kind(), FsErrorKind::Permission);

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("restore dir");
    assert_eq!(
        fs::read(&target).expect("read target"),
        b"old-complete".as_slice(),
        "the failed replace must leave the old complete file"
    );
    let names: Vec<_> = fs::read_dir(&dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(
        names,
        vec![std::ffi::OsString::from("target.bin")],
        "the failed replace must not leave a temporary behind"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn conde_directory_target_errors_and_leaves_no_temporary() {
    let dir = temp_path("conde-directory");
    fs::create_dir_all(&dir).expect("create dir");
    let sibling = dir.join("sibling.bin");
    fs::write(&sibling, b"sibling").expect("write sibling");
    let target_dir = dir.join("target-dir");
    fs::create_dir(&target_dir).expect("create target dir");

    serve(
        "solum:conde",
        &[
            Arg::Str(target_dir.to_string_lossy().into_owned()),
            Arg::Bytes(vec![1, 2, 3]),
        ],
    )
    .expect("fs route")
    .expect_err("a file cannot replace a directory");

    let mut names: Vec<_> = fs::read_dir(&dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            std::ffi::OsString::from("sibling.bin"),
            std::ffi::OsString::from("target-dir"),
        ],
        "only the pre-existing entries may remain, and no .faber-conde temporary"
    );
    assert!(target_dir.is_dir(), "the directory target was not modified");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn touch_updates_mtime_and_does_not_truncate() {
    let path = temp_path("touch");
    let path_text = path.to_string_lossy().into_owned();
    fs::write(&path, "content").expect("write");
    let file = fs::File::options().write(true).open(&path).expect("open");
    file.set_modified(std::time::SystemTime::UNIX_EPOCH)
        .expect("set old mtime");
    drop(file);
    let mut perms = fs::metadata(&path).expect("metadata").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms).expect("readonly");

    assert!(matches!(
        serve_ok("solum:tange", &[Arg::Str(path_text.clone())]),
        Reply::Unit
    ));

    let modified = fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    assert!(
        modified > std::time::SystemTime::UNIX_EPOCH,
        "touch must advance mtime"
    );
    assert_eq!(fs::read_to_string(&path).expect("read"), "content");
    let _ = fs::remove_file(path);
}

#[test]
fn touch_creates_missing_file() {
    let path = temp_path("touch-create");
    let path_text = path.to_string_lossy().into_owned();
    assert!(matches!(
        serve_ok("solum:tange", &[Arg::Str(path_text)]),
        Reply::Unit
    ));
    assert_eq!(fs::read(&path).expect("read"), Vec::<u8>::new());
    let _ = fs::remove_file(path);
}

#[cfg(unix)]
#[test]
fn mode_follows_symlink() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let target = temp_path("mode-target");
    let link = temp_path("mode-link");
    fs::write(&target, "").expect("write");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).expect("perm");
    symlink(&target, &link).expect("symlink");
    let link_text = link.to_string_lossy().into_owned();
    let Reply::Int(mode) = serve_ok("solum:modus", &[Arg::Str(link_text)]) else {
        panic!("expected int mode");
    };
    assert_eq!(mode, 0o640, "mode must follow the symlink");
    let _ = fs::remove_file(&link);
    let _ = fs::remove_file(&target);
}

#[test]
fn resolve_missing_path_falls_back_to_cwd() {
    let relative = "faber-leaf-resolve-missing";
    let Reply::Str(resolved) = serve_ok("solum:absolve", &[Arg::Str(relative.to_owned())]) else {
        panic!("expected string");
    };
    let cwd = std::env::current_dir().expect("cwd");
    assert_eq!(PathBuf::from(&resolved), cwd.join(relative));
    assert!(Path::new(&resolved).is_absolute());

    let absolute = "/no/such/faber-leaf/resolve";
    assert_eq!(
        serve_ok("solum:absolve", &[Arg::Str(absolute.to_owned())]),
        Reply::Str(absolute.to_owned()),
        "an absolute path is returned as-is"
    );
}
