//! Faber-owned target capability reporting.
//!
//! `faber targets` reports **capability truth** for the compiled build
//! (DDPP1-U3): every row's `available` flag and build/run/package claims
//! reflect what this faber binary was actually compiled with — capability
//! truth, not a shipping promise (§`SelectionPolicy`).
//!
//! Row-set policy (decision #6): emit-target rows always list with honest
//! per-row availability; device/host-leaf/device-runtime capability rows are
//! **compiled-only** — a build without the feature reports no row for it.
//!
//! The emit-target row set is **derived** (defect-audit S09-3 / B3-2), never
//! hand-typed: the universe is the CLI grammar's own enumeration
//! ([`FaberCliTarget::value_variants`], minus the hidden `scena` spelling),
//! row names come from [`Target`]'s `Display` (the CLI spellings), and each
//! row's feature gate from [`Target::required_feature`] with the documented
//! faber-specific overrides below. A target added to the CLI joins this
//! report by construction.

use crate::cli::FaberCliTarget;
use clap::ValueEnum;
use faber_cli::package::DeviceChoice;
use radix::module::codegen::Target;
use radix::services::{
    TargetCapabilities, TargetCommandSurface, target_capabilities_for_surface, target_leaves_label,
};
use std::collections::BTreeSet;

/// One derived emit-target row: the backend target, its CLI spelling, and
/// the faber feature that must be compiled for the row to be available.
struct FaberTargetRowSpec {
    target: Target,
    name: String,
    feature: &'static str,
}

/// The emit-target row set, derived from the CLI grammar (S09-3).
///
/// The only literal rows left are documented overrides:
///
/// - the fmir family's shared feature gate ([`faber_row_feature`]); and
/// - `MirWasm`'s compatibility alias row (`wasm`): the CLI keeps two live
///   spellings — primary `wasm-text` and the `wasm` alias — so the report
///   carries both rows with identical capability truth (goal Open question 4
///   ruling: the double row may stay explicit).
fn faber_target_row_specs() -> Result<Vec<FaberTargetRowSpec>, String> {
    let mut rows = Vec::new();
    for variant in FaberCliTarget::value_variants() {
        if matches!(variant, FaberCliTarget::MirScena | FaberCliTarget::ClifJit) {
            // Hidden CLI spellings — not discoverable `faber targets` rows:
            // the legacy source-backed scena runtime, and the `clif-jit`
            // engine, which is the `clif` row's in-memory run route.
            continue;
        }
        let target = variant.to_radix();
        rows.push(FaberTargetRowSpec {
            target,
            name: target.to_string(),
            feature: faber_row_feature(target)?,
        });
        if matches!(variant, FaberCliTarget::MirWasm) {
            rows.push(FaberTargetRowSpec {
                target,
                name: "wasm".to_string(),
                feature: faber_row_feature(target)?,
            });
        }
    }
    Ok(rows)
}

/// The faber feature gating one emit-target row.
///
/// Delegates to [`Target::required_feature`] (the feature strings forward
/// 1:1 into faber's own Cargo features), with one documented faber-specific
/// override: the fmir package-image family (`fmir-text` / `fmir`) is
/// faber-build-gated on the shared `mir-fmir` feature, which radix's neutral
/// mapping leaves ungated (`None`) because the runner consumes images
/// whenever any emit surface is present.
fn faber_row_feature(target: Target) -> Result<&'static str, String> {
    match target {
        Target::MirFmir | Target::MirFmirBinary => Ok("mir-fmir"),
        other => other.required_feature().ok_or_else(|| {
            format!(
                "CLI target {other} has no faber feature mapping: every \
                 `faber targets` row must name a faber feature (extend \
                 faber_row_feature's overrides)"
            )
        }),
    }
}

/// Compiled-only capability rows for the device/host-leaf/device-runtime
/// surfaces. Each row appears only when its faber feature is compiled; a
/// build without the feature reports no row for it (capability truth — the
/// surface is not part of this build). No build/run/package claim: these are
/// compiled support surfaces, not `faber build`/`run` targets.
const FABER_CAPABILITY_ROWS: &[(&str, &str, &str)] = &[(
    "device-runtime",
    "device-runtime",
    "device runtime support surface (faber runtime + package/device modules); compiled only under the device-runtime feature",
)];

/// Whether `faber run` reaches `target`.
///
/// `faber run` interprets source by default; the targets it reaches are the
/// device routes selected by `--device` ([`DeviceChoice::run_target_spelling`],
/// the same match that maps a device choice to its route) and the compiled
/// targets `--target` builds and runs ([`RunTarget`]). A built FMIR image, a
/// FHIR envelope, a native host executable and a Rust package are build
/// outputs, never `faber run` inputs.
fn faber_run_reaches(target: Target) -> bool {
    let name = target.to_string();
    DeviceChoice::value_variants()
        .iter()
        .any(|choice| choice.run_target_spelling() == Some(name.as_str()))
        || run_target_names().contains(&name)
}

/// The spellings `faber run --target` accepts.
fn run_target_names() -> Vec<String> {
    crate::cli::RunTarget::value_variants()
        .iter()
        .filter_map(|target| target.to_possible_value())
        .map(|value| value.get_name().to_owned())
        .collect()
}

/// One rendered capability row.
struct TargetRow {
    name: String,
    available: bool,
    capabilities: TargetCapabilities,
    leaves: String,
}

impl TargetRow {
    fn render(&self) -> String {
        format!(
            "{} available={} check={} build={} run={} package={} leaves={} note={}",
            self.name,
            yes_no(self.available),
            yes_no(self.capabilities.check),
            yes_no(self.capabilities.build),
            yes_no(self.capabilities.run),
            yes_no(self.capabilities.package),
            self.leaves,
            self.capabilities.note
        )
    }
}

/// The capability-driven row set for one compiled feature set.
///
/// Emit-target rows always list (honest per-row availability); device/
/// host-leaf/device-runtime capability rows list only when compiled.
///
/// The `available` flag is faber-side (`cfg!` on the faber feature), never
/// radix's target availability — a faber build can omit a radix target radix
/// considers always-compiled (e.g. `MirLlvm`), and only faber knows its own
/// feature gates. When a row's feature is not compiled, build/run/package are
/// forced off so no row claims a capability the build did not compile.
fn target_rows(compiled: &BTreeSet<&'static str>) -> Result<Vec<TargetRow>, String> {
    let specs = faber_target_row_specs()?;
    let mut rows = Vec::with_capacity(specs.len() + FABER_CAPABILITY_ROWS.len());
    for spec in specs {
        let available = compiled.contains(spec.feature);
        let mut capabilities = faber_surface_capabilities(
            spec.target,
            target_capabilities_for_surface(TargetCommandSurface::Faber, spec.target),
        );
        capabilities.run = faber_run_reaches(spec.target);
        if !available {
            // Capability truth: a row whose feature is not compiled claims no
            // build/run/package capability (mirrors radix's availability
            // zeroing, including targets radix treats as always-compiled).
            capabilities.build = false;
            capabilities.run = false;
            capabilities.package = false;
        }
        rows.push(TargetRow {
            name: spec.name,
            available,
            capabilities,
            leaves: target_leaves_label(spec.target),
        });
    }
    for &(name, feature, note) in FABER_CAPABILITY_ROWS {
        if !compiled.contains(feature) {
            continue;
        }
        rows.push(TargetRow {
            name: name.to_string(),
            available: true,
            capabilities: TargetCapabilities {
                check: false,
                build: false,
                run: false,
                package: false,
                note,
            },
            leaves: "-".to_owned(),
        });
    }
    Ok(rows)
}

/// Faber-surface capability overrides on top of radix's
/// `target_capabilities_for_surface(TargetCommandSurface::Faber, ..)`.
///
/// `faber targets` reports the *faber* package surface, not radix emit truth.
/// FMIR image targets are faber package build/run/package targets
/// (`faber build --target fmir-*`), so radix's "delegated … radix emit
/// rejects" note and zeroed build/run/package are replaced by the faber
/// package surface truth (capability truth T1 — what faber can express, not a
/// shipping promise). `run` is not decided here: [`target_rows`] sets it from
/// [`faber_run_reaches`], and these rows claim `build`/`package` only.
fn faber_surface_capabilities(
    target: Target,
    capabilities: TargetCapabilities,
) -> TargetCapabilities {
    match target {
        Target::MirFmir => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "faber package MIR image target: `faber build --target fmir-text`; inspection image, `faber run` does not take it",
        },
        Target::MirFmirBinary => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "faber package MIR image target: `faber build --target fmir`; `faber run` refuses the image, the hidden `faber __fmir-run` executes it",
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
    if cfg!(feature = "hir-python") {
        compiled.insert("hir-python");
    }
    if cfg!(feature = "hir-haskell") {
        compiled.insert("hir-haskell");
    }
    if cfg!(feature = "hir-pascal") {
        compiled.insert("hir-pascal");
    }
    if cfg!(feature = "hir-cobol") {
        compiled.insert("hir-cobol");
    }
    if cfg!(feature = "hir-fortran") {
        compiled.insert("hir-fortran");
    }
    if cfg!(feature = "hir-odin") {
        compiled.insert("hir-odin");
    }
    if cfg!(feature = "hir-c") {
        compiled.insert("hir-c");
    }
    if cfg!(feature = "hir-swift") {
        compiled.insert("hir-swift");
    }
    if cfg!(feature = "mir-wasm") {
        compiled.insert("mir-wasm");
    }
    if cfg!(feature = "mir-llvm") {
        compiled.insert("mir-llvm");
    }
    if cfg!(feature = "mir-evm") {
        compiled.insert("mir-evm");
    }
    if cfg!(feature = "mir-clif") {
        compiled.insert("mir-clif");
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
    if cfg!(feature = "mir-basic") {
        compiled.insert("mir-basic");
    }
    if cfg!(feature = "hir-ts") {
        compiled.insert("hir-ts");
    }
    if cfg!(feature = "device-runtime") {
        compiled.insert("device-runtime");
    }
    compiled
}

/// Render the full `faber targets` table for the compiled build.
pub(crate) fn rendered_targets_table() -> Result<String, String> {
    let compiled = compiled_features();
    Ok(target_rows(&compiled)?
        .iter()
        .map(TargetRow::render)
        .collect::<Vec<_>>()
        .join("\n"))
}

pub(crate) fn cmd_targets() {
    match rendered_targets_table() {
        Ok(table) => println!("{table}"),
        Err(message) => {
            eprintln!("error: {message}");
            faber_cli::exit_status::exit_software();
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
#[path = "targets_test.rs"]
mod tests;
