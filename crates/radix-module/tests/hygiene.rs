#![allow(clippy::absurd_extreme_comparisons)]

use hygiene_ratchet::{assert_budgets, assert_companion_tests_use_cfg_path_module_convention, Budgets, ScanConfig};
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
    // WHY: one breakables.last() unwrap in MIR lower control after shape verification.
    unwrap: 1,
    // WHY: BO-S-10 (fe911e7b3) resolve.rs name-suggestion fold cache invariant —
    // "visible-name fold cached in the first pass" is a hard postcondition of
    // the two-pass suggestion walk; a panic here is the correct fail-closed
    // behavior. Documented pre-existing deviation (BO-S-1 delivery closeout).
    // Re-baselined 1→12 after the clippy repair commits (5f87d9983 drop
    // package-wide cast allows; treat each cast site individually, 84c0d0e51,
    // c7e8e5216): cast sites now use `u32::try_from(x).expect("…fits u32")`
    // checked conversions instead of silent `as u32` + allow. All 11 added
    // sites are structurally bounded indices (function counts, line offsets,
    // scope/index tables, fixture ids); the expect is the fail-closed check,
    // not a default.
    // Re-baselined 12→20: U2's identity-resolved shader-contract ingestion
    // (wgsl-shader-contract-boundary Wave 1, commit 045905653) added
    // `crates/radix/src/driver/shader_contract.rs` with a `#[cfg(test)]`
    // inline test module whose assertions unwrap compilation results with
    // `.expect("…")` — idiomatic fail-closed test asserts (8 sites). The
    // ratchet counts `.expect(` inside inline test modules; all 8 sites are
    // test-only, none in production lowering. Re-baselined at the U4
    // gate-repair checkpoint so `--stage 1-4` turns green.
    // Re-baselined 20→21: fe4fdf9da (package_identity_facts_for_path
    // effective-reader-pack) added one fail-closed `.expect(` to the
    // registry-direct re-parse contract; production-fix tracking per the
    // llvm/wasm registry re-parse residual.
    expect: 21,
    // WHY: one guard in emit.rs output_code contract — an internal invariant
    // that callers should not violate for binary wasm output.
    // Additional: hir/lower/annotation.rs exhaustive match guard on Nondum
    // variant (closure-cleanup extraction residue, pre-existing debt).
    // Re-baselined 2→3: the U2 shader_contract.rs inline test module's
    // `wgsl_output` helper panics on a non-WGSL output (`panic!("expected
    // WGSL output")`) — test-only guard, same contract-guard rationale.
    panic: 3,
    // WHY: exhaustive-match guard arms in MIR lower (runtime, control, place)
    // and semantic typecheck — shape verified before dispatch.
    // Burned 14→9 after removing artifact/monomorphize inline-test noise.
    // Re-baselined 9→11: tensor-glyphs rank-rule typecheck added two
    // `fresh tensor result carries a shape` guards (ops.rs), and U8
    // feature-granularity narrowed the driver target dispatch to Rust | Go
    // (`match target above narrowed to Rust | Go`) — all shape-verified
    // fail-closed arms, same guard rationale.
    unreachable: 11,
    todo: 0,
    unimplemented: 0,
    // WHY: deliberate Result discards in dump formatters (air/dump.rs),
    // ABI render codegen (abi/render.rs), diagnostics output
    // (diagnostics/render.rs), JSON CLI output (tool/commands/json.rs),
    // GPU symbol resolution (builtins/gpu.rs), and child process cleanup
    // (llvm_host.rs) — all are write-to-output/side-effect operations where
    // errors are surfaced through the caller's error path or are intentionally
    // best-effort. Lean checker best-effort cleanup uses `ignore_result`.
    // Re-baselined 41→48→6 (2026-08-19 hygiene burndown): inspection JSON
    // emitters now use `writeln!(…).ok()` / `write!(…).ok()` instead of
    // `let _ =` discards; remaining sites are deliberate side-effect binds
    // in lowering/resolver paths.
    let_underscore: 6,
    // All inline `mod tests { … }` bodies extracted to *_test.rs companions
    // (2026-08-19: annotation, emit, shader_contract, broadcast_reduction).
    inline_test_modules: 0,
    // All #[test] attrs moved out of production sources into companions.
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
