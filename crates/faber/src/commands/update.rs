//! `faber update` — resolve, verify, and install a public release binary.
//!
//! The install layout is one file per version plus a symlink:
//!
//! ```text
//! <prefix>/bin/faber-1.14.0
//! <prefix>/bin/faber-1.13.0
//! <prefix>/bin/faber -> faber-1.14.0
//! ```
//!
//! The versioned file is the receipt: the symlink target says which version is
//! active, and rolling back is another symlink swap. There is no install
//! metadata, no version lane, and no downloaded script. The updater resolves
//! the release itself, verifies the archive SHA-256 before extraction, and
//! replaces the symlink atomically.
//!
//! `faber update --check` is offline-independent of the install: it compares
//! `CARGO_PKG_VERSION` with the newest public `faber-v*` release and exits 1
//! when an update is available.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Args;
use handroll::sha256_hex;
use semver::Version;
use serde::Deserialize;

/// An updater failure and the exit status that names its class. A failure
/// with no known class (`From<String>`) is [`FAILURE`](faber_cli::exit_status::FAILURE);
/// a file or directory the install could not write is
/// [`CANT_CREATE`](faber_cli::exit_status::CANT_CREATE).
#[derive(Debug)]
pub(crate) struct UpdateError {
    status: i32,
    message: String,
}

impl UpdateError {
    fn cant_create(message: String) -> Self {
        Self {
            status: faber_cli::exit_status::CANT_CREATE,
            message,
        }
    }
}

impl From<String> for UpdateError {
    fn from(message: String) -> Self {
        Self {
            status: faber_cli::exit_status::FAILURE,
            message,
        }
    }
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::ops::Deref for UpdateError {
    type Target = str;

    fn deref(&self) -> &str {
        &self.message
    }
}

/// Release repository that hosts the public `faber-v*` releases.
const RELEASES_REPO: &str = "faberlang/releases";
/// GitHub release list endpoint for [`RELEASES_REPO`].
const RELEASES_API: &str = "https://api.github.com/repos/faberlang/releases/releases";
/// Download base for a release tag (`<base>/<tag>/<asset>`).
const DOWNLOAD_BASE: &str = "https://github.com/faberlang/releases/releases/download";
/// Release tags for this component start with this prefix.
const TAG_PREFIX: &str = "faber-v";
/// The release list is paginated; walk a bounded number of pages so a repo
/// that also carries other components cannot hide the newest faber release.
const MAX_RELEASE_PAGES: usize = 5;
/// Largest release artifact (archive or checksum) the updater will buffer.
/// A prebuilt archive is a few tens of MiB; the bound exists so a hostile
/// mirror cannot exhaust memory, not because real releases approach it.
const MAX_DOWNLOAD_BYTES: usize = 256 << 20;

/// Arguments for `faber update`.
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Report whether a newer public release exists; do not install
    #[arg(long, conflicts_with = "version")]
    pub check: bool,

    /// Install this exact version (also used to switch versions or roll back)
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,

    /// Release asset base URL or directory (default: the public release host)
    #[arg(long = "base-url", value_name = "URL")]
    pub base_url: Option<String>,

    /// Install prefix; `faber-<version>` and the `faber` symlink live under
    /// `<PREFIX>/bin` (default: the running binary's directory)
    #[arg(long, value_name = "DIR")]
    pub prefix: Option<PathBuf>,
}

/// Arguments for `faber uninstall`.
#[derive(Args, Debug)]
pub struct UninstallArgs {
    /// Remove one installed version
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,

    /// Remove every installed version and the `faber` symlink
    #[arg(long, conflicts_with = "version")]
    pub all: bool,

    /// Install prefix (default: the running binary's directory)
    #[arg(long, value_name = "DIR")]
    pub prefix: Option<PathBuf>,
}

/// Arguments for `faber versions`.
#[derive(Args, Debug)]
pub struct VersionsArgs {
    /// Install prefix (default: the running binary's directory)
    #[arg(long, value_name = "DIR")]
    pub prefix: Option<PathBuf>,
}

/// One release row from the GitHub release list.
#[derive(Debug, Deserialize)]
pub(crate) struct ReleaseJson {
    pub(crate) tag_name: String,
    #[serde(default)]
    pub(crate) draft: bool,
    #[serde(default)]
    pub(crate) prerelease: bool,
    #[serde(default)]
    pub(crate) html_url: String,
}

/// The newest public stable `faber-v*` release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublicRelease {
    pub(crate) version: Version,
    pub(crate) tag: String,
    pub(crate) url: String,
}

/// Dispatch `faber update`.
pub(crate) fn cmd_update(args: &UpdateArgs) {
    if args.check {
        check_for_update();
        return;
    }
    if let Err(error) = run_update(args) {
        eprintln!("error: {error}");
        std::process::exit(error.status);
    }
}

/// Dispatch `faber uninstall`.
pub(crate) fn cmd_uninstall(args: &UninstallArgs) {
    if let Err(error) = run_uninstall(args) {
        eprintln!("error: {error}");
        std::process::exit(error.status);
    }
}

/// Dispatch `faber versions`.
pub(crate) fn cmd_versions(args: &VersionsArgs) {
    let bin_dir = match managed_bin_dir(args.prefix.as_deref()) {
        Ok(bin_dir) => bin_dir,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };
    let active = active_version(&bin_dir);
    let versions = installed_versions(&bin_dir);
    if versions.is_empty() {
        println!("no installed faber versions in {}", bin_dir.display());
        return;
    }
    for version in &versions {
        let name = format!("faber-{version}");
        let mark = if active.as_deref() == Some(name.as_str()) {
            "*"
        } else {
            " "
        };
        println!("{mark} {version}");
    }
    println!();
    println!("directory: {}", bin_dir.display());
}

fn check_for_update() {
    let current = match current_version() {
        Ok(current) => current,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(2);
        }
    };
    let release = match latest_release() {
        Ok(release) => release,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(2);
        }
    };
    println!("faber {current}");
    println!(
        "latest public release: {} ({})",
        release.version, release.tag
    );
    match release.version.cmp(&current) {
        Ordering::Greater => {
            println!("  release: {}", release.url);
            println!("update available: faber update");
            std::process::exit(1);
        }
        Ordering::Equal => println!("up to date"),
        Ordering::Less => println!("ahead of the latest public release"),
    }
}

fn run_update(args: &UpdateArgs) -> Result<(), UpdateError> {
    let current = current_version()?;
    let target = match args.version.as_deref() {
        Some(raw) => parse_version(raw)?,
        None => latest_release()?.version,
    };
    let bin_dir = managed_bin_dir(args.prefix.as_deref())?;
    let triple = current_triple()?;
    let base = args
        .base_url
        .as_deref()
        .unwrap_or(DOWNLOAD_BASE)
        .trim_end_matches('/');
    let target_name = format!("faber-{target}");
    let target_path = bin_dir.join(&target_name);
    let active = active_version(&bin_dir);

    println!("faber update — {current} -> {target} ({triple})");
    println!("  bin:      {}", bin_dir.display());

    if target_path.is_file() {
        println!("  binary:   {} (already installed)", target_path.display());
    } else {
        let archive = format!("faber-v{target}-{triple}.tar.gz");
        println!("  download: {base}/faber-v{target}/{archive}");
        install_release(base, &target, triple, &bin_dir)?;
    }

    if active.as_deref() == Some(target_name.as_str()) {
        println!("active:     {target_name} (already active)");
        return Ok(());
    }

    point_symlink(&bin_dir, &target)?;
    println!("active:     {target_name}");
    Ok(())
}

pub(crate) fn run_uninstall(args: &UninstallArgs) -> Result<(), UpdateError> {
    let bin_dir = managed_bin_dir(args.prefix.as_deref())?;
    let active = active_version(&bin_dir);
    let targets: Vec<Version> = if args.all {
        installed_versions(&bin_dir)
    } else if let Some(raw) = args.version.as_deref() {
        vec![parse_version(raw)?]
    } else {
        active
            .as_deref()
            .and_then(|name| name.strip_prefix("faber-"))
            .and_then(|raw| Version::parse(raw).ok())
            .map(|version| vec![version])
            .ok_or_else(|| {
                "no active faber version to remove; pass --version <VERSION> or --all".to_owned()
            })?
    };

    let link = bin_dir.join("faber");
    let mut removed_link = false;
    for version in &targets {
        let name = format!("faber-{version}");
        let path = bin_dir.join(&name);
        match fs::remove_file(&path) {
            Ok(()) => println!("removed {}", path.display()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                println!("missing {}", path.display());
            }
            Err(err) => return Err(format!("cannot remove {}: {err}", path.display()).into()),
        }
        if !args.all && active.as_deref() == Some(name.as_str()) && !removed_link {
            match fs::remove_file(&link) {
                Ok(()) => {
                    println!("removed {}", link.display());
                    removed_link = true;
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(format!("cannot remove {}: {err}", link.display()).into()),
            }
        }
    }

    if args.all && !removed_link {
        match fs::symlink_metadata(&link) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                fs::remove_file(&link)
                    .map_err(|err| format!("cannot remove {}: {err}", link.display()))?;
                println!("removed {}", link.display());
                removed_link = true;
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("cannot inspect {}: {err}", link.display()).into()),
        }
    }

    if targets.is_empty() {
        println!("no installed faber versions in {}", bin_dir.display());
        return Ok(());
    }

    let remaining = installed_versions(&bin_dir);
    if remaining.is_empty() {
        println!("no installed faber versions remain");
    } else {
        let names: Vec<String> = remaining.iter().map(ToString::to_string).collect();
        println!("remaining: {}", names.join(", "));
        if removed_link {
            println!(
                "no active version; point {} at one of the remaining versions",
                link.display()
            );
        }
    }
    Ok(())
}

/// Resolve the newest public `faber-v*` release. Other components in the
/// shared release repository (`radix-v*`, `cista-v*`) and prereleases are
/// ignored, and the winner is chosen by semver rather than list order.
pub(crate) fn latest_release() -> Result<PublicRelease, String> {
    let mut best = None;
    for page in 1..=MAX_RELEASE_PAGES {
        let url = format!("{RELEASES_API}?per_page=100&page={page}");
        let body = http_get(&url)?;
        let releases: Vec<ReleaseJson> = serde_json::from_slice(&body)
            .map_err(|err| format!("cannot parse the release list from {url}: {err}"))?;
        if releases.is_empty() {
            break;
        }
        for release in releases {
            consider_release(&mut best, &release);
        }
    }
    best.ok_or_else(|| format!("no public faber release found in {RELEASES_REPO}"))
}

/// Fold one release row into the current best stable `faber-v*` release.
pub(crate) fn consider_release(best: &mut Option<PublicRelease>, release: &ReleaseJson) {
    if release.draft || release.prerelease {
        return;
    }
    let Some(raw) = release.tag_name.strip_prefix(TAG_PREFIX) else {
        return;
    };
    let Ok(version) = Version::parse(raw) else {
        return;
    };
    // A semver prerelease (for example `faber-v2.0.0-rc.1`) is not a stable
    // public release even when GitHub's prerelease flag is unset.
    if !version.pre.is_empty() {
        return;
    }
    if best
        .as_ref()
        .is_none_or(|current| version > current.version)
    {
        *best = Some(PublicRelease {
            version,
            tag: release.tag_name.clone(),
            url: release.html_url.clone(),
        });
    }
}

fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let mut headers = vec![
        (
            "User-Agent".to_owned(),
            concat!("faber/", env!("CARGO_PKG_VERSION")).to_owned(),
        ),
        (
            "Accept".to_owned(),
            "application/vnd.github+json".to_owned(),
        ),
    ];
    if let Some(token) = github_token() {
        headers.push(("Authorization".to_owned(), format!("Bearer {token}")));
    }
    let response = faber_leaf::http::fetch(url, &headers, MAX_DOWNLOAD_BYTES)
        .map_err(|err| format!("cannot reach {url}: {err}"))?;
    let status = usize::from(response.status);
    if (200..300).contains(&status) {
        return Ok(response.body);
    }
    let detail = String::from_utf8_lossy(&response.body);
    if response.status == 403 || response.status == 429 {
        Err(format!(
            "GitHub rate limit or authorization failure ({}) for {url}; \
             set GITHUB_TOKEN or GH_TOKEN and retry. {detail}",
            response.status
        ))
    } else {
        Err(format!(
            "GET {url} failed with HTTP {}: {detail}",
            response.status
        ))
    }
}

fn github_token() -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
}

/// Read a release asset from an HTTP(S) URL or a local directory path. Local
/// paths keep the updater and its tests usable against a mirror or fixture.
pub(crate) fn fetch_location(location: &str) -> Result<Vec<u8>, UpdateError> {
    if location.contains("://") {
        return Ok(http_get(location)?);
    }
    fs::read(location).map_err(|err| UpdateError {
        status: faber_cli::exit_status::NO_INPUT,
        message: format!("cannot read {location}: {err}"),
    })
}

/// Download, verify, and stage one release binary inside `bin_dir`. Nothing is
/// written beside the target until the checksum matches.
pub(crate) fn install_release(
    base: &str,
    version: &Version,
    triple: &str,
    bin_dir: &Path,
) -> Result<(), UpdateError> {
    let archive_name = format!("faber-v{version}-{triple}.tar.gz");
    let archive_url = format!("{base}/faber-v{version}/{archive_name}");
    let checksum_url = format!("{archive_url}.sha256");
    let archive = fetch_location(&archive_url)?;
    let checksum = fetch_location(&checksum_url)?;
    let expected = parse_checksum(&checksum)?;
    let actual = sha256_hex(&archive);
    if actual != expected {
        return Err(format!(
            "checksum mismatch for {archive_url}\n  expected: {expected}\n  actual:   {actual}\n  the downloaded archive was not installed"
        )
        .into());
    }

    fs::create_dir_all(bin_dir).map_err(|err| {
        UpdateError::cant_create(format!("cannot create {}: {err}", bin_dir.display()))
    })?;
    let staged = bin_dir.join(format!(".faber-{version}.tmp"));
    fs::remove_file(&staged).ok();
    extract_bin(&archive, &staged)?;
    let target = bin_dir.join(format!("faber-{version}"));
    if let Err(err) = fs::rename(&staged, &target) {
        fs::remove_file(&staged).ok();
        return Err(UpdateError::cant_create(format!(
            "cannot install {}: {err}",
            target.display()
        )));
    }
    Ok(())
}

/// Extract exactly `bin/faber` from the release archive.
pub(crate) fn extract_bin(archive: &[u8], dest: &Path) -> Result<(), UpdateError> {
    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(archive));
    let mut tar = tar::Archive::new(decoder);
    let entries = tar
        .entries()
        .map_err(|err| format!("cannot read the release archive: {err}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|err| format!("cannot read the release archive: {err}"))?;
        let path = entry
            .path()
            .map_err(|err| format!("bad path in the release archive: {err}"))?
            .into_owned();
        let path = path.strip_prefix("./").unwrap_or(&path);
        if path != Path::new("bin/faber") {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err("the release archive's bin/faber is not a regular file"
                .to_owned()
                .into());
        }
        let mut file = fs::File::create(dest).map_err(|err| {
            UpdateError::cant_create(format!("cannot create {}: {err}", dest.display()))
        })?;
        std::io::copy(&mut entry, &mut file)
            .map_err(|err| UpdateError::cant_create(format!("cannot extract bin/faber: {err}")))?;
        set_executable(dest)?;
        return Ok(());
    }
    Err("the release archive does not contain bin/faber"
        .to_owned()
        .into())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|err| {
        UpdateError::cant_create(format!("cannot make {} executable: {err}", path.display()))
    })
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), UpdateError> {
    Ok(())
}

/// Point `<bin_dir>/faber` at `faber-<version>` with an atomic rename of a
/// freshly created symlink. The target is relative so the prefix can move.
#[cfg(unix)]
pub(crate) fn point_symlink(bin_dir: &Path, version: &Version) -> Result<(), UpdateError> {
    let link = bin_dir.join("faber");
    let staged = bin_dir.join(".faber.link.tmp");
    fs::remove_file(&staged).ok();
    std::os::unix::fs::symlink(format!("faber-{version}"), &staged).map_err(|err| {
        UpdateError::cant_create(format!("cannot create the faber symlink: {err}"))
    })?;
    if let Err(err) = fs::rename(&staged, &link) {
        fs::remove_file(&staged).ok();
        return Err(UpdateError::cant_create(format!(
            "cannot point {} at faber-{version}: {err}",
            link.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn point_symlink(_bin_dir: &Path, _version: &Version) -> Result<(), UpdateError> {
    Err("faber updates require a Unix filesystem (symlinks)"
        .to_owned()
        .into())
}

/// The active version's file name, read from the `faber` symlink.
pub(crate) fn active_version(bin_dir: &Path) -> Option<String> {
    fs::read_link(bin_dir.join("faber"))
        .ok()
        .and_then(|target| target.file_name()?.to_str().map(str::to_owned))
}

/// Installed versioned binaries, newest first.
pub(crate) fn installed_versions(bin_dir: &Path) -> Vec<Version> {
    let mut versions = Vec::new();
    let Ok(entries) = fs::read_dir(bin_dir) else {
        return versions;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(raw) = name.strip_prefix("faber-") else {
            continue;
        };
        let Ok(version) = Version::parse(raw) else {
            continue;
        };
        if entry.path().is_file() {
            versions.push(version);
        }
    }
    versions.sort();
    versions.reverse();
    versions
}

/// Resolve the directory that holds `faber-<version>` and the `faber` symlink.
/// Without an explicit prefix the running binary must be part of a managed
/// install (a `faber` symlink beside it); a cargo-installed or ad-hoc binary
/// is refused instead of silently replaced.
pub(crate) fn resolve_bin_dir(
    prefix: Option<&Path>,
    exe: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(prefix) = prefix {
        return Ok(prefix.join("bin"));
    }
    let exe = exe.ok_or_else(|| "cannot locate the running faber binary".to_owned())?;
    let parent = exe
        .parent()
        .ok_or_else(|| format!("cannot derive an install directory from {}", exe.display()))?;
    let link = parent.join("faber");
    if fs::symlink_metadata(&link).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Ok(parent.to_path_buf());
    }
    Err(format!(
        "no managed faber install found at {}\n  cause: the running binary ({}) is not a versioned install\n  next: reinstall with install.sh, or pass --prefix <DIR>",
        parent.display(),
        exe.display()
    ))
}

fn managed_bin_dir(prefix: Option<&Path>) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().ok();
    resolve_bin_dir(prefix, exe.as_deref())
}

fn current_version() -> Result<Version, String> {
    parse_version(env!("CARGO_PKG_VERSION"))
}

fn parse_version(raw: &str) -> Result<Version, String> {
    Version::parse(raw).map_err(|err| format!("{raw} is not a semver version: {err}"))
}

/// The two triples that publish a prebuilt faber binary.
fn current_triple() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        (os, arch) => Err(format!(
            "no published faber binary for {arch}-{os}; releases support \
             aarch64-apple-darwin and x86_64-unknown-linux-gnu"
        )),
    }
}

pub(crate) fn parse_checksum(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|err| format!("the checksum file is not UTF-8: {err}"))?;
    let token = text
        .split_whitespace()
        .next()
        .ok_or_else(|| "the checksum file is empty".to_owned())?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "the checksum file does not start with a SHA-256 digest: {token:?}"
        ));
    }
    Ok(token.to_ascii_lowercase())
}
