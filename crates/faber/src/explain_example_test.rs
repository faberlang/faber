//! Every `faber explain` example must survive `faber check`.
//!
//! The explain registry embeds one corpus exemplum per canonical term; each
//! rendered body carries exactly one fenced `fab` example. This test extracts
//! that example, reconstructs the exemplum's own corpus directory beside it
//! (so sibling embeds and module imports resolve), and runs the freshly built
//! `faber` binary's `check` command on it.
//!
//! An exemplum whose frontmatter declares a `diagnostic = "CODE:slug"` key is
//! a deliberate rejection: its example must fail `check` and name that code.
//! The assertion reads the structured code only, never diagnostic prose.

use crate::reference::ReferencePack;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Frontmatter keys this test reads from an exemplum source.
#[derive(Debug, Default, Deserialize)]
struct ExampleFrontmatter {
    diagnostic: Option<String>,
}

/// The freshly built `faber` binary that CLI-spawning tests run.
///
/// Integration-test targets get `CARGO_BIN_EXE_faber` from cargo, which builds
/// the binary before they run. Unit tests (lib and bin targets) do not, and a
/// plain `cargo test` never rebuilds `target/<profile>/faber` for them, so a
/// stale artifact would green or red these tests without testing the code
/// under change. Build it once per test process instead; a failed build fails
/// the test loudly, it never falls back to whatever binary is on disk.
///
/// This mirrors `crate::commands::format_test::faber_binary`; the bin target
/// owns that module, so a lib unit test cannot reach it across the crate
/// boundary.
fn faber_binary() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

    if let Ok(path) = std::env::var("CARGO_BIN_EXE_faber") {
        return PathBuf::from(path);
    }
    BUILT.get_or_init(build_faber_binary).clone()
}

fn build_faber_binary() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build
        .args(["build", "--quiet", "-p", "faber", "--bin", "faber"])
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    if !cfg!(debug_assertions) {
        build.arg("--release");
    }
    let status = build
        .status()
        .expect("spawn `cargo build -p faber --bin faber`");
    assert!(
        status.success(),
        "`cargo build -p faber --bin faber` failed; CLI-spawning tests need a fresh binary"
    );

    // The binary sits beside `deps/` of the running test executable (this
    // honors CARGO_TARGET_DIR and shared-target layouts).
    let exe = std::env::current_exe().expect("current test executable");
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("test executable lives under <target>/<profile>/deps");
    let binary = profile_dir.join(if cfg!(windows) { "faber.exe" } else { "faber" });
    assert!(
        binary.is_file(),
        "built faber binary not found at {}",
        binary.display()
    );
    binary
}

/// The corpus directory that owns an exemplum (`summa/summa.fab` → `corpus/summa`).
fn exemplum_dir(primary_path: &str) -> PathBuf {
    let exemplum = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(primary_path);
    exemplum
        .parent()
        .expect("exemplum path has a parent directory")
        .to_path_buf()
}

/// The first fenced `fab` block in a rendered entry body.
fn first_fab_example(body: &str) -> &str {
    const OPEN: &str = "```fab\n";
    let after_open = body
        .find(OPEN)
        .map(|index| index + OPEN.len())
        .expect("entry body carries a fab example");
    let rest = &body[after_open..];
    let end = rest
        .find("\n```")
        .expect("fab example carries a closing fence");
    &rest[..end]
}

/// The `diagnostic` code an exemplum frontmatter declares, if any.
fn expected_diagnostic(source: &str) -> Option<String> {
    let split = radix::module::driver::split_frontmatter(source).expect("split exemplum");
    let text = split.frontmatter_text?;
    let frontmatter: ExampleFrontmatter = toml::from_str(text).expect("parse exemplum frontmatter");
    frontmatter.diagnostic
}

/// Copy every regular file of a corpus directory into a scratch directory so
/// sibling embeds and relative imports resolve from a lone extracted example.
fn copy_corpus_files(source_dir: &Path, dest: &Path) {
    let entries = fs::read_dir(source_dir).expect("read exemplum directory");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            fs::copy(&path, dest.join(entry.file_name())).expect("copy corpus file");
        }
    }
}

/// Run `faber check` on `example`; return success and the combined output.
fn run_check(example: &Path) -> (bool, String) {
    let output = Command::new(faber_binary())
        .arg("check")
        .arg(example)
        .current_dir(example.parent().expect("example has a parent directory"))
        .output()
        .expect("run faber check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), combined)
}

/// Every canonical explain example compiles, except the exempla that declare an
/// expected diagnostic and must therefore fail with that exact code.
#[test]
fn every_explain_example_checks() {
    let pack = ReferencePack::embedded().expect("embedded reference pack");
    let registry = pack.build_registry().expect("embedded explain registry");

    let mut checked = 0usize;
    let mut failures = Vec::new();

    for entry in registry.entries().iter().filter(|entry| entry.canonical) {
        let record = pack
            .term(&entry.term)
            .expect("canonical entry has a term record");
        let source = pack
            .source(&record.primary)
            .expect("exemplum source is embedded");
        let example = first_fab_example(&entry.body);
        let diagnostic = expected_diagnostic(source);

        let dir = tempfile::tempdir().expect("create explain example directory");
        copy_corpus_files(&exemplum_dir(&record.primary), dir.path());
        let example_path = dir.path().join("__explain_example.fab");
        fs::write(&example_path, format!("{example}\n")).expect("write explain example");

        let (success, output) = run_check(&example_path);
        checked += 1;

        match diagnostic {
            Some(code) => {
                if success || !output.contains(&code) {
                    failures.push(format!(
                        "{} ({}) must fail with {code}: success={success}\n{output}",
                        entry.term, record.primary
                    ));
                }
            }
            None => {
                if !success {
                    failures.push(format!(
                        "{} ({}) must pass check\n{output}",
                        entry.term, record.primary
                    ));
                }
            }
        }
    }

    assert!(checked > 0, "registry has no canonical explain entries");
    assert!(
        failures.is_empty(),
        "explain examples failed check ({} of {checked}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
