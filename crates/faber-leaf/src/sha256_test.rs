use super::{Sha256, hex};

/// FIPS 180-4 §B.1: the empty message.
#[test]
fn sha256_empty_message() {
    let mut hasher = Sha256::new();
    hasher.update(b"");
    assert_eq!(
        hex(&hasher.finalize()),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

/// FIPS 180-4 §B.2: "abc".
#[test]
fn sha256_abc() {
    let mut hasher = Sha256::new();
    hasher.update(b"abc");
    assert_eq!(
        hex(&hasher.finalize()),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// FIPS 180-4 §B.3: 56 bytes, so padding and the length field spill into a
/// second block.
#[test]
fn sha256_448_bit_message() {
    let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    let mut hasher = Sha256::new();
    hasher.update(input);
    assert_eq!(
        hex(&hasher.finalize()),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

/// FIPS 180-4 §B.5: one million 'a' bytes, fed in 1000-byte updates.
#[test]
fn sha256_one_million_a() {
    let mut hasher = Sha256::new();
    let chunk = [b'a'; 1000];
    for _ in 0..1000 {
        hasher.update(&chunk);
    }
    assert_eq!(
        hex(&hasher.finalize()),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

/// The same 300-byte input, hashed in one update and in updates of 1, 63, 64,
/// and 65 bytes, must give one digest.
#[test]
fn sha256_chunking_agrees() {
    let mut input = Vec::with_capacity(300);
    while input.len() < 300 {
        input.extend_from_slice(b"The quick brown fox jumps over the lazy dog.");
    }
    input.truncate(300);

    let mut one_shot = Sha256::new();
    one_shot.update(&input);
    let expected = hex(&one_shot.finalize());

    for chunk in [1_usize, 63, 64, 65] {
        let mut hasher = Sha256::new();
        for piece in input.chunks(chunk) {
            hasher.update(piece);
        }
        assert_eq!(hex(&hasher.finalize()), expected, "chunk size {chunk}");
    }
}
