# NGAB0 Joint Cross-Repo Receipt Schema

**Unit**: NGAB0-U10 (C7) — joint cross-repo receipt schema + Faber-scoped audit
entrypoint — see `ngab0-delivery.md` NGAB0-U10.
**Status**: frozen (U10) — schema version `ngab0-receipt-1.0.0`. Version
authority + change procedure ride NGAB0-U7 §Versioning.
**Alignment**: `ngab0-composite-contract.md` §Manifest (content-addressed
artifact identity), §Verification (identity-before-backend-selection order,
receipt alignment), §ResourceIdentity (observations are evidence, never
identity inputs), §OwnershipMatrix (per-surface owners).

## Purpose

A joint cross-repo receipt is the content-addressed convergence record that a
later auditor **re-verifies** rather than trusts (`ngab0-composite-contract.md`
§Verification, "Receipt alignment": verification records exact commands,
content digests, and dirty-state declarations in the joint cross-repo receipt
schema). It binds, at one captured point in time:

- the four repo surfaces — **compiler** (radix), **faber**, **host** (hosts),
  **gradus** — to their content-addressed commits and dirty-state
  declarations;
- **artifact identities** and **content digests** (canonical SHA-256,
  §Manifest default) that the §Manifest rows and §Verification gate consume;
- the **exact commands** that produced every evidence artifact, so a later
  auditor re-runs rather than re-infers.

**Authority order** (same as `ngab0-composite-contract.md`): live
source/tests → accepted artifact schemas + hardware receipts → frozen
contracts → campaign prose. A receipt is **evidence, not an authority** — it
never overrides a frozen contract, and no receipt field mints identity
(§ResourceIdentity: observations are evidence, never identity inputs).

## Receipt blocks (frozen shape)

A receipt is a single document with six blocks. Every block is required; a
block may be empty only when its content is inapplicable (e.g. a
compiler-facts receipt carries no Block 3 hardware context).

### Block 1 — Header

| Field | Carries | Rule |
| --- | --- | --- |
| `receipt_id` | SHA-256 digest over the receipt body | Content-addressed; the receipt is identified by its digest, nothing else |
| `schema_version` | `ngab0-receipt-1.0.0` | Own ratchet (MD2-W1 sibling-field precedent, §Manifest/§Abi); a schema change is a packet change under §Versioning |
| `campaign` | convergence name (e.g. `NGAB0`…`NGAB7`, `PML0`…`PML7`) | Which convergence the receipt closes |
| `phase` | phase id (e.g. NGAB7/PML7 closeout) | Closeout reference |
| `captured_at` | ISO-8601 timestamp | Capture time, never backdated |
| `captured_by` | identity + unit (e.g. `hand-3`, NGAB0-U10) | Who recorded the receipt |

### Block 2 — Repo surfaces (compiler, faber, host, gradus)

One row per surface, mirroring §OwnershipMatrix owners. Rows use the same
container-relative repo paths as `evidence/ngab0-snapshot.md`.

| Field | Carries | Rule |
| --- | --- | --- |
| `surface` | `compiler` (radix) \| `faber` \| `host` (hosts) \| `gradus` | §OwnershipMatrix owner names |
| `repo` | container-relative repo path (e.g. `radix/`) | Same layout as `ngab0-snapshot.md` |
| `commit` | full git SHA of HEAD | Content-addressed repo identity; a short SHA is not a receipt row |
| `dirty_state` | `git status --porcelain` capture: `clean`, or the declared list of uncommitted paths | A receipt is valid only for the declared state; a dirty tree must be declared, never silently accepted |
| `dirty_after` | `git status --porcelain` re-capture after the evidence commands ran | Catches evidence commands that dirty the tree |

### Block 3 — Toolchain / hardware context

Required for hardware-backed evidence (backend-bound, §Verification); may be
omitted for compiler-fact evidence.

| Field | Carries | Rule |
| --- | --- | --- |
| `os` | OS + version (e.g. `macOS 14.5`, `Linux 6.8`) | Named, never inferred |
| `driver` | GPU driver identity (Metal driver / CUDA driver version) | Named, never inferred |
| `device` | physical device name + compute capability where applicable | Backend admission context, §Verification |
| `backend` | admitted variant: `msl-source` \| `metallib` \| `ptx` | Admitted variants (U5); verification precedes backend selection |

### Block 4 — Artifact identities

Per-artifact rows mirroring the §Manifest row fields exactly.

| Field | Carries | Rule |
| --- | --- | --- |
| `artifact_kind` | `msl-source` \| `metallib` \| `ptx` | §Manifest admitted variants |
| `artifact_id` | content digest over artifact bytes (SHA-256) | Identity is derived from bytes only; never reconstructed from emitted text, file names, or paths (§Manifest, Dependency Rule 2) |
| `device_program_version` | wire version of the typed `DeviceProgram` | Rides the accepted wire version; no unversioned artifact |
| `bounds` | byte length + structural ceilings | Recorded, not re-derived; checked before allocation (§Manifest) |
| `target` | backend target the artifact is emitted for (Metal / CUDA) | Verified before backend selection (§Verification) |
| `carrier` | composite executable / manifest row that embeds the artifact | One manifest per executable (§Manifest) |

### Block 5 — Exact commands

| Field | Carries | Rule |
| --- | --- | --- |
| `evidence` | which artifact/observation the command produces | Every evidence artifact maps to its producing command |
| `cwd` | working directory the command runs from | Reproducibility |
| `cmd` | the exact shell command | Verbatim, never paraphrased; a command an auditor cannot re-run is not a command row |
| `env` | required non-default environment (e.g. `RADIX_ROOT`) | Only non-default env is recorded |

### Block 6 — Verification linkage

| Field | Carries | Alignment |
| --- | --- | --- |
| `order` | identity verification → model-to-kernel binding → capability admission → session | §Verification fixed order: verification precedes every downstream selection |
| `fail_closed` | `true` | Tamper/mismatch → pre-launch failure, no CPU fallback (§Verification, Development Posture) |
| `digest_algo` | `SHA-256` | §Manifest canonical default; no other algorithm may be admitted |
| `read_only` | `true` | Verification/admission never mutates artifacts or mints identity (§Verification read-only gate) |

## Receipt lifecycle rules (frozen)

- A receipt is captured when evidence is produced; it is never backdated.
- `git status --porcelain` is recorded per repo **before and after** evidence
  commands (Block 2 `dirty_state` / `dirty_after`).
- Digests are computed over artifact bytes alone (e.g. `shasum -a 256`
  precedent, §Manifest content-addressed rule).
- Commands are recorded verbatim; a summarized command is not evidence.
- Hardware-backed evidence carries Block 3; compiler-fact evidence may omit it.
- A receipt is evidence, not an identity input (§ResourceIdentity); it cannot
  alter a resource's identity, which is derived from bytes and the manifest
  only.

## Audit entrypoint selection rationale (council C7)

**C7**: *"Joint cross-repo receipt schema + scoped audit entrypoints. NGAB0
must add/select a Faber-scoped audit entrypoint (shared radix status audit is
bookkeeping, not artifact proof) and a content-addressed convergence receipt
for NGAB7/PML7."*

**Selected**: add `faber/scripta/check-factory-goal-status` — a thin wrapper
invoking `radix/scripta/audit-factory-goal-status.py` with faber's
`docs/factory` root, mirroring the README generator's `--factory-root`.

Rationale:

- The shared audit script already accepts `--factory-root` (consumed at line
  591; recorded as a drift correction in `evidence/ngab0-snapshot.md` §5), so
  a faber-scoped entrypoint reuses the **same classifier** against faber's own
  inventory — one source of truth for the status vocabulary and buckets, no
  forked second audit.
- The wrapper is intentionally thin (the only code this phase writes per
  `ngab0-delivery.md` NGAB0-U10): selection only, all other arguments pass
  through to the shared script.

Alternatives considered and rejected:

| Option | Verdict | Reason |
| --- | --- | --- |
| Reuse `radix/scripta/check-factory-goal-status` as-is | rejected | Hard-bound to radix's `docs/factory` (it `cd`s to the radix root); does not audit faber goals |
| New standalone faber audit script | rejected | Forks the classification logic and status vocabulary from the shared audit — dual authority |
| No faber-scoped entrypoint | rejected | C7 requires faber-scoped status proof; the shared container audit is bookkeeping, not artifact proof for faber |

## Versioning

Schema `ngab0-receipt-1.0.0` is frozen here. Revisions follow NGAB0-U7
§Versioning change procedure and are labeled revisable through PML1/NGAB1.
