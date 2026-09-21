//! Pin (S08-2): the failable-closure fixed point is derived only in this
//! crate. A hit under a sibling `radix-hir-*` leaf means a backend started
//! re-deriving the walk + worklist instead of consuming
//! [`crate::failable::derive_failable_defs`].

use std::fs;
use std::path::{Path, PathBuf};

/// Definition signatures unique to the derivation home. Matched as plain
/// substrings so both the current and any future local spellings
/// (`propagate_failable`, `propagate_failable_functions`, …) trip the pin.
const DERIVATION_SYMBOLS: [&str; 6] = [
    "fn derive_failable_defs",
    "fn propagate_failable",
    "fn collect_function_dependencies",
    "fn collect_failable_functions",
    "fn with_suppressed",
    "struct DependencyVisitor",
];

#[test]
fn hir_backend_leaves_do_not_rederive_the_failable_fixed_point() {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory");
    let mut offenders: Vec<String> = Vec::new();
    for entry in fs::read_dir(crates_dir).expect("crates directory is readable") {
        let entry = entry.expect("crate entry");
        let name = entry.file_name();
        // `radix-hir-` excludes this crate itself: only sibling leaves scan.
        if !name.to_string_lossy().starts_with("radix-hir-") {
            continue;
        }
        for file in rust_files(&entry.path().join("src")) {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            for symbol in DERIVATION_SYMBOLS {
                if text.contains(symbol) {
                    offenders.push(format!("{}: defines `{symbol}`", file.display()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "failable derivation re-derived outside radix-hir:\n{}",
        offenders.join("\n")
    );
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    out
}
