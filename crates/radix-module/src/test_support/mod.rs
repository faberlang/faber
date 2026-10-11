//! Shared unit-test helpers for `radix` crate-local tests.
//!
//! Factories use the `test_*` prefix; assertion helpers use `assert_*`; pipeline
//! helpers return results. See `docs/factory/unit-test-infrastructure-discovery/naming-analysis.md`.
//!
//! Language keyword corpus lives in-tree at `radix/corpus/` (single-file only;
//! see `docs/archived/corpus-split-radix-faber`). From this crate's
//! `CARGO_MANIFEST_DIR` (`crates/radix/`), that root is `../../corpus/`.
//! Package fixtures and package-shaped proofs live under `faber/corpus` —
//! not in this crate.

use crate::CompileResult;
use crate::codegen::Target;
use crate::diagnostics::{Diagnostic, DiagnosticArg};
use crate::driver::{Config, ModuleCompileRequest, Session, compile};
use crate::lexer::Span;
use std::path::{Path, PathBuf};

/// Root of the language keyword corpus (`radix/corpus/`).
#[must_use]
pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Path to one language corpus file relative to [`corpus_dir`]
/// (e.g. `incipit/salve-munde.fab`).
pub fn corpus_file(rel: impl AsRef<Path>) -> PathBuf {
    corpus_dir().join(rel)
}

/// Locates the in-tree runtime directory `<name>` (`rust`, `go`, `typescript`,
/// `swift`). `FABER_SUPPORT_PATH_OVERRIDE` names a root holding
/// `runtime/<name>` and wins when that directory exists; otherwise the repo's
/// own `runtime/<name>` (`CARGO_MANIFEST_DIR` is `<repo>/crates/radix-module`).
///
/// # Panics
///
/// Panics naming the path when the in-tree directory is absent.
#[must_use]
pub fn runtime_dir(name: &str) -> PathBuf {
    let overridden = std::env::var_os("FABER_SUPPORT_PATH_OVERRIDE")
        .map(|root| PathBuf::from(root).join("runtime").join(name))
        .filter(|runtime| runtime.is_dir());
    let runtime = overridden.unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../runtime")
            .join(name)
    });
    assert!(runtime.is_dir(), "runtime/{name} not found at {}", runtime.display());
    runtime.canonicalize().unwrap_or(runtime)
}

/// Opt-in gate and explicit crate-path override for the emitted-Rust cargo
/// smoke checks: they run only when this names the `runtime/rust` package.
pub const FABER_RUNTIME_CRATE_ENV: &str = "FABER_RUST_RUNTIME_CRATE";

/// Resolve the generated-Rust package for cargo path-deps in smoke tests.
///
/// Returns [`None`] when unset or not a crate root — callers must skip, not panic.
/// Monorepo CI should export [`FABER_RUNTIME_CRATE_ENV`] (see `scripta/test`).
#[must_use]
pub fn faber_runtime_crate_path() -> Option<PathBuf> {
    let raw = std::env::var_os(FABER_RUNTIME_CRATE_ENV)?;
    let path = PathBuf::from(raw);
    if !path.join("Cargo.toml").is_file() {
        return None;
    }
    Some(path.canonicalize().unwrap_or(path))
}

/// Default zero-width span for hand-built IR fixtures.
#[must_use]
pub fn test_span() -> Span {
    Span::default()
}

/// Byte span covering `start..end` in hand-built fixtures.
#[must_use]
pub fn test_span_between(start: u32, end: u32) -> Span {
    Span::new(start, end)
}

/// Driver session with default config for the requested backend.
///
/// Corpus and fixture sources carry explicit locale frontmatter, so
/// the session resolves the dev stdlib reader packs when the workspace
/// checkout provides them.
#[must_use]
pub fn test_session(target: Target) -> Session {
    Session::new(Config::default().with_target(target).with_dev_locale_root())
}

/// Cargo-managed temp root for test fixtures: `<target-dir>/tmp/`.
///
/// All test temp data must live under the cargo target tree so `cargo clean`
/// sweeps it. Prefers `CARGO_TARGET_TMPDIR` (set by cargo for integration
/// tests/benches/examples); otherwise derives `<target-dir>/tmp` from the
/// running test binary's own path (`<target-dir>/<profile>/deps/<crate>-<hash>`).
///
/// # Panics
///
/// Panics when neither source is available. There is deliberately no system-temp
/// fallback: leaked test temp dirs in `/tmp` have filled operator disk before,
/// so a test that cannot reach its managed root must die fast.
#[must_use]
pub fn cargo_managed_temp_root() -> PathBuf {
    let root = if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
        PathBuf::from(dir)
    } else if let Ok(exe) = std::env::current_exe() {
        // <target-dir>/<profile>/deps/<crate>-<hash> → <target-dir>/tmp
        let under_deps_dir = exe
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "deps" | "examples" | "benches"));
        if under_deps_dir {
            if let Some(target) = exe.parent().and_then(Path::parent).and_then(Path::parent) {
                target.join("tmp")
            } else {
                panic!(
                    "cargo_managed_temp_root: cannot walk up to the target dir from {}",
                    exe.display()
                );
            }
        } else {
            panic!(
                "cargo_managed_temp_root: test binary {} is not running from \
                 target/<profile>/deps|examples|benches",
                exe.display()
            );
        }
    } else {
        panic!("cargo_managed_temp_root: cannot resolve the test binary path");
    };
    std::fs::create_dir_all(&root).unwrap_or_else(|err| panic!("cargo_managed_temp_root: create {root:?}: {err}"));
    root
}

/// Remove stale `prefix`-named entries under the managed temp root, once per
/// process per prefix. Recovers dirs orphaned by crashed/killed test runs;
/// `cargo clean` is the primary sweep and this is the safety net for abnormal
/// termination (entries older than 24h).
///
/// # Panics
///
/// Panics if the sweep mutex is poisoned, or if the managed temp root cannot
/// be resolved by [`cargo_managed_temp_root`].
pub fn sweep_managed_temp(prefix: &str) {
    static SWEPT: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut swept = SWEPT.lock().expect("sweep lock");
    if swept.iter().any(|p| p == prefix) {
        return;
    }
    swept.push(prefix.to_owned());
    drop(swept); // do filesystem work outside the lock

    let Ok(entries) = std::fs::read_dir(cargo_managed_temp_root()) else {
        return;
    };
    let max_age = std::time::Duration::from_hours(24);
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with(prefix) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Owned temp directory that removes itself when dropped.
///
/// Wraps [`tempfile::TempDir`] rooted under [`cargo_managed_temp_root`] so
/// test fixtures cannot leak on the filesystem: normal runs drop-clean, and
/// crash orphans stay inside the cargo target tree where `cargo clean` (and
/// [`sweep_managed_temp`]) reclaim them.
/// # Panics
///
/// Panics when the managed temp root is unavailable or creation fails —
/// never falls back to the system temp dir (test/helper utilities).
#[must_use]
pub fn test_temp_dir(label: &str) -> tempfile::TempDir {
    sweep_managed_temp("radix-test-dir-");
    let prefix = format!("radix-test-dir-{label}-{}-", std::process::id());
    tempfile::Builder::new()
        .prefix(&prefix)
        .rand_bytes(8)
        .tempdir_in(cargo_managed_temp_root())
        .expect("create temp dir under the cargo-managed temp root")
}

/// Compact code/issue labels for assertion failure messages.
#[must_use]
pub fn diagnostic_facts(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diag| match (diag.code, diag.issue()) {
            (Some(code), Some(issue)) => format!("{code}:{issue}"),
            (Some(code), None) => code.to_string(),
            (None, Some(issue)) => issue.to_string(),
            (None, None) => format!("{:?}", diag.severity),
        })
        .collect()
}

/// Whether `diag` carries a named arg with the expected value.
#[must_use]
pub fn diagnostic_arg_eq(diag: &Diagnostic, name: &str, value: &str) -> bool {
    diag.args
        .iter()
        .any(|arg| arg.name == name && arg.value == value)
}

/// Assert that `diag` carries a named arg with the expected value.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_diagnostic_arg(diag: &Diagnostic, name: &str, value: &str) {
    assert!(
        diagnostic_arg_eq(diag, name, value),
        "expected diagnostic arg {name}={value:?}, got {:?}",
        diag.args
    );
}

/// Assert the full named-arg list on a normalized diagnostic.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_diagnostic_args_eq(diag: &Diagnostic, args: &[DiagnosticArg]) {
    assert_eq!(diag.args, args);
}

/// Whether structured args omit legacy prose attachments.
#[must_use]
pub fn args_have_no_legacy_message(args: &[DiagnosticArg]) -> bool {
    !args.iter().any(|arg| arg.name == "legacy_message")
}

/// Migration invariant: emit sites must not attach legacy prose args.
#[must_use]
pub fn diagnostic_has_no_legacy_message(diag: &Diagnostic) -> bool {
    args_have_no_legacy_message(&diag.args)
}

/// Assert structured args omit legacy prose attachments.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_args_have_no_legacy_message(args: &[DiagnosticArg], context: &str) {
    assert!(
        args_have_no_legacy_message(args),
        "{context}: args must not carry legacy_message"
    );
}

/// Assert an error diagnostic with the given stable code and issue slug.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_has_error_issue(diagnostics: &[Diagnostic], code: &str, issue: &str) {
    assert!(
        diagnostics
            .iter()
            .any(|diag| { diag.is_error() && diag.code == Some(code) && diag.issue() == Some(issue) }),
        "expected error {code} with issue {issue:?}, got {:?}",
        diagnostic_facts(diagnostics)
    );
}

/// Assert no diagnostic carries the given issue slug (regression / `no_longer_*` tests).
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_no_issue(diagnostics: &[Diagnostic], issue: &str) {
    assert!(
        !diagnostics.iter().any(|diag| diag.issue() == Some(issue)),
        "expected no diagnostic with issue {issue:?}, got {:?}",
        diagnostic_facts(diagnostics)
    );
}

/// Compile and assert success.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
#[must_use]
pub fn compile_ok(session: &Session, name: &str, source: &str) -> CompileResult {
    let result = compile(session, name, source, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "expected compile success for {name}, got {:?}",
        diagnostic_facts(&result.diagnostics)
    );
    result
}

/// Compile and assert failure.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
#[must_use]
pub fn compile_fail(session: &Session, name: &str, source: &str) -> CompileResult {
    let result = compile(session, name, source, ModuleCompileRequest::default());
    assert!(
        !result.success(),
        "expected compile failure for {name}, got {:?}",
        diagnostic_facts(&result.diagnostics)
    );
    result
}

/// Compile, expect failure, and assert an error issue slug is present.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_compile_fails_with_issue(session: &Session, name: &str, source: &str, issue: &str) {
    let result = compile_fail(session, name, source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
        "expected diagnostic issue '{issue}', got {:?}",
        diagnostic_facts(&result.diagnostics)
    );
}

/// Compile, expect failure, and assert a diagnostic carries exactly these args.
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_compile_fails_with_args(session: &Session, name: &str, source: &str, args: &[DiagnosticArg]) {
    let result = compile_fail(session, name, source);
    assert!(
        result.diagnostics.iter().any(|diag| diag.args == args),
        "expected diagnostic with args {args:?}, got {:?}",
        result.diagnostics
    );
}
