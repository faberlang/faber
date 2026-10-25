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
fn capture_accepts_flattened_argv() {
    let reply = serve(
        "processus:captura",
        &[
            Arg::Str("sh".to_owned()),
            Arg::Str("-c".to_owned()),
            Arg::Str("printf '%s %s' a b".to_owned()),
        ],
    )
    .expect("route")
    .expect("capture");
    let Reply::Map(fields) = reply else {
        panic!("expected map, got {reply:?}");
    };
    assert!(fields.contains(&("status".to_owned(), Reply::Int(0))));
    assert!(fields.contains(&("stdout".to_owned(), Reply::Str("a b".to_owned()))));
}

#[test]
fn spawn_accepts_flattened_argv() {
    let reply = serve(
        "processus:genera",
        &[
            Arg::Str("sh".to_owned()),
            Arg::Str("-c".to_owned()),
            Arg::Str("exit 0".to_owned()),
        ],
    )
    .expect("route")
    .expect("spawn");
    match reply {
        Reply::Int(pid) => assert!(pid > 0),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn exec_returns_stdout() {
    let reply = serve("processus:exsequi", &[Arg::Str("printf hello".to_owned())])
        .expect("route")
        .expect("exec");
    assert_eq!(reply, Reply::Str("hello".to_owned()));
}

#[test]
fn capture_decodes_invalid_and_mixed_utf8_like_go_leaf() {
    const STDOUT_COMMAND: &str = r"printf '\377A\342\202B\303\251'";
    const STDERR_COMMAND: &str = r"printf '\376C\342\202D\303\251' >&2";
    let script = format!("{STDOUT_COMMAND}; {STDERR_COMMAND}");
    let reply = serve(
        "processus:captura",
        &[
            Arg::Str("sh".to_owned()),
            Arg::Str("-c".to_owned()),
            Arg::Str(script),
        ],
    )
    .expect("route")
    .expect("capture");
    let Reply::Map(fields) = reply else {
        panic!("expected map, got {reply:?}");
    };
    assert!(fields.contains(&("stdout".to_owned(), Reply::Str("�A�Bé".to_owned()))));
    assert!(fields.contains(&("stderr".to_owned(), Reply::Str("�C�Dé".to_owned()))));
}

#[test]
fn both_exec_routes_decode_invalid_and_mixed_utf8_like_go_leaf() {
    const COMMAND: &str = r"printf '\377A\342\202B\303\251'";
    for route in ["processus:exsequi", "processus:exsequetur"] {
        let reply = serve(route, &[Arg::Str(COMMAND.to_owned())])
            .expect("route")
            .expect("exec");
        assert_eq!(reply, Reply::Str("�A�Bé".to_owned()));
    }
}

#[test]
fn read_env_unset_is_an_absent_record() {
    let reply = serve(
        "processus:lege",
        &[Arg::Str("FABER_LEAF_NO_SUCH_ENV".to_owned())],
    )
    .expect("route")
    .expect("read_env");
    assert_eq!(
        reply,
        Reply::Map(vec![
            ("present".to_owned(), Reply::Bool(false)),
            ("value".to_owned(), Reply::Str(String::new())),
        ])
    );
}

#[test]
fn read_env_set_is_a_present_record() {
    let name = "FABER_LEAF_TEST_ENV_SET";
    // SAFETY: distinct name per test; parallel tests never touch this var.
    unsafe { std::env::set_var(name, "leaf-value") };
    let reply = serve("processus:lege", &[Arg::Str(name.to_owned())])
        .expect("route")
        .expect("read_env");
    unsafe { std::env::remove_var(name) };
    assert_eq!(
        reply,
        Reply::Map(vec![
            ("present".to_owned(), Reply::Bool(true)),
            ("value".to_owned(), Reply::Str("leaf-value".to_owned())),
        ])
    );
}

#[test]
fn read_env_empty_is_a_present_empty_record() {
    let name = "FABER_LEAF_TEST_ENV_EMPTY";
    // SAFETY: distinct name per test; parallel tests never touch this var.
    unsafe { std::env::set_var(name, "") };
    let reply = serve("processus:lege", &[Arg::Str(name.to_owned())])
        .expect("route")
        .expect("read_env");
    unsafe { std::env::remove_var(name) };
    assert_eq!(
        reply,
        Reply::Map(vec![
            ("present".to_owned(), Reply::Bool(true)),
            ("value".to_owned(), Reply::Str(String::new())),
        ])
    );
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

#[test]
fn exspecta_returns_exit_code() {
    let Reply::Int(pid) = serve(
        "processus:genera",
        &[Arg::Strs(vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "exit 3".to_owned(),
        ])],
    )
    .expect("route")
    .expect("spawn") else {
        panic!("expected pid");
    };
    match serve("processus:exspecta", &[Arg::Int(pid)])
        .expect("route")
        .expect("wait")
    {
        Reply::Int(code) => assert_eq!(code, 3),
        other => panic!("unexpected {other:?}"),
    }
    // Reaped: the pid is no longer known.
    assert!(
        serve("processus:exspecta", &[Arg::Int(pid)])
            .expect("route")
            .is_err()
    );
}

#[test]
fn neca_kills_and_exspecta_returns_promptly() {
    let Reply::Int(pid) = serve(
        "processus:genera",
        &[Arg::Strs(vec!["sleep".to_owned(), "30".to_owned()])],
    )
    .expect("route")
    .expect("spawn") else {
        panic!("expected pid");
    };
    assert_eq!(
        serve("processus:neca", &[Arg::Int(pid)])
            .expect("route")
            .expect("kill"),
        Reply::Unit
    );
    let started = std::time::Instant::now();
    match serve("processus:exspecta", &[Arg::Int(pid)])
        .expect("route")
        .expect("wait")
    {
        Reply::Int(code) => assert_eq!(code, -1, "signal-killed child has no exit code"),
        other => panic!("unexpected {other:?}"),
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
