#![allow(clippy::absurd_extreme_comparisons)]

use radix_hygiene::{Budgets, ScanConfig, assert_budgets};
use std::path::Path;

fn config() -> ScanConfig {
    ScanConfig {
        source_roots: vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
        exclude_path_suffixes: Vec::new(),
        subtract_self_expect: false,
        check_companion_convention: false,
    }
}

const BUDGETS: Budgets = Budgets {
    // WHY: minted at the housekeeping Step 3 baseline; ratchet down only.
    unwrap: 0,
    // WHY: minted at the housekeeping Step 3 baseline; ratchet down only.
    expect: 0,
    // WHY: minted at the housekeeping Step 3 baseline; ratchet down only.
    panic: 0,
    // WHY: minted at the housekeeping Step 3 baseline; ratchet down only.
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    // WHY: minted at the housekeeping Step 3 baseline; ratchet down only.
    let_underscore: 0,
    inline_test_modules: 0,
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = radix_hygiene::collect_production_files(&cfg);
    let counts = radix_hygiene::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
