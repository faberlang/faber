//! Ratchet: the emit/lowering crates never spawn toolchains.
//!
//! cuda-ptx-build-flip D5 (OQ3 corrected default). Radix's emit/lowering
//! boundary is text-out: artifacts are emitted as source text and the product
//! build owns every toolchain invocation (`compile_nvvm_to_ptx` moved
//! product-side in D2+D3). A `Command::new` in the scanned set is either a
//! toolchain dependency leaking back onto the emit path or an accidental
//! regression.
//!
//! Scanned set — every non-test `src` file under:
//!   - `crates/radix-mir-*` — all MIR backends + runner
//!   - `crates/radix-hir-*` — all HIR backends
//!   - `crates/radix-lower-ts`
//!   - `crates/radix-module/src/codegen`
//!
//! Allowlisted by policy (spawns are deliberate; entries only shrink):
//!   - `radix-mir-runner/src/kernel/processus.rs` — the `processus` provider
//!     (`exec`/`captura`/shell) is a Faber *language capability* kernel, not a
//!     toolchain dependency.
//!   - cfg(test) harness spawns — files compiled only under `#[cfg(test)]`
//!     (`radix-hir-python/src/golden.rs` pinned-interpreter checks; the scan's
//!     test-support convention already excludes `test_support` files).
//!
//! Documented out of scope (non-emit spawn surfaces, deliberately unpinned):
//!   - `radix-module/src/llvm_host.rs` — product toolchain discovery + link
//!     helper (llvm-config/llvm-as/clang), invoked from the faber pipeline.
//!   - `radix-module/src/aspect/{lean,tla}_checker.rs` — lake/lean/java aspect
//!     verifier runners.
//!   - `radix-lower-ts/src/driver.rs` — the `ts-import` node driver
//!     (default-off feature).
//!   - `crates/exempla` — corpus e2e harness (not an emit crate).
//!
//! `allowlisted_and_out_of_scope_surfaces_still_spawn` below asserts each
//! documented surface still contains its spawn so this documentation cannot
//! silently rot; the lists only shrink.

use hygiene_ratchet::{ScanConfig, collect_production_files, scan_command_new};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("hygiene-ratchet lives under crates/")
        .to_path_buf()
}

/// Emit/lowering crate families pinned by this ratchet, mirroring the D5
/// scope (`radix-mir-*`, `radix-hir-*`, `radix-lower-*`, `radix-module`
/// codegen). New emit/lowering backends must be added here deliberately.
fn scanned_roots(workspace: &Path) -> Vec<PathBuf> {
    const CRATES: &[&str] = &[
        "radix-mir",
        "radix-mir-coverage",
        "radix-mir-fmir",
        "radix-mir-llvm",
        "radix-mir-metal",
        "radix-mir-parity",
        "radix-mir-python",
        "radix-mir-runner",
        "radix-mir-sexp",
        "radix-mir-wasm",
        "radix-mir-wgsl",
        "radix-hir",
        "radix-hir-faber",
        "radix-hir-fhir",
        "radix-hir-go",
        "radix-hir-haskell",
        "radix-hir-lean",
        "radix-hir-python",
        "radix-hir-rust",
        "radix-hir-swift",
        "radix-hir-ts",
        "radix-lower-ts",
    ];
    let mut roots: Vec<PathBuf> = CRATES
        .iter()
        .map(|name| workspace.join("crates").join(name).join("src"))
        .collect();
    roots.push(
        workspace
            .join("crates")
            .join("radix-module")
            .join("src")
            .join("codegen"),
    );
    roots
}

#[test]
fn emit_crates_do_not_spawn_toolchains() {
    let workspace = workspace();
    let config = ScanConfig {
        source_roots: scanned_roots(&workspace),
        exclude_path_suffixes: vec![
            // Allowlisted: processus provider — Faber language capability
            // kernel (`exec`/`captura`/shell), not a toolchain dependency.
            "radix-mir-runner/src/kernel/processus.rs".to_string(),
            // cfg(test) harness spawns: pinned-interpreter golden checks.
            "radix-hir-python/src/golden.rs".to_string(),
            // Documented out of scope: the ts-import node driver (default-off
            // feature).
            "radix-lower-ts/src/driver.rs".to_string(),
        ],
        subtract_self_expect: true,
        check_companion_convention: false,
    };
    let files = collect_production_files(&config);
    let spawns = scan_command_new(&files);

    assert!(
        spawns.is_empty(),
        "emit/lowering crates must not spawn toolchains; found {}:\n{}",
        spawns.len(),
        spawns
            .iter()
            .map(|spawn| format!(
                "  {}:{}",
                display_relative(&workspace, &spawn.path),
                spawn.line
            ))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// Every allowlisted / out-of-scope surface documented above must still
/// contain its `Command::new`; otherwise its entry is stale and should be
/// removed from this ratchet, not kept as decoration.
#[test]
fn allowlisted_and_out_of_scope_surfaces_still_spawn() {
    let workspace = workspace();
    for rel in [
        "crates/radix-mir-runner/src/kernel/processus.rs",
        "crates/radix-hir-python/src/golden.rs",
        "crates/radix-lower-ts/src/driver.rs",
        "crates/radix-module/src/llvm_host.rs",
        "crates/radix-module/src/aspect/lean_checker.rs",
        "crates/radix-module/src/aspect/tla_checker.rs",
        "crates/exempla/src/exempla_e2e/llvm_runtime.rs",
    ] {
        let path = workspace.join(rel);
        let content = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {rel}: {err}"));
        let scrubbed = hygiene_ratchet::scrub_rust_source(&content);
        assert!(
            scrubbed.contains("Command::new"),
            "{rel} is documented as an allowlisted/out-of-scope spawn surface \
             but no longer contains Command::new; remove its entry from this ratchet",
        );
    }
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}
