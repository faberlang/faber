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
    expect: 3,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    panic: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    let_underscore: 0,
    // WHY: D03-FMIR extracted 5 inline modules (codec, schema/mod,
    // schema/validate, schema/admit kv_cache_admission_tests,
    // schema/executor_plan_admission).
    inline_test_modules: 0,
    // WHY: D03-FMIR extracted test attributes from the five assigned
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
