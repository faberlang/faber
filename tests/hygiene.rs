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
    // WHY: four documented static invariants — lockfile duplicate-diagnostic
    // pop (lockfile.rs), device slot-buffer registration (program.rs), and the
    // two prefill registered-input/query-head invariants (prefill_run.rs).
    // cargo.rs / path_deps.rs carry zero expects (fail-closed paths).
    expect: 4,
    // WHY: one guard in the package build error path.
    panic: 1,
    // WHY: one exhaustive-match arm in CLI dispatch (cmd.rs) plus one
    // exhaustiveness guard in the mir lowering path (mir/lower.rs).
    unreachable: 2,
    todo: 0,
    unimplemented: 0,
    // WHY: deliberate discards in library resolution and diagnostic paths
    // (io_buf, core_support assembler/materialize, prefill_run) — pre-existing
    // drift documented by the prior ratchet (expect 1→2, let_ 3→8).
    let_underscore: 10,
    inline_test_modules: 0,
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let files = hygiene_ratchet::collect_production_files(&config());
    let counts = hygiene_ratchet::count_budgets(&files, config().subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
