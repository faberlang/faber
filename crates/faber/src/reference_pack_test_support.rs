//! Shared helpers for disk-backed reference pack integration tests.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn repo_exempla_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../../corpus");
    assert!(
        root.join("index.toml").is_file(),
        "expected language corpus at {}",
        root.display()
    );
    root
}

/// Resolve the faberlang container root by walking ancestors of this crate.
///
/// The container root is the checkout that holds the sibling repos
/// (`norma/`, `examples/`, `gradus/`, `faber-web/`, `faber-runtime/`). The
/// walk is depth-independent: it works both from the deployed layout
/// (`…/faberlang/radix/crates/faber`) and from a hand-packet worktree
/// (`…/faberlang/worktrees/<lane>/radix/crates/faber`), where the radix repo
/// sits several levels deeper and the container siblings are not packet
/// members. `norma` is the discriminator: it is present at every real
/// container root and never in a hand packet, so a packet that happens to
/// contain `examples/` and `radix/` (a test-lane style materialization) does
/// not masquerade as the container root and cut off `gradus/`/`faber-web/`
/// resolution. Mirrors `exempla::paths::faberlang_home`.
pub(crate) fn faberlang_home() -> Option<PathBuf> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while let Some(parent) = dir.parent() {
        dir = parent.to_path_buf();
        if dir.join("examples").is_dir() && dir.join("norma").is_dir() {
            return Some(dir);
        }
    }
    None
}
