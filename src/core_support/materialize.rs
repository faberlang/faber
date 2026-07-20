//! Verified, content-addressed extraction of Faber's embedded core support.

use super::{ARCHIVE, FILE_MANIFEST, SHA256};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const PAYLOAD_FORMAT: &str = "tar-zst-v1";
const COMPLETION_FILE: &str = ".faber-core-support-complete";
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 256 * 1024 * 1024;

/// An immutable, verified extraction of the embedded core-support payload.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MaterializedCoreSupport {
    root: PathBuf,
}

impl MaterializedCoreSupport {
    /// The content-addressed root suitable for later generated-Cargo routing.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Verified source root for the core runtime crate.
    pub fn faber_runtime(&self) -> Result<PathBuf, MaterializeError> {
        self.required_directory("faber-runtime")
    }

    /// Verified source root for the native kernel crate.
    pub fn host_kernel(&self) -> Result<PathBuf, MaterializeError> {
        self.required_directory("host-kernel-rs")
    }

    /// Verified source root for the native host crate.
    pub fn host_native(&self) -> Result<PathBuf, MaterializeError> {
        self.required_directory("host-native-rs")
    }

    /// Verified source root for one explicit embedded provider crate.
    pub fn provider(&self, provider: &str) -> Result<PathBuf, MaterializeError> {
        if !matches!(
            provider,
            "aleator" | "consolum" | "http" | "processus" | "solum" | "tempus"
        ) {
            return Err(MaterializeError::InvalidPayload(
                "unsupported core-support provider",
            ));
        }
        self.required_directory(&format!("host-providers-rs/crates/{provider}"))
    }

    fn required_directory(&self, relative: &str) -> Result<PathBuf, MaterializeError> {
        let path = self.root.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(MaterializeError::io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(MaterializeError::InvalidPayload(
                "verified core-support path is unavailable",
            ));
        }
        Ok(path)
    }
}

/// Materializes the embedded payload beneath the platform cache directory.
pub fn materialize() -> Result<MaterializedCoreSupport, MaterializeError> {
    materialize_into(&platform_cache_dir()?)
}

/// Materializes the embedded payload beneath `cache_root`.
///
/// This explicit seam exists for callers that own an application cache root and
/// for tests; it never derives a path from a workspace or checkout location.
pub fn materialize_into(cache_root: &Path) -> Result<MaterializedCoreSupport, MaterializeError> {
    materialize_payload(cache_root, ARCHIVE, SHA256, FILE_MANIFEST)
}

fn materialize_payload(
    cache_root: &Path,
    archive: &[u8],
    expected_archive_hash: &str,
    file_manifest: &str,
) -> Result<MaterializedCoreSupport, MaterializeError> {
    verify_archive_hash(archive, expected_archive_hash)?;
    let expected_files = parse_manifest(file_manifest)?;
    let parent = cache_root
        .join("faber")
        .join("core-support")
        .join(PAYLOAD_FORMAT);
    let created_cache_root = match fs::symlink_metadata(cache_root) {
        Ok(_) => false,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => return Err(MaterializeError::io(error)),
    };
    fs::create_dir_all(&parent).map_err(MaterializeError::io)?;
    if created_cache_root {
        secure_created_cache_root(cache_root)?;
    }
    secure_managed_cache_directories(cache_root)?;
    ensure_real_cache_directories(cache_root, &parent)?;

    let target = parent.join(expected_archive_hash);
    let lock = open_lock(&parent, expected_archive_hash)?;
    lock.lock_exclusive().map_err(MaterializeError::io)?;
    let result = materialize_locked(
        &parent,
        &target,
        archive,
        expected_archive_hash,
        &expected_files,
    );
    // Unlock errors are non-fatal: the file descriptor closes after return,
    // which releases the OS-level lock.  Prefer the materialization result.
    drop(FileExt::unlock(&lock));
    result
}

fn materialize_locked(
    parent: &Path,
    target: &Path,
    archive: &[u8],
    expected_archive_hash: &str,
    expected_files: &BTreeMap<String, String>,
) -> Result<MaterializedCoreSupport, MaterializeError> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(MaterializeError::InvalidPayload("cache entry is a symlink"));
        }
        Ok(metadata) if metadata.is_dir() => {
            if completed_entry_is_valid(target, expected_archive_hash, expected_files)? {
                return Ok(MaterializedCoreSupport {
                    root: target.to_path_buf(),
                });
            }
            quarantine_incomplete_entry(target)?;
        }
        Ok(_) => quarantine_incomplete_entry(target)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(MaterializeError::io(error)),
    }

    let temp = unique_temp_dir(parent, expected_archive_hash)?;
    let published = (|| {
        extract(archive, &temp, expected_files)?;
        fsync_tree(&temp)?;
        write_completion(&temp, expected_archive_hash, expected_files)?;
        make_immutable(&temp)?;
        fsync_tree(&temp)?;
        fs::rename(&temp, target).map_err(MaterializeError::io)?;
        fsync_dir(parent)?;
        Ok(MaterializedCoreSupport {
            root: target.to_path_buf(),
        })
    })();
    match published {
        Ok(materialized) => Ok(materialized),
        Err(error) => {
            if fs::symlink_metadata(&temp).is_ok() {
                remove_our_temp(&temp)?;
            }
            Err(error)
        }
    }
}

#[cfg(unix)]
fn secure_created_cache_root(cache_root: &Path) -> Result<(), MaterializeError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::symlink_metadata(cache_root).map_err(MaterializeError::io)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(MaterializeError::InvalidPayload("cache root is unsafe"));
    }
    fs::set_permissions(cache_root, fs::Permissions::from_mode(0o700)).map_err(MaterializeError::io)
}

#[cfg(not(unix))]
fn secure_created_cache_root(_: &Path) -> Result<(), MaterializeError> {
    Ok(())
}

#[cfg(unix)]
fn secure_managed_cache_directories(cache_root: &Path) -> Result<(), MaterializeError> {
    use std::os::unix::fs::PermissionsExt;

    let mut current = cache_root.to_path_buf();
    for component in Path::new("faber/core-support").components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(MaterializeError::io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(MaterializeError::InvalidPayload(
                "cache directory is unsafe",
            ));
        }
        fs::set_permissions(&current, fs::Permissions::from_mode(0o700))
            .map_err(MaterializeError::io)?;
    }
    let payload = current.join(PAYLOAD_FORMAT);
    let metadata = fs::symlink_metadata(&payload).map_err(MaterializeError::io)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(MaterializeError::InvalidPayload(
            "cache directory is unsafe",
        ));
    }
    fs::set_permissions(payload, fs::Permissions::from_mode(0o700))
        .map_err(MaterializeError::io)?;
    Ok(())
}

#[cfg(not(unix))]
fn secure_managed_cache_directories(_: &Path) -> Result<(), MaterializeError> {
    Ok(())
}

fn ensure_real_cache_directories(cache_root: &Path, parent: &Path) -> Result<(), MaterializeError> {
    let root_metadata = fs::symlink_metadata(cache_root).map_err(MaterializeError::io)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(MaterializeError::InvalidPayload("cache root is unsafe"));
    }
    ensure_secure_cache_dir(cache_root)?;
    let mut current = cache_root.to_path_buf();
    for component in Path::new("faber/core-support").components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(MaterializeError::io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(MaterializeError::InvalidPayload(
                "cache directory is unsafe",
            ));
        }
        ensure_secure_cache_dir(&current)?;
    }
    let metadata = fs::symlink_metadata(parent).map_err(MaterializeError::io)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(MaterializeError::InvalidPayload(
            "cache directory is unsafe",
        ));
    }
    ensure_secure_cache_dir(parent)?;
    Ok(())
}

#[cfg(unix)]
fn ensure_secure_cache_dir(path: &Path) -> Result<(), MaterializeError> {
    use std::os::unix::fs::PermissionsExt;

    if fs::metadata(path)
        .map_err(MaterializeError::io)?
        .permissions()
        .mode()
        & 0o022
        != 0
    {
        return Err(MaterializeError::InvalidPayload(
            "cache directory is writable by another user",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_secure_cache_dir(_: &Path) -> Result<(), MaterializeError> {
    Ok(())
}

fn verify_archive_hash(archive: &[u8], expected: &str) -> Result<(), MaterializeError> {
    if !is_sha256(expected) || hex(&Sha256::digest(archive)) != expected {
        return Err(MaterializeError::InvalidPayload("archive SHA-256 mismatch"));
    }
    Ok(())
}

fn parse_manifest(manifest: &str) -> Result<BTreeMap<String, String>, MaterializeError> {
    let mut files = BTreeMap::new();
    for line in manifest.lines() {
        let (hash, path) = line
            .split_once("  ")
            .ok_or(MaterializeError::InvalidPayload(
                "invalid file hash manifest",
            ))?;
        if !is_sha256(hash)
            || !is_safe_relative(Path::new(path))
            || files.insert(path.to_owned(), hash.to_owned()).is_some()
        {
            return Err(MaterializeError::InvalidPayload(
                "invalid or duplicate manifest entry",
            ));
        }
    }
    if files.is_empty() {
        return Err(MaterializeError::InvalidPayload("empty file hash manifest"));
    }
    Ok(files)
}

fn extract(
    archive: &[u8],
    temp: &Path,
    expected_files: &BTreeMap<String, String>,
) -> Result<(), MaterializeError> {
    let decoder = zstd::stream::read::Decoder::new(archive).map_err(MaterializeError::io)?;
    let mut tar = tar::Archive::new(decoder);
    let mut seen = BTreeSet::new();
    let mut total = 0_u64;
    for entry in tar.entries().map_err(MaterializeError::io)? {
        let mut entry = entry.map_err(MaterializeError::io)?;
        if !entry.header().entry_type().is_file() {
            return Err(MaterializeError::InvalidPayload(
                "archive contains a non-regular entry",
            ));
        }
        let path = entry.path().map_err(MaterializeError::io)?.into_owned();
        if !is_safe_relative(&path) {
            return Err(MaterializeError::InvalidPayload("archive path is unsafe"));
        }
        let name = path.to_str().ok_or(MaterializeError::InvalidPayload(
            "archive path is not UTF-8",
        ))?;
        let expected_hash = expected_files
            .get(name)
            .ok_or(MaterializeError::InvalidPayload(
                "archive contains an unexpected file",
            ))?;
        if !seen.insert(name.to_owned()) {
            return Err(MaterializeError::InvalidPayload(
                "archive contains a duplicate file",
            ));
        }
        let size = entry.size();
        if size > MAX_FILE_BYTES
            || total
                .checked_add(size)
                .filter(|size| *size <= MAX_PAYLOAD_BYTES)
                .is_none()
        {
            return Err(MaterializeError::InvalidPayload(
                "archive exceeds extraction size limits",
            ));
        }
        total += size;
        let destination = temp.join(&path);
        let parent = destination
            .parent()
            .ok_or(MaterializeError::InvalidPayload(
                "archive path has no parent",
            ))?;
        fs::create_dir_all(parent).map_err(MaterializeError::io)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(MaterializeError::io)?;
        let actual_hash = copy_and_hash(&mut entry, &mut output, size)?;
        output.sync_all().map_err(MaterializeError::io)?;
        if actual_hash != *expected_hash {
            return Err(MaterializeError::InvalidPayload(
                "extracted file SHA-256 mismatch",
            ));
        }
        set_read_only_file(&destination)?;
    }
    if seen.len() != expected_files.len() {
        return Err(MaterializeError::InvalidPayload(
            "archive is missing expected files",
        ));
    }
    Ok(())
}

fn copy_and_hash(
    input: &mut impl Read,
    output: &mut impl Write,
    expected_size: u64,
) -> Result<String, MaterializeError> {
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 8192];
    let mut written = 0_u64;
    loop {
        let count = input.read(&mut buffer).map_err(MaterializeError::io)?;
        if count == 0 {
            break;
        }
        written = written
            .checked_add(count as u64)
            .ok_or(MaterializeError::InvalidPayload("file size overflow"))?;
        if written > expected_size {
            return Err(MaterializeError::InvalidPayload(
                "archive entry exceeds declared size",
            ));
        }
        hash.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(MaterializeError::io)?;
    }
    if written != expected_size {
        return Err(MaterializeError::InvalidPayload(
            "archive entry is truncated",
        ));
    }
    Ok(hex(&hash.finalize()))
}

fn completed_entry_is_valid(
    target: &Path,
    expected_archive_hash: &str,
    expected_files: &BTreeMap<String, String>,
) -> Result<bool, MaterializeError> {
    let completion = target.join(COMPLETION_FILE);
    let Ok(metadata) = fs::symlink_metadata(&completion) else {
        return Ok(false);
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let expected = completion_contents(expected_archive_hash, expected_files);
    if fs::read_to_string(completion).map_err(MaterializeError::io)? != expected {
        return Err(MaterializeError::InvalidPayload(
            "completed cache entry metadata is corrupt",
        ));
    }
    for (path, expected_hash) in expected_files {
        let file = target.join(path);
        let metadata = fs::symlink_metadata(&file).map_err(MaterializeError::io)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || hash_file(&file)? != *expected_hash
        {
            return Err(MaterializeError::InvalidPayload(
                "completed cache entry file is corrupt",
            ));
        }
    }
    if !has_only_expected_files(target, expected_files)? {
        return Err(MaterializeError::InvalidPayload(
            "completed cache entry contains an unexpected file",
        ));
    }
    Ok(true)
}

fn has_only_expected_files(
    root: &Path,
    expected_files: &BTreeMap<String, String>,
) -> Result<bool, MaterializeError> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(MaterializeError::io)? {
            let path = entry.map_err(MaterializeError::io)?.path();
            let metadata = fs::symlink_metadata(&path).map_err(MaterializeError::io)?;
            if metadata.file_type().is_symlink() {
                return Ok(false);
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                return Ok(false);
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| MaterializeError::InvalidPayload("cache path escapes entry"))?;
            if relative != Path::new(COMPLETION_FILE)
                && !relative
                    .to_str()
                    .is_some_and(|path| expected_files.contains_key(path))
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn hash_file(path: &Path) -> Result<String, MaterializeError> {
    let mut file = File::open(path).map_err(MaterializeError::io)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(MaterializeError::io)?;
        if count == 0 {
            return Ok(hex(&hash.finalize()));
        }
        hash.update(&buffer[..count]);
    }
}

fn write_completion(
    temp: &Path,
    archive_hash: &str,
    files: &BTreeMap<String, String>,
) -> Result<(), MaterializeError> {
    let completion = temp.join(COMPLETION_FILE);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&completion)
        .map_err(MaterializeError::io)?;
    file.write_all(completion_contents(archive_hash, files).as_bytes())
        .map_err(MaterializeError::io)?;
    file.sync_all().map_err(MaterializeError::io)?;
    set_read_only_file(&completion)
}

fn completion_contents(archive_hash: &str, files: &BTreeMap<String, String>) -> String {
    format!(
        "format={PAYLOAD_FORMAT}\narchive_sha256={archive_hash}\nfiles_sha256={}\n",
        hex(&Sha256::digest(file_manifest_contents(files)))
    )
}

fn file_manifest_contents(files: &BTreeMap<String, String>) -> String {
    files
        .iter()
        .map(|(path, hash)| format!("{hash}  {path}\n"))
        .collect()
}

fn open_lock(parent: &Path, hash: &str) -> Result<File, MaterializeError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join(format!("{hash}.lock")))
        .map_err(MaterializeError::io)
}

fn unique_temp_dir(parent: &Path, hash: &str) -> Result<PathBuf, MaterializeError> {
    for attempt in 0..128_u32 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| MaterializeError::InvalidPayload("system clock precedes epoch"))?
            .as_nanos();
        let path = parent.join(format!(
            ".{hash}.tmp-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(MaterializeError::io(error)),
        }
    }
    Err(MaterializeError::InvalidPayload(
        "could not create a unique extraction directory",
    ))
}

fn remove_our_temp(temp: &Path) -> Result<(), MaterializeError> {
    make_tree_writable(temp)?;
    fs::remove_dir_all(temp).map_err(MaterializeError::io)
}

#[cfg(unix)]
fn make_tree_writable(root: &Path) -> Result<(), MaterializeError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::symlink_metadata(root).map_err(MaterializeError::io)?;
    if metadata.file_type().is_symlink() {
        return Err(MaterializeError::InvalidPayload(
            "temporary extraction tree contains a symlink",
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(root).map_err(MaterializeError::io)? {
            make_tree_writable(&entry.map_err(MaterializeError::io)?.path())?;
        }
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o755)).map_err(MaterializeError::io)
}

#[cfg(not(unix))]
fn make_tree_writable(root: &Path) -> Result<(), MaterializeError> {
    let metadata = fs::symlink_metadata(root).map_err(MaterializeError::io)?;
    if metadata.file_type().is_symlink() {
        return Err(MaterializeError::InvalidPayload(
            "temporary extraction tree contains a symlink",
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(root).map_err(MaterializeError::io)? {
            make_tree_writable(&entry.map_err(MaterializeError::io)?.path())?;
        }
    }
    let mut permissions = metadata.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(root, permissions).map_err(MaterializeError::io)
}

fn quarantine_incomplete_entry(target: &Path) -> Result<(), MaterializeError> {
    let parent = target.parent().ok_or(MaterializeError::InvalidPayload(
        "cache entry has no parent",
    ))?;
    let name = target.file_name().and_then(|name| name.to_str()).ok_or(
        MaterializeError::InvalidPayload("cache entry name is not UTF-8"),
    )?;
    for attempt in 0..128_u32 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| MaterializeError::InvalidPayload("system clock precedes epoch"))?
            .as_nanos();
        let quarantined = parent.join(format!(
            ".{name}.incomplete-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::rename(target, quarantined) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(MaterializeError::io(error)),
        }
    }
    Err(MaterializeError::InvalidPayload(
        "could not quarantine incomplete cache entry",
    ))
}

fn is_safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(target_os = "macos")]
fn platform_cache_dir() -> Result<PathBuf, MaterializeError> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Caches"))
        .ok_or(MaterializeError::InvalidPayload(
            "HOME is unavailable for the platform cache",
        ))
}

#[cfg(target_os = "windows")]
fn platform_cache_dir() -> Result<PathBuf, MaterializeError> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or(MaterializeError::InvalidPayload(
            "LOCALAPPDATA is unavailable for the platform cache",
        ))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn platform_cache_dir() -> Result<PathBuf, MaterializeError> {
    env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or(MaterializeError::InvalidPayload(
            "no platform cache directory is available",
        ))
}

fn make_immutable(root: &Path) -> Result<(), MaterializeError> {
    let mut directories = vec![root.to_path_buf()];
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(MaterializeError::io)? {
            let path = entry.map_err(MaterializeError::io)?.path();
            if fs::symlink_metadata(&path)
                .map_err(MaterializeError::io)?
                .is_dir()
            {
                directories.push(path.clone());
                pending.push(path);
            }
        }
    }
    for directory in directories.into_iter().rev() {
        set_read_only_dir(&directory)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_read_only_file(path: &Path) -> Result<(), MaterializeError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o444)).map_err(MaterializeError::io)
}

#[cfg(not(unix))]
fn set_read_only_file(path: &Path) -> Result<(), MaterializeError> {
    let mut permissions = fs::metadata(path)
        .map_err(MaterializeError::io)?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(MaterializeError::io)
}

#[cfg(unix)]
fn set_read_only_dir(path: &Path) -> Result<(), MaterializeError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o555)).map_err(MaterializeError::io)
}

#[cfg(not(unix))]
fn set_read_only_dir(path: &Path) -> Result<(), MaterializeError> {
    set_read_only_file(path)
}

fn fsync_tree(root: &Path) -> Result<(), MaterializeError> {
    let mut directories = vec![root.to_path_buf()];
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(MaterializeError::io)? {
            let path = entry.map_err(MaterializeError::io)?.path();
            if fs::symlink_metadata(&path)
                .map_err(MaterializeError::io)?
                .is_dir()
            {
                directories.push(path.clone());
                pending.push(path);
            }
        }
    }
    directories.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
    for directory in directories {
        fsync_dir(&directory)?;
    }
    Ok(())
}

#[cfg(unix)]
fn fsync_dir(path: &Path) -> Result<(), MaterializeError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(MaterializeError::io)
}

#[cfg(not(unix))]
fn fsync_dir(_: &Path) -> Result<(), MaterializeError> {
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A closed set of materialization failures. No failure falls back to a source checkout.
#[derive(Debug)]
pub enum MaterializeError {
    Io(io::Error),
    InvalidPayload(&'static str),
}

impl MaterializeError {
    fn io(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for MaterializeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(
                formatter,
                "core-support materialization I/O failure: {error}"
            ),
            Self::InvalidPayload(reason) => {
                write!(formatter, "invalid embedded core-support payload: {reason}")
            }
        }
    }
}

impl std::error::Error for MaterializeError {}

#[cfg(test)]
#[path = "materialize_test.rs"]
mod tests;
