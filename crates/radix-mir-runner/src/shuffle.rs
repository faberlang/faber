//! Iteration-order shuffle for `copia`/`tabula` (D19.2, D19.6; ledger O6).
//!
//! WHY: `copia` and `tabula` iteration order is language-unspecified, not
//! deliberately random (D19.2). `Value::Map`/`Value::Set` are backed by
//! `FxHashMap`/`FxHashSet`, which — unlike `std::collections::HashMap` — are
//! *not* randomized: their iteration order is a fixed function of the keys'
//! hashes. A program that quietly depends on that order would stay green
//! forever, even though the language makes no promise, because the runner
//! never varies it. This module is the perturbation: materialized map/set
//! snapshots are permuted, ON BY DEFAULT, so a program that depends on hash
//! order fails in the runner the way it would on a target.
//!
//! [`configure_shuffle_seed`] controls it:
//! - unconfigured: a random seed per process;
//! - `n` (n > 0): that seed, reproducing one order;
//! - `0`: shuffle OFF, the natural hash order.

use std::sync::OnceLock;

static SEED: OnceLock<Option<u64>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    /// Per-thread seed override so a test pins its own order instead of
    /// relying on the process default. `Some(seed)` replaces the cached seed.
    static OVERRIDE: std::cell::Cell<Option<Option<u64>>> = const { std::cell::Cell::new(None) };
}

/// Maps a shuffle-seed flag value to a seed: `0` is shuffle off, any other
/// value is that seed.
pub(crate) fn seed_from_flag(value: u64) -> Option<u64> {
    match value {
        0 => None,
        seed => Some(seed),
    }
}

/// Configures the process shuffle seed from an explicit caller-supplied value
/// (`0` = off). Returns `true` when this is the first call; a later call leaves
/// the seed set by the first in place (first call wins), so a nested
/// configuration cannot silently re-seed a running process.
#[must_use]
pub fn configure_shuffle_seed(value: u64) -> bool {
    SEED.set(seed_from_flag(value)).is_ok()
}

/// A per-process random non-zero seed from the standard library's randomly
/// keyed hasher.
fn random_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let seed = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    seed.max(1)
}

/// Reads the configured seed once per process and caches it; unconfigured
/// means a random seed per process.
fn shuffle_seed() -> Option<u64> {
    #[cfg(test)]
    if let Some(forced) = OVERRIDE.with(std::cell::Cell::get) {
        return forced;
    }
    *SEED.get_or_init(|| Some(random_seed()))
}

/// Runs `body` with the shuffle seed pinned on this thread (`None` = off).
#[cfg(test)]
pub(crate) fn with_seed<R>(seed: Option<u64>, body: impl FnOnce() -> R) -> R {
    let previous = OVERRIDE.with(|cell| cell.replace(Some(seed)));
    let result = body();
    OVERRIDE.with(|cell| cell.set(previous));
    result
}

/// splitmix64: a small, fast, well-mixed generator. Not a
/// security or statistical-quality requirement — determinism per seed is the
/// only property that matters here.
fn splitmix64(state: u64) -> u64 {
    let x = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let z = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Permutes `items` in place via Fisher-Yates under the process shuffle seed
/// (random unless [`configure_shuffle_seed`] pins it); a no-op when the seed
/// is `0` (off). Call this at every point a `copia`/`tabula` snapshot becomes an
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
/// process-wide configured seed. Split out so a unit test can compare two
/// seeds directly without needing two separate process invocations (the
/// process seed is cached for the process's lifetime).
pub(crate) fn permute<T>(items: &mut [T], seed: u64) {
    if items.len() < 2 {
        return;
    }
    let mut state = seed ^ (items.len() as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
    for i in (1..items.len()).rev() {
        state = splitmix64(state);
        // The remainder is at most `i`, so the conversion cannot fail; `i` is the identity fallback.
        let j = usize::try_from(state % (i as u64 + 1)).unwrap_or(i);
        items.swap(i, j);
    }
}
