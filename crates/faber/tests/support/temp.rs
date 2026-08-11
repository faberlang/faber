#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str, label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{label}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
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
