//! FIPS 180-4 SHA-256, streaming.
//!
//! No caller yet: `lib.rs` declares the module and nothing uses it, so the
//! `#[allow(dead_code)]` markers below stay until the fs leaf's
//! `solum:digere` reads a file through this engine in fixed blocks. The
//! engine keeps at most one 64-byte block buffered and allocates nothing per
//! update, so a file of any size is hashed without loading it whole. Content
//! hash, not a secret: no side-channel hardening is attempted.

#[cfg(test)]
#[path = "sha256_test.rs"]
mod tests;

/// One SHA-256 block, in bytes (FIPS 180-4 §5.1.1).
const BLOCK_BYTES: usize = 64;

/// Round constants `K[0..64]` (FIPS 180-4 §4.2.2): the first 32 bits of the
/// fractional parts of the cube roots of the first 64 primes.
const ROUND_CONSTANTS: [u32; 64] = [
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
const INITIAL_STATE: [u32; 8] = [
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
#[allow(dead_code)]
pub(crate) struct Sha256 {
    state: [u32; 8],
    buffer: [u8; BLOCK_BYTES],
    buffered: usize,
    total_bytes: u64,
}

#[allow(dead_code)]
impl Sha256 {
    /// A fresh hasher in the FIPS 180-4 initial state.
    pub(crate) fn new() -> Self {
        Self {
            state: INITIAL_STATE,
            buffer: [0; BLOCK_BYTES],
            buffered: 0,
            total_bytes: 0,
        }
    }

    /// Feed `data` into the running digest. Any chunking is accepted; a
    /// partial block is buffered until it fills.
    pub(crate) fn update(&mut self, data: &[u8]) {
        self.total_bytes = self
            .total_bytes
            .wrapping_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        let mut rest = data;
        if self.buffered != 0 {
            let take = (BLOCK_BYTES - self.buffered).min(rest.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&rest[..take]);
            self.buffered += take;
            rest = &rest[take..];
            if self.buffered == BLOCK_BYTES {
                compress(&mut self.state, &self.buffer);
                self.buffered = 0;
            }
        }
        let (blocks, tail) = rest.as_chunks::<BLOCK_BYTES>();
        for block in blocks {
            compress(&mut self.state, block);
        }
        if !tail.is_empty() {
            self.buffer[..tail.len()].copy_from_slice(tail);
            self.buffered = tail.len();
        }
    }

    /// Finalize the digest, consuming the hasher. Standard padding: `0x80`,
    /// zeroes, then the big-endian bit length in the final 8 bytes.
    pub(crate) fn finalize(mut self) -> [u8; 32] {
        // The length field is the message length in bits, mod 2^64 (§5.1.2).
        let bit_len = self.total_bytes.wrapping_mul(8);
        // `buffered` is always below BLOCK_BYTES: `update` flushes a full block.
        self.buffer[self.buffered] = 0x80;
        self.buffered += 1;
        if self.buffered > 56 {
            self.buffer[self.buffered..].fill(0);
            compress(&mut self.state, &self.buffer);
            self.buffered = 0;
        }
        self.buffer[self.buffered..56].fill(0);
        self.buffer[56..BLOCK_BYTES].copy_from_slice(&bit_len.to_be_bytes());
        compress(&mut self.state, &self.buffer);
        let mut digest = [0_u8; 32];
        let (slots, _tail) = digest.as_chunks_mut::<4>();
        for (slot, word) in slots.iter_mut().zip(&self.state) {
            *slot = word.to_be_bytes();
        }
        digest
    }
}

/// Render a digest as 64 lowercase hex characters.
#[allow(dead_code)]
pub(crate) fn hex(digest: &[u8; 32]) -> String {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// One FIPS 180-4 compression over a single 64-byte block.
#[allow(clippy::many_single_char_names)] // FIPS 180-4 working-variable names (a–h)
fn compress(state: &mut [u32; 8], block: &[u8; BLOCK_BYTES]) {
    // Message schedule W[0..64] (§6.2.2 step 1).
    let mut w = [0_u32; 64];
    let (words, _tail) = block.as_chunks::<4>();
    for (word, bytes) in w.iter_mut().zip(words) {
        *word = u32::from_be_bytes(*bytes);
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
    for (&k, &wi) in ROUND_CONSTANTS.iter().zip(&w) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(k)
            .wrapping_add(wi);
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
    for (slot, word) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *slot = slot.wrapping_add(word);
    }
}
