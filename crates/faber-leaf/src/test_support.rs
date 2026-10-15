//! Test-only serialization for the process-global handle tables.
//!
//! `cargo test` runs tests in one binary in parallel; tests that open,
//! inspect, or clear a leaf's process-global handle table must hold this
//! guard for their whole body. Poisoning is recovered so one failed test
//! does not cascade into every later test.

use std::sync::{Mutex, MutexGuard, PoisonError};

static LOCK: Mutex<()> = Mutex::new(());

/// Exclusive access to the process-global handle tables.
pub(crate) fn handle_table_guard() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}
