//! Faber-owned target capability reporting.
//!
//! `faber targets` reports **capability truth** for the compiled build
//! (DDPP1-U3): every row's `available` flag and build/run/package claims
//! reflect what this faber binary was actually compiled with — capability
//! truth, not a shipping promise (§SelectionPolicy).
//!
//! Row-set policy (decision #6): emit-target rows always list with honest
//! per-row availability; device/host-leaf/device-runtime capability rows are
//! **compiled-only** — a build without the feature reports no row for it.

use radix::tool::{target_capabilities_for_surface, TargetCapabilities, TargetCommandSurface};
use std::collections::BTreeSet;

/// Emit-target rows keyed to the faber feature that must be compiled for the
/// row to be available.
///
/// The `available` flag is faber-side (`cfg!` on the faber feature), never
/// radix's target availability — a faber build can omit a radix target radix
/// considers always-compiled (e.g. `MirLlvm`), and only faber knows its own
/// feature gates. When a row's feature is not compiled, build/run/package are
/// forced off so no row claims a capability the build did not compile.
const FABER_TARGET_ROWS: &[(radix::codegen::Target, &str, &str)] = &[
    (radix::codegen::Target::HirRust, "rust", "hir-rust"),
    (radix::codegen::Target::HirFhir, "fhir", "hir-fhir"),
    (radix::codegen::Target::MirFmir, "fmir-text", "mir-fmir"),
    (radix::codegen::Target::MirFmirBinary, "fmir", "mir-fmir"),
    (
        radix::codegen::Target::MirFmirBundle,
        "fmir-bin",
        "mir-fmir",
    ),
    (radix::codegen::Target::HirFaber, "faber", "hir-faber"),
    (radix::codegen::Target::HirGo, "go", "hir-go"),
    (radix::codegen::Target::MirWasmBinary, "wasm", "mir-wasm"),
    (radix::codegen::Target::MirWasm, "wasm-text", "mir-wasm"),
    (radix::codegen::Target::MirLlvm, "llvm-text", "mir-llvm"),
    (radix::codegen::Target::MirLlvmHost, "llvm-host", "mir-llvm"),
    (radix::codegen::Target::MirMetal, "metal-text", "mir-metal"),
    (radix::codegen::Target::MirWgsl, "wgsl-text", "mir-wgsl"),
    (radix::codegen::Target::MirSexp, "sexp", "mir-sexp"),
    (radix::codegen::Target::HirTypeScript, "ts", "hir-ts"),
];

/// Compiled-only capability rows for the device/host-leaf/device-runtime
/// surfaces. Each row appears only when its faber feature is compiled; a
/// build without the feature reports no row for it (capability truth — the
/// surface is not part of this build). No build/run/package claim: these are
/// compiled support surfaces, not `faber build`/`run` targets.
const FABER_CAPABILITY_ROWS: &[(&str, &str, &str)] = &[
    (
        "device-runtime",
        "device-runtime",
        "device runtime support surface (faber-runtime + package/device modules); compiled only under the device-runtime feature",
    ),
    (
        "host-macos-arm64",
        "host-macos-arm64",
        "native host leaf (Metal/CUDA host session support); compiled only under the host-macos-arm64 feature",
    ),
    (
        "host-wasm",
        "host-wasm",
        "wasm host leaf (browser host session support); compiled only under the host-wasm feature",
    ),
];

/// One rendered capability row.
struct TargetRow {
    name: &'static str,
    available: bool,
    capabilities: TargetCapabilities,
}

impl TargetRow {
    fn render(&self) -> String {
        format!(
            "{} available={} check={} build={} run={} package={} note={}",
            self.name,
            yes_no(self.available),
            yes_no(self.capabilities.check),
            yes_no(self.capabilities.build),
            yes_no(self.capabilities.run),
            yes_no(self.capabilities.package),
            self.capabilities.note
        )
    }
}

/// The capability-driven row set for one compiled feature set.
///
/// Emit-target rows always list (honest per-row availability); device/
/// host-leaf/device-runtime capability rows list only when compiled.
fn target_rows(compiled: &BTreeSet<&'static str>) -> Vec<TargetRow> {
    let mut rows = Vec::with_capacity(FABER_TARGET_ROWS.len() + FABER_CAPABILITY_ROWS.len());
    for &(target, name, feature) in FABER_TARGET_ROWS {
        let available = compiled.contains(feature);
        let mut capabilities = faber_surface_capabilities(
            target,
            target_capabilities_for_surface(TargetCommandSurface::Faber, target),
        );
        if !available {
            // Capability truth: a row whose feature is not compiled claims no
            // build/run/package capability (mirrors radix's availability
            // zeroing, including targets radix treats as always-compiled).
            capabilities.build = false;
            capabilities.run = false;
            capabilities.package = false;
        }
        rows.push(TargetRow {
            name,
            available,
            capabilities,
        });
    }
    for &(name, feature, note) in FABER_CAPABILITY_ROWS {
        if !compiled.contains(feature) {
            continue;
        }
        rows.push(TargetRow {
            name,
            available: true,
            capabilities: TargetCapabilities {
                check: false,
                build: false,
                run: false,
                package: false,
                note,
            },
        });
    }
    rows
}

/// Faber-surface capability overrides on top of radix's
/// `faber_target_capabilities`.
///
/// `faber targets` reports the *faber* package surface, not radix emit truth.
/// FMIR image targets are faber package build/run/package targets
/// (`faber build --target fmir-*`), so radix's "delegated … radix emit
/// rejects" note and zeroed build/run/package are replaced by the faber
/// package surface truth (capability truth T1 — what faber can express, not a
/// shipping promise).
fn faber_surface_capabilities(
    target: radix::codegen::Target,
    capabilities: TargetCapabilities,
) -> TargetCapabilities {
    match target {
        radix::codegen::Target::MirFmir => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "faber package MIR image target: `faber build --target fmir-text`",
        },
        radix::codegen::Target::MirFmirBinary => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "faber package MIR image target: `faber build --target fmir`",
        },
        radix::codegen::Target::MirFmirBundle => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "faber package MIR image target: `faber build --target fmir-bin`",
        },
        _ => capabilities,
    }
}

/// The feature set this binary was compiled with.
fn compiled_features() -> BTreeSet<&'static str> {
    let mut compiled = BTreeSet::new();
    if cfg!(feature = "hir-rust") {
        compiled.insert("hir-rust");
    }
    if cfg!(feature = "hir-fhir") {
        compiled.insert("hir-fhir");
    }
    if cfg!(feature = "mir-fmir") {
        compiled.insert("mir-fmir");
    }
    if cfg!(feature = "hir-faber") {
        compiled.insert("hir-faber");
    }
    if cfg!(feature = "hir-go") {
        compiled.insert("hir-go");
    }
    if cfg!(feature = "mir-wasm") {
        compiled.insert("mir-wasm");
    }
    if cfg!(feature = "mir-llvm") {
        compiled.insert("mir-llvm");
    }
    if cfg!(feature = "mir-metal") {
        compiled.insert("mir-metal");
    }
    if cfg!(feature = "mir-wgsl") {
        compiled.insert("mir-wgsl");
    }
    if cfg!(feature = "mir-sexp") {
        compiled.insert("mir-sexp");
    }
    if cfg!(feature = "hir-ts") {
        compiled.insert("hir-ts");
    }
    if cfg!(feature = "device-runtime") {
        compiled.insert("device-runtime");
    }
    if cfg!(feature = "host-macos-arm64") {
        compiled.insert("host-macos-arm64");
    }
    if cfg!(feature = "host-wasm") {
        compiled.insert("host-wasm");
    }
    compiled
}

/// Render the full `faber targets` table for the compiled build.
pub(crate) fn rendered_targets_table() -> String {
    let compiled = compiled_features();
    target_rows(&compiled)
        .iter()
        .map(TargetRow::render)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn cmd_targets() {
    println!("{}", rendered_targets_table());
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
#[path = "targets_test.rs"]
mod tests;
