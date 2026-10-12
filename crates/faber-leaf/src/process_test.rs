use super::{Arg, Reply, install_argv, is_route, reset_argv, serve};

#[test]
fn route_list_matches_serve() {
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("processus:nope"));
    assert!(serve("processus:nope", &[]).is_none());
}

#[test]
fn argv_uses_the_installed_override() {
    install_argv(vec!["alpha".to_owned(), "beta".to_owned()]);
    let reply = serve("processus:argumenta", &[])
        .expect("route")
        .expect("argv");
    assert_eq!(
        reply,
        Reply::Strs(vec!["alpha".to_owned(), "beta".to_owned()])
    );
    reset_argv();
}

#[test]
fn pid_is_positive() {
    match serve("processus:identitas", &[])
        .expect("route")
        .expect("pid")
    {
        Reply::Int(pid) => assert!(pid > 0),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn cwd_is_non_empty() {
    match serve("processus:sedes", &[]).expect("route").expect("cwd") {
        Reply::Str(path) => assert!(!path.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn capture_returns_status_stdout_and_stderr() {
    let reply = serve(
        "processus:captura",
        &[Arg::Strs(vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "printf out; printf err >&2; exit 7".to_owned(),
        ])],
    )
    .expect("route")
    .expect("capture");
    let Reply::Map(fields) = reply else {
        panic!("expected map, got {reply:?}");
    };
    assert!(fields.contains(&("status".to_owned(), Reply::Int(7))));
    assert!(fields.contains(&("stdout".to_owned(), Reply::Str("out".to_owned()))));
    assert!(fields.contains(&("stderr".to_owned(), Reply::Str("err".to_owned()))));
}

#[test]
fn exec_returns_stdout() {
    let reply = serve("processus:exsequi", &[Arg::Str("printf hello".to_owned())])
        .expect("route")
        .expect("exec");
    assert_eq!(reply, Reply::Str("hello".to_owned()));
}

#[test]
fn unset_env_is_unset() {
    let error = serve(
        "processus:lege",
        &[Arg::Str("FABER_LEAF_NO_SUCH_ENV".to_owned())],
    )
    .expect("route")
    .expect_err("unset");
    assert_eq!(error.kind(), crate::LeafErrorKind::Unset);
}

#[test]
fn spawn_returns_a_positive_pid() {
    let reply = serve("processus:genera", &[Arg::Strs(vec!["true".to_owned()])])
        .expect("route")
        .expect("spawn");
    match reply {
        Reply::Int(pid) => assert!(pid > 0),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn empty_detach_argv_is_invalid() {
    let error = serve("processus:dimitte", &[Arg::Strs(Vec::new())])
        .expect("route")
        .expect_err("empty");
    assert_eq!(error.kind(), crate::LeafErrorKind::InvalidArgument);
}
