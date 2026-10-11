#![allow(clippy::absurd_extreme_comparisons)]

use hygiene_ratchet::{Budgets, ScanConfig, assert_budgets, assert_companion_tests_use_cfg_path_module_convention};
use std::path::Path;

fn config() -> ScanConfig {
    ScanConfig {
        source_roots: vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
        exclude_path_suffixes: vec!["backend_smoke.rs".to_owned(), "mir/text_test_helpers.rs".to_owned()],
        subtract_self_expect: true,
        check_companion_convention: true,
    }
}

const BUDGETS: Budgets = Budgets {
    unwrap: 0,
    // WHY: one `u32::try_from(line)` in `SourceFile::offset_to_line_col`. The
    // line index is a position in the `Vec<u32>` of strictly increasing line
    // starts, so it fits u32 by construction (`SourceFile::from_raw` rejects
    // 4 GB+ bodies); the method is public API returning `(u32, u32)`.
    expect: 1,
    panic: 0,
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    // WHY: the four `substitute_tensor_from_flat_shape_size_constants` /
    // `substitute_shape_size_constants` calls that left these `#[must_use]`
    // bools unread moved with `mir/lower/{monomorphize,package_instantiate}.rs`
    // into `radix-mir-lower` (goal `radix-crate-dag`, C-M1), so the ratchet
    // drops to zero here and the budget is minted in the new crate.
    let_underscore: 0,
    // D03-MODULE-PI extracted package_instantiate.rs `mod tests` (path-wired);
    // D03-MODULE-LH moved the `library_home_test_support` helper into the
    // import_resolve.rs companion (pub(crate) re-export preserves its path).
    inline_test_modules: 0,
    // D03-MODULE-PI extracted the inline test from package_instantiate.rs.
    // Other `#[test]` attrs live in companions.
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = hygiene_ratchet::collect_production_files(&cfg);
    let counts = hygiene_ratchet::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}

#[test]
fn companion_tests_use_cfg_path_module_convention() {
    let files = hygiene_ratchet::collect_production_files(&config());
    assert_companion_tests_use_cfg_path_module_convention(&files);
}
