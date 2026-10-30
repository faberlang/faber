//! Shared smoke helpers for focused driver, codegen, and MIR probe tests.
//!
//! **Not an ordinary correctness gate.** These helpers mutate the host (temp
//! projects, external compilers) and skip when toolchains are absent. Ordinary
//! lanes (`./scripta/test`, `./scripta/ci`, `.github/workflows/pr.yml`) must not
//! require them. Use the opt-in `./scripta/smoke-ci` lane when host tools may be
//! installed and emission validity must hard-fail.
//!
//! **Family A (HIR codegen):** Faber source → analyze → HIR codegen → smoke with the
//! target compiler (`rustc`, `go build`, `tsc` / `deno check`).
//!
//! **Family B (MIR probe):** Faber source → analyze → MIR lower → MIR emit → smoke with
//! `wasm-tools` / `llvm-as`.
//!
//! Missing toolchains skip with traced `eprintln!` and return; present toolchains
//! hard-fail on invalid artifacts via `assert!`. Skip/pass lines are also appended to
//! `target/smoke-trace.logl` for verification captures.

use crate::codegen::Target;
use crate::test_support::test_temp_dir;
use radix_codegen_shared::runtime_api::{self, GEN_BLOCK_ON, GEN_BLOCK_ON_TOKIO};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Cargo target dir for emitted-Rust smoke builds. Lives under the
/// cargo-managed temp root (`<target-dir>/tmp/smoke-target`) so `cargo clean`
/// sweeps it; there is deliberately no system-temp fallback.
fn smoke_cargo_target_dir() -> PathBuf {
    crate::test_support::cargo_managed_temp_root().join("smoke-target")
}

/// Temp project directory with automatic cleanup via [`tempfile::TempDir`].
fn smoke_temp_dir(label: &str) -> tempfile::TempDir {
    test_temp_dir(&format!("smoke-{label}"))
}

fn smoke_trace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/smoke-trace.logl")
}

fn trace_smoke_line(line: &str) {
    eprintln!("{line}");
    let path = smoke_trace_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        writeln!(file, "{line}").ok();
    }
}

fn io_ok<T>(result: io::Result<T>, ctx: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{ctx}: {err}"),
    }
}

fn command_ok(result: io::Result<Output>, ctx: &str) -> Output {
    match result {
        Ok(output) => output,
        Err(err) => panic!("{ctx}: {err}"),
    }
}

fn command_available(command: &str, args: &[&str]) -> bool {
    Command::new(command)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[must_use]
pub fn rustc_available() -> bool {
    command_available("rustc", &["--version"])
}

#[must_use]
pub fn cargo_available() -> bool {
    command_available("cargo", &["--version"])
}

#[must_use]
pub fn go_available() -> bool {
    command_available("go", &["version"])
}

#[must_use]
pub fn wasm_tools_available() -> bool {
    command_available("wasm-tools", &["--version"])
}

#[must_use]
pub fn llvm_as_available() -> bool {
    command_available("llvm-as", &["--version"])
}

#[must_use]
pub fn ts_typechecker_available() -> bool {
    command_available("tsc", &["--version"]) || command_available("deno", &["--version"])
}

#[must_use]
pub fn swiftc_available() -> bool {
    command_available("swiftc", &["--version"])
}

#[must_use]
pub fn python3_available() -> bool {
    command_available(
        "python3",
        &[
            "-c",
            "import sys; raise SystemExit(0 if sys.version_info >= (3, 12) else 1)",
        ],
    )
}

fn generated_rust_needs_tokio(code: &str) -> bool {
    // The `block_on` spellings come from the runtime table, never typed here.
    code.contains("tokio::")
        || [GEN_BLOCK_ON, GEN_BLOCK_ON_TOKIO]
            .iter()
            .filter_map(|row| runtime_api::spell(Target::HirRust, row).ok())
            .any(|spelled| code.contains(&spelled))
}

fn render_smoke_cargo_dependencies(code: &str, runtime: &Path) -> String {
    let needs_tokio = generated_rust_needs_tokio(code);
    // The `tokio` feature of the runtime crate carries `block_on_tokio`.
    let faber_features = if needs_tokio { ", features = [\"tokio\"]" } else { "" };
    let mut deps = format!("faber = {{ path = \"{}\"{faber_features} }}\n", runtime.display());
    if needs_tokio {
        deps.push_str("tokio = { version = \"1\", features = [\"rt\", \"net\", \"time\"] }\n");
    }
    deps
}

fn write_rust_cargo_project(project_dir: &Path, package_name: &str, code: &str, runtime: &Path) -> PathBuf {
    io_ok(fs::create_dir_all(project_dir.join("src")), "create cargo src dir");
    io_ok(fs::write(project_dir.join("src/main.rs"), code), "write generated rust");
    let manifest = format!(
        r#"[package]
name = "{package_name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{package_name}"
path = "src/main.rs"

[workspace]

[dependencies]
{deps}"#,
        deps = render_smoke_cargo_dependencies(code, runtime)
    );
    let manifest_path = project_dir.join("Cargo.toml");
    io_ok(fs::write(&manifest_path, manifest), "write Cargo.toml");
    manifest_path
}

const TSC_SMOKE_ARGS: &[&str] = &["--strict", "--noEmit", "--target", "ES2022", "--module", "commonjs"];
fn log_smoke_passed(label: &str, tool: &str) {
    trace_smoke_line(&format!("smoke passed: {label} via {tool}"));
}

fn log_smoke_skip(tool: &str, label: &str, action: &str) {
    trace_smoke_line(&format!("{tool} not found on PATH; skipping smoke {action} for {label}"));
}

/// Built `(rlib, deps)` pair for the in-tree `faber` runtime crate, shared by
/// the bare-`rustc` emitted-Rust lib checks.
///
/// The rlib is built once per test process into the smoke cargo target dir
/// (so `cargo clean` sweeps it). `None` (traced skip) when `cargo` cannot run;
/// a runtime crate that is present but fails to build hard-fails, because a
/// silently skipped compile proof would hide emission regressions.
fn faber_runtime_rlib() -> Option<(PathBuf, PathBuf)> {
    static RLIB: std::sync::OnceLock<Option<(PathBuf, PathBuf)>> = std::sync::OnceLock::new();
    RLIB.get_or_init(|| {
        if !cargo_available() {
            log_smoke_skip("cargo", "faber runtime rlib", "build");
            return None;
        }
        let runtime =
            crate::test_support::faber_runtime_crate_path().unwrap_or_else(|| crate::test_support::runtime_dir("rust"));
        let target_dir = smoke_cargo_target_dir().join("faber-runtime");
        let output = command_ok(
            Command::new("cargo")
                .args(["build", "--manifest-path"])
                .arg(runtime.join("Cargo.toml"))
                .arg("--target-dir")
                .arg(&target_dir)
                .output(),
            "build faber runtime rlib",
        );
        assert!(
            output.status.success(),
            "faber runtime rlib build failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let profile = target_dir.join("debug");
        let rlib = profile.join("libfaber.rlib");
        assert!(rlib.is_file(), "faber runtime rlib missing at {}", rlib.display());
        Some((rlib, profile.join("deps")))
    })
    .clone()
}

/// Type-checks emitted Rust library text with bare `rustc`, linking the
/// in-tree `faber` runtime crate (`faber::FaberZero`, `faber::trap`, …) the
/// way the corpus Rust leg does, and returns the compiler [`Output`] so a
/// caller can also assert on diagnostics (for example, warning-free arms).
///
/// `None` (traced skip) when `rustc` or the runtime rlib cannot run; the
/// opt-in toolchain convention of this module. Present toolchains hard-fail
/// on invalid artifacts via the caller's `assert!` on `status.success()`.
#[must_use]
pub fn rustc_typecheck_emitted_rust_lib(code: &str, label: &str) -> Option<Output> {
    if !rustc_available() {
        log_smoke_skip("rustc", label, "rustc lib typecheck");
        return None;
    }
    let Some((rlib, deps)) = faber_runtime_rlib() else {
        trace_smoke_line(&format!(
            "faber runtime rlib unavailable; skipping smoke rustc lib typecheck for {label}"
        ));
        return None;
    };
    let dir = smoke_temp_dir(label);
    let source_path = dir.path().join("lib.rs");
    io_ok(fs::write(&source_path, code), "write emitted rust");
    let output = command_ok(
        Command::new("rustc")
            .arg("--edition=2021")
            .arg("--crate-type=lib")
            .arg(&source_path)
            .arg("-o")
            .arg(dir.path().join("lib.rlib"))
            // The emitted text references `faber::…` unqualified, so the lib
            // check links the runtime crate explicitly — never by editing the
            // emitted text.
            .arg("--extern")
            .arg(format!("faber={}", rlib.display()))
            .arg("-L")
            .arg(format!("dependency={}", deps.display()))
            .output(),
        "execute rustc",
    );
    log_smoke_passed(label, "rustc lib");
    Some(output)
}

/// The in-tree Go runtime module (`runtime/go`). Mirrors `inf_audit_test.rs`.
fn go_runtime_dir() -> PathBuf {
    crate::test_support::runtime_dir("go")
}

fn go_mod_content(runtime: &Path) -> String {
    format!(
        "module smoke\n\ngo 1.22\n\nrequire faber/rt v0.0.0\n\nreplace faber/rt => {}\n",
        runtime.display()
    )
}

/// Appends a deterministic syntax-breaking trailer to a real emitted artifact.
#[must_use]
pub fn mangle_emitted_text(code: &str) -> String {
    format!("{code}\n/*smoke-regression-guard*/ __BROKEN_SYNTAX__((!!!")
}

/// Family A entry point: structural validation of full HIR-emitted source text.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_hir_emitted_smoke(target: Target, code: &str, label: &str) {
    match target {
        Target::HirRust => assert_emitted_rust_typechecks(code, label),
        Target::HirGo => assert_emitted_go_compiles(code, label),
        Target::HirTypeScript => assert_emitted_ts_typechecks(code, label),
        Target::HirSwift => assert_emitted_swift_typechecks(code, label),
        Target::HirPython => assert_emitted_python_parses(code, label),
        other => {
            panic!("assert_hir_emitted_smoke called with non-Family-A target {other:?}; use MIR smoke helpers instead")
        }
    }
}

/// Type-checks emitted Rust via a mini-Cargo project when `cargo` is on PATH.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_emitted_rust_typechecks(code: &str, label: &str) {
    if !cargo_available() {
        log_smoke_skip("cargo", label, "typecheck");
        return;
    }
    let Some(runtime) = crate::test_support::faber_runtime_crate_path() else {
        log_smoke_skip(
            "Faber Rust runtime",
            label,
            &format!(
                "typecheck (set {} to the runtime/rust package)",
                crate::test_support::FABER_RUNTIME_CRATE_ENV
            ),
        );
        return;
    };

    let dir = smoke_temp_dir(label);
    let package_name = format!("smoke_{}", label.replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
    let manifest_path = write_rust_cargo_project(dir.path(), &package_name, code, &runtime);
    let target_dir = smoke_cargo_target_dir();
    io_ok(fs::create_dir_all(&target_dir), "create smoke cargo target dir");

    let output = command_ok(
        Command::new("cargo")
            .arg("check")
            .arg("--manifest-path")
            .arg(&manifest_path)
            .arg("--target-dir")
            .arg(&target_dir)
            .output(),
        "execute cargo check",
    );
    assert!(
        output.status.success(),
        "cargo smoke typecheck failed for {label}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    log_smoke_passed(label, "cargo check");
}

/// Builds emitted Rust as an executable via a mini-Cargo project and returns the binary path.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
#[must_use]
pub fn compile_emitted_rust_binary(code: &str, label: &str) -> PathBuf {
    assert!(
        cargo_available(),
        "cargo not found on PATH; required for executable Rust smoke {label}"
    );
    let runtime = crate::test_support::faber_runtime_crate_path().unwrap_or_else(|| {
        panic!(
            "{} must point at the runtime/rust package for executable Rust smoke {label}",
            crate::test_support::FABER_RUNTIME_CRATE_ENV
        )
    });

    // Temp project source is cleaned up when `dir` drops. Compiled artifacts
    // land in the shared smoke target dir so the returned binary path survives.
    let dir = smoke_temp_dir(label);
    let package_name = format!("smoke_{}", label.replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
    let manifest_path = write_rust_cargo_project(dir.path(), &package_name, code, &runtime);
    let target_dir = smoke_cargo_target_dir();
    io_ok(fs::create_dir_all(&target_dir), "create smoke cargo target dir");

    let output = command_ok(
        Command::new("cargo")
            .arg("build")
            .arg("--manifest-path")
            .arg(&manifest_path)
            .arg("--target-dir")
            .arg(&target_dir)
            .output(),
        "execute cargo build",
    );
    assert!(
        output.status.success(),
        "cargo smoke executable build failed for {label}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    log_smoke_passed(label, "cargo build");

    let binary = target_dir
        .join("debug")
        .join(format!("{package_name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary.exists(),
        "cargo build did not produce expected binary {}",
        binary.display()
    );
    binary
}

/// Compiles emitted Go in a temp module when `go` is on PATH.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_emitted_go_compiles(code: &str, label: &str) {
    if !go_available() {
        log_smoke_skip("go", label, "compile");
        return;
    }

    let dir = smoke_temp_dir(label);
    let runtime = go_runtime_dir();
    io_ok(fs::write(dir.path().join("go.mod"), go_mod_content(&runtime)), "write go.mod");
    io_ok(fs::write(dir.path().join("main.go"), code), "write generated go");

    let output = command_ok(
        Command::new("go")
            // Generated smoke modules are temporary and have no VCS identity.
            // Disable Go's repository stamping so detached/worktree runs stay hermetic.
            .env("GOFLAGS", "-buildvcs=false")
            .args(["build", "-o", "/dev/null", "."])
            .current_dir(dir.path())
            .output(),
        "execute go build",
    );
    assert!(
        output.status.success(),
        "go smoke compile failed for {label}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    log_smoke_passed(label, "go build");
}

/// Type-checks emitted TypeScript with pinned flags when `tsc` or `deno` is on PATH.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_emitted_ts_typechecks(code: &str, label: &str) {
    if !ts_typechecker_available() {
        log_smoke_skip("tsc/deno", label, "typecheck");
        return;
    }

    let dir = smoke_temp_dir(label);
    let source = dir.path().join("main.ts");
    io_ok(fs::write(&source, code), "write generated typescript");

    let (tool, output) = if command_available("tsc", &["--version"]) {
        let mut command = Command::new("tsc");
        command
            .args(TSC_SMOKE_ARGS)
            .arg("main.ts")
            .current_dir(dir.path());
        ("tsc --strict --noEmit", command_ok(command.output(), "execute tsc"))
    } else {
        (
            "deno check",
            command_ok(
                Command::new("deno")
                    .args(["check", "main.ts"])
                    .current_dir(dir.path())
                    .output(),
                "execute deno check",
            ),
        )
    };
    assert!(
        output.status.success(),
        "TypeScript smoke typecheck failed for {label}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    log_smoke_passed(label, tool);
}

/// Compiles emitted Swift source with `swiftc` when it is on PATH.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_emitted_swift_typechecks(code: &str, label: &str) {
    if !swiftc_available() {
        log_smoke_skip("swiftc", label, "compile");
        return;
    }

    let dir = smoke_temp_dir(label);
    let source = dir.path().join("main.swift");
    io_ok(fs::write(&source, code), "write generated swift");

    let output = command_ok(
        Command::new("swiftc")
            .args(["-typecheck", &source.to_string_lossy()])
            .output(),
        "execute swiftc -typecheck",
    );
    assert!(
        output.status.success(),
        "Swift smoke typecheck failed for {label}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    log_smoke_passed(label, "swiftc");
}

/// Parses and compiles emitted Python with `python3` ≥ 3.12 when it is on PATH.
///
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_emitted_python_parses(code: &str, label: &str) {
    if !python3_available() {
        log_smoke_skip("python3>=3.12", label, "parse");
        return;
    }

    let dir = smoke_temp_dir(label);
    let source = dir.path().join("main.py");
    io_ok(fs::write(&source, code), "write generated python");

    let output = command_ok(
        Command::new("python3")
            .args([
                "-c",
                "import ast, pathlib, sys; p = pathlib.Path(sys.argv[1]); s = p.read_text(encoding='utf-8'); ast.parse(s); compile(s, str(p), 'exec')",
                source.to_str().expect("temp python path is utf-8"),
            ])
            .output(),
        "execute python3 ast.parse/compile",
    );
    assert!(
        output.status.success(),
        "Python smoke parse failed for {label}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    log_smoke_passed(label, "python3 ast.parse");
}

/// Family B entry point: validates Wasm module bytes with `wasm-tools validate`.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_mir_wasm_smoke(bytes: &[u8], label: &str) {
    if !wasm_tools_available() {
        log_smoke_skip("wasm-tools", label, "validate");
        return;
    }

    let dir = smoke_temp_dir(label);
    let wasm_file = dir.path().join("module.wasm");
    io_ok(fs::write(&wasm_file, bytes), "write wasm bytes");

    let output = command_ok(
        Command::new("wasm-tools")
            .args(["validate", &wasm_file.to_string_lossy()])
            .output(),
        "execute wasm-tools validate",
    );
    assert!(
        output.status.success(),
        "wasm-tools validate failed for {label}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    log_smoke_passed(label, "wasm-tools validate");
}

/// Family B entry point: round-trips WAT through `wasm-tools parse` then validates binary output.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_mir_wasm_text_smoke(wat: &str, label: &str) {
    if !wasm_tools_available() {
        log_smoke_skip("wasm-tools", label, "text round-trip");
        return;
    }

    let dir = smoke_temp_dir(label);
    let wat_file = dir.path().join("module.wat");
    let wasm_file = dir.path().join("module.wasm");
    io_ok(fs::write(&wat_file, wat), "write wat");

    let parse = command_ok(
        Command::new("wasm-tools")
            .args(["parse", &wat_file.to_string_lossy(), "-o", &wasm_file.to_string_lossy()])
            .output(),
        "execute wasm-tools parse",
    );
    assert!(
        parse.status.success(),
        "wasm-tools parse failed for {label}:\n{}",
        String::from_utf8_lossy(&parse.stderr)
    );

    let bytes = io_ok(fs::read(&wasm_file), "read parsed wasm");
    assert_mir_wasm_smoke(&bytes, label);
}

/// Family B entry point: assembles LLVM IR with `llvm-as` when available.
///
/// Opt-in only: ordinary suites must assert emitted text instead of calling
/// this. The remaining callers are the ignored manual verifier proof and the
/// `./scripta/smoke-ci` lane, where a missing tool skips with a traced line.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_mir_llvm_smoke(ll: &str, label: &str) {
    if !llvm_as_available() {
        log_smoke_skip("llvm-as", label, "assemble");
        return;
    }

    let dir = smoke_temp_dir(label);
    let ll_file = dir.path().join("module.ll");
    io_ok(fs::write(&ll_file, ll), "write llvm ir");

    let output = command_ok(
        Command::new("llvm-as")
            .args([&ll_file.to_string_lossy(), "-o", "/dev/null"])
            .output(),
        "execute llvm-as",
    );
    assert!(
        output.status.success(),
        "llvm-as smoke failed for {label}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    log_smoke_passed(label, "llvm-as");
}

#[cfg(test)]
#[cfg(feature = "mir-llvm")]
#[path = "backend_smoke_test.rs"]
mod tests;
