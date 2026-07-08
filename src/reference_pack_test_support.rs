//! Shared helpers for disk-backed reference pack integration tests.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn repo_exempla_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../examples/corpus");
    assert!(
        root.join("index.toml").is_file(),
        "expected repo exempla at {}",
        root.display()
    );
    root
}
