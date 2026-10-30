#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str, label: &str) -> Self {
        // Uniqueness needs more than the wall clock: worker threads creating
        // directories in the same nanosecond would collide (create_dir_all is
        // idempotent), share one tree, and one drop would delete the other's
        // case mid-run. The pid separates processes; the counter separates
        // same-nanosecond creations inside this one.
        static CREATION_COUNTER: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let ordinal = CREATION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{label}-{}-{nonce}-{ordinal}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => Self { path },
            Err(_) if path.is_dir() => {
                // A same-name directory already exists: uniqueness failed,
                // so fail loudly rather than share the tree.
                panic!("temp dir name collision: {}", path.display());
            }
            Err(error) => panic!("create temp dir {}: {error}", path.display()),
        }
    }
}

impl Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<OsStr> for TempDir {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(first_error) = fs::remove_dir_all(&self.path) {
            let result =
                make_tree_writable(&self.path).and_then(|()| fs::remove_dir_all(&self.path));
            if let Err(error) = result {
                eprintln!(
                    "failed to remove test temp dir {}: {error} (initially: {first_error})",
                    self.path.display()
                );
            }
        }
    }
}

pub struct TempPath {
    _root: TempDir,
    path: PathBuf,
}

impl TempPath {
    pub fn new(root: TempDir, path: PathBuf) -> Self {
        Self { _root: root, path }
    }
}

impl Deref for TempPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<OsStr> for TempPath {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

#[cfg(unix)]
fn make_tree_writable(root: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(root)? {
            make_tree_writable(&entry?.path())?;
        }
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    } else {
        fs::set_permissions(root, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_tree_writable(root: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(root)? {
            make_tree_writable(&entry?.path())?;
        }
    }
    let mut permissions = metadata.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(root, permissions)
}
