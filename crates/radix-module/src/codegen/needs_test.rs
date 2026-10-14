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
fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack should load")
}

#[cfg(feature = "hir-rust")]
fn analyze_hir(source: &str) -> (HirModule, TypeTable) {
    let lex = crate::lexer::lex_with_locale_pack(source, &la_pack());
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
fn rust_needs_regex_follows_the_regex_type_in_use() {
    let (hir, types) = analyze_hir(
        r#"
incipit {
  fixum textus s ← "a+"
  fixum regex r ← s ↦ regex
  nota r
}
"#,
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(needs.needs_regex, "a regex literal requires the regex feature");
    assert!(needs.needs_faber_runtime);

    let (hir, types) = analyze_hir(
        r"
incipit {
  fixum valor v ← 1
  nota v
}
",
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(!needs.needs_regex, "a program without a regex stays off the feature");
    assert!(needs.needs_faber_runtime);
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
#[cfg(feature = "hir-rust")]
fn rust_needs_faber_runtime_for_a_format_only_program() {
    // The `¶` renderers live in `faber::format`, so a program whose only helper
    // use is `¶` over text depends on the runtime crate.
    let (hir, types) = analyze_hir(
        r#"
incipit {
  fixum textus name ← "faber"
  nota name ¶ ">8"
}
"#,
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(!needs.needs_exact_int_helper);
    assert!(!needs.needs_decimal_helper);
    assert!(!needs.needs_magnus_helper);
    assert!(needs.needs_faber_runtime, "¶ requires the faber runtime crate");
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_needs_faber_runtime_for_a_sync_generator_program() {
    // The lazy sync-generator carrier lives in `faber::Gen` / `faber::Yield`,
    // so a program that declares a `fiunt` function depends on the runtime crate.
    let (hir, types) = analyze_hir(
        r"
functio tres() fiunt → numerus {
  cede 1
  cede 2
}

incipit {
  itera ex tres() fixum n {
    nota n
  }
}
",
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(needs.needs_faber_runtime, "a sync generator requires the faber runtime crate");
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_needs_faber_runtime_for_an_async_generator_program() {
    // The async-generator cursor lives in `faber::AsyncCursor`.
    let (hir, types) = analyze_hir(
        r"
functio tres() fient → numerus {
  cede 1
}

incipit {
  itera ex tres() fixum n {
    nota n
  }
}
",
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(needs.needs_faber_runtime, "an async generator requires the faber runtime crate");
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_needs_faber_runtime_for_an_entry_that_runs_under_block_on() {
    // `block_on` lives in `faber::r#gen`: an entry that awaits calls it.
    let (hir, types) = analyze_hir(
        r#"
@ futura
functio fetchData() → textus {
  redde "data"
}

incipiet {
  figendum _ data ← fetchData()
  nota data
}
"#,
    );
    let needs = collect_rust_needs(&hir, &types, BTreeSet::new(), None);
    assert!(needs.needs_faber_runtime, "an awaiting entry requires the faber runtime crate");
}
