//! cap2 (F9 ruling 17, D11.6): `place ↑` and `place ↓` are checked stores on the
//! runner, at every place kind and at both ends of every slot. A successor
//! outside the slot traps with the store identity and the store text; one
//! outside the whole 64-bit exact range is the arithmetic-range trap of
//! ruling 17a, the same text `m - 1` gives.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

const CELL: &str = "genus Cell {\n    varia numerus c\n    varia u8 b\n    varia i8 k\n    varia u64 u\n    varia modulus<u8> w\n    varia saturatus<u8> s\n    varia modulus<i64> mw\n    varia saturatus<i64> ms\n}\n";

fn cell(overrides: &str) -> String {
    let base = [
        ("c", "0"),
        ("b", "0"),
        ("k", "0"),
        ("u", "0"),
        ("w", "0"),
        ("s", "0"),
        ("mw", "0"),
        ("ms", "0"),
    ];
    let fields: Vec<String> = base
        .iter()
        .map(|(name, zero)| {
            let value = overrides
                .split(';')
                .find_map(|pair| {
                    pair.trim()
                        .strip_prefix(&format!("{name}="))
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| (*zero).to_owned());
            format!("{name} = {value}")
        })
        .collect();
    format!("varia Cell p ← Cell {{ {} }}", fields.join(", "))
}

fn run(body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("{CELL}incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "incdec.fab", &source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{body}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

fn ok(body: &str) -> Vec<String> {
    let (out, errors) = run(body);
    assert!(errors.is_empty(), "{body}: {errors:?}");
    out
}

/// The single trap of a body that first prints `a`, so the trap position is
/// proven to come after earlier output and before the `nota` after the step.
fn trap(body: &str) -> (String, String) {
    let (out, errors) = run(&format!("nota \"a\"\n{body}\nnota \"after\""));
    assert_eq!(out, ["a"], "{body}");
    assert_eq!(errors.len(), 1, "{body}: {errors:?}");
    errors.into_iter().next().unwrap()
}

fn store_trap(body: &str, expected: &str) {
    let (issue, message) = trap(body);
    assert_eq!(issue, "implicit_store_out_of_range", "{body}");
    assert_eq!(message, expected, "{body}");
}

fn range_trap(body: &str) {
    let (issue, message) = trap(body);
    assert_eq!(issue, "runner_numerus_overflow", "{body}");
    assert_eq!(message, "numerus overflow", "{body}");
}

#[test]
fn increment_past_i64_max_traps_at_the_store_on_every_place_kind() {
    store_trap(
        "varia i64 u ← 9223372036854775807\nu ↑",
        "9223372036854775808 does not fit in `i64` (assignment to `u`)",
    );
    store_trap(
        "varia numerus u ← 9223372036854775807\nu ↑",
        "9223372036854775808 does not fit in `i64` (assignment to `u`)",
    );
    store_trap(
        &format!("{}\np.c ↑", cell("c=9223372036854775807")),
        "9223372036854775808 does not fit in `i64` (assignment to `c`)",
    );
    store_trap(
        "varia lista<numerus> xs ← [9223372036854775807]\nxs[0] ↑",
        "9223372036854775808 does not fit in `i64` (assignment)",
    );
}

#[test]
fn a_successor_outside_the_whole_64_bit_range_is_the_range_trap() {
    // The same identity and text as `m - 1` and `u + 1` at the cap.
    range_trap("varia i64 u ← -9223372036854775808\nu ↓");
    range_trap(&format!("{}\np.c ↓", cell("c=-9223372036854775808")));
    range_trap("varia lista<numerus> xs ← [-9223372036854775808]\nxs[0] ↓");
    range_trap("varia u64 u ← 18446744073709551615\nu ↑");
    range_trap(&format!("{}\np.u ↑", cell("u=18446744073709551615")));
    range_trap("varia lista<u64> xs ← [18446744073709551615]\nxs[0] ↑");
    range_trap("varia i64 m ← -9223372036854775808\nsit numerus r ← m - 1");
    range_trap("varia u64 m ← 18446744073709551615\nsit u64 r ← m + 1");
}

#[test]
fn narrow_slots_trap_at_both_ends_on_every_place_kind() {
    store_trap("varia u8 x ← 255\nx ↑", "256 does not fit in `u8` (assignment to `x`)");
    store_trap(
        "varia u8 x ← 0\nx ↓",
        "-1 does not fit in `u8` (assignment to `x`) (a negative value cannot be stored in an unsigned slot)",
    );
    store_trap("varia i8 x ← -128\nx ↓", "-129 does not fit in `i8` (assignment to `x`)");
    store_trap(
        "varia u64 x ← 0\nx ↓",
        "-1 does not fit in `u64` (assignment to `x`) (a negative value cannot be stored in an unsigned slot)",
    );
    store_trap(
        &format!("{}\np.b ↑", cell("b=255")),
        "256 does not fit in `u8` (assignment to `b`)",
    );
    store_trap(
        &format!("{}\np.k ↓", cell("k=-128")),
        "-129 does not fit in `i8` (assignment to `k`)",
    );
    store_trap(
        &format!("{}\np.u ↓", cell("u=0")),
        "-1 does not fit in `u64` (assignment to `u`) (a negative value cannot be stored in an unsigned slot)",
    );
    store_trap("varia lista<u8> xs ← [255]\nxs[0] ↑", "256 does not fit in `u8` (assignment)");
    store_trap(
        "varia lista<u8> xs ← [0]\nxs[0] ↓",
        "-1 does not fit in `u8` (assignment) (a negative value cannot be stored in an unsigned slot)",
    );
}

#[test]
fn word_slots_reduce_or_clamp_at_both_ends_on_every_place_kind() {
    assert_eq!(
        ok("varia modulus<u8> x ← 255\nx ↑\nnota x\nvaria modulus<u8> y ← 0\ny ↓\nnota y"),
        ["0", "255"]
    );
    assert_eq!(
        ok("varia saturatus<u8> x ← 255\nx ↑\nnota x\nvaria saturatus<u8> y ← 0\ny ↓\nnota y"),
        ["255", "0"]
    );
    assert_eq!(
        ok(&format!("{}\np.w ↑\np.s ↑\nnota p.w\nnota p.s", cell("w=255; s=255"))),
        ["0", "255"]
    );
    assert_eq!(
        ok(&format!("{}\np.w ↓\np.s ↓\nnota p.w\nnota p.s", cell("w=0; s=0"))),
        ["255", "0"]
    );
    assert_eq!(
        ok(
            "varia lista<modulus<u8>> xs ← [255]\nxs[0] ↑\nnota xs[0]\nvaria lista<saturatus<u8>> ys ← [0]\nys[0] ↓\nnota ys[0]"
        ),
        ["0", "0"]
    );
    // The 64-bit word slots: the exact successor is reduced or clamped.
    assert_eq!(
        ok(&format!(
            "{}\np.mw ↑\np.ms ↑\nnota p.mw\nnota p.ms",
            cell("mw=9223372036854775807; ms=9223372036854775807")
        )),
        ["-9223372036854775808", "9223372036854775807"]
    );
    assert_eq!(
        ok(&format!("{}\np.mw ↓\nnota p.mw", cell("mw=-9223372036854775808"))),
        ["9223372036854775807"]
    );
}

#[test]
fn in_range_steps_still_store_at_the_edges() {
    assert_eq!(
        ok("varia i64 a ← 9223372036854775806\na ↑\nnota a\nvaria i64 b ← -9223372036854775807\nb ↓\nnota b"),
        ["9223372036854775807", "-9223372036854775808"]
    );
    assert_eq!(
        ok("varia u64 a ← 18446744073709551614\na ↑\nnota a\nvaria u64 b ← 9223372036854775808\nb ↓\nnota b"),
        ["18446744073709551615", "9223372036854775807"]
    );
    assert_eq!(
        ok("varia i64 a ← 9223372036854775807\na ↓\nnota a\nvaria i64 b ← -9223372036854775808\nb ↑\nnota b"),
        ["9223372036854775806", "-9223372036854775807"]
    );
    assert_eq!(ok("varia numerus i ← 41\ni ↑\ni ↑\ni ↓\nnota i"), ["42"]);
    assert_eq!(ok("varia u8 x ← 254\nx ↑\nnota x\nvaria u8 y ← 1\ny ↓\nnota y"), ["255", "0"]);
    assert_eq!(
        ok(&format!(
            "{}\np.c ↑\np.k ↓\np.b ↑\nnota p.c\nnota p.k\nnota p.b",
            cell("c=1; k=-127; b=254")
        )),
        ["2", "-128", "255"]
    );
    assert_eq!(
        ok("varia lista<numerus> xs ← [1, 2]\nxs[0] ↑\nxs[1] ↓\nnota xs[0]\nnota xs[1]"),
        ["2", "1"]
    );
}
