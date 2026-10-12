use super::{Arg, Reply, is_route, serve};

#[test]
fn route_list_matches_serve() {
    for route in super::ROUTES {
        assert!(is_route(route), "{route} not recognized");
        assert!(serve(route, &[]).is_some(), "{route} not served");
    }
    assert!(!is_route("consolum:nope"));
    assert!(serve("consolum:nope", &[]).is_none());
}

#[test]
fn unknown_route_is_not_this_leaf() {
    assert!(!is_route("solum:lege"));
    assert!(serve("solum:lege", &[]).is_none());
}

#[test]
fn tty_probes_return_bool() {
    match serve("consolum:loquitur", &[]).expect("route") {
        Ok(Reply::Bool(_)) => {}
        other => panic!("unexpected {other:?}"),
    }
    match serve("consolum:audit", &[]).expect("route") {
        Ok(Reply::Bool(_)) => {}
        other => panic!("unexpected {other:?}"),
    }
    match serve("consolum:admonet", &[]).expect("route") {
        Ok(Reply::Bool(_)) => {}
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn write_partial_rejects_missing_text() {
    let error = serve("consolum:dic", &[])
        .expect("route")
        .expect_err("arity");
    assert!(error.to_string().contains("missing"));
}

#[test]
fn read_bytes_rejects_negative_length() {
    let error = serve("consolum:hauri", &[Arg::Int(-1)])
        .expect("route")
        .expect_err("negative");
    assert_eq!(error.kind(), crate::LeafErrorKind::InvalidArgument);
}
