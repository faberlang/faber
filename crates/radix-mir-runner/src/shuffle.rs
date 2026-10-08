//! Test-only iteration-order shuffle for `copia`/`tabula` (D19.2, D19.6).
//!
//! WHY: `copia` and `tabula` iteration order is language-unspecified, not
//! deliberately random (D19.2). `Value::Map`/`Value::Set` are backed by
//! `FxHashMap`/`FxHashSet`, which — unlike `std::collections::HashMap` — are
//! *not* randomized: their iteration order is a fixed function of the keys'
//! hashes. A golden that quietly depends on that order stays green forever,
//! even though the language makes no promise, because the runner never
//! varies it. This module is the perturbation: `RADIX_RUNNER_SHUFFLE_SEED`
//! is a test-only env var that permutes materialized map/set snapshots
//! deterministically per seed, so a golden lane run under two different
//! seeds disagrees unless the corpus sorted first (D1.7 `ordina`). Unset
//! (the default), this is a no-op — normal `faber run`/`faber test`
//! execution never engages it.

use std::sync::OnceLock;

static SEED: OnceLock<Option<u64>> = OnceLock::new();

/// Reads `RADIX_RUNNER_SHUFFLE_SEED` once per process and caches it.
fn shuffle_seed() -> Option<u64> {
    *SEED.get_or_init(|| {
        std::env::var("RADIX_RUNNER_SHUFFLE_SEED")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
    })
}

/// splitmix64: a small, fast, well-mixed generator. Test-only shuffle, not a
/// security or statistical-quality requirement — determinism per seed is the
/// only property that matters here.
fn splitmix64(state: u64) -> u64 {
    let x = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let z = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministically permutes `items` in place via Fisher-Yates when the
/// test-only shuffle seed (`RADIX_RUNNER_SHUFFLE_SEED`) is set; a no-op
/// otherwise. Call this at every point a `copia`/`tabula` snapshot becomes an
/// observable sequence (map/set → array conversion, key/value snapshots,
/// display formatting) so no corpus exemplum can quietly pin the runner's
/// otherwise-deterministic `FxHashMap`/`FxHashSet` order.
pub(crate) fn shuffle_unordered<T>(items: &mut [T]) {
    let Some(seed) = shuffle_seed() else {
        return;
    };
    permute(items, seed);
}

/// Pure Fisher-Yates permutation, seed as a plain argument rather than the
/// process-wide cached env var. Split out so a unit test can compare two
/// seeds directly without needing two separate process invocations (the env
/// var is read once and cached for the process's lifetime).
pub(crate) fn permute<T>(items: &mut [T], seed: u64) {
    if items.len() < 2 {
        return;
    }
    let mut state = seed ^ (items.len() as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
    for i in (1..items.len()).rev() {
        state = splitmix64(state);
        let j = usize::try_from(state % (i as u64 + 1)).expect("index below the slice length");
        items.swap(i, j);
    }
}
