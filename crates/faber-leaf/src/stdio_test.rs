use super::{Arg, Reply, input_ended, is_route, read_line_from, serve};

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

#[test]
fn input_ended_is_true_only_for_empty_input() {
    use std::io::Cursor;

    let mut empty = Cursor::new(b"".to_vec());
    assert!(input_ended(&mut empty).expect("probe"));

    let mut blank = Cursor::new(b"\n".to_vec());
    assert!(!input_ended(&mut blank).expect("probe"));

    let mut text = Cursor::new(b"x".to_vec());
    assert!(!input_ended(&mut text).expect("probe"));
    assert_eq!(read_line_from(&mut text).expect("read"), "x");
}

#[test]
fn blank_line_is_not_end_of_input() {
    use std::io::Cursor;

    let mut reader = Cursor::new(b"\n".to_vec());
    assert!(!input_ended(&mut reader).expect("probe"));
    assert_eq!(read_line_from(&mut reader).expect("read"), "");
    assert!(input_ended(&mut reader).expect("probe"));
}

#[test]
fn read_line_from_strips_crlf_and_reads_unterminated_last_line() {
    use std::io::Cursor;

    let mut reader = Cursor::new(b"a\r\nb".to_vec());
    assert_eq!(read_line_from(&mut reader).expect("read"), "a");
    assert_eq!(read_line_from(&mut reader).expect("read"), "b");
    assert!(input_ended(&mut reader).expect("probe"));
}

#[test]
fn desinit_rejects_arguments() {
    let error = serve("consolum:desinit", &[Arg::Int(1)])
        .expect("route")
        .expect_err("arity");
    assert_eq!(error.kind(), crate::LeafErrorKind::InvalidArgument);
}

#[test]
fn tty_size_is_zero_zero_when_stdout_is_not_a_terminal() {
    let reply = serve("consolum:metire", &[])
        .expect("route")
        .expect("measure");
    assert_eq!(reply, Reply::List(vec![Reply::Int(0), Reply::Int(0)]));
}

#[test]
fn tty_size_rejects_arguments() {
    let error = serve("consolum:metire", &[Arg::Int(1)])
        .expect("route")
        .expect_err("arity");
    assert_eq!(error.kind(), crate::LeafErrorKind::InvalidArgument);
}
