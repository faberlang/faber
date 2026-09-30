//! The `faber const-codemod` seam resolves each file's reader pack and reads
//! both `.fab` and `.proba` files.

use super::{codemod_constants, resolve_paths};
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("faber-const-codemod-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn en_frontmatter_file_gets_const() {
    let source = "+++\nlocale = \"en\"\n+++\nstatic int LIMIT = 4\n";
    let result = codemod_constants(Path::new("limit.fab"), source).expect("codemod accepted");
    assert_eq!(result.converted, 1);
    assert_eq!(
        result.source,
        "+++\nlocale = \"en\"\n+++\nconst int LIMIT = 4\n"
    );
}

#[test]
fn la_frontmatter_file_gets_fixum() {
    let source = "+++\nlocale = \"la\"\n+++\ngeneris numerus LIMES = 4\n";
    let result = codemod_constants(Path::new("limes.fab"), source).expect("codemod accepted");
    assert_eq!(result.converted, 1);
    assert_eq!(
        result.source,
        "+++\nlocale = \"la\"\n+++\nfixum numerus LIMES = 4\n"
    );
}

#[test]
fn proba_files_are_collected_and_accepted() {
    let dir = scratch("collect");
    fs::create_dir_all(dir.join("src")).expect("src dir");
    fs::create_dir_all(dir.join("target")).expect("target dir");
    fs::write(dir.join("src/a.fab"), "").expect("write");
    fs::write(dir.join("src/b.proba"), "").expect("write");
    fs::write(dir.join("src/c.txt"), "").expect("write");
    fs::write(dir.join("target/skipped.fab"), "").expect("write");

    let files = resolve_paths(std::slice::from_ref(&dir)).expect("resolve");
    let names: Vec<_> = files
        .iter()
        .map(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_owned()
        })
        .collect();
    assert_eq!(names, ["a.fab", "b.proba"]);

    let single = resolve_paths(&[dir.join("src/b.proba")]).expect("a .proba file is accepted");
    assert_eq!(single.len(), 1);
    assert!(resolve_paths(&[dir.join("src/c.txt")]).is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn proba_source_with_a_static_converts() {
    let dir = scratch("proba");
    let path = dir.join("tokenizer.proba");
    let source = "+++\nlocale = \"en\"\n+++\nstatic string F_KIND = \"gpt2\"\n";
    fs::write(&path, source).expect("write");
    let result = codemod_constants(&path, source).expect("codemod accepted");
    assert_eq!(result.converted, 1);
    assert_eq!(
        result.source,
        "+++\nlocale = \"en\"\n+++\nconst string F_KIND = \"gpt2\"\n"
    );
    let _ = fs::remove_dir_all(&dir);
}
