//! Compress `locale/<id>/pack.toml` into `OUT_DIR` gzip artifacts.
//!
//! The eight language packs are the authoring source of truth. The `faber`
//! binary embeds the compressed bytes and decompresses only the selected
//! locale at runtime.
//!
//! It embeds the `faber explain` reference: every corpus exemplum whose
//! frontmatter declares `explains = [...]`, plus `legacy-redirects.toml`.
//!
//! It also stamps the faber runtime and hosts source pins from the release
//! manifest (`release-manifest.yaml`), so generated manifests emit the exact
//! commits selected by release inputs.

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

    embed_reference(&manifest_dir.join("../../corpus"), &out_dir);

    stamp_source_revisions(&manifest_dir.join("../../release-manifest.yaml"));
}

/// Write `reference_embedded.rs`: one `include_str!` per exemplum that
/// declares `explains`, sorted by corpus-relative path.
fn embed_reference(corpus: &Path, out_dir: &Path) {
    println!("cargo:rerun-if-changed={}", corpus.display());
    let corpus = corpus.canonicalize().unwrap_or_else(|err| {
        panic!("corpus {}: {err}", corpus.display());
    });
    let mut files = Vec::new();
    collect_explaining_exempla(&corpus, &mut files);
    files.sort();

    let mut generated = String::from("static EMBEDDED_EXEMPLA: &[(&str, &str)] = &[\n");
    for file in &files {
        let relative = file
            .strip_prefix(&corpus)
            .expect("exemplum under corpus")
            .to_str()
            .expect("utf-8 corpus path")
            .replace('\\', "/");
        generated.push_str(&format!(
            "    ({relative:?}, include_str!({:?})),\n",
            file.to_str().expect("utf-8 corpus path")
        ));
    }
    generated.push_str("];\n");
    generated.push_str(&format!(
        "static EMBEDDED_LEGACY_REDIRECTS: &str = include_str!({:?});\n",
        corpus
            .join("legacy-redirects.toml")
            .to_str()
            .expect("utf-8 corpus path")
    ));

    let target = out_dir.join("reference_embedded.rs");
    fs::write(&target, generated).unwrap_or_else(|err| {
        panic!("write {}: {err}", target.display());
    });
}

fn collect_explaining_exempla(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|err| {
        panic!("read {}: {err}", dir.display());
    });
    for entry in entries {
        let path = entry
            .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
            .path();
        if path.is_dir() {
            collect_explaining_exempla(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "fab") && declares_explains(&path) {
            files.push(path);
        }
    }
}

/// Whether the `+++` frontmatter block has an `explains` key.
fn declares_explains(path: &Path) -> bool {
    let text = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("read {}: {err}", path.display());
    });
    let mut lines = text.lines();
    if lines.next() != Some("+++") {
        return false;
    }
    lines
        .take_while(|line| *line != "+++")
        .any(|line| line.starts_with("explains ="))
}

/// Export the faber runtime and hosts source pins from `pinnedInputs.source`.
/// Source commit rows are scoped by their `- name:` row, so unrelated
/// `commit:` fields elsewhere in the release manifest cannot become pins.
fn stamp_source_revisions(manifest: &Path) {
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = fs::read_to_string(manifest).unwrap_or_else(|err| {
        panic!("read release manifest {}: {err}", manifest.display());
    });
    for source in ["faber", "hosts"] {
        let mut source_name: Option<&str> = None;
        let mut source_commit: Option<&str> = None;
        for line in text.lines() {
            let trimmed = line.trim_start();
            if let Some(name) = trimmed.strip_prefix("- name:") {
                source_name = Some(name.trim().trim_matches('"'));
            } else if let Some(commit) = trimmed.strip_prefix("commit:") {
                if source_name == Some(source) {
                    source_commit = Some(commit.trim().trim_matches('"'));
                    break;
                }
            }
        }
        let commit = source_commit.unwrap_or_else(|| {
            panic!("{} has no source row named {source}", manifest.display());
        });
        assert!(
            commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "{}: {source} source pin {commit:?} is not a 40-hex commit",
            manifest.display()
        );
        let variable = match source {
            "faber" => "FABER_RUNTIME_PINNED_REV",
            "hosts" => "FABER_HOSTS_PINNED_REV",
            _ => unreachable!(),
        };
        println!("cargo:rustc-env={variable}={commit}");
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
