//! AEU-4 (Route B): runtime-len `lista<T, magnitudo N>` `itera ex` lowers in the base
//! MIR lane to a static split on the carrier `len` — a switch with N+1
//! literal arms, arm k a guard-free k-lane unroll. The AIR lane never admits
//! the loop (runtime-len declines `not_concrete` at `air_unroll`), so the
//! branch stays base-lane and reverse AD never sees a non-literal scrutinee.

use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::mir::MirLoweringOptions;
use crate::mir::{dump_program, lower_analyzed_unit, validate_program};

fn session() -> Session {
    Session::new(Config::default().with_latin_locale())
}

#[test]
fn bounded_lista_ex_lowers_to_len_static_split() {
    let source = r"functio sumBounded(lista<numerus, magnitudo 8> xs) → numerus {
  varia numerus total ← 0
  itera ex xs sit x {
    total ← total + x
  }
  redde total
}";
    let unit = analyze_source(&session(), "bounded-itera.fab", source, AnalyzeRequest::default())
        .expect("bounded lista itera source analyzes");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("runtime-len bounded lista itera ex lowers in the base MIR lane");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the len static split MIR must validate");
    let dump = dump_program(lowered.validated.program());
    assert!(
        !dump.contains("itera collection before iterator MIR lowering"),
        "runtime-len lista<T, magnitudo 8> must lower to the Route B split: {dump}"
    );
    assert!(
        dump.contains("runtime collection length("),
        "the split scrutinee is the runtime carrier len: {dump}"
    );
    let switch = dump
        .lines()
        .find(|line| line.trim_start().starts_with("switch "))
        .expect("the split must be a switch on len");
    for k in 0..=8 {
        assert!(
            switch.contains(&format!("{k}: ")),
            "arm {k} (0..=8, N+1 literal cases) must appear in: {switch}"
        );
    }
    assert!(
        switch.contains("default "),
        "the split carries a default arm (len is always <= N; it is the join): {switch}"
    );
}

/// `itera de` binds the index, so a bounded lista lowers to the same
/// runtime-length index loop an unbounded lista does, not a static split.
#[test]
fn bounded_lista_de_lowers_to_index_loop() {
    let source = r"functio firstIndex(lista<numerus, magnitudo 8> xs) → numerus {
  itera de xs sit index {
    redde index
  }
  redde 0
}";
    let unit = analyze_source(&session(), "bounded-itera.fab", source, AnalyzeRequest::default())
        .expect("bounded lista itera de source analyzes");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("bounded lista itera de lowers to an index loop");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the index loop MIR must validate");
    let dump = dump_program(lowered.validated.program());
    assert!(
        !dump.contains("itera collection before iterator MIR lowering"),
        "bounded lista<T, magnitudo 8> itera de must lower to an index loop: {dump}"
    );
    assert!(
        dump.contains("runtime collection length("),
        "the index loop bound is the runtime carrier len: {dump}"
    );
}

/// The N <= 10 code-size cap belongs to the `ex` static split; the `de` index
/// loop has no literal-capacity dependence.
#[test]
fn bounded_lista_de_above_ten_lowers() {
    let source = r"functio firstIndex(lista<numerus, magnitudo 16> xs) → numerus {
  itera de xs sit index {
    redde index
  }
  redde 0
}";
    let unit = analyze_source(&session(), "bounded-itera.fab", source, AnalyzeRequest::default())
        .expect("bounded lista itera de source analyzes");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("capacity above ten still lowers for itera de");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the index loop MIR must validate");
    let dump = dump_program(lowered.validated.program());
    assert!(
        !dump.contains("itera collection before iterator MIR lowering"),
        "bounded lista<T, magnitudo 16> itera de must lower to an index loop: {dump}"
    );
    assert!(
        dump.contains("runtime collection length("),
        "the index loop bound is the runtime carrier len: {dump}"
    );
}

#[test]
fn bounded_lista_ex_arm_k_unrolls_only_lanes_below_k() {
    // lista<numerus, magnitudo 3>: arm 2 must read exactly lanes 0 and 1 by constant
    // index — no dead-slot (i >= len) read anywhere in the split.
    let source = r"functio visit(lista<numerus, magnitudo 3> xs) → vacuum {
  itera ex xs sit x {
    nota x
  }
  redde
}";
    let unit = analyze_source(&session(), "bounded-itera.fab", source, AnalyzeRequest::default())
        .expect("bounded lista itera source analyzes");
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("the split lowers");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the len static split MIR must validate");
    let dump = dump_program(lowered.validated.program());
    let index_constants = ["0", "1", "2"]
        .iter()
        .map(|lane| format!("[const int {lane}]"))
        .collect::<Vec<_>>();
    for constant in &index_constants {
        assert!(
            dump.contains(constant),
            "arm lane gather must use the constant index {constant}: {dump}"
        );
    }
    // Capacity 3: arm k reads only lanes 0..k, so lane 3 (a dead slot when
    // len < 4... here len <= 3) is never projected.
    assert!(!dump.contains("[const int 3]"), "no arm may read lane >= capacity: {dump}");
}

/// Above the N <= 10 split cap the `ex` value loop lowers as the plain
/// runtime-length loop an unbounded lista uses: the bound is the carrier
/// `len`, and there is no static split.
#[test]
fn bounded_lista_ex_capacity_above_ten_lowers_to_length_loop() {
    let source = r"functio big(lista<numerus, magnitudo 11> xs) → vacuum {
  itera ex xs sit x {
    nota x
  }
  redde
}";
    let unit = analyze_source(&session(), "bounded-itera.fab", source, AnalyzeRequest::default())
        .expect("source analyzes; the loop is not a decline");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("capacity above ten lowers as the plain runtime-length loop");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the runtime-length loop MIR must validate");
    let dump = dump_program(lowered.validated.program());
    assert!(
        !dump.contains("itera collection before iterator MIR lowering"),
        "bounded lista<T, magnitudo 11> itera ex must lower to the length loop: {dump}"
    );
    assert!(
        dump.contains("runtime collection length("),
        "the loop bound is the runtime carrier len: {dump}"
    );
    assert!(
        !dump
            .lines()
            .any(|line| line.trim_start().starts_with("switch ")),
        "above the cap there is no static split on len: {dump}"
    );
}

// --- AEU-7: scalar bounded buffer forms (`lista<b8, magnitudo N>` / `ascii<N>` / `textus<N>`)

/// Route B proof: the scalar buffers lower to the same live-prefix static
/// split as `lista<T, magnitudo N>` — a switch on the carrier `len` with N+1 arms, arm
/// k reading only lanes 0..k by constant index. Arm k ⊆ 0..len holds for
/// every arm, so the `textus<N>` non-ASCII trap row can never fire from a
/// dead slot.
#[test]
fn lowers_scalar_bounded_buffers_to_live_prefix_static_splits() {
    let cases: [(&str, &str); 3] = [
        (
            "lista<b8, magnitudo 8>",
            r"functio visit(lista<b8, magnitudo 8> xs) → vacuum {
  itera ex xs sit b {
    nota b
  }
  redde
}",
        ),
        (
            "ascii<magnitudo 8>",
            r"functio visit(ascii<magnitudo 8> xs) → vacuum {
  itera ex xs sit c {
    nota c
  }
  redde
}",
        ),
        (
            "textus<magnitudo 8>",
            r"functio visit(textus<magnitudo 8> xs) → vacuum {
  itera ex xs sit c {
    nota c
  }
  redde
}",
        ),
    ];
    for (name, source) in cases {
        let unit = analyze_source(&session(), "bounded-itera.fab", source, AnalyzeRequest::default())
            .unwrap_or_else(|error| panic!("{name} source analyzes: {error:?}"));
        let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
            .unwrap_or_else(|error| panic!("{name} itera ex lowers in the base MIR lane: {error:?}"));
        validate_program(lowered.validated.program(), lowered.validated.validation())
            .unwrap_or_else(|error| panic!("{name} live-prefix split MIR must validate: {error:?}"));
        let dump = dump_program(lowered.validated.program());
        assert!(
            !dump.contains("itera collection before iterator MIR lowering"),
            "{name} must lower to the Route B split: {dump}"
        );
        assert!(
            dump.contains("runtime collection length("),
            "{name} split scrutinee is the runtime carrier len: {dump}"
        );
        let switch = dump
            .lines()
            .find(|line| line.trim_start().starts_with("switch "))
            .unwrap_or_else(|| panic!("{name} split must be a switch on len: {dump}"));
        for k in 0..=8 {
            assert!(
                switch.contains(&format!("{k}: ")),
                "{name} arm {k} (0..=8, N+1 literal cases) must appear in: {switch}"
            );
        }
        assert!(
            !dump.contains("[const int 8]"),
            "{name} no arm may read lane >= capacity (dead slot): {dump}"
        );
    }
}

/// The N <= 10 code-size cap governs the scalar buffer forms exactly as it
/// governs `lista<T, magnitudo N>`.
#[test]
fn scalar_bounded_buffers_capacity_above_ten_stay_unsupported() {
    let unit = analyze_source(
        &session(),
        "bounded-itera.fab",
        r"functio big(textus<magnitudo 11> xs) → vacuum {
  itera ex xs sit c {
    nota c
  }
  redde
}",
        AnalyzeRequest::default(),
    )
    .expect("source analyzes; the decline is MIR's");
    let errors = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect_err("N > 10 exceeds the Route B code-size cap and must fail closed");
    assert_eq!(errors.len(), 1, "exactly the itera decline fires: {errors:?}");
    assert!(
        errors[0]
            .issue
            .contains("itera_collection_before_iterator_mir_lowering"),
        "the N <= 10 cap keeps the Unsupported diagnostic family: {:?}",
        errors[0].issue
    );
}

/// `itera ab` over a VALUE of `intervallum` type (not a range literal)
/// materializes the interval into a `lista<T>` temp through the runtime
/// `interval_materialize` call and walks that array in order.
#[test]
fn itera_interval_value_materializes_in_order() {
    let source = r"functio intervalTotal() → numerus {
  sit intervallum<numerus> r ← 2‥5
  varia numerus total ← 0
  itera ab r sit i {
    total ← total + i
  }
  redde total
}";
    let unit = analyze_source(&session(), "interval-itera.fab", source, AnalyzeRequest::default())
        .expect("interval value itera source analyzes");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("itera ab over an intervallum value must lower");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the materialized interval itera MIR must validate");
    let dump = dump_program(lowered.validated.program());
    assert!(
        !dump.contains("itera ab source before range MIR lowering"),
        "the interval value must materialize instead of declining: {dump}"
    );
    assert!(
        dump.contains("runtime collection interval_materialize("),
        "the source must materialize through the interval materialize runtime call: {dump}"
    );
    assert!(
        dump.contains("runtime collection length("),
        "the materialized array feeds the ordinary list-iteration loop bound: {dump}"
    );
}
