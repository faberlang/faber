//! Unique temporary directories for staged writes.
//!
//! One place owns the exclusive-create retry policy, so every caller that
//! needs a private temp directory (generated crates, lint staging) shares it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use radix::module::diagnostics::Diagnostic;

/// Monotonic nanoseconds since the Unix epoch for unique temp path names.
pub(super) fn process_nanos() -> Result<u128, Box<Diagnostic>> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            Box::new(crate::package_diagnostic_error(
                "system clock precedes epoch",
            ))
        })
        .map(|duration| duration.as_nanos())
}

/// Create a unique temporary sibling directory for staged writes.
///
/// Crate-internal shared mechanism for exclusive temp-root creation: any
/// caller that needs a private temp directory must come through here so the
/// exclusive-create retry policy lives in exactly one place.
pub(crate) fn unique_temp_sibling(parent: &Path, label: &str) -> Result<PathBuf, Box<Diagnostic>> {
    for attempt in 0..128_u32 {
        let nonce = process_nanos()?;
        let path = parent.join(format!(
            ".{label}.tmp-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(Box::new(crate::exit_status::write_diagnostic_error(
                    &path, &error,
                )));
            }
        }
    }
    Err(Box::new(crate::package_diagnostic_error(
        "could not create a unique temporary directory",
    )))
}
