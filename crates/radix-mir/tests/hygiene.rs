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
    unwrap: 1,
    // WHY: minted at D-03 baseline after D03-MIR extraction; frozen, not audited here.
    // Re-baselined 59→69: the transformer-plan landings added fail-closed
    // plan/probe asserts — RMSNorm gamma resource binding (1cd75e23f),
    // interleaved rank-3 RoPE tables (a1c2b104e), and queue/stack elementwise
    // instantiation (ef44b2751), plus one RoPE row in the abi/tests.rs probe.
    // Re-baselined 69→71: the staged composed-matmul ABI probe (8b36e7c92)
    // added two abi/tests.rs probe asserts without this bump.
    // Re-baselined 71→74: the U3 head-axis batched-transpose admission
    // (head-axis-attention) added three abi/tests.rs probe asserts (rank-3,
    // rank-4 contract resolution and the rank-5 typed gate).
    expect: 74,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    panic: 9,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    // Re-baselined 1→5: SSM state-family plan selection (66b482134) added
    // the three conv/scan fact guards in device_plan_compiler.rs, and
    // queue/stack elementwise (ef44b2751) added the intrinsic validation
    // arm; literal_indexed_access.rs carries the checked-index arm. All
    // exhaustive over selections verified before dispatch.
    // Re-baselined 5→7: the package shape-size substitution finalizer
    // (3729cbb6c) added two exhaustive-selection arms without this bump.
    // Re-baselined 7→10: the tensor const validation refactor (9b62da240)
    // added four fail-closed unreachable! guards in validate/intrinsic.rs
    // (kinds pre-matched by the dispatcher — provably unreachable
    // invariants), and deleting the stale TensorConstExemption machinery
    // (f29cb5063) removed one; net +3 without this bump.
    unreachable: 10,
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
