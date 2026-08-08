# NGAB0 Fixture Contract — Generic host-plus-device fixture

**Unit**: NGAB0-U11 — generic fixture contract; see `ngab0-delivery.md`
NGAB0-U11.
**Status**: frozen (spec only) — the fixture is **not runnable**. Execution is
NGAB1's vertical slice; no product code is written in NGAB0.
**Dependencies**: §Partition + §Abi (NGAB0-U3) and §BackendVariants +
§Admission (NGAB0-U5) of `ngab0-composite-contract.md`; both frozen and
committed.
**Authority order** (from the packet): live source/tests → accepted artifact
schemas + hardware receipts → this packet's frozen contracts → campaign prose.
This contract sits inside the packet layer; where a shape below suggests
syntax, the syntax is illustrative only and the shape is the frozen fact.

## Purpose

One generic host-plus-device fixture proving the packet's partition shape —
**one scalar host function calling one device kernel** through the versioned
typed boundary — against a CPU oracle, with the expected evidence rows named
so NGAB1 can prove the vertical slice without inventing its own acceptance
criteria. This is the campaign's *generic host-plus-device fixture* (desired
end state item 6). It is deliberately **not** the LLM-shaped Gradus consumer:
no model, tokenizer, KV, or serving facts cross this boundary.

## Fixture shape (frozen)

The fixture is one checked package carrying ordinary host code and one
explicit device kernel — exactly the §PackageGraph source-package node — and
exactly one cross-boundary call shape per §Partition/§Abi:

```text
Host side — one native executable
  one host function  run_scale(x: f32) -> f32
  composite session surface: dispatch / observe / teardown over ProgramSession
        |
        |  versioned typed boundary — one call, one device kernel (§Abi)
        v
Device side
  one device kernel  scale_kernel(x: f32) -> f32  (typed, target-neutral DeviceProgram)
  backend serializations: MSL/metallib and NVVM/PTX (admitted variants, U5)
```

Shape facts (frozen):

- **One call**: the host function performs exactly one boundary call per
  invocation; call and entry are the same typed operation seen from either
  side (§Abi "one call"). No host function reaches a kernel except through
  this declared cross-boundary call; anything else fails closed in NGAB1.
- **Scalar-first**: the first accepted call shape is scalar-in/scalar-out
  (`f32 → f32`). This is the first row of the extensible variant table, not a
  hard-code: NGAB1-U4 batches compatible shapes (vector/2D args, two-kernel
  composition) through the same ABI without new mechanisms (NGAB1-U4; R3).
- **Typed, never text-parsed**: identity/type/lifetime facts cross the
  boundary in serialized form; they are **never reconstructed from emitted
  LLVM/MSL/PTX text or naming conventions** (§Abi; campaign Dependency Rule 2).
- **Target-neutral device program**: the kernel is one typed, target-neutral
  `DeviceProgram`; MSL/PTX are serialization choices of that one program,
  never separate programs (§BackendVariants; §PackageGraph invariant).
- **No runnable code in NGAB0**: this document freezes the fixture's shape,
  oracle, and evidence expectations. There is no runnable fixture, no CLI, no
  package build, and no host session in this phase — those are NGAB1's
  vertical slice (see §NGAB1 handoff).

### Package source sketch (shape, not runnable)

```text
fixture-scalar-kernel/                  one package (sketch shape)
  src/main.faber                        host + device kernel in one checked package
```

Illustrative sketch — shape only, **not runnable**; exact syntax is NGAB1's
lowering decision, not frozen here:

```faber
// host side: one scalar host function, one boundary call
fn run_scale(x: f32) -> f32 {
    device_call scale_kernel(x)      // one call; the typed boundary carries x
}

// device side: one typed, target-neutral device kernel
device kernel scale_kernel(x: f32) -> f32 {
    x * 2.0 + 1.0
}
```

The sketch fixes the **shape** — one host function, one declared device
kernel, scalar in/out through the typed boundary — not the syntax. NGAB1-U1
derives validated host MIR/LLVM *and* a typed `DeviceProgram` from the package
and executes the call through the existing llvm-host path; it may render this
shape with whatever surface the analyzed package requires. Whatever it emits
must still satisfy the evidence rows of §Expected evidence rows.

## CPU oracle (frozen)

The oracle is the reference computation the device result is measured against.
It is a CPU implementation of the kernel's arithmetic, owned by the fixture's
test harness (NGAB1's narrow test), not by the executable.

Oracle definition (frozen):

- **Reference computation**: `oracle(x) = x * 2.0 + 1.0` evaluated on CPU in
  `f32` arithmetic — the same declared inputs the host function passes to the
  boundary, no backend-specific transformation.
- **Deterministic**: one declared input value, one oracle result; no RNG, no
  wall-clock or memory-layout dependence. The fixture fixes a small declared
  input set (the scalar `x` plus the boundary's declared output slot).
- **Tolerance**: the observed device result must match the oracle result.
  Exact bit-match is expected for this scalar fixture; a backend that
  reorders arithmetic may declare a relative/absolute epsilon, but the
  epsilon is **recorded and declared**, never silent, and never widens past
  the declared fixture tolerance.
- **Mismatch is evidence failure**: a device result outside the declared
  tolerance fails the run — it is not retried, degraded, or silently accepted.
  There is no CPU fallback path (campaign Development Posture; §Admission
  "no CPU fallback, enumerated").
- **Oracle is evidence input**: the oracle comparison feeds the observations
  evidence row (§Expected evidence rows); it never alters resource identity or
  admission (observations are evidence, not identity inputs — §ResourceIdentity).

## Expected evidence rows

When NGAB1 executes the fixture (vertical slice), it must record each row
below, citing the frozen contract surface each row must match. A row that
cannot be produced from typed, serialized facts — instead reconstructed from
emitted text or path conventions — is a contract violation, not an acceptable
workaround (campaign Dependency Rule 2).

| # | Evidence row | What NGAB1 records | Frozen contract |
| --- | --- | --- | --- |
| 1 | **Partition** | The static split in the executed package: one host function (host side = the one native executable) calling exactly one device kernel through the declared boundary; no second authority on either side; call and entry are the same typed operation | §Partition; §OwnershipMatrix |
| 2 | **ABI version** | The versioned typed boundary the call rides: the accepted wire version carried by the device-program wire (`FmirDeviceSection` / `DEVICE_RUN_PLAN_VERSION` hot-path facts), with no `WIRE_DEVICE_PROGRAM_VERSION` bump; every crossing value carries its type fact, never re-derived from emitted text | §Abi; hot-path serialization #2 |
| 3 | **Manifest identity** | The manifest row for the embedded artifact: `artifact_kind` (admitted variant), `artifact_id` = SHA-256 digest over the embedded artifact bytes, `device_program_version`, `bounds`, `target`; identity verified **before** backend selection | §Manifest (row shape, schema `manifest-1.0.0`); §Verification |
| 4 | **Admitted backend** | The admitted variant row selected and its admission path: Metal `msl-source` (admitted first) or CUDA `ptx` (admitted only when a build-time clang NVPTX compiler was present); admission failed closed, no CPU fallback; a missing declared artifact fails closed as a missing declared artifact | §BackendVariants; §Admission |
| 5 | **Observations** | Dispatch report, step-run report, and teardown report bound to the session and the generation they were taken at; session identity recorded; the oracle comparison result recorded in the run report; observations are evidence, never identity inputs | §ResourceIdentity (observations) |

Evidence-row facts (frozen):

- All five rows are produced by the single vertical-slice execution; a row
  may be produced by inspection of typed artifacts, but never by parsing
  emitted LLVM/MSL/PTX text (§Partition/§Abi "never reconstructed").
- Row 3 must exist for every embedded artifact before any backend selection;
  a manifest missing an expected row, or carrying an unexpected row, fails
  verification (§Verification).
- Row 4's admitted backend must be one of the admitted variant rows
  (§BackendVariants); admission after verification, before session
  (§Admission).
- Rows feed the joint cross-repo receipt schema (NGAB0-U10): exact commands,
  content digests, and dirty-state declarations are recorded so a later
  auditor re-verifies rather than trusting this contract's claim (§Verification
  "receipt alignment").

## NGAB1 handoff

This contract is executed by **NGAB1** — explicitly, by NGAB1-U1 of
`ngab1-delivery.md`, whose `done_when` states: *"the NGAB0-U11 fixture (one
scalar host function calling one device kernel) is the proof: one analyzed
package derives validated host MIR → LLVM AND a typed `DeviceProgram`; device
facts (identity, resources, launches, lifetimes, observations) are typed, not
text-parsed; the call links and executes through the existing llvm-host path
(targeted, narrow test)."*

Handoff facts (frozen):

- NGAB1 owns execution and enforcement; NGAB0 owns the frozen contract. The
  fixture becomes runnable only when NGAB1-U1 lands.
- NGAB1-U2 adds the negative proof on the same fixture shape: invalid
  cross-boundary values (wrong type, shape, lifetime, mutation of read-only
  resource, observation of unlaunched work) fail at compile time with typed
  diagnostics — the compile-time failure is the packet's contract (§Partition,
  §Abi cross-boundary validity); NGAB1 must prove rejection without a launch.
- The scalar shape is the extensible first row, not a hard-code: NGAB1-U4
  batches compatible call shapes through the same ABI without new mechanisms
  (R3).
- This contract is **revisable through PML1/NGAB1** — the packet's version
  authority and change procedure (NGAB0-U7, §Versioning) govern any revision;
  no implementation may drift from it silently.
