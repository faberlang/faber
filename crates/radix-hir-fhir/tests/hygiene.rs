#![allow(clippy::absurd_extreme_comparisons)]

use hygiene_ratchet::{
    Budgets, ScanConfig, assert_budgets, assert_companion_tests_use_cfg_path_module_convention,
};
use std::path::Path;

fn config() -> ScanConfig {
    ScanConfig {
        source_roots: vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
        exclude_path_suffixes: Vec::new(),
        subtract_self_expect: false,
        check_companion_convention: true,
    }
}

const BUDGETS: Budgets = Budgets {
    // WHY: minted at D-03 baseline; frozen, not audited here.
    unwrap: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    // Re-baselined 0→10: the MessagePack package codec (FPF-3 54654badb /
    // FPF-5 b9d708660) encodes fixint/str/array/map headers through
    // `try_from(...).expect("checked <= bound")` — each site converts a length
    // whose bound is established by the enclosing match arm, so the expect is
    // the fail-closed bound check, not a default.
    expect: 10,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    panic: 0,
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    let_underscore: 0,
    // WHY: D03-HIRFHIR extracted all four inline test modules (artifact,
    // decode, package, and validate).
    inline_test_modules: 0,
    // WHY: D03-HIRFHIR extracted the test attributes from all four assigned
    // production files.
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = hygiene_ratchet::collect_production_files(&cfg);
    let counts = hygiene_ratchet::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}

#[test]
fn companion_tests_use_cfg_path_module_convention() {
    let cfg = config();
    let files = hygiene_ratchet::collect_production_files(&cfg);
    assert_companion_tests_use_cfg_path_module_convention(&files);
}
