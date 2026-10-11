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
    // WHY: minted from the moved production files; ratchet down only.
    unwrap: 0,
    expect: 0,
    panic: 0,
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    // WHY: the four `substitute_tensor_from_flat_shape_size_constants` /
    // `substitute_shape_size_constants` calls in `lower/monomorphize.rs` and
    // `lower/package_instantiate.rs` return `#[must_use]` bools whose `false`
    // (a term that cannot fold yet) is deliberately left to the post-finalize
    // survivor net, which reports one stable diagnostic. Carried from
    // `radix-module` with the moved files (goal `radix-crate-dag`, C-M1).
    let_underscore: 4,
    inline_test_modules: 0,
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = hygiene_ratchet::collect_production_files(&cfg);
    let counts = hygiene_ratchet::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
