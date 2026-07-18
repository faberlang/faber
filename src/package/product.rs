use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use radix::diagnostics::Diagnostic;
use sha2::{Digest, Sha256};

use super::manifest::{ManifestProduct, ManifestProductKind};
use super::paths::normalize_path;

/// Generated product output path components — the single source of truth for
/// directory and file names written by the browser product build. Both
/// [`product_generated_output_paths`] and the build function consume these
/// constants so the registry and the writer never diverge.
const FABER_TS_DIR: &str = "faber-ts";
const FABER_ESM_DIR: &str = "faber-esm";
const TSCONFIG_FILE: &str = "tsconfig.faber-browser.json";
const BROWSER_ENTRY_TS: &str = "faber-browser.ts";
const WEB_AMBIENT_DTS: &str = "faber-web.d.ts";
const BROWSER_ENTRY_JS: &str = "faber-browser.js";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProductAssetBuild {
    pub out_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub assets: Vec<BrowserProductAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProductAsset {
    pub kind: &'static str,
    pub source: PathBuf,
    pub output: PathBuf,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy)]
struct AssetRoot<'a> {
    kind: &'static str,
    source: &'a str,
    output_prefix: &'a str,
}

/// Planned static-asset set after preflight checks pass.
struct StaticAssetPlan {
    out_dir: PathBuf,
    manifest_path: PathBuf,
    planned: BTreeMap<PathBuf, PlannedAsset>,
}

/// Collect planned static assets and run preflight checks (stale outputs,
/// collision containment). This is the fail-closed gate: it must run before
/// any cleanup or copy/write so that a collision error does not leave the
/// output directory in a partially destroyed state.
fn plan_browser_product_static_assets(
    package_root: &Path,
    product: &ManifestProduct,
) -> Result<StaticAssetPlan, Box<Diagnostic>> {
    match product.kind {
        ManifestProductKind::BrowserApp => {}
    }

    let package_root = normalize_path(package_root);
    let out_dir = normalize_path(&package_root.join(&product.out));
    let roots = [
        AssetRoot {
            kind: "template",
            source: &product.templates,
            output_prefix: &product.templates,
        },
        AssetRoot {
            kind: "style",
            source: &product.styles,
            output_prefix: &product.styles,
        },
        AssetRoot {
            kind: "public",
            source: &product.public,
            output_prefix: &product.public,
        },
    ];

    let manifest_path = out_dir.join(&product.assets_manifest);
    let mut planned = BTreeMap::<PathBuf, PlannedAsset>::new();
    for root in roots {
        collect_root(&package_root, &out_dir, root, &mut planned)?;
    }

    let generated = product_generated_output_paths(&out_dir, product);
    reject_stale_outputs(&out_dir, &planned, &generated)?;
    reject_output_collisions(&planned, &generated)?;

    Ok(StaticAssetPlan {
        out_dir,
        manifest_path,
        planned,
    })
}

/// Write planned static assets and the asset manifest to disk. Called only
/// after preflight ([`plan_browser_product_static_assets`]) and cleanup.
fn write_browser_product_static_assets(
    plan: StaticAssetPlan,
) -> Result<BrowserProductAssetBuild, Box<Diagnostic>> {
    let StaticAssetPlan {
        out_dir,
        manifest_path,
        planned,
    } = plan;

    for (output, asset) in &planned {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|err| io_diag(parent, err))?;
        }
        fs::copy(&asset.source, output).map_err(|err| io_diag(output, err))?;
    }

    let assets = planned
        .into_iter()
        .map(|(output, planned)| BrowserProductAsset {
            kind: planned.kind,
            source: planned.source,
            output,
            size: planned.size,
            sha256: planned.sha256,
        })
        .collect::<Vec<_>>();

    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent).map_err(|err| io_diag(parent, err))?;
    }
    fs::write(&manifest_path, render_asset_manifest(&out_dir, &assets))
        .map_err(|err| io_diag(&manifest_path, err))?;

    Ok(BrowserProductAssetBuild {
        out_dir,
        manifest_path,
        assets,
    })
}

/// Build the static-asset portion of a browser-app product recipe.
///
/// WEB2 owns only deterministic HTML/CSS/public asset copying. Controller TS,
/// `tsc`, and `controllers.json` are later WEB3 work; the asset manifest written
/// here gives those stages deterministic static paths without inventing a Radix
/// `web` target.
///
/// Convenience wrapper: plan (preflight) + write in one call. For callers that
/// need cleanup between preflight and write (e.g. [`build_browser_product`]),
/// call the two phases directly.
#[cfg(test)]
pub(crate) fn build_browser_product_static_assets(
    package_root: &Path,
    product: &ManifestProduct,
) -> Result<BrowserProductAssetBuild, Box<Diagnostic>> {
    let plan = plan_browser_product_static_assets(package_root, product)?;
    write_browser_product_static_assets(plan)
}

#[derive(Debug)]
struct PlannedAsset {
    kind: &'static str,
    source: PathBuf,
    size: u64,
    sha256: String,
}

/// A generated product output path — written by the build into `out_dir`
/// beyond copied static assets. `is_dir` marks directory outputs that own
/// their entire subtree (e.g. `faber-ts/`, `faber-esm/`).
#[derive(Debug)]
struct GeneratedOutput {
    label: &'static str,
    path: PathBuf,
    is_dir: bool,
}

fn collect_root(
    package_root: &Path,
    out_dir: &Path,
    root: AssetRoot<'_>,
    planned: &mut BTreeMap<PathBuf, PlannedAsset>,
) -> Result<(), Box<Diagnostic>> {
    let source_root = normalize_path(&package_root.join(root.source));
    if source_root == *out_dir || path_is_inside(out_dir, &source_root) {
        return Err(Box::new(
            product_diag(format!(
                "browser product output `{}` must not be inside static asset root `{}`",
                out_dir.display(),
                source_root.display()
            ))
            .with_arg("issue", "product_output_overlaps_asset_root"),
        ));
    }
    if !source_root.exists() {
        return Err(Box::new(
            product_diag(format!(
                "browser product {} root `{}` must be a real directory",
                root.kind,
                source_root.display()
            ))
            .with_arg("issue", "product_asset_root_missing"),
        ));
    }
    let metadata = fs::symlink_metadata(&source_root).map_err(|err| io_diag(&source_root, err))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Box::new(
            product_diag(format!(
                "browser product {} root `{}` must be a real directory",
                root.kind,
                source_root.display()
            ))
            .with_arg("issue", "product_asset_root_missing"),
        ));
    }

    collect_dir(&source_root, &source_root, root, out_dir, planned)
}

fn collect_dir(
    dir: &Path,
    source_root: &Path,
    root: AssetRoot<'_>,
    out_dir: &Path,
    planned: &mut BTreeMap<PathBuf, PlannedAsset>,
) -> Result<(), Box<Diagnostic>> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| io_diag(dir, err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| io_diag(dir, err))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|err| io_diag(&path, err))?;
        if metadata.file_type().is_symlink() {
            return Err(Box::new(
                product_diag(format!(
                    "browser product asset `{}` must not be a symlink",
                    path.display()
                ))
                .with_arg("issue", "product_asset_symlink"),
            ));
        }
        if metadata.is_dir() {
            collect_dir(&path, source_root, root, out_dir, planned)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(Box::new(
                product_diag(format!(
                    "browser product asset `{}` must be a regular file",
                    path.display()
                ))
                .with_arg("issue", "product_asset_not_file"),
            ));
        }

        let rel = path.strip_prefix(source_root).map_err(|_| {
            product_diag(format!(
                "browser product asset `{}` escaped root `{}`",
                path.display(),
                source_root.display()
            ))
            .with_arg("issue", "product_asset_path_escape")
        })?;
        reject_relative_escape(rel)?;
        let output = normalize_path(&out_dir.join(root.output_prefix).join(rel));
        let bytes = fs::read(&path).map_err(|err| io_diag(&path, err))?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let planned_asset = PlannedAsset {
            kind: root.kind,
            source: normalize_path(&path),
            size: bytes.len() as u64,
            sha256,
        };
        if let Some(existing) = planned.insert(output.clone(), planned_asset) {
            return Err(Box::new(
                product_diag(format!(
                    "browser product assets `{}` and `{}` both write `{}`",
                    existing.source.display(),
                    path.display(),
                    output.display()
                ))
                .with_arg("issue", "product_asset_collision"),
            ));
        }
    }
    Ok(())
}

fn reject_relative_escape(path: &Path) -> Result<(), Box<Diagnostic>> {
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return Err(Box::new(
            product_diag(format!(
                "browser product asset path `{}` must stay inside its root",
                path.display()
            ))
            .with_arg("issue", "product_asset_path_escape"),
        ));
    }
    Ok(())
}

fn reject_stale_outputs(
    out_dir: &Path,
    planned: &BTreeMap<PathBuf, PlannedAsset>,
    generated: &[GeneratedOutput],
) -> Result<(), Box<Diagnostic>> {
    let Ok(metadata) = fs::symlink_metadata(out_dir) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Box::new(
            product_diag(format!(
                "browser product output `{}` must be a real directory",
                out_dir.display()
            ))
            .with_arg("issue", "product_output_not_directory"),
        ));
    }
    let allowed = planned
        .keys()
        .cloned()
        .chain(generated.iter().map(|gen| gen.path.clone()))
        .collect::<BTreeSet<_>>();
    reject_stale_dir(out_dir, &allowed)
}

/// Collect all generated product output paths — files and directories the
/// product build writes into `out_dir` beyond copied static assets.
///
/// This is the single source of truth for collision guards, stale-output
/// checking, and cleanup.
fn product_generated_output_paths(
    out_dir: &Path,
    product: &ManifestProduct,
) -> Vec<GeneratedOutput> {
    vec![
        GeneratedOutput {
            label: "assets manifest",
            path: normalize_path(&out_dir.join(&product.assets_manifest)),
            is_dir: false,
        },
        GeneratedOutput {
            label: "controllers json",
            path: normalize_path(&out_dir.join(&product.controllers_json)),
            is_dir: false,
        },
        GeneratedOutput {
            label: "faber-ts directory",
            path: normalize_path(&out_dir.join(FABER_TS_DIR)),
            is_dir: true,
        },
        GeneratedOutput {
            label: "faber-esm directory",
            path: normalize_path(&out_dir.join(FABER_ESM_DIR)),
            is_dir: true,
        },
        GeneratedOutput {
            label: "tsconfig",
            path: normalize_path(&out_dir.join(TSCONFIG_FILE)),
            is_dir: false,
        },
    ]
}

/// Fail closed when any generated product output path collides with a planned
/// static asset or with another generated output. The check is symmetric and
/// component-aware: it rejects equal paths OR either path being an ancestor of
/// the other. This does not depend on `is_dir` — a configurable generated file
/// can still land inside a static directory subtree or vice versa. Without this
/// guard a static asset under a generated directory can be silently overwritten
/// while the asset manifest still records the original file's hash.
fn reject_output_collisions(
    planned: &BTreeMap<PathBuf, PlannedAsset>,
    generated: &[GeneratedOutput],
) -> Result<(), Box<Diagnostic>> {
    // Generated outputs vs planned static assets — symmetric component-aware
    // overlap: equal paths OR either path an ancestor of the other.
    for gen in generated {
        for (planned_path, asset) in planned {
            if planned_path == &gen.path
                || path_is_inside(planned_path, &gen.path)
                || path_is_inside(&gen.path, planned_path)
            {
                return Err(Box::new(
                    product_diag(format!(
                        "browser product {} path `{}` collides with static asset from `{}`",
                        gen.label,
                        gen.path.display(),
                        asset.source.display()
                    ))
                    .with_arg("issue", "product_output_collision"),
                ));
            }
        }
    }

    // Generated outputs vs each other.
    for (i, gen_a) in generated.iter().enumerate() {
        for gen_b in generated.iter().skip(i + 1) {
            let collides = path_is_inside(&gen_b.path, &gen_a.path)
                || path_is_inside(&gen_a.path, &gen_b.path);
            if collides {
                return Err(Box::new(
                    product_diag(format!(
                        "browser product {} path `{}` collides with {} path `{}`",
                        gen_a.label,
                        gen_a.path.display(),
                        gen_b.label,
                        gen_b.path.display(),
                    ))
                    .with_arg("issue", "product_output_collision"),
                ));
            }
        }
    }
    Ok(())
}

fn reject_stale_dir(dir: &Path, allowed: &BTreeSet<PathBuf>) -> Result<(), Box<Diagnostic>> {
    for entry in fs::read_dir(dir).map_err(|err| io_diag(dir, err))? {
        let entry = entry.map_err(|err| io_diag(dir, err))?;
        let path = normalize_path(&entry.path());
        let metadata = fs::symlink_metadata(&path).map_err(|err| io_diag(&path, err))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            // Generated directories own their subtrees — stale checking does
            // not recurse into them.
            if allowed.contains(&path) {
                continue;
            }
            reject_stale_dir(&path, allowed)?;
            continue;
        }
        if !allowed.contains(&path) {
            return Err(Box::new(
                product_diag(format!(
                    "browser product output contains stale file `{}`",
                    path.display()
                ))
                .with_arg("issue", "product_stale_output"),
            ));
        }
    }
    Ok(())
}

fn path_is_inside(path: &Path, parent: &Path) -> bool {
    path.strip_prefix(parent).is_ok()
}

fn render_asset_manifest(out_dir: &Path, assets: &[BrowserProductAsset]) -> String {
    let mut out = String::from("{\n  \"version\": 1,\n  \"assets\": [\n");
    for (index, asset) in assets.iter().enumerate() {
        let comma = if index + 1 == assets.len() { "" } else { "," };
        out.push_str(&format!(
            "    {{ \"kind\": \"{}\", \"path\": \"{}\", \"size\": {}, \"sha256\": \"{}\" }}{}\n",
            asset.kind,
            json_escape(&relative_manifest_path(out_dir, &asset.output)),
            asset.size,
            asset.sha256,
            comma
        ));
    }
    out.push_str("  ]\n}\n");
    out
}

fn relative_manifest_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserProductBuild {
    pub out_dir: PathBuf,
    pub controllers_json: PathBuf,
    pub esm_entry: PathBuf,
    pub controllers: Vec<BrowserController>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct BrowserController {
    pub name: String,
    pub selector: String,
    pub module: String,
    pub export: String,
}

/// Build a browser application product from a package graph.
///
/// Invariant: browser packaging consumes Radix's TypeScript backend as a host
/// language and owns controller manifests/`tsc`; it never introduces a Radix
/// web codegen target.
pub(crate) fn build_browser_product(
    config: &radix::driver::Config,
    input: &Path,
    product: &ManifestProduct,
) -> Result<BrowserProductBuild, Box<Diagnostic>> {
    let layout = super::discover_build_layout(input)?;
    // Preflight (collision containment) BEFORE cleanup so a collision error
    // does not destroy previous generated outputs.
    let plan = plan_browser_product_static_assets(&layout.package_root, product)?;
    remove_previous_product_generated_outputs(&layout.package_root, product)?;
    let static_build = write_browser_product_static_assets(plan)?;
    let package = super::analyze_package(config, input).map_err(|diagnostics| {
        Box::new(diagnostics.into_iter().next().unwrap_or_else(|| {
            product_diag("browser product package analysis failed")
                .with_file(input.display().to_string())
                .with_arg("issue", "product_package_analysis_failed")
        }))
    })?;
    let controllers = discover_controllers(&package)?;
    let ts_root = static_build.out_dir.join(FABER_TS_DIR);
    let esm_root = static_build.out_dir.join(FABER_ESM_DIR);
    fs::create_dir_all(&ts_root).map_err(|err| io_diag(&ts_root, err))?;
    fs::create_dir_all(&esm_root).map_err(|err| io_diag(&esm_root, err))?;

    emit_typescript_modules(&package, &ts_root, &controllers)?;
    let browser_entry = ts_root.join(BROWSER_ENTRY_TS);
    fs::write(&browser_entry, render_browser_entry(&controllers))
        .map_err(|err| io_diag(&browser_entry, err))?;
    let declarations = ts_root.join(WEB_AMBIENT_DTS);
    fs::write(&declarations, web_ambient_declarations())
        .map_err(|err| io_diag(&declarations, err))?;
    let tsconfig = static_build.out_dir.join(TSCONFIG_FILE);
    fs::write(&tsconfig, render_tsconfig(&ts_root, &esm_root))
        .map_err(|err| io_diag(&tsconfig, err))?;
    invoke_tsc(&tsconfig)?;

    let controllers_json = static_build.out_dir.join(&product.controllers_json);
    fs::write(&controllers_json, render_controllers_json(&controllers)?)
        .map_err(|err| io_diag(&controllers_json, err))?;
    let esm_entry = esm_root.join(BROWSER_ENTRY_JS);
    if !esm_entry.is_file() {
        return Err(Box::new(
            product_diag(format!(
                "browser product TypeScript build did not write `{}`",
                esm_entry.display()
            ))
            .with_arg("issue", "product_esm_entry_missing"),
        ));
    }

    Ok(BrowserProductBuild {
        out_dir: static_build.out_dir,
        controllers_json,
        esm_entry,
        controllers,
    })
}

/// Remove all generated product outputs from a previous build.
///
/// Uses [`product_generated_output_paths`] as the single source of truth so
/// that collision guards and cleanup stay in sync. Previously this function
/// had its own hardcoded list that diverged (omitted `assets_manifest`).
fn remove_previous_product_generated_outputs(
    package_root: &Path,
    product: &ManifestProduct,
) -> Result<(), Box<Diagnostic>> {
    let out_dir = normalize_path(&package_root.join(&product.out));
    for gen in product_generated_output_paths(&out_dir, product) {
        // Use the declared output kind, not the live path shape. A shape
        // mismatch is corrupted state — fail closed rather than guessing.
        if gen.is_dir {
            if gen.path.exists() && !gen.path.is_dir() {
                return Err(Box::new(
                    product_diag(format!(
                        "browser product {} path `{}` is declared as a directory but exists as a non-directory",
                        gen.label,
                        gen.path.display()
                    ))
                    .with_arg("issue", "product_output_shape_mismatch"),
                ));
            }
            if gen.path.exists() {
                fs::remove_dir_all(&gen.path).map_err(|err| io_diag(&gen.path, err))?;
            }
        } else {
            if gen.path.exists() && gen.path.is_dir() {
                return Err(Box::new(
                    product_diag(format!(
                        "browser product {} path `{}` is declared as a file but exists as a directory",
                        gen.label,
                        gen.path.display()
                    ))
                    .with_arg("issue", "product_output_shape_mismatch"),
                ));
            }
            if gen.path.exists() {
                fs::remove_file(&gen.path).map_err(|err| io_diag(&gen.path, err))?;
            }
        }
    }
    Ok(())
}

fn discover_controllers(
    package: &super::AnalyzedPackage,
) -> Result<Vec<BrowserController>, Box<Diagnostic>> {
    let mut controllers = Vec::new();
    let mut selectors = BTreeMap::<String, String>::new();
    for unit in &package.units {
        let module = ts_module_file_name(unit);
        for item in &unit.analysis.hir.items {
            let radix::hir::HirItemKind::Function(function) = &item.kind else {
                continue;
            };
            let Some(selector) = web_controller_selector(unit, function) else {
                continue;
            };
            validate_selector(&selector, &unit.path)?;
            validate_controller_origin(unit, function)?;
            validate_controller_signature(unit, function)?;
            let name = unit.analysis.interner.resolve(function.name).to_owned();
            if let Some(existing) = selectors.insert(selector.clone(), name.clone()) {
                return Err(Box::new(
                    product_diag(format!(
                        "browser controllers `{existing}` and `{name}` both mount `{selector}`"
                    ))
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "product_duplicate_mount")
                    .with_arg("selector", selector),
                ));
            }
            controllers.push(BrowserController {
                name: name.clone(),
                selector,
                module: format!("./{}", module.replace(".ts", ".js")),
                export: name,
            });
        }
    }
    if controllers.is_empty() {
        return Err(Box::new(
            product_diag("browser product declares no WebController functions")
                .with_file(package.spec.package_root.display().to_string())
                .with_arg("issue", "product_controller_missing"),
        ));
    }
    controllers.sort_by(|a, b| (&a.selector, &a.name).cmp(&(&b.selector, &b.name)));
    Ok(controllers)
}

fn web_controller_selector(
    unit: &super::AnalyzedPackageUnit,
    function: &radix::hir::HirFunction,
) -> Option<String> {
    function.annotations.iter().find_map(|annotation| {
        let contract_id = annotation.contract_id?;
        let contract = unit
            .analysis
            .annotation_contracts
            .registry
            .get(contract_id)?;
        if unit.analysis.interner.resolve(contract.name) != "WebController" {
            return None;
        }
        annotation.fields.iter().find_map(|field| {
            if unit.analysis.interner.resolve(field.name) != "selector" {
                return None;
            }
            match field.value {
                radix::hir::HirAnnotationValue::String(symbol) => {
                    Some(unit.analysis.interner.resolve(symbol).to_owned())
                }
                _ => None,
            }
        })
    })
}

/// Verify the WebController annotation contract originates from the `web`
/// package's `web` module — not a local shadowing definition.
fn validate_controller_origin(
    unit: &super::AnalyzedPackageUnit,
    function: &radix::hir::HirFunction,
) -> Result<(), Box<Diagnostic>> {
    for annotation in &function.annotations {
        let Some(contract_id) = annotation.contract_id else {
            continue;
        };
        let Some(contract) = unit.analysis.annotation_contracts.registry.get(contract_id) else {
            continue;
        };
        if unit.analysis.interner.resolve(contract.name) != "WebController" {
            continue;
        }
        let controller_name = unit.analysis.interner.resolve(function.name);
        match unit.analysis.libraries.items.get(&contract.def_id) {
            Some(item)
                if matches!(&item.identity.provider, radix::hir::LibraryProvider::Package(name) if name == "web")
                    && item.identity.module_path == ["web".to_owned()]
                    && item.exported_name == "WebController" =>
            {
                return Ok(());
            }
            Some(item) => {
                return Err(Box::new(
                    product_diag(format!(
                        "browser controller `{controller_name}` annotation `WebController` must originate from web:web; found `{}`",
                        library_item_display_key(item)
                    ))
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "product_controller_unqualified_origin")
                    .with_arg("controller", controller_name.to_owned()),
                ));
            }
            None => {
                return Err(Box::new(
                    product_diag(format!(
                        "browser controller `{controller_name}` annotation `WebController` must be imported from web:web; local definitions are rejected"
                    ))
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "product_controller_unqualified_origin")
                    .with_arg("controller", controller_name.to_owned()),
                ));
            }
        }
    }
    Ok(())
}

fn validate_selector(selector: &str, file: &Path) -> Result<(), Box<Diagnostic>> {
    let valid = !selector.is_empty()
        && !selector
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
        && matches!(selector.as_bytes().first(), Some(b'#' | b'.' | b'['));
    if !valid {
        return Err(Box::new(
            product_diag(format!(
                "browser controller selector `{selector}` must be a static id, class, or attribute selector"
            ))
            .with_file(file.display().to_string())
            .with_arg("issue", "product_invalid_static_selector")
            .with_arg("selector", selector),
        ));
    }
    Ok(())
}

fn validate_controller_signature(
    unit: &super::AnalyzedPackageUnit,
    function: &radix::hir::HirFunction,
) -> Result<(), Box<Diagnostic>> {
    let name = unit.analysis.interner.resolve(function.name).to_owned();
    if function.params.len() != 1 {
        return Err(Box::new(
            product_diag(format!(
                "browser controller `{name}` must take exactly one DOM Scope parameter"
            ))
            .with_file(unit.path.display().to_string())
            .with_arg("issue", "product_invalid_controller_signature")
            .with_arg("controller", name),
        ));
    }
    if !param_is_dom_scope(unit, &function.params[0]) {
        return Err(Box::new(
            product_diag(format!(
                "browser controller `{name}` first parameter must be web:dom Scope"
            ))
            .with_file(unit.path.display().to_string())
            .with_arg("issue", "product_invalid_controller_signature")
            .with_arg("controller", name),
        ));
    }
    Ok(())
}

fn param_is_dom_scope(unit: &super::AnalyzedPackageUnit, param: &radix::hir::HirParam) -> bool {
    let radix::semantic::Type::Struct(def_id) = unit.analysis.types.get(param.ty) else {
        return false;
    };
    let symbol = match unit.analysis.resolver.get_symbol(*def_id) {
        Some(symbol) => symbol,
        None => return false,
    };
    if unit.analysis.interner.resolve(symbol.name) != "Scope" {
        return false;
    }
    // Provenance must originate from web:dom — reject local shadowing.
    matches!(
        unit.analysis.libraries.items.get(def_id),
        Some(item)
            if matches!(&item.identity.provider, radix::hir::LibraryProvider::Package(name) if name == "web")
                && item.identity.module_path == ["dom".to_owned()]
                && item.exported_name == "Scope"
    )
}

fn emit_typescript_modules(
    package: &super::AnalyzedPackage,
    ts_root: &Path,
    controllers: &[BrowserController],
) -> Result<(), Box<Diagnostic>> {
    let latin = radix::reader_locale::latin_reader_pack();
    let surface = radix::reader_locale::KeywordSurface::new(&latin);
    for unit in &package.units {
        let code = match radix::codegen::generate_from_analyzed(
            radix::codegen::Target::TypeScript,
            &unit.analysis,
            &surface,
        ) {
            Ok(radix::Output::TypeScript(output)) => output.code,
            Ok(_) => {
                return Err(Box::new(
                    product_diag("TypeScript product codegen returned a non-TypeScript output")
                        .with_file(unit.path.display().to_string())
                        .with_arg("issue", "product_typescript_codegen_failed"),
                ))
            }
            Err(err) => {
                let mut diag = product_diag(err.message)
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "product_typescript_codegen_failed");
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                return Err(Box::new(diag));
            }
        };
        let code = adapt_controller_typescript(code, controllers);
        let path = ts_root.join(ts_module_file_name(unit));
        fs::write(&path, code).map_err(|err| io_diag(&path, err))?;
    }
    Ok(())
}

fn adapt_controller_typescript(mut code: String, controllers: &[BrowserController]) -> String {
    for controller in controllers {
        code = code.replace(
            &format!("function {}(", controller.export),
            &format!("export function {}(", controller.export),
        );
    }
    // Imported nominal types are package-interface facts today, but Radix's TS
    // printer has no portable module-qualified type spelling yet. Faber's
    // product layer already validated the controller signature structurally;
    // keep `tsc` fail-closed for the emitted JavaScript while WEB4 supplies the
    // concrete DOM runtime surface.

    // Struct construction `new unresolved_def()` must become an empty object
    // before the type-name pass, otherwise `new any()` is not a valid value.
    code = code.replace("new unresolved_def()", "{}");
    // Arrow-function closures with explicit `: void` return annotations reject
    // bodies that return a Promise (async handler).  Drop the annotation so
    // TypeScript infers the return type; assignment to a `void`-typed handler
    // parameter still accepts any return value.
    code = code.replace("): void =>", ") =>");
    code.replace("unresolved_def", "any")
}

fn ts_module_file_name(unit: &super::AnalyzedPackageUnit) -> String {
    if unit.module_segments.is_empty() {
        return "main.ts".to_owned();
    }
    format!("{}.ts", unit.module_segments.join("_"))
}

fn render_browser_entry(controllers: &[BrowserController]) -> String {
    let mut out = String::from("// Generated by faber browser product packaging.\n");
    for controller in controllers {
        out.push_str(&format!(
            "import {{ {} as {} }} from {:?};\n",
            controller.export, controller.name, controller.module
        ));
    }
    out.push_str("\nexport const controllers = [\n");
    for controller in controllers {
        out.push_str(&format!(
            "  {{ name: {:?}, selector: {:?}, mount: {} }},\n",
            controller.name, controller.selector, controller.name
        ));
    }
    out.push_str("] as const;\n");
    out
}

fn web_ambient_declarations() -> &'static str {
    r#"declare module "web:dom" {
  export class Scope { selector: string; constructor(fields: { selector?: string }); }
  export class Element { selector: string; constructor(fields: { selector?: string }); }
  export class DomEvent { kind: string; default_prevented: boolean; }
  export class Subscription { id: number; }
  export class SubmitOptions { prevent_default: boolean; constructor(fields?: { prevent_default?: boolean }); }
  export class FetchRequest { url: string; method: string; body: string | null; constructor(fields: { url: string; method?: string; body?: string | null }); }
  export class FetchResponse { status: number; ok: boolean; body: string; }
  export type EventHandler = (event: DomEvent) => void;
  export type InputHandler = (element: Element, value: string) => void;
  export type SubmitHandler = (form: Element) => void;
  export function scope(selector: string): Scope;
  export function element(selector: string): Element;
  export function query(scope: Scope, selector: string): Element | null;
  export function require(scope: Scope, selector: string): Element;
  export function all(scope: Scope, selector: string): Element[];
  export function text_set(element: Element, value: string): void;
  export function attr_set(element: Element, name: string, value: string): void;
  export function attr_remove(element: Element, name: string): void;
  export function class_add(element: Element, class_name: string): void;
  export function class_remove(element: Element, class_name: string): void;
  export function class_toggle(element: Element, class_name: string): void;
  export function on(element: Element, event_name: string, handler: EventHandler): Subscription;
  export function unsubscribe(subscription: Subscription): void;
  export function value(element: Element): string;
  export function value_set(element: Element, value: string): void;
  export function on_input(element: Element, handler: InputHandler): Subscription;
  export function on_submit(form: Element, options: SubmitOptions, handler: SubmitHandler): Subscription;
  export function prevent_default(event: DomEvent): DomEvent;
  export function fetch_text(request: FetchRequest): Promise<FetchResponse>;
  export const dom: {
    scope(selector: string): Scope;
    element(selector: string): Element;
    query(scope: Scope, selector: string): Element | null;
    require(scope: Scope, selector: string): Element;
    all(scope: Scope, selector: string): Element[];
    text_set(element: Element, value: string): void;
    attr_set(element: Element, name: string, value: string): void;
    attr_remove(element: Element, name: string): void;
    class_add(element: Element, class_name: string): void;
    class_remove(element: Element, class_name: string): void;
    class_toggle(element: Element, class_name: string): void;
    on(element: Element, event_name: string, handler: EventHandler): Subscription;
    unsubscribe(subscription: Subscription): void;
    value(element: Element): string;
    value_set(element: Element, value: string): void;
    on_input(element: Element, handler: InputHandler): Subscription;
    on_submit(form: Element, options: SubmitOptions, handler: SubmitHandler): Subscription;
    prevent_default(event: DomEvent): DomEvent;
    fetch_text(request: FetchRequest): Promise<FetchResponse>;
  };
}
declare module "web:web" {
  export class Mount { selector: string; constructor(fields: { selector?: string }); }
  export function mount(selector: string): Mount;
  export function selector_of(mount: Mount): string;
  export const web: {
    mount(selector: string): Mount;
    selector_of(mount: Mount): string;
  };
}
"#
}

fn render_tsconfig(ts_root: &Path, esm_root: &Path) -> String {
    format!(
        r#"{{
  "compilerOptions": {{
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmitOnError": true,
    "rootDir": {root_dir:?},
    "outDir": {out_dir:?},
    "skipLibCheck": true
  }},
  "include": [{include:?}]
}}
"#,
        root_dir = ts_root.to_string_lossy().to_string(),
        out_dir = esm_root.to_string_lossy().to_string(),
        include = format!("{}/*.ts", ts_root.to_string_lossy())
    )
}

fn invoke_tsc(tsconfig: &Path) -> Result<(), Box<Diagnostic>> {
    let output = std::process::Command::new("tsc")
        .arg("--project")
        .arg(tsconfig)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(Box::new(
                product_diag("browser product requires `tsc` on PATH")
                    .with_file(tsconfig.display().to_string())
                    .with_arg("issue", "product_tsc_missing"),
            ))
        }
        Err(err) => return Err(io_diag(tsconfig, err)),
    };
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Box::new(
            product_diag(format!(
                "browser product TypeScript check failed: {stdout}{stderr}"
            ))
            .with_file(tsconfig.display().to_string())
            .with_arg("issue", "product_tsc_failed"),
        ));
    }
    Ok(())
}

fn render_controllers_json(controllers: &[BrowserController]) -> Result<String, Box<Diagnostic>> {
    serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "controllers": controllers,
    }))
    .map(|mut json| {
        json.push('\n');
        json
    })
    .map_err(|err| {
        Box::new(product_diag(format!(
            "failed to render controllers.json: {err}"
        )))
    })
}

fn product_diag(message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message.into())
}

/// Human-readable display key for a library item's provenance.
fn library_item_display_key(item: &radix::hir::LibraryItem) -> String {
    let provider = match &item.identity.provider {
        radix::hir::LibraryProvider::Builtin(name) => format!("builtin:{name}"),
        radix::hir::LibraryProvider::Package(name) => format!("package:{name}"),
    };
    let module = item.identity.module_path.join(":");
    format!("{provider}:{module}:{}", item.exported_name)
}

fn io_diag(path: &Path, err: std::io::Error) -> Box<Diagnostic> {
    Box::new(Diagnostic::io_error(path, err))
}
