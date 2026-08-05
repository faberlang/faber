use super::{FNV1A64_OFFSET, FNV1A64_PRIME};

/// Standard FNV-1a 64-bit hash, used for source-identity hashes and the
/// generated runner's embedded-image fingerprint.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV1A64_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }
    hash
}

/// FNV-1a 64-bit per-blob provenance hash for a backend artifact (A9). Returns
/// the canonical `fnv64:<16-hex>` spelling used by source identities, so a
/// stored artifact hash is directly comparable across the text and binary
/// images. Reuses [`fnv1a64`]; backend blobs carry their own provenance and are
/// excluded from the same-package identity (§3.4).
#[must_use]
pub fn fnv1a64_blob_hash(blob: &[u8]) -> String {
    format!("fnv64:{:016x}", fnv1a64(blob))
}

/// SHA-256 same-package identity (A10 / §3.4): over (a) the source content
/// hashes and (b) the canonical backend-neutral device-program payload bytes.
/// Backend blobs are excluded (each carries its own FNV-1a provenance via
/// [`fnv1a64_blob_hash`]). Metal and CUDA routes derive identical semantic
/// bytes and therefore an identical identity hash.
///
/// # Canonicalization
///
/// The digest is taken over a length-prefixed, unambiguous byte stream so
/// distinct inputs never collide:
///
/// - for each source hash string (in image order): little-endian `u64` byte
///   length, then the UTF-8 bytes of the hash string;
/// - then the little-endian `u64` byte length of the canonical payload,
///   followed by the canonical payload bytes.
///
/// Returns the canonical `sha256:<64-hex>` spelling.
#[must_use]
pub fn device_identity_hash(source_hashes: &[&str], canonical_payload: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for source_hash in source_hashes {
        let bytes = source_hash.as_bytes();
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hasher.update((canonical_payload.len() as u64).to_le_bytes());
    hasher.update(canonical_payload);
    let digest = hasher.finalize();
    format!("sha256:{:x}", digest)
}
