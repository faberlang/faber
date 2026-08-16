//! Library interface cache and expansion for the faber package surface
//! (forwarded).
//!
//! RP2c: the library interface cache, import expansion, export surfaces, and
//! provenance attachment moved to `radix-package`; this module forwards the
//! moved surface and supplies the faber-owned locale-pack loader seam (locale
//! packs are product layout — installed packs beside the binary, manifest
//! `[locale] pack` overrides). RP3: the loader and analysis consume the cache
//! in `radix-package` directly; only the surfaces the emission half still
//! consumes are forwarded here.

use radix::driver::Config;

use super::locale::load_locale_pack_for_input;

#[allow(unused_imports)] // synthetic_library_item_def_id consumed by the moved Rust assembly until RP7
pub use radix_package::library::{
    is_builtin_norma_http_module, library_cached_analysis, library_cached_file_interface,
    synthetic_library_item_def_id, LibraryInterfaceCache,
};
pub use radix_package::library::with_library_cached_analysis_mut;
#[cfg(feature = "hir-rust")]
#[allow(unused_imports)] // consumed by the moved Rust assembly until RP7
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
