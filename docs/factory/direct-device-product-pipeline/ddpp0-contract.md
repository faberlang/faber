# DDPP0 Contract — Product shape, identity domains, canonical encoding, FNV removal

**Units**: DDPP0-U2 (contract core — the contract-spine root for `ddpp0-contract.md`),
DDPP0-U3 (performance invariants + prepared-region policy + selection + evidence tiers),
DDPP0-U4 (partition ownership + generated-Rust support destination + deletion rule + child routing).
**Date**: 2026-08-08 (U3 appended 2026-08-09; U4 appended 2026-08-09).
**Repo**: faber (control plane). `faber-runtime/` read-only; no product code; no cargo.
**Paired contract**: Radix DDCP0-U3 §ArtifactPacket/§IdentityDomains/§HotPathGate
(`radix/docs/factory/direct-device-compilation-pipeline/ddcp0-delivery.md`).
**Sections frozen here**: `## ProductShape`, `## IdentityDomains`,
`## CanonicalEncoding`, `## FnvRemoval`, `## RoundTripFixture` (U2);
`## PerformanceInvariants`, `## PreparedRegion`, `## SelectionPolicy`,
`## EvidenceTiers` (U3); `## PartitionOwnership`, `## GeneratedRustSupport`,
`## DeletionRule`, `## ChildRouting` (U4). All contract spine sections are
frozen as of DDPP0-U4.

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

---

## PerformanceInvariants

**Frozen** (DDPP0-U3): prepared-submission and explicit-observation performance
invariants. This is the product-side reading of the DDCP0 §SemanticProgram
prepared-submission-region rules and the DDCP0-U3 §HotPathGate (C2) mechanical
gate; the compiler-side authority text lives in
`radix/docs/factory/direct-device-compilation-pipeline/ddcp0-contract.md`
(§SemanticProgram landed; §ArtifactPacket/§IdentityDomains/§HotPathGate in
flight at DDCP0-U3). The two contracts agree field-by-field at the DDPP0 phase
gate; any field not yet landed in DDCP0 is recorded `PENDING-AGREEMENT` by
DDPP0-U6, never silently reconciled.

### Preparation runs once

1. **Prepared submission = once-per-session preparation.** The selected backend
   leaf **loads modules, resolves functions/pipelines, allocates persistent
   state, and prepares native argument/submission layouts once** — before the
   hot loop. Nothing is re-resolved, re-looked-up, or re-allocated per call
   (DDCP0-U3 §HotPathGate preparation counters; DDPP2 gate: module/function/
   pipeline preparation measured once per session).
2. **One host call → one prepared submission region.** A host call enqueues one
   prepared submission region containing **one or more kernels**. The region is
   the NGAB0 major-revision call granularity (H1; DDPP0-U6); the one-kernel
   fixture remains the minimal case, not the ABI limit
   (§RoundTripFixture).
3. **Synchronize/readback only at declared boundaries.** The hot path enqueues
   a prepared region and synchronizes/readbacks only at an **explicit
   observation, cancellation, dependency, or product boundary** — never per
   kernel, never per launch, never on a timer or a generic runtime wrapper
   (DDCP0 §SemanticProgram prepared-submission-region rules 1–3; DDPP2 gate: a
   multi-kernel region submits on one Metal command buffer and one CUDA stream
   respectively). Synchronization counts derive from declared observations,
   never from kernel count (DDCP0-U3 §HotPathGate).
4. **No name/map lookup on the hot path.** The emitted hot call carries no
   route string, no `Valor`, no descriptor map, no backend tag, and no kernel
   name (§ProductShape host/device call facts; DDCP0-U3 §HotPathGate zero
   post-prepare name/map/kernel lookup).
5. **Regions cannot cross effect boundaries.** A prepared region cannot cross an
   `ad`/Sermo boundary, an observation boundary, a cancellation boundary, or any
   other CPU-effect/host-control point; a candidate group containing one is
   split there or the program **fails closed** (DDCP0 §SemanticProgram rule 3;
   §ProductShape effect/capability requirements).
6. **Observation is an explicit declared fact.** A write does not imply
   observability; readback is a deliberate declared observation (DDCP0
   §SemanticProgram observation/readback axis; §ProductShape submission-region
   facts).

### The hot path, canonically

```text
prepare    (once, before hot loop): load modules → resolve functions/pipelines
            → allocate persistent state → prepare native argument/submission
            layouts (names, maps, dimensions, layout all fixed here)
hot loop   (per host call): enqueue one prepared region (bounded numeric
            invocation fields only) → no lookup, no map construction, no sync
sync/readback: only at explicit observation, cancellation, dependency, or
            product boundary
```

The invariant is: the hot loop **enqueues and finishes**; everything else is
preparation or a declared observation. Where the invariant would be violated
(undeclared per-kernel synchronization, post-prepare name lookup, generic
handle-map work, runtime kernel interpretation), the product **fails closed**
rather than degrading (campaign Stop Conditions; DDCP0-U3 §HotPathGate).

### Layer targets

- **DDPP1** proves the package-build path (materialization emits the fixed
  region identities and prepared layouts; build planning imports no Hosts
  driver implementation and reparses no emitted text).
- **DDPP2** proves the invariants in physical leaves: one prepared region → one
  Metal command buffer / one CUDA stream; sync only at declared observations;
  preparation measured once per session.

---

## PreparedRegion

**Frozen** (DDPP0-U3): prepared-region regime/shape-class identity, bounded
dynamic invocation fields, cache keys, bounds checks, and cache-miss behavior
(campaign freeze list item 6; DDCP0 §SemanticProgram prepared-submission-region
rules 4–5). The concrete regime/shape-class **enumeration is deferred** — the
principle is frozen here and in DDCP0 (DDCP0-U3 C5 deferral recorded: regime/
shape-class enumeration lands at a later stage; tagged-sibling decision → DDCP6).

### Region identity

1. **Compiler-owned, static.** A prepared region has a **compiler-owned
   regime/shape-class identity**: static at compile time, derived from the
   dependency graph by the compiler, never composed at run time from strings,
   maps, or emitted names. The compiler owns region formation; a region is
   never introduced by a generic runtime wrapper or a runtime scheduling policy
   (§ProductShape submission-region facts; DDCP0 §SemanticProgram rule 4).
2. **One or more kernels per region.** A prepared submission region contains
   one or more kernels; the one-kernel fixture proves the minimal one-region
   case, not the ABI limit (NGAB0 major revision, DDPP0-U6).
3. **Shape class.** The shape class is the compile-time regime family a region's
   layouts are prepared for. Bounds and argument layouts are compiled into the
   region; the invocation carries only the dynamic extent, validated against
   the compiled bounds. A call outside the prepared shape class is a cache miss
   (below), never a silent re-layout on the hot path.

### Bounded dynamic invocation fields

1. **Only bounded numeric fields.** A prepared-region invocation carries only
   **bounded numeric dynamic fields** — active prompt length, dispatch extent,
   and comparable bounded extents. These are the only **bounded dynamic** values
   on the hot path; everything else (symbols, resources, dimensions, argument
   layouts, effect/capability requirements) is fixed at prepare time
   (§PerformanceInvariants rule 4).
2. **Validated against compiled bounds.** Each dynamic field is validated
   against the region's compiled bounds before enqueue — per-output bounds and
   index domains, never a shared extent that could overrun or under-fill
   heterogeneous outputs (DDCP0 §SemanticProgram §Per-output bounds and index
   domains). An out-of-bounds extent **fails closed** with a stable structured
   diagnostic.
3. **No name lookup, no map construction.** Validation is arithmetic against
   compiled bounds, not a lookup: no kernel name, no route string, no descriptor
   map, no handle-map construction on the hot path (DDCP0-U3 §HotPathGate).

### Cache keys and bounds checks

1. **Cache key = (artifact identity, region identity, shape class).** Cache keys
   are the triple of artifact identity (`content_sha256`/`packet_sha256` per
   §IdentityDomains), compiler-owned region identity, and shape class — semantic
   and artifact facts, never kernel-name or descriptor lookups (DDCP0
   §SemanticProgram rule 5).
2. **Cache admission is never FNV64-only** (§IdentityDomains rule 5); it uses a
   collision-resistant digest or canonical-descriptor equality.
3. **Bounds checks before allocation.** Compiled bounds are checked before any
   allocation or enqueue; allocation uses the recorded structural ceilings
   (artifact packet `bounds`; DDCP0-U3 §ArtifactPacket), never re-derived sizes
   from emitted text or binary.

### Cache miss

1. **Prepare outside the hot loop or fail closed.** A cache miss **prepares
   outside the hot loop or fails closed**. The miss is a prepared-region
   preparation event off the hot path (the same once-per-session prepare path),
   reported with a stable structured diagnostic; if the off-path prepare is not
   available, the call **fails closed**.
2. **It never interprets a kernel.** Cache miss never interprets a kernel — no
   runtime kernel interpretation, no re-derivation of compiler facts from
   emitted text or binary (§ProductShape assembly rule; campaign Stop
   Conditions).

---

## SelectionPolicy

**Frozen** (DDPP0-U3): release selection policy (council disposition C1). The
v1 release is **single-backend by default**; optional fat products select once
at startup; the host×device matrix is **capability truth, not a shipping
promise** — every product cell needs its own residency + performance receipt.
The defaults below are resolved here in DDPP0, not left to Hands; operator
gates are named per decision.

### Release defaults (v1)

1. **Single-backend default release.** A v1 release builds with **one backend
   leaf at build time**; the shipped product is single-backend by default.
   Fat binaries are **deferred** (campaign OQ2 — no fat-product promise in v1).
2. **Optional fat products select once at startup.** If a fat product ships
   (deferred, not planned for v1), it selects the backend leaf **once at
   startup** — one selection, fixed for the process lifetime, never per call,
   never on the hot path (campaign Desired End State #4; §ProductShape
   `DeviceArtifact[]`).
3. **Capability truth, not a shipping promise.** The host×device matrix is
   **capability truth** — the host/device pairs the compiler and product can
   express — not a promise that every pair ships. Every product cell
   (host×device pair) needs its own **residency + performance receipt** before
   it is claimed as shipped; an unclaimed pair is capability truth (T1) but not
   a shipping claim (§EvidenceTiers).
4. **Pair validation before toolchain work.** Faber validates the requested
   host/device pair and enabled features before any toolchain work; an
   unsupported pair rejects during planning and acquires no unrelated target
   dependencies (§ProductShape assembly rule). `faber targets` reports matching
   capability truth (DDPP1 gate proof, DDPP0-U7).

### Backend artifact defaults

| Decision | Default | Status / gate |
| --- | --- | --- |
| PTX vs cubin (campaign OQ3) | **PTX** — driver JIT per TR7 Stage-0 contract (`cuModuleLoadData`; no NVCC/NVVM at runtime) | cubin deferred; startup-preparation tradeoff resolved here |
| MSL vs metallib (campaign OQ4) | **MSL source first** — NGAB0 precedent (`msl-source` admitted first) | metallib reserved (NGAB0 reserved row); operator gate on Metal embedding |
| AMD identity (campaign OQ5) | **`amd` + HSA-native** — no HIP, no CUDA-translation identity | first-leaf API default **HSA/ROCr**; operator gate at DDPP5 |
| Fat binaries | **deferred** | not a v1 shipping promise (OQ2) |

1. **PTX default.** The CUDA artifact default is **PTX**: the driver
   JIT-compiles PTX at module load (`cuModuleLoadData`), consistent with the
   TR7 Stage-0 clean-install contract (only the NVIDIA driver required at
   runtime; no toolkit, no clang, no NVCC). cubin is **deferred** until
   delivery evidence requires it.
2. **MSL source first.** The Metal artifact default is **MSL source**, following
   the NGAB0 admitted-first `msl-source` row (§`artifact_kind`); metallib stays
   **reserved** until an operator decision on Metal embedding (NGAB0 reserved
   row / operator gate).
3. **AMD stays `amd` + HSA-native.** AMD identity stays **`amd` + HSA-native**;
   there is no HIP identity and no CUDA-translation identity. The first-leaf
   API default is **HSA/ROCr** (the pinned external materializer path,
   §ProductShape `DeviceArtifact[]`), with an **operator gate at DDPP5** before
   any first AMD product leaf.
4. **No translation identities.** Selection never introduces cross-backend
   translation identities (no HIP-from-CUDA, no PTX→AMDGPU translation); each
   leaf is materialized by its own backend from the shared target-neutral
   `DeviceProgram` (§ProductShape; campaign Stop Condition: a generic
   abstraction that erases backend-specific memory/queue/target requirements).

---

## EvidenceTiers

**Frozen** (DDPP0-U3): evidence-tier labels (council disposition C5). Every
product claim carries a tier label + a named receipt. The four labels:

| Tier | Label | Meaning | Receipt example |
| --- | --- | --- | --- |
| T1 | **compiler emission** | the compiler emits/claims the fact | DDCP/DDPP contract fixtures; `faber targets` feature/capability truth |
| T2 | **materialization** | the artifact was materialized with verifiable identity | `content_sha256`/`packet_sha256` receipts (§IdentityDomains); build-plan receipt; round-trip fixture (§RoundTripFixture) |
| T3 | **physical execution** | it ran on a named physical device | NGAB/DDPP device receipts; TR7 receipts with the device + driver/toolchain envelope |
| T4 | **performance** | measured performance on a named cell | residency + performance receipt per product cell (C1, §SelectionPolicy) |

### Rules

1. **Every claim carries a tier + a named receipt.** Every product claim is
   labeled with one of the four evidence tiers and tied to a **named receipt**
   (contract section, fixture, build receipt, device receipt, performance
   receipt). An unlabeled claim is not a product claim.
2. **Tier monotonicity.** A claim is never promoted to a higher tier without its
   own receipt at that tier: compiler emission is not materialization,
   materialization is not physical execution, physical execution is not
   performance.
3. **Capability truth is T1.** The host×device matrix and `faber targets` output
   are capability truth (T1, compiler emission) until each product cell earns a
   residency + performance receipt (T4, §SelectionPolicy). Over-claims on
   "direct GPU execution" (AMD/WebGPU/prepared path) are caught at the tier
   label (council review recorded risk; TR7: consume each fact at its recorded
   evidence tier).
4. **Physical and performance tiers are named-device receipts.** T3/T4 claims
   are named-device receipts with their environment envelope (device, driver,
   toolchain, OS) — never generic claims.
5. **Identity-bearing receipts are hash-bound.** Artifact-level claims cite
   `content_sha256`/`packet_sha256` receipts (§IdentityDomains); performance
   claims cite their measurement receipt.
6. **Cross-reference.** DDPP0-U8 records the C5 migration-note requirement
   (before DDPP8 release work); this section is the label authority every DDPP
   artifact cites.

---

## PartitionOwnership

**Frozen** (DDPP0-U4): CPU/device partition ownership — the owner-per-surface
rows below (campaign §Ownership Map, §Scope Routing, §`faber-runtime`
Decomposition Target; council dispositions C2/C4). Every surface has **exactly
one owner**; no surface has dual authority, and no owner is re-introduced
through a forwarding crate or route alias (§DeletionRule).

| Surface | Owner | Product consequence |
| --- | --- | --- |
| Faber product/assembly/UX | **Faber** | target pairing, external toolchains, materialization, linking, support selection, product manifests, inspection — one product build plan (§ProductShape assembly rule) |
| Compiler facts (`DeviceProgram`, value generations, dependencies, lifetimes, observations, target requirements, hashes) | **Radix** (MIR + backend leaves) | consumed as typed facts; never reconstructed from emitted text/binary (§ProductShape; §FnvRemoval) |
| Physical device leaves + effects/sessions | **Hosts** | backend probe, module load, physical allocation, queue/stream, launch, sync; `ad` effect providers; browser adapter/WebGPU product host |
| ML semantics (model/tokenizer/forward/decode/logical KV/sampling/prefill) | **Gradus** | inference/training semantics only — **no device handle or driver** |
| Graphics source facts (shader contracts) | **Triga** (+ Radix shader facts) | source-to-MIR contracts — no WebGPU handles in public types |
| Generated-Rust support | **Faber**-owned generated-Rust support crate | **Rust-target support only**, no device session behavior (§GeneratedRustSupport) |

### Rules (normative)

1. **CPU partition.** The CPU/host side is Faber product/assembly/UX plus Hosts
   provider crates for concrete effects. Ordinary `ad` remains the language's
   IO-bound host-effect seam:
   `Faber ad` → Radix Sermo MIR → generated-language/LLVM effect ABI → Hosts
   dispatch/kernel → concrete provider (campaign §Decomposition Target). CPU
   host execution is not owned by the generated-Rust support crate.
2. **Device partition.** Physical device ownership lives in Hosts backend
   leaves (or a later execution coordinator): device handles, discovery,
   identity, topology, partition, health, transport, transactions, bound plans
   — no generated-Rust/runtime ownership (campaign Decomposition Target row).
   Backend-specific lowering/repacking is Radix leaf-owned; physical
   upload/repacking is Hosts-owned; no model semantics in Hosts.
3. **Gradus/Triga carry no device handles.** Gradus owns ML semantics and Triga
   owns graphics source contracts; neither owns driver handles, device handles,
   or product linking (§Scope Routing). Serving, distributed scheduling,
   deployments, and provider purchasing remain outside this campaign.
4. **Single-owner discipline.** The owner-per-surface table is the routing
   authority for DDPP0-U5's inventory and for later moves. A surface's owner
   may change only by a contract revision that records the old owner, the new
   owner, and the migration consequence — never by silent dual ownership.
5. **No universal runtime owner.** The ownership partition leaves **no
   universal runtime owner**: the generated-Rust support is explicitly
   Rust-target support, not a universal runtime (campaign Desired End State
   #10; §DeletionRule wording requirement).

---

## GeneratedRustSupport

**Frozen** (DDPP0-U4): the generated-Rust support destination (campaign Open
Question 1 / OQ1; §`faber-runtime` Decomposition Target rows 1–2; council
disposition C2).

1. **Destination default (OQ1).** The destination is a **Faber-owned support
   crate with a target-specific name**. The default is recorded here; the final
   crate name/home is an **operator gate at DDPP1** (campaign Open Questions
   folded in DDPP0, OQ1). DDPP0-U5 records it as the per-module destination for
   the generated-Rust families.
2. **Rust-target support only.** The crate carries the generated-Rust surface:
   `ascii`, `textus`, `valor`, `json`, `instans`, `intervallum`,
   display/failable/recovery helpers, CPU `tensor`/`sparsa` semantics needed by
   generated Rust, Sermo/frame language carriers, generated-Rust client calls,
   and the Rust `HostDispatch` trait. It has **no device session behavior** —
   no concrete effect or GPU launch behavior (§PartitionOwnership; campaign
   Decomposition Target rows 1–2).
3. **No transitive device or Hosts (C2).** The generated-Rust support crate
   **must not transitively pull device or Hosts** (council disposition C2;
   DDPP0-U7 restates this as a DDPP1 gate proof). It is linked only by
   HIR-Rust products; a small Rust-only build excludes device runtime and
   physical Hosts leaves.
4. **Contract split.** Faber owns the generated-Rust support **contract**
   (`HostDispatch` trait + frame carriers) as a narrow surface; Hosts depends on
   it and installs an implementation. No concrete effect implementation and no
   GPU launch behavior lives in the contract or the support crate
   (§PartitionOwnership; campaign Decomposition Target row 2).

---

## DeletionRule

**Frozen** (DDPP0-U4): the final `faber-runtime` deletion rule (campaign
Desired End State #10, §`faber-runtime` Decomposition Target closeout, Scope
Routing, Dependency Rules; council dispositions C3/C4/C5).

1. **Delete only after every listed consumer migrates.** `faber-runtime` is
   deleted only after **every listed consumer** migrates (module-by-module and
   import-by-import per DDPP0-U5 `ddpp0-runtime-inventory.md`, each with
   exactly one destination and a deletion receipt), an **external consumer
   audit** passes (TR7 included; PML2 GI1 admission migration named as a DDPP8
   prerequisite; PML0 capsule carriage reconciled), and accepted native/browser
   capstones land (campaign Dependency Rule 5).
2. **No forwarding crate / route alias / dual authority (C4).** Deletion must
   not install a forwarding crate, a route alias, or dual authority that
   preserves the old architecture. The temporary `faber-runtime` repository is
   in migration scope; it is **not a forward owner** and **cannot become a
   containment facade** (campaign §Scope Routing; development posture). No
   forwarding shims that preserve the old architecture.
3. **Renaming ≠ decomposition.** Renaming only the crate while leaving the same
   ownership mix does not satisfy the campaign (campaign Decomposition Target
   closeout). Decomposition is a per-module/per-import move to one destination
   + a deletion receipt, not a rename.
4. **Wording (C5).** Say **"no universal runtime owner"**, not "no runtime"
   (council disposition C5). The remaining generated-Rust support is explicitly
   Rust-target support, not a universal runtime (campaign Desired End State
   #10; §PartitionOwnership rule 5).
5. **DDPP8 support-ABI/product-version gate surface (C3).** DDPP8's deletion
   gate checks the full support-ABI / product-version surface. The listed gate
   items are recorded here (council disposition C3; cross-ref DDPP0-U8):

   | Gate item | Role at the deletion gate |
   | --- | --- |
   | `Cargo.toml` | faber + sibling manifests: remove `faber-runtime` dependency/feature edges |
   | `Cargo.lock` | lockfile entries referencing `faber-runtime` and its dependents |
   | `core-support-manifest.txt` | faber core-support roots: drop `faber-runtime`, reassemble support |
   | `build.rs` | core-support assembly (`core-support.tar.zst` + `.sha256` + files sha256) without `faber-runtime` |
   | generated Cargo manifests | materialized `support.faber_runtime()` + path-links regenerated to new destinations |
   | CI sibling checkouts | sibling-repo checkouts that ride faber's pinned faber-runtime revision |
   | release notes | release notes and version pins citing `faber-runtime` |
   | stale-archive fallback | no stale last-good archive reuse; archive identity fails closed (§IdentityDomains support-archive row; DDPP0-U8) |

   Before DDPP8 release work, a **migration note** is required (DDPP0-U8; C5)
   covering deletion, support surfaces, feature/toolchain changes,
   `core-support-manifest`, sibling checkouts, and the no-facade rule.
6. **Closeout posture.** The deletion gate is a clean break: producers and
   consumers migrate together, no compatibility fallback survives closeout
   (§FnvRemoval exemption path; campaign Development Posture).

---

## ChildRouting

**Frozen** (DDPP0-U4): exact child-campaign routing (campaign §Related
Campaigns routing + §Scope Routing; NGAB amendment path; H1).

1. **NGAB0 stays an accepted historical record.** NGAB0 remains the accepted
   NGAB contract as a **historical record with amended call granularity** —
   the one-call/one-kernel ABI is amended to **one host call → one prepared
   submission region containing one or more kernels** (§ProductShape
   submission-region facts; DDPP0-U6 applies the major revision). NGAB0's
   operator gates (llvm-host identity, MSL source-first, PTX arch set) are
   retained and extended, never silently changed (NGAB0 §Versioning).
2. **NGAB1–NGAB4 → DDPP3 child packets.** NGAB1–NGAB4 lower and implement only
   as **DDPP3 child packets**, not as a parallel authority (campaign routing;
   H1 hold). DDPP0-U6 un-holds NGAB1; NGAB1–NGAB4 do not start implementation
   until the amendment lands.
3. **NGAB5 → DDPP7 capstone.** NGAB5 remains the native ML capstone and feeds
   **DDPP7** (Gradus inference/training device integration proofs; gated on
   accepted PML stages).
4. **NGAB6–NGAB7 → DDPP8.** NGAB6–NGAB7 feed **DDPP8** (support decomposition,
   old-runtime deletion, qualification, release).
5. **MIR Swarm routes shared MIR writes.** Shared MIR writes route through the
   Radix **MIR Swarm** (single-writer authority). Hosts, Gradus, Triga, and
   temporary `faber-runtime` migration work stay in repo-local delivery/factory
   artifacts (campaign routing rule 4).
6. **Superseded historical clauses.** The following historical clauses are
   superseded by this contract:
   - **Target-build-pipelines GPU/LLVM packaging clauses → DDPP1.** The
     broader product-build predecessor's GPU/LLVM packaging clauses are
     absorbed into **DDPP1**; unrelated target-build work stays independent
     (campaign §Related Campaigns).
   - **Inference-session-boundary ownership superseded.** The historical
     session/artifact-handoff goal preserves only its valid metadata
     boundaries; runtime/model/serving ownership is superseded through DDPP and
     Gradus (campaign §Related Campaigns).
