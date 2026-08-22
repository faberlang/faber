//! AEU-4 (Route B): runtime-len `lista<T, N>` `itera ex` lowers in the base
//! MIR lane to a static split on the carrier `len` — a switch with N+1
//! literal arms, arm k a guard-free k-lane unroll. The AIR lane never admits
//! the loop (runtime-len declines `not_concrete` at `air_unroll`), so the
//! branch stays base-lane and reverse AD never sees a non-literal scrutinee.

use crate::driver::{analyze_source, AnalyzeRequest, Config, Session};
use crate::mir::{dump_program, lower_analyzed_unit, validate_program};

fn session() -> Session {
    Session::new(Config::default())
}

#[test]
fn bounded_lista_ex_lowers_to_len_static_split() {
    let source = r"functio sumBounded(lista<numerus, 8> xs) → numerus {
  varia numerus total ← 0
  itera ex xs fixum x {
    total ← total + x
  }
  redde total
}";
    let mut unit =
        analyze_source(&session(), "bounded-itera.fab", source, AnalyzeRequest::default())
            .expect("bounded lista itera source analyzes");
    let lowered = lower_analyzed_unit(&mut unit, Default::default())
        .expect("runtime-len bounded lista itera ex lowers in the base MIR lane");
    validate_program(&lowered.program, lowered.validated.validation())
        .expect("the len static split MIR must validate");
    let dump = dump_program(&lowered.program);
    assert!(
        !dump.contains("itera collection before iterator MIR lowering"),
        "runtime-len lista<T, 8> must lower to the Route B split: {dump}"
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
            switch.contains(&format!("{}: ", k)),
            "arm {k} (0..=8, N+1 literal cases) must appear in: {switch}"
        );
    }
    assert!(
        switch.contains("default "),
        "the split carries a default arm (len is always <= N; it is the join): {switch}"
    );
}

#[test]
fn bounded_lista_ex_arm_k_unrolls_only_lanes_below_k() {
    // lista<numerus, 3>: arm 2 must read exactly lanes 0 and 1 by constant
    // index — no dead-slot (i >= len) read anywhere in the split.
    let source = r"functio visit(lista<numerus, 3> xs) → vacuum {
  itera ex xs fixum x {
    nota x
  }
  redde
}";
    let mut unit =
        analyze_source(&session(), "bounded-itera.fab", source, AnalyzeRequest::default())
            .expect("bounded lista itera source analyzes");
    let lowered = lower_analyzed_unit(&mut unit, Default::default()).expect("the split lowers");
    validate_program(&lowered.program, lowered.validated.validation())
        .expect("the len static split MIR must validate");
    let dump = dump_program(&lowered.program);
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
    assert!(
        !dump.contains("[const int 3]"),
        "no arm may read lane >= capacity: {dump}"
    );
}

#[test]
fn bounded_lista_ex_capacity_above_ten_stays_unsupported() {
    let mut unit = analyze_source(
        &session(),
        "bounded-itera.fab",
        r"functio big(lista<numerus, 11> xs) → vacuum {
  itera ex xs fixum x {
    nota x
  }
  redde
}",
        AnalyzeRequest::default(),
    )
    .expect("source analyzes; the decline is MIR's");
    let errors = lower_analyzed_unit(&mut unit, Default::default())
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
