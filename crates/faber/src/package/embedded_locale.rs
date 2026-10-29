//! Compressed reader-locale packs carried in the `faber` binary.
//!
//! Build.rs gzips `locale/<id>/pack.toml` into `OUT_DIR`. This module
//! embeds those artifacts and decompresses **only** the selected locale
//! before parsing with [`LocalePack::from_toml_str`]. Unused locales stay
//! compressed.

use flate2::read::GzDecoder;
use radix::module::locale::{LocalePack, LocalePackLoadError};
use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::sync::{Mutex, OnceLock, PoisonError};

/// Locale ids with an embedded pack. Order matches the authoring tree.
pub(crate) const EMBEDDED_LOCALE_IDS: &[&str] =
    &["ar", "en", "hi", "la", "th-TH", "vi", "zh-Hans", "zh-Hant"];

const PACK_AR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/locale/ar.pack.toml.gz"));
const PACK_EN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/locale/en.pack.toml.gz"));
const PACK_HI: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/locale/hi.pack.toml.gz"));
const PACK_LA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/locale/la.pack.toml.gz"));
const PACK_TH_TH: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/locale/th-TH.pack.toml.gz"));
const PACK_VI: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/locale/vi.pack.toml.gz"));
const PACK_ZH_HANS: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/locale/zh-Hans.pack.toml.gz"));
const PACK_ZH_HANT: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/locale/zh-Hant.pack.toml.gz"));

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static DECOMPRESSIONS: Cell<usize> = const { Cell::new(0) };
}

/// Failure loading one embedded reader pack.
#[derive(Debug)]
pub(crate) enum EmbeddedLocaleError {
    /// `locale` is not one of [`EMBEDDED_LOCALE_IDS`].
    UnknownLocale(String),
    /// Gzip decode of the selected pack failed.
    Decompress(std::io::Error),
    /// Decompressed bytes are not UTF-8.
    Utf8(std::str::Utf8Error),
    /// TOML parse or pack validation failed.
    Parse(LocalePackLoadError),
}

impl fmt::Display for EmbeddedLocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownLocale(locale) => {
                write!(f, "no embedded reader pack for locale '{locale}'")
            }
            Self::Decompress(err) => {
                write!(f, "cannot decompress embedded reader pack: {err}")
            }
            Self::Utf8(err) => {
                write!(f, "embedded reader pack is not UTF-8: {err}")
            }
            Self::Parse(err) => write!(f, "cannot parse embedded reader pack: {err}"),
        }
    }
}

impl std::error::Error for EmbeddedLocaleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnknownLocale(_) => None,
            Self::Decompress(err) => Some(err),
            Self::Utf8(err) => Some(err),
            Self::Parse(err) => Some(err),
        }
    }
}

/// Gzip payload for one embedded locale. `None` if `locale` is unknown.
pub(crate) fn compressed_pack_bytes(locale: &str) -> Option<&'static [u8]> {
    match locale {
        "ar" => Some(PACK_AR),
        "en" => Some(PACK_EN),
        "hi" => Some(PACK_HI),
        "la" => Some(PACK_LA),
        "th-TH" => Some(PACK_TH_TH),
        "vi" => Some(PACK_VI),
        "zh-Hans" => Some(PACK_ZH_HANS),
        "zh-Hant" => Some(PACK_ZH_HANT),
        _ => None,
    }
}

/// Decompress one gzip payload to the original pack.toml bytes.
pub(crate) fn decompress_gzip(gzip: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    #[cfg(test)]
    DECOMPRESSIONS.with(|count| count.set(count.get() + 1));
    let mut decoder = GzDecoder::new(gzip);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// Decompress and parse the embedded pack for `locale`, once per process.
///
/// Only that locale's gzip blob is decoded. Other embedded packs stay
/// compressed. The loader does not consult `share/`, an install prefix, or a
/// sibling checkout. A successful parse is cached per locale; failures are
/// not cached.
pub(crate) fn load_embedded_locale_pack(locale: &str) -> Result<LocalePack, EmbeddedLocaleError> {
    static CACHE: OnceLock<Mutex<HashMap<String, LocalePack>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(pack) = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(locale)
    {
        return Ok(pack.clone());
    }
    let pack = parse_embedded_locale_pack(locale)?;
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(locale.to_owned(), pack.clone());
    Ok(pack)
}

/// Decompress and parse the embedded pack for `locale`, uncached.
fn parse_embedded_locale_pack(locale: &str) -> Result<LocalePack, EmbeddedLocaleError> {
    let gzip = compressed_pack_bytes(locale)
        .ok_or_else(|| EmbeddedLocaleError::UnknownLocale(locale.to_owned()))?;
    let bytes = decompress_gzip(gzip).map_err(EmbeddedLocaleError::Decompress)?;
    let toml = std::str::from_utf8(&bytes).map_err(EmbeddedLocaleError::Utf8)?;
    LocalePack::from_toml_str(toml).map_err(EmbeddedLocaleError::Parse)
}

#[cfg(test)]
fn reset_decompressions() {
    DECOMPRESSIONS.with(|count| count.set(0));
}

#[cfg(test)]
fn decompressions() -> usize {
    DECOMPRESSIONS.with(Cell::get)
}

#[cfg(test)]
#[path = "embedded_locale_test.rs"]
mod tests;
