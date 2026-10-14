#![allow(clippy::absurd_extreme_comparisons)]

use radix_hygiene::{
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
    expect: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    panic: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    let_underscore: 0,
    // WHY: D03-CODEGENSHARED extracted all four inline test modules (names,
    // conversio_arm, hygiene, and text_literal).
    inline_test_modules: 0,
    // WHY: D03-CODEGENSHARED extracted test attributes from the four assigned
    // production files.
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = radix_hygiene::collect_production_files(&cfg);
    let counts = radix_hygiene::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}

#[test]
fn companion_tests_use_cfg_path_module_convention() {
    let cfg = config();
    let files = radix_hygiene::collect_production_files(&cfg);
    assert_companion_tests_use_cfg_path_module_convention(&files);
}
