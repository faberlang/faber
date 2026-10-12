//! Deterministic PRNG, std-only OS entropy, and RFC 4122 v4 UUIDs.
//!
//! Replaces `rand`/`getrandom` in the MIR runner (removal-delivery-spec U5b).
//! The generator and every sampling method are pure integer arithmetic —
//! deterministic across runs, platforms, and builds. OS entropy feeds only
//! the unseeded path ([`Rng::from_entropy`], [`os_entropy`]) and is documented
//! per source below. None of the sampling APIs fall back to wall-clock time.
//!
//! # Algorithm
//!
//! - Core generator: **xoshiro256\*\*** (Blackman & Vigna 2018,
//!   <https://prng.di.unimi.it/>). 256-bit state, 64-bit output, passes
//!   `BigCrush`. `next_u64` is the reference `(rotl(s[1]*5, 7)*9)` scrambler.
//! - Seeding: **`SplitMix64`** (Steele, Lea & Flood 2014) expands any `u64`
//!   seed into the four state words — the reference seeding recommendation.
//!   Contract: `seed_from_u64(s)` yields the identical sequence on every
//!   platform and build.
//! - Uniform ranges: 52-bit `[0, 1)` floats via IEEE-754 bit assembly (no
//!   precision-loss casts); inclusive `i64` ranges via Lemire's multiply-shift
//!   rejection (unbiased, any span including the full `2^64` range).
//!
//! **Not cryptographically secure** — never use for keys, nonces, or
//! signing (goal-d3-crypta keeps the aleator routes non-cryptographic).
//! [`os_entropy`] is the only kernel-backed source.

use std::io;

/// The seeded/unseeded generator replacing `rand::rngs::StdRng`.
pub struct Rng {
    state: [u64; 4],
}

impl Rng {
    /// Seed from a single `u64` via `SplitMix64` (deterministic, cross-platform).
    ///
    /// Contract: the same seed reproduces the identical sequence forever.
    #[must_use]
    pub fn seed_from_u64(seed: u64) -> Self {
        let mut sm = seed;
        let mut state = [0_u64; 4];
        for word in &mut state {
            *word = splitmix64(&mut sm);
        }
        Self { state }
    }

    /// Unseeded: 32 bytes from [`os_entropy`] (kernel CSPRNG where available).
    ///
    /// If the OS source is unreachable, falls back to a documented degraded
    /// seed ([`degraded_seed`]) — never time-based sampling inside the
    /// generator itself.
    #[must_use]
    pub fn from_entropy() -> Self {
        let mut seed = [0_u8; 32];
        let mut state = [0_u64; 4];
        if os_entropy(&mut seed).is_ok() {
            // 32 bytes split into four exact 8-byte words; no remainder.
            let (words, _) = seed.as_chunks::<8>();
            for (word, bytes) in state.iter_mut().zip(words) {
                *word = u64::from_le_bytes(*bytes);
            }
        }
        if state == [0_u64; 4] {
            // All-zero state makes xoshiro256** degenerate (outputs zero
            // forever). The kernel CSPRNG cannot realistically return 32 zero
            // bytes; this guard covers the degraded-source path only.
            Self::seed_from_u64(degraded_seed())
        } else {
            Self { state }
        }
    }

    /// Next 64-bit output — the xoshiro256\*\* step with the `**` scrambler.
    pub fn next_u64(&mut self) -> u64 {
        let state = &mut self.state;
        let result = state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = state[1] << 17;
        state[2] ^= state[0];
        state[3] ^= state[1];
        state[1] ^= state[2];
        state[0] ^= state[3];
        state[2] ^= t;
        state[3] = state[3].rotate_left(45);
        result
    }

    /// Fill `buf` with uniform bytes from the generator.
    pub fn fill_bytes(&mut self, buf: &mut [u8]) {
        let (chunks, remainder) = buf.as_chunks_mut::<8>();
        for chunk in chunks {
            chunk.copy_from_slice(&self.next_u64().to_le_bytes());
        }
        if !remainder.is_empty() {
            let bytes = self.next_u64().to_le_bytes();
            remainder.copy_from_slice(&bytes[..remainder.len()]);
        }
    }

    /// Uniform `f64` in `[0, 1)` — 52 random mantissa bits over `[1, 2)` minus
    /// 1.0. No float casts, so no precision loss beyond the intended 52 bits.
    pub fn gen_f64(&mut self) -> f64 {
        const ONE_HALF_OPEN: u64 = 0x3FF0_0000_0000_0000;
        f64::from_bits(ONE_HALF_OPEN | (self.next_u64() >> 12)) - 1.0
    }

    /// Uniform `i64` in `[lo, hi]` inclusive — any span, including the full
    /// `i64::MIN..=i64::MAX` range. Unbiased via multiply-shift rejection.
    pub fn gen_i64_inclusive(&mut self, lo: i64, hi: i64) -> i64 {
        debug_assert!(lo <= hi);
        // Number of outcomes in [lo, hi] computed mod 2^64: the full i64
        // span wraps to 0, which the `== 0` branch handles by drawing a raw
        // word directly.
        let span = hi
            .cast_unsigned()
            .wrapping_sub(lo.cast_unsigned())
            .wrapping_add(1);
        if span == 0 {
            lo.wrapping_add(self.next_u64().cast_signed())
        } else {
            let sample = self.next_u64_below(span);
            lo.wrapping_add(sample.cast_signed())
        }
    }

    /// Uniform `u64` in `[0, range)` — Lemire (2019) multiply-shift with
    /// rejection; no modulo bias. `range` must be non-zero.
    #[allow(clippy::cast_possible_truncation)]
    // Both casts take one half of the 128-bit product deliberately: `low` is
    // the low 64 bits, and the high 64 bits are < 2^64 by construction.
    fn next_u64_below(&mut self, range: u64) -> u64 {
        let mut x = self.next_u64();
        let mut product = u128::from(x) * u128::from(range);
        let mut low = product as u64;
        if low < range {
            let threshold = range.wrapping_neg() % range;
            while low < threshold {
                x = self.next_u64();
                product = u128::from(x) * u128::from(range);
                low = product as u64;
            }
        }
        (product >> 64) as u64
    }
}

/// `SplitMix64` step (Steele, Lea & Flood 2014). Increments the seed and
/// returns the next scrambled 64-bit word.
fn splitmix64(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Fill `buf` with fresh OS entropy. Replaces `getrandom::getrandom`.
///
/// - **Unix**: reads `/dev/urandom` (the kernel CSPRNG — the same source the
///   `getrandom` crate used).
/// - **Non-Unix**: no std OS-entropy API exists; a documented degraded seed
///   (wall-clock ⊕ process id ⊕ stack address) is expanded via `SplitMix64`.
///   This is **not** cryptographically secure and exists only so the runner's
///   unseeded aleator keeps working on platforms without `/dev/urandom`.
///
/// # Errors
///
/// Returns an I/O error when the OS entropy source cannot be opened or read
/// (Unix only; the non-Unix fallback always succeeds).
#[cfg(unix)]
pub fn os_entropy(buf: &mut [u8]) -> io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

/// Non-Unix fallback for [`os_entropy`] — documented degraded source, see the
/// Unix variant's docs for the security caveat.
#[cfg(not(unix))]
pub fn os_entropy(buf: &mut [u8]) -> io::Result<()> {
    let seed = degraded_seed();
    let mut sm = seed;
    for chunk in buf.chunks_mut(8) {
        let bytes = splitmix64(&mut sm).to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
    Ok(())
}

/// Wall-clock nanoseconds ⊕ process id ⊕ stack address. Used only when no OS
/// entropy source is reachable. NOT cryptographic (goal-d3-crypta); every
/// Unix build reaches `/dev/urandom` instead, so this is a last-resort path.
fn degraded_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    // Fold the 128-bit count into 64 bits: low half ⊕ high half.
    let bytes = nanos.to_le_bytes();
    let (halves, _) = bytes.as_chunks::<8>();
    let nanos_lo = u64::from_le_bytes(halves[0]);
    let nanos_hi = (nanos >> 64) as u64;
    let local = 0_u64;
    let stack = std::ptr::addr_of!(local) as usize as u64;
    nanos_lo ^ nanos_hi ^ stack ^ u64::from(std::process::id()).rotate_left(17)
}

/// RFC 4122 version-4 UUID string, `8-4-4-4-12` lowercase hex.
///
/// The 122 random bits come from a fresh [`os_entropy`] read per call — the
/// seeded [`Rng`] state never influences UUIDs, mirroring the runner's
/// aleator contract (uuid/octetos bypass `semina`). Version nibble pinned to
/// 4 (byte 6) and RFC 4122 variant bits to `10xx` (byte 8).
///
/// # Errors
///
/// Returns an I/O error when the OS entropy source cannot be read.
pub fn uuid_v4() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    os_entropy(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    ))
}

#[cfg(test)]
#[path = "entropy_test.rs"]
mod tests;
