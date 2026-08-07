use std::path::{Path, PathBuf};

use radix::diagnostics::Diagnostic;

use super::BuildLayout;

pub(crate) struct LinkedLibraryCrate {
    pub(crate) crate_name: String,
    pub(crate) crate_root: PathBuf,
}

pub(crate) fn emit_linked_library_crates(
    app_root: &Path,
    _layout: &BuildLayout,
) -> Result<Vec<LinkedLibraryCrate>, Vec<Diagnostic>> {
    Err(vec![
        crate::package_diagnostic_error(
            "Rust linked-library crate emission is not available in this faber build; rebuild with feature `hir-rust`",
        )
        .with_file(app_root.display().to_string())
        .with_arg("issue", "package_target_unavailable")
        .with_arg("target", "rust"),
    ])
}
