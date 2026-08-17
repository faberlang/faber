//! FORMAT-PRETTY S1 — byte-identity harness for the `normalise-v1` policy.
//!
//! HARD CONTRACT gate: `normalise-v1` output is byte-identical to the
//! pre-change `compile_author` output. This test author-formats every `*.fab`
//! fixture under `radix/corpus/` (and `faber/corpus/`, when the sibling repo is
//! present) and records each result to a snapshot tree.
//!
//! Run it twice around a change and diff the two snapshot trees:
//!
//! ```text
//! FORMA_BI_SNAPSHOT=before cargo test -p radix --test forma_byte_identity_test
//! # ... make the change ...
//! FORMA_BI_SNAPSHOT=after  cargo test -p radix --test forma_byte_identity_test
//! diff -r before after   # must be EMPTY
//! ```
//!
//! Without the env var the test walks the same corpus and asserts every
//! fixture still formats successfully — a cheap permanent corpus gate.

use radix_module::forma::compile_author_str;
use std::fs;
use std::path::{Path, PathBuf};

fn collect_fab_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fab_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("fab") {
            out.push(path);
        }
    }
}

/// (label, corpus-relative name, source) for every corpus fixture.
fn corpus_sources() -> Vec<(String, String, String)> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    for (label, root) in [
        ("radix", crate_dir.join("../../corpus")),
        ("faber", crate_dir.join("../../../faber/corpus")),
    ] {
        if !root.is_dir() {
            if label == "faber" {
                // faber/corpus is a sibling-repo surface; absent in standalone checkouts.
                continue;
            }
            panic!("corpus dir missing: {}", root.display());
        }
        let mut files = Vec::new();
        collect_fab_files(&root, &mut files);
        files.sort();
        for file in files {
            let rel = file
                .strip_prefix(&root)
                .expect("under corpus")
                .to_string_lossy()
                .replace('\\', "/");
            let source = fs::read_to_string(&file).unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
            sources.push((label.to_owned(), rel, source));
        }
    }
    sources
}

#[test]
fn normalise_v1_corpus_byte_identity_snapshot() {
    let snapshot = std::env::var_os("FORMA_BI_SNAPSHOT");
    let mut failures = Vec::new();
    let mut written = 0usize;

    for (label, rel, source) in corpus_sources() {
        let result = compile_author_str(&rel, &source);
        if let Some(dir) = &snapshot {
            let out_path = Path::new(dir).join(&label).join(&rel);
            let out_content = if let Some(output) = result.output.as_ref() {
                output.code.clone()
            } else {
                let detail: Vec<_> = result
                    .diagnostics
                    .iter()
                    .filter(|d| d.is_error())
                    .map(|d| format!("{:?}: {:?}", d.code, d.issue()))
                    .collect();
                format!("# FORMAT FAILED\n{detail:#?}\n")
            };
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).unwrap_or_else(|err| panic!("mkdir {}: {err}", parent.display()));
            }
            fs::write(&out_path, out_content).unwrap_or_else(|err| panic!("write {}: {err}", out_path.display()));
            written += 1;
        }
        if !result.success() {
            failures.push(format!("{label}/{rel}"));
        }
    }

    if let Some(dir) = &snapshot {
        eprintln!(
            "byte-identity snapshot: wrote {written} fixture outputs to {}",
            dir.to_string_lossy()
        );
        eprintln!("format failures in snapshot run: {}", failures.len());
        return;
    }

    assert!(
        failures.is_empty(),
        "{} corpus fixture(s) did not author-format cleanly:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
