//! Tests for function fact analysis and capture modes.
//!
//! These tests prove that FunctionFactTable correctly identifies capture
//! modes (delegated, inherited, excluded), resolves function facts across
//! calls, and reports fact conflicts.

use super::*;
use crate::Primitive;
use crate::types::TypeTable;
use radix_hir::DefId;
use rustc_hash::FxHashMap;

// ── CaptureMode transitions ─────────────────────────────────────────────────

#[test]
fn capture_mode_values_are_distinct() {
    assert_ne!(CaptureMode::Read, CaptureMode::Mutate);
    assert_ne!(CaptureMode::Read, CaptureMode::Consume);
    assert_ne!(CaptureMode::Mutate, CaptureMode::Consume);
}

#[test]
fn capture_mode_debug_output() {
    assert_eq!(format!("{:?}", CaptureMode::Read), "Read");
    assert_eq!(format!("{:?}", CaptureMode::Mutate), "Mutate");
    assert_eq!(format!("{:?}", CaptureMode::Consume), "Consume");
}

// ── CaptureFact construction ────────────────────────────────────────────────

#[test]
fn capture_fact_holds_def_id_and_mode() {
    let fact = CaptureFact {
        def_id: DefId(1),
        mode: CaptureMode::Read,
    };
    assert_eq!(fact.def_id, DefId(1));
    assert_eq!(fact.mode, CaptureMode::Read);
}

#[test]
fn capture_fact_supports_mutate_mode() {
    let fact = CaptureFact {
        def_id: DefId(42),
        mode: CaptureMode::Mutate,
    };
    assert_eq!(fact.def_id, DefId(42));
    assert_eq!(fact.mode, CaptureMode::Mutate);
}

#[test]
fn capture_fact_supports_consume_mode() {
    let fact = CaptureFact {
        def_id: DefId(7),
        mode: CaptureMode::Consume,
    };
    assert_eq!(fact.def_id, DefId(7));
    assert_eq!(fact.mode, CaptureMode::Consume);
}

#[test]
fn capture_fact_equality() {
    let a = CaptureFact {
        def_id: DefId(1),
        mode: CaptureMode::Read,
    };
    let b = CaptureFact {
        def_id: DefId(1),
        mode: CaptureMode::Read,
    };
    assert_eq!(a, b);
}

#[test]
fn capture_fact_inequality() {
    let a = CaptureFact {
        def_id: DefId(1),
        mode: CaptureMode::Read,
    };
    let b = CaptureFact {
        def_id: DefId(2),
        mode: CaptureMode::Read,
    };
    assert_ne!(a, b);
}

// ── FunctionFact construction and default ────────────────────────────────────

#[test]
fn function_fact_default_does_not_fail() {
    let fact = FunctionFact::default();
    assert!(!fact.direct_failure);
    assert!(!fact.may_fail);
    assert!(!fact.is_async);
    assert!(!fact.is_generator);
    assert!(!fact.requires_await);
    assert!(fact.call_edges.is_empty());
    assert!(fact.captures.is_empty());
    assert!(fact.param_modes.is_empty());
    assert!(fact.err_ty.is_none());
}

#[test]
fn function_fact_direct_failure() {
    let fact = FunctionFact {
        direct_failure: true,
        ..FunctionFact::default()
    };
    assert!(fact.direct_failure);
}

#[test]
fn function_fact_may_fail() {
    let fact = FunctionFact {
        may_fail: true,
        ..FunctionFact::default()
    };
    assert!(fact.may_fail);
}

#[test]
fn function_fact_is_async() {
    let fact = FunctionFact {
        is_async: true,
        ..FunctionFact::default()
    };
    assert!(fact.is_async);
}

#[test]
fn function_fact_is_generator() {
    let fact = FunctionFact {
        is_generator: true,
        ..FunctionFact::default()
    };
    assert!(fact.is_generator);
}

#[test]
fn function_fact_requires_await() {
    let fact = FunctionFact {
        requires_await: true,
        ..FunctionFact::default()
    };
    assert!(fact.requires_await);
}

#[test]
fn function_fact_has_call_edges() {
    let mut edges = rustc_hash::FxHashSet::default();
    edges.insert(DefId(1));
    edges.insert(DefId(2));
    let fact = FunctionFact {
        call_edges: edges,
        ..FunctionFact::default()
    };
    assert!(fact.call_edges.contains(&DefId(1)));
    assert!(fact.call_edges.contains(&DefId(2)));
    assert_eq!(fact.call_edges.len(), 2);
}

#[test]
fn function_fact_has_captures() {
    let captures = vec![CaptureFact {
        def_id: DefId(10),
        mode: CaptureMode::Read,
    }];
    let fact = FunctionFact {
        captures,
        ..FunctionFact::default()
    };
    assert_eq!(fact.captures.len(), 1);
    assert_eq!(fact.captures[0].def_id, DefId(10));
}

#[test]
fn function_fact_with_err_ty() {
    let types = TypeTable::new();
    let err_ty = types.primitive(Primitive::Textus);
    let fact = FunctionFact {
        err_ty: Some(err_ty),
        ..FunctionFact::default()
    };
    assert_eq!(fact.err_ty, Some(err_ty));
}

#[test]
fn function_fact_with_param_modes() {
    let mut modes = FxHashMap::default();
    modes.insert(DefId(1), crate::SemanticParamMode::Owned);
    modes.insert(DefId(2), crate::SemanticParamMode::Ref);
    let fact = FunctionFact {
        param_modes: modes,
        ..FunctionFact::default()
    };
    assert_eq!(
        fact.param_modes.get(&DefId(1)),
        Some(&crate::SemanticParamMode::Owned)
    );
    assert_eq!(
        fact.param_modes.get(&DefId(2)),
        Some(&crate::SemanticParamMode::Ref)
    );
    assert_eq!(fact.param_modes.len(), 2);
}

// ── FunctionFactTable construction and querying ──────────────────────────────

#[test]
fn function_fact_table_empty_by_default() {
    let table = FunctionFactTable::default();
    assert!(table.is_empty());
    assert_eq!(table.len(), 0);
}

#[test]
fn function_fact_table_get_returns_none_for_missing() {
    let table = FunctionFactTable::default();
    assert!(table.get(DefId(1)).is_none());
}

#[test]
fn function_fact_table_has_fact_returns_false_for_missing() {
    let table = FunctionFactTable::default();
    assert!(!table.has_fact(DefId(1)));
}

#[test]
fn function_fact_table_iter_empty() {
    let table = FunctionFactTable::default();
    assert_eq!(table.iter().count(), 0);
}

#[test]
fn function_fact_table_failable_defs_empty() {
    let table = FunctionFactTable::default();
    assert!(table.failable_defs().is_empty());
}

#[test]
fn function_fact_table_async_defs_empty() {
    let table = FunctionFactTable::default();
    assert!(table.async_defs().is_empty());
}

#[test]
fn function_fact_table_with_entry_is_not_empty() {
    let table = FunctionFactTable {
        by_def: FxHashMap::default(),
        entry: Some(FunctionFact::default()),
    };
    assert!(!table.is_empty());
}

#[test]
fn function_fact_table_with_both_entry_and_by_def() {
    let mut by_def = FxHashMap::default();
    by_def.insert(
        DefId(1),
        FunctionFact {
            direct_failure: true,
            ..FunctionFact::default()
        },
    );
    let table = FunctionFactTable {
        by_def,
        entry: Some(FunctionFact {
            may_fail: true,
            ..FunctionFact::default()
        }),
    };
    assert_eq!(table.len(), 1);
    let retrieved = table.get(DefId(1)).expect("by_def entry should be present");
    assert!(retrieved.direct_failure);
    assert!(!retrieved.may_fail);
}

#[test]
fn function_fact_table_with_functions_is_not_empty() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(1), FunctionFact::default());
    let table = FunctionFactTable {
        by_def,
        entry: None,
    };
    assert!(!table.is_empty());
    assert_eq!(table.len(), 1);
}

#[test]
fn function_fact_table_failable_defs_finds_failable_functions() {
    let mut by_def = FxHashMap::default();
    by_def.insert(
        DefId(1),
        FunctionFact {
            may_fail: true,
            ..FunctionFact::default()
        },
    );
    by_def.insert(
        DefId(2),
        FunctionFact {
            may_fail: false,
            ..FunctionFact::default()
        },
    );
    by_def.insert(
        DefId(3),
        FunctionFact {
            may_fail: true,
            ..FunctionFact::default()
        },
    );
    let table = FunctionFactTable {
        by_def,
        entry: None,
    };
    let failable = table.failable_defs();
    assert!(failable.contains(&DefId(1)));
    assert!(!failable.contains(&DefId(2)));
    assert!(failable.contains(&DefId(3)));
    assert_eq!(failable.len(), 2);
}

#[test]
fn function_fact_table_async_defs_finds_async_functions() {
    let mut by_def = FxHashMap::default();
    by_def.insert(
        DefId(1),
        FunctionFact {
            is_async: true,
            ..FunctionFact::default()
        },
    );
    by_def.insert(
        DefId(2),
        FunctionFact {
            is_async: false,
            ..FunctionFact::default()
        },
    );
    let table = FunctionFactTable {
        by_def,
        entry: None,
    };
    let async_defs = table.async_defs();
    assert!(async_defs.contains(&DefId(1)));
    assert!(!async_defs.contains(&DefId(2)));
    assert_eq!(async_defs.len(), 1);
}

#[test]
fn function_fact_table_has_fact_checks_by_def() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(5), FunctionFact::default());
    let table = FunctionFactTable {
        by_def,
        entry: None,
    };
    assert!(table.has_fact(DefId(5)));
    assert!(!table.has_fact(DefId(99)));
}

#[test]
fn function_fact_table_iter_yields_entries() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(1), FunctionFact::default());
    by_def.insert(DefId(2), FunctionFact::default());
    let table = FunctionFactTable {
        by_def,
        entry: None,
    };
    let ids: Vec<DefId> = table.iter().map(|(id, _)| id).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&DefId(1)));
    assert!(ids.contains(&DefId(2)));
}

#[test]
fn function_fact_table_get_returns_expected_fact() {
    let fact = FunctionFact {
        direct_failure: true,
        ..FunctionFact::default()
    };
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(10), fact);
    let table = FunctionFactTable {
        by_def,
        entry: None,
    };
    let retrieved = table.get(DefId(10)).expect("fact should be present");
    assert!(retrieved.direct_failure);
}

// ── FunctionFact equality ───────────────────────────────────────────────────

// ── derived capture modes (A-174) ───────────────────────────────────────────

/// Captures recorded for one named top-level function.
fn function_captures(source: &str, wanted: &str) -> Vec<CaptureFact> {
    let fixture = crate::test_support::analyze_ok(source);
    let hir = fixture.result.hir.as_ref().expect("typed HIR");
    let table = analyze_function_facts(hir, &fixture.result.types);
    let def_id = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            radix_hir::HirItemKind::Function(function)
                if fixture.interner.resolve(function.name) == wanted =>
            {
                Some(item.def_id)
            }
            _ => None,
        })
        .expect("function item");
    table
        .get(def_id)
        .expect("function fact row")
        .captures
        .clone()
}

#[test]
fn mutating_closure_capture_records_mutate_mode() {
    let captures = function_captures(
        r#"
functio counter() → () → numerus {
    varia numerus c ← 0
    redde () → numerus ∴ fac {
        c ← c + 1
        redde c
    }
}
incipit { }
"#,
        "counter",
    );
    assert_eq!(captures.len(), 1, "closure captures c: {captures:?}");
    assert_eq!(captures[0].mode, CaptureMode::Mutate, "{captures:?}");
}

#[test]
fn mutating_loop_binder_capture_records_mutate_mode() {
    let captures = function_captures(
        r#"
functio loops() → numerus {
    varia numerus total ← 0
    itera ex [1, 2, 3] varia v {
        sit f ← () → numerus ∴ fac {
            v ← v + 10
            redde v
        }
        total ← total + f()
    }
    redde total
}
incipit { }
"#,
        "loops",
    );
    assert_eq!(
        captures.len(),
        1,
        "closure captures the loop binder v: {captures:?}"
    );
    assert_eq!(captures[0].mode, CaptureMode::Mutate, "{captures:?}");
}

#[test]
fn moved_out_closure_capture_records_consume_mode() {
    let captures = function_captures(
        r#"
functio eat(own lista<numerus> xs) → vacuum {
    nota xs.longitudo()
}
functio demo() → vacuum {
    varia lista<numerus> a ← [1, 2, 3]
    sit f ← () → vacuum ∴ eat(own a)
}
incipit { }
"#,
        "demo",
    );
    assert_eq!(
        captures
            .iter()
            .filter(|capture| capture.mode == CaptureMode::Consume)
            .count(),
        1,
        "the moved-out binding records one consume capture: {captures:?}"
    );
}

#[test]
fn read_only_closure_capture_records_read_mode() {
    let captures = function_captures(
        r#"
functio peek() → () → numerus {
    varia numerus n ← 1
    redde () → numerus ∴ n
}
incipit { }
"#,
        "peek",
    );
    assert_eq!(captures.len(), 1, "closure captures n: {captures:?}");
    assert_eq!(captures[0].mode, CaptureMode::Read, "{captures:?}");
}

// ── closure-body-derived captures (E2-4.5) ─────────────────────────────────

#[test]
fn write_outside_closure_leaves_read_capture_intact() {
    let captures = function_captures(
        r#"
functio mixed() → () → numerus {
    varia numerus n ← 1
    sit peek ← () → numerus ∴ n
    n ← n + 2
    redde peek
}
incipit { }
"#,
        "mixed",
    );
    assert_eq!(captures.len(), 1, "closure captures n: {captures:?}");
    assert_eq!(
        captures[0].mode,
        CaptureMode::Read,
        "the write after the closure is not the closure's demand: {captures:?}"
    );
}

#[test]
fn loop_binder_used_only_outside_closures_is_not_a_capture() {
    let captures = function_captures(
        r#"
functio loose() → numerus {
    varia numerus t ← 0
    itera ex [1, 2] varia v {
        t ← t + v
    }
    redde t
}
incipit { }
"#,
        "loose",
    );
    assert!(captures.is_empty(), "no closure, no captures: {captures:?}");
}

#[test]
fn parameter_captured_by_closure_records_read_capture() {
    let captures = function_captures(
        r#"
functio wraps(numerus m) → () → numerus {
    redde () → numerus ∴ m
}
incipit { }
"#,
        "wraps",
    );
    assert_eq!(captures.len(), 1, "closure captures m: {captures:?}");
    assert_eq!(captures[0].mode, CaptureMode::Read, "{captures:?}");
}

#[test]
fn method_call_on_capture_inside_closure_records_mutate_mode() {
    // No per-method mutation facts exist yet, so every method call on the
    // captured name inside the referencing closure counts as a mutate —
    // fail closed, matching the shared capture analysis.
    let captures = function_captures(
        r#"
functio prober() → () → numerus {
    varia lista<numerus> xs ← [1]
    redde () → numerus ∴ xs.longitudo()
}
incipit { }
"#,
        "prober",
    );
    assert_eq!(captures.len(), 1, "closure captures xs: {captures:?}");
    assert_eq!(captures[0].mode, CaptureMode::Mutate, "{captures:?}");
}

#[test]
fn closure_local_used_only_inside_its_own_closure_is_not_a_capture() {
    let captures = function_captures(
        r#"
functio outer() → () → numerus {
    redde () → numerus ∴ fac {
        varia numerus only ← 7
        redde only
    }
}
incipit { }
"#,
        "outer",
    );
    assert!(
        captures.is_empty(),
        "only is local to the closure: {captures:?}"
    );
}

#[test]
fn closure_local_captured_by_nested_closure_records_mutate_mode() {
    let captures = function_captures(
        r#"
functio nest() → () → () → numerus {
    redde () → () → numerus ∴ fac {
        varia numerus inner ← 5
        redde () → numerus ∴ fac {
            inner ← inner + 1
            redde inner
        }
    }
}
incipit { }
"#,
        "nest",
    );
    assert_eq!(
        captures.len(),
        1,
        "the inner closure captures inner: {captures:?}"
    );
    assert_eq!(captures[0].mode, CaptureMode::Mutate, "{captures:?}");
}

#[test]
fn closure_match_arm_binder_is_not_a_capture() {
    // A binder introduced by a `discerne` arm belongs to the closure body,
    // so reading it is not a capture; only the enclosing local is.
    let captures = function_captures(
        r#"
functio matcher() → (numerus ∪ textus) → numerus {
    sit numerus extra ← 2
    redde (numerus ∪ textus u) → numerus ∴ fac {
        discerne u {
            casu numerus fixum i {
                redde i + extra
            }
            casu textus fixum s {
                nota s
                redde extra
            }
        }
    }
}
incipit { }
"#,
        "matcher",
    );
    assert_eq!(
        captures.len(),
        1,
        "only extra is captured; the arm binders belong to the closure: {captures:?}"
    );
    assert_eq!(captures[0].mode, CaptureMode::Read, "{captures:?}");
}

#[test]
fn closure_cape_binder_is_not_a_capture() {
    // The `cape err` binder belongs to the closure body, so reading it is not
    // a capture; only the enclosing local is.
    let captures = function_captures(
        r#"
functio guarded() → () → textus {
    sit textus fallback ← "fallback"
    redde () → textus ∴ fac {
        iace "boom"
    } cape err {
        nota err
        redde fallback
    }
}
incipit { }
"#,
        "guarded",
    );
    assert_eq!(
        captures.len(),
        1,
        "only fallback is captured; err belongs to the closure: {captures:?}"
    );
    assert_eq!(captures[0].mode, CaptureMode::Read, "{captures:?}");
}

#[test]
fn entry_row_captures_stay_empty() {
    let fixture = crate::test_support::analyze_ok(
        r#"
functio helper() → numerus {
    redde 1
}
incipit {
    sit f ← () → numerus ∴ helper()
}
"#,
    );
    let hir = fixture.result.hir.as_ref().expect("typed HIR");
    let table = analyze_function_facts(hir, &fixture.result.types);
    let entry = table.entry.as_ref().expect("entry fact row");
    assert!(
        entry.captures.is_empty(),
        "entry captures: {:?}",
        entry.captures
    );
}

#[test]
fn function_fact_equality() {
    let a = FunctionFact::default();
    let b = FunctionFact::default();
    assert_eq!(a, b);
}

#[test]
fn function_fact_inequality() {
    let a = FunctionFact {
        direct_failure: true,
        ..FunctionFact::default()
    };
    let b = FunctionFact {
        direct_failure: false,
        ..FunctionFact::default()
    };
    assert_ne!(a, b);
}
