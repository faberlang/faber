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
    // WHY: 43 are `.expect(` in the legacy probe source abi/tests.rs, one is in
    // the packed_negative_matrix test source, and three are infallible
    // invariants in device_semantics.rs (consumer in the component's indegree
    // map) and device_program/constructor.rs (selected function ids and
    // registered resource buffers); converting the constructor sites needs a
    // new variant on the public DeviceProgramBuildError.
    expect: 47,
    // WHY: abi/tests.rs carries two probe panics; MirCompanionMap::insert and
    // the build_subchain_function tuple-type guard (reached from the public
    // KernelDecomposition::subchain_function) are documented `# Panics`
    // entry points.
    panic: 4,
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    let_underscore: 0,
    // D03-MIR extracted all four cfg(test)-gated inline modules in the unit.
    inline_test_modules: 0,
    // The two remaining production #[test] files are intentional legacy
    // probe/test surfaces: abi/tests.rs and packed_negative_matrix.rs.
    test_attr_in_production: 2,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = hygiene_ratchet::collect_production_files(&cfg);
    let counts = hygiene_ratchet::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
