use crate::cli::InstallArgs;
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

    match install_store_source(
        library,
        args.store.as_deref(),
        args.registry.as_deref(),
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

/// Product composition path: install an exact registry pin or git URL through the Cista store.
pub(super) fn install_store_source(
    library_or_url: &str,
    store: Option<&Path>,
    registry: Option<&Path>,
    project: Option<&Path>,
    target_language: &str,
) -> Result<(), Vec<String>> {
    if looks_like_registry_pin(library_or_url) {
        return install_store_registry_package(
            library_or_url,
            store,
            registry,
            project,
            target_language,
        );
    }

    if looks_like_library_name(library_or_url) {
        return Err(vec![InstallError::UnpinnedRegistryPackage(
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
            "{} has no cista.toml; `faber install <git-url>` installs Cista packages only",
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

fn install_store_registry_package(
    package: &str,
    store: Option<&Path>,
    registry: Option<&Path>,
    project: Option<&Path>,
    target_language: &str,
) -> Result<(), Vec<String>> {
    cista::install(&cista::cli::InstallArgs {
        path: None,
        package: Some(package.to_owned()),
        manifest: PathBuf::from("cista.toml"),
        target_language: target_language.to_owned(),
        store: store.map(Path::to_path_buf),
        registry: registry.map(Path::to_path_buf),
        project: project.map(Path::to_path_buf),
        verify_target_build: false,
    })
}

fn install_store_cista_path(
    path: PathBuf,
    store: Option<&Path>,
    project: Option<&Path>,
    target_language: &str,
) -> Result<(), Vec<String>> {
    cista::install(&cista::cli::InstallArgs {
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

#[derive(Debug)]
pub(super) enum InstallError {
    InvalidLibraryName(String),
    GitFailed {
        action: &'static str,
        message: String,
    },
    UnpinnedRegistryPackage(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::InvalidLibraryName(name) => write!(
                f,
                "invalid library name `{name}`; use ASCII letters, numbers, underscore, or hyphen"
            ),
            InstallError::GitFailed { action, message } => {
                write!(f, "failed to {action} for `faber install`: {message}")
            }
            InstallError::UnpinnedRegistryPackage(name) => write!(
                f,
                "registry install requires an exact name@version pin, got `{name}`; pass --registry or set CISTA_REGISTRY when installing a registry package"
            ),
        }
    }
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

fn looks_like_registry_pin(input: &str) -> bool {
    looks_like_library_name(input) && input.split_once('@').is_some()
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

fn temp_checkout_dir(home: &std::path::Path, input: &str) -> Result<PathBuf, InstallError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| InstallError::GitFailed {
            action: "create temporary checkout name",
            message: err.to_string(),
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
