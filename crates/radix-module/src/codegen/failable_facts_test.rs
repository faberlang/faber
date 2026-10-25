//! Focused tests for the dispatch-boundary failable-facts conversion shared by
//! the Go/TS/Swift backend adapters.
//!
//! The conversion is gated to the non-rust backends, so this module only
//! compiles under the same feature gate (see the module declaration in
//! `codegen/mod.rs`).

use super::failable_facts;
use crate::hir::DefId;
use crate::semantic::{FunctionFact, FunctionFactTable, TypeId};
use handroll::FxHashMap;

/// A failable def with a declared error type contributes to both parts; a
/// failable def without one and a plain non-failable def stay out of the parts
/// they do not participate in.
#[test]
fn conversion_preserves_failable_set_and_err_tys() {
    let err_ty = TypeId(7);
    let mut by_def = FxHashMap::default();
    by_def.insert(
        DefId(1),
        FunctionFact { may_fail: true, err_ty: Some(err_ty), ..FunctionFact::default() },
    );
    by_def.insert(
        DefId(2),
        FunctionFact { may_fail: true, err_ty: None, ..FunctionFact::default() },
    );
    by_def.insert(DefId(3), FunctionFact::default());
    let facts = FunctionFactTable::from_raw(by_def, None);

    let converted = failable_facts(&facts);

    assert_eq!(converted.failable.len(), 2, "may_fail defs only");
    assert!(converted.failable.contains(&DefId(1)));
    assert!(converted.failable.contains(&DefId(2)));

    assert_eq!(converted.err_tys.len(), 1, "only defs with a declared error type");
    assert_eq!(converted.err_tys.get(&DefId(1)), Some(&err_ty));
    assert!(!converted.err_tys.contains_key(&DefId(2)));
    assert!(!converted.err_tys.contains_key(&DefId(3)));
}

/// Synthetic entry facts never map to a `DefId`, so they must not leak into the
/// def-keyed set/map the backends consume.
#[test]
fn synthetic_entry_fact_stays_out_of_parts() {
    let facts = FunctionFactTable::from_raw(
        FxHashMap::default(),
        Some(FunctionFact { may_fail: true, err_ty: Some(TypeId(9)), ..FunctionFact::default() }),
    );

    let converted = failable_facts(&facts);

    assert!(converted.failable.is_empty(), "entry fact is not a def-keyed failable row");
    assert!(converted.err_tys.is_empty(), "entry fact is not a def-keyed err_ty row");
}
