//! Compress `locale/<id>/pack.toml` into `OUT_DIR` gzip artifacts.
//!
//! The eight language packs are the authoring source of truth. The `faber`
//! binary embeds the compressed bytes and decompresses only the selected
//! locale at runtime.
//!
//! It embeds the `faber explain` reference: every corpus exemplum whose
//! frontmatter declares `explains = [...]`, plus `legacy-redirects.toml`.
//!
//! It embeds the runtime support sources (`runtime/*`, `crates/radix-host-abi`)
//! so an installed binary can materialize a path-dependency root, and the
//! Norma library (`lib/norma` source, locale packs and manifests) so an
//! installed binary resolves `norma:*` without a checkout.

use flate2::Compression;
use flate2::write::GzEncoder;
use std::env;
use std::fmt::Write as _;
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
    embed_runtime(&manifest_dir.join("../.."), &out_dir);
}

/// Repository-relative roots whose files the binary carries.
const RUNTIME_ROOTS: &[&str] = &[
    "runtime/rust",
    "runtime/go",
    "runtime/typescript",
    "runtime/swift",
    "runtime/llvm",
    "crates/radix-host-abi",
    "lib/norma/src",
    "lib/norma/locale",
];

/// Repository-relative single files the binary carries (Norma manifests the
/// library resolver reads).
const RUNTIME_FILES: &[&str] = &["lib/norma/faber.toml", "lib/norma/cista.toml"];

/// Write `runtime_embedded.rs`: `EMBEDDED_RUNTIME` (sorted repo-relative path
/// and `include_bytes!` of the file) and `EMBEDDED_RUNTIME_DIGEST` (FNV-1a 64
/// over paths and bytes, so the extraction directory tracks the content).
fn embed_runtime(repo_root: &Path, out_dir: &Path) {
    let repo_root = repo_root.canonicalize().unwrap_or_else(|err| {
        panic!("repository root {}: {err}", repo_root.display());
    });
    let mut files = Vec::new();
    for root in RUNTIME_ROOTS {
        let dir = repo_root.join(root);
        println!("cargo:rerun-if-changed={}", dir.display());
        collect_runtime_files(&dir, &mut files);
    }
    for file in RUNTIME_FILES {
        let path = repo_root.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        assert!(path.is_file(), "embedded file {} missing", path.display());
        files.push(path);
    }
    files.sort();

    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            digest ^= u64::from(*byte);
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    let mut generated = String::from("static EMBEDDED_RUNTIME: &[(&str, &[u8])] = &[\n");
    for file in &files {
        let relative = file
            .strip_prefix(&repo_root)
            .expect("runtime file under repository root")
            .to_str()
            .expect("utf-8 runtime path")
            .replace('\\', "/");
        let bytes = fs::read(file).unwrap_or_else(|err| {
            panic!("read {}: {err}", file.display());
        });
        feed(relative.as_bytes());
        feed(&[0]);
        feed(&(bytes.len() as u64).to_le_bytes());
        feed(&bytes);
        writeln!(
            generated,
            "    ({relative:?}, include_bytes!({:?})),",
            file.to_str().expect("utf-8 runtime path")
        )
        .expect("write to String");
    }
    generated.push_str("];\n");
    writeln!(
        generated,
        "const EMBEDDED_RUNTIME_DIGEST: &str = \"{digest:016x}\";"
    )
    .expect("write to String");

    let target = out_dir.join("runtime_embedded.rs");
    fs::write(&target, generated).unwrap_or_else(|err| {
        panic!("write {}: {err}", target.display());
    });
}

fn collect_runtime_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|err| {
        panic!("read {}: {err}", dir.display());
    });
    for entry in entries {
        let path = entry
            .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
            .path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !matches!(name, "target" | "node_modules" | ".git") {
                collect_runtime_files(&path, files);
            }
        } else if path.is_file() {
            let ts_lock = path.extension().is_some_and(|ext| ext == "lock")
                && path
                    .components()
                    .any(|c| c.as_os_str() == std::ffi::OsStr::new("typescript"));
            if !ts_lock {
                files.push(path);
            }
        }
    }
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
        writeln!(
            generated,
            "    ({relative:?}, include_str!({:?})),",
            file.to_str().expect("utf-8 corpus path")
        )
        .expect("write to String");
    }
    generated.push_str("];\n");
    writeln!(
        generated,
        "static EMBEDDED_LEGACY_REDIRECTS: &str = include_str!({:?});",
        corpus
            .join("legacy-redirects.toml")
            .to_str()
            .expect("utf-8 corpus path")
    )
    .expect("write to String");

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
