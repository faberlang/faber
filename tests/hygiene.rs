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
    unwrap: 0,
    expect: 1,
    panic: 1,
    unreachable: 1,
    todo: 0,
    unimplemented: 0,
    let_underscore: 3,
    inline_test_modules: 0,
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let files = hygiene_ratchet::collect_production_files(&config());
    let counts = hygiene_ratchet::count_budgets(&files, config().subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
