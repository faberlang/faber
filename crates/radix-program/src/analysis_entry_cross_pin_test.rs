//! CDC-1 (h) cross-pin: every production `analyze_source*` caller injects
//! the package facts or is an enumerated fail-closed site.
//!
//! The `b73f39671` defect class was one-site-only fact injection: the
//! PKG-ADM kernel-template admissions landed on the package-own analysis
//! entry (`analyze.rs`) while the library-interface entry (`library.rs`)
//! silently missed them. New analysis entry paths re-create that class the
//! same way, so this test derives the production call-site set from the
//! owning crates' own source (never a hand table alone) and pins each site
//! as either `injects` (calls `analyze_source_with_kernel_template_admissions`
//! with export-surface-derived admissions) or `fail_closed` (the bare
//! `analyze_source` wrapper — standalone single-unit semantics, empty
//! admissions by construction). A new production call site reds this test
//! until it is classified here; a removed site reds it until the table is
//! pruned.
//!
//! Scan boundary: the two crates that own analysis entry paths —
//! `radix-program/src` and `radix-module/src` (test files and test-support
//! excluded). Product/CLI crates delegate to these entry points; a
//! package-aware entry path cannot be built outside them without calling
//! through them.

use std::path::{Path, PathBuf};

/// One production `analyze_source*` call site.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CallSite {
    file: String,
    variant: Variant,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Variant {
    /// `analyze_source_with_kernel_template_admissions(` — must inject the
    /// package facts (admissions derived from an export surface).
    WithAdmissions,
    /// bare `analyze_source(` — the fail-closed standalone wrapper.
    Bare,
}

/// Classify one source line as a call site, if it is one. Definitions
/// (`fn analyze_source*`), `use` imports, and non-call mentions produce no
/// site.
fn classify_line(line: &str) -> Option<Variant> {
    let trimmed = line.trim_start();
    let signature = trimmed.strip_prefix("pub ").unwrap_or(trimmed).trim_start();
    if signature.starts_with("fn ") || trimmed.starts_with("use ") {
        return None;
    }
    if trimmed.contains("analyze_source_with_kernel_template_admissions(") {
        Some(Variant::WithAdmissions)
    } else if trimmed.contains("analyze_source(") {
        Some(Variant::Bare)
    } else {
        None
    }
}

fn collect_call_sites(root: &Path, label: &str) -> Vec<CallSite> {
    let mut sites = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.ends_with("_test.rs") || name == "test_support.rs" {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in source.lines() {
                if let Some(variant) = classify_line(line) {
                    sites.push(CallSite {
                        file: format!(
                            "{label}/src/{}",
                            path.strip_prefix(root).unwrap_or(&path).display()
                        ),
                        variant,
                    });
                }
            }
        }
    }
    sites.sort();
    sites
}

/// The enumerated truth: every production `analyze_source*` call site with
/// its classification. `injects` sites must derive their admissions from an
/// export surface (`PackageKernelTemplateAdmissions::from_export_surface`).
/// The `fail_closed` rows are standalone single-unit entries (no package
/// facts exist for a standalone unit — the wrapper's own contract).
const EXPECTED: &[(&str, Variant, Classification)] = &[
    // Package-own path (analyze.rs): facts from `export_surfaces_by_path`.
    (
        "crates/radix-program/src/analyze.rs",
        Variant::WithAdmissions,
        Classification::Injects,
    ),
    // Library-interface path (library.rs): facts from the cached
    // `program_export_surface` — the b73f39671 mirror-repair seam.
    (
        "crates/radix-program/src/library.rs",
        Variant::WithAdmissions,
        Classification::Injects,
    ),
    // The driver entry-point family: `compile`, `cuda_kernel_descriptor_json`,
    // and `partition_record_json` analyze one standalone source.
    (
        "crates/radix-module/src/driver/mod.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    (
        "crates/radix-module/src/driver/mod.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    (
        "crates/radix-module/src/driver/mod.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    // The wrapper's own delegation to the admissions-aware entry with
    // default (empty) admissions — fail-closed by construction.
    (
        "crates/radix-module/src/driver/mod.rs",
        Variant::WithAdmissions,
        Classification::FailClosed,
    ),
    // Format/convert single-source compile (locale-faithful emit); moved
    // here from codegen/faber/compile.rs by C-D1a (driver/faber_format).
    (
        "crates/radix-module/src/driver/faber_format/compile.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    // Emitted-output revalidation (fmt-revalidation-gate): re-analyzes the
    // emitted single unit through the same standalone pipeline as the
    // format compile above; moved with it to driver/faber_format.
    (
        "crates/radix-module/src/driver/faber_format/compile.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    // `analyze_prepared_program_path`/`_name` — documented fail-closed
    // module analysis for path-shaped e2e/interpret/verify callers.
    (
        "crates/radix-module/src/program/compile.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    (
        "crates/radix-module/src/program/compile.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    // `provider_unit_for_target` — one parsed provider target analyzed
    // under the provider's own contract (carried-provider carriage, moved
    // here from codegen/mod.rs by 6f7333697); no package facts by
    // construction.
    (
        "crates/radix-module/src/program/compile.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    // `export_device_instance` (device_export.rs): the device-instance
    // export seam compiles one standalone entry source through the
    // single-unit route — no package facts exist for it by construction.
    (
        "crates/radix-module/src/device_export.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    // `mir_output_for_source` (services inspect, one source).
    (
        "crates/radix-module/src/services/inspect.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    // Verification-plan gates on in-memory single sources.
    (
        "crates/radix-module/src/tensor_goal_gate.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    (
        "crates/radix-module/src/tensor_goal_gate.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    (
        "crates/radix-module/src/tensor_goal_gate.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
    (
        "crates/radix-module/src/tensor_goal_gate.rs",
        Variant::Bare,
        Classification::FailClosed,
    ),
];

#[derive(Debug, PartialEq, Eq)]
enum Classification {
    Injects,
    FailClosed,
}

/// done_when (h) cross-pin: the derived production call-site set equals the
/// enumerated classification, and every `injects` file derives its
/// admissions from an export surface.
#[test]
fn analysis_entry_admissions_cross_pin() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_crates = manifest
        .parent()
        .expect("radix-program manifest sits inside crates/")
        .to_path_buf();
    let workspace_root = workspace_crates
        .parent()
        .expect("crates/ sits inside the radix workspace root")
        .to_path_buf();

    let mut derived = Vec::new();
    derived.extend(collect_call_sites(
        &workspace_crates.join("radix-program/src"),
        "crates/radix-program",
    ));
    derived.extend(collect_call_sites(
        &workspace_crates.join("radix-module/src"),
        "crates/radix-module",
    ));

    let mut expected: Vec<(String, Variant)> = EXPECTED
        .iter()
        .map(|(file, variant, _)| ((*file).to_owned(), (*variant).clone()))
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut derived_pairs: Vec<(String, Variant)> = derived
        .into_iter()
        .map(|site| (site.file, site.variant))
        .collect();
    derived_pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    assert_eq!(
        derived_pairs, expected,
        "production analyze_source* call sites changed — classify every new site in EXPECTED \
         (injects or fail-closed) or prune removed ones; an unclassified entry path is the \
         b73f39671 one-site-only defect class"
    );

    for (file, variant, classification) in EXPECTED {
        if *classification != Classification::Injects {
            continue;
        }
        assert_eq!(
            *variant,
            Variant::WithAdmissions,
            "{file} must inject through the admissions-aware entry"
        );
        let source = std::fs::read_to_string(workspace_root.join(file))
            .unwrap_or_else(|error| panic!("read {file}: {error}"));
        assert!(
            source.contains("PackageKernelTemplateAdmissions::from_export_surface"),
            "{file} must derive its admissions from an export surface, never default"
        );
    }
}
