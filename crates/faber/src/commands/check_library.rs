//! `faber check-library <name>` — named-library interface-graph check.
//!
//! v1 resolves `<name>` from the library home only and mirrors the import
//! resolver's root rule (`faber.toml` else `<home>/<name>/src`). It does not
//! parse `cista.toml`, does not discover store/lock roots, and does not
//! re-derive `LibraryProvider` from a manifest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use radix::module::codegen::Target;
use radix::module::diagnostics::Diagnostic;
use radix::program::discovery::PackageSpec;
use radix::program::library::is_valid_provider_segment;

use crate::cli::CheckLibraryArgs;
use faber_cli::library::{
    FABER_LIBRARY_HOME_ENV, LibraryRootRule, ResolvedLibraryHome, library_root_rule,
    resolved_library_home, specifier_identity,
};

/// Successful name→checkout resolution. Identity is not stored: callers that
/// need it must use [`specifier_identity`] on `name`.
#[derive(Debug)]
pub(crate) struct DevLibraryCheckout {
    pub name: String,
    pub home: ResolvedLibraryHome,
    pub provider_repo: PathBuf,
    pub source_root: PathBuf,
    pub root_rule: LibraryRootRule,
    pub spec: PackageSpec,
}

#[derive(Debug)]
pub(crate) enum CheckLibraryError {
    InvalidName {
        name: String,
    },
    MissingLibraryHome,
    UnknownProvider {
        name: String,
        home: PathBuf,
    },
    MissingSourceRoot {
        name: String,
        provider_repo: PathBuf,
    },
    NotALibrary {
        name: String,
        kind: String,
    },
    Diagnostic(Box<Diagnostic>),
}

impl CheckLibraryError {
    fn into_message(self) -> String {
        match self {
            Self::InvalidName { name } => {
                format!("error: '{name}' is not a valid library provider name")
            }
            Self::MissingLibraryHome => {
                format!(
                    "error: no library home; set {FABER_LIBRARY_HOME_ENV} to a directory containing provider checkouts"
                )
            }
            Self::UnknownProvider { name, home } => {
                format!(
                    "error: library '{name}' not found under library home {}",
                    home.display()
                )
            }
            Self::MissingSourceRoot {
                name,
                provider_repo,
            } => {
                format!(
                    "error: library '{name}' at {} has neither faber.toml nor src/",
                    provider_repo.display()
                )
            }
            Self::NotALibrary { name, kind } => {
                format!(
                    "error: '{name}' is a {kind} package, not a library; use `faber check` for application packages"
                )
            }
            Self::Diagnostic(diag) => format!("error: {}", diag.message),
        }
    }
}

fn validate_provider_name(name: &str) -> Result<(), CheckLibraryError> {
    if is_valid_provider_segment(name) {
        Ok(())
    } else {
        Err(CheckLibraryError::InvalidName {
            name: name.to_owned(),
        })
    }
}

/// Resolve `<name>` from the library home using the resolver root rule.
pub(crate) fn resolve_dev_library(name: &str) -> Result<DevLibraryCheckout, CheckLibraryError> {
    validate_provider_name(name)?;
    let home = resolved_library_home().ok_or(CheckLibraryError::MissingLibraryHome)?;
    resolve_dev_library_from(name, home)
}

pub(crate) fn resolve_dev_library_from(
    name: &str,
    home: ResolvedLibraryHome,
) -> Result<DevLibraryCheckout, CheckLibraryError> {
    validate_provider_name(name)?;
    let provider_repo = home.path.join(name);
    if !provider_repo.is_dir() {
        return Err(CheckLibraryError::UnknownProvider {
            name: name.to_owned(),
            home: home.path,
        });
    }
    let root_rule =
        library_root_rule(&provider_repo).ok_or_else(|| CheckLibraryError::MissingSourceRoot {
            name: name.to_owned(),
            provider_repo: provider_repo.clone(),
        })?;
    let spec = spec_for_root_rule(name, &provider_repo, root_rule)?;
    Ok(DevLibraryCheckout {
        name: name.to_owned(),
        home,
        provider_repo,
        source_root: spec.source_root.clone(),
        root_rule,
        spec,
    })
}

fn spec_for_root_rule(
    name: &str,
    provider_repo: &Path,
    root_rule: LibraryRootRule,
) -> Result<PackageSpec, CheckLibraryError> {
    match root_rule {
        LibraryRootRule::FaberToml => {
            let manifest_path = provider_repo.join("faber.toml");
            let manifest = faber_cli::package::read_manifest(&manifest_path)
                .map_err(CheckLibraryError::Diagnostic)?;
            if manifest.build.kind != "lib" {
                return Err(CheckLibraryError::NotALibrary {
                    name: name.to_owned(),
                    kind: manifest.build.kind,
                });
            }
            let mut spec = faber_cli::package::discover_package(provider_repo)
                .map_err(CheckLibraryError::Diagnostic)?;
            // Full interface graph: every `*.fab` under the source root, not
            // an application entry. This is not `faber check` package mode.
            spec.entry = spec.source_root.clone();
            Ok(spec)
        }
        LibraryRootRule::SrcFallback => {
            let source_root = provider_repo.join("src");
            Ok(PackageSpec {
                package_root: provider_repo.to_path_buf(),
                source_root: source_root.clone(),
                entry: source_root,
                templates: BTreeMap::new(),
                manifest_backed: false,
            })
        }
    }
}

pub(crate) fn render_checkout(checkout: &DevLibraryCheckout) -> String {
    format!(
        "library_home={}\nlibrary_home_source={}\nname={}\nidentity={}\nroot={}\nroot_rule={}\n",
        checkout.home.path.display(),
        checkout.home.source.as_key(),
        checkout.name,
        specifier_identity(&checkout.name).as_key(),
        checkout.source_root.display(),
        checkout.root_rule.as_key(),
    )
}

fn check_target(
    provider_repo: &Path,
    root_rule: LibraryRootRule,
) -> Result<Target, Box<Diagnostic>> {
    if root_rule != LibraryRootRule::FaberToml {
        return Ok(Target::HirRust);
    }
    match faber_cli::package::manifest_build_target_for_input(provider_repo)? {
        Some(target) => Ok(target),
        None => Ok(Target::HirRust),
    }
}

/// Analyze the library interface graph. Does not include `.proba`.
pub(crate) fn check_dev_library(checkout: &DevLibraryCheckout) -> Vec<Diagnostic> {
    let locale_input = match checkout.root_rule {
        LibraryRootRule::FaberToml => checkout.provider_repo.as_path(),
        LibraryRootRule::SrcFallback => checkout.source_root.as_path(),
    };
    let target = match check_target(&checkout.provider_repo, checkout.root_rule) {
        Ok(target) => target,
        Err(diag) => return vec![*diag],
    };
    let (config, _locale_pack) =
        match faber_cli::package::config_with_locale(target, locale_input, None, None) {
            Ok(pair) => pair,
            Err(diag) => return vec![*diag],
        };
    let resolver = match faber_cli::package::library_resolver_for_package(
        &config,
        &checkout.spec.package_root,
    ) {
        Ok(resolver) => resolver,
        Err(diagnostics) => return diagnostics,
    };
    match radix::program::analyze::analyze_package_spec(
        &config,
        checkout.spec.clone(),
        &resolver,
        false,
        None,
        &faber_cli::package::faber_analysis_seams(),
    ) {
        Ok(program) => program.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

pub(super) fn cmd_check_library(args: CheckLibraryArgs) {
    let checkout = match resolve_dev_library(&args.name) {
        Ok(checkout) => checkout,
        Err(err) => {
            eprintln!("{}", err.into_message());
            std::process::exit(1);
        }
    };
    print!("{}", render_checkout(&checkout));

    let diagnostics = check_dev_library(&checkout);
    let mut fatal = 0usize;
    for diag in &diagnostics {
        eprintln!("{}", radix::module::diagnostics::render_plain(diag));
        if diag.is_error() {
            fatal += 1;
        }
    }
    if fatal == 0 {
        eprintln!("ok: {}", checkout.name);
    } else {
        std::process::exit(1);
    }
}
