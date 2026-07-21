use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub const EXPECTED_ROOTS: &[&str] = &[
    "faber-runtime",
    "hosts/crates/host-kernel",
    "hosts/crates/host-native",
    "hosts/crates/aleator",
    "hosts/crates/http",
    "hosts/crates/consolum",
    "hosts/crates/processus",
    "hosts/crates/solum",
    "hosts/crates/tempus",
];

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FileRecord {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Assembly {
    pub archive: Vec<u8>,
    pub archive_sha256: String,
    pub files: Vec<FileRecord>,
}

pub fn read_roots(manifest: &Path) -> io::Result<Vec<String>> {
    let roots = fs::read_to_string(manifest)?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let expected = EXPECTED_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect::<Vec<_>>();
    if roots != expected {
        return Err(invalid(
            "core-support manifest must match the canonical allowlist",
        ));
    }
    Ok(roots)
}

pub fn assemble(workspace: &Path, roots: &[String]) -> io::Result<Assembly> {
    let mut files = BTreeSet::new();
    for root in roots {
        let path = checked_root(workspace, root)?;
        collect(workspace, &path, &mut files)?;
    }

    let mut tar = tar::Builder::new(Vec::new());
    let mut records = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(workspace)
            .map_err(|_| invalid("payload path escapes workspace"))?;
        let name = archive_name(relative)?;
        let data = fs::read(&path)?;
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, &name, data.as_slice())?;
        records.push(FileRecord {
            path: name,
            sha256: hex(&Sha256::digest(&data)),
        });
    }
    let tar = tar.into_inner()?;
    let archive = zstd::stream::encode_all(tar.as_slice(), 19)?;
    let archive_sha256 = hex(&Sha256::digest(&archive));
    Ok(Assembly {
        archive,
        archive_sha256,
        files: records,
    })
}

pub fn file_manifest(files: &[FileRecord]) -> String {
    let mut output = String::new();
    for file in files {
        use std::fmt::Write;
        let _ = writeln!(output, "{}  {}", file.sha256, file.path);
    }
    output
}

fn checked_root(workspace: &Path, entry: &str) -> io::Result<PathBuf> {
    let relative = Path::new(entry);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(invalid("unsafe core-support manifest entry"));
    }
    let path = workspace.join(relative);
    if !path.exists() {
        return Err(invalid("required core-support path is missing"));
    }
    Ok(path)
}

fn collect(workspace: &Path, path: &Path, files: &mut BTreeSet<PathBuf>) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        if is_executable(&metadata) {
            return Err(invalid("core-support payload rejects executable files"));
        }
        let relative = path
            .strip_prefix(workspace)
            .map_err(|_| invalid("payload path escapes workspace"))?;
        if is_secret(relative) {
            return Err(invalid("core-support payload rejects secret-like files"));
        }
        let data = fs::read(path)?;
        if data.contains(&0) || std::str::from_utf8(&data).is_err() {
            return Err(invalid("core-support payload rejects binary files"));
        }
        files.insert(path.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(invalid("core-support payload rejects non-regular files"));
    }
    for entry in fs::read_dir(path)? {
        let child = entry?.path();
        let name = child
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid("non-UTF-8 payload path"))?;
        if matches!(
            name,
            ".git"
                | "target"
                | ".cache"
                | "cache"
                | "build"
                | "__pycache__"
                | "node_modules"
                | ".DS_Store"
        ) {
            continue;
        }
        collect(workspace, &child, files)?;
    }
    Ok(())
}

fn archive_name(relative: &Path) -> io::Result<String> {
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid("unsafe payload path"));
    }
    relative
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid("non-UTF-8 payload path"))
}

fn is_secret(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let name = name.to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.contains("secret")
        || name.contains("credential")
        || matches!(name.as_str(), "id_rsa" | "id_ed25519")
        || ["pem", "key", "p12", "pfx", "der"]
            .iter()
            .any(|suffix| name.ends_with(&format!(".{suffix}")))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut acc, byte| {
            use std::fmt::Write;
            let _ = write!(acc, "{byte:02x}");
            acc
        })
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &fs::Metadata) -> bool {
    false
}
