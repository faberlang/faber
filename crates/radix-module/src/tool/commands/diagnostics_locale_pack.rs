//! Reader-locale pack loading from TOML paths for developer-tool commands.
//!
//! Owns [`load_diagnostics_locale_pack_or_exit`] which reads a
//! [`LocalePack`] from a `--diagnostics-locale` file path and exits with an
//! error on load failure. Delegates TOML deserialization to the library type.
//! The returned pack is consumed by check/emit commands for localized diagnostic
//! rendering.

use crate::locale::LocalePack;
use std::path::{Path, PathBuf};

#[must_use]
pub fn load_diagnostics_locale_pack_or_exit(path: Option<&PathBuf>) -> Option<LocalePack> {
    let path = path?;
    match LocalePack::from_toml_path(path) {
        Ok(pack) => Some(pack),
        Err(err) => {
            eprintln!("error: failed to load reader pack '{}': {err}", display_path(path));
            std::process::exit(1);
        }
    }
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}
