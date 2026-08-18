//! Workspace container-root discovery.
//!
//! [`faberlang_home`] is the single canonical implementation of faberlang
//! container-root resolution. It is shared by the `faber` product crate and
//! the `exempla` harness crate (both depend on `radix`), so a container-layout
//! change is fixed in exactly one place instead of being mirrored across
//! per-crate copies.

use std::path::{Path, PathBuf};

/// Resolve the faberlang container root by walking ancestors of this crate.
///
/// The container root is the checkout that holds the sibling repos (`norma/`,
/// `examples/`, `gradus/`, `faber-web/`, `hosts/`, …). The walk is
/// depth-independent: it works from the deployed layout
/// (`…/faberlang/radix/crates/faber`) and from a hand-packet worktree
/// (`…/faberlang/worktrees/<lane>/radix/crates/faber`), where the radix repo
/// sits several levels deeper and the container siblings are not packet
/// members.
///
/// A directory qualifies when it holds `examples/` plus at least one of
/// `radix/` or `norma/`: the public examples repo is present at every real
/// container root, and the compiler/stdlib siblings distinguish the faberlang
/// container from an unrelated checkout. A `worktrees/<lane>/` packet root
/// is never the container — even when the packet happens to include
/// `examples/` plus `radix/` or `norma/` — so a partial-membership packet
/// cannot hide the real container's `hosts/`, `norma/`, or `gradus/`.
/// The cwd is probed as an additional start (after the crate manifest dir)
/// so harnesses run from a project directory resolve the container root too.
#[must_use]
pub fn faberlang_home() -> Option<PathBuf> {
    let mut starts = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    faberlang_home_from(starts)
}

/// Walk each start and its ancestors for the first real container root.
fn faberlang_home_from(starts: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for mut dir in starts {
        loop {
            if is_faberlang_container(&dir) {
                return Some(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

/// Packet roots live at `worktrees/<lane>/` and may carry a subset of the
/// container siblings; they are never the container.
fn is_worktree_packet_root(dir: &Path) -> bool {
    dir.parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "worktrees")
}

fn is_faberlang_container(dir: &Path) -> bool {
    if is_worktree_packet_root(dir) {
        return false;
    }
    let has_examples = dir.join("examples").is_dir();
    let has_radix = dir.join("radix").is_dir();
    let has_norma = dir.join("norma").is_dir();
    has_examples && (has_radix || has_norma)
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod tests;
