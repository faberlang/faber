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

mod assets;
mod build;
mod controllers;
mod ts_emit;
mod ts_render;
mod ts_rewrite;

// External contract seams: package/mod.rs and package_test.rs resolve these
// through the product module (package/mod.rs stays byte-untouched).
#[cfg(test)]
pub(crate) use assets::build_browser_product_static_assets;
pub(crate) use build::build_browser_product;
#[cfg(test)]
pub(crate) use build::inject_product_failure_at;
// The product test companion imports these through `use super::*;` (mir house
// style); production consumers import from the defining submodule directly.
#[allow(unused_imports)]
pub(super) use assets::{BrowserProductAsset, BrowserProductAssetBuild};
#[allow(unused_imports)]
pub(super) use build::{BrowserController, BrowserProductBuild};
#[allow(unused_imports)]
pub(super) use ts_rewrite::{
    build_library_ts_module_map, normalize_library_namespace_bindings, rewrite_import_specifiers,
};

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
