//! FxHash: the fast non-cryptographic hasher behind `FxHashMap`/`FxHashSet`.
//!
//! Verbatim port of `rustc-hash` 2.1.3 (MIT OR Apache-2.0, algorithm by
//! Orson Peters) so hash values — and therefore `FxHashMap` iteration order
//! across the workspace — stay bit-identical to the replaced crate. Do not
//! "improve" the constants or the mixing steps in place: every persisted or
//! order-sensitive consumer assumes this exact function.
//!
//! The polynomial multiply-hash (`add_to_hash`), the wyhash-inspired byte
//! mixer (`hash_bytes`/`multiply_mix`), and the top-bits rotate in `finish`
//! are all copied unchanged. `FxHashMap`/`FxHashSet` are plain type aliases
//! over the std collections, matching the replaced crate's API surface the
//! workspace uses (`FxHashMap`, `FxHashSet`, `FxBuildHasher`, `FxHasher`).

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};

/// Type alias for a hash map that uses the Fx hashing algorithm.
pub type FxHashMap<K, V> = HashMap<K, V, FxBuildHasher>;

/// Type alias for a hash set that uses the Fx hashing algorithm.
pub type FxHashSet<V> = HashSet<V, FxBuildHasher>;

/// A speedy hash algorithm for use within a compiler. The hashmap in the
/// standard library by default uses `SipHash` which is not as speedy as we
/// want. In a compiler we are not worried about `DoS` attempts, so we use a
/// fast non-cryptographic hash.
///
/// The current implementation is a fast polynomial hash with a single
/// bit rotation as a finishing step designed by Orson Peters.
#[derive(Clone)]
pub struct FxHasher {
    hash: usize,
}

// One might view a polynomial hash
//    m[0] * k    + m[1] * k^2  + m[2] * k^3  + ...
// as a multilinear hash with keystream k[..]
//    m[0] * k[0] + m[1] * k[1] + m[2] * k[2] + ...
// where keystream k just happens to be generated using a multiplicative
// congruential pseudorandom number generator (MCG). For that reason we chose
// a constant that was found to be good for a MCG in:
//     "Computationally Easy, Spectrally Good Multipliers for Congruential
//     Pseudorandom Number Generators" by Guy Steele and Sebastiano Vigna.
#[cfg(target_pointer_width = "64")]
const K: usize = 0xf135_7aea_2e62_a9c5;
#[cfg(target_pointer_width = "32")]
const K: usize = 0x93d765dd;

#[inline]
fn le_u64(bytes: &[u8]) -> u64 {
    let mut out = [0_u8; 8];
    out.copy_from_slice(&bytes[..8]);
    u64::from_le_bytes(out)
}

#[inline]
fn le_u32(bytes: &[u8]) -> u32 {
    let mut out = [0_u8; 4];
    out.copy_from_slice(&bytes[..4]);
    u32::from_le_bytes(out)
}

impl FxHasher {
    /// Creates an `fx` hasher with a given seed.
    #[must_use]
    pub const fn with_seed(seed: usize) -> FxHasher {
        FxHasher { hash: seed }
    }

    /// Creates a default `fx` hasher.
    #[must_use]
    pub const fn default() -> FxHasher {
        FxHasher { hash: 0 }
    }

    #[inline]
    fn add_to_hash(&mut self, i: usize) {
        self.hash = self.hash.wrapping_add(i).wrapping_mul(K);
    }
}

impl Default for FxHasher {
    #[inline]
    fn default() -> FxHasher {
        Self::default()
    }
}

// The `as usize` casts mirror rustc-hash exactly: on 32-bit targets the
// high halves fold into the hash deliberately.
#[allow(clippy::cast_possible_truncation)]
impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // Compress the byte string to a single u64 and add to our hash.
        self.write_u64(hash_bytes(bytes));
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add_to_hash(i as usize);
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add_to_hash(i as usize);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add_to_hash(i as usize);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add_to_hash(i as usize);
        #[cfg(target_pointer_width = "32")]
        self.add_to_hash((i >> 32) as usize);
    }

    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.add_to_hash(i as usize);
        #[cfg(target_pointer_width = "32")]
        self.add_to_hash((i >> 32) as usize);
        self.add_to_hash((i >> 64) as usize);
        #[cfg(target_pointer_width = "32")]
        self.add_to_hash((i >> 96) as usize);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add_to_hash(i);
    }

    #[inline]
    fn finish(&self) -> u64 {
        // Since we used a multiplicative hash our top bits have the most
        // entropy (with the top bit having the most, decreasing as you go).
        // As most hash table implementations (including hashbrown) compute
        // the bucket index from the bottom bits we want to move bits from
        // the top to the bottom. Ideally we'd rotate left by exactly the
        // hash table size, but as we don't know this we'll choose 26 bits,
        // giving decent entropy up until 2^26 table sizes. On 32-bit hosts
        // we'll dial it back down a bit to 15 bits.

        #[cfg(target_pointer_width = "64")]
        const ROTATE: u32 = 26;
        #[cfg(target_pointer_width = "32")]
        const ROTATE: u32 = 15;

        self.hash.rotate_left(ROTATE) as u64
    }
}

// Nothing special, digits of pi.
const SEED1: u64 = 0x243f_6a88_85a3_08d3;
const SEED2: u64 = 0x1319_8a2e_0370_7344;
const PREVENT_TRIVIAL_ZERO_COLLAPSE: u64 = 0xa409_3822_299f_31d0;

// The widening `as` casts are infallible conversions clippy wants as `From`;
// the narrowing ones are the algorithm's deliberate 32-bit decomposition.
#[allow(clippy::cast_possible_truncation)]
#[inline]
fn multiply_mix(x: u64, y: u64) -> u64 {
    // The following code path is only fast if 64-bit to 128-bit widening
    // multiplication is supported by the architecture. Most 64-bit
    // architectures except SPARC64 and Wasm64 support it. However, the
    // target pointer width does not always indicate that we are dealing
    // with a 64-bit architecture, as there are ABIs that reduce the pointer
    // width, especially on AArch64 and x86-64. WebAssembly (regardless of
    // pointer width) supports 64-bit to 128-bit widening multiplication with
    // the `wide-arithmetic` proposal.
    if cfg!(any(
        all(
            target_pointer_width = "64",
            not(any(target_arch = "sparc64", target_arch = "wasm64")),
        ),
        target_arch = "aarch64",
        target_arch = "x86_64",
        all(target_family = "wasm", target_feature = "wide-arithmetic"),
    )) {
        // We compute the full u64 x u64 -> u128 product, this is a single
        // mul instruction on x86-64, one mul plus one mulhi on ARM64.
        let full = (u128::from(x)).wrapping_mul(u128::from(y));
        let lo = full as u64;
        let hi = (full >> 64) as u64;

        // The middle bits of the full product fluctuate the most with small
        // changes in the input. This is the top bits of lo and the bottom
        // bits of hi. We can thus make the entire output fluctuate with
        // small changes to the input by XOR'ing these two halves.
        lo ^ hi

        // Unfortunately both 2^64 + 1 and 2^64 - 1 have small prime factors,
        // otherwise combining with + or - could result in a really strong
        // hash, as:
        //     x * y = 2^64 * hi + lo = (-1) * hi + lo = lo - hi, (mod 2^64 + 1)
        //     x * y = 2^64 * hi + lo =    1 * hi + lo = lo + hi, (mod 2^64 - 1)
        // Multiplicative hashing is universal in a field (like mod p).
    } else {
        // u64 x u64 -> u128 product is prohibitively expensive on 32-bit.
        // Decompose into 32-bit parts.
        let lx = x as u32;
        let ly = y as u32;
        let hx = (x >> 32) as u32;
        let hy = (y >> 32) as u32;

        // u32 x u32 -> u64 the low bits of one with the high bits of the other.
        let afull = u64::from(lx).wrapping_mul(u64::from(hy));
        let bfull = u64::from(hx).wrapping_mul(u64::from(ly));

        // Combine, swapping low/high of one of them so the upper bits of
        // the product of one combine with the lower bits of the other.
        afull ^ bfull.rotate_right(32)
    }
}

/// A wyhash-inspired non-collision-resistant hash for strings/slices.
fn hash_bytes(bytes: &[u8]) -> u64 {
    let len = bytes.len();
    let mut s0 = SEED1;
    let mut s1 = SEED2;

    if len <= 16 {
        // XOR the input into s0, s1.
        if len >= 8 {
            s0 ^= le_u64(&bytes[0..8]);
            s1 ^= le_u64(&bytes[len - 8..]);
        } else if len >= 4 {
            s0 ^= u64::from(le_u32(&bytes[0..4]));
            s1 ^= u64::from(le_u32(&bytes[len - 4..]));
        } else if len > 0 {
            let lo = bytes[0];
            let mid = bytes[len / 2];
            let hi = bytes[len - 1];
            s0 ^= u64::from(lo);
            s1 ^= (u64::from(hi) << 8) | u64::from(mid);
        }
    } else {
        // Handle bulk (can partially overlap with suffix).
        let mut bulk = &bytes[..(len - 1)];
        while let Some((chunk, rest)) = bulk.split_first_chunk::<16>() {
            let x = le_u64(&chunk[..8]);
            let y = le_u64(&chunk[8..]);

            // Replace s1 with a mix of s0, x, and y, and s0 with s1.
            // This ensures the compiler can unroll this loop into two
            // independent streams, one operating on s0, the other on s1.
            //
            // Since zeroes are a common input we prevent an immediate
            // trivial collapse of the hash function by XOR'ing a constant
            // with y.
            let t = multiply_mix(s0 ^ x, PREVENT_TRIVIAL_ZERO_COLLAPSE ^ y);
            s0 = s1;
            s1 = t;
            bulk = rest;
        }

        let suffix = &bytes[len - 16..];
        s0 ^= le_u64(&suffix[0..8]);
        s1 ^= le_u64(&suffix[8..16]);
    }

    multiply_mix(s0, s1) ^ u64::try_from(len).unwrap_or(u64::MAX)
}

/// An implementation of [`BuildHasher`] that produces [`FxHasher`]s.
#[derive(Copy, Clone, Default, Debug)]
pub struct FxBuildHasher;

impl BuildHasher for FxBuildHasher {
    type Hasher = FxHasher;
    fn build_hasher(&self) -> FxHasher {
        FxHasher::default()
    }
}

#[cfg(test)]
#[path = "fx_test.rs"]
mod tests;
