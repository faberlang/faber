use crate::{Arg, LeafError, LeafErrorKind, serve};
use std::io;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn missing_path(label: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path: PathBuf = std::env::temp_dir()
        .join(format!("faber-leaf-error-{label}-{n}"))
        .join("absent");
    path.to_string_lossy().into_owned()
}

fn failure(route: &str, args: &[Arg]) -> LeafError {
    match serve(route, args).expect("leaf route") {
        Ok(reply) => panic!("{route} unexpectedly succeeded: {reply:?}"),
        Err(error) => error,
    }
}

fn kind_of(kind: io::ErrorKind) -> LeafErrorKind {
    LeafError::from_io("solum:lege", "p", &io::Error::new(kind, "x")).kind()
}

#[test]
fn from_io_keeps_the_kinds_that_have_a_home() {
    let table = [
        (io::ErrorKind::NotFound, LeafErrorKind::NotFound),
        (io::ErrorKind::PermissionDenied, LeafErrorKind::Permission),
        (io::ErrorKind::BrokenPipe, LeafErrorKind::Closed),
        (io::ErrorKind::AlreadyExists, LeafErrorKind::AlreadyExists),
        (io::ErrorKind::InvalidInput, LeafErrorKind::InvalidArgument),
        (io::ErrorKind::TimedOut, LeafErrorKind::TimedOut),
        (
            io::ErrorKind::ConnectionRefused,
            LeafErrorKind::ConnectionRefused,
        ),
        (
            io::ErrorKind::ConnectionReset,
            LeafErrorKind::ConnectionReset,
        ),
        (io::ErrorKind::AddrInUse, LeafErrorKind::AddrInUse),
        (io::ErrorKind::IsADirectory, LeafErrorKind::IsADirectory),
        (io::ErrorKind::NotADirectory, LeafErrorKind::NotADirectory),
    ];
    for (io_kind, expected) in table {
        assert_eq!(kind_of(io_kind), expected, "{io_kind:?}");
    }
}

#[test]
fn from_io_leaves_kinds_without_a_home_as_io() {
    for io_kind in [
        io::ErrorKind::Other,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::Interrupted,
        io::ErrorKind::UnexpectedEof,
    ] {
        assert_eq!(kind_of(io_kind), LeafErrorKind::Io, "{io_kind:?}");
    }
}

#[test]
fn from_io_kinds_are_pairwise_distinct() {
    let kinds = [
        kind_of(io::ErrorKind::NotFound),
        kind_of(io::ErrorKind::PermissionDenied),
        kind_of(io::ErrorKind::BrokenPipe),
        kind_of(io::ErrorKind::AlreadyExists),
        kind_of(io::ErrorKind::InvalidInput),
        kind_of(io::ErrorKind::TimedOut),
        kind_of(io::ErrorKind::ConnectionRefused),
        kind_of(io::ErrorKind::ConnectionReset),
        kind_of(io::ErrorKind::AddrInUse),
    ];
    for (index, left) in kinds.iter().enumerate() {
        for right in &kinds[index + 1..] {
            assert_ne!(left, right);
        }
    }
}

#[test]
fn display_names_route_and_path() {
    let err = io::Error::new(io::ErrorKind::NotFound, "gone");
    let with_path = LeafError::from_io("solum:lege", "/no/such", &err);
    assert_eq!(with_path.to_string(), "solum:lege /no/such: gone");
    assert_eq!(with_path.route(), "solum:lege");
    assert_eq!(with_path.path(), "/no/such");

    let without_path = LeafError::from_io("caelum:lege", "", &err);
    assert_eq!(without_path.to_string(), "caelum:lege: gone");
}

#[test]
fn fs_routes_keep_the_path_they_were_given() {
    let path = missing_path("fs");
    let other = missing_path("fs-dest");
    let text = |value: &str| Arg::Str(value.to_owned());
    let cases: Vec<(&str, Vec<Arg>)> = vec![
        ("solum:lege", vec![text(&path)]),
        ("solum:hauri", vec![text(&path)]),
        ("solum:hauri", vec![text(&path), Arg::Int(10)]),
        ("solum:partem", vec![text(&path), Arg::Int(0), Arg::Int(4)]),
        ("solum:carpe", vec![text(&path)]),
        ("solum:scribe", vec![text(&path), text("x")]),
        ("solum:funde", vec![text(&path), Arg::Bytes(vec![1])]),
        ("solum:appone", vec![text(&path), text("x")]),
        ("solum:mensura", vec![text(&path)]),
        ("solum:modus", vec![text(&path)]),
        ("solum:status", vec![text(&path)]),
        ("solum:sequere", vec![text(&path)]),
        ("solum:dele", vec![text(&path)]),
        ("solum:enumera", vec![text(&path)]),
        ("solum:amputa", vec![text(&path)]),
        ("solum:modum", vec![text(&path), Arg::Int(0o644)]),
        ("solum:tange", vec![text(&path)]),
        ("solum:copy", vec![text(&path), text(&other)]),
        ("solum:renomina", vec![text(&path), text(&other)]),
    ];
    for (route, args) in cases {
        let error = failure(route, &args);
        assert_eq!(error.path(), path, "{route}");
        assert_eq!(error.kind(), LeafErrorKind::NotFound, "{route}");
        assert!(error.to_string().contains(&path), "{route}: {error}");
    }
}

#[test]
fn spawn_keeps_the_program_it_was_given() {
    let program = missing_path("spawn");
    let error = failure("processus:genera", &[Arg::Str(program.clone())]);
    assert_eq!(error.kind(), LeafErrorKind::NotFound);
    assert_eq!(error.path(), program);
}

#[test]
fn listen_on_a_taken_port_names_the_address() {
    let taken = TcpListener::bind(("0.0.0.0", 0)).expect("bind probe");
    let port = taken.local_addr().expect("probe addr").port();
    let error = failure("caelum:ausculta", &[Arg::Int(i64::from(port))]);
    assert_eq!(error.kind(), LeafErrorKind::AddrInUse);
    assert_eq!(error.path(), format!("0.0.0.0:{port}"));
}

#[test]
fn connect_to_a_closed_port_names_the_address() {
    let port = {
        let probe = TcpListener::bind(("127.0.0.1", 0)).expect("bind probe");
        probe.local_addr().expect("probe addr").port()
    };
    let error = failure(
        "caelum:necte",
        &[
            Arg::Str("127.0.0.1".to_owned()),
            Arg::Int(i64::from(port)),
            Arg::Bool(false),
        ],
    );
    assert_eq!(error.kind(), LeafErrorKind::ConnectionRefused);
    assert_eq!(error.path(), format!("127.0.0.1:{port}"));
}

#[test]
fn leaf_kind_words_are_distinct_and_pascal_case() {
    let words = LeafErrorKind::ALL.map(LeafErrorKind::word);
    for word in words {
        let first = word.chars().next().expect("kind word is never empty");
        assert!(first.is_ascii_uppercase(), "{word}");
        assert!(!word.contains('_'), "{word}");
        assert!(!word.contains(' '), "{word}");
    }
    for (index, left) in words.iter().enumerate() {
        for right in &words[index + 1..] {
            assert_ne!(left, right);
        }
    }
}

#[test]
fn leaf_facts_carry_kind_word_and_path_only() {
    let error = LeafError::from_io(
        "solum:lege",
        "/nonexistent/x",
        &io::Error::from(io::ErrorKind::NotFound),
    );
    assert_eq!(error.facts(), ("NotFound", "/nonexistent/x"));
    let (word, path) = error.facts();
    assert!(!word.contains("os error"), "{word}");
    assert!(!path.contains("os error"), "{path}");
    assert!(!word.contains("solum"), "{word}");
    assert!(!path.contains("solum"), "{path}");
}
