//! c9775819-4: `read` returns all of stdin to EOF; `read line` one line
//! without its terminator. `read_all` joins the seeded lines back into the
//! verbatim fixture bytes, and reports EOF (not empty text) at the immediate
//! read.

use super::*;

#[test]
fn read_all_returns_joined_stdin_lines() {
    let mut host = BufferHost::default().with_stdin_lines(["one", "two"]);
    assert_eq!(host.read_all().unwrap(), Some("one\ntwo\n".to_owned()));
}

#[test]
fn read_all_second_call_reports_eof() {
    let mut host = BufferHost::default().with_stdin_lines(["one", "two"]);
    assert_eq!(host.read_all().unwrap(), Some("one\ntwo\n".to_owned()));
    assert_eq!(host.read_all().unwrap(), None);
}

#[test]
fn read_all_default_host_reports_eof() {
    let mut host = BufferHost::default();
    assert_eq!(host.read_all().unwrap(), None);
}

#[test]
fn read_all_after_read_line_returns_remainder() {
    let mut host = BufferHost::default().with_stdin_lines(["one", "two"]);
    assert_eq!(host.read_line().unwrap(), Some("one".to_owned()));
    assert_eq!(host.read_all().unwrap(), Some("two\n".to_owned()));
}
