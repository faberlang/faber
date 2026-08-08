# NGAB0 Composite Contract — Package graph and ownership matrix

**Unit**: NGAB0-U2–U5 (package graph + ownership matrix; host/device partition
+ entry/call ABI; manifest schema + resource identity + verification; backend
variants + artifact layout + admission) — see `ngab0-delivery.md` NGAB0-U2
through NGAB0-U5.
**Status**: frozen (U2–U5) — §PackageGraph + §OwnershipMatrix + §Partition +
§Abi + §Manifest + §ResourceIdentity + §Verification + §BackendVariants +
§ArtifactLayout + §Admission. Further sections (§Ux/§Errors,
§FrozenVsReserved/§Unsupported/§Versioning) are added by NGAB0-U6–U7; the
version authority and change procedure freeze at NGAB0-U7.
**Authority order**: live source/tests and live `faber targets` → accepted
artifact schemas + hardware receipts → this packet's frozen contracts →
campaign prose.
**Sibling packet**: Gradus PML0 exchange partner —
[`gradus/docs/factory/production-ml-library/pml0-gradus-contract.md`](../../../../gradus/docs/factory/production-ml-library/pml0-gradus-contract.md)
(assembly is PML0-U10; cited here as the exchange partner path per the packet
contract regardless of its in-flight state).

## PackageGraph

The accepted package graph for a composite native-GPU application. Frozen
shape: **one source package → host MIR/LLVM modules + device program/artifacts
→ composite build/link → one native executable**.

```text
Faber application source (one package)
  -> Radix analyzed package
       -> host MIR -> LLVM host modules
       -> device program -> MSL/metallib and NVVM/PTX
  -> Faber composite build and link manifest
  -> one native executable
  -> embedded-artifact verification and host session
  -> Metal or CUDA execution
```

| Node | Contents | Owner | Live baseline |
| --- | --- | --- | --- |
| Source package | One checked Faber package carrying ordinary host functions and explicit device computation | faber (product input) | `faber build/run` package surface |
| Analyzed package → host MIR → LLVM host modules | Validated MIR lowered to LLVM host modules; `faber-host-llvm` runtime archive link | radix (compiler facts) | `faber/src/package/llvm_host.rs` (one `.ll` per unit via `build_package_llvm`, `llvm-as` verify, pinned `opt -O2`, `clang` link) |
| Analyzed package → device program → MSL/metallib + NVVM/PTX | Typed, target-neutral device program carried as versioned wire `DeviceProgram` with MSL/PTX artifacts | radix (device program/emission) | `faber/src/package/device/section.rs` (`FmirDeviceSection`, `DEVICE_RUN_PLAN_VERSION`), `faber/src/package/mir/image.rs` |
| Composite build + link manifest | Build plan joining LLVM host link with embedded device artifacts; inspectable layout + link manifest + runtime identity | faber (assembly) | `target/faber-llvm/{debug|release}/` precedent, `link-manifest.toml`, `write_runtime_identity` |
| One native executable | Application binary with embedded content-addressed device artifacts + manifest | faber (assembly) | NGAB2 composite build (contract here; implementation later) |
| Verification + host session | Embedded-artifact identity verification before backend selection; persistent device session; dispatch/observe/teardown | hosts (effects/sessions) | `faber/src/package/device/run.rs` (`DeviceBackend`, `ProgramSession`); host provider contracts |

Graph invariants (frozen):

- Exactly one source package per composite application; one native executable
  per composite build. No undeclared external files on the runtime path —
  hosts must not depend on loose developer-tree kernel paths (campaign
  Dependency Rule 3).
- Identity, type, resource, and lifetime facts survive lowering; they are
  **never reconstructed from emitted LLVM/MSL/PTX text or naming conventions**
  (campaign Dependency Rule 2; frozen for NGAB1).
- The device program is target-neutral and typed; backend variants (MSL source
  vs metallib, PTX) are serialization choices of one program, not separate
  programs.
- Verification precedes backend selection; unsupported variants fail closed
  with no CPU fallback (campaign Development Posture).

## OwnershipMatrix

Owner-per-surface matrix, frozen. Each surface has exactly one owner; no
duplicate authority.

| Surface | Owner | Scope of ownership | Non-owner / exclusion |
| --- | --- | --- | --- |
| Product workflow, assembly, UX | faber | Build plans, external toolchain invocation, final layout, link manifest, embedded-artifact assembly, `build/run` UX, capability admission surfaced as product commands | Does not own kernel semantics, model semantics, or device physics |
| Compiler facts, emission, device program | radix | Host/device partitioning, validated MIR, kernel identity, resource semantics, backend emission, device-program serialization | Does not run target formatters/linters/package builds/hosts; does not own physical device effects |
| Effects, sessions | hosts | Driver discovery, module loading, physical buffers, dispatch, synchronization, observation, teardown, persistent `ProgramSession` lifecycle | Holds only versioned logical handles and call state, not compiler/package policy |
| ML semantics | gradus | Model, tokenizer, transformer, decode, cache, sampling semantics; paired `pml0-gradus-contract.md` is the exchange partner — **no device handle**, no serving policy in the Gradus packet | Gradus receives **no device handle** and no backend handle; stays device neutral |
| Inference product repo | later — not this phase | Serving/HTTP, request scheduling, batching, deployment, and public tuning API | A separate application repository, not yet drafted; NGAB supplies its executable path but does not implement the product |

Cross-campaign ownership rules cited (campaign Dependency Rules):

- **Dependency Rule 1**: NGAB0 and Gradus PML0 exchange one versioned
  interface packet before either campaign generalizes its public boundary —
  this packet pairs with `pml0-gradus-contract.md`.
- **Dependency Rule 6**: GI3 compiler evidence is reusable; GI4–GI7 ownership
  and product clauses are re-lowered under the new Gradus (PML5 semantics) and
  separate-inference-product decision before further implementation. Model
  runtime and serving never land in faber, radix, or hosts.
- **Dependency Rule 7**: multi-device work consumes this single-device
  executable contract but does not block NGAB0–NGAB5.
- Gradus packet non-goals mirror here: no device handle, no HTTP policy
  anywhere in this packet.

### Hot-path serialization list

Facts that must survive lowering, assembly, and launch in serialized form —
frozen so NGAB1–NGAB3 lower against stable wire surfaces.

| # | Serialized fact | Carried by | Owner | Crossing | Cross-campaign rule |
| --- | --- | --- | --- | --- | --- |
| 1 | DeviceProgram | Typed, target-neutral device program; wire `DeviceProgram` in the FMIR device section | radix (compiler source of truth) | radix → faber → hosts | Rule 2: never reconstructed from emitted text |
| 2 | FMIR / device wire versions | `FmirDeviceSection`, `DEVICE_RUN_PLAN_VERSION`, MSL/PTX artifacts | radix (emission); faber (wire code) | radix → faber | Rule 6: device-program facts stay radix-owned, re-lowered with GI4+ |
| 3 | Materializer | Device artifacts + manifest materialized into the composite image (source-built/text/binary construction and merged-program revalidation) | faber (assembly) | radix → faber → native executable | Rule 3: no loose developer-tree kernel paths |
| 4 | Host construction | Runtime identity, link manifest, host `ProgramSession` composition and backend selection | hosts (effects/sessions); faber (executable integration) | faber → native executable → hosts | Rule 7: multi-device consumes, does not block |
| 5 | Package admission | Composite package admitted to the build/run product surface; capability admission gates | faber (product workflow) | package → faber build/run UX | Rule 1: interface packet exchanged with PML0 before generalization |

The serialization homes named above are the live baselines from
`ngab0-delivery.md` §Repo-Aware Baseline; their contract shape freezes here
and NGAB1–NGAB3 implement against it. Ownership of every surface is single
and non-transferable by later stages without the packet change procedure
(§Versioning, NGAB0-U7).

## Partition

The host/device partition, frozen at NGAB0-U3. Shape: **one host function
calls one device kernel through a versioned typed boundary**. The partition is
the static split that survives lowering, assembly, and launch — the same split
as the §PackageGraph nodes and §OwnershipMatrix rows, owned from their
separate surfaces.

```text
Host side — one native executable
  ordinary Faber host functions (MIR -> LLVM host modules)
  composite session surface: dispatch / observe / teardown over ProgramSession
        |
        |  versioned typed boundary  (one call, one device kernel)
        v
Device side
  one device kernel per call; typed, target-neutral DeviceProgram
  backend serializations: MSL/metallib and NVVM/PTX (admitted variants, U5)
```

Partition facts (frozen):

- The host side is the one native executable of §PackageGraph; it owns the
  composite session surface and capability admission. Per §OwnershipMatrix,
  effects/sessions belong to hosts, assembly/UX to faber, emission facts to
  radix — the partition binds the same ownership, never a second authority.
- The device side is one typed device kernel per call, emitted from the
  target-neutral device program; backend variants are serialization choices of
  one program, never separate programs (§PackageGraph invariant).
- The boundary admits exactly one call shape: **one host function invokes one
  device kernel**. No host function reaches a kernel except through a declared
  cross-boundary call; anything else fails closed in NGAB1.
- Identity, type, and lifetime facts survive lowering; they are **never
  reconstructed from emitted text** — neither LLVM/MSL/PTX text nor naming
  conventions (NGAB1 rule; frozen here, no seam in NGAB0).
- Invalid cross-boundary values fail at compile time — before composite
  build/link and before launch. Enforcement is NGAB1's; the compile-time
  failure is the contract this packet freezes.
- The partition is stable across the hot-path serialization list
  (§OwnershipMatrix): DeviceProgram, wire versions, materializer, host
  construction, and package admission cross the same boundary, never a
  parallel unversioned channel.

## Abi

The versioned typed entry/call boundary, frozen at NGAB0-U3. The call/entry
surface matches the GI4 session facts referenced by U8
(`radix/docs/factory/gpu-inference-gguf/gi4-contract.md` — U8 keeps those
session facts as compiler evidence); nothing on this boundary contradicts an
accepted GI4 fact.

Boundary rules (frozen):

- **Versioned**: the boundary is a versioned surface with its own ratchet
  (MD2-W1 sibling-field precedent); it rides the accepted wire version and
  requires no `WIRE_DEVICE_PROGRAM_VERSION` bump. Wire revisions of the
  session surface are GI4-2's, not this packet's.
- **Typed**: every value crossing the boundary carries its type fact. Types
  are carried in the serialized form (§OwnershipMatrix hot-path list); they
  are never re-derived from emitted LLVM/MSL/PTX text or naming conventions —
  identity/type/lifetime facts are **never reconstructed from emitted text**
  (NGAB1 rule, frozen here).
- **One call**: one host function → one device kernel. Call and entry are the
  same typed operation seen from either side.

Entry surface (host → device):

| Surface | Fact | GI4 contract source |
| --- | --- | --- |
| Call | One invocation per call; declared inputs and declared output | `gi4-contract.md` §2.4 (`Invocation`) |
| Invocation inputs | token id + absolute position (per-invocation only; resident inputs never ride an invocation) | §2.4 |
| Invocation output | full-vocab logits `[49152]` (tied-head projection) or the selected token | §2.4 |
| Workload mode | exactly one `InvocationMode`: `Prefill` or `ScalarDecode`; regime labels reported separately | §4 |
| Reuse | `ReuseKey` = `(session, sequence, epoch)`; resident resources reusable iff all three match | §6 |

Session facts that stay resident (never per-call):

| Fact | Carriage | GI4 contract source |
| --- | --- | --- |
| `ModelInstance` | model id + SHA-256 + byte length; load-once at session creation | §2.1 |
| `ExecutionSession` | binds one `ModelInstance`, typed `KvCacheLayout`, current `SequenceState`, minting `ReuseKey`; resident weights + KV uploaded once, never re-copied | §2.2 |
| `SequenceState` | position, token history, KV generations; advances only through a committed `TokenCommit` | §2.3 |
| `KvCacheLayout` | `slots`, `context_length`, `layer_count`, `kv_head_count`, `head_dim`, `dtype`, `reserve_policy`; byte accounting consumed, not re-derived | §3 |
| Token mutation | `TokenCommit` advances token id, position, KV generations, visible output together; no retry without deterministic replay from the last committed generation | §5 |

Cross-boundary validity (frozen):

- Invalid cross-boundary values — wrong type, wrong shape, out-of-order
  position, KV-generation gap, unknown workload mode, mismatched `ReuseKey` —
  fail at compile time for static facts and at admission/commit time for
  session facts; neither class silently reaches launch. Enforcement is NGAB1's;
  the contract is NGAB0.
- This packet adds no host-session surface (`faber-runtime/src/device.rs`,
  hosts) — the GI4-4 bounded session writer owns that; the boundary here
  carries the versioned facts the session writer consumes.
- Boundary typing binds to the composite session (§Manifest/§ResourceIdentity
  freeze at U4); no value crosses the boundary without a carried identity.

## Manifest

The versioned, **content-addressed** embedded-artifact manifest, frozen at
NGAB0-U4. The manifest is the sole carrier of embedded device-artifact
identity from assembly (§PackageGraph materializer node) to the composite
session (§Verification) — matching §OwnershipMatrix hot-path serialization #3
(materializer): artifact identity is a serialized fact, never re-derived at
launch.

```text
composite native executable
  -> one embedded manifest (versioned schema)          faber assembly
       -> per-artifact row: kind, wire version, digest, bounds
            MSL source artifact    (Metal; source-first default, U5)
            metallib artifact      (Metal; reserved, U5)
            NVVM/PTX artifact      (CUDA; admitted arch set, U5)
  -> identity verification gate (before backend selection; §Verification)
  -> backend admission + device session
```

Manifest facts (frozen):

- **Versioned schema**: the manifest conforms to a versioned schema with its
  own ratchet (MD2-W1 sibling-field precedent, as §Abi). The schema version is
  part of the manifest's identity surface; a schema change is a packet change
  under the §Versioning change procedure (NGAB0-U7).
- **Content-addressed**: each artifact row is identified by a digest computed
  over the embedded artifact bytes alone. Artifact identity is the digest —
  nothing else; a digest is the key the row is admitted and later verified
  under (§Verification).
- **Canonical digest algorithm named**: default **SHA-256**, matching the
  MD-A9 collision-resistant precedent (`radix/docs/factory/gpu-inference-
  multi-device/CAMPAIGN.md`, MD-A9 row: non-cryptographic provenance hashes are
  not cache/identity authorization) and the PML0 capsule contract default
  (`gradus/docs/factory/production-ml-library/pml0-model-capsule-contract.md`
  §3.2, field 2). Operator confirms the algorithm choice at NGAB0-U12; until
  then SHA-256 is the frozen default and no other algorithm may be admitted.
- **Never reconstructed**: manifest and artifact identity is **never
  reconstructed from emitted text or path conventions** — neither from
  LLVM/MSL/PTX text, nor file names, nor directory layout, nor link-manifest
  provenance strings (campaign Dependency Rule 2; the same rule §Abi and
  §Partition freeze for the boundary). A path may be recorded for provenance;
  it is locator metadata only.
- **Bounds recorded**: per-artifact byte length and structural ceilings are
  recorded in the manifest at assembly time; downstream admission checks
  recorded bounds before any allocation sized by them (PML0 capsule bounds
  precedent — bounds precede allocation).
- **One manifest per executable**: exactly one embedded manifest per composite
  native executable; no artifact reaches a device session without a manifest
  row, and no manifest row exists without an embedded artifact.

Manifest row shape (frozen shape, schema version `manifest-1.0.0`; U5 fixes
the variant rows, U7 fixes version authority):

| Row field | Carries | Rule |
| --- | --- | --- |
| `artifact_kind` | `msl-source` \| `metallib` \| `ptx` (admitted variants per U5) | Kind is a serialization choice of one device program, never a separate program |
| `artifact_id` | Content digest over artifact bytes (default **SHA-256**) | Identity is derived from bytes only; mismatch → pre-launch failure (§Verification) |
| `device_program_version` | Wire version of the typed `DeviceProgram` (§OwnershipMatrix hot-path #1) | Rides the accepted wire version; no unversioned artifact |
| `bounds` | Byte length + structural ceilings (e.g. PTX cap size, MSL source size) | Recorded, not re-derived; checked before allocation |
| `target` | Backend target the artifact is emitted for (Metal / CUDA) | Verified before backend selection (§Verification) |

## ResourceIdentity

Resource identity for the composite session, frozen at NGAB0-U4. Resource
identity — buffers, lifetimes, generations, observations — is **bound to the
composite session**: the session is the identity root, and no resource fact
exists outside the session that minted it.

Resource identity facts (frozen):

- **Buffers**: every buffer is identified by a session-scoped logical handle —
  never by a GPU pointer value, driver resource ID, or path-derived label. A
  buffer handle is valid only within the session that minted it; it carries
  its type fact and shape (§Abi: every value crossing the boundary carries its
  type fact) and is never reconstructed from emitted text.
- **Lifetimes**: buffer and session-resource lifetimes are scoped to the
  composite session — allocation and release are session operations, and
  teardown releases every resource the session owns. No resource outlives its
  session; no session outlives its executable (§PackageGraph one-executable
  invariant).
- **Generations**: generation-bearing facts (KV generations, sequence
  position, token history) advance only through a committed mutation
  (`TokenCommit` precedent, §Abi session facts, GI4 §5). Generation identity
  is `(session, sequence, epoch)` per the `ReuseKey` fact (§Abi); a resource
  is reusable iff all three match, and a generation gap is an invalid
  cross-boundary value (§Abi cross-boundary validity).
- **Observations**: observations (dispatch reports, step-run reports, teardown
  reports) are bound to the session and to the generation they were taken at.
  An observation records its session identity and the generation/commit it
  observes; it can never be attributed to a different session or generation.
  Observations are evidence, not identity inputs — no observation can alter a
  resource's identity (identity is derived from bytes and the manifest only,
  §Manifest).
- **Session binding**: resource identity is composite-session-bound — the same
  logical buffer/generation/observation referenced outside its minting session
  is a different, invalid resource. No cross-session resource alias exists in
  NGAB0.

## Verification

The verification order and tamper/mismatch behavior, frozen at NGAB0-U4
(council C8). Identity is verified **before backend selection**: verification
precedes every downstream selection, load, or backend binding.

```text
embedded manifest + artifacts -> identity verification
        |  (digest match, wire-version match, model-to-kernel binding match)
        v
backend selection (admitted capability; U5)     <- never earlier
        v
device session / dispatch / observe / teardown
```

Verification facts (frozen):

- **Order fixed**: artifact identity is verified **before backend selection**.
  No backend is selected, no driver/module load happens, and no device session
  opens until every embedded artifact verifies against the manifest —
  identity verification, then model-to-kernel binding, then capability
  admission, then session, in that order.
- **Tamper/mismatch → pre-launch failure**: any digest mismatch, wire-version
  mismatch, manifest-schema mismatch, or model-to-kernel binding mismatch is a
  **pre-launch failure** — typed, reported, and closed. The executable does
  not reach a device session and does not fall back to CPU or another backend
  (campaign Development Posture; no CPU fallback, matching §PackageGraph and
  the PML0 capsule fail-closed behavior).
- **Model-to-kernel compatibility binding**: the manifest binds the model
  identity (model id + SHA-256 + byte length, §Abi session facts / GI4 §2.1)
  to the kernel identity (device-program version + artifact digests). A
  composite application may only run the kernel the manifest binds to its
  model; a model/kernel pair from different manifest rows is a compatibility
  failure at verification, before launch.
- **Never reconstructed**: verification consumes only manifest-carried facts
  and artifact bytes; it never reconstructs identity from emitted text or path
  conventions (§Manifest; campaign Dependency Rule 2). A manifest missing an
  expected row, or carrying an unexpected row, fails verification.
- **Read-only gate**: verification verifies and admits; it does not mutate
  artifacts, rewrite the manifest, or mint new identities. Embedded artifacts
  are immutable bytes, so post-verification tampering cannot be silent — any
  change re-fails verification on the next launch.
- **Receipt alignment**: verification records exact commands, content digests,
  and dirty-state declarations in the joint cross-repo receipt schema
  (NGAB0-U10, §Manifest/§Verification-aligned), so a later auditor
  re-verifies rather than trusting this packet's claim.

## BackendVariants

The backend variant matrix for the composite native executable, frozen at
NGAB0-U5. U5 fixes the variant rows of the §Manifest `artifact_kind` field
(`msl-source` | `metallib` | `ptx`). Variants are **serialization choices of
one typed device program** — never separate programs (§PackageGraph invariant)
— emitted by radix and carried through the hot-path serialization list
(§OwnershipMatrix hot-path #2: FMIR device wire + MSL/PTX artifacts).

| Variant | `artifact_kind` | Backend | Admitted state (frozen) | Live baseline |
| --- | --- | --- | --- | --- |
| MSL source | `msl-source` | Metal | **Admitted first** — matches the current FMIR-carried MSL | `faber/src/package/device/section.rs`: Metal MSL always emitted through the S1-3 emitters |
| metallib | `metallib` | Metal | **Reserved** — no admitted row until the operator decision on Metal embedding (gate below) | Not produced by the live pipeline; Metal toolchain path only |
| PTX | `ptx` | CUDA | **Admitted** only when a build-time clang NVPTX compiler is present; otherwise the composite carries no CUDA artifact | `device/section.rs`: `compile_nvvm_to_ptx`; the packaged CUDA artifact is PTX (N1.3 §3.1), provenance hash covers the PTX blob, not the NVVM source |

Variant facts (frozen):

- **One program, many serializations**: every variant row is a serialization
  of the same typed, target-neutral `DeviceProgram`. Emitting or omitting a
  row never changes the program; admission (below) decides which rows may
  reach a session.
- **Launch identity rides the artifact**: Metal launches by the logical entry
  (the emitted MSL kernel name); CUDA launches by the emitted PTX `.entry`
  symbol carried as per-artifact metadata (N3.3). The CUDA logical-entry →
  symbol mapping is an artifact fact, never a program semantic, and never
  reconstructed from emitted text (§Partition/§Abi).
- **Best-effort emission, fail-closed execution**: MSL is always emitted;
  CUDA PTX emission is build-time best-effort (S3-A7 emitter surface or
  missing clang NVPTX). A composite that carries no CUDA artifact makes
  `--backend cuda` fail closed at run time as a missing declared artifact —
  it never silently falls back to Metal or CPU (§Admission).
- **Target binding**: each variant row binds the `target` fact of the §Manifest
  row (Metal / CUDA); verification precedes backend selection (§Verification),
  so the row is verified before any variant is admitted.
- **Defaults are frozen until an operator decision fires**: the Metal
  source-first default and the admitted PTX arch set are operator decision
  gates (see §Admission); no implementation admits a variant outside its
  default before its gate closes.

## ArtifactLayout

The composite artifact layout, frozen at NGAB0-U5. Shape: **one native
executable + embedded artifacts + inspectable build directory**, following the
live `target/faber-llvm/{debug|release}/` precedent (`faber/src/package/
llvm_host.rs`: one `.ll` per unit, `llvm-as` verify, pinned `opt -O2` in
release, `clang` link, inspectable link manifest + runtime identity).

```text
target/faber-llvm/{debug|release}/          build profile root (precedent)
  <product>/                                composite build root per product
    modules/          one .ll per host unit (llvm-as verified)
    optimized/        pinned opt -O2 outputs (release only)
    device/           embedded-artifact copies + embedded manifest
                      (msl-source; metallib reserved; ptx when emitted)
    link-manifest.toml  inspectable link manifest (host triple, profile,
                        runtime archive, composite variant rows)
    runtime/          runtime identity file
    <product>         ONE native executable (composite binary)
```

Layout facts (frozen):

- **One native executable**: exactly one composite binary per composite build
  (§PackageGraph invariant). The binary embeds the content-addressed device
  artifacts and the embedded manifest (§Manifest) plus the host LLVM image;
  it is the only artifact that launches.
- **Embedded artifacts + manifest**: the device artifacts and the manifest
  travel inside the executable. The inspectable build directory records the
  same identity facts (link manifest, runtime identity, device-artifact copies)
  for inspection and receipts — but identity is **never reconstructed from the
  build directory** on the runtime path (§Manifest/§Verification): the
  embedded manifest in the binary is the sole identity carrier.
- **Inspectable build dir**: `target/faber-llvm/{debug|release}/` precedent
  is extended by a `device/` directory carrying the emitted artifact copies
  and the manifest. Debug and release are separate profile roots; the profile
  (debug = `-g`, no `opt`; release = pinned `opt -O2`) is recorded in the
  link manifest.
- **No loose developer-tree kernel paths** (campaign Dependency Rule 3): hosts
  never depend on files in the inspectable build directory at run time. The
  build directory is an inspection/receipt surface, not a runtime dependency.
- **Receipt alignment**: the build directory contents (link manifest, runtime
  identity, artifact copies, digests) feed the joint cross-repo receipt schema
  (NGAB0-U10); they are evidence, not identity inputs.

## Admission

Capability admission for the composite executable, frozen at NGAB0-U5.
Unsupported hardware, driver, version, architecture, dtype, quantization, or
kernel capability fails **fail-closed** — a typed, reported, pre-launch
failure with **no CPU fallback** (campaign Development Posture; §PackageGraph
and §Verification already freeze the no-CPU-fallback rule at the variant and
verification layers; this section freezes the admission gate itself).

```text
identity verification + model-to-kernel binding (§Verification, in order)
  -> capability admission (this section)
       -> backend selection
       -> device session (dispatch / observe / teardown)
```

Admission facts (frozen):

- **Fail-closed by default**: every admission gate (hardware, driver, version,
  arch, dtype, quant, capability) admits only when positively known and
  matching the admitted row. Unknown, unsupported, or mismatched → pre-launch
  failure, typed and reported. There is no degradation path.
- **Hardware / arch**: the native host set follows the
  `E_LLVMHOST_UNSUPPORTED_HOST` precedent (`llvm_host.rs`): aarch64/x86_64
  macOS + aarch64/x86_64 Linux, native host builds only, no cross compile.
  Device admission is per-backend: Metal on macOS, CUDA on Linux; an
  unsupported device or arch fails closed before any module load.
- **Driver / version**: driver and module versions must match the admitted
  row. Driver discovery and module-loading failures fail closed before launch
  (`device/run.rs` fail-closed wire admission precedent, S3-A4).
- **dtype / quant / capability**: dtype, quantization, and kernel-capability
  requests outside the admitted row fail closed at admission, before any
  session opens.
- **Missing declared artifact**: a variant the manifest does not carry (for
  example no `ptx` row because the build-time clang NVPTX compiler was absent)
  makes the corresponding backend request fail closed as a missing declared
  artifact — never a silent switch to the other backend.
- **Admission after verification**: identity is verified (§Verification) and
  the model-to-kernel binding checked **before** capability admission;
  admission precedes backend selection and session. Admission verifies and
  admits; it does not mutate artifacts, open sessions, or mint identities.
- **No CPU fallback, enumerated**: Rust, CPU, a subprocess compiler,
  `llama.cpp`, or a separately installed kernel are all closed paths. The only
  admitted paths are the verified variant rows of §BackendVariants.

### Operator decision gates (frozen defaults)

The three operator open questions from `ngab0-delivery.md` §Open Questions are
recorded here with their frozen defaults and explicit operator decision gates.
Each default holds until the named gate closes; the gate closes by operator
decision at NGAB0-U12 phase closeout (U12 folds the answers or defers with
these recorded defaults). The packet cannot claim the full phase gate while
any of these dangle.

1. **Artifact identity** — keep the stable user-facing selector `llvm-host`
   with an embedded-device capability, or define a broader application
   artifact identity with `llvm-host` as the host lane? **Default**: retain
   `llvm-host`, extend capability. **Operator decision gate**: NGAB0-U12; a
   broader identity is a packet change under the §Versioning change procedure
   (NGAB0-U7).
2. **Metal embedding** — MSL source, metallib, or both for the first admitted
   macOS row? **Default**: source first (matches the current FMIR-carried
   MSL), metallib reserved. **Operator decision gate**: NGAB0-U12; the
   `metallib` variant row may not be admitted before this decision.
3. **CUDA PTX arch set** — the minimum portable PTX architecture set for
   NGAB6. **Default**: the admitted row's arch set recorded and
   operator-confirmed at NGAB0-U12; until then the `ptx_target` carried by the
   FMIR device section (`device/section.rs`) is the working target and the
   `ptx` row is admitted only for it. **Operator decision gate**: NGAB0-U12.

## Ux

Build/run UX for the composite path, frozen at NGAB0-U6. The composite
application is a **product surface of `faber build` and `faber run`**, not a
separate kernel-toolchain workflow: the user builds one package and runs one
native executable, and the embedded-artifact assembly, verification, backend
selection, and session lifecycle are product steps the executable performs
itself (§PackageGraph, §ArtifactLayout, §Verification, §Admission).

```text
faber build <package>            composite build: one native executable
                                   + embedded content-addressed artifacts
                                   + embedded manifest + inspectable build dir
faber run <package>              identity verification -> capability admission
                                   -> backend selection -> persistent session
                                   -> dispatch / observe / teardown
```

UX facts (frozen):

- **Build surface**: `faber build` produces the composite artifact of
  §ArtifactLayout — one native executable with embedded device artifacts and
  manifest, plus the inspectable `target/faber-llvm/{debug|release}/` build
  directory (link manifest, runtime identity, device-artifact copies). No
  user step compiles, locates, or launches a kernel artifact; the build plan
  joins the LLVM host link with the emitted device rows (§PackageGraph).
- **Run surface**: `faber run` on a composite package executes the product
  path of §Verification → §Admission → backend selection → session, mirroring
  the live `device/run.rs` `ProgramSession` lifecycle (`DeviceBackend`,
  dispatch reports, step-run reports, teardown). Ordinary Faber host code
  owns CLI, files, and control flow; device effects stay with hosts
  (§OwnershipMatrix).
- **`--backend` is capability admission, not default UX**: the default run
  path admits the backend the machine provides — one admitted variant row
  (§BackendVariants), fail-closed (§Admission) — and opens the required
  Metal or CUDA host internally. `--backend` names a specific variant and
  remains admission-gated: a backend the manifest does not carry fails closed
  as a missing declared artifact (§Admission); it never silently switches
  backends.
- **No default-UX device choice**: the normal user never chooses a device or
  backend; capability decides (design rule 2 below).
- **Build/run separation of duties**: faber owns the build plan, assembly,
  and product commands; hosts owns session effects; radix owns emission
  facts; the run path consumes only manifest-carried identity (§Manifest) and
  never reconstructs facts from the inspectable build directory
  (§ArtifactLayout).
- **Receipt alignment**: build and run record exact commands, content
  digests, and dirty-state declarations into the joint cross-repo receipt
  schema (NGAB0-U10), so a later auditor re-verifies rather than trusting
  this packet's claim (§Verification).

### Design rules (cpo/cxo, recorded verbatim)

Frozen as product design rules for NGAB5 and later stages. These are unit
outputs of NGAB0-U6 — contract, not code.

> **NGAB5 tuning surface is an adapter over the Gradus generation-config
> contract, never a second authority** — the composite product's bounded
> tuning parameters (model path, prompt, context length, prompt batch size,
> maximum generated tokens, seed, temperature, top-k, top-p, min-p,
> repetition penalty; campaign NGAB5 gate) map onto the Gradus
> generation-config contract (`gradus/docs/factory/production-ml-library/`).
> Faber's CLI/config surface adapts that one contract; it defines no second
> tuning schema, no second authority, and no independent parameter semantics.
> Gradus owns the semantics; the adapter follows.

> **backend/device selection is an operator/diagnostic override, not default
> UX** — the default product experience selects the admitted backend
> automatically and opens the required host internally. Explicit backend or
> device selection exists for operators and diagnostics (isolating a failure,
> forcing a variant for receipts, §Admission operator gates); it is never
> advertised as, nor promoted to, default user UX.

## Errors

Error taxonomy, frozen at NGAB0-U6. Every composite-path failure belongs to
exactly one failure class, fixed by **where it fires** — before launch, in
session, or at teardown — never by severity. Each class fails **typed,
reported, and closed**; no class degrades to a CPU or alternate-backend run
(campaign Development Posture; §Admission no-CPU-fallback rule).

| Class | Fires | Trigger examples | Live precedent |
| --- | --- | --- | --- |
| Identity | pre-launch | digest mismatch, wire-version mismatch, manifest-schema mismatch, model-to-kernel binding mismatch, missing/unexpected manifest row | §Verification (tamper/mismatch → pre-launch failure) |
| Admission | pre-launch | unsupported host arch/os, unsupported hardware, driver/version mismatch, dtype/quant/capability outside the admitted row, missing declared artifact | `E_LLVMHOST_UNSUPPORTED_HOST` (`llvm_host.rs`); §Admission fail-closed gates |
| Capability | pre-launch, before session | driver discovery failure, module loading failure, device absent, runtime capability probe fails | fail-closed wire admission (`device/run.rs`, S3-A4); `Diagnostic::error` |
| Session | mid-session | dispatch failure, transfer/resource failure, synchronization/observation failure, generation mismatch, invalid session-bound cross-boundary value, device fault/reset | `device/run.rs` `ProgramSession` dispatch/step-run reports; §Abi session-commit-time validity |
| Teardown | teardown | resource release failure, incomplete teardown, cancellation path | §ResourceIdentity lifetimes; §Abi session facts |

Error facts (frozen):

- **Class is fixed by timing, not severity**: identity, admission, and
  capability classes fire before launch and are pre-launch failures
  (§Verification, §Admission); session failures fire mid-session and bind to
  the session and generation they occurred at; teardown failures fire during
  resource release. A failure never re-classifies by consequence.
- **Typed and reported**: every failure is a typed diagnostic carrying its
  class and code plus the carried identity facts (§Manifest) and, for
  session/teardown classes, its session identity and generation
  (§ResourceIdentity observations — evidence, never identity inputs).
- **Fail-closed, no fallback**: identity/admission/capability failures close
  the run before any backend selection, module load, or session; session
  failures close the session and proceed to teardown; teardown failures are
  reported with best-effort full release. No class falls back to Rust, CPU,
  a subprocess compiler, `llama.cpp`, or a separately installed kernel
  (§Admission enumerated closed paths).
- **Boundary violations are classified by where they surface**: static
  cross-boundary violations are compile-time (NGAB1 enforcement, §Abi);
  session-bound violations (generation gap, out-of-order position, mismatched
  `ReuseKey`) surface at admission/commit time as session-class failures —
  neither class reaches launch silently (§Abi cross-boundary validity).
- **Teardown is unconditional**: every session must release every resource it
  owns on success, error, and cancellation (§ResourceIdentity lifetimes —
  no resource outlives its session). Teardown failure never leaks silently;
  the release is completed best-effort and the failure reported.
- **Observations are evidence**: error reports are observations bound to
  (session, generation) and cannot be attributed to another session or
  generation (§ResourceIdentity); they are evidence for receipts, not inputs
  to identity.
- **Receipt alignment**: failures record exact commands and digests in the
  joint cross-repo receipt schema (NGAB0-U10) so a later auditor reproduces
  and re-verifies the failure rather than trusting this packet's claim
  (§Verification receipt alignment).
