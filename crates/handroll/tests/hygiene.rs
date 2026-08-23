use hygiene_ratchet::{
    assert_budgets, assert_companion_tests_use_cfg_path_module_convention, Budgets, ScanConfig,
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
    expect: 4,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    panic: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    let_underscore: 1,
    // WHY: D03-HANDROLL extracts both inline test modules (entropy, sha256);
    // minted at observed, lowered to 0 by this unit.
    inline_test_modules: 0,
    // WHY: D03-HANDROLL extracts test attributes from both production files
    // (entropy, sha256). Residual 1 = lib.rs `manifest_is_std_only` guard
    // test (U5a), outside this unit's file row; frozen at observed.
    test_attr_in_production: 1,
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
