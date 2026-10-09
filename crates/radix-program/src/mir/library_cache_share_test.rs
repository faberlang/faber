//! Library interface analysis runs once per library across the analysis and
//! link/lower stages of one package preparation (front-end fix 2).
//!
//! The analysis stage hands its `LibraryInterfaceCache` to link and lowering
//! when both analyze libraries identically (same target, same locale loader).
//! The proof is a per-thread log of every library interface analysis plus a
//! byte comparison of the produced fmir-text image against a build that gives
//! every stage a fresh cache (the pre-sharing behavior, via the
//! `FRESH_LIBRARY_CACHE_PER_STAGE` seam).
//!
//! The Rust-target def-id closure check keeps its own `HirRust` cache: it
//! analyzes libraries under a different target than the executed (fmir) lane,
//! so its results are not interchangeable and it is deliberately not merged.

use super::*;
use crate::library::LIBRARY_ANALYSIS_LOG;
use std::collections::BTreeMap as Counts;

const MAIN: &str = r#"+++
locale = "en"
+++

import from "gradus:math" math
import from "gradus:loss" loss

main {
    const tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    const tensor<f32, [2, 2]> b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    const tensor<f32, [2, 2]> sum ← math.add(a, b)
    const f32 error ← loss.mse(sum, b)
    print error
}
"#;

fn write_package(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"library-cache-share\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n[locale]\nlocale = \"en\"\n",
    )
    .expect("write faber.toml");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
}

fn config() -> Config {
    let library_home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("resolve packet library home");
    Config::default().with_library_home(library_home)
}

/// Build the fmir-text image and return its bytes plus the library analyses
/// the build performed, as `(target, library identity) -> count`.
fn build_image(
    package: &Path,
    fresh_cache_per_stage: bool,
) -> (Vec<u8>, Counts<(String, String), usize>) {
    let config = config();
    let seams = crate::test_support::test_mir_seams();
    LIBRARY_ANALYSIS_LOG.with(|log| log.borrow_mut().clear());
    driver::FRESH_LIBRARY_CACHE_PER_STAGE.with(|flag| flag.set(fresh_cache_per_stage));
    let built = build_package_fmir_text_image(&config, package, &[], &seams);
    driver::FRESH_LIBRARY_CACHE_PER_STAGE.with(|flag| flag.set(false));
    let image = built.unwrap_or_else(|diagnostics| {
        panic!(
            "build: {}",
            diagnostics
                .iter()
                .map(|diagnostic| format!("{diagnostic:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    let bytes = fs::read(&image.image_path).expect("read fmir-text image");
    let mut counts = Counts::new();
    LIBRARY_ANALYSIS_LOG.with(|log| {
        for entry in log.borrow().iter() {
            *counts.entry(entry.clone()).or_insert(0) += 1;
        }
    });
    (bytes, counts)
}

fn executed_lane(counts: &Counts<(String, String), usize>) -> Counts<String, usize> {
    counts
        .iter()
        .filter(|((target, _), _)| target.contains("Fmir"))
        .map(|((_, library), count)| (library.clone(), *count))
        .collect()
}

#[test]
fn library_analysis_runs_once_across_analysis_link_and_lowering() {
    let dir = crate::test_support::test_temp_dir("library-cache-share");
    write_package(dir.as_ref());

    let (shared_image, shared_counts) = build_image(dir.as_ref(), false);
    let (fresh_image, fresh_counts) = build_image(dir.as_ref(), true);

    let shared = executed_lane(&shared_counts);
    let fresh = executed_lane(&fresh_counts);
    assert!(
        shared.len() >= 2,
        "two library imports must analyze at least two library modules: {shared:?}"
    );
    for (library, count) in &shared {
        assert_eq!(
            *count, 1,
            "library `{library}` analyzed {count} times with a shared cache"
        );
    }
    assert!(
        fresh.values().any(|count| *count >= 2),
        "the fresh-cache-per-stage baseline must re-analyze libraries: {fresh:?}"
    );
    assert!(
        fresh.values().sum::<usize>() > shared.values().sum::<usize>(),
        "sharing must reduce total library analyses: shared {shared:?} fresh {fresh:?}"
    );

    // The Rust-target closure check analyzes under HirRust, a different cache
    // key space: it never feeds (or reads) the executed-lane cache.
    assert!(
        shared_counts
            .keys()
            .any(|(target, _)| !target.contains("Fmir")),
        "expected the separate HirRust closure-check analyses: {shared_counts:?}"
    );

    assert!(
        shared_image == fresh_image,
        "fmir-text image differs between shared and fresh library caches"
    );
}
