//! `faber self update` — upgrade the installed faber to a newer released version.
//!
//! The update engine is the channel's verified bootstrap (`scripta/install-faber`,
//! published at the release tag per the R1 interlock): it downloads a FIXED
//! release payload and verifies SHA-256 before any unpack or execution, installs
//! the new version as a side-by-side lane (`<prefix>/versions/<version>/`),
//! preserves the current install as a lane, flips the active launcher/receipt,
//! and leaves user projects and the package store untouched (faber-onboarding
//! Stage 3, unit A2). This command locates the install prefix from the running
//! binary, reads the install receipt, and re-runs the bootstrap with the update
//! flags.
//!
//! This is a channel operation: like the `curl | python3` bootstrap it uses the
//! release host as its only source (never a second release system, CAMPAIGN
//! Stage 3 overlap rule) and it needs the channel runtime — `python3` to run
//! the bootstrap, `curl` to fetch the published script. Version-lane and
//! downgrade guards are enforced by the engine (single source of truth); this
//! command forwards `--allow-lane-change` and fails closed on its own errors.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Arguments for `faber self`.
#[derive(clap::Args, Debug)]
pub struct SelfManageArgs {
    /// Subcommand for managing this faber installation.
    #[command(subcommand)]
    pub command: SelfCommand,
}

/// `faber self` subcommands.
#[derive(clap::Subcommand, Debug)]
pub enum SelfCommand {
    /// Update the installed faber to a newer released version
    Update(SelfUpdateArgs),
}

/// Arguments for `faber self update`.
#[derive(clap::Args, Debug)]
pub struct SelfUpdateArgs {
    /// Target release version to upgrade to (e.g. 1.6.0)
    #[arg(long, value_name = "VERSION")]
    pub version: String,

    /// Install prefix (default: discovered from this binary's install receipt)
    #[arg(long, value_name = "PATH")]
    pub prefix: Option<PathBuf>,

    /// Asset base URL or directory (default: from the install receipt)
    #[arg(long = "base-url", value_name = "URL")]
    pub base_url: Option<String>,

    /// Explicitly allow crossing the odd dev / even LTS version lane
    #[arg(long)]
    pub allow_lane_change: bool,
}

/// Installed-receipt path relative to the install prefix.
pub(crate) const RECEIPT_REL: &str = "share/faber/install-receipt.json";
/// Name of the published bootstrap script beside the release archives.
const INSTALLER_SCRIPT: &str = "install-faber";

/// The resolved update operation: what to fetch and how to run it.
#[derive(Debug)]
pub(crate) struct SelfUpdatePlan {
    pub prefix: PathBuf,
    pub current_version: String,
    pub target_version: String,
    pub triple: String,
    pub base_url: String,
    pub script_url: String,
    pub allow_lane_change: bool,
}

/// Dispatch `faber self <subcommand>`.
pub(super) fn cmd_self(args: &SelfManageArgs) {
    match &args.command {
        SelfCommand::Update(args) => cmd_self_update(args),
    }
}

pub(super) fn cmd_self_update(args: &SelfUpdateArgs) {
    let exe = std::env::current_exe().ok();
    let cwd = std::env::current_dir().ok();
    let plan = match plan_self_update(args, exe.as_deref(), cwd.as_deref()) {
        Ok(plan) => plan,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };
    execute_self_update(&plan);
}

/// Pure resolution core (injectable exe/cwd for tests): locate the prefix,
/// read the receipt, and derive the bootstrap invocation.
pub(crate) fn plan_self_update(
    args: &SelfUpdateArgs,
    exe: Option<&Path>,
    cwd: Option<&Path>,
) -> Result<SelfUpdatePlan, String> {
    if args.version.trim().is_empty() {
        return Err("--version is required: faber self update --version <version>".to_owned());
    }
    let prefix = resolve_prefix(args.prefix.as_deref(), exe, cwd)?;
    let receipt = read_receipt(&prefix)?;
    let current_version = receipt
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("install receipt at {} has no version", prefix.display()))?;
    let triple = receipt
        .get("triple")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("install receipt at {} has no triple", prefix.display()))?;
    let source = receipt
        .get("source")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("install receipt at {} has no source", prefix.display()))?;
    let base_url = if let Some(url) = args.base_url.as_deref() {
        url.to_owned()
    } else {
        receipt
            .get("assetBase")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| derive_base_url(source, &args.version))
    };
    Ok(SelfUpdatePlan {
        prefix,
        current_version: current_version.to_owned(),
        target_version: args.version.clone(),
        triple: triple.to_owned(),
        script_url: format!("{base_url}/{INSTALLER_SCRIPT}"),
        base_url,
        allow_lane_change: args.allow_lane_change,
    })
}

/// Locate the install prefix: explicit `--prefix`, else the receipt found
/// relative to the running binary, else the user-local default `~/.faber`.
pub(crate) fn resolve_prefix(
    explicit: Option<&Path>,
    exe: Option<&Path>,
    _cwd: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Some(exe_path) = exe {
        if let Some(bin_dir) = exe_path.parent() {
            // `<prefix>/bin/faber` resolves its receipt at `<prefix>/share/faber/`.
            if let Some(prefix) = bin_dir.parent() {
                if prefix.join(RECEIPT_REL).is_file() {
                    return Ok(prefix.to_path_buf());
                }
            }
        }
    }
    let default = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".faber");
    if default.join(RECEIPT_REL).is_file() {
        return Ok(default);
    }
    Err(format!(
        "no faber install found for this binary \
         (no receipt at {}); install first via scripta/install-faber or pass --prefix",
        default.join(RECEIPT_REL).display()
    ))
}

/// Read and parse the install receipt at `<prefix>/share/faber/install-receipt.json`.
pub(crate) fn read_receipt(prefix: &Path) -> Result<serde_json::Value, String> {
    let path = prefix.join(RECEIPT_REL);
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("cannot read install receipt {}: {err}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|err| format!("install receipt {} is not valid JSON: {err}", path.display()))
}

/// Derive the asset base for the TARGET version from the recorded source URL.
///
/// `source` is `<base>/<archive>`. When the base's last segment is a version
/// tag directory (`faber-v<version>`, the release-host shape), the tag is
/// replaced with the target version; flat directories and mirrors that serve
/// every version beside each other pass through unchanged.
pub(crate) fn derive_base_url(source: &str, target_version: &str) -> String {
    let Some((base, _archive)) = source.rsplit_once('/') else {
        return source.to_owned();
    };
    if let Some((host, tag)) = base.rsplit_once('/') {
        if tag.starts_with("faber-v") {
            return format!("{host}/faber-v{target_version}");
        }
    }
    base.to_owned()
}

/// Fetch the published bootstrap script and run it under `python3` with the
/// update flags; the engine enforces checksum-before-exec, lane guards, and
/// rollback. The script's stdout/stderr pass through; its exit code is ours.
fn execute_self_update(plan: &SelfUpdatePlan) {
    println!(
        "faber self update — {} -> {} ({})",
        plan.current_version, plan.target_version, plan.triple
    );
    println!("  prefix:     {}", plan.prefix.display());
    println!("  engine:     fetching {INSTALLER_SCRIPT} from {}", plan.script_url);

    let script = match fetch_installer_script(&plan.script_url) {
        Ok(bytes) => bytes,
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("  offline or unreachable? retry, or use scripta/install-faber directly");
            std::process::exit(1);
        }
    };

    let mut args_vec: Vec<String> = vec![
        "-".to_owned(),
        "--version".to_owned(),
        plan.target_version.clone(),
        "--prefix".to_owned(),
        plan.prefix.display().to_string(),
        "--triple".to_owned(),
        plan.triple.clone(),
        "--base-url".to_owned(),
        plan.base_url.clone(),
        "--update".to_owned(),
    ];
    if plan.allow_lane_change {
        args_vec.push("--allow-lane-change".to_owned());
    }

    let mut child = match Command::new("python3")
        .args(&args_vec)
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!(
                "error: cannot run the faber installer (python3): {err}\n\
                 hint: the faber install channel runs on python3; the update engine \
                 is the published scripta/install-faber"
            );
            std::process::exit(1);
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(err) = stdin.write_all(&script) {
            eprintln!("error: cannot feed the installer script: {err}");
        }
        drop(stdin);
    }
    let status = match child.wait() {
        Ok(status) => status,
        Err(err) => {
            eprintln!("error: installer run failed: {err}");
            std::process::exit(1);
        }
    };
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Fetch the published installer script. URL bases go through `curl` (the
/// channel runtime); local directory bases are read directly, mirroring the
/// engine's fetch behavior for mirrors and test hosts.
fn fetch_installer_script(script_url: &str) -> Result<Vec<u8>, String> {
    if !script_url.contains("://") {
        let path = Path::new(script_url);
        return std::fs::read(path)
            .map_err(|err| format!("cannot read {INSTALLER_SCRIPT} from {}: {err}", path.display()));
    }
    let output = Command::new("curl")
        .args(["-fsSL", script_url])
        .output()
        .map_err(|err| {
            format!("cannot fetch {INSTALLER_SCRIPT} from {script_url}: {err}")
        })?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(format!(
        "failed to download {INSTALLER_SCRIPT} from {script_url} ({})",
        output.status
    ))
}
