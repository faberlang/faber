use super::{Sha256, sha256, sha256_hex};

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
