use super::*;
use crate::cli::FaberCliTarget;
use clap::ValueEnum;

/// The compiled feature set of a hir-rust-only small build.
fn small_compiled() -> BTreeSet<&'static str> {
    ["hir-rust"].into_iter().collect()
}

fn row<'a>(rows: &'a [TargetRow], name: &str) -> &'a TargetRow {
    rows.iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("missing row {name}"))
}

#[test]
fn small_build_reports_only_compiled_capabilities() {
    let rows = target_rows(&small_compiled());

    // The rust host lane is present under hir-rust.
    let rust = row(&rows, "rust");
    assert!(rust.available);
    assert!(
        rust.capabilities.check
            && rust.capabilities.build
            && rust.capabilities.run
            && rust.capabilities.package,
        "rust host lane must claim its full capability set:\n{}",
        rust.render()
    );

    // Emit rows whose feature is not compiled claim no capability.
    for name in [
        "fhir",
        "fmir-text",
        "fmir",
        "fmir-bin",
        "faber",
        "go",
        "haskell",
        "wasm",
        "wasm-text",
        "llvm-text",
        "llvm-host",
        "metal-text",
        "wgsl-text",
        "sexp",
        "ts",
        "swift",
    ] {
        let r = row(&rows, name);
        assert!(
            !r.available,
            "{name} must not be available in a small build"
        );
        assert!(
            !r.capabilities.build && !r.capabilities.run && !r.capabilities.package,
            "{name} must claim no build/run/package capability in a small build:\n{}",
            r.render()
        );
    }

    // No device/host-leaf/device-runtime capability rows at all.
    for name in ["device-runtime", "host-macos-arm64"] {
        assert!(
            !rows.iter().any(|r| r.name == name),
            "{name} row must be absent in a small build (capability not compiled)"
        );
    }
}

#[test]
fn default_build_reports_full_surface() {
    // The test binary builds under default features (full-targets), so the
    // real cfg!-derived compiled set is the full emit surface — no
    // table-derived expectation (that circularity hid `swift`).
    let rows = target_rows(&compiled_features());
    // All emit rows; no capability rows compile under the default build
    // (device-runtime / host-macos-arm64 since the S3-U2 cut) — both are
    // opt-in and report no row.
    assert_eq!(rows.len(), FABER_TARGET_ROWS.len());

    for &(_, name, _) in FABER_TARGET_ROWS {
        assert!(row(&rows, name).available, "{name} must be available");
    }
    for name in ["device-runtime", "host-macos-arm64"] {
        assert!(
            !rows.iter().any(|r| r.name == name),
            "{name} row must be absent in the default build (feature left full-targets)"
        );
    }

    // FMIR rows are faber package build/run/package targets.
    for name in ["fmir-text", "fmir", "fmir-bin"] {
        let r = row(&rows, name);
        assert!(
            r.capabilities.check
                && r.capabilities.build
                && r.capabilities.run
                && r.capabilities.package,
            "faber package FMIR row must show package build/run truth:\n{}",
            r.render()
        );
        assert!(
            r.capabilities.note.contains("faber build --target"),
            "FMIR row note must point at the faber package build surface:\n{}",
            r.render()
        );
        assert!(
            !r.capabilities.note.contains("radix emit rejects"),
            "FMIR row must not present radix emit truth as faber command truth:\n{}",
            r.render()
        );
    }
}

#[test]
fn rendered_table_contains_capability_rows_under_full_build() {
    let table = rendered_targets_table();
    assert!(table.contains("rust available=yes"), "{table}");
    // Capability truth under the default build: device-runtime and
    // host-macos-arm64 (opt-in since S3-U2) report no row for an unbuilt
    // surface.
    for name in ["device-runtime", "host-macos-arm64"] {
        assert!(
            !table
                .lines()
                .any(|line| line.starts_with(&format!("{name} "))),
            "unexpected {name} capability row under the default build (feature left full-targets):\n{table}"
        );
    }
}

/// The CLI-selectable target universe: every `FaberCliTarget` variant except
/// the deliberately hidden `scena` spelling. Row-table completeness derives
/// from this set (the CLI grammar's own enumeration), never from the table
/// under test.
fn selectable_cli_targets() -> Vec<radix::module::codegen::Target> {
    FaberCliTarget::value_variants()
        .iter()
        .filter(|variant| !matches!(variant, FaberCliTarget::MirScena))
        .map(|variant| variant.to_radix())
        .collect()
}

#[test]
fn every_cli_selectable_target_has_a_row() {
    let selectable = selectable_cli_targets();
    assert!(
        !selectable.is_empty(),
        "FaberCliTarget::value_variants() must enumerate the CLI grammar"
    );
    for target in &selectable {
        assert!(
            FABER_TARGET_ROWS
                .iter()
                .any(|&(row_target, _, _)| row_target == *target),
            "FABER_TARGET_ROWS must report CLI-selectable target {target:?} \
             (faber targets must not silently omit a live backend)",
        );
    }
}

#[test]
fn every_row_target_is_cli_selectable() {
    // The reverse direction: a row whose Target no CLI spelling selects lies
    // to the reader about selectability. MirWasm's double row
    // (`wasm`/`wasm-text`) is fine — both rows share the MirWasm variant.
    let selectable = selectable_cli_targets();
    for &(target, name, _) in FABER_TARGET_ROWS {
        assert!(
            selectable.contains(&target),
            "row {name} reports {target:?}, which no FaberCliTarget variant selects"
        );
    }
}

#[test]
fn every_row_feature_is_compiled_in_the_default_test_build() {
    // The test binary builds under default features (full-targets): every
    // row's feature string must name a feature this build actually compiled.
    // Catches both a stale feature string and a `cfg!` arm missing from
    // compiled_features() — the two omissions that hid `swift`.
    let compiled = compiled_features();
    for &(_, name, feature) in FABER_TARGET_ROWS {
        assert!(
            compiled.contains(feature),
            "row {name} names feature {feature}, which the default \
             (full-targets) build did not compile: stale row feature string \
             or missing cfg! arm in compiled_features()"
        );
    }
}
