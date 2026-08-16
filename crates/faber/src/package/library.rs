//! Library interface cache and expansion for the faber package surface
//! (forwarded).
//!
//! RP2c: the library interface cache, import expansion, export surfaces, and
//! provenance attachment moved to `radix-package`; this module forwards the
//! moved surface and supplies the faber-owned locale-pack loader seam (locale
//! packs are product layout — installed packs beside the binary, manifest
//! `[locale] pack` overrides).

use radix::driver::Config;

use super::locale::load_locale_pack_for_input;

pub use radix_package::library::{
    analysis_source_for_file, attach_library_provenance_with_links, expand_library_imports,
    is_builtin_norma_http_module, library_cached_analysis, library_cached_file_interface,
    library_interface_export_surface, library_interface_has_module, program_export_surface,
    synthetic_library_item_def_id, LibraryInterfaceCache,
};
pub use radix_package::library::with_library_cached_analysis_mut;
#[cfg(feature = "hir-rust")]
pub use radix_package::library::{
    library_cached_expanded_imports, library_generates_rust_module, library_module_segments,
};

/// Build a library interface cache wired to faber's product locale-pack
/// loader.
pub(crate) fn library_cache_with_config(config: &Config) -> LibraryInterfaceCache {
    LibraryInterfaceCache::with_config(config).with_locale_loader(load_locale_pack_for_input)
}

/// Build a default library interface cache wired to faber's product
/// locale-pack loader.
pub(crate) fn library_cache_default() -> LibraryInterfaceCache {
    LibraryInterfaceCache::default().with_locale_loader(load_locale_pack_for_input)
}
