//! Resolve public exempla roots after the org corpus split.
//!
//! Language keyword programs live in sibling `examples/corpus/`. Named public
//! tracks are peer directories under `examples/`. Norma stdlib tours live in
//! `norma/exempla/`. Environment variables override every root for CI and
//! alternate checkouts.

use std::path::{Path, PathBuf};

/// Env override for the keyword / language reference corpus root
/// (`examples/corpus/` — directory containing `index.toml`).
pub const CORPUS_ENV: &str = "FABER_EXEMPLA_CORPUS";

/// Env override for the examples repo root (`…/examples`).
pub const EXAMPLES_ENV: &str = "FABER_EXAMPLES_HOME";

/// Env override for Norma stdlib exempla root (`…/norma/exempla`).
pub const NORMA_EXEMPLA_ENV: &str = "FABER_NORMA_EXEMPLA";

/// Keyword / language reference corpus directory.
///
/// Prefer `FABER_EXEMPLA_CORPUS`, then sibling `examples/corpus` from a
/// `faberlang/` layout, then a local `corpus/` fallback (pointer README only).
pub fn corpus_dir() -> PathBuf {
    if let Ok(path) = std::env::var(CORPUS_ENV) {
        return PathBuf::from(path);
    }
    if let Some(examples) = examples_home() {
        let corpus = examples.join("corpus");
        if corpus.join("index.toml").is_file() || corpus.is_dir() {
            return corpus;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

/// GPU workload rungs (`examples/gpu-workload/`).
pub fn gpu_workload_dir() -> PathBuf {
    track_dir("gpu-workload")
}

/// AIR lane demos (`examples/air/`).
pub fn air_dir() -> PathBuf {
    track_dir("air")
}

/// Script-kernel demos (`examples/script-kernel/`).
pub fn script_kernel_dir() -> PathBuf {
    track_dir("script-kernel")
}

/// Norma stdlib instructional exempla (`norma/exempla/`).
pub fn norma_exempla_dir() -> PathBuf {
    if let Ok(path) = std::env::var(NORMA_EXEMPLA_ENV) {
        return PathBuf::from(path);
    }
    if let Some(home) = faberlang_home() {
        let dir = home.join("norma/exempla");
        if dir.is_dir() {
            return dir;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../norma/exempla")
}

fn track_dir(name: &str) -> PathBuf {
    if let Some(examples) = examples_home() {
        let dir = examples.join(name);
        if dir.is_dir() {
            return dir;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../examples")
        .join(name)
}

fn examples_home() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(EXAMPLES_ENV) {
        let p = PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }
    if let Some(home) = faberlang_home() {
        let examples = home.join("examples");
        if examples.is_dir() {
            return Some(examples);
        }
    }
    let sibling = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples");
    if sibling.is_dir() {
        return Some(sibling);
    }
    None
}

/// Walk ancestors from the exempla crate (and optional cwd) looking for a
/// directory that contains both `examples/` and `radix/` (or `norma/`).
pub fn faberlang_home() -> Option<PathBuf> {
    let mut starts = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    for mut dir in starts {
        loop {
            let has_examples = dir.join("examples").is_dir();
            let has_radix = dir.join("radix").is_dir();
            let has_norma = dir.join("norma").is_dir();
            if has_examples && (has_radix || has_norma) {
                return Some(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

/// Absolute path to the public `faber-runtime` crate (`use faber::…`).
pub fn faber_runtime_crate_path() -> PathBuf {
    if let Some(home) = faberlang_home() {
        let runtime = home.join("faber-runtime");
        if runtime.is_dir() {
            return runtime.canonicalize().unwrap_or(runtime);
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../faber-runtime")
        .canonicalize()
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../faber-runtime"))
}
