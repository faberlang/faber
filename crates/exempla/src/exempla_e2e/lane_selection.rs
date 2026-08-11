//! Diff-derived lane selection (delivery EL-4).
//!
//! Maps a changed-file path set (e.g. the output of `git diff --name-only`)
//! to the backend lanes the exempla lane grid must run. A packet touching a
//! leaf target crate gates on exactly that lane (`radix-hir-go` → go only);
//! anything that compiles into more than one lane build — shared radix core
//! crates, the exempla harness itself, the shared corpus, faber — selects all
//! lanes. The all-lanes default is the conservative choice: selection trims
//! grid cost for leaf crates and must never hide a red lane.

use std::path::Path;

/// The backend lanes the exempla harness runs in feature isolation
/// (per-lane-e2e-validation delivery decision 1 + 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Lane {
    Go,
    Ts,
    Wasm,
    Rust,
    Swift,
    Sexp,
    Llvm,
    Metal,
    Mir,
    Roundtrip,
}

impl Lane {
    /// The exempla feature that gates this lane's harness build (EL-1
    /// pass-through). `mir` (stepper) and `roundtrip` are the no-backend
    /// minimal lane (delivery decision 2) and run on the bare default build.
    pub(crate) fn feature(self) -> &'static str {
        match self {
            Lane::Go => "hir-go",
            Lane::Ts => "hir-ts",
            Lane::Wasm => "mir-wasm",
            Lane::Rust => "hir-rust",
            Lane::Swift => "hir-swift",
            Lane::Sexp => "mir-sexp",
            Lane::Llvm => "mir-llvm",
            Lane::Metal => "mir-metal",
            Lane::Mir | Lane::Roundtrip => "default",
        }
    }

    /// Stable lane label used in grid receipts and the dry-run output.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Lane::Go => "go",
            Lane::Ts => "ts",
            Lane::Wasm => "wasm",
            Lane::Rust => "rust",
            Lane::Swift => "swift",
            Lane::Sexp => "sexp",
            Lane::Llvm => "llvm",
            Lane::Metal => "metal",
            Lane::Mir => "mir",
            Lane::Roundtrip => "roundtrip",
        }
    }
}

/// All lanes, in canonical (enum) order.
pub(crate) const ALL_LANES: &[Lane] = &[
    Lane::Go,
    Lane::Ts,
    Lane::Wasm,
    Lane::Rust,
    Lane::Swift,
    Lane::Sexp,
    Lane::Llvm,
    Lane::Metal,
    Lane::Mir,
    Lane::Roundtrip,
];

/// Whether the metal lane rides the `radix-mir-llvm` leaf crate.
///
/// Verified 2026-08-11 against `radix/crates/*/Cargo.toml`: `radix-mir-metal`
/// declares no dependency on `radix-mir-llvm` (they are sibling leaf crates;
/// the only cross-references are docs and shared test comments), so a
/// `radix-mir-llvm` change does NOT pull the metal lane. If metal ever gains
/// an llvm dep edge, flip this to `true` and the selection test re-proves the
/// set as `{llvm, metal}`.
const METAL_RIDES_LLVM: bool = false;

/// Leaf target crate → the single lane it gates, via the exempla feature
/// pass-through (delivery decision 1; `radix/crates/radix/Cargo.toml`
/// `[features]` is the ground truth for each `dep:` edge).
const LEAF_CRATE_LANES: &[(&str, Lane)] = &[
    ("radix-hir-go", Lane::Go),
    ("radix-hir-ts", Lane::Ts),
    ("radix-hir-rust", Lane::Rust),
    ("radix-hir-swift", Lane::Swift),
    ("radix-mir-sexp", Lane::Sexp),
    ("radix-mir-metal", Lane::Metal),
    ("radix-mir-wasm", Lane::Wasm),
    ("radix-mir-llvm", Lane::Llvm),
];

/// Lanes a single changed path requires.
///
/// Paths are repo-relative as produced by `git diff --name-only`
/// (`crates/<crate>/…` inside radix, `crates/exempla/…` inside faber).
fn lanes_for_path(path: &Path) -> Vec<Lane> {
    let normalized = path.to_string_lossy().replace('\\', "/");

    // The exempla harness + its helpers: every lane compiles and runs this
    // code, so a harness change gates the whole grid.
    if normalized.contains("crates/exempla") {
        return ALL_LANES.to_vec();
    }

    // Shared corpus data: every lane consumes the same corpus.
    if normalized.contains("corpus/") {
        return ALL_LANES.to_vec();
    }

    if let Some(rest) = normalized.strip_prefix("crates/") {
        if let Some((crate_name, _)) = rest.split_once('/') {
            if let Some((_, lane)) = LEAF_CRATE_LANES
                .iter()
                .find(|(name, _)| *name == crate_name)
            {
                let mut lanes = vec![*lane];
                if *lane == Lane::Llvm && METAL_RIDES_LLVM {
                    lanes.push(Lane::Metal);
                }
                return lanes;
            }
        }
    }

    // Conservative default: shared radix core (facade, mir, stepper, hir,
    // parser, …), faber itself, or an unknown path compiles into every lane
    // build — run all lanes.
    ALL_LANES.to_vec()
}

/// Union of the lanes required by a changed-path set, in canonical order.
pub(crate) fn select_lanes(changed_paths: &[String]) -> Vec<Lane> {
    let mut selected = std::collections::BTreeSet::new();
    for path in changed_paths {
        for lane in lanes_for_path(Path::new(path)) {
            selected.insert(lane);
        }
    }
    ALL_LANES
        .iter()
        .copied()
        .filter(|lane| selected.contains(lane))
        .collect()
}

/// Sample diff used by the selection tests and the manual dry-run
/// (`cargo test -p exempla --lib lane_selection_dry_run -- --ignored --nocapture`).
fn sample_diff() -> Vec<String> {
    [
        // A leaf-crate change: go lane only.
        "crates/radix-hir-go/src/codegen/mod.rs",
        // A shared-core change folds into the all-lanes default.
        "crates/radix/src/driver/mod.rs",
        // A harness change folds into the all-lanes default.
        "crates/exempla/src/exempla_e2e/lane_selection.rs",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn radix_hir_go_change_selects_go_lane_only() {
        let lanes = select_lanes(&diff(&["crates/radix-hir-go/src/codegen/mod.rs"]));
        assert_eq!(lanes, vec![Lane::Go]);
    }

    #[test]
    fn radix_mir_llvm_change_selects_llvm_and_metal_if_metal_rides_llvm() {
        let lanes = select_lanes(&diff(&["crates/radix-mir-llvm/src/lib.rs"]));
        // Metal rides llvm iff the leaf crates share a dep edge. Verified
        // against `radix/crates/*/Cargo.toml`: no edge today, so the set is
        // {llvm}. If METAL_RIDES_LLVM ever flips, this test re-proves {llvm,
        // metal} and the comment in `lane_selection.rs` must be updated.
        let expected: Vec<Lane> = if METAL_RIDES_LLVM {
            vec![Lane::Llvm, Lane::Metal]
        } else {
            vec![Lane::Llvm]
        };
        assert_eq!(lanes, expected);
    }

    #[test]
    fn exempla_only_change_selects_all_lanes() {
        let lanes = select_lanes(&diff(&[
            "crates/exempla/src/exempla_e2e/expectations/go.rs",
            "crates/exempla/Cargo.toml",
        ]));
        assert_eq!(lanes, ALL_LANES.to_vec());
    }

    #[test]
    fn shared_radix_crate_change_selects_all_lanes() {
        // The frontend/MIR core compiles into every lane build.
        for path in [
            "crates/radix/src/driver/mod.rs",
            "crates/radix-mir/src/lower.rs",
            "crates/radix-mir-stepper/src/lib.rs",
            "crates/radix-hir/src/lib.rs",
        ] {
            assert_eq!(select_lanes(&diff(&[path])), ALL_LANES.to_vec(), "{path}");
        }
    }

    #[test]
    fn corpus_change_selects_all_lanes() {
        let lanes = select_lanes(&diff(&["corpus/conversio/conversio.fab"]));
        assert_eq!(lanes, ALL_LANES.to_vec());
    }

    #[test]
    fn mixed_diff_selects_union_in_canonical_order() {
        let lanes = select_lanes(&diff(&[
            "crates/radix-hir-go/src/lib.rs",
            "crates/radix-mir-sexp/src/emit.rs",
        ]));
        assert_eq!(lanes, vec![Lane::Go, Lane::Sexp]);
    }

    #[test]
    fn empty_diff_selects_nothing() {
        assert!(select_lanes(&[]).is_empty());
    }

    #[test]
    fn lane_features_are_distinct_and_match_the_exempla_pass_through() {
        // hir-* / mir-* feature names mirror `crates/exempla/Cargo.toml`
        // `[features]` (EL-1). `mir` + `roundtrip` share the no-backend
        // default build (delivery decision 2).
        let mut features: Vec<&'static str> = ALL_LANES.iter().map(|l| l.feature()).collect();
        features.sort_unstable();
        features.dedup();
        assert_eq!(
            features,
            vec![
                "default",
                "hir-go",
                "hir-rust",
                "hir-swift",
                "hir-ts",
                "mir-llvm",
                "mir-metal",
                "mir-sexp",
                "mir-wasm",
            ]
        );
    }

    #[test]
    fn sample_diff_selects_the_full_lane_set() {
        // The dry-run's sample diff spans a leaf crate + shared core + the
        // harness itself, so the union is all lanes.
        assert_eq!(select_lanes(&sample_diff()), ALL_LANES.to_vec());
    }

    #[test]
    #[ignore = "manual dry-run; run: cargo test -p exempla --lib lane_selection_dry_run -- --ignored --nocapture"]
    fn lane_selection_dry_run() {
        let lanes = select_lanes(&sample_diff());
        let labels: Vec<&'static str> = lanes.iter().map(|lane| lane.label()).collect();
        println!("sample diff (git diff --name-only):");
        for path in sample_diff() {
            println!("  {path}");
        }
        println!("selected lanes: {{{}}}", labels.join(", "));
        assert_eq!(lanes, ALL_LANES.to_vec());
    }
}
