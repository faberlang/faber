use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use radix::driver::Config;
use radix::locale::LocalePack;
use std::path::{Path, PathBuf};

use super::discovery::discover_package;
use super::manifest::{manifest_for_spec, FaberManifest};
use super::paths::normalize_path;
use super::PackageSpec;

/// Build a driver config (code locale) and the pack used for diagnostic rendering.
///
/// Code locale (`cli_locale` / manifest / frontmatter) drives lexing and sits on
/// `Config::locale_pack`. Diagnostics locale (`cli_diagnostic_locale`) drives
/// message templates only. When diagnostics locale is omitted, the code pack is
/// reused for rendering (legacy one-flag behavior). When neither is selected,
/// both are `None` and the diagnostic catalog's English prose is used.
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
                "failed to load reader locale '{locale}' pack '{}': {err}",
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

    Ok(manifest
        .and_then(|manifest| manifest.locale.locale.as_deref())
        .map(str::trim)
        .map(str::to_owned))
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
/// there is no package context to consult. `None` locale yields `None`.
pub fn locale_pack_for_emit(
    input: &[String],
    cli_locale: Option<&str>,
) -> Result<Option<LocalePack>, String> {
    let Some(locale) = cli_locale
        .map(str::trim)
        .filter(|locale| !locale.is_empty())
    else {
        return Ok(None);
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
                "failed to load reader locale '{locale}' pack '{}': {err}",
                pack_path.display()
            )
        })
}

fn installed_locale_pack_path(locale: &str) -> PathBuf {
    normalize_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../radix/stdlib")
            .join("locale")
            .join(locale)
            .join("pack.toml"),
    )
}

#[cfg(test)]
#[path = "locale_test.rs"]
mod tests;
