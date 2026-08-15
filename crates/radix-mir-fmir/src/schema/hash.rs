use crate::schema::{
    FmirArtifactReflection, FmirDeviceSymbol, FmirReflectedKernel, FmirReflectedResource,
    MaterializationStage,
};

/// SHA-256 digest, canonical `sha256:<64-hex>` spelling (DDCP2-U3 / contract
/// §`IdentityDomains` digest rules). Digests cover decoded bytes, never the
/// transport spelling — the base64 spelling, the length prefix, and the
/// encoding tag are transport, not payload.
#[must_use]
pub fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", handroll::sha256_hex(bytes))
}

/// `content_sha256` (DDCP2-U3 / contract §`IdentityDomains` rule 1): the
/// plain SHA-256 over the **canonical decoded raw payload bytes only** —
/// never the transport spelling (the unpadded base64 spelling, the length
/// prefix, and the encoding tag are transport, not payload). This is the
/// artifact's content identity.
#[must_use]
pub fn content_sha256(bytes: &[u8]) -> String {
    sha256_digest(bytes)
}

/// `packet_sha256` (DDCP2-U3 / contract §`IdentityDomains` rule 1): the
/// digest over the **versioned artifact metadata + `content_sha256`** —
/// ABI/schema version, backend, format, materialization stage, target id +
/// required features (canonical order), the entrypoint symbol map (canonical
/// order), the content digest, and the parent `compiler_input_packet_sha256`
/// provenance when present. Canonical map ordering, domain tags, target
/// normalization, and ABI-version inputs are normative inputs (rule 4).
/// **Never semantic identity** (the artifact packet is an artifact fact, not
/// a program semantic).
///
/// Backward-compatible reflection-less form: equivalent to
/// [`packet_sha256_canonical`] with `reflection: None` (a carried `None`
/// folds nothing into the digest). The canonical ordering enforcement is the
/// DDCP3-U0 (b) change — `required_features`/`entrypoints` are canonicalized
/// internally before hashing, so equivalent sets produce an identical packet
/// identity and a stored packet whose `packet_sha256` was computed over a
/// non-canonical order fails admission re-verification.
#[must_use]
#[allow(clippy::too_many_arguments)] // public packet-identity API — a params struct would change the public hashing surface
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
    packet_sha256_canonical(
        abi_version,
        backend,
        format,
        stage,
        target_id,
        required_features,
        entrypoints,
        None,
        content_sha256,
        compiler_input_packet_sha256,
    )
}

/// The canonical packet/admission identity (DDCP3-U0 (a)/(b); CTO9-6 a/b):
/// `packet_sha256` over the versioned artifact metadata + `content_sha256`
/// with `required_features`, `entrypoints`, and the typed `reflection`
/// (when present) **canonicalized internally before hashing** — canonical
/// map ordering + ABI-version inputs are normative inputs (contract
/// §`IdentityDomains` rule 4). Equivalent sets (differing caller order, no
/// duplicates) hash to an **identical** packet identity; a stored packet
/// whose `packet_sha256` was computed over a non-canonical order fails
/// admission re-verification. The typed reflection fold completes the frozen
/// §`ArtifactPacket` `backend reflection/requirements` field (canonical
/// order); a carried `None` folds nothing, so reflection-less packets keep
/// the DDCP2 identity. **Never semantic identity.**
#[must_use]
#[allow(clippy::too_many_arguments)] // public packet-identity API — a params struct would change the public hashing surface
pub fn packet_sha256_canonical(
    abi_version: u32,
    backend: &str,
    format: &str,
    stage: MaterializationStage,
    target_id: &str,
    required_features: &[String],
    entrypoints: &[FmirDeviceSymbol],
    reflection: Option<&FmirArtifactReflection>,
    content_sha256: &str,
    compiler_input_packet_sha256: Option<&str>,
) -> String {
    // Canonical order is ENFORCED before hashing (CTO9-6 b): the stored
    // caller order never affects the packet identity.
    let mut features: Vec<&str> = required_features.iter().map(String::as_str).collect();
    features.sort_unstable();
    let mut entries: Vec<&FmirDeviceSymbol> = entrypoints.iter().collect();
    entries.sort_unstable_by_key(|entry| (entry.entry.as_str(), entry.symbol.as_str()));

    let mut canonical = Vec::new();
    push_field(&mut canonical, "abi-version", &abi_version.to_le_bytes());
    push_field(&mut canonical, "backend", backend.as_bytes());
    push_field(&mut canonical, "format", format.as_bytes());
    push_field(&mut canonical, "stage", stage.spelling().as_bytes());
    push_field(&mut canonical, "target", target_id.as_bytes());
    for feature in &features {
        push_field(&mut canonical, "feature", feature.as_bytes());
    }
    for entrypoint in &entries {
        push_field(&mut canonical, "entrypoint", entrypoint.entry.as_bytes());
        push_field(&mut canonical, "symbol", entrypoint.symbol.as_bytes());
    }
    if let Some(reflection) = reflection {
        push_field(&mut canonical, "reflection-v", &reflection.v.to_le_bytes());
        let mut kernels: Vec<&FmirReflectedKernel> = reflection.kernels.iter().collect();
        kernels
            .sort_unstable_by_key(|kernel| (kernel.function.as_str(), kernel.entry_name.as_str()));
        for kernel in &kernels {
            push_field(
                &mut canonical,
                "reflected-kernel",
                kernel.function.as_bytes(),
            );
            push_field(
                &mut canonical,
                "reflected-entry",
                kernel.entry_name.as_bytes(),
            );
            let mut resources: Vec<&FmirReflectedResource> = kernel.resources.iter().collect();
            resources.sort_unstable_by_key(|resource| (resource.binding, resource.name.as_str()));
            for resource in &resources {
                push_field(
                    &mut canonical,
                    "reflected-resource",
                    resource.name.as_bytes(),
                );
                push_field(
                    &mut canonical,
                    "reflected-binding",
                    &resource.binding.to_le_bytes(),
                );
            }
        }
    }
    push_field(&mut canonical, "content-sha256", content_sha256.as_bytes());
    if let Some(parent) = compiler_input_packet_sha256 {
        push_field(&mut canonical, "parent-packet-sha256", parent.as_bytes());
    }
    sha256_digest(&canonical)
}

/// The pre-DDCP3-U0 packet digest — the versioned artifact metadata hashed
/// **in the caller's declared order** with no canonicalization and no
/// reflection fold. Retained only as the declared-order substrate: the
/// admission-negative fixtures (a stored packet whose `packet_sha256` was
/// computed over a non-canonical order must fail re-verification) and the
/// canonical function's sorted-input substrate. Not the packet identity.
#[doc(hidden)]
#[must_use]
#[allow(clippy::too_many_arguments)] // public packet-identity API — a params struct would change the public hashing surface
pub fn packet_sha256_in_declared_order(
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
    sha256_digest(&canonical)
}

/// Length-prefixed, domain-tagged field of the canonical packet byte stream
/// (contract §`IdentityDomains` rule 4: canonical map ordering + domain tags
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
    use handroll::Sha256;
    let mut hasher = Sha256::new();
    for source_hash in source_hashes {
        let bytes = source_hash.as_bytes();
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hasher.update((canonical_payload.len() as u64).to_le_bytes());
    hasher.update(canonical_payload);
    format!("sha256:{}", hasher.finalize_hex())
}
