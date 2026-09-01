#![allow(clippy::absurd_extreme_comparisons)]

use hygiene_ratchet::{Budgets, ScanConfig, assert_budgets};
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
    // WHY: minted at D-03 baseline; frozen, not audited here.
    unwrap: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    expect: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    panic: 2,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    unreachable: 2,
    todo: 0,
    unimplemented: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    let_underscore: 1,
    // D03-RUNNER extracted 2 inline modules (kernel/aleator.rs `tests`,
    // kernel/solum.rs `range_tests`).
    inline_test_modules: 0,
    // WHY: D03-RUNNER extracted test attributes from the two assigned
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
