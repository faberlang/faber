//! SHA-256 (FIPS 180-4) — replaces the `sha2` crate in the default build
//! (removal-delivery-spec U5c).
//!
//! Standard iterative SHA-256: one-shot [`sha256`], lowercase-hex
//! convenience [`sha256_hex`], and the incremental [`Sha256`] hasher
//! (fed via [`Sha256::update`], finished via [`Sha256::finalize`]). These
//! are content hashes, not secrets — no side-channel hardening is
//! attempted, and data-dependent control flow is fine.

/// Round constants `K[0..64]` (FIPS 180-4 §4.2.2): the first 32 bits of the
/// fractional parts of the cube roots of the first 64 primes.
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// Initial hash state `H0` (FIPS 180-4 §5.3.3): the first 32 bits of the
/// fractional parts of the square roots of the first 8 primes.
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// Incremental SHA-256 hasher.
///
/// ```
/// let mut hasher = handroll::Sha256::new();
/// hasher.update(b"hello ");
/// hasher.update(b"world");
/// assert_eq!(
///     hasher.finalize_hex(),
///     "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
/// );
/// ```
#[derive(Debug, Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    total_bytes: u64,
}

impl Sha256 {
    /// A fresh hasher in the FIPS 180-4 initial state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: H0,
            buffer: [0; 64],
            buffered: 0,
            total_bytes: 0,
        }
    }

    /// Feed `data` into the running digest. Accepts anything that borrows as
    /// `&[u8]` (byte arrays and slices, like the replaced sha2 `Digest`
    /// trait).
    pub fn update(&mut self, data: impl AsRef<[u8]>) {
        let mut data = data.as_ref();
        self.total_bytes = self.total_bytes.wrapping_add(data.len() as u64);
        // Top off the partial block, if any.
        if self.buffered != 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                compress(&mut self.state, &self.buffer);
                self.buffered = 0;
            }
        }
        // Full blocks.
        while data.len() >= 64 {
            compress(&mut self.state, &data[..64]);
            data = &data[64..];
        }
        // Partial-block tail.
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    /// Finalize the digest, consuming the hasher.
    #[must_use]
    pub fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_bytes.wrapping_mul(8);
        // FIPS 180-4 §5.1.1 padding: `0x80`, zeroes, then the 64-bit
        // big-endian message length (in bits) in the final block.
        self.buffer[self.buffered] = 0x80;
        self.buffered += 1;
        if self.buffered > 56 {
            self.buffer[self.buffered..].fill(0);
            compress(&mut self.state, &self.buffer);
            self.buffered = 0;
        }
        self.buffer[self.buffered..56].fill(0);
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        compress(&mut self.state, &self.buffer);
        let mut digest = [0_u8; 32];
        let (outs, _) = digest.as_chunks_mut::<4>();
        for (out, &word) in outs.iter_mut().zip(&self.state) {
            out.copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    /// Finalize as 64 lowercase hex characters, consuming the hasher.
    #[must_use]
    pub fn finalize_hex(self) -> String {
        hex(&self.finalize())
    }
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

/// One-shot SHA-256 over `data` (FIPS 180-4). Accepts anything that borrows
/// as `&[u8]` (byte arrays, slices, and byte strings).
#[must_use]
pub fn sha256(data: impl AsRef<[u8]>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

/// One-shot SHA-256 over `data` as 64 lowercase hex characters.
#[must_use]
pub fn sha256_hex(data: impl AsRef<[u8]>) -> String {
    hex(&sha256(data))
}

/// Lowercase hex encoding with zero-padded bytes — matches the `{:x}`
/// formatting the replaced sha2 call sites used.
///
/// Public so callers that hold an already-computed digest (rather than the
/// hashed input) can render it, instead of carrying a private hex helper.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// One FIPS 180-4 compression round over a single 64-byte block.
#[allow(clippy::many_single_char_names)] // FIPS 180-4 working-variable names (a–h)
fn compress(state: &mut [u32; 8], block: &[u8]) {
    debug_assert_eq!(block.len(), 64);
    // Message schedule W[0..64] (§6.2.2 step 1).
    let mut w = [0_u32; 64];
    for (i, word) in w.iter_mut().take(16).enumerate() {
        *word = u32::from_be_bytes([
            block[4 * i],
            block[4 * i + 1],
            block[4 * i + 2],
            block[4 * i + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    // Working variables + the 64 rounds (§6.2.2 steps 2–3).
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    // Intermediate hash value update (§6.2.2 step 4).
    for (s, &word) in state.iter_mut().zip(&[a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(word);
    }
}

#[cfg(test)]
#[path = "sha256_test.rs"]
mod tests;
