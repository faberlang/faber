#![allow(clippy::absurd_extreme_comparisons)]

use hygiene_ratchet::{assert_budgets, Budgets, ScanConfig};
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
    // WHY: minted at D-03 baseline after D03-MIR extraction; frozen, not audited here.
    expect: 59,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    panic: 9,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    unreachable: 1,
    todo: 0,
    unimplemented: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    let_underscore: 33,
    // D03-MIR extracted all four cfg(test)-gated inline modules in the unit.
    inline_test_modules: 0,
    // The five remaining production #[test] files are intentional legacy
    // probe/test surfaces outside this unit: abi/tests.rs and the four
    // packed_* probe/negative sources.
    test_attr_in_production: 5,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = hygiene_ratchet::collect_production_files(&cfg);
    let counts = hygiene_ratchet::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
