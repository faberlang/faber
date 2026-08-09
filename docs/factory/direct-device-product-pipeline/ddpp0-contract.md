# DDPP0 Contract — Product shape, identity domains, canonical encoding, FNV removal

**Unit**: DDPP0-U2 (contract core — the contract-spine root for `ddpp0-contract.md`).
**Date**: 2026-08-08.
**Repo**: faber (control plane). `faber-runtime/` read-only; no product code; no cargo.
**Paired contract**: Radix DDCP0-U3 §ArtifactPacket/§IdentityDomains/§HotPathGate
(`radix/docs/factory/direct-device-compilation-pipeline/ddcp0-delivery.md`).
**Sections frozen here**: `## ProductShape`, `## IdentityDomains`,
`## CanonicalEncoding`, `## FnvRemoval`, `## RoundTripFixture`. The remaining
sections of this contract are frozen by DDPP0-U3 (§PerformanceInvariants,
§PreparedRegion, §SelectionPolicy, §EvidenceTiers) and DDPP0-U4
(§PartitionOwnership, §GeneratedRustSupport, §DeletionRule, §ChildRouting).

**Authority order** (campaign §Repo-Aware Baseline): live source/tests and live
`faber targets` → accepted artifact schemas + hardware receipts → **this phase's
frozen contracts** → campaign prose. Where a field is also frozen by DDCP0, the
DDCP0 contract text is the compiler-side authority and this contract records the
product-side reading of the same field; the two must agree field-by-field (DDPP0
phase gate; U6 records `PENDING-AGREEMENT` for any field that has not yet landed).

---

## ProductShape

**Frozen**: one analyzed Faber package produces exactly one `CompiledPackage`
with the following shape. It is the product-side reading of the campaign
Desired End State #1 tree and the DDCP0 §ArtifactPacket/§SemanticProgram fields.

```text
CompiledPackage
├── HostArtifact                  (exactly one)
├── DeviceProgram                 (optional, target-neutral, at most one)
├── DeviceArtifact[]              (zero or more, versioned)
├── host/device call facts        (host callsite + prepared-region identity)
├── submission-region facts       (prepared region identity + regime/shape class)
└── effect/capability requirements
```

### Elements

**`HostArtifact` — exactly one per `CompiledPackage`.**
The materialized host product for one host lane (for example `hir-rust` or
`mir-llvm`). Faber selects the host target, materializes the host module(s),
assembles, and links the product. The host module must omit physical device
kernel bodies; it declares direct backend-specific prepared-submission symbols
only (campaign §LLVM native materialization). Host/device selection are
independent axes (campaign Desired End State #2); an unsupported pair rejects
during planning and acquires no unrelated target dependencies.

**`DeviceProgram` — optional, at most one, target-neutral.**
The target-neutral semantic program produced by Radix MIR
(`radix-mir/src/device_program/` is the canonical semantic center). It carries
compiler-owned semantic facts — value generations, producer/consumer
dependencies, resource init/access/lifetime/mutation/observation, prepared
submission-region boundaries — never backend blobs. It is the semantic input
whose identity is the `device_identity_hash` domain (§IdentityDomains). Field
freeze: DDCP0-U2 §SemanticProgram.

**`DeviceArtifact[]` — zero or more, each versioned.**
Each entry is a backend artifact with its own `content_sha256` and
`packet_sha256` (versioned SHA-256 identities). A release/native build selects
one backend leaf by default; optional fat products select once at startup
(campaign Desired End State #4; the selection policy is frozen in
§SelectionPolicy by U3). Field freeze: DDCP0-U3 §ArtifactPacket — backend id,
format id, materialization stage (`compiler-input` | `finalized-binary`),
target id + required features, ABI/schema version, canonical raw bytes,
entrypoint symbol map, backend reflection/requirements, `content_sha256` +
`packet_sha256`. Faber materializes external-toolchain outputs (e.g. AMDGPU ELF
via the pinned external materializer), assembles, and links; it never
reconstructs compiler facts from emitted text or binary.

**Host/device call facts.**
Per-callsite identity (static symbol or fixed compiled region identity — no
route string, `Valor`, descriptor map, backend tag, or kernel name on the hot
path), typed input/output/resource bindings, effect/observation boundary, and
failure/cancellation behavior. Field freeze: DDCP0-U3 §ArtifactPacket
`HostDeviceCall`.

**Submission-region facts.**
One host call → one prepared submission region containing one or more kernels
(the NGAB0 major revision, H1). Each region has a compiler-owned
regime/shape-class identity; invocation carries only bounded numeric dynamic
fields validated against compiled bounds; cache keys = (artifact identity,
region identity, shape class); cache miss prepares outside the hot loop or
fails closed — it never interprets a kernel. Principle frozen here; the
prepared-region regime/shape policy is detailed in §PreparedRegion by U3 and in
DDCP0-U3 §ArtifactPacket.

**Effect/capability requirements.**
The package records the effect and capability requirements its product path
may exercise: ordinary `ad` host effects (preserved as the language's IO-bound
host-effect seam, campaign §`faber-runtime` Decomposition Target) and the
device capability requirements named in the selected `DeviceArtifact` (target
required features). GPU submission is statically unreachable from `ad`,
Sermo, `Valor`, `HostDispatch`, and route selection — it is a compiled
artifact path, not an effect-provider path.

**Assembly rule.** Faber validates the requested host/device pair and enabled
features before any toolchain work, then materializes target files, embeds
device bytes, and links one host support leaf and one selected device support
leaf (campaign §Product Build Pipeline). The product build plan never imports a
Hosts driver implementation and never reparses emitted target text.

---

## IdentityDomains

**Frozen**: exactly six identity domains. The table below is **field-for-field
the shared normative table** of the campaign §Artifact identity and of DDCP0-U3
§IdentityDomains: the same six domain names, the same authority-and-inputs, the
same migration rule. Any landed difference in DDCP0's table is caught at the
U6/U9 field-by-field agreement gate and recorded, never silently reconciled.

| Identity domain | Authority and inputs | Migration rule |
| --- | --- | --- |
| semantic `device_identity_hash` | Radix; target-neutral `DeviceProgram` semantics, excluding backend blobs | preserve as a separate domain; an algorithm/domain change needs its own major semantic-contract revision |
| artifact `content_sha256` | backend/materializer producer; finalized canonical payload bytes only | replaces only Faber's current FNV backend-artifact provenance and NGAB's byte-only `artifact_id` spelling |
| `packet_sha256` | Radix packet producer or Faber final-materialization producer; versioned artifact metadata plus `content_sha256` | new packet/admission identity; never becomes semantic program identity |
| `execution_descriptor_hash` | Radix typed call/region descriptor | preserve independently; artifact materialization cannot change its call semantics |
| distributed logical/bound-plan hashes | multi-device authority | outside this migration and unchanged by DDPP |
| support-archive ABI/content identity | Faber build plus Hosts-owned support source | versioned ABI plus SHA-256 content receipt; stale last-good archive reuse forbidden |

### Domain rules (normative)

1. **Digest domain separation.** `content_sha256` covers **canonical decoded raw
   payload bytes only** — the artifact's payload, nothing else. `packet_sha256`
   covers the **versioned artifact metadata plus `content_sha256`** — schema
   version, backend, format, materialization stage, target identity,
   `content_sha256`, entrypoint map, and canonical reflection/requirements —
   and is **never semantic identity**. The two domains are never conflated.
2. **NGAB `artifact_id` role.** NGAB0's current byte-only `artifact_id` is
   amended to the `content_sha256` role; it is not confused with packet
   identity. `packet_sha256` is the packet/admission identity (DDPP0-U6 applies
   the NGAB0 major revision).
3. **Canonicalization inputs (C1).** Canonical map ordering, domain tags, target
   normalization, and ABI-version inputs are normative inputs to the hashes.
   The canonical spellings are defined in §CanonicalEncoding.
4. **Hash-bound parent provenance (C1).** A finalized binary artifact that
   replaces an accepted compiler-input artifact preserves hash-bound
   `compiler_input_packet_sha256` parent provenance and re-verifies packet
   identity — provenance is never silently replaced (same-artifact evidence
   rule; cross-ref DDCP0-U6 fixtures).
5. **Cache admission.** Cache admission is never FNV64-only; it uses a
   collision-resistant digest (`content_sha256`/`packet_sha256`) or
   canonical-descriptor equality (DDCP0-U3 §HotPathGate).
6. **Support-archive identity.** The support-archive ABI/content identity is a
   distinct domain (row 6); its ABI version and SHA-256 content receipt policy
   is detailed by DDPP0-U8 and cross-referenced from §FnvRemoval.

---

## CanonicalEncoding

**Frozen**: the canonical byte spellings that the `content_sha256` /
`packet_sha256` digests cover. Hashes always cover **decoded bytes, never the
transport spelling** — a base64 or escaped-text spelling is never hashed in
place of the bytes it carries.

1. **Text payloads: canonical UTF-8.** Text payloads (e.g. MSL, PTX, WGSL,
   reflection text) are canonical UTF-8: valid UTF-8, no BOM, no
   alternative-encoding disguise. The canonical representation is the byte
   sequence itself; `content_sha256` covers exactly those bytes.
2. **Binary payloads in `fmir-text`: explicit declaration.** Binary payloads
   carried in `fmir-text` use an explicit schema-level binary encoding
   declaration with three parts:
   `binary:<encoding-tag>:<decoded-byte-length>:<canonical-unpadded-base64>`
   — an explicit binary **encoding tag** (identifying the byte-level encoding
   of the decoded payload, e.g. `raw`), the **decoded byte length**, and the
   **canonical unpadded base64** spelling of the decoded bytes. No binary is
   smuggled through UTF-8 or base64 without this schema-level encoding
   declaration (DDCP0-U3 §ArtifactPacket).
3. **Digest over decoded bytes.** `content_sha256` is computed over the decoded
   bytes (the `binary:` payload's byte length and byte content). The unpadded
   base64 spelling, the length prefix, and the encoding tag are transport
   spellings and are not hashed as if they were the payload. Decoding and
   re-encoding must reproduce the same canonical unpadded base64.
4. **Packet metadata canonicalization.** `packet_sha256` covers the versioned
   artifact metadata in canonical form: schema version, backend, format,
   materialization stage, target identity (normalized), `content_sha256`,
   entrypoint map, and canonical reflection/requirements. Map fields are
   ordered canonically (lexicographic key order); domain tags use their
   canonical spelling; target identities are normalized; the ABI/schema
   version is an input. The worked serialization is defined in
   §RoundTripFixture.
5. **Round-trip invariant.** Any consumer (Faber materialization, NGAB manifest
   row, Hosts admission) that receives a transport spelling decodes it back to
   the canonical bytes before verifying identity; a hash mismatch on the
   decoded bytes fails closed (DDCP0-U3 §ArtifactPacket / §HotPathGate).

---

## FnvRemoval

**Frozen — FNV removal default.** Legacy FNV backend-artifact provenance is
**removed in the coordinated schema migration** (the FMIR device schema +
consumer migration of DDCP0/DDPP). An **external-contract exemption requires
recorded evidence**; none is presumed. No implementation unit may keep an
FNV provenance path "temporarily" as a compatibility fallback.

### Removal scope (backend-artifact provenance, named in DDCP0-U1 inventory)

| # | Site | Role |
| --- | --- | --- |
| B1 | `radix/crates/radix-mir-fmir/src/schema/hash.rs` | `fnv1a64` + `fnv1a64_blob_hash` definitions; canonical `fnv64:<16-hex>` spelling |
| B2 | `radix/crates/radix-mir-fmir/src/schema/device.rs` | `FmirDeviceArtifact.hash` — FNV-1a 64-bit provenance hash field |
| B3 | `radix/crates/radix-mir-fmir/src/schema/admit.rs:87` | admission re-verifies `artifact.hash` vs `artifact.blob` via `fnv1a64_blob_hash` |
| B4 | `faber/src/package/device/section.rs:131` | Metal artifact FNV-1a provenance carried into the section |
| B5 | `faber/src/package/device/section.rs:139` | CUDA PTX blob provenance via `radix_mir_fmir::fnv1a64_blob_hash` |
| B6 | `faber/src/package/device/run.rs:297` | A9 receipt — `module hash fnv64:{:016x} …` provenance line |
| B7 | `faber/src/package/host_factory.rs:202–204` | `BackendDiscoveryReceipt.artifact_hash` — FNV-1a provenance of the declared artifact |

### Replacement

- **`content_sha256`** over canonical decoded payload bytes replaces the FNV
  backend-artifact provenance (B1–B7) for artifact identity and admission
  verification (admission re-verifies `content_sha256` against the canonical
  bytes).
- **`packet_sha256`** provides the versioned metadata/packet admission identity
  (NGAB `artifact_id` role; §IdentityDomains).
- **Cache admission** is never FNV64-only (§IdentityDomains rule 5).
- Schema fixtures citing `fnv64:` (radix schema/mod tests) are part of the
  removal awareness set and migrate with the schema.

### Separate surface (not backend-artifact provenance)

Source-identity FNV sites — `faber/src/package/mir/image.rs:353` (S1,
`fnv64_hex` over package source paths, the A10 identity's source half) and
`faber/src/package/mir/bin_runner.rs:85,177,193` (S2, embedded-image
fingerprint + `FmirTextSourceIdentity.hash`) — are a **separate source-identity
surface**, not the backend-artifact provenance removed here; they are listed for
scope completeness (DDCP0-U1 §5) and are outside this unit's removal decision.

### Exemption path

Any claim that a live external consumer makes FNV a real compatibility event
(campaign Open Question 7 surface) must produce **recorded evidence** — named
consumer, contract, and migration consequence — and be routed as a decision
through the campaign Open Questions path. No such evidence is presumed today;
the default stands: remove FNV in the coordinated schema migration, producers
and consumers migrate together (clean break; no FNV fallback or compatibility
translation survives closeout).

---

## RoundTripFixture

**Frozen**: a **spec-only, schema fixture** — no runnable code — that fixes the
canonical-encoding round trip across the FMIR / Faber / NGAB / Hosts surfaces:
the same canonical decoded bytes traverse the FMIR device wire (`fmir-text`),
the NGAB manifest row (as `content_sha256` in the `artifact_id` role), and
Hosts admission (hash-bound identity verification), always hashed over decoded
bytes. The worked example below is normative for the encoding spellings; its
declared `content_sha256` is verifiable with `shasum`.

### Worked example — binary payload (normative spelling)

Canonical decoded payload bytes (37 bytes, printable ASCII):

```text
faber.direct-device.region-fixture.v1
```

`fmir-text` transport spelling (encoding tag + decoded byte length + canonical
unpadded base64):

```text
binary:raw:37:ZmFiZXIuZGlyZWN0LWRldmljZS5yZWdpb24tZml4dHVyZS52MQ
```

Declared `content_sha256` — over the **decoded bytes** (not the base64 spelling):

```text
8140337f1952abf554671f4c996fc1d13536ce53575352194049912654a1ab86
```

Verification (spec-only, runnable anywhere):

```bash
printf '%s' 'faber.direct-device.region-fixture.v1' | shasum -a 256
# 8140337f1952abf554671f4c996fc1d13536ce53575352194049912654a1ab86
```

### Worked example — packet metadata and `packet_sha256`

Canonical packet metadata serialization (schema version, backend, format,
materialization stage, target identity, `content_sha256`, entrypoint map,
canonical reflection/requirements; map keys in lexicographic order, no
whitespace):

```text
{"schema_version":"ddpp0-2026-08-08","backend":"metal","format":"msl","materialization_stage":"finalized-binary","target":"metal-gpu-m2","content_sha256":"8140337f1952abf554671f4c996fc1d13536ce53575352194049912654a1ab86","entrypoints":{"region_compute":"kernel_fixture_v1"},"reflection":{"binding_count":2,"max_region_bytes":16384}}
```

Declared `packet_sha256` — over the canonical serialization above, which
includes the `content_sha256` of the payload:

```text
b0fe4bc5612b67e5f2d32efd69909bc0e72ff44903f09e250b57ba514d72271f
```

### Worked example — text payload (canonical UTF-8)

Canonical text payload (canonical UTF-8, hashed as its byte sequence):

```text
faber.fmir-text.region.declare v1; kernel region_compute; backend metal
```

Declared `content_sha256`:

```text
1aaec00a331c2a3d30c5a5f20c082c2d4b13a529bc4629e6649e54d3a9c5f888
```

Verification:

```bash
printf '%s' 'faber.fmir-text.region.declare v1; kernel region_compute; backend metal' | shasum -a 256
# 1aaec00a331c2a3d30c5a5f20c082c2d4b13a529bc4629e6649e54d3a9c5f888
```

### Cross-surface fixture contract (spec-only)

For each consumer surface, the round-trip invariant is: **transport spelling →
decoded canonical bytes → `content_sha256` (and `packet_sha256` for packet
admission) → identity verification against the declared value**. Concretely:

| Surface | Carries | Verifies |
| --- | --- | --- |
| FMIR device wire (`fmir-text`) | `binary:<tag>:<len>:<base64>` spelling or canonical UTF-8 text | decode to canonical bytes; `content_sha256` over decoded bytes |
| Faber materialization | canonical bytes embedded into the product | `content_sha256`/`packet_sha256` survive materialization unchanged; finalized artifacts preserve `compiler_input_packet_sha256` parent provenance |
| NGAB manifest row | `artifact_id` = `content_sha256`; `packet_sha256` row | packet identity admission before backend selection |
| Hosts admission | canonical bytes loaded from the embedded payload | hash-bound identity; load failure or identity mismatch fails closed |

The one-kernel fixture remains the minimal case, not the ABI limit (the
prepared-region granularity is the NGAB0 major revision applied by DDPP0-U6).
The full fixture files with recorded SHA-256s land in the DDCP0 fixture evidence
(`radix/.../ddcp0-fixtures.md` + `evidence/fixtures/`); this section is the
product-side schema fixture with the worked canonical-encoding example.
