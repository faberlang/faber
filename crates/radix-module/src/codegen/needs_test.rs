//! Structural emission needs (`collect_*_needs`) are derived from type tables
//! and HIR content, not body-text patterns. Tests verify import sets, helper
//! flags, and the absence of body-scanning heuristics for each target backend.

#[cfg(feature = "hir-rust")]
use super::*;
#[cfg(feature = "hir-rust")]
use crate::hir::HirModule;
#[cfg(feature = "hir-rust")]
use crate::semantic::{PassConfig, TypeTable, analyze};
#[cfg(feature = "hir-rust")]
use std::collections::BTreeSet;

#[cfg(feature = "hir-rust")]
fn analyze_hir(source: &str) -> (HirModule, TypeTable) {
    let lex = crate::lexer::lex(source);
    let mut parsed = crate::parser::parse(lex);
    assert!(parsed.success(), "{:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    let result = analyze(&program, &PassConfig::base(), &mut parsed.interner);
    assert!(result.success(), "{:?}", result.diagnostics);
    (result.hir.expect("hir"), result.types)
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_needs_records_hashmap_from_types_not_body_text() {
    let (hir, types) = analyze_hir(
        r"
incipit {
  fixum tabula<textus, numerus> m ← vacua
  nota m
}
",
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(
        needs.imports.contains("std::collections::HashMap"),
        "expected HashMap from type table: {:?}",
        needs.imports
    );
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_needs_faber_runtime_from_valor_type() {
    let (hir, types) = analyze_hir(
        r"
incipit {
  fixum valor v ← 1
  nota v
}
",
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(needs.needs_faber_runtime, "valor type requires faber runtime");
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_needs_faber_runtime_for_an_integer_only_program() {
    // The exact-integer helpers live in `faber::exact`, so integer arithmetic
    // alone makes a program depend on the runtime crate.
    let (hir, types) = analyze_hir(
        r"
incipit {
  fixum numerus a ← 7
  fixum numerus b ← a + 1
  nota b
}
",
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(needs.needs_exact_int_helper);
    assert!(needs.needs_faber_runtime, "integer arithmetic requires the faber runtime crate");
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_needs_faber_runtime_for_a_decimal_program() {
    // The decimal helpers live in `faber::dec`, so a decimal width alone makes
    // a program depend on the runtime crate.
    let (hir, types) = analyze_hir(
        r"
incipit {
  fixum d64 a ← 1.5
  fixum d64 b ← a + 1
  nota b
}
",
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(needs.needs_decimal_helper);
    assert!(needs.needs_faber_runtime, "decimal arithmetic requires the faber runtime crate");
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_needs_faber_runtime_for_an_inf_program() {
    // The unbounded-integer helpers live in `faber::inf`, so an `inf` alone
    // makes a program depend on the runtime crate.
    let (hir, types) = analyze_hir(
        r"
incipit {
  fixum inf a ← 18446744073709551616
  nota a / 7
}
",
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(needs.needs_magnus_helper);
    assert!(needs.needs_faber_runtime, "inf arithmetic requires the faber runtime crate");
}

#[test]
fn rust_prelude_policy_no_longer_scans_body_for_hashmap() {
    let prelude = include_str!("../../../radix-hir-rust/src/prelude.rs");
    assert!(!prelude.contains("code.contains(\"HashMap\""));
    assert!(!prelude.contains("needs_faber_runtime_dep"));
}
