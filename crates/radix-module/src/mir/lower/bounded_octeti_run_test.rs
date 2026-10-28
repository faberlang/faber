//! Runner coverage for bounded `list<b8, size N>` carriers: the store-time capacity
//! check, `⊥` recovery, and the bridge back to unbounded `list<b8>`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> Result<Vec<String>, RunSourceError> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    run_source(&session, "bounded.fab", &source, &mut host).map(|()| host.stdout_lines)
}

#[test]
fn bounded_octeti_const_within_bound_reports_length_and_emptiness() {
    let lines =
        run("let list<b8> raw ← |61 62|\nlet list<b8, size 8> a ← raw ↦ list<b8, size 8>\nprint a.length()\nprint a.is_empty()")
            .expect("runs");
    assert_eq!(lines, ["2", "false"]);
}

#[test]
fn bounded_octeti_store_over_the_bound_traps_with_the_rust_text() {
    let error = run("let list<b8> raw ← |61 62 63|\nlet list<b8, size 2> a ← raw ↦ list<b8, size 2>\nprint a.length()")
        .expect_err("overflow must fail");
    assert!(
        format!("{error:?}").contains("list<b8> to bounded list<b8> conversion failed"),
        "got: {error:?}"
    );
}

#[test]
fn bounded_octeti_recovery_supplies_the_fallback_on_overflow() {
    let lines = run(
        "let list<b8> raw ← |61 62 63|\nlet list<b8, size 2> a ← raw ↦ list<b8, size 2> ⊥ (|7a| ↦ list<b8, size 2>)\nprint a.length()",
    )
    .expect("runs");
    assert_eq!(lines, ["1"]);
}

#[test]
fn bounded_octeti_widens_back_to_unbounded_bytes() {
    let lines = run(
        "let list<b8> raw ← |61 62|\nlet list<b8, size 2> a ← raw ↦ list<b8, size 2>\nlet list<b8> back ← a ↦ list<b8>\nprint back.length()\nprint a.length()",
    )
    .expect("runs");
    assert_eq!(lines, ["2", "2"]);
}

/// `W ↦ list<b8, size N> via Le|Be` writes the scalar's own bytes; `list<b8> ↦ W via
/// Le|Be` reads them back. The width equals the capacity, so this is a pure
/// bit round trip.
fn scalar_round_trip_body(ty: &str, width: usize, value: &str, hint: &str) -> String {
    format!(
        "let {ty} n ← {value}\nlet list<b8, size {width}> o ← n ↦ list<b8, size {width}> via {hint}\nlet list<b8> raw ← o ↦ list<b8>\nlet {ty} back ← raw ↦ {ty} via {hint}\nprint back"
    )
}

#[test]
fn scalar_to_bounded_octeti_u16_le_round_trips() {
    let lines = run(&scalar_round_trip_body("u16", 2, "258", "Le")).expect("runs");
    assert_eq!(lines, ["258"]);
}

#[test]
fn scalar_to_bounded_octeti_u16_be_round_trips() {
    let lines = run(&scalar_round_trip_body("u16", 2, "258", "Be")).expect("runs");
    assert_eq!(lines, ["258"]);
}

#[test]
fn scalar_to_bounded_octeti_byte_order_follows_the_hint() {
    let lines = run(
        "let u16 n ← 258\nlet list<b8, size 2> be ← n ↦ list<b8, size 2> via Be\nlet u8 be_first ← (be ↦ list<b8>)[0] ↦ u8\nlet list<b8, size 2> le ← n ↦ list<b8, size 2> via Le\nlet u8 le_first ← (le ↦ list<b8>)[0] ↦ u8\nprint be_first\nprint le_first",
    )
    .expect("runs");
    assert_eq!(lines, ["1", "2"]);
}

#[test]
fn scalar_to_bounded_octeti_all_widths_round_trip_le_and_be() {
    for (ty, width, value) in [
        ("i16", 2, "-2"),
        ("u16", 2, "65535"),
        ("i32", 4, "-123456"),
        ("u32", 4, "4000000000"),
        ("i64", 8, "-9000000000000000000"),
        ("u64", 8, "18000000000000000000"),
        ("f32", 4, "1.5"),
        ("f64", 8, "-0.25"),
    ] {
        for hint in ["Le", "Be"] {
            let lines = run(&scalar_round_trip_body(ty, width, value, hint))
                .unwrap_or_else(|error| panic!("{ty} via {hint} must run: {error:?}"));
            assert_eq!(lines, [value], "{ty} via {hint}");
        }
    }
}

/// `list<b8, size N> ↦ W via Le|Be` reads directly from the bounded carrier; no
/// widening to unbounded `list<b8>` first. The capacity equals the scalar width,
/// so this is a pure bit round trip.
fn direct_round_trip_body(ty: &str, width: usize, value: &str, hint: &str) -> String {
    format!(
        "let {ty} n ← {value}\nlet list<b8, size {width}> o ← n ↦ list<b8, size {width}> via {hint}\nlet {ty} back ← o ↦ {ty} via {hint}\nprint back"
    )
}

#[test]
fn bounded_octeti_endian_read_u16_le_round_trips() {
    let lines = run(&direct_round_trip_body("u16", 2, "258", "Le")).expect("runs");
    assert_eq!(lines, ["258"]);
}

#[test]
fn bounded_octeti_endian_read_u16_be_round_trips() {
    let lines = run(&direct_round_trip_body("u16", 2, "258", "Be")).expect("runs");
    assert_eq!(lines, ["258"]);
}

#[test]
fn bounded_octeti_endian_read_all_widths_round_trip_le_and_be() {
    for (ty, width, value) in [
        ("i16", 2, "-2"),
        ("u16", 2, "65535"),
        ("i32", 4, "-123456"),
        ("u32", 4, "4000000000"),
        ("i64", 8, "-9000000000000000000"),
        ("u64", 8, "18000000000000000000"),
        ("f32", 4, "1.5"),
        ("f64", 8, "-0.25"),
    ] {
        for hint in ["Le", "Be"] {
            let lines = run(&direct_round_trip_body(ty, width, value, hint))
                .unwrap_or_else(|error| panic!("{ty} via {hint} must run: {error:?}"));
            assert_eq!(lines, [value], "{ty} via {hint}");
        }
    }
}

#[test]
fn bounded_octeti_endian_read_follows_the_hint() {
    let lines = run(
        "let list<b8> raw ← |01 02|\nlet list<b8, size 2> b ← raw ↦ list<b8, size 2>\nprint b ↦ u16 via Be\nprint b ↦ u16 via Le",
    )
    .expect("runs");
    assert_eq!(lines, ["258", "513"]);
}

/// `octeti<N>[lo‥hi]` is an `octeti` window (semantic access.rs), so the
/// window feeds the same `↦ T via Le|Be` conversion as an unbounded carrier.
fn window_round_trip_body(ty: &str, width: usize, value: &str, hint: &str) -> String {
    format!(
        "let {ty} n ← {value}\nlet list<b8, size {width}> o ← n ↦ list<b8, size {width}> via {hint}\nlet {ty} back ← o[0‥{width}] ↦ {ty} via {hint}\nprint back"
    )
}

#[test]
fn bounded_octeti_window_reads_back_through_le() {
    let lines = run(&window_round_trip_body("u16", 2, "258", "Le")).expect("runs");
    assert_eq!(lines, ["258"]);
}

#[test]
fn bounded_octeti_window_all_widths_round_trip() {
    for (ty, width, value) in [
        ("i16", 2, "-2"),
        ("u16", 2, "65535"),
        ("i32", 4, "-123456"),
        ("u32", 4, "4000000000"),
        ("i64", 8, "-9000000000000000000"),
        ("u64", 8, "18000000000000000000"),
        ("f32", 4, "1.5"),
        ("f64", 8, "-0.25"),
    ] {
        for hint in ["Le", "Be"] {
            let lines = run(&window_round_trip_body(ty, width, value, hint))
                .unwrap_or_else(|error| panic!("{ty} via {hint} must run: {error:?}"));
            assert_eq!(lines, [value], "{ty} via {hint}");
        }
    }
}

#[test]
fn bounded_octeti_window_inner_slice() {
    let lines = run(
        "let list<b8> raw ← |00 01 02 00|\nlet list<b8, size 4> o ← raw ↦ list<b8, size 4>\nprint o[1‥3] ↦ u16 via Be",
    )
    .expect("runs");
    assert_eq!(lines, ["258"]);
}

#[test]
fn bounded_octeti_scalar_index_still_reads_a_byte() {
    let lines = run(
        "let list<b8> raw ← |07 08|\nlet list<b8, size 2> o ← raw ↦ list<b8, size 2>\nlet u8 first ← o[1]\nprint first",
    )
    .expect("runs");
    assert_eq!(lines, ["8"]);
}
