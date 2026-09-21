//! Pin for DEF S08-3: breakable emission has exactly one home.
//!
//! Each C-family backend carried a full copy of `breakable.rs`, including a
//! `BreakableLoopStyle` enum that already parametrized all four targets. The
//! copies were replaced by `radix_codegen_shared::breakable`. If this test
//! fails, a backend re-copied the file: delete the copy and use
//! `radix_codegen_shared::breakable` instead.

use std::path::{Path, PathBuf};

fn collect_breakable_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str());
        if path.is_dir() {
            // VCS metadata and build caches are not source.
            if name == Some(".git") || name == Some("target") {
                continue;
            }
            collect_breakable_files(&path, out);
        } else if name == Some("breakable.rs") {
            out.push(path);
        }
    }
}

#[test]
fn breakable_emission_has_one_home() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("workspace root above crates/");
    let mut found = Vec::new();
    collect_breakable_files(workspace_root, &mut found);
    let home = manifest_dir.join("src").join("breakable.rs");
    let copies: Vec<&PathBuf> = found.iter().filter(|p| **p != home).collect();
    assert_eq!(
        found.len(),
        1,
        "breakable.rs must live only at {home:?}; found copies at {copies:?}"
    );
}
