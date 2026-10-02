//! Compress `locale/<id>/pack.toml` into `OUT_DIR` gzip artifacts.
//!
//! The eight language packs are the authoring source of truth. The `faber`
//! binary embeds the compressed bytes and decompresses only the selected
//! locale at runtime.
//!
//! It also stamps `FABER_HOSTS_PINNED_REV` from the release manifest's hosts
//! source pin (`release-manifest.yaml`), so the rev the generated manifests
//! emit equals the manifest pin by construction.

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
    let locale_root = manifest_dir.join("../../locale");
    let artifact_dir = out_dir.join("locale");
    fs::create_dir_all(&artifact_dir).unwrap_or_else(|err| {
        panic!("create {}: {err}", artifact_dir.display());
    });

    for locale in LOCALES {
        compress_pack(&locale_root, &artifact_dir, locale);
    }

    stamp_hosts_pinned_rev(&manifest_dir.join("../../release-manifest.yaml"));
}

/// Export the hosts source pin (`pinnedInputs.source[name = hosts].commit`) as
/// the `FABER_HOSTS_PINNED_REV` compile-time env. `commit:` rows exist only in
/// that list, so a name-to-commit scan cannot cross into other sections.
fn stamp_hosts_pinned_rev(manifest: &Path) {
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = fs::read_to_string(manifest).unwrap_or_else(|err| {
        panic!("read release manifest {}: {err}", manifest.display());
    });
    let mut source_name: Option<&str> = None;
    let mut hosts_commit: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(name) = trimmed.strip_prefix("- name:") {
            source_name = Some(name.trim().trim_matches('"'));
        } else if let Some(commit) = trimmed.strip_prefix("commit:") {
            if source_name == Some("hosts") {
                hosts_commit = Some(commit.trim().trim_matches('"'));
                break;
            }
        }
    }
    let commit = hosts_commit.unwrap_or_else(|| {
        panic!("{} has no source row named hosts", manifest.display());
    });
    assert!(
        commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "{}: hosts source pin {commit:?} is not a 40-hex commit",
        manifest.display()
    );
    println!("cargo:rustc-env=FABER_HOSTS_PINNED_REV={commit}");
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
