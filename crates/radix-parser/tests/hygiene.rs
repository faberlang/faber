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
    unwrap: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    // Re-baselined 93→96: the grammar-admission landings added token-shape
    // expects — equality ladder glyph synonyms (24647652c), minmax additive
    // nodes (e54a7b74d), and applied-generic nominal record construction
    // (ea8728ffb); the parser's existing fail-closed token idiom.
    expect: 96,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    panic: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here.
    // Re-baselined 0→1: the conversio annotation decl arm (fcab66016)
    // guards an exhaustive match whose shape was checked above the dispatch.
    unreachable: 1,
    todo: 0,
    unimplemented: 0,
    // WHY: minted at D-03 baseline; frozen, not audited here. The single
    // `let _ = parser.advance();` lived inside the inline non_trivia_index_tests
    // mod; D03-PARSER extraction moved it to the non_trivia_index_test.rs
    // companion, so no production-file `let _ =` remains.
    let_underscore: 0,
    // D03-PARSER extracts lib.rs's non_trivia_index_tests to
    // non_trivia_index_test.rs; zero inline modules remain.
    inline_test_modules: 0,
    // No production #[test] remains after the extraction.
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = hygiene_ratchet::collect_production_files(&cfg);
    let counts = hygiene_ratchet::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}
