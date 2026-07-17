use crate::cli::InstallArgs;
use crate::library::FABER_LIBRARY_HOME_ENV;
use crate::package::read_manifest;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn cmd_install(args: InstallArgs) {
    if let Some(path) = args.path.as_ref() {
        match install_store_path(
            path,
            args.store.as_deref(),
            args.project.as_deref(),
            &args.target_language,
        ) {
            Ok(()) => {}
            Err(errors) => {
                for err in errors {
                    eprintln!("error: {err}");
                }
                std::process::exit(1);
            }
        }
        return;
    }

    let Some(library) = args.library.as_ref() else {
        eprintln!("error: `faber install` requires a library name/URL or --path");
        std::process::exit(1);
    };

    if args.legacy_library_home {
        let Some(home) = std::env::var_os(FABER_LIBRARY_HOME_ENV).map(PathBuf::from) else {
            eprintln!(
                "error: FABER_LIBRARY_HOME is required for `faber install --legacy-library-home <name|url>`\n\
                 hint: omit --legacy-library-home to install cista.toml packages into the Cista store"
            );
            std::process::exit(1);
        };

        if let Err(err) = std::fs::create_dir_all(&home) {
            eprintln!(
                "error: failed to create FABER_LIBRARY_HOME at {}: {err}",
                home.display()
            );
            std::process::exit(1);
        }

        match install_library(library, home) {
            Ok(report) => {
                println!(
                    "installed {} at {} (legacy FABER_LIBRARY_HOME source-library path)",
                    report.provider,
                    report.target.display()
                );
            }
            Err(err) => {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    match install_store_source(
        library,
        args.store.as_deref(),
        args.project.as_deref(),
        &args.target_language,
    ) {
        Ok(()) => {}
        Err(errors) => {
            for err in errors {
                eprintln!("error: {err}");
            }
            std::process::exit(1);
        }
    }
}

/// Product composition path: install a local `cista.toml` package into the store.
fn install_store_path(
    path: &Path,
    store: Option<&Path>,
    project: Option<&Path>,
    target_language: &str,
) -> Result<(), Vec<String>> {
    let path = path
        .canonicalize()
        .map_err(|err| vec![format!("{}: {err}", path.display())])?;
    install_store_cista_path(path, store, project, target_language)
}

/// Product composition path: clone a git/name source, require cista.toml, then install it.
pub(super) fn install_store_source(
    library_or_url: &str,
    store: Option<&Path>,
    project: Option<&Path>,
    target_language: &str,
) -> Result<(), Vec<String>> {
    if looks_like_library_name(library_or_url) && !valid_library_name(library_or_url) {
        return Err(vec![InstallError::InvalidLibraryName(
            library_or_url.to_owned(),
        )
        .to_string()]);
    }

    let source = install_source(library_or_url).map_err(|err| vec![err.to_string()])?;
    let checkout_parent = std::env::temp_dir().join("faber-install-checkouts");
    std::fs::create_dir_all(&checkout_parent).map_err(|err| {
        vec![format!(
            "failed to create temporary install checkout directory {}: {err}",
            checkout_parent.display()
        )]
    })?;
    let checkout =
        temp_checkout_dir(&checkout_parent, library_or_url).map_err(|err| vec![err.to_string()])?;
    git_clone(&source, &checkout).map_err(|err| vec![err.to_string()])?;

    let result = if checkout.join("cista.toml").is_file() {
        install_store_cista_path(checkout.clone(), store, project, target_language)
    } else {
        Err(vec![format!(
            "{} has no cista.toml; `faber install <git-url>` installs Cista packages only (use --legacy-library-home for old faber.toml source-library clones)",
            checkout.display()
        )])
    };

    match std::fs::remove_dir_all(&checkout) {
        Ok(()) => result,
        Err(cleanup_error) => match result {
            Ok(()) => Err(vec![format!(
                "installed package but failed to remove temporary checkout {}: {cleanup_error}",
                checkout.display()
            )]),
            Err(mut errors) => {
                errors.push(format!(
                    "failed to remove temporary checkout {}: {cleanup_error}",
                    checkout.display()
                ));
                Err(errors)
            }
        },
    }
}

fn install_store_cista_path(
    path: PathBuf,
    store: Option<&Path>,
    project: Option<&Path>,
    target_language: &str,
) -> Result<(), Vec<String>> {
    cista::install(cista::cli::InstallArgs {
        path: Some(path),
        package: None,
        manifest: PathBuf::from("cista.toml"),
        target_language: target_language.to_owned(),
        store: store.map(Path::to_path_buf),
        registry: None,
        project: project.map(Path::to_path_buf),
        verify_target_build: false,
    })
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
    let checkout = temp_checkout_dir(&home, library_or_url)?;
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

fn temp_checkout_dir(home: &std::path::Path, input: &str) -> Result<PathBuf, InstallError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| InstallError::Io {
            path: home.to_path_buf(),
            message: format!("failed to derive temporary checkout name from system clock: {err}"),
        })?
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
    Ok(home.join(format!(".faber-install-{stem}-{nanos}")))
}

fn valid_library_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}
