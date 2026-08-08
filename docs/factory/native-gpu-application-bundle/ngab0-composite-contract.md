# NGAB0 Composite Contract — Package graph and ownership matrix

**Unit**: NGAB0-U2–U3 (package graph + ownership matrix; host/device partition
+ entry/call ABI) — see `ngab0-delivery.md` NGAB0-U2 and NGAB0-U3.
**Status**: frozen (U2–U3) — §PackageGraph + §OwnershipMatrix + §Partition +
§Abi. Further sections (§Manifest/§ResourceIdentity/§Verification,
§BackendVariants/§ArtifactLayout/§Admission, §Ux/§Errors,
§FrozenVsReserved/§Unsupported/§Versioning) are added by NGAB0-U4–U7; the
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
