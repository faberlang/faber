use super::assets::{
    plan_browser_product_static_assets, render_product_json, write_browser_product_static_assets,
    BrowserProductAsset, StaticAssetPlan,
};
use super::controllers::discover_controllers;
use super::ts_emit::{emit_library_typescript_modules, emit_typescript_modules};
use super::ts_render::{
    invoke_tsc, render_browser_entry, render_controllers_json, render_tsconfig,
    web_ambient_declarations,
};
use super::ts_rewrite::build_library_ts_module_map;
use super::{
    fs, io_diag, normalize_path, product_diag, BTreeMap, Diagnostic, Digest, ManifestProduct, Path,
    PathBuf, Sha256, SystemTime, BROWSER_ENTRY_JS, BROWSER_ENTRY_TS, FABER_ESM_DIR, FABER_TS_DIR,
    GENERATED_DIR, REFLECTION_FILE, TSCONFIG_FILE, UNIX_EPOCH, WEB_AMBIENT_DTS, WGSL_FILE,
};

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
    let layout = super::super::discover_build_layout(input)?;
    // Preflight (collision + stale-output containment) runs against the final
    // output directory BEFORE any staging, so a preflight error never disturbs
    // a previously published product.
    let plan = plan_browser_product_static_assets(&layout.package_root, product)?;
    let package = super::super::analyze_package(config, input).map_err(|diagnostics| {
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
    if let Ok(()) = result {}
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
