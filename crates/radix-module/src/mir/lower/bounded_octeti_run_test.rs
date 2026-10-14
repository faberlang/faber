//! Runner coverage for bounded `bytes<N>` carriers: the store-time capacity
//! check, `⊥` recovery, and the bridge back to unbounded `bytes`.

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
        run("const bytes raw ← |61 62|\nconst bytes<8> a ← raw ↦ bytes<8>\nprint a.length()\nprint a.is_empty()")
            .expect("runs");
    assert_eq!(lines, ["2", "false"]);
}

#[test]
fn bounded_octeti_store_over_the_bound_traps_with_the_rust_text() {
    let error = run("const bytes raw ← |61 62 63|\nconst bytes<2> a ← raw ↦ bytes<2>\nprint a.length()")
        .expect_err("overflow must fail");
    assert!(
        format!("{error:?}").contains("octeti to bounded octeti conversion failed"),
        "got: {error:?}"
    );
}

#[test]
fn bounded_octeti_recovery_supplies_the_fallback_on_overflow() {
    let lines =
        run("const bytes raw ← |61 62 63|\nconst bytes<2> a ← raw ↦ bytes<2> ⊥ (|7a| ↦ bytes<2>)\nprint a.length()")
            .expect("runs");
    assert_eq!(lines, ["1"]);
}

#[test]
fn bounded_octeti_widens_back_to_unbounded_bytes() {
    let lines = run(
        "const bytes raw ← |61 62|\nconst bytes<2> a ← raw ↦ bytes<2>\nconst bytes back ← a ↦ bytes\nprint back.length()\nprint a.length()",
    )
    .expect("runs");
    assert_eq!(lines, ["2", "2"]);
}

/// `W ↦ bytes<N> via Le|Be` writes the scalar's own bytes; `bytes ↦ W via
/// Le|Be` reads them back. The width equals the capacity, so this is a pure
/// bit round trip.
fn scalar_round_trip_body(ty: &str, width: usize, value: &str, hint: &str) -> String {
    format!(
        "const {ty} n ← {value}\nconst bytes<{width}> o ← n ↦ bytes<{width}> via {hint}\nconst bytes raw ← o ↦ bytes\nconst {ty} back ← raw ↦ {ty} via {hint}\nprint back"
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
        "const u16 n ← 258\nconst bytes<2> be ← n ↦ bytes<2> via Be\nconst u8 be_first ← (be ↦ bytes)[0] ↦ u8\nconst bytes<2> le ← n ↦ bytes<2> via Le\nconst u8 le_first ← (le ↦ bytes)[0] ↦ u8\nprint be_first\nprint le_first",
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

/// `bytes<N> ↦ W via Le|Be` reads directly from the bounded carrier; no
/// widening to unbounded `bytes` first. The capacity equals the scalar width,
/// so this is a pure bit round trip.
fn direct_round_trip_body(ty: &str, width: usize, value: &str, hint: &str) -> String {
    format!(
        "const {ty} n ← {value}\nconst bytes<{width}> o ← n ↦ bytes<{width}> via {hint}\nconst {ty} back ← o ↦ {ty} via {hint}\nprint back"
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
    let lines =
        run("const bytes raw ← |01 02|\nconst bytes<2> b ← raw ↦ bytes<2>\nprint b ↦ u16 via Be\nprint b ↦ u16 via Le")
            .expect("runs");
    assert_eq!(lines, ["258", "513"]);
}

/// `octeti<N>[lo‥hi]` is an `octeti` window (semantic access.rs), so the
/// window feeds the same `↦ T via Le|Be` conversion as an unbounded carrier.
fn window_round_trip_body(ty: &str, width: usize, value: &str, hint: &str) -> String {
    format!(
        "const {ty} n ← {value}\nconst bytes<{width}> o ← n ↦ bytes<{width}> via {hint}\nconst {ty} back ← o[0‥{width}] ↦ {ty} via {hint}\nprint back"
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
    let lines = run("const bytes raw ← |00 01 02 00|\nconst bytes<4> o ← raw ↦ bytes<4>\nprint o[1‥3] ↦ u16 via Be")
        .expect("runs");
    assert_eq!(lines, ["258"]);
}

#[test]
fn bounded_octeti_scalar_index_still_reads_a_byte() {
    let lines = run("const bytes raw ← |07 08|\nconst bytes<2> o ← raw ↦ bytes<2>\nconst u8 first ← o[1]\nprint first")
        .expect("runs");
    assert_eq!(lines, ["8"]);
}
