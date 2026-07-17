use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use radix::diagnostics::Diagnostic;
use sha2::{Digest, Sha256};

use super::manifest::{ManifestProduct, ManifestProductKind};
use super::paths::normalize_path;

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

/// Build the static-asset portion of a browser-app product recipe.
///
/// WEB2 owns only deterministic HTML/CSS/public asset copying. Controller TS,
/// `tsc`, and `controllers.json` are later WEB3 work; the asset manifest written
/// here gives those stages deterministic static paths without inventing a Radix
/// `web` target.
pub(crate) fn build_browser_product_static_assets(
    package_root: &Path,
    product: &ManifestProduct,
) -> Result<BrowserProductAssetBuild, Box<Diagnostic>> {
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

    reject_stale_outputs(&out_dir, &planned, &manifest_path)?;

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

#[derive(Debug)]
struct PlannedAsset {
    kind: &'static str,
    source: PathBuf,
    size: u64,
    sha256: String,
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
    manifest_path: &Path,
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
        .chain(std::iter::once(normalize_path(manifest_path)))
        .collect::<BTreeSet<_>>();
    reject_stale_dir(out_dir, &allowed)
}

fn reject_stale_dir(dir: &Path, allowed: &BTreeSet<PathBuf>) -> Result<(), Box<Diagnostic>> {
    for entry in fs::read_dir(dir).map_err(|err| io_diag(dir, err))? {
        let entry = entry.map_err(|err| io_diag(dir, err))?;
        let path = normalize_path(&entry.path());
        let metadata = fs::symlink_metadata(&path).map_err(|err| io_diag(&path, err))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
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

fn product_diag(message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message.into())
}

fn io_diag(path: &Path, err: std::io::Error) -> Box<Diagnostic> {
    Box::new(Diagnostic::io_error(path, err))
}
