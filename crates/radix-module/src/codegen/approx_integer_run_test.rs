//! Run-level proof of the exact integer `≈` rule (audit1-e).
//!
//! `10^9 · |a − b| ≤ max(|a|, |b|)` is hand-written on every backend, and each
//! backend's emitted-text tests pin only its shape: a truncating 64-bit product
//! or an `abs(MIN)` overflow still passes them. These tests execute the
//! emitted program of `operatores/approx-integers.fab` (opposite-sign pairs,
//! `i64::MIN` / `i32::MIN` pairs, `u64` pairs whose scaled product passes
//! 2^64) and compare stdout with the sibling `.expected`, which the MIR runner
//! produces (the `runner golden` lane re-proves it). A missing toolchain skips
//! with a notice, like the Python `inf` tests.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};
use crate::test_support::test_temp_dir;
use std::process::Command;

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/operatores/approx-integers.fab"
));
const EXPECTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/operatores/approx-integers.expected"
));

fn emitted(target: Target) -> String {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let result = crate::driver::compile(&session, "approx-integers.fab", SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "{:?}", result.diagnostics);
    match result.output {
        Some(crate::Output::Rust(out)) => out.code,
        Some(crate::Output::Go(out)) => out.code,
        Some(crate::Output::Swift(out)) => out.code,
        Some(crate::Output::Python(out)) => out.code,
        _ => panic!("no emitted program for {target:?}"),
    }
}

fn stdout_of(mut command: Command, label: &str) -> String {
    let output = command
        .output()
        .unwrap_or_else(|err| panic!("{label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

fn tool_present(tool: &str, probe: &str) -> bool {
    Command::new(tool)
        .arg(probe)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn skip(label: &str, what: &str) {
    eprintln!("{what} not available; skipping run of {label}");
}

/// The first interpreter at or above 3.12 (the Python floor of the PY5 lane).
fn python_312() -> Option<&'static str> {
    ["python3.14", "python3.13", "python3.12", "python3"]
        .into_iter()
        .find(|candidate| {
            Command::new(candidate)
                .args([
                    "-c",
                    "import sys; raise SystemExit(0 if sys.version_info >= (3, 12) else 1)",
                ])
                .output()
                .is_ok_and(|output| output.status.success())
        })
}

#[test]
fn rust_prints_the_runner_verdicts() {
    let code = emitted(Target::HirRust);
    if crate::test_support::faber_runtime_crate_path().is_none() {
        skip("approx-integers (rust)", crate::test_support::FABER_RUNTIME_CRATE_ENV);
        return;
    }
    let binary = crate::backend_smoke::compile_emitted_rust_binary(&code, "approx-integers");
    assert_eq!(stdout_of(Command::new(binary), "approx-integers (rust)"), EXPECTED);
}

/// go.mod for the emitted module; a program importing `faber/rt` resolves it
/// through the in-tree `runtime/go` module (as the Go e2e lane does).
fn go_mod(code: &str) -> String {
    let mut content = "module smoke\n\ngo 1.22\n".to_owned();
    if code.contains("\"faber/rt\"") {
        let runtime = crate::test_support::runtime_dir("go");
        content.push_str("require faber/rt v0.0.0\n");
        content.push_str(&format!("replace faber/rt => {}\n", runtime.display()));
    }
    content
}

#[test]
fn go_prints_the_runner_verdicts() {
    let code = emitted(Target::HirGo);
    if !tool_present("go", "version") {
        skip("approx-integers (go)", "go");
        return;
    }
    let dir = test_temp_dir("approx-integers-go");
    std::fs::write(dir.path().join("go.mod"), go_mod(&code)).expect("write go.mod");
    std::fs::write(dir.path().join("main.go"), code).expect("write main.go");
    let mut command = Command::new("go");
    command
        .env("GOFLAGS", "-buildvcs=false")
        .args(["run", "."])
        .current_dir(dir.path());
    assert_eq!(stdout_of(command, "approx-integers (go)"), EXPECTED);
}

#[test]
fn swift_prints_the_runner_verdicts() {
    let code = emitted(Target::HirSwift);
    if !tool_present("swiftc", "--version") {
        skip("approx-integers (swift)", "swiftc");
        return;
    }
    let dir = test_temp_dir("approx-integers-swift");
    let source = dir.path().join("main.swift");
    let binary = dir.path().join("approx-integers");
    std::fs::write(&source, code).expect("write main.swift");
    let mut compile = Command::new("swiftc");
    compile.arg(&source).arg("-o").arg(&binary);
    stdout_of(compile, "approx-integers (swiftc)");
    assert_eq!(stdout_of(Command::new(binary), "approx-integers (swift)"), EXPECTED);
}

#[test]
fn python_prints_the_runner_verdicts() {
    let code = emitted(Target::HirPython);
    let Some(python) = python_312() else {
        skip("approx-integers (python)", "python3 >= 3.12");
        return;
    };
    let dir = test_temp_dir("approx-integers-python");
    let path = dir.path().join("main.py");
    std::fs::write(&path, code).expect("write main.py");
    let mut command = Command::new(python);
    command.arg(&path);
    assert_eq!(stdout_of(command, "approx-integers (python)"), EXPECTED);
}

/// A genus `static` field is a Rust associated const; its `≈` above 2^53 takes
/// the exact `const fn` helper, and the emitted program builds (rustc
/// const-checks every const item, so a non-const call would fail here).
#[test]
fn rust_static_field_approx_is_const_exact() {
    let source = "+++\nlocale = \"en\"\n+++\nclass Cfg {\n    static bool CLOSE = 9007199254740993 ≈ 9007199254740992\n    static bool FAR = 5 ≉ -5\n    mut int n\n}\nmain {\n    let Cfg c ← Cfg { n = 1 }\n    print c.n\n}\n";
    let session = Session::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_dev_locale_root(),
    );
    let result = crate::driver::compile(&session, "static-approx.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "{:?}", result.diagnostics);
    let Some(crate::Output::Rust(out)) = result.output else {
        panic!("no Rust output");
    };
    assert!(
        out.code.contains(
            "pub const CLOSE: bool = faber::exact::approx_int(9007199254740993_i128, 9007199254740992_i128);"
        ),
        "{}",
        out.code
    );
    assert!(
        out.code
            .contains("pub const FAR: bool = !faber::exact::approx_int(5_i128,"),
        "{}",
        out.code
    );
    assert!(!out.code.contains("1e-9"), "no f64 fallback in a const item:\n{}", out.code);
    if crate::test_support::faber_runtime_crate_path().is_none() {
        skip("static-approx (rust)", crate::test_support::FABER_RUNTIME_CRATE_ENV);
        return;
    }
    let binary = crate::backend_smoke::compile_emitted_rust_binary(&out.code, "static-approx");
    assert_eq!(stdout_of(Command::new(binary), "static-approx (rust)"), "1\n");
}
