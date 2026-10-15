//! Runner proofs for multi-value `print`: `print a, b` writes its values on
//! ONE line separated by one space, whatever the argument types are.
//!
//! Sized integers, wider floats, policy cells (`wrapping<u8>`,
//! `saturating<u8>`) and raw bits (`b8`) used to fall off the grouped path and
//! print one line per argument.

use crate::script::{BufferHost, interpret_source};

fn run(body: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    interpret_source("multi_print_one_line.fab", body, &mut host)
        .expect("multi-value print fixture runs");
    host.stdout_lines
}

#[test]
fn policy_cells_print_on_one_line() {
    let lines = run("
main {
    const wrapping<u8> w ← 44
    const saturating<u8> c ← 255
    print w, c
}
");
    assert_eq!(lines, ["44 255"]);
}

#[test]
fn sized_integers_print_on_one_line() {
    let lines = run("
main {
    const u8 a ← 7
    const i16 b ← -3
    print a, b
}
");
    assert_eq!(lines, ["7 -3"]);
}

#[test]
fn raw_bits_print_on_one_line_at_their_own_width() {
    let separate = run("
main {
    const b8 p ← 0xf3
    print p ⇐ 4
    print p
}
");
    assert_eq!(separate.len(), 2);
    let joined = run("
main {
    const b8 p ← 0xf3
    print p ⇐ 4, p
}
");
    assert_eq!(joined, [format!("{} {}", separate[0], separate[1])]);
}

#[test]
fn mixed_plain_policy_and_bits_print_on_one_line() {
    let separate = run("
main {
    const wrapping<u8> w ← 44
    const b8 p ← 0xf3
    print 1
    print w
    print p
}
");
    assert_eq!(separate.len(), 3);
    let joined = run("
main {
    const wrapping<u8> w ← 44
    const b8 p ← 0xf3
    print 1, w, p
}
");
    assert_eq!(
        joined,
        [format!("{} {} {}", separate[0], separate[1], separate[2])]
    );
}

#[test]
fn wide_float_and_sized_integer_print_on_one_line() {
    let lines = run("
main {
    const f64 x ← 1.5
    const u8 k ← 2
    print x, k
}
");
    assert_eq!(lines, ["1.5 2"]);
}
