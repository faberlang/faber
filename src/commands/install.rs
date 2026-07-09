use crate::cli::InstallArgs;
use crate::library::FABER_LIBRARY_HOME_ENV;
use crate::package::read_manifest;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn cmd_install(args: InstallArgs) {
    let Some(home) = std::env::var_os(FABER_LIBRARY_HOME_ENV).map(PathBuf::from) else {
        eprintln!("error: FABER_LIBRARY_HOME is required for `faber install`");
        std::process::exit(1);
    };

    if let Err(err) = std::fs::create_dir_all(&home) {
        eprintln!(
            "error: failed to create FABER_LIBRARY_HOME at {}: {err}",
            home.display()
        );
        std::process::exit(1);
    }

    match install_library(&args.library, home) {
        Ok(report) => {
            println!(
                "installed {} at {}",
                report.provider,
                report.target.display()
            );
        }
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InstallReport {
    pub(super) provider: String,
    pub(super) target: PathBuf,
}

#[derive(Debug)]
pub(super) enum InstallError {
    InvalidLibraryName(String),
    Io {
        path: PathBuf,
        message: String,
    },
    GitFailed {
        action: &'static str,
        message: String,
    },
    InvalidManifest {
        path: PathBuf,
        message: String,
    },
    NotLibraryPackage {
        path: PathBuf,
        kind: String,
    },
    MissingLibraryTable {
        path: PathBuf,
    },
    MissingSourceRoot {
        path: PathBuf,
    },
    ConflictingInstall {
        target: PathBuf,
        reason: String,
    },
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::InvalidLibraryName(name) => write!(
                f,
                "invalid library name `{name}`; use ASCII letters, numbers, underscore, or hyphen"
            ),
            InstallError::Io { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
            InstallError::GitFailed { action, message } => {
                write!(f, "failed to {action} for `faber install`: {message}")
            }
            InstallError::InvalidManifest { path, message } => {
                write!(f, "invalid library manifest at {}: {message}", path.display())
            }
            InstallError::NotLibraryPackage { path, kind } => write!(
                f,
                "{} is not an installable Faber source library: build.kind is `{kind}`, expected `lib`",
                path.display()
            ),
            InstallError::MissingLibraryTable { path } => write!(
                f,
                "{} is not an installable Faber source library: missing [library]",
                path.display()
            ),
            InstallError::MissingSourceRoot { path } => {
                write!(f, "installed library source root does not exist: {}", path.display())
            }
            InstallError::ConflictingInstall { target, reason } => {
                write!(f, "refusing to update existing install at {}: {reason}", target.display())
            }
        }
    }
}

pub(super) fn install_library(
    library_or_url: &str,
    home: PathBuf,
) -> Result<InstallReport, InstallError> {
    if looks_like_library_name(library_or_url) && !valid_library_name(library_or_url) {
        return Err(InstallError::InvalidLibraryName(library_or_url.to_owned()));
    }

    std::fs::create_dir_all(&home).map_err(|err| InstallError::Io {
        path: home.clone(),
        message: err.to_string(),
    })?;

    let source = install_source(library_or_url)?;
    let checkout = temp_checkout_dir(&home, library_or_url);
    git_clone(&source, &checkout)?;
    let manifest_path = checkout.join("faber.toml");
    let install = read_install_manifest(&manifest_path)?;
    let target = home.join(&install.provider);

    if target.exists() {
        let result = validate_existing_install(&target, &source, &install).and_then(|()| {
            std::fs::remove_dir_all(&checkout).map_err(|err| InstallError::Io {
                path: checkout.clone(),
                message: err.to_string(),
            })?;
            git_pull(&target)
        });
        result?;
        return Ok(InstallReport {
            provider: install.provider,
            target,
        });
    }

    std::fs::rename(&checkout, &target).map_err(|err| InstallError::Io {
        path: target.clone(),
        message: err.to_string(),
    })?;

    Ok(InstallReport {
        provider: install.provider,
        target,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallManifest {
    provider: String,
    package_name: String,
}

fn read_install_manifest(path: &std::path::Path) -> Result<InstallManifest, InstallError> {
    let package_root = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    crate::package::discover_package(package_root).map_err(|diag| {
        InstallError::InvalidManifest {
            path: path.to_path_buf(),
            message: diag.message,
        }
    })?;
    let manifest = read_manifest(path).map_err(|diag| InstallError::InvalidManifest {
        path: path.to_path_buf(),
        message: diag.message,
    })?;
    if manifest.build.kind != "lib" {
        return Err(InstallError::NotLibraryPackage {
            path: path.to_path_buf(),
            kind: manifest.build.kind,
        });
    }
    let Some(library) = manifest.library else {
        return Err(InstallError::MissingLibraryTable {
            path: path.to_path_buf(),
        });
    };
    let source_root = package_root.join(&manifest.paths.source);
    if !source_root.is_dir() {
        return Err(InstallError::MissingSourceRoot { path: source_root });
    }
    Ok(InstallManifest {
        provider: library.provider,
        package_name: manifest.package.name,
    })
}

fn validate_existing_install(
    target: &std::path::Path,
    source: &str,
    incoming: &InstallManifest,
) -> Result<(), InstallError> {
    let existing = read_install_manifest(&target.join("faber.toml"))?;
    if existing != *incoming {
        return Err(InstallError::ConflictingInstall {
            target: target.to_path_buf(),
            reason: format!(
                "existing package identity {}:{}, incoming {}:{}",
                existing.provider, existing.package_name, incoming.provider, incoming.package_name
            ),
        });
    }

    let existing_remote = git_origin_url(target)?;
    if existing_remote != source {
        return Err(InstallError::ConflictingInstall {
            target: target.to_path_buf(),
            reason: format!("existing remote `{existing_remote}` differs from `{source}`"),
        });
    }
    Ok(())
}

fn install_source(input: &str) -> Result<String, InstallError> {
    if std::path::Path::new(input).exists() || input.contains("://") || input.starts_with("git@") {
        return Ok(input.to_owned());
    }
    if !valid_library_name(input) {
        return Err(InstallError::InvalidLibraryName(input.to_owned()));
    }
    Ok(format!("https://github.com/faberlang/{input}.git"))
}

fn looks_like_library_name(input: &str) -> bool {
    !input.contains("://") && !input.starts_with("git@") && !std::path::Path::new(input).exists()
}

fn git_clone(source: &str, target: &std::path::Path) -> Result<(), InstallError> {
    let output = Command::new("git")
        .arg("clone")
        .arg(source)
        .arg(target)
        .output()
        .map_err(|err| InstallError::GitFailed {
            action: "run git clone",
            message: err.to_string(),
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(InstallError::GitFailed {
        action: "clone",
        message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn git_pull(target: &std::path::Path) -> Result<(), InstallError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(target)
        .arg("pull")
        .arg("--ff-only")
        .output()
        .map_err(|err| InstallError::GitFailed {
            action: "run git pull",
            message: err.to_string(),
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(InstallError::GitFailed {
        action: "pull",
        message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn git_origin_url(target: &std::path::Path) -> Result<String, InstallError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .map_err(|err| InstallError::GitFailed {
            action: "inspect git remote",
            message: err.to_string(),
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    }
    Err(InstallError::GitFailed {
        action: "inspect git remote",
        message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn temp_checkout_dir(home: &std::path::Path, input: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let stem = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    home.join(format!(".faber-install-{stem}-{nanos}"))
}

fn valid_library_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}
