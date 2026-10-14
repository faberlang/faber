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
    expect: 0,
    // WHY: `BufferHost::abort`/`exit` return `!` by the `Host` contract; the capture
    // host unwinds so tests can observe them.
    panic: 2,
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    // WHY: `StdioHost::abort` drops a failed stderr write on its way to `exit(1)`.
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
    let files = radix_hygiene::collect_production_files(&cfg);
    let counts = radix_hygiene::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
