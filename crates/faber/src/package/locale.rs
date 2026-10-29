use radix::module::codegen::Target;
use radix::module::diagnostics::Diagnostic;
use radix::module::driver::{Config, LocalePackLoader};
use radix::module::locale::LocalePack;
use std::path::{Path, PathBuf};

use super::discover_package;
use super::embedded_locale::{EMBEDDED_LOCALE_IDS, EmbeddedLocaleError, load_embedded_locale_pack};
use super::manifest::{FaberManifest, manifest_for_spec};
use radix::program::discovery::PackageSpec;
use radix::program::paths::normalize_path;

pub const DEFAULT_CODE_LOCALE: &str = "en";

/// Load a conversion target from the embedded pack set. Conversion targets
/// are command-wide and must not vary with the first input package.
///
/// # Errors
///
/// Returns an error when no embedded pack exists for `locale`; the diagnostic
/// names the embedded reader-locale ids.
pub fn embedded_conversion_locale_pack(locale: &str) -> Result<LocalePack, Box<Diagnostic>> {
    load_embedded_locale_pack(locale).map_err(|err| Box::new(embedded_load_diagnostic(locale, err)))
}

fn embedded_locales_list() -> String {
    EMBEDDED_LOCALE_IDS.join(", ")
}

fn embedded_load_diagnostic(locale: &str, err: EmbeddedLocaleError) -> Diagnostic {
    let embedded_locales = embedded_locales_list();
    crate::exit_status::config_diagnostic_error(format!(
        "failed to load reader locale '{locale}': {err}\n\
         next action: choose one of the embedded reader locales ({embedded_locales})"
    ))
    .with_arg("locale", locale.to_owned())
    .with_arg("embedded_locales", embedded_locales)
}

/// The product's embedded reader-pack set, as a driver [`LocalePackLoader`].
///
/// Frontmatter `+++ locale = "\u{2026}" +++` resolution in the module driver
/// consults this seam when the workspace language-pack root cannot serve the
/// id, so an installed binary reads a declared locale with no checkout beside
/// it (`faber build -t llvm-text` on a Latin source must not fall back to the
/// English pack on the published binary).
pub fn embedded_locale_pack_loader() -> LocalePackLoader {
    LocalePackLoader::new(|locale| load_embedded_locale_pack(locale).map_err(|err| err.to_string()))
}

/// Build the product-default code config for inputs without package context.
///
/// # Errors
///
/// Returns an error if the embedded product-default `en` pack cannot be loaded.
pub fn default_config_with_locale(target: Target) -> Result<Config, Box<Diagnostic>> {
    let pack = embedded_conversion_locale_pack(DEFAULT_CODE_LOCALE)?;
    Ok(Config::default()
        .with_target(target)
        .with_locale_pack_loader(embedded_locale_pack_loader())
        .with_locale_pack(pack))
}

/// Build a driver config (code locale) and the pack used for diagnostic rendering.
///
/// Code locale (`cli_locale` / manifest / frontmatter) drives lexing and sits on
/// `Config::locale_pack`. Diagnostics locale (`cli_diagnostics_locale`) drives
/// message templates only. When diagnostics locale is omitted, the code pack is
/// reused for rendering. When no code locale is selected explicitly, the
/// product default is the English `en` code pack.
///
/// # Errors
///
/// Returns an error when package discovery, reader-locale selection, or
/// diagnostic-locale resolution fails.
pub fn config_with_locale(
    target: Target,
    input: &Path,
    cli_locale: Option<&str>,
    cli_diagnostics_locale: Option<&str>,
) -> Result<(Config, Option<LocalePack>), Box<Diagnostic>> {
    let code_pack = load_locale_pack_for_input(input, cli_locale)?;
    let base = Config::default()
        .with_target(target)
        .with_locale_pack_loader(embedded_locale_pack_loader());
    let config = match code_pack.as_ref() {
        Some(pack) => match cli_locale {
            Some(_) => base.with_resolved_locale_pack(pack.clone()),
            None => base.with_locale_pack(pack.clone()),
        },
        None => base,
    };
    let diagnostic_pack =
        resolve_diagnostics_locale_pack(input, cli_diagnostics_locale, code_pack.as_ref())?;
    Ok((config, diagnostic_pack))
}

/// Resolve the pack used for diagnostic message rendering.
///
/// Chain: `--diagnostics-locale` → code pack → none (catalog English).
pub(crate) fn resolve_diagnostics_locale_pack(
    input: &Path,
    cli_diagnostics_locale: Option<&str>,
    code_pack: Option<&LocalePack>,
) -> Result<Option<LocalePack>, Box<Diagnostic>> {
    if let Some(locale) = cli_diagnostics_locale {
        let trimmed = locale.trim();
        if trimmed.is_empty() {
            return Err(Box::new(
                crate::exit_status::usage_diagnostic_error(
                    "--diagnostics-locale must not be empty",
                )
                .with_arg("flag", "--diagnostics-locale"),
            ));
        }
        return load_locale_pack_for_input(input, Some(trimmed));
    }
    Ok(code_pack.cloned())
}

/// Load the reader pack selected by CLI locale or package manifest.
///
/// # Errors
///
/// Returns an error when package discovery or manifest loading fails, when the
/// `--locale` value is empty, or when the selected pack is missing or its id
/// does not match the selected locale.
pub fn load_locale_pack_for_input(
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

    let locale = selected_locale(cli_locale, manifest.as_ref())?;
    let pack =
        load_selected_locale_pack(input, &package_root, &locale, cli_locale, manifest.as_ref())?;
    Ok(Some(pack))
}

fn selected_locale<'a>(
    cli_locale: Option<&'a str>,
    manifest: Option<&'a FaberManifest>,
) -> Result<String, Box<Diagnostic>> {
    if let Some(locale) = cli_locale {
        let trimmed = locale.trim();
        if trimmed.is_empty() {
            return Err(Box::new(
                crate::exit_status::usage_diagnostic_error("--locale must not be empty")
                    .with_arg("flag", "--locale"),
            ));
        }
        return Ok(trimmed.to_owned());
    }

    let manifest_locale = manifest
        .and_then(|manifest| manifest.locale.locale.as_deref())
        .map(str::trim)
        .map(str::to_owned);

    Ok(manifest_locale.unwrap_or_else(|| DEFAULT_CODE_LOCALE.to_owned()))
}

fn load_selected_locale_pack(
    input: &Path,
    package_root: &Path,
    locale: &str,
    cli_locale: Option<&str>,
    manifest: Option<&FaberManifest>,
) -> Result<LocalePack, Box<Diagnostic>> {
    if let Some(pack_path) = file_locale_pack_path(package_root, locale, cli_locale, manifest) {
        return load_locale_pack_from_file(input, locale, &pack_path);
    }
    let pack = load_embedded_locale_pack(locale).map_err(|err| {
        Box::new(embedded_load_diagnostic(locale, err).with_file(input.display().to_string()))
    })?;
    if pack.metadata.id != locale {
        return Err(Box::new(
            crate::exit_status::config_diagnostic_error(format!(
                "reader locale '{locale}' selected embedded pack with id '{}'",
                pack.metadata.id
            ))
            .with_file(input.display().to_string()),
        ));
    }
    Ok(pack)
}

/// Manifest `[locale] pack` path, else package-local `locale/<locale>.toml`
/// when that file exists. `None` means the embedded pack is the source.
fn file_locale_pack_path(
    package_root: &Path,
    locale: &str,
    cli_locale: Option<&str>,
    manifest: Option<&FaberManifest>,
) -> Option<PathBuf> {
    let manifest_pack = manifest.and_then(|manifest| {
        let manifest_locale = manifest.locale.locale.as_deref();
        if cli_locale.is_none() || manifest_locale == Some(locale) {
            manifest.locale.pack.as_deref().map(str::trim)
        } else {
            None
        }
    });

    if let Some(pack) = manifest_pack {
        return Some(normalize_path(&package_root.join(pack)));
    }

    let package_pack = normalize_path(&package_root.join("locale").join(format!("{locale}.toml")));
    package_pack.exists().then_some(package_pack)
}

fn load_locale_pack_from_file(
    input: &Path,
    locale: &str,
    pack_path: &Path,
) -> Result<LocalePack, Box<Diagnostic>> {
    let pack = LocalePack::from_toml_path(pack_path).map_err(|err| {
        Box::new(
            crate::exit_status::config_diagnostic_error(format!(
                "failed to load reader locale '{locale}' pack '{}': {err}\n\
                 next action: fix the package pack path",
                pack_path.display()
            ))
            .with_file(input.display().to_string())
            .with_arg("locale", locale.to_owned())
            .with_arg("pack", pack_path.display().to_string()),
        )
    })?;

    if pack.metadata.id != locale {
        return Err(Box::new(
            crate::exit_status::config_diagnostic_error(format!(
                "reader locale '{locale}' selected pack '{}' with id '{}'",
                pack_path.display(),
                pack.metadata.id
            ))
            .with_file(input.display().to_string()),
        ));
    }

    Ok(pack)
}

fn package_root_for_selection(spec: &PackageSpec, manifest_path: Option<&Path>) -> PathBuf {
    manifest_path
        .and_then(Path::parent)
        .map_or_else(|| spec.source_root.clone(), normalize_path)
}

/// Resolve a CLI reader locale to a pack for single-file emit while retaining
/// the structured diagnostic produced by package discovery and locale loading.
///
/// File input uses the package-aware resolver (manifest pack path, else
/// package-local pack, else the embedded pack); stdin uses the embedded pack
/// directly, since there is no package context to consult. An omitted locale
/// selects the product-default `en` pack.
///
/// # Errors
///
/// Returns an error when `--locale` is empty, when a file input fails package
/// or locale-pack resolution (a missing package path is reported specifically),
/// or when the embedded pack for the locale cannot be loaded for stdin.
pub fn locale_pack_for_emit_with_diagnostic(
    input: &[String],
    cli_locale: Option<&str>,
) -> Result<Option<LocalePack>, Box<Diagnostic>> {
    let locale = match cli_locale {
        Some(locale) if locale.trim().is_empty() => {
            return Err(Box::new(crate::exit_status::usage_diagnostic_error(
                "--locale must not be empty",
            )));
        }
        Some(locale) => locale.trim(),
        None => DEFAULT_CODE_LOCALE,
    };

    if let Some(path) = input.iter().find(|s| !s.is_empty() && s.as_str() != "-") {
        let path = Path::new(path);
        return match load_locale_pack_for_input(path, Some(locale)) {
            Err(_diagnostic) if path_is_missing(path) => {
                Err(super::missing_package_path_diagnostic(path))
            }
            result => result,
        };
    }

    // Stdin: no package context, use the embedded pack for the locale.
    load_embedded_locale_pack(locale)
        .map(Some)
        .map_err(|err| Box::new(embedded_load_diagnostic(locale, err)))
}

/// Compatibility wrapper for library callers that only need the normalized
/// message. The CLI uses [`locale_pack_for_emit_with_diagnostic`] so it can
/// preserve structured package-path failures.
///
/// # Errors
///
/// Returns the diagnostic message when [`locale_pack_for_emit_with_diagnostic`]
/// fails.
pub fn locale_pack_for_emit(
    input: &[String],
    cli_locale: Option<&str>,
) -> Result<Option<LocalePack>, String> {
    locale_pack_for_emit_with_diagnostic(input, cli_locale)
        .map_err(|diagnostic| diagnostic.message.clone())
}

fn path_is_missing(path: &Path) -> bool {
    matches!(
        std::fs::metadata(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    )
}

/// Reader-locale ids with an embedded pack, for CLI error listing
/// (`faber convert --to <unknown>` fails closed naming the real options).
pub fn available_locales() -> Vec<String> {
    EMBEDDED_LOCALE_IDS
        .iter()
        .map(|id| (*id).to_owned())
        .collect()
}

#[cfg(test)]
#[path = "locale_test.rs"]
mod tests;
