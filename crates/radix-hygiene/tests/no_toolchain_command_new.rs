//! Ratchet: the emit/lowering crates never spawn toolchains.
//!
//! cuda-ptx-build-flip D5 (OQ3 corrected default). Radix's emit/lowering
//! boundary is text-out: artifacts are emitted as source text and the product
//! build owns every toolchain invocation (`compile_nvvm_to_ptx` moved
//! product-side in D2+D3). A `Command::new` in the scanned set is either a
//! toolchain dependency leaking back onto the emit path or an accidental
//! regression.
//!
//! Scanned set — every non-test `src` file under workspace members named:
//!   - `radix-mir` / `radix-mir-*` — all MIR backends + runner
//!   - `radix-hir` / `radix-hir-*` — all HIR backends
//!   - `radix-lower-*`
//!   - `radix-codegen-shared`
//!
//! plus `crates/radix-module/src/codegen`.
//!
//! Allowlisted by policy (spawns are deliberate; entries only shrink):
//!   - `faber-leaf/src/process.rs` — the process leaf (`exec`/`captura`/shell)
//!     is a Faber language capability, not a toolchain dependency.
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

use radix_hygiene::{ScanConfig, collect_production_files, scan_command_new};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("radix-hygiene lives under crates/")
        .to_path_buf()
}

/// Discover emit/lowering package roots from Cargo's workspace membership,
/// not a second hand-maintained crate list.
fn scanned_roots(workspace: &Path) -> (Vec<PathBuf>, usize) {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace)
        .output()
        .expect("run cargo metadata to discover workspace members");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse cargo metadata JSON");
    let members = metadata["workspace_members"]
        .as_array()
        .expect("cargo metadata workspace_members array");
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages array");
    let member_ids: std::collections::HashSet<&str> = members
        .iter()
        .map(|id| id.as_str().expect("workspace member package ID"))
        .collect();

    let mut roots: Vec<PathBuf> = packages
        .iter()
        .filter(|package| {
            let id = package["id"].as_str().expect("package ID");
            if !member_ids.contains(id) {
                return false;
            }
            let name = package["name"].as_str().expect("package name");
            name == "radix-mir"
                || name.starts_with("radix-mir-")
                || name == "radix-hir"
                || name.starts_with("radix-hir-")
                || name.starts_with("radix-lower-")
                || name == "radix-codegen-shared"
        })
        .map(|package| {
            Path::new(package["manifest_path"].as_str().expect("manifest path"))
                .parent()
                .expect("package manifest directory")
                .join("src")
        })
        .collect();
    roots.push(
        workspace
            .join("crates")
            .join("radix-module")
            .join("src")
            .join("codegen"),
    );
    (roots, members.len())
}

#[test]
fn emit_crates_do_not_spawn_toolchains() {
    let workspace = workspace();
    let (source_roots, workspace_member_count) = scanned_roots(&workspace);
    eprintln!(
        "workspace members: {workspace_member_count}; emit/lowering roots: {}",
        source_roots.len() - 1
    );
    let config = ScanConfig {
        source_roots,
        exclude_path_suffixes: vec![
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
        "crates/faber-leaf/src/process.rs",
        "crates/radix-hir-python/src/golden.rs",
        "crates/radix-lower-ts/src/driver.rs",
        "crates/radix-module/src/llvm_host.rs",
        "crates/radix-module/src/aspect/lean_checker.rs",
        "crates/radix-module/src/aspect/tla_checker.rs",
        "crates/exempla/src/exempla_e2e/llvm_runtime.rs",
    ] {
        let path = workspace.join(rel);
        let content = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {rel}: {err}"));
        let scrubbed = radix_hygiene::scrub_rust_source(&content);
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
