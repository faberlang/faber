#![allow(clippy::absurd_extreme_comparisons)]

use std::fs;
use std::path::Path;

const MAX_UNWRAP: usize = 0;
const MAX_EXPECT: usize = 0;
const MAX_PANIC: usize = 0;
const MAX_UNREACHABLE: usize = 0;
const MAX_TODO: usize = 0;
const MAX_UNIMPLEMENTED: usize = 0;
const MAX_INLINE_TEST_MODULES: usize = 0;
const MAX_TEST_ATTR_IN_PRODUCTION: usize = 0;

struct SourceFile {
    path: String,
    content: String,
}

fn source_files() -> Vec<SourceFile> {
    let mut files = Vec::new();
    collect_rs_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files
}

fn collect_rs_files(dir: &Path, out: &mut Vec<SourceFile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.ends_with("_test.rs") || name.ends_with(".test.rs") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                out.push(SourceFile {
                    path: path.to_string_lossy().into_owned(),
                    content,
                });
            }
        }
    }
}

fn count_in_source(files: &[SourceFile], pattern: &str) -> usize {
    files
        .iter()
        .map(|file| {
            file.content
                .lines()
                .filter(|line| line.contains(pattern))
                .count()
        })
        .sum()
}

fn production_violations(files: &[SourceFile]) -> Vec<String> {
    let mut hits = Vec::new();
    for file in files {
        if file.content.contains("#[test]") || file.content.contains("#[tokio::test]") {
            hits.push(format!(
                "{}: contains #[test] in production file",
                file.path
            ));
        }
        if file.content.contains("#[cfg(test)]") && file.content.contains("mod tests {") {
            hits.push(format!("{}: inline #[cfg(test)] mod tests {{", file.path));
        }
    }
    hits
}

#[test]
fn unwrap_budget() {
    let files = source_files();
    let count = count_in_source(&files, ".unwrap()");
    assert!(
        count <= MAX_UNWRAP,
        ".unwrap() budget exceeded: found {count}, max {MAX_UNWRAP}."
    );
}

#[test]
fn expect_budget() {
    let files = source_files();
    let count = count_in_source(&files, ".expect(");
    assert!(
        count <= MAX_EXPECT,
        ".expect( budget exceeded: found {count}, max {MAX_EXPECT}."
    );
}

#[test]
fn panic_budget() {
    let files = source_files();
    let count = count_in_source(&files, "panic!(");
    assert!(
        count <= MAX_PANIC,
        "panic!( budget exceeded: found {count}, max {MAX_PANIC}."
    );
}

#[test]
fn unreachable_budget() {
    let files = source_files();
    let count = count_in_source(&files, "unreachable!(");
    assert!(
        count <= MAX_UNREACHABLE,
        "unreachable!( budget exceeded: found {count}, max {MAX_UNREACHABLE}."
    );
}

#[test]
fn todo_budget() {
    let files = source_files();
    let count = count_in_source(&files, "todo!(");
    assert!(
        count <= MAX_TODO,
        "todo!( budget exceeded: found {count}, max {MAX_TODO}."
    );
}

#[test]
fn unimplemented_budget() {
    let files = source_files();
    let count = count_in_source(&files, "unimplemented!(");
    assert!(
        count <= MAX_UNIMPLEMENTED,
        "unimplemented!( budget exceeded: found {count}, max {MAX_UNIMPLEMENTED}."
    );
}

#[test]
fn inline_test_modules_budget() {
    let files = source_files();
    let hits = production_violations(&files);
    let inline = hits
        .iter()
        .filter(|hit| hit.contains("inline #[cfg(test)]"))
        .count();
    assert!(
        inline <= MAX_INLINE_TEST_MODULES,
        "inline test module budget exceeded: found {inline}, max {MAX_INLINE_TEST_MODULES}. {hits:?}"
    );
}

#[test]
fn test_attr_in_production_budget() {
    let files = source_files();
    let hits = production_violations(&files);
    let attrs = hits
        .iter()
        .filter(|hit| hit.contains("contains #[test]"))
        .count();
    assert!(
        attrs <= MAX_TEST_ATTR_IN_PRODUCTION,
        "#[test] in production budget exceeded: found {attrs}, max {MAX_TEST_ATTR_IN_PRODUCTION}. {hits:?}"
    );
}
