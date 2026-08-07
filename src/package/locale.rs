use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use radix::driver::Config;
use radix::locale::LocalePack;
use std::path::{Path, PathBuf};

use super::discovery::discover_package;
use super::manifest::{manifest_for_spec, FaberManifest};
use super::paths::normalize_path;
use super::PackageSpec;

pub(crate) const DEFAULT_CODE_LOCALE: &str = "en";

/// Build the product-default code config for inputs without package context.
pub(crate) fn default_config_with_locale(target: Target) -> Result<Config, Box<Diagnostic>> {
    let pack_path = installed_locale_pack_path(DEFAULT_CODE_LOCALE);
    let pack = LocalePack::from_toml_path(&pack_path).map_err(|err| {
        Box::new(crate::package_diagnostic_error(format!(
            "failed to load default code locale '{}' pack '{}': {err}\n\
             next action: reinstall the matching faber dev kit so the pack ships at \
             share/faber/locale/{}/pack.toml beside the faber binary",
            DEFAULT_CODE_LOCALE,
            pack_path.display(),
            DEFAULT_CODE_LOCALE
        )))
    })?;
    Ok(Config::default().with_target(target).with_locale_pack(pack))
}

/// Build a driver config (code locale) and the pack used for diagnostic rendering.
///
/// Code locale (`cli_locale` / manifest / frontmatter) drives lexing and sits on
/// `Config::locale_pack`. Diagnostics locale (`cli_diagnostic_locale`) drives
/// message templates only. When diagnostics locale is omitted, the code pack is
/// reused for rendering. When no code locale is selected explicitly, the
/// product default is the English `en` code pack.
pub(crate) fn config_with_locale(
    target: Target,
    input: &Path,
    cli_locale: Option<&str>,
    cli_diagnostic_locale: Option<&str>,
) -> Result<(Config, Option<LocalePack>), Box<Diagnostic>> {
    let code_pack = load_locale_pack_for_input(input, cli_locale)?;
    let config = match code_pack.as_ref() {
        Some(pack) => Config::default()
            .with_target(target)
            .with_locale_pack(pack.clone()),
        None => Config::default().with_target(target),
    };
    let diagnostic_pack =
        resolve_diagnostic_locale_pack(input, cli_diagnostic_locale, code_pack.as_ref())?;
    Ok((config, diagnostic_pack))
}

/// Resolve the pack used for diagnostic message rendering.
///
/// Chain: `--diagnostic-locale` → code pack → none (catalog English).
pub(crate) fn resolve_diagnostic_locale_pack(
    input: &Path,
    cli_diagnostic_locale: Option<&str>,
    code_pack: Option<&LocalePack>,
) -> Result<Option<LocalePack>, Box<Diagnostic>> {
    if let Some(locale) = cli_diagnostic_locale {
        let trimmed = locale.trim();
        if trimmed.is_empty() {
            return Err(Box::new(crate::package_diagnostic_error(
                "--diagnostic-locale must not be empty",
            )));
        }
        return load_locale_pack_for_input(input, Some(trimmed));
    }
    Ok(code_pack.cloned())
}

/// Load the reader pack selected by CLI locale or package manifest.
pub(crate) fn load_locale_pack_for_input(
    input: &Path,
    cli_locale: Option<&str>,
) -> Result<Option<LocalePack>, Box<Diagnostic>> {
    let spec = discover_package(input)?;
    // A validated manifest that disappeared is a diagnostic (FBR-P1-001);
    // legacy manifestless inputs legitimately have none.
    let manifest = manifest_for_spec(&spec)?;
    let manifest_path = spec
        .manifest_backed
        .then(|| spec.package_root.join(super::MANIFEST_FILE));
    let package_root = package_root_for_selection(&spec, manifest_path.as_deref());

    let Some(locale) = selected_locale(cli_locale, manifest.as_ref())? else {
        return Ok(None);
    };
    let pack_path = locale_pack_path(&package_root, &locale, cli_locale, manifest.as_ref());
    let pack = LocalePack::from_toml_path(&pack_path).map_err(|err| {
        Box::new(
            crate::package_diagnostic_error(format!(
                "failed to load reader locale '{locale}' pack '{}': {err}\n\
                 next action: install the matching reader pack for locale '{locale}' \
                 (share/faber/locale/{locale}/pack.toml beside the faber binary) or fix the \
                 package pack path",
                pack_path.display()
            ))
            .with_file(input.display().to_string()),
        )
    })?;

    if pack.metadata.id != locale {
        return Err(Box::new(
            crate::package_diagnostic_error(format!(
                "reader locale '{locale}' selected pack '{}' with id '{}'",
                pack_path.display(),
                pack.metadata.id
            ))
            .with_file(input.display().to_string()),
        ));
    }

    Ok(Some(pack))
}

fn selected_locale<'a>(
    cli_locale: Option<&'a str>,
    manifest: Option<&'a FaberManifest>,
) -> Result<Option<String>, Box<Diagnostic>> {
    if let Some(locale) = cli_locale {
        let trimmed = locale.trim();
        if trimmed.is_empty() {
            return Err(Box::new(crate::package_diagnostic_error(
                "--locale must not be empty",
            )));
        }
        return Ok(Some(trimmed.to_owned()));
    }

    let manifest_locale = manifest
        .and_then(|manifest| manifest.locale.locale.as_deref())
        .map(str::trim)
        .map(str::to_owned);

    Ok(Some(
        manifest_locale.unwrap_or_else(|| DEFAULT_CODE_LOCALE.to_owned()),
    ))
}

fn locale_pack_path(
    package_root: &Path,
    locale: &str,
    cli_locale: Option<&str>,
    manifest: Option<&FaberManifest>,
) -> PathBuf {
    let manifest_pack = manifest.and_then(|manifest| {
        let manifest_locale = manifest.locale.locale.as_deref();
        if cli_locale.is_none() || manifest_locale == Some(locale) {
            manifest.locale.pack.as_deref().map(str::trim)
        } else {
            None
        }
    });

    if let Some(pack) = manifest_pack {
        return normalize_path(&package_root.join(pack));
    }

    let package_pack = normalize_path(&package_root.join("locale").join(format!("{locale}.toml")));
    if package_pack.exists() {
        return package_pack;
    }

    installed_locale_pack_path(locale)
}

fn package_root_for_selection(spec: &PackageSpec, manifest_path: Option<&Path>) -> PathBuf {
    manifest_path
        .and_then(Path::parent)
        .map(normalize_path)
        .unwrap_or_else(|| spec.source_root.clone())
}

/// Resolve a CLI reader locale to a pack for single-file emit.
///
/// File input uses the package-aware resolver (package-local pack, else the
/// installed pack); stdin falls back to the installed pack directly, since
/// there is no package context to consult. An omitted locale selects the
/// product-default `en` pack.
pub fn locale_pack_for_emit(
    input: &[String],
    cli_locale: Option<&str>,
) -> Result<Option<LocalePack>, String> {
    let locale = match cli_locale {
        Some(locale) if locale.trim().is_empty() => {
            return Err("--locale must not be empty".to_owned());
        }
        Some(locale) => locale.trim(),
        None => DEFAULT_CODE_LOCALE,
    };

    if let Some(path) = input.iter().find(|s| !s.is_empty() && s.as_str() != "-") {
        return load_locale_pack_for_input(Path::new(path), Some(locale))
            .map_err(|diag| diag.message.clone());
    }

    // Stdin: no package context, use the installed pack for the locale.
    let pack_path = installed_locale_pack_path(locale);
    LocalePack::from_toml_path(&pack_path)
        .map(Some)
        .map_err(|err| {
            format!(
                "failed to load reader locale '{locale}' pack '{}': {err}\n\
                 next action: reinstall the matching faber dev kit so the pack ships at \
                 share/faber/locale/{locale}/pack.toml beside the faber binary",
                pack_path.display()
            )
        })
}

/// Installed reader-pack path for `locale` (env + exe + cwd resolution).
pub(crate) fn installed_locale_pack_path(locale: &str) -> PathBuf {
    installed_locale_pack_path_in(
        std::env::current_exe().ok().as_deref(),
        std::env::current_dir().ok().as_deref(),
        locale,
    )
}

/// The pure installed-pack resolution core, injectable for tests.
///
/// Installed binaries resolve `<prefix>/share/faber/locale/<locale>/pack.toml`
/// relative to the running binary — never a `CARGO_MANIFEST_DIR`-baked build
/// path (E8) and never an ambient walk-up (E5/G2). Development builds may fall
/// back to a sibling `radix/stdlib/locale/<locale>/pack.toml`. When no pack is
/// found the returned path does not exist; callers turn the load failure into
/// a nonzero, actionable error naming the missing pack and one next action.
pub(crate) fn installed_locale_pack_path_in(
    exe: Option<&Path>,
    cwd: Option<&Path>,
    locale: &str,
) -> PathBuf {
    if let Some(path) = install_locale_pack(exe, locale) {
        return path;
    }
    if cfg!(debug_assertions) {
        if let Some(path) = dev_locale_pack(cwd, exe, locale) {
            return path;
        }
    }
    fallback_installed_locale_pack(exe, locale)
}

fn install_locale_pack(exe: Option<&Path>, locale: &str) -> Option<PathBuf> {
    let bin_dir = exe?.parent()?;
    for relative in ["../share/faber/locale", "../lib/faber/locale"] {
        let candidate = bin_dir.join(relative).join(locale).join("pack.toml");
        if candidate.is_file() {
            return candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    None
}

fn dev_locale_pack(cwd: Option<&Path>, exe: Option<&Path>, locale: &str) -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Some(cwd) = cwd {
        starts.push(cwd.to_path_buf());
    }
    if let Some(exe) = exe {
        if let Some(parent) = exe.parent() {
            starts.push(parent.to_path_buf());
        }
    }
    for mut dir in starts {
        loop {
            let candidate = dir
                .join("radix")
                .join("stdlib")
                .join("locale")
                .join(locale)
                .join("pack.toml");
            if candidate.is_file() {
                return candidate.canonicalize().ok().or(Some(candidate));
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

fn fallback_installed_locale_pack(exe: Option<&Path>, locale: &str) -> PathBuf {
    exe.as_deref()
        .and_then(Path::parent)
        .map(|bin| {
            bin.join("../share/faber/locale")
                .join(locale)
                .join("pack.toml")
        })
        .unwrap_or_else(|| {
            PathBuf::from("share/faber/locale")
                .join(locale)
                .join("pack.toml")
        })
}

#[cfg(test)]
#[path = "locale_test.rs"]
mod tests;
