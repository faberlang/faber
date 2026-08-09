use super::*;

/// The compiled feature set of a hir-rust-only small build.
fn small_compiled() -> BTreeSet<&'static str> {
    ["hir-rust"].into_iter().collect()
}

/// Every feature key the row tables reference (the full-targets surface).
fn full_compiled() -> BTreeSet<&'static str> {
    FABER_TARGET_ROWS
        .iter()
        .map(|(_, _, feature)| *feature)
        .chain(FABER_CAPABILITY_ROWS.iter().map(|(_, feature, _)| *feature))
        .collect()
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
        "wasm",
        "wasm-text",
        "llvm-text",
        "llvm-host",
        "metal-text",
        "wgsl-text",
        "sexp",
        "ts",
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
    for name in ["device-runtime", "host-macos-arm64", "host-wasm"] {
        assert!(
            !rows.iter().any(|r| r.name == name),
            "{name} row must be absent in a small build (capability not compiled)"
        );
    }
}

#[test]
fn default_build_reports_full_surface() {
    let rows = target_rows(&full_compiled());
    assert_eq!(
        rows.len(),
        FABER_TARGET_ROWS.len() + FABER_CAPABILITY_ROWS.len()
    );

    for &(_, name, _) in FABER_TARGET_ROWS {
        assert!(row(&rows, name).available, "{name} must be available");
    }
    for &(name, _, _) in FABER_CAPABILITY_ROWS {
        assert!(row(&rows, name).available, "{name} must be available");
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
    for name in ["device-runtime", "host-macos-arm64", "host-wasm"] {
        assert!(
            table
                .lines()
                .any(|line| line.starts_with(&format!("{name} "))),
            "missing {name} capability row:\n{table}"
        );
    }
}

#[test]
fn compiled_features_match_full_row_keys_in_default_test_build() {
    // The test binary builds under default features (full-targets): the
    // cfg!-derived compiled set must cover every row key the tables reference.
    assert_eq!(compiled_features(), full_compiled());
}
