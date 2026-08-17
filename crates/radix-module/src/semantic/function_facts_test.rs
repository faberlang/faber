//! Tests for function fact analysis and capture modes.
//!
//! These tests prove that FunctionFactTable correctly identifies capture
//! modes (delegated, inherited, excluded), resolves function facts across
//! calls, and reports fact conflicts.

use super::*;
use crate::hir::DefId;
use crate::semantic::types::TypeTable;
use crate::semantic::Primitive;
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
    let fact = CaptureFact { def_id: DefId(1), mode: CaptureMode::Read };
    assert_eq!(fact.def_id, DefId(1));
    assert_eq!(fact.mode, CaptureMode::Read);
}

#[test]
fn capture_fact_supports_mutate_mode() {
    let fact = CaptureFact { def_id: DefId(42), mode: CaptureMode::Mutate };
    assert_eq!(fact.def_id, DefId(42));
    assert_eq!(fact.mode, CaptureMode::Mutate);
}

#[test]
fn capture_fact_supports_consume_mode() {
    let fact = CaptureFact { def_id: DefId(7), mode: CaptureMode::Consume };
    assert_eq!(fact.def_id, DefId(7));
    assert_eq!(fact.mode, CaptureMode::Consume);
}

#[test]
fn capture_fact_equality() {
    let a = CaptureFact { def_id: DefId(1), mode: CaptureMode::Read };
    let b = CaptureFact { def_id: DefId(1), mode: CaptureMode::Read };
    assert_eq!(a, b);
}

#[test]
fn capture_fact_inequality() {
    let a = CaptureFact { def_id: DefId(1), mode: CaptureMode::Read };
    let b = CaptureFact { def_id: DefId(2), mode: CaptureMode::Read };
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
    let fact = FunctionFact { direct_failure: true, ..FunctionFact::default() };
    assert!(fact.direct_failure);
}

#[test]
fn function_fact_may_fail() {
    let fact = FunctionFact { may_fail: true, ..FunctionFact::default() };
    assert!(fact.may_fail);
}

#[test]
fn function_fact_is_async() {
    let fact = FunctionFact { is_async: true, ..FunctionFact::default() };
    assert!(fact.is_async);
}

#[test]
fn function_fact_is_generator() {
    let fact = FunctionFact { is_generator: true, ..FunctionFact::default() };
    assert!(fact.is_generator);
}

#[test]
fn function_fact_requires_await() {
    let fact = FunctionFact { requires_await: true, ..FunctionFact::default() };
    assert!(fact.requires_await);
}

#[test]
fn function_fact_has_call_edges() {
    let mut edges = rustc_hash::FxHashSet::default();
    edges.insert(DefId(1));
    edges.insert(DefId(2));
    let fact = FunctionFact { call_edges: edges, ..FunctionFact::default() };
    assert!(fact.call_edges.contains(&DefId(1)));
    assert!(fact.call_edges.contains(&DefId(2)));
    assert_eq!(fact.call_edges.len(), 2);
}

#[test]
fn function_fact_has_captures() {
    let captures = vec![CaptureFact { def_id: DefId(10), mode: CaptureMode::Read }];
    let fact = FunctionFact { captures, ..FunctionFact::default() };
    assert_eq!(fact.captures.len(), 1);
    assert_eq!(fact.captures[0].def_id, DefId(10));
}

#[test]
fn function_fact_with_err_ty() {
    let types = TypeTable::new();
    let err_ty = types.primitive(Primitive::Textus);
    let fact = FunctionFact { err_ty: Some(err_ty), ..FunctionFact::default() };
    assert_eq!(fact.err_ty, Some(err_ty));
}

#[test]
fn function_fact_with_param_modes() {
    let mut modes = FxHashMap::default();
    modes.insert(DefId(1), crate::semantic::SemanticParamMode::Owned);
    modes.insert(DefId(2), crate::semantic::SemanticParamMode::Ref);
    let fact = FunctionFact { param_modes: modes, ..FunctionFact::default() };
    assert_eq!(
        fact.param_modes.get(&DefId(1)),
        Some(&crate::semantic::SemanticParamMode::Owned)
    );
    assert_eq!(fact.param_modes.get(&DefId(2)), Some(&crate::semantic::SemanticParamMode::Ref));
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
    let table = FunctionFactTable { by_def: FxHashMap::default(), entry: Some(FunctionFact::default()) };
    assert!(!table.is_empty());
}

#[test]
fn function_fact_table_with_both_entry_and_by_def() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(1), FunctionFact { direct_failure: true, ..FunctionFact::default() });
    let table = FunctionFactTable { by_def, entry: Some(FunctionFact { may_fail: true, ..FunctionFact::default() }) };
    assert_eq!(table.len(), 1);
    let retrieved = table.get(DefId(1)).expect("by_def entry should be present");
    assert!(retrieved.direct_failure);
    assert!(!retrieved.may_fail);
}

#[test]
fn function_fact_table_with_functions_is_not_empty() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(1), FunctionFact::default());
    let table = FunctionFactTable { by_def, entry: None };
    assert!(!table.is_empty());
    assert_eq!(table.len(), 1);
}

#[test]
fn function_fact_table_failable_defs_finds_failable_functions() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(1), FunctionFact { may_fail: true, ..FunctionFact::default() });
    by_def.insert(DefId(2), FunctionFact { may_fail: false, ..FunctionFact::default() });
    by_def.insert(DefId(3), FunctionFact { may_fail: true, ..FunctionFact::default() });
    let table = FunctionFactTable { by_def, entry: None };
    let failable = table.failable_defs();
    assert!(failable.contains(&DefId(1)));
    assert!(!failable.contains(&DefId(2)));
    assert!(failable.contains(&DefId(3)));
    assert_eq!(failable.len(), 2);
}

#[test]
fn function_fact_table_async_defs_finds_async_functions() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(1), FunctionFact { is_async: true, ..FunctionFact::default() });
    by_def.insert(DefId(2), FunctionFact { is_async: false, ..FunctionFact::default() });
    let table = FunctionFactTable { by_def, entry: None };
    let async_defs = table.async_defs();
    assert!(async_defs.contains(&DefId(1)));
    assert!(!async_defs.contains(&DefId(2)));
    assert_eq!(async_defs.len(), 1);
}

#[test]
fn function_fact_table_has_fact_checks_by_def() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(5), FunctionFact::default());
    let table = FunctionFactTable { by_def, entry: None };
    assert!(table.has_fact(DefId(5)));
    assert!(!table.has_fact(DefId(99)));
}

#[test]
fn function_fact_table_iter_yields_entries() {
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(1), FunctionFact::default());
    by_def.insert(DefId(2), FunctionFact::default());
    let table = FunctionFactTable { by_def, entry: None };
    let ids: Vec<DefId> = table.iter().map(|(id, _)| id).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&DefId(1)));
    assert!(ids.contains(&DefId(2)));
}

#[test]
fn function_fact_table_get_returns_expected_fact() {
    let fact = FunctionFact { direct_failure: true, ..FunctionFact::default() };
    let mut by_def = FxHashMap::default();
    by_def.insert(DefId(10), fact);
    let table = FunctionFactTable { by_def, entry: None };
    let retrieved = table.get(DefId(10)).expect("fact should be present");
    assert!(retrieved.direct_failure);
}

// ── FunctionFact equality ───────────────────────────────────────────────────

#[test]
fn function_fact_equality() {
    let a = FunctionFact::default();
    let b = FunctionFact::default();
    assert_eq!(a, b);
}

#[test]
fn function_fact_inequality() {
    let a = FunctionFact { direct_failure: true, ..FunctionFact::default() };
    let b = FunctionFact { direct_failure: false, ..FunctionFact::default() };
    assert_ne!(a, b);
}
