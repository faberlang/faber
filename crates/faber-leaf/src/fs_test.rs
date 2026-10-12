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
