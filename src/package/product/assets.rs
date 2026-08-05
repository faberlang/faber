use super::{
    fs, io_diag, normalize_path, product_diag, BTreeMap, BTreeSet, Diagnostic, Digest,
    ManifestProduct, ManifestProductKind, Path, PathBuf, Sha256, SystemTime, FABER_ESM_DIR,
    FABER_TS_DIR, GENERATED_DIR, TSCONFIG_FILE, UNIX_EPOCH,
};

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
pub(super) struct StaticAssetPlan {
    pub(super) out_dir: PathBuf,
    pub(super) manifest_path: PathBuf,
    pub(super) planned: BTreeMap<PathBuf, PlannedAsset>,
}

/// Collect planned static assets and run preflight checks (stale outputs,
/// collision containment). This is the fail-closed gate: it must run before
/// any cleanup or copy/write so that a collision error does not leave the
/// output directory in a partially destroyed state.
pub(super) fn plan_browser_product_static_assets(
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
/// after preflight ([`plan_browser_product_static_assets`]).
pub(super) fn write_browser_product_static_assets(
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
/// need staging between preflight and write (e.g. [`build_browser_product`]),
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
pub(super) struct PlannedAsset {
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
pub(super) fn render_product_json(
    out_dir: &Path,
    esm_entry: &Path,
    controllers_json: &Path,
    static_assets: &[BrowserProductAsset],
    shader_artifacts: Option<&(BrowserProductAsset, BrowserProductAsset)>,
) -> Result<String, Box<Diagnostic>> {
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
