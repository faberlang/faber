use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use radix::diagnostics::Diagnostic;
use sha2::{Digest, Sha256};

use super::manifest::{ManifestProduct, ManifestProductKind};
use super::paths::normalize_path;

/// Generated product output path components — the single source of truth for
/// directory and file names written by the browser product build.
///
/// [`product_generated_output_paths`] consumes `FABER_TS_DIR`, `FABER_ESM_DIR`,
/// and `TSCONFIG_FILE` for collision guards, stale-output checking, and cleanup
/// registration. The build function consumes all constants. Files inside
/// directory outputs (`BROWSER_ENTRY_TS`, `WEB_AMBIENT_DTS`, `BROWSER_ENTRY_JS`)
/// are covered indirectly through their owning directory entries.
const FABER_TS_DIR: &str = "faber-ts";
const FABER_ESM_DIR: &str = "faber-esm";
const TSCONFIG_FILE: &str = "tsconfig.faber-browser.json";
const BROWSER_ENTRY_TS: &str = "faber-browser.ts";
const WEB_AMBIENT_DTS: &str = "faber-web.d.ts";
const BROWSER_ENTRY_JS: &str = "faber-browser.js";
const GENERATED_DIR: &str = "generated";
const WGSL_FILE: &str = "kernel.wgsl";
const REFLECTION_FILE: &str = "reflection.json";

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
        // Only BrowserApp exists today; future kind variants will need their
        // own dispatch here rather than falling through silently.
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
/// beyond copied static assets.
#[derive(Debug)]
struct GeneratedOutput {
    label: &'static str,
    path: PathBuf,
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
/// checking, and cleanup. Includes optional shader-generated outputs when
/// the product config specifies a shader source directory.
fn product_generated_output_paths(
    out_dir: &Path,
    product: &ManifestProduct,
) -> Vec<GeneratedOutput> {
    let mut outputs = vec![
        GeneratedOutput {
            label: "assets manifest",
            path: normalize_path(&out_dir.join(&product.assets_manifest)),
        },
        GeneratedOutput {
            label: "controllers json",
            path: normalize_path(&out_dir.join(&product.controllers_json)),
        },
        GeneratedOutput {
            label: "faber-ts directory",
            path: normalize_path(&out_dir.join(FABER_TS_DIR)),
        },
        GeneratedOutput {
            label: "faber-esm directory",
            path: normalize_path(&out_dir.join(FABER_ESM_DIR)),
        },
        GeneratedOutput {
            label: "tsconfig",
            path: normalize_path(&out_dir.join(TSCONFIG_FILE)),
        },
        GeneratedOutput {
            label: "product manifest",
            path: normalize_path(&out_dir.join("product.json")),
        },
    ];

    // When shader config is present, add generated WGSL and reflection paths
    // so collision/stale-output checking tracks them.
    if product.shaders.is_some() {
        let generated_dir = normalize_path(&out_dir.join(GENERATED_DIR));
        outputs.push(GeneratedOutput {
            label: "generated directory",
            path: generated_dir,
        });
    }

    outputs
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

/// Render the product identity manifest (`product.json`) after a successful
/// browser product build.
///
/// Records every same-build artifact: the ESM entry, the controller manifest,
/// host runtime files (`faber-kernel.js`, `webgpu-runtime.js`), shader
/// artifacts (WGSL + reflection when configured), and a reference to the
/// asset manifest (`assets.json`). Stage 2+ includes WGSL + reflection in
/// the artifacts array and omits the `next_stage_artifacts` hint.
fn render_product_json(
    out_dir: &Path,
    esm_entry: &Path,
    controllers_json: &Path,
    static_assets: &[BrowserProductAsset],
    shader_artifacts: Option<&(BrowserProductAsset, BrowserProductAsset)>,
) -> Result<String, Box<Diagnostic>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut artifacts: Vec<serde_json::Value> = Vec::new();

    // ESM entry: faber-browser.js
    let entry_path = relative_manifest_path(out_dir, esm_entry);
    let (size, sha256) = file_digest(esm_entry)?;
    artifacts.push(serde_json::json!({
        "path": entry_path,
        "kind": "esm-entry",
        "size": size,
        "sha256": sha256,
    }));

    // Controller manifest: controllers.json
    let ctrl_path = relative_manifest_path(out_dir, controllers_json);
    let (size, sha256) = file_digest(controllers_json)?;
    artifacts.push(serde_json::json!({
        "path": ctrl_path,
        "kind": "controller-manifest",
        "size": size,
        "sha256": sha256,
    }));

    // Host runtime files discovered from static assets.
    for asset in static_assets {
        let rel = relative_manifest_path(out_dir, &asset.output);
        let fname = Path::new(&rel)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let kind = match fname {
            "faber-kernel.js" => "host-runtime",
            "webgpu-runtime.js" => "host-runtime",
            _ => continue,
        };
        artifacts.push(serde_json::json!({
            "path": rel,
            "kind": kind,
            "size": asset.size,
            "sha256": asset.sha256,
        }));
    }

    // Shader artifacts (WGSL + reflection) when configured.
    let has_shaders = shader_artifacts.is_some();
    if let Some((wgsl, reflection)) = shader_artifacts {
        let wgsl_path = relative_manifest_path(out_dir, &wgsl.output);
        artifacts.push(serde_json::json!({
            "path": wgsl_path,
            "kind": "wgsl",
            "size": wgsl.size,
            "sha256": wgsl.sha256,
        }));
        let reflection_path = relative_manifest_path(out_dir, &reflection.output);
        artifacts.push(serde_json::json!({
            "path": reflection_path,
            "kind": "reflection",
            "size": reflection.size,
            "sha256": reflection.sha256,
        }));
    }

    // Build timestamp (ISO 8601).
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let build_timestamp = format_iso8601(duration);

    // Stage 2 when shader artifacts are present; Stage 1 otherwise.
    let stage: u32 = if has_shaders { 2 } else { 1 };

    let mut product = serde_json::json!({
        "version": 1,
        "stage": stage,
        "build_timestamp": build_timestamp,
        "artifacts": artifacts,
        "assets_manifest": "assets.json",
    });

    // Stage 1 includes the next_stage_artifacts hint; Stage 2 omits it
    // (shader compilation is now part of the build).
    if !has_shaders {
        product["next_stage_artifacts"] = serde_json::json!(["wgsl", "reflection"]);
    }

    serde_json::to_string_pretty(&product)
        .map(|mut json| {
            json.push('\n');
            json
        })
        .map_err(|err| {
            Box::new(product_diag(format!(
                "failed to render product.json: {err}"
            )))
        })
}

/// Compute the SHA-256 digest and size of a file.
fn file_digest(path: &Path) -> Result<(u64, String), Box<Diagnostic>> {
    let bytes = fs::read(path).map_err(|err| io_diag(path, err))?;
    let size = bytes.len() as u64;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok((size, sha256))
}

/// Format a `Duration` since Unix epoch as an ISO 8601 UTC timestamp string.
///
/// Uses the civil-date algorithm from Howard Hinnant
/// (<https://howardhinnant.github.io/date_algorithms.html>) — no external
/// datetime dependency needed.
fn format_iso8601(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let days = total_secs / 86400;
    let time_secs = total_secs % 86400;

    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Civil date from days since 1970-01-01 (Howard Hinnant algorithm).
    let z = days as i64 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y } as u32;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
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
    // Preflight (collision + stale-output containment) runs against the final
    // output directory BEFORE any staging, so a preflight error never disturbs
    // a previously published product.
    let plan = plan_browser_product_static_assets(&layout.package_root, product)?;
    let package = super::analyze_package(config, input).map_err(|diagnostics| {
        Box::new(diagnostics.into_iter().next().unwrap_or_else(|| {
            product_diag("browser product package analysis failed")
                .with_file(input.display().to_string())
                .with_arg("issue", "product_package_analysis_failed")
        }))
    })?;
    let controllers = discover_controllers(&package)?;

    // FBR-P2-005: build the complete product into a unique temporary sibling
    // of the final output directory, run every check and write against that
    // staging directory, and atomically swap the validated snapshot into the
    // final output. A failure at any stage leaves the previous product usable.
    let out_dir = plan.out_dir.clone();
    let staging_parent = out_dir.parent().ok_or_else(|| {
        Box::new(
            product_diag("browser product output has no parent directory")
                .with_arg("issue", "product_output_path_invalid"),
        )
    })?;
    let temp = unique_product_temp(staging_parent, &out_dir)?;
    let staged = (|| {
        let staged_plan = remap_static_asset_plan(plan, &temp)?;
        let static_build = write_browser_product_static_assets(staged_plan)?;
        #[cfg(test)]
        maybe_inject_product_failure(1)?;
        let ts_root = static_build.out_dir.join(FABER_TS_DIR);
        let esm_root = static_build.out_dir.join(FABER_ESM_DIR);
        fs::create_dir_all(&ts_root).map_err(|err| io_diag(&ts_root, err))?;
        fs::create_dir_all(&esm_root).map_err(|err| io_diag(&esm_root, err))?;

        let library_imports = build_library_ts_module_map(&layout.package_root)?;
        emit_typescript_modules(&package, &ts_root, &controllers, &library_imports)?;
        emit_library_typescript_modules(config, &layout.package_root, &ts_root, &library_imports)?;
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
        #[cfg(test)]
        maybe_inject_product_failure(2)?;

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
        #[cfg(test)]
        maybe_inject_product_failure(3)?;

        // Copy shader artifacts (WGSL + reflection) when configured.
        let shader_artifacts =
            copy_shader_artifacts(&layout.package_root, product, &static_build.out_dir)?;
        #[cfg(test)]
        maybe_inject_product_failure(4)?;

        // Emit product identity manifest after all build stages succeed.
        let product_json_path = static_build.out_dir.join("product.json");
        let product_json_content = render_product_json(
            &static_build.out_dir,
            &esm_entry,
            &controllers_json,
            &static_build.assets,
            shader_artifacts.as_ref(),
        )?;
        fs::write(&product_json_path, product_json_content)
            .map_err(|err| io_diag(&product_json_path, err))?;
        #[cfg(test)]
        maybe_inject_product_failure(5)?;

        // Every stage validated against the staging directory; publish now.
        publish_product_directory(&temp, &out_dir)?;
        Ok(())
    })();
    match staged {
        Ok(()) => Ok(BrowserProductBuild {
            out_dir: out_dir.clone(),
            controllers_json: out_dir.join(&product.controllers_json),
            esm_entry: out_dir.join(FABER_ESM_DIR).join(BROWSER_ENTRY_JS),
            controllers,
        }),
        Err(error) => {
            if fs::symlink_metadata(&temp).is_ok() {
                remove_product_temp(&temp)?;
            }
            Err(error)
        }
    }
}

/// Re-target a static asset plan from the final output directory onto a
/// temporary sibling with the same relative layout (FBR-P2-005).
fn remap_static_asset_plan(
    plan: StaticAssetPlan,
    temp: &Path,
) -> Result<StaticAssetPlan, Box<Diagnostic>> {
    let out_dir = plan.out_dir.clone();
    let planned = plan
        .planned
        .into_iter()
        .map(|(output, asset)| {
            let relative = output.strip_prefix(&out_dir).map_err(|_| {
                product_diag(format!(
                    "browser product asset `{}` escaped output `{}`",
                    output.display(),
                    out_dir.display()
                ))
                .with_arg("issue", "product_output_path_escape")
            })?;
            Ok((temp.join(relative), asset))
        })
        .collect::<Result<BTreeMap<_, _>, Box<Diagnostic>>>()?;
    let manifest_path = plan
        .manifest_path
        .strip_prefix(&out_dir)
        .map(|relative| temp.join(relative))
        .map_err(|_| {
            Box::new(
                product_diag("browser product asset manifest escaped output directory")
                    .with_arg("issue", "product_output_path_escape"),
            )
        })?;
    Ok(StaticAssetPlan {
        out_dir: temp.to_path_buf(),
        manifest_path,
        planned,
    })
}

/// Create a unique temporary sibling directory for the product output.
fn unique_product_temp(parent: &Path, out_dir: &Path) -> Result<PathBuf, Box<Diagnostic>> {
    let name = out_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("product");
    for attempt in 0..128_u32 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                Box::new(
                    product_diag("system clock precedes epoch")
                        .with_arg("issue", "product_temp_path_invalid"),
                )
            })?
            .as_nanos();
        let path = parent.join(format!(
            ".{name}.faber.tmp-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_diag(&path, error)),
        }
    }
    Err(Box::new(
        product_diag("could not create a unique temporary product directory")
            .with_arg("issue", "product_temp_path_invalid"),
    ))
}

/// Atomically swap a fully-built product directory into the final output path,
/// quarantining the previous product and removing it only after the swap
/// succeeds (FBR-P2-005). A failed swap restores the previous product, so the
/// last good output survives any single publish failure.
fn publish_product_directory(temp: &Path, target: &Path) -> Result<(), Box<Diagnostic>> {
    if fs::symlink_metadata(target).is_err() {
        return fs::rename(temp, target).map_err(|err| io_diag(target, err));
    }
    let quarantine = unique_product_quarantine(target)?;
    fs::rename(target, &quarantine).map_err(|err| io_diag(target, err))?;
    match fs::rename(temp, target) {
        Ok(()) => {
            ignore_io(fs::remove_dir_all(&quarantine));
            Ok(())
        }
        Err(error) => {
            let restored = fs::rename(&quarantine, target);
            ignore_io(fs::remove_dir_all(temp));
            match restored {
                Ok(()) => Err(io_diag(target, error)),
                Err(restore_error) => Err(Box::new(
                    product_diag(format!(
                        "product publish rename failed ({error}); restoring previous product failed ({restore_error})"
                    ))
                    .with_arg("issue", "product_publish_failed"),
                )),
            }
        }
    }
}

/// Pick an unused sibling name for the previous product during the swap.
fn unique_product_quarantine(target: &Path) -> Result<PathBuf, Box<Diagnostic>> {
    let parent = target.parent().ok_or_else(|| {
        Box::new(
            product_diag("browser product output has no parent directory")
                .with_arg("issue", "product_output_path_invalid"),
        )
    })?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("product");
    for attempt in 0..128_u32 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                Box::new(
                    product_diag("system clock precedes epoch")
                        .with_arg("issue", "product_output_path_invalid"),
                )
            })?
            .as_nanos();
        let path = parent.join(format!(
            ".{name}.faber.old-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        if fs::symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err(Box::new(
        product_diag("could not allocate a quarantine path for the previous product")
            .with_arg("issue", "product_output_path_invalid"),
    ))
}

/// Best-effort cleanup that must never mask the caller's result.
fn ignore_io(result: std::io::Result<()>) {
    match result {
        Ok(()) => {}
        Err(_) => {}
    }
}

fn remove_product_temp(temp: &Path) -> Result<(), Box<Diagnostic>> {
    fs::remove_dir_all(temp).map_err(|err| io_diag(temp, err))
}

/// Copy pre-compiled shader artifacts from a package source directory into
/// `dist/generated/`. Returns the paths of the copied WGSL and reflection
/// files for inclusion in the product manifest.
///
/// Since the MIR → WGSL compiler pass (Faber V/F lower) is not yet
/// implemented, this function packages reference artifacts produced by U1
/// (checked into `src/shaders/test-data/`). The `shaders.source` field in
/// `faber.toml` specifies the source directory relative to the package root.
fn copy_shader_artifacts(
    package_root: &Path,
    product: &ManifestProduct,
    out_dir: &Path,
) -> Result<Option<(BrowserProductAsset, BrowserProductAsset)>, Box<Diagnostic>> {
    let shaders = match &product.shaders {
        Some(s) => s,
        None => return Ok(None),
    };

    let source_dir = normalize_path(&package_root.join(&shaders.source));
    if !source_dir.is_dir() {
        return Err(Box::new(
            product_diag(format!(
                "shader source directory `{}` does not exist",
                source_dir.display()
            ))
            .with_arg("issue", "product_shader_source_missing"),
        ));
    }

    let wgsl_source = source_dir.join(WGSL_FILE);
    let reflection_source = source_dir.join(REFLECTION_FILE);

    if !wgsl_source.is_file() {
        return Err(Box::new(
            product_diag(format!(
                "shader WGSL source `{}` does not exist",
                wgsl_source.display()
            ))
            .with_arg("issue", "product_shader_wgsl_missing"),
        ));
    }
    if !reflection_source.is_file() {
        return Err(Box::new(
            product_diag(format!(
                "shader reflection source `{}` does not exist",
                reflection_source.display()
            ))
            .with_arg("issue", "product_shader_reflection_missing"),
        ));
    }

    let generated_dir = out_dir.join(GENERATED_DIR);
    fs::create_dir_all(&generated_dir).map_err(|err| io_diag(&generated_dir, err))?;

    let wgsl_output = generated_dir.join(WGSL_FILE);
    fs::copy(&wgsl_source, &wgsl_output).map_err(|err| io_diag(&wgsl_output, err))?;

    let reflection_output = generated_dir.join(REFLECTION_FILE);
    fs::copy(&reflection_source, &reflection_output)
        .map_err(|err| io_diag(&reflection_output, err))?;

    let wgsl_bytes = fs::read(&wgsl_output).map_err(|err| io_diag(&wgsl_output, err))?;
    let wgsl_sha256 = format!("{:x}", Sha256::digest(&wgsl_bytes));
    let wgsl_asset = BrowserProductAsset {
        kind: "wgsl",
        source: wgsl_source,
        output: wgsl_output,
        size: wgsl_bytes.len() as u64,
        sha256: wgsl_sha256,
    };

    let reflection_bytes =
        fs::read(&reflection_output).map_err(|err| io_diag(&reflection_output, err))?;
    let reflection_sha256 = format!("{:x}", Sha256::digest(&reflection_bytes));
    let reflection_asset = BrowserProductAsset {
        kind: "reflection",
        source: reflection_source,
        output: reflection_output,
        size: reflection_bytes.len() as u64,
        sha256: reflection_sha256,
    };

    Ok(Some((wgsl_asset, reflection_asset)))
}

// Test-only failure injection for the browser product build (FBR-P2-005).
// When the injected stage is non-zero, the build fails once the current stage
// reaches that value. Never compiled into production binaries.
#[cfg(test)]
thread_local! {
    static PRODUCT_STAGE_FAILURE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn inject_product_failure_at(stage: u8) {
    PRODUCT_STAGE_FAILURE.with(|cell| cell.set(stage));
}

#[cfg(test)]
fn maybe_inject_product_failure(stage: u8) -> Result<(), Box<Diagnostic>> {
    PRODUCT_STAGE_FAILURE.with(|target| {
        if target.get() != 0 && stage >= target.get() {
            Err(Box::new(
                product_diag("injected browser product failure")
                    .with_arg("issue", "product_injected_failure"),
            ))
        } else {
            Ok(())
        }
    })
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
    library_imports: &BTreeMap<String, String>,
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
        let local_names = top_level_ts_decl_names(unit);
        let code = adapt_controller_typescript(code, controllers);
        let code = augment_namespace_imports(code, &unit.namespace_exports, &local_names);
        let module_name = unit
            .module_segments
            .last()
            .map(|s| s.as_str())
            .unwrap_or("main");
        let code = wrap_module_exports(code, module_name);
        let code = rewrite_import_specifiers(code, library_imports);
        let path = ts_root.join(ts_module_file_name(unit));
        fs::write(&path, code).map_err(|err| io_diag(&path, err))?;
    }
    Ok(())
}

/// Emit TypeScript for library dependencies (kind=lib, target=ts) into the
/// TypeScript output directory alongside app modules.
///
/// Reads `faber.lock` to discover TS-targeting library packages, then
/// emits each `.fab` source file via `faber emit -t ts` (same approach
/// as `link-triga-ts.mjs`). Emitted files are named `{package}-{module}.ts`
/// and get namespace-wrapped exports the same way app modules do.
///
/// Phase 2: minimal emit — no import rewriting, no tsc passthrough.
/// Radix codegen defects are handled in Phase 3.
/// Apply emit-defect post-processing for library TypeScript files.
///
/// Phase 3: these are minimal patches for codegen defects that would otherwise
/// block TypeScript compilation. Each patch corresponds to a filed radix issue
/// so the root cause is tracked in the compiler, not papered over indefinitely.
fn apply_library_emit_fixes(mut code: String) -> String {
    // Fix 1: construction of unresolved types → empty object (value position).
    // Handle both bare and marker forms before any type-position rewrite.
    code = code.replace("new unresolved_def()", "{}");
    code = code.replace("new /* unresolved_def */()", "{}");

    // Fix 2: Codegen marker `/* unresolved_def */` is not a valid type position
    // (`/* any */` after a naive substring replace is still invalid). Use `any`.
    code = code.replace("/* unresolved_def */", "any");

    // Fix 3: bare `unresolved_def` identifiers still need rewriting.
    code = code.replace("unresolved_def", "any");

    // Fix 4: if a value-position rewrite was missed, `new any()` is still illegal.
    code = code.replace("new any()", "{}");

    // Radix codegen emits IIFE array-access expressions as the left-hand side
    // of assignment, which TypeScript rejects (TS2364).  Rewrite:
    //   ((__o, __i) => { ... })(arr, idx) = value;
    // → arr[idx] = value;
    //
    // The IIFE pattern (index trap form; keeps a stored `undefined` distinct
    // from a missing index via the `in` check):
    //   ((__o, __i) => { if (!(__i in __o)) throw new Error("index trap"); return __o[__i]; })(arr, idx)
    // We replace it with a plain arr[idx] when it appears as LHS of `=`.
    let iife_pattern = r#"((__o, __i) => { if (!(__i in __o)) throw new Error("index trap"); return __o[__i]; })("#;
    // Process line by line to handle IIFE-on-LHS.
    let mut result = String::with_capacity(code.len() + 4096);
    for line in code.lines() {
        let trimmed = line.trim();
        // Detect IIFE-as-LHS: line matches pattern `((__o, ...)(expr, idx) = ...`
        if trimmed.starts_with(iife_pattern) && trimmed.contains(") = ") {
            // Extract array expression and index from IIFE call.
            // Pattern: ((__o, __i) => { ... })(array_expr, index_expr) = value_expr
            let rest = &trimmed[iife_pattern.len()..];
            // Find the closing paren of the IIFE call.
            if let Some(close_idx) = rest.rfind(") = ") {
                let args_part = &rest[..close_idx];
                // Split at the comma to get array and index
                if let Some(comma_idx) = args_part.rfind(',') {
                    let array_expr = args_part[..comma_idx].trim();
                    let index_expr = args_part[comma_idx + 1..].trim();
                    let value_part = rest[close_idx + 4..].trim(); // after ") = "
                                                                   // Determine indentation from original line
                    let indent = &line[..line.len() - line.trim_start().len()];
                    result.push_str(&format!(
                        "{}{}[{}] = {};\n",
                        indent,
                        array_expr,
                        index_expr,
                        value_part.trim_end_matches(';').trim()
                    ));
                    continue;
                }
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

/// Emit TypeScript for library dependencies (kind=lib, target=ts) into the
/// TypeScript output directory alongside app modules.
///
/// Reads `faber.lock` to discover TS-targeting library packages, then
/// emits each `.fab` source file via `faber emit -t ts` (same approach
/// as `link-triga-ts.mjs`). Emitted files are named `{package}-{module}.ts`
/// and get namespace-wrapped exports the same way app modules do.
///
/// Phase 2: minimal emit — no import rewriting, no tsc passthrough.
/// Phase 3: post-processing fixes for known codegen defects (unresolved_def,
///          IIFE-as-LHS) applied via [`apply_library_emit_fixes`].
/// Resolve the faber CLI binary for subprocess `emit`.
///
/// Unit tests run under a cargo test harness binary (`deps/faber-<hash>`), so
/// `current_exe()` is not a usable CLI. Prefer `CARGO_BIN_EXE_faber`, then a
/// sibling `faber` next to `deps/`, then `current_exe` when it *is* the CLI.
fn resolve_faber_cli_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_faber") {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(name) = exe.file_name().and_then(|n| n.to_str()) {
            if name == "faber" || name == "faber.exe" {
                return exe;
            }
        }
        if let Some(parent) = exe.parent() {
            // .../debug/deps/faber-<hash> → .../debug/faber
            if parent.file_name().and_then(|n| n.to_str()) == Some("deps") {
                if let Some(debug_dir) = parent.parent() {
                    let candidate = debug_dir.join("faber");
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
            let sibling = parent.join("faber");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("faber")
}

fn emit_library_typescript_modules(
    _config: &radix::driver::Config,
    package_root: &Path,
    ts_root: &Path,
    library_imports: &BTreeMap<String, String>,
) -> Result<(), Box<Diagnostic>> {
    let lock = match super::lockfile::read_lock(package_root)? {
        Some(lock) => lock,
        None => return Ok(()),
    };
    let lock_path = package_root.join(super::lockfile::LOCK_FILE);
    let index = super::lockfile::lock_index(&lock_path, &lock).map_err(|mut diags| {
        diags
            .pop()
            .unwrap_or_else(|| product_diag("failed to index faber.lock for library TS emit"))
    })?;

    // Locate the faber binary for subprocess emit.
    // Prefer the real CLI binary (not the cargo test harness binary).
    let faber_bin = resolve_faber_cli_binary();

    // Pass 1: emit all library TS files into memory.
    // We need all files emitted before augmenting imports so cross-module
    // type references can be resolved.
    struct EmittedLibFile {
        ts_path: PathBuf,
        stem: String,
        code: String,
    }
    let mut emitted: Vec<EmittedLibFile> = Vec::new();

    for pkg in index.values() {
        if pkg.kind != "lib" || pkg.target_language != "ts" {
            continue;
        }
        let pkg_root = pkg.package_root_path(package_root);
        let src_dir = pkg_root.join("src");
        if !src_dir.is_dir() {
            continue;
        }
        // FBR-P2-001: directory read errors anywhere in the tree are
        // diagnostic failures, never silently dropped modules.
        let src_files = library_src_fab_files(&src_dir)?;

        // Prefer package-owned TS binding shims (e.g. faber-web/runtime/dom.ts)
        // over emitting `nota` stubs from .fab — stubs only console.log.
        let ts_bindings = load_ts_library_bindings(&pkg_root)?;

        for fab_path in &src_files {
            let Some(naming) = ts_lib_module_naming(&pkg.name, &src_dir, fab_path) else {
                continue;
            };
            let ts_path = ts_root.join(&naming.file_name);

            if let Some(bindings) = ts_bindings.as_ref() {
                if let Some(exports) = bindings.module_exports(&naming.leaf_stem) {
                    if !exports.is_empty() {
                        emit_ts_binding_facade(
                            &pkg_root,
                            &pkg.name,
                            &naming.leaf_stem,
                            &naming.file_name,
                            bindings,
                            &exports,
                            ts_root,
                        )?;
                        continue;
                    }
                }
            }

            let output = std::process::Command::new(&faber_bin)
                .args(["emit", "-t", "ts"])
                .arg(&fab_path)
                .output();
            let output = match output {
                Ok(output) => output,
                Err(err) => {
                    eprintln!(
                        "faber: library TS emit subprocess failed for `{}`: {err}",
                        fab_path.display()
                    );
                    continue;
                }
            };
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!(
                    "faber: library TS emit failed for `{}`: {stderr}",
                    fab_path.display()
                );
                continue;
            }
            let code = String::from_utf8_lossy(&output.stdout).to_string();
            emitted.push(EmittedLibFile {
                ts_path,
                stem: naming.leaf_stem,
                code,
            });
        }
    }

    // Pass 2: build namespace export map from emitted files.
    // Each library module's raw declarations become its namespace exports.
    // Other files that import that module namespace need the individual type
    // names added to the import statement.
    let mut namespace_exports: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in &emitted {
        let exports = collect_ts_bare_decl_names(&file.code);
        namespace_exports
            .entry(file.stem.clone())
            .or_default()
            .extend(exports);
    }

    // Pass 3: write each file with import augmentation applied.
    for file in &emitted {
        // Phase 3: apply emit-defect fixes before namespace wrapping.
        let code = apply_library_emit_fixes(file.code.clone());
        let code = wrap_module_exports(code, &file.stem);
        // Emitted modules export their leaf stem as the namespace const; a
        // `privata` binding that differs (e.g. `import { lighting } from
        // "triga:lighting/light"`) must alias the leaf stem or tsc fails
        // (TS2305).
        let code = normalize_library_namespace_bindings(code, library_imports);
        // Augment namespace imports with named type exports so TypeScript
        // can resolve cross-module type references (e.g. Vector3 from math).
        let local_names = collect_ts_local_decl_names(&code);
        let code = augment_namespace_imports(code, &namespace_exports, &local_names);
        // Phase 4: one-pass rewrite of import/export specifier tokens.
        let code = rewrite_import_specifiers(code, library_imports);
        fs::write(&file.ts_path, code).map_err(|err| io_diag(&file.ts_path, err))?;
    }

    Ok(())
}

/// Collect top-level declaration names from raw TS codegen output.
/// Only matches declarations at indent level 0 (no leading whitespace).
/// The subprocess emit does not add `export` keywords — those are added
/// later by `wrap_module_exports`.
fn collect_ts_bare_decl_names(code: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in code.lines() {
        // Only top-level declarations (no leading whitespace).
        if line.chars().next().is_some_and(|c| c.is_whitespace()) {
            continue;
        }
        for prefix in &[
            "class ",
            "function ",
            "enum ",
            "interface ",
            "type ",
            "const ",
        ] {
            if let Some(rest) = line.strip_prefix(prefix) {
                let name = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    names.push(name.to_owned());
                }
            }
        }
    }
    names
}

/// Collect top-level declaration names from emitted TS code.
fn collect_ts_local_decl_names(code: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in code.lines() {
        let trimmed = line.trim_start();
        for prefix in &[
            "export class ",
            "export function ",
            "export enum ",
            "export interface ",
            "export type ",
            "export const ",
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let name = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    names.insert(name.to_owned());
                }
            }
        }
    }
    names
}

/// Parsed `[target.ts]` binding manifest for a TS library package.
struct TsLibraryBindings {
    /// Absolute path to the package-owned runtime shim (e.g. `runtime/dom.ts`).
    shim_path: PathBuf,
    /// Keyed by function route: `"web:dom.attr_set"` → `"webDomAttrSet"`.
    functions: BTreeMap<String, String>,
}

impl TsLibraryBindings {
    /// Faber API names and host symbols for one module stem (`dom` → attr_set…).
    fn module_exports(&self, stem: &str) -> Option<Vec<(String, String)>> {
        let mut exports = Vec::new();
        let needle = format!(":{stem}.");
        for (route, symbol) in &self.functions {
            // route = "{provider}:{module}.{fn}" e.g. web:dom.attr_set
            if let Some(idx) = route.find(&needle) {
                // Ensure module boundary: character before ':' segment is provider end.
                // Accept any provider prefix that ends with `:{stem}.`.
                let after = &route[idx + needle.len()..];
                if !after.is_empty() && !after.contains('.') && !after.contains(':') {
                    exports.push((after.to_owned(), symbol.clone()));
                }
            }
        }
        if exports.is_empty() {
            None
        } else {
            exports.sort_by(|a, b| a.0.cmp(&b.0));
            Some(exports)
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct TsBindingFile {
    #[serde(default)]
    functions: BTreeMap<String, TsBindingFn>,
    #[serde(default)]
    shim: Option<TsBindingShim>,
}

#[derive(Debug, serde::Deserialize)]
struct TsBindingFn {
    symbol: String,
}

#[derive(Debug, serde::Deserialize)]
struct TsBindingShim {
    path: String,
}

/// Load optional TS binding shim config from a library package's `faber.toml`.
fn load_ts_library_bindings(pkg_root: &Path) -> Result<Option<TsLibraryBindings>, Box<Diagnostic>> {
    let manifest_path = pkg_root.join(super::MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let manifest = match super::manifest::read_manifest(&manifest_path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    let Some(target) = manifest.target.get("ts") else {
        return Ok(None);
    };
    let Some(bindings_rel) = target.bindings.as_deref() else {
        return Ok(None);
    };
    let binding_path = match super::resolve_package_member(pkg_root, bindings_rel, &manifest_path) {
        Ok(p) => p,
        Err(err) => return Err(Box::new(err)),
    };
    let source = fs::read_to_string(&binding_path).map_err(|err| io_diag(&binding_path, err))?;
    let file: TsBindingFile = toml::from_str(&source).map_err(|err| {
        Box::new(
            product_diag(format!(
                "invalid TS binding manifest `{}`: {err}",
                binding_path.display()
            ))
            .with_file(binding_path.display().to_string())
            .with_arg("issue", "product_ts_binding_manifest_invalid"),
        )
    })?;
    let Some(shim) = file.shim else {
        return Ok(None);
    };
    if shim.path.trim().is_empty() {
        return Ok(None);
    }
    let shim_path = match super::resolve_package_member(pkg_root, &shim.path, &binding_path) {
        Ok(p) => p,
        Err(err) => return Err(Box::new(err)),
    };
    if !shim_path.is_file() {
        return Err(Box::new(
            product_diag(format!("TS binding shim missing: {}", shim_path.display()))
                .with_file(binding_path.display().to_string())
                .with_arg("issue", "product_ts_binding_shim_missing"),
        ));
    }
    let functions: BTreeMap<String, String> = file
        .functions
        .into_iter()
        .map(|(k, v)| (k, v.symbol))
        .collect();
    Ok(Some(TsLibraryBindings {
        shim_path,
        functions,
    }))
}

/// Copy the package TS runtime shim into the product tree and write a facade
/// that re-exports host symbols under Faber API names (`attr_set`, …).
fn emit_ts_binding_facade(
    _pkg_root: &Path,
    pkg_name: &str,
    stem: &str,
    file_name: &str,
    bindings: &TsLibraryBindings,
    exports: &[(String, String)],
    ts_root: &Path,
) -> Result<(), Box<Diagnostic>> {
    let shim_stem = bindings
        .shim_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("runtime");
    let runtime_name = format!("{pkg_name}-shim-{shim_stem}.ts");
    let runtime_path = ts_root.join(&runtime_name);
    // Copy once (idempotent if several modules share the same shim).
    if !runtime_path.is_file() {
        fs::copy(&bindings.shim_path, &runtime_path).map_err(|err| io_diag(&runtime_path, err))?;
    }

    let runtime_import = format!("./{pkg_name}-shim-{shim_stem}.js");
    let mut code =
        String::from("// Generated by faber product packaging — TS binding shim facade.\n");
    code.push_str("import {\n");
    for (api_name, symbol) in exports {
        code.push_str(&format!("  {symbol} as {api_name},\n"));
    }
    code.push_str(&format!("}} from {runtime_import:?};\n\n"));
    if stem == "dom" {
        code.push_str("import type {\n");
        for (_, runtime_name) in DOM_TYPE_ALIASES {
            code.push_str(&format!("  {runtime_name},\n"));
        }
        code.push_str(&format!("}} from {runtime_import:?};\n\n"));
    }

    code.push_str("export {\n");
    for (api_name, _) in exports {
        code.push_str(&format!("  {api_name},\n"));
    }
    code.push_str("};\n\n");

    // Namespace object matches wrap_module_exports shape: `import { dom } from …`.
    code.push_str(&format!("export const {stem} = {{\n"));
    for (i, (api_name, _)) in exports.iter().enumerate() {
        let comma = if i + 1 < exports.len() { "," } else { "" };
        code.push_str(&format!("  {api_name}{comma}\n"));
    }
    code.push_str("};\n");
    if stem == "dom" {
        code.push('\n');
        for (api_name, runtime_name) in DOM_TYPE_ALIASES {
            code.push_str(&format!("export type {api_name} = {runtime_name};\n"));
        }
    }

    let facade_path = ts_root.join(file_name);
    fs::write(&facade_path, code).map_err(|err| io_diag(&facade_path, err))?;
    Ok(())
}

/// Recursively enumerate `.fab` files under a library `src/` directory.
///
/// FBR-P2-001: a directory read error anywhere in the tree is a diagnostic
/// failure, never a silently dropped module. Reuses the package discovery
/// walk in [`super::source_files::package_source_files`] (BFS, symlink-escape
/// guarded) with `include_proba = false`.
fn library_src_fab_files(src_dir: &Path) -> Result<Vec<PathBuf>, Box<Diagnostic>> {
    super::source_files::package_source_files(src_dir, false).map_err(|mut diags| {
        Box::new(diags.pop().unwrap_or_else(|| {
            product_diag(format!(
                "failed to read library source root {}",
                src_dir.display()
            ))
            .with_file(src_dir.display().to_string())
        }))
    })
}

/// TypeScript product naming derived from one library `.fab` source file.
struct TsLibModuleNaming {
    /// Library import specifier, e.g. `triga:lighting/light`.
    spec: String,
    /// Emitted file name (relative to the TS output root), e.g.
    /// `triga-lighting-light.ts`.
    file_name: String,
    /// Relative ESM path used for import rewrites, e.g.
    /// `./triga-lighting-light.js`.
    rel_path: String,
    /// Leaf module stem — the namespace-export name importers bind, e.g.
    /// `light` for `src/lighting/light.fab`.
    leaf_stem: String,
}

/// Derive TypeScript product naming for a library `.fab` source file.
///
/// Top-level modules keep their historical flat names (`src/math.fab` →
/// `triga:math` / `triga-math.ts` / `./triga-math.js`). Nested leaves always
/// use the full relative path so a nested leaf can never collide with a
/// top-level module that shares its stem (`src/lighting/light.fab` →
/// `triga:lighting/light` / `triga-lighting-light.ts` / `./triga-lighting-light.js`).
/// The `-` delimiter is shared between the emitted `.ts` file names and the
/// `.js` map values so rewritten specifiers resolve against emitted files.
fn ts_lib_module_naming(
    pkg_name: &str,
    src_dir: &Path,
    fab_path: &Path,
) -> Option<TsLibModuleNaming> {
    let rel = fab_path.strip_prefix(src_dir).ok()?;
    let segments: Vec<String> = rel
        .with_extension("")
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(segment) => {
                Some(segment.to_string_lossy().into_owned())
            }
            _ => None,
        })
        .collect();
    if segments.is_empty() {
        return None;
    }
    let rel_str = segments.join("/");
    let dash_name = segments.join("-");
    Some(TsLibModuleNaming {
        spec: format!("{pkg_name}:{rel_str}"),
        file_name: format!("{pkg_name}-{dash_name}.ts"),
        rel_path: format!("./{pkg_name}-{dash_name}.js"),
        leaf_stem: segments.last().cloned().unwrap_or_default(),
    })
}

/// Build a map from library import specifiers to relative ESM paths for all
/// `kind=lib`, `target_language=ts` packages discovered via the lockfile.
///
/// Returns `{specifier} → {relative_path}` mappings, e.g.:
///   "triga:triga" → "./triga-triga.js"
///   "triga:geometry" → "./triga-geometry.js"
///   "triga:lighting/light" → "./triga-lighting-light.js"
///
/// Nested leaves are enumerated recursively and keyed by their full relative
/// module path; top-level modules keep their historical flat names.
///
/// Phase 4: used by [`rewrite_import_specifiers`] to rewrite bare library
/// specifiers in emitted TypeScript to relative ESM paths.
fn build_library_ts_module_map(
    package_root: &Path,
) -> Result<BTreeMap<String, String>, Box<Diagnostic>> {
    let lock = match super::lockfile::read_lock(package_root)? {
        Some(lock) => lock,
        None => return Ok(BTreeMap::new()),
    };
    let lock_path = package_root.join(super::lockfile::LOCK_FILE);
    let index = super::lockfile::lock_index(&lock_path, &lock).map_err(|mut diags| {
        diags.pop().unwrap_or_else(|| {
            product_diag("failed to index faber.lock for library import map")
                .with_file(package_root.display().to_string())
                .with_arg("issue", "product_library_import_map_failed")
        })
    })?;

    let mut map = BTreeMap::new();
    for pkg in index.values() {
        if pkg.kind != "lib" || pkg.target_language != "ts" {
            continue;
        }
        let pkg_root = pkg.package_root_path(package_root);
        let src_dir = pkg_root.join("src");
        if !src_dir.is_dir() {
            continue;
        }
        // FBR-P2-001: per-entry directory read errors are diagnostic failures,
        // never silently dropped modules.
        let src_files = library_src_fab_files(&src_dir)?;
        let mut emitted_file_names: BTreeMap<String, String> = BTreeMap::new();

        for fab_path in &src_files {
            let Some(naming) = ts_lib_module_naming(&pkg.name, &src_dir, fab_path) else {
                continue;
            };
            // Fail closed on emitted-file name collisions (FBR-P2-001): two
            // modules mapping to one output file would silently drop a module
            // and corrupt the import map.
            if let Some(existing_spec) = emitted_file_names.get(&naming.file_name) {
                return Err(Box::new(
                    product_diag(format!(
                        "library TS modules `{existing_spec}` and `{}` both map to `{}`",
                        naming.spec, naming.file_name
                    ))
                    .with_file(fab_path.display().to_string())
                    .with_arg("issue", "product_library_ts_module_name_collision"),
                ));
            }
            emitted_file_names.insert(naming.file_name.clone(), naming.spec.clone());
            map.insert(naming.spec, naming.rel_path);
        }
    }
    Ok(map)
}

/// One-pass rewrite of import/export specifier tokens in emitted TypeScript.
///
/// Merges the former two-pass `rewrite_library_imports` +
/// `rewrite_relative_import_extensions` into a single byte scan
/// (FBR-P2-008):
///
/// - Library specifiers (`from "name:stem"`) are replaced through exact map
///   membership (`library_imports` from [`build_library_ts_module_map`]) — one
///   pass, no per-specifier whole-file `String::replace` loop.
/// - Relative specifiers (`./` / `../`) get a `.js` extension appended when
///   they do not already carry a known extension.
/// - Comments, string literals, and template literals are skipped, so
///   specifier-looking text inside them is never rewritten.
/// - Only `from "<spec>"` / `from '<spec>'` tokens are rewritten, and only
///   when `from` is a standalone token (exact-token contract). Dynamic
///   `import("...")` calls are never touched; `radix-hir-ts` emits only
///   static `import { ... } from "..."` declarations, pinned by test.
fn rewrite_import_specifiers(code: String, library_imports: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(code.len() + 32);
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        // Line comments are copied verbatim; their text is never a specifier.
        if rest.starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(bytes[i] as char);
                i += 1;
            }
            continue;
        }
        // Block comments are copied verbatim.
        if rest.starts_with(b"/*") {
            out.push_str("/*");
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                out.push(bytes[i] as char);
                i += 1;
            }
            if i + 1 < bytes.len() {
                out.push_str("*/");
                i += 2;
            }
            continue;
        }
        // Import/export specifier token: `from "` / `from '`. The `from` must
        // be a standalone token (not part of a longer identifier) and the
        // quote pair encloses the whole specifier string.
        if rest.starts_with(b"from ")
            && i + 6 <= bytes.len()
            && (bytes[i + 5] == b'"' || bytes[i + 5] == b'\'')
            && (i == 0 || !is_ts_ident_byte(bytes[i - 1]))
        {
            let quote = bytes[i + 5];
            out.push_str("from ");
            out.push(quote as char);
            i += 6;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let spec = &code[start..i];
            match library_imports.get(spec) {
                Some(rel_path) => out.push_str(rel_path),
                None => {
                    out.push_str(spec);
                    if spec.starts_with("./") || spec.starts_with("../") {
                        let needs_js = !spec.ends_with(".js")
                            && !spec.ends_with(".json")
                            && !spec.ends_with(".ts")
                            && !spec.ends_with(".mjs")
                            && !spec.ends_with(".cjs");
                        if needs_js {
                            out.push_str(".js");
                        }
                    }
                }
            }
            if i < bytes.len() {
                out.push(quote as char);
                i += 1;
            }
            continue;
        }
        // String literals are copied verbatim (backslash escapes respected).
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            out.push(quote as char);
            i += 1;
            while i < bytes.len() {
                let current = bytes[i];
                out.push(current as char);
                i += 1;
                if current == b'\\' {
                    if i < bytes.len() {
                        out.push(bytes[i] as char);
                        i += 1;
                    }
                } else if current == quote {
                    break;
                }
            }
            continue;
        }
        // Template literals are copied verbatim (backslash escapes respected).
        if bytes[i] == b'`' {
            out.push('`');
            i += 1;
            while i < bytes.len() {
                let current = bytes[i];
                out.push(current as char);
                i += 1;
                if current == b'\\' {
                    if i < bytes.len() {
                        out.push(bytes[i] as char);
                        i += 1;
                    }
                } else if current == b'`' {
                    break;
                }
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Rewrite namespace-import bindings to resolve against the emitted target
/// module's namespace export (its leaf stem).
///
/// The radix TS emitter spells a module import's binding name verbatim from
/// the `importa … privata <name>` alias (`import { lighting } from
/// "triga:lighting/light"`), but emitted library modules export their leaf
/// stem as the namespace const (`export const light = { … }`). A binding that
/// does not match the target leaf stem fails `tsc` (TS2305); emit the alias
/// form `import { light as lighting }` instead, matching what codegen already
/// produces for `privata <stem> ut <name>`. Importers that bind the leaf stem
/// (the norm) are untouched.
///
/// Only single-binding module imports are namespace bindings; augmented type
/// names join later, after this pass.
fn normalize_library_namespace_bindings(
    code: String,
    library_imports: &BTreeMap<String, String>,
) -> String {
    let mut out = String::with_capacity(code.len() + 64);
    for line in code.lines() {
        let trimmed = line.trim_start();
        let rewritten = if let Some(rest) = trimmed.strip_prefix("import { ") {
            if let Some((binding, from_part)) = rest.split_once(" } from ") {
                let binding = binding.trim();
                if !binding.is_empty() && !binding.contains(',') && !binding.contains(" as ") {
                    let spec = import_clause_specifier(from_part);
                    let spec = spec.and_then(|spec| library_imports.get_key_value(spec));
                    if let Some((spec, _rel)) = spec {
                        // Leaf module stem: last segment after both the
                        // provider `:` and any nested `/` (`triga:math` →
                        // `math`; `triga:lighting/light` → `light`).
                        let leaf = spec.rsplit('/').next().and_then(|s| s.rsplit(':').next());
                        if let Some(leaf) = leaf {
                            if leaf != binding {
                                let indent = &line[..line.len() - trimmed.len()];
                                Some(format!(
                                    "{indent}import {{ {leaf} as {binding} }} from {from_part}"
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        match rewritten {
            Some(rewritten) => {
                out.push_str(&rewritten);
                out.push('\n');
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Extract the specifier string from an import clause's `from "<spec>";`
/// remainder (which may carry a trailing `;` and any surrounding trivia).
fn import_clause_specifier(from_part: &str) -> Option<&str> {
    let from = from_part.trim();
    let bytes = from.as_bytes();
    if bytes.first().map_or(true, |first| *first != b'"' && *first != b'\'') {
        return None;
    }
    let quote = char::from(bytes[0]);
    let after = &from[1..];
    let end = after.find(quote)?;
    Some(&after[..end])
}

/// Byte-level TypeScript identifier character check used for the exact
/// `from` token boundary in [`rewrite_import_specifiers`] (FBR-P2-008).
fn is_ts_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

const DOM_TYPE_ALIASES: &[(&str, &str)] = &[
    ("Scope", "WebDomScope"),
    ("Element", "WebDomElement"),
    ("DomEvent", "WebDomEvent"),
    ("FrameState", "WebDomFrameState"),
    ("ResizeState", "WebDomResizeState"),
    ("KeyboardState", "WebDomKeyboardState"),
    ("PointerState", "WebDomPointerState"),
    ("FocusState", "WebDomFocusState"),
    ("PointerLockState", "WebDomPointerLockState"),
    ("Subscription", "WebDomSubscription"),
    ("SubmitOptions", "WebDomSubmitOptions"),
    ("FetchRequest", "WebDomFetchRequest"),
    ("FetchResponse", "WebDomFetchResponse"),
    ("EventHandler", "WebDomEventHandler"),
    ("InputHandler", "WebDomInputHandler"),
    ("SubmitHandler", "WebDomSubmitHandler"),
    ("FrameHandler", "WebDomFrameHandler"),
    ("ResizeHandler", "WebDomResizeHandler"),
    ("KeyboardHandler", "WebDomKeyboardHandler"),
    ("PointerHandler", "WebDomPointerHandler"),
    ("FocusHandler", "WebDomFocusHandler"),
    ("PointerLockHandler", "WebDomPointerLockHandler"),
];

fn top_level_ts_decl_names(unit: &super::AnalyzedPackageUnit) -> BTreeSet<String> {
    unit.analysis
        .hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            radix::hir::HirItemKind::Function(function) => {
                Some(unit.analysis.interner.resolve(function.name).to_owned())
            }
            radix::hir::HirItemKind::Struct(strukt) => {
                Some(unit.analysis.interner.resolve(strukt.name).to_owned())
            }
            radix::hir::HirItemKind::Enum(enm) => {
                Some(unit.analysis.interner.resolve(enm.name).to_owned())
            }
            radix::hir::HirItemKind::Interface(interface) => {
                Some(unit.analysis.interner.resolve(interface.name).to_owned())
            }
            radix::hir::HirItemKind::TypeAlias(alias) => {
                Some(unit.analysis.interner.resolve(alias.name).to_owned())
            }
            radix::hir::HirItemKind::Constant(konst) => {
                Some(unit.analysis.interner.resolve(konst.name).to_owned())
            }
            radix::hir::HirItemKind::Import(_) => None,
        })
        .collect()
}

fn augment_namespace_imports(
    code: String,
    namespace_exports: &BTreeMap<String, Vec<String>>,
    local_names: &BTreeSet<String>,
) -> String {
    let mut output = String::with_capacity(code.len() + 256);
    let mut reserved_names = local_names.clone();
    for imported in all_named_imports(&code) {
        reserved_names.insert(imported);
    }
    for line in code.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let Some(rest) = trimmed.strip_prefix("import { ") else {
            output.push_str(line);
            output.push('\n');
            continue;
        };
        let Some((imports_part, from_part)) = rest.split_once(" } from ") else {
            output.push_str(line);
            output.push('\n');
            continue;
        };
        let mut imports = imports_part
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let existing = imports.iter().cloned().collect::<BTreeSet<_>>();
        let mut type_imports = Vec::new();
        let mut changed = false;

        for binding in existing.iter() {
            let Some(exports) = namespace_exports.get(binding) else {
                continue;
            };
            if binding == "dom" {
                for export in exports {
                    if is_dom_type_export(export)
                        && is_ts_identifier(export)
                        && bare_import_needed(&code, export)
                        && !reserved_names.contains(export)
                    {
                        type_imports.push(export.clone());
                        reserved_names.insert(export.clone());
                    }
                }
                continue;
            }
            for export in exports {
                if export == binding
                    || !is_ts_identifier(export)
                    || !bare_import_needed(&code, export)
                    || reserved_names.contains(export)
                {
                    continue;
                }
                imports.push(export.clone());
                reserved_names.insert(export.clone());
                changed = true;
            }
        }

        if changed {
            imports.sort();
            output.push_str(indent);
            output.push_str("import { ");
            output.push_str(&imports.join(", "));
            output.push_str(" } from ");
            output.push_str(from_part);
            output.push('\n');
        } else {
            output.push_str(line);
            output.push('\n');
        }

        type_imports.sort();
        type_imports.dedup();
        type_imports.retain(|name| !existing.contains(name));
        if !type_imports.is_empty() {
            output.push_str(indent);
            output.push_str("import type { ");
            output.push_str(&type_imports.join(", "));
            output.push_str(" } from ");
            output.push_str(from_part);
            output.push('\n');
        }
    }
    output
}

fn all_named_imports(code: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in code.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("import { ") else {
            continue;
        };
        let Some((imports_part, _)) = rest.split_once(" } from ") else {
            continue;
        };
        for item in imports_part
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let imported = item
                .split_once(" as ")
                .map_or(item, |(_, alias)| alias)
                .trim();
            if is_ts_identifier(imported) {
                names.push(imported.to_owned());
            }
        }
    }
    names
}

fn bare_import_needed(code: &str, name: &str) -> bool {
    [
        format!(": {name}"),
        format!("!: {name}"),
        format!("<{name}>"),
        format!("| {name}"),
        format!("new {name}("),
        format!("new {name}()"),
        format!("new {name},"),
        format!("as {name}"),
    ]
    .iter()
    .any(|needle| code.contains(needle))
}

fn is_dom_type_export(name: &str) -> bool {
    DOM_TYPE_ALIASES
        .iter()
        .any(|(api_name, _)| *api_name == name)
}

fn is_ts_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
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

    // Struct construction of unresolved types → empty object (value position).
    code = code.replace("new unresolved_def()", "{}");
    code = code.replace("new /* unresolved_def */()", "{}");
    code = rewrite_dom_type_constructors(code);
    // Codegen marker is invalid in type position; replace whole marker first.
    code = code.replace("/* unresolved_def */", "any");
    // Arrow-function closures with explicit `: void` return annotations reject
    // bodies that return a Promise (async handler).  Drop the annotation so
    // TypeScript infers the return type; assignment to a `void`-typed handler
    // parameter still accepts any return value.
    code = code.replace("): void =>", ") =>");
    code = code.replace("unresolved_def", "any");
    code = code.replace("new any()", "{}");
    code
}

fn rewrite_dom_type_constructors(mut code: String) -> String {
    for (api_name, runtime_name) in DOM_TYPE_ALIASES {
        for name in [*api_name, *runtime_name] {
            code = code.replace(&format!("new {name}()"), "{}");
            code = code.replace(&format!("new {name}("), "(");
        }
    }
    code
}

/// Make a generated TypeScript module valid for `tsc` by adding `export` to
/// every top-level function and class declaration, and appending a namespace
/// export object that enables `import { name } from "./file"` to resolve.
///
/// Radix's TS codegen emits module-scoped declarations without `export` and
/// generates `import { moduleName } from "./module"` in consumer files.  This
/// function bridges the gap until codegen adds native module support.  The
/// post-build `link-triga-ts.mjs` script also adds namespace exports via
/// `wrapNamespace` and skips re-adding when `export const <name> = {` exists.
fn wrap_module_exports(mut code: String, module_name: &str) -> String {
    // Export every top-level declaration. The namespace export object only
    // carries value members; pure types (`type`, `interface`) cannot be
    // object properties.
    let mut export_names: Vec<String> = Vec::new();

    // Process line by line to find top-level declarations (indent level 0).
    let mut lines: Vec<String> = code.lines().map(|l| l.to_owned()).collect();
    for line in &mut lines {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim();
        for prefix in [
            "function ",
            "class ",
            "enum ",
            "interface ",
            "type ",
            "const ",
        ] {
            let Some(rest) = trimmed.strip_prefix(prefix) else {
                continue;
            };
            let name = rest
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                continue;
            }
            if prefix != "type " && prefix != "interface " {
                export_names.push(name.to_owned());
            }
            *line = format!("export {}", trimmed);
            break;
        }
    }
    code = lines.join("\n");

    // Build the namespace export for this module (if not already present).
    let ns_marker = format!("export const {} = {{", module_name);
    if !export_names.is_empty() && !code.contains(&ns_marker) {
        code.push_str(&format!("\nexport const {} = {{\n", module_name));
        for (i, name) in export_names.iter().enumerate() {
            let comma = if i < export_names.len() - 1 { "," } else { "" };
            code.push_str(&format!("  {}{}\n", name, comma));
        }
        code.push_str("};\n");
    }

    // Ensure the file is treated as a module by TypeScript.
    if !code.contains("export ") {
        code.push_str("\nexport {};\n");
    }

    code
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
    out.push_str(
        r#"] as const;

export type ControllerMount = {
  name: string;
  selector: string;
  root: Element;
  cleanup: unknown;
};

export type ControllerFailure = {
  name: string;
  selector: string;
  error: unknown;
};

export type ControllerRuntime = {
  mounts: ControllerMount[];
  failures: ControllerFailure[];
  dispose(): void;
};

function disposeCleanup(cleanup: unknown): void {
  if (Array.isArray(cleanup)) {
    for (const item of cleanup) {
      disposeCleanup(item);
    }
    return;
  }
  if (
    cleanup !== null &&
    typeof cleanup === "object" &&
    "dispose" in cleanup &&
    typeof cleanup.dispose === "function"
  ) {
    (cleanup as { dispose: () => void }).dispose();
  }
}

export function mountControllers(root: ParentNode = globalThis.document): ControllerRuntime {
  const mounts: ControllerMount[] = [];
  const failures: ControllerFailure[] = [];
  for (const controller of controllers) {
    const element = root.querySelector(controller.selector);
    if (element === null) {
      failures.push({
        name: controller.name,
        selector: controller.selector,
        error: new Error(`browser controller mount root not found: ${controller.selector}`),
      });
      continue;
    }
    try {
      // WebDomScope shape: real runtime requires `root` for query/require.
      const cleanup = controller.mount({ root: element, selector: controller.selector });
      mounts.push({
        name: controller.name,
        selector: controller.selector,
        root: element,
        cleanup,
      });
    } catch (error) {
      failures.push({ name: controller.name, selector: controller.selector, error });
    }
  }
  return {
    mounts,
    failures,
    dispose() {
      for (let index = mounts.length - 1; index >= 0; index -= 1) {
        disposeCleanup(mounts[index].cleanup);
      }
    },
  };
}
"#,
    );
    out
}

fn web_ambient_declarations() -> String {
    r#"declare module "web:dom" {
  export class Scope { selector: string; constructor(fields: { selector?: string }); }
  export class Element { selector: string; constructor(fields: { selector?: string }); }
  export class DomEvent { kind: string; default_prevented: boolean; }
  export class FrameState { frame: number; time_ms: number; delta_ms: number; }
  export class ResizeState { width: number; height: number; device_pixel_ratio: number; }
  export class KeyboardState { kind: string; key: string; code: string; repeat: boolean; alt: boolean; ctrl: boolean; shift: boolean; meta: boolean; }
  export class PointerState { kind: string; x: number; y: number; movement_x: number; movement_y: number; button: number; primary: boolean; }
  export class FocusState { focused: boolean; }
  export class PointerLockState { supported: boolean; locked: boolean; denied: boolean; target_matches: boolean; }
  export class Subscription { id: number; }
  export class SubmitOptions { prevent_default: boolean; constructor(fields?: { prevent_default?: boolean }); }
  export class FetchRequest { url: string; method: string; body: string | null; constructor(fields: { url: string; method?: string; body?: string | null }); }
  export class FetchResponse { status: number; ok: boolean; body: string; }
  export type EventHandler = (event: DomEvent) => void;
  export type InputHandler = (element: Element, value: string) => void;
  export type SubmitHandler = (form: Element) => void;
  export type FrameHandler = (state: FrameState) => void;
  export type ResizeHandler = (state: ResizeState) => void;
  export type KeyboardHandler = (state: KeyboardState) => void;
  export type PointerHandler = (state: PointerState) => void;
  export type FocusHandler = (state: FocusState) => void;
  export type PointerLockHandler = (state: PointerLockState) => void;
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
  export function on_frame(handler: FrameHandler): Subscription;
  export function on_resize(handler: ResizeHandler): Subscription;
  export function on_keyboard(element: Element, event_name: string, handler: KeyboardHandler): Subscription;
  export function on_pointer(element: Element, event_name: string, handler: PointerHandler): Subscription;
  export function on_focus(element: Element, event_name: string, handler: FocusHandler): Subscription;
  export function pointer_lock_state(element: Element): PointerLockState;
  export function request_pointer_lock(element: Element): PointerLockState;
  export function exit_pointer_lock(): PointerLockState;
  export function on_pointer_lock(element: Element, handler: PointerLockHandler): Subscription;
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
    on_frame(handler: FrameHandler): Subscription;
    on_resize(handler: ResizeHandler): Subscription;
    on_keyboard(element: Element, event_name: string, handler: KeyboardHandler): Subscription;
    on_pointer(element: Element, event_name: string, handler: PointerHandler): Subscription;
    on_focus(element: Element, event_name: string, handler: FocusHandler): Subscription;
    pointer_lock_state(element: Element): PointerLockState;
    request_pointer_lock(element: Element): PointerLockState;
    exit_pointer_lock(): PointerLockState;
    on_pointer_lock(element: Element, handler: PointerLockHandler): Subscription;
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
"#.to_string()
}

fn render_tsconfig(ts_root: &Path, esm_root: &Path) -> String {
    format!(
        r#"{{
  "compilerOptions": {{
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "bundler",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
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
    Box::new(Diagnostic::io_error(path, &err))
}

#[cfg(test)]
#[path = "product_test.rs"]
mod tests;
