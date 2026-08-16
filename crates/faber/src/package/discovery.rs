//! Package discovery for the faber package surface.
//!
//! RP2b: discovery/build-layout logic is compiler-owned and lives in
//! `radix-package`; this module forwards the moved surface. Two faber-owned
//! seams stay here (CTO memo): input absolutization reads the ambient
//! CWD (`paths::absolutize_path`), and the full product-view
//! `validate_manifest` (`[device]`, `[format]`, `[product]`, `[dispatch]`,
//! locale policy) still runs at discovery time in faber flows.

use std::path::Path;

use radix::diagnostics::Diagnostic;

use super::manifest::{read_manifest, validate_manifest};
use super::paths::absolutize_path;
use super::MANIFEST_FILE;

pub use radix_package::discovery::{sanitize_crate_name, BuildLayout};
pub(crate) use radix_package::discovery::{resolve_source_root, PackageSpec};

pub(crate) fn discover_package(input: &Path) -> Result<PackageSpec, Box<Diagnostic>> {
    let spec = radix_package::discovery::discover_package(&absolutize_path(input))?;
    // Product-only manifest policy is faber-owned; discovery validates it so
    // invalid packages fail early with the same diagnostics as before RP2b.
    if spec.manifest_backed {
        let manifest_path = spec.package_root.join(MANIFEST_FILE);
        if manifest_path.exists() {
            let manifest = read_manifest(&manifest_path)?;
            validate_manifest(&manifest, &manifest_path)?;
        }
    }
    Ok(spec)
}

#[allow(dead_code)] // consumed by the bin `commands/run` interpreted dispatch.
pub(crate) fn is_manifest_backed_or_directory_package_input(input: &Path) -> bool {
    radix_package::discovery::is_manifest_backed_or_directory_package_input(&absolutize_path(
        input,
    ))
}

pub fn discover_build_layout(input: &Path) -> Result<BuildLayout, Box<Diagnostic>> {
    radix_package::discovery::discover_build_layout(&absolutize_path(input))
}
