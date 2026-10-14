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
    unwrap: 0,
    // WHY: minted at D-03 baseline (expect 1); lowered 1→0 after the five
    // u32-ordinal and ergo-gate expects were routed through
    // `FaberCodegen::slot_ordinal` / `ergo_return_expr` real handling.
    expect: 0,
    panic: 0,
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    let_underscore: 0,
    // WHY: D03-HIRFABER extracted all three inline test modules (figura,
    // module, types).
    inline_test_modules: 0,
    // WHY: D03-HIRFABER extracted test attributes from the three assigned
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
