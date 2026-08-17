//! Author-mode idempotency on exempla files containing `#` comment lines (SC-10).

use radix_module::forma::test_gate::assert_author_idempotent;
use std::fs;
use std::path::{Path, PathBuf};

fn exempla_corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

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

/// Files whose source contains at least one `#` comment line (`^\s*#`), per SC-10.
fn comment_bearing_exempla(corpus: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_fab_files(corpus, &mut files);
    files.retain(|path| {
        let Ok(source) = fs::read_to_string(path) else {
            return false;
        };
        source
            .lines()
            .any(|line| line.trim_start().starts_with('#'))
    });
    files.sort();
    files
}

fn relative_corpus_path(corpus: &Path, file: &Path) -> String {
    file.strip_prefix(corpus)
        .expect("under corpus")
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn author_idempotent_on_hash_comment_exempla_corpus() {
    let corpus = exempla_corpus_dir();
    let files = comment_bearing_exempla(&corpus);
    assert!(
        !files.is_empty(),
        "expected at least one # comment exempla under {}",
        corpus.display()
    );

    let mut failures = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).expect("read exempla");
        let name = relative_corpus_path(&corpus, file);
        if let Err(reason) = assert_author_idempotent(&name, &source) {
            failures.push(format!("{name}: {reason}"));
        }
    }

    let passed = files.len() - failures.len();
    eprintln!("SC-10 author idempotency: {passed}/{} # comment exempla passed", files.len());
    assert!(
        failures.is_empty(),
        "author idempotency failures on # comment exempla ({} of {} files):\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
