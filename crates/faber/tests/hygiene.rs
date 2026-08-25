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
    // WHY: three documented static invariants — device slot-buffer
    // registration (program.rs), and the two prefill registered-input/query-head
    // invariants (prefill_run.rs). cargo.rs / path_deps.rs carry zero expects
    // (fail-closed paths); dense_full_model_run.rs reports missing inputs.
    expect: 4,
    // WHY: two fail-closed device-wire layout guards in package/device/wire.rs
    // (dense matmul + packed storage): the mirrored scalar/packed layout is
    // not on the accepted wire and the mapping must not guess.
    panic: 2,
    // WHY: one exhaustive-match arm in CLI dispatch (cmd.rs) plus one
    // exhaustiveness guard in the mir lowering path (mir/lower.rs).
    unreachable: 2,
    todo: 0,
    unimplemented: 0,
    // WHY: six documented discards — unused device-runtime cfg tuple in
    // mir/mod.rs, three Drop best-effort cleanups in host_spawn.rs
    // (kill, wait, workdir teardown), and the two eval bindings consumed
    // only under the runner feature in commands/mod.rs (b24d13148 gates the
    // runner-only MIR surface; without the feature both stay unused).
    let_underscore: 6,
    inline_test_modules: 0,
    // WHY: one feature-gated ignored device-runtime probe remains in
    // dense_full_model_run.rs; its owner is handling that production test surface.
    test_attr_in_production: 1,
};

#[test]
fn production_hygiene_budgets() {
    let files = hygiene_ratchet::collect_production_files(&config());
    let counts = hygiene_ratchet::count_budgets(&files, config().subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
