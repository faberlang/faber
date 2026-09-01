//! Compress `stdlib/locale/<id>/pack.toml` into `OUT_DIR` gzip artifacts.
//!
//! The eight reader packs are the authoring source of truth. The `faber`
//! binary embeds the compressed bytes and decompresses only the selected
//! locale at runtime.

use flate2::Compression;
use flate2::write::GzEncoder;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const LOCALES: &[&str] = &["ar", "en", "hi", "la", "th-TH", "vi", "zh-Hans", "zh-Hant"];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let locale_root = manifest_dir.join("../../stdlib/locale");
    let artifact_dir = out_dir.join("locale");
    fs::create_dir_all(&artifact_dir).unwrap_or_else(|err| {
        panic!("create {}: {err}", artifact_dir.display());
    });

    for locale in LOCALES {
        compress_pack(&locale_root, &artifact_dir, locale);
    }
}

fn compress_pack(locale_root: &Path, artifact_dir: &Path, locale: &str) {
    let src = locale_root.join(locale).join("pack.toml");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = fs::read(&src).unwrap_or_else(|err| {
        panic!("read reader pack {}: {err}", src.display());
    });
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&raw).unwrap_or_else(|err| {
        panic!("gzip {locale}: {err}");
    });
    let gz = encoder.finish().unwrap_or_else(|err| {
        panic!("gzip finish {locale}: {err}");
    });
    let dest = artifact_dir.join(format!("{locale}.pack.toml.gz"));
    fs::write(&dest, gz).unwrap_or_else(|err| {
        panic!("write {}: {err}", dest.display());
    });
}
