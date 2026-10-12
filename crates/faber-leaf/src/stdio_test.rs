use super::{Arg, Reply, is_route, serve};

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
fn write_bytes_accepts_flattened_byte_integers() {
    let reply = serve(
        "consolum:funde",
        &[Arg::Int(111), Arg::Int(107), Arg::Int(10)],
    )
    .expect("route")
    .expect("write");
    assert_eq!(reply, Reply::Unit);
}

#[test]
fn write_bytes_accepts_a_single_bytes_argument() {
    let reply = serve("consolum:funde", &[Arg::Bytes(b"ok\n".to_vec())])
        .expect("route")
        .expect("write");
    assert_eq!(reply, Reply::Unit);
}

#[test]
fn write_bytes_rejects_non_byte_argument() {
    let error = serve("consolum:funde", &[Arg::Str("nope".to_owned())])
        .expect("route")
        .expect_err("type");
    assert_eq!(error.kind(), crate::LeafErrorKind::InvalidArgument);
}

#[test]
fn read_bytes_rejects_negative_length() {
    let error = serve("consolum:hauri", &[Arg::Int(-1)])
        .expect("route")
        .expect_err("negative");
    assert_eq!(error.kind(), crate::LeafErrorKind::InvalidArgument);
}
