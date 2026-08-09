use crate::schema::MaterializationStage;
use crate::schema::FmirDeviceSymbol;

/// SHA-256 digest with a domain tag, canonical `sha256:<64-hex>` spelling
/// (DDCP2-U3 / contract §IdentityDomains digest rules). Digests cover
/// decoded bytes, never the transport spelling — the base64 spelling, the
/// length prefix, and the encoding tag are transport, not payload.
#[must_use]
pub fn sha256_digest(domain: &str, bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update((domain.len() as u64).to_le_bytes());
    hasher.update(domain.as_bytes());
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("sha256:{digest:x}")
}

/// `content_sha256` (DDCP2-U3 / contract §IdentityDomains rule 1): the
/// digest over the **canonical decoded raw payload bytes only** — never the
/// transport spelling (the unpadded base64 spelling, the length prefix, and
/// the encoding tag are transport, not payload). This is the artifact's
/// content identity.
#[must_use]
pub fn content_sha256(bytes: &[u8]) -> String {
    sha256_digest("content", bytes)
}

/// `packet_sha256` (DDCP2-U3 / contract §IdentityDomains rule 1): the
/// digest over the **versioned artifact metadata + `content_sha256`** —
/// ABI/schema version, backend, format, materialization stage, target id +
/// required features (canonical order), the entrypoint symbol map (canonical
/// order), the content digest, and the parent `compiler_input_packet_sha256`
/// provenance when present. Canonical map ordering, domain tags, target
/// normalization, and ABI-version inputs are normative inputs (rule 4).
/// **Never semantic identity** (the artifact packet is an artifact fact, not
/// a program semantic).
#[must_use]
pub fn packet_sha256(
    abi_version: u32,
    backend: &str,
    format: &str,
    stage: MaterializationStage,
    target_id: &str,
    required_features: &[String],
    entrypoints: &[FmirDeviceSymbol],
    content_sha256: &str,
    compiler_input_packet_sha256: Option<&str>,
) -> String {
    let mut canonical = Vec::new();
    push_field(&mut canonical, "abi-version", &abi_version.to_le_bytes());
    push_field(&mut canonical, "backend", backend.as_bytes());
    push_field(&mut canonical, "format", format.as_bytes());
    push_field(&mut canonical, "stage", stage.spelling().as_bytes());
    push_field(&mut canonical, "target", target_id.as_bytes());
    for feature in required_features {
        push_field(&mut canonical, "feature", feature.as_bytes());
    }
    for entrypoint in entrypoints {
        push_field(&mut canonical, "entrypoint", entrypoint.entry.as_bytes());
        push_field(&mut canonical, "symbol", entrypoint.symbol.as_bytes());
    }
    push_field(&mut canonical, "content-sha256", content_sha256.as_bytes());
    if let Some(parent) = compiler_input_packet_sha256 {
        push_field(&mut canonical, "parent-packet-sha256", parent.as_bytes());
    }
    sha256_digest("packet", &canonical)
}

/// Length-prefixed, domain-tagged field of the canonical packet byte stream
/// (contract §IdentityDomains rule 4: canonical map ordering + domain tags
/// are normative inputs; length-prefixing keeps distinct fields
/// collision-free).
fn push_field(out: &mut Vec<u8>, tag: &str, bytes: &[u8]) {
    out.extend_from_slice(&(tag.len() as u64).to_le_bytes());
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// SHA-256 same-package identity (A10 / §3.4): over (a) the source content
/// hashes and (b) the canonical backend-neutral device-program payload bytes.
/// Backend blobs are excluded (each artifact carries its own content/packet
/// identity via [`content_sha256`] / [`packet_sha256`]). Metal and CUDA
/// routes derive identical semantic bytes and therefore an identical
/// identity hash.
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
    format!("sha256:{digest:x}")
}
