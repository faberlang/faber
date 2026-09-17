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
    unwrap: 0,
    // WHY: derived from the post-R2 production scan. Two expects remain in
    // the retained product sources; the moved proof files contribute none to
    // this production-root count.
    expect: 2,
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
    // WHY: the feature-gated ignored device-runtime probe moved out of the
    // production source root with the dense/KV proof lane.
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let files = hygiene_ratchet::collect_production_files(&config());
    let counts = hygiene_ratchet::count_budgets(&files, config().subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
