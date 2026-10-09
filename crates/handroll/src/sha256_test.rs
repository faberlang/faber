use super::{
    BlocksFn, H0, Sha256, compress_blocks_portable, hardware_blocks, hex, sha256, sha256_hex,
};

/// FIPS 180-4 §B.1: the empty message.
#[test]
fn nist_empty_string() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

/// FIPS 180-4 §B.2: the two-block message "abc".
#[test]
fn nist_abc() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// FIPS 180-4 §B.3: the 448-bit message (single block, padding spills
/// into a second block).
#[test]
fn nist_448_bit_message() {
    let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    assert_eq!(input.len(), 56, "the 448-bit vector input is 56 bytes");
    assert_eq!(
        sha256_hex(input),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

/// 512-bit single-block boundary: a message whose length exactly equals
/// the 64-byte block size, fed via streaming in two 32-byte chunks.
/// (FIPS 180-4 §B.3's third SHA-256 example message is 112 bytes / 896
/// bits — not 512 — so this test uses the 64-byte prefix of that
/// message, the digest verified against an independent reference.)
#[test]
fn nist_512_bit_boundary_streamed() {
    let input = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmno";
    assert_eq!(input.len(), 64, "the 512-bit vector input is 64 bytes");
    let mut hasher = Sha256::new();
    hasher.update(&input[..32]);
    hasher.update(&input[32..]);
    assert_eq!(
        hasher.finalize_hex(),
        "2ff100b36c386c65a1afc462ad53e25479bec9498ed00aa5a04de584bc25301b"
    );
}

/// FIPS 180-4 §B.5: one million 'a' characters, fed via streaming in
/// 4096-byte chunks.
#[test]
fn nist_one_million_a_streamed() {
    let mut hasher = Sha256::new();
    let chunk = [b'a'; 4096];
    for _ in 0..(1_000_000 / 4096) {
        hasher.update(chunk);
    }
    hasher.update(&[b'a'; 4096][..1_000_000 % 4096]);
    assert_eq!(
        hasher.finalize_hex(),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

/// The one-shot and incremental surfaces agree, including odd chunk
/// splits through the partial-block buffer.
#[test]
fn one_shot_and_incremental_agree() {
    let data = b"streaming equivalence across arbitrary chunk boundaries";
    assert_eq!(
        sha256_hex(data),
        "28521e7ea00757cc599ee2e058ca1c8cec5baea7dde55bd3c0b5fa68ffb353d8"
    );
    let mut bytewise = Sha256::new();
    for &byte in data {
        bytewise.update([byte]);
    }
    let mut odd = Sha256::new();
    for chunk in data.chunks(7) {
        odd.update(chunk);
    }
    assert_eq!(bytewise.finalize(), sha256(data));
    assert_eq!(odd.finalize_hex(), sha256_hex(data));
}

/// Every backend this machine can run: the portable path always, plus the
/// hardware path when the CPU has it (aarch64 `sha2`).
fn backends() -> Vec<(&'static str, BlocksFn)> {
    let mut all: Vec<(&'static str, BlocksFn)> = vec![("portable", compress_blocks_portable)];
    if let Some(hardware) = hardware_blocks() {
        all.push(("hardware", hardware));
    }
    all
}

/// Digest `chunks` of `data` through one explicit backend.
fn digest_via(backend: BlocksFn, data: &[u8], chunk: usize) -> String {
    let mut hasher = Sha256::new();
    for piece in data.chunks(chunk.max(1)) {
        hasher.update_with(piece, backend);
    }
    hex(&hasher.finalize_with(backend))
}

/// Deterministic pseudo-random bytes (xorshift64*), no dependencies.
fn pseudo_random(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..len)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 56) as u8
        })
        .collect()
}

/// On aarch64 hardware with the SHA-2 extension the hardware path must be
/// selected; elsewhere it must be absent (so the portable path is the one
/// the public API uses).
#[test]
fn hardware_backend_selected_where_available() {
    #[cfg(target_arch = "aarch64")]
    assert_eq!(
        hardware_blocks().is_some(),
        std::arch::is_aarch64_feature_detected!("sha2")
    );
    #[cfg(not(target_arch = "aarch64"))]
    assert!(hardware_blocks().is_none());
}

/// The NIST vectors through every available backend, byte-identical digests.
#[test]
fn nist_vectors_through_every_backend() {
    let million_a = vec![b'a'; 1_000_000];
    let vectors: [(&[u8], &str); 4] = [
        (
            b"",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            &million_a,
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        ),
    ];
    for (name, backend) in backends() {
        for (data, expected) in vectors {
            assert_eq!(
                digest_via(backend, data, 4096),
                expected,
                "{name} backend, {} bytes",
                data.len()
            );
        }
    }
}

/// A 4 MB pseudo-random buffer, split at awkward chunk sizes, must produce
/// the portable single-shot digest on every backend and through the public
/// (dispatching) API.
#[test]
fn random_buffer_awkward_chunks_match_portable() {
    let data = pseudo_random(4 * 1024 * 1024 + 37);
    let reference = digest_via(compress_blocks_portable, &data, data.len());
    for (name, backend) in backends() {
        for chunk in [1_usize, 63, 64, 65, 4096] {
            // Chunk size 1 over 4 MB is slow in debug builds; bound it to a prefix
            // and compare against the portable digest of the same prefix.
            let slice = if chunk == 1 {
                &data[..65_537]
            } else {
                &data[..]
            };
            let expected = if chunk == 1 {
                digest_via(compress_blocks_portable, slice, slice.len())
            } else {
                reference.clone()
            };
            assert_eq!(
                digest_via(backend, slice, chunk),
                expected,
                "{name} backend, chunk {chunk}"
            );
        }
    }
    let mut public = Sha256::new();
    for piece in data.chunks(4099) {
        public.update(piece);
    }
    assert_eq!(public.finalize_hex(), reference);
}

/// Block-level agreement: the hardware compression of many blocks equals the
/// portable compression from the same starting state.
#[test]
fn block_runs_match_across_backends() {
    let data = pseudo_random(64 * 33);
    let mut expected = H0;
    compress_blocks_portable(&mut expected, &data);
    for (name, backend) in backends() {
        let mut state = H0;
        backend(&mut state, &data);
        assert_eq!(state, expected, "{name} backend");
    }
}
