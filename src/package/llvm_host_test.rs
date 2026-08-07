//! Tests for the Stage 9 product `llvm-host` builder (S9.2–S9.5).
//!
//! The toolchain-dependent proofs (debug/release layout, manifest contents,
//! binary execution + exit forwarding) run only when a coherent LLVM toolchain
//! is discoverable on the host; otherwise they skip with a printed note so the
//! suite stays green on machines without llvm-as/clang. The pure host-triple
//! contract tests always run.

use super::*;
use crate::package::test_support::{test_temp_dir, TestDir};
use radix::codegen::Target;
use std::fs;
use std::process::Command;

const GREETING_FIXTURE: &str = r#"nota "Salve, Munde!""#;

const EXIT_FIXTURE: &str = r#"@ cli "exitura-smoke"
incipit argumenta args exitus 7 {
    nota "before exit"
}"#;

fn toolchain_available() -> bool {
    LlvmHostToolchain::discover().is_ok()
}

fn llvm_host_config() -> radix::Config {
    radix::Config::default().with_target(Target::MirLlvmHost)
}

/// Write a single-unit greeting fixture under a temp package dir and return
/// the `.fab` entry path. The directory name is fixed so the product identity
/// is deterministic.
fn write_greeting_fixture(dir: &TestDir) -> PathBuf {
    let package_dir = dir.join("salve-munde");
    fs::create_dir_all(&package_dir).expect("create salve-munde dir");
    let entry = package_dir.join("salve-munde.fab");
    fs::write(&entry, GREETING_FIXTURE).expect("write salve-munde.fab");
    fs::write(package_dir.join("salve-munde.expected"), "Salve, Munde!\n")
        .expect("write salve-munde.expected");
    entry
}

// ── host triple contract (always runs) ────────────────────────────────────

#[test]
fn llvm_host_triple_supports_native_host_pairs() {
    assert_eq!(
        host_triple_for("aarch64", "macos").as_deref(),
        Some("aarch64-apple-darwin")
    );
    assert_eq!(
        host_triple_for("x86_64", "macos").as_deref(),
        Some("x86_64-apple-darwin")
    );
    assert_eq!(
        host_triple_for("aarch64", "linux").as_deref(),
        Some("aarch64-unknown-linux-gnu")
    );
    assert_eq!(
        host_triple_for("x86_64", "linux").as_deref(),
        Some("x86_64-unknown-linux-gnu")
    );
}

#[test]
fn llvm_host_triple_rejects_unsupported_and_cross_hosts() {
    // Unsupported arch/os pairs and cross targets fail closed.
    for (arch, os) in [
        ("sparc64", "macos"),
        ("aarch64", "windows"),
        ("x86_64", "freebsd"),
        ("riscv64", "linux"),
    ] {
        assert_eq!(
            host_triple_for(arch, os),
            None,
            "{arch}-{os} must not be admitted"
        );
    }
}

#[test]
fn llvm_host_triple_diagnostic_is_structured() {
    let diagnostic = host_llvm_target_triple();
    if let Ok(triple) = diagnostic {
        assert!(!triple.is_empty());
        return;
    }
    // On an unsupported host the error must carry the structured issue.
    let diagnostic = match diagnostic {
        Err(diagnostic) => diagnostic,
        Ok(_) => unreachable!(),
    };
    assert_eq!(diagnostic.issue(), Some("E_LLVMHOST_UNSUPPORTED_HOST"));
}

// ── product build proofs (need llvm-as/clang on the host) ─────────────────

#[test]
fn llvm_host_debug_build_produces_inspectable_layout() {
    if !toolchain_available() {
        eprintln!("llvm-host test skipped: coherent LLVM toolchain not available");
        return;
    }
    let dir = test_temp_dir("llvm-host");
    let entry = write_greeting_fixture(&dir);
    let build = build_host_program(&llvm_host_config(), &entry, LlvmHostProfile::Debug)
        .expect("debug llvm-host build must succeed");

    // S9.4 layout: target/faber-llvm/debug/ with modules/, manifest, runtime/, binary.
    assert!(build.target_dir.ends_with("target/faber-llvm/debug"));
    assert!(build.binary_path.starts_with(&build.target_dir));
    assert!(build.binary_path.is_file(), "binary must exist");
    assert!(build.modules_dir.is_dir(), "modules dir must exist");
    assert_eq!(
        build.optimized_dir, None,
        "debug must not run the opt pipeline"
    );

    let module_count = fs::read_dir(&build.modules_dir)
        .expect("read modules dir")
        .count();
    assert_eq!(
        module_count, 1,
        "single-unit greeting fixture emits one .ll"
    );

    assert!(
        build.manifest_path.is_file(),
        "link-manifest.toml must exist"
    );
    let manifest = fs::read_to_string(&build.manifest_path).expect("read link manifest");
    assert!(
        manifest.contains("host_triple"),
        "manifest records host triple"
    );
    assert!(
        manifest.contains("profile = \"debug\""),
        "manifest records profile"
    );
    assert!(manifest.contains("llvm_as"), "manifest records llvm-as");
    assert!(manifest.contains("clang"), "manifest records clang");
    assert!(
        manifest.contains("runtime_archive"),
        "manifest records the archive"
    );
    assert!(manifest.contains(".ll"), "manifest records module paths");
    assert!(
        manifest.contains("native_flags") && manifest.contains("-g"),
        "debug manifest records the -g native flag"
    );
    assert!(
        !manifest.contains("pipeline"),
        "debug manifest must not record an opt pipeline"
    );

    let identity = build.target_dir.join("runtime/identity.toml");
    let identity = fs::read_to_string(&identity).expect("read runtime identity");
    assert!(identity.contains("faber-host-llvm"));
    assert!(identity.contains("archive"));
}

#[test]
fn llvm_host_release_build_pins_opt_pipeline() {
    if !toolchain_available() {
        eprintln!("llvm-host test skipped: coherent LLVM toolchain not available");
        return;
    }
    let dir = test_temp_dir("llvm-host");
    let entry = write_greeting_fixture(&dir);
    let build = build_host_program(&llvm_host_config(), &entry, LlvmHostProfile::Release)
        .expect("release llvm-host build must succeed");

    assert!(build.target_dir.ends_with("target/faber-llvm/release"));
    assert!(build.binary_path.is_file(), "release binary must exist");
    let optimized_dir = build
        .optimized_dir
        .expect("release must run the opt pipeline");
    let opt_count = fs::read_dir(&optimized_dir).expect("read opt dir").count();
    assert_eq!(
        opt_count, 1,
        "release runs the pinned pipeline over each module"
    );

    let manifest = fs::read_to_string(&build.manifest_path).expect("read link manifest");
    assert!(manifest.contains("profile = \"release\""));
    assert!(
        manifest.contains("opt") && manifest.contains("pipeline") && manifest.contains("-O2"),
        "release manifest must record the pinned opt -O2 pipeline"
    );
}

#[test]
fn llvm_host_binary_runs_and_forwards_exit_code() {
    if !toolchain_available() {
        eprintln!("llvm-host test skipped: coherent LLVM toolchain not available");
        return;
    }
    let dir = test_temp_dir("llvm-host");
    let greeting = write_greeting_fixture(&dir);
    let build = build_host_program(&llvm_host_config(), &greeting, LlvmHostProfile::Debug)
        .expect("greeting build must succeed");
    let run = Command::new(&build.binary_path)
        .output()
        .expect("run binary");
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "Salve, Munde!\n",
        "built binary stdout must match the fixture"
    );

    // S9.3: a fixture exiting 7 must produce exit code 7 from the binary.
    let package_dir = dir.join("exitura");
    fs::create_dir_all(&package_dir).expect("create exitura dir");
    let exit_entry = package_dir.join("exitura.fab");
    fs::write(&exit_entry, EXIT_FIXTURE).expect("write exitura.fab");
    let exit_build = build_host_program(&llvm_host_config(), &exit_entry, LlvmHostProfile::Debug)
        .expect("exitura build must succeed");
    let run = Command::new(&exit_build.binary_path)
        .output()
        .expect("run exitura binary");
    assert_eq!(
        run.status.code(),
        Some(7),
        "llvm-host binary must forward the program exit code"
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("before exit"),
        "stdout before exit must be forwarded"
    );
}

#[test]
fn llvm_host_debug_and_release_agree_on_greeting() {
    // S9.5 focused semantic parity: the same fixture produces the same stdout
    // in BOTH profiles (opt must not change behavior for the smoke fixture).
    if !toolchain_available() {
        eprintln!("llvm-host test skipped: coherent LLVM toolchain not available");
        return;
    }
    let dir = test_temp_dir("llvm-host");
    let entry = write_greeting_fixture(&dir);
    let debug = build_host_program(&llvm_host_config(), &entry, LlvmHostProfile::Debug)
        .expect("debug build must succeed");
    let release = build_host_program(&llvm_host_config(), &entry, LlvmHostProfile::Release)
        .expect("release build must succeed");

    let debug_run = Command::new(&debug.binary_path)
        .output()
        .expect("run debug binary");
    let release_run = Command::new(&release.binary_path)
        .output()
        .expect("run release binary");
    assert!(debug_run.status.success());
    assert!(release_run.status.success());
    assert_eq!(debug_run.stdout, release_run.stdout);
    assert_eq!(
        String::from_utf8_lossy(&release_run.stdout),
        "Salve, Munde!\n"
    );
}
