# Delivery — Component Release Streamline Stage 0: Live Baseline Reconciliation And Stale-Doc Retirement

**Status**: ready — single discovery-first Hand unit; docs only, no product code
**Campaign**: [component-release-streamline](CAMPAIGN.md) — Stage 0 of 10
**Unit title**: `component-release-streamline-stage0-inventory`
**Control plane**: `/Users/ianzepp/work/faberlang/faber`
**Owners**: container docs + read-only survey of `faber` / `radix` / `cista`
**Created**: 2026-08-07
**Source of truth**: the Stage 0 section, Release Object Model, Authority And
Durable Homes, Ground Truth Researched, and Current State sections of
`CAMPAIGN.md`. This spec pins the delivery contract; it does not reopen
campaign decisions.

---

## Outcome

Reconcile the existing release analysis, policies, notes, AGENTS protocols,
scripts, workflows, live tags/versions, public artifact claims, and current
dependency topology into **one current fact set**. Stale claims are retired or
dated, never silently rewritten. Stage 0 **does not create a second unlinked
inventory** and does not perform a release.

The campaign's gate: current facts committed under `faber/docs/release/`, every
row with live source evidence and a `local` / `controlled-builder` / `network`
/ `public mutation` classification, historical facts dated, no product code.

## write_scope

Writes (docs only, all inside the faber repo):

- **`faber/docs/factory/component-release-streamline/stage0-baseline.md`** —
  the Stage 0 delivery/inventory summary: side-by-side protocol step table
  (faber / radix / cista AGENTS + workflows), local script inventory, network
  vs offline classification, failure modes, tags/artifacts mutability and
  readback table, consumers of published URLs/versions/checksums/`latest`,
  stale-doc disposition ledger, and the Stage 1 open-decision handoff. Links
  to the canonical reconciled facts (below); it is the routing summary, not a
  competing fact set.
- **`faber/docs/release/process-versioning-and-deps.md`** — the canonical
  reconciled fact set (the campaign's stated reconcile target). Update stale
  topology/version/tag/platform claims to the current state — Faber `1.4.0` /
  tag `v1.4.0`, Radix `0.79.0` across 30 release-aligned crates / tag
  `v0.79.0`, Cista `0.1.0` / tag `v0.1.0`; date historical facts and demote
  obsolete content to dated history instead of deleting or rewriting it.

Read-only references (cite, never modify):

- `faber/AGENTS.md`, `radix/AGENTS.md` — release protocol sections.
- `faber/.github/workflows/release.yml`, `radix/.github/workflows/release.yml`,
  `cista/.github/workflows/release.yml` — tag/dispatch workflows, matrix,
  `RELEASE_REPO: faberlang/releases`, `--clobber` upload behavior (observed in
  all three: faber L223, cista L183, radix L186).
- `faber/scripta/release-gate`, `faber/scripta/test`,
  `faber/scripta/nextest-safe`, `faber/scripta/check-store-only-resolve.sh`,
  `radix/scripta/test` ladder — release-adjacent local scripts.
- `faber/core-support-manifest.txt` — pins logical roots by path, not SHA.
- `faber/docs/release/policy.md` and per-version notes `faber/docs/release/v*.md`
  (incl. `v1.4.0.md`, `v1.5.0-dev-notes.md`) — historical records; date-stamp,
  do not rewrite.
- `cista/docs/release/`, `radix/docs/release/` — component release notes
  (read-only in this stage).
- `faber/docs/factory/release-and-portable-default/delivery.md` — sibling:
  **what** a good archive contains; this campaign owns **how** components ship.
- `radix/docs/factory/worktree-convention.md` — rehearsal-layout reference
  (no worktree is created in this stage).
- `faberlang/releases` (GitHub) — observation only; never mutated.

Forbidden roots: `radix/`, `cista/`, `hosts/`, `faber-runtime/`,
`faberlang.dev/` — survey and cite only; their protocol docs are later stages.

## done_when

Objective inventory completeness — every item below must be satisfied:

- **B1.** `stage0-baseline.md` and the reconciled
  `process-versioning-and-deps.md` are committed (faber repo, explicit paths).
- **B2.** Side-by-side step table covers faber, radix, and cista release
  protocols (AGENTS prose + workflow steps), one row per step, each with live
  source evidence (file path or `file:line`).
- **B3.** Every local release-adjacent script is inventoried and classified
  (`faber/scripta/release-gate`, `radix/scripta/test` ladder,
  `faber/scripta/{test,nextest-safe,check-store-only-resolve.sh}`, any cista
  scripts found) — gate / helper / release-adjacent.
- **B4.** Every inventory row carries a classification:
  `local` | `controlled-builder` | `network` | `public mutation`.
- **B5.** Network-required items are named: private radix clone/token in faber
  GHA, `gh release upload` to `faberlang/releases`, remote tag pushes.
- **B6.** Offline-capable items on `burgus`/`pharos` are named (local builds,
  gates, archive+checksum, tag bookkeeping).
- **B7.** Failure modes observed are recorded with evidence: GHA queue
  (macos-13 Intel dropped), lockfile mistakes, sibling path drift,
  `--clobber` overwrite risk.
- **B8.** Tags/artifacts table: faber `v1.4.0`, radix `v0.79.0`, cista
  `v0.1.0` (tag + workflow_dispatch paths), mutability/readback state
  (current workflows upload with `--clobber`), and the checksum naming gap
  (`dist/<archive>` vs downloaded basename) for radix/cista.
- **B9.** Consumers table: current consumers of published URLs, versions,
  checksums, and `latest` — e.g. `faberlang.dev` install docs → GitHub assets,
  `faberlang.dev/src/en-US/toolchain/cli.md` → `faber/docs/release/v1.4.0.md`.
- **B10.** Stale-doc disposition ledger: every stale claim in
  `process-versioning-and-deps.md` is updated-and-dated, demoted to dated
  history, or routed — never silently rewritten; historical release notes are
  untouched.
- **B11.** Stage 1 open-decision handoff: the campaign's 10 Open Questions
  mapped to the Stage 1 artifacts (`release-contract.md`,
  `release-manifest-schema.md`, `platform-builder-matrix.md`, `authority.md`,
  `failure-recovery-matrix.md`) with `answered-by-evidence` |
  `carried-to-stage-1` | `needs-stage-1-decision` markings.
- **B12.** No product code, no scripts authored, no release executed, no tag
  pushed, main untouched.

## validation

Docs-only. At closeout, exactly **one** pass:

1. Regenerate the faber factory README (the campaign dir gains files, so the
   generated README's per-goal doc counts change):
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/generate-factory-readme.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`
   then `... --check`.
2. Goal-status audit, target **0 findings**:
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/audit-factory-goal-status.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`.
3. Internal proof: every row cites live source evidence and carries a
   classification; historical facts are dated; the inventory is date-stamped.

No cargo builds, no ladder runs, no `release-gate`, no workflow execution —
this stage is docs-only.

## forbids

- No product code; no scripts authored (scripts are Stage 3).
- No release execution: no tags, no pushes, no `gh release`, no `--clobber`,
  no asset mutation, no workflow runs, no cargo builds, no `release-gate`,
  no ladder runs.
- No edits to `radix/`, `cista/`, `hosts/`, `faber-runtime/`,
  `faberlang.dev/` (read-only references; their protocol docs are later
  stages).
- No second unlinked inventory: `stage0-baseline.md` links to
  `process-versioning-and-deps.md` as the canonical fact set.
- Do not rewrite historical release notes (`faber/docs/release/v*.md`) —
  date-stamp and demote.
- No worktree creation (worktree rehearsal is Stage 3+).
- No ambient credentials or private tokens; never run the private-radix GHA
  path.
- No verification loops: one closeout pass, then done.

## risks

- **Reconcile, not rewrite** — `process-versioning-and-deps.md` is ~21KB of
  stale-plus-current content; scope discipline is required to avoid turning
  reconciliation into a rewrite.
- **Live drift** — tags/versions may change between inventory and Stage 1;
  date-stamp and re-verify at Stage 1.
- **Uncommitted CAMPAIGN.md revision** — the faber working tree carries an
  uncommitted revision of `component-release-streamline/CAMPAIGN.md`; this
  spec follows the working-tree text. The executor cites repo files, not the
  WIP.
- **Foreign WIP in faber `src/package/*`** — uncommitted unrelated changes;
  do not touch.
- **Mutability claims** — `--clobber` and checksum-naming claims must be
  re-verified against the current workflow YAML at inventory time (observed
  2026-08-07: all three workflows upload with `--clobber`).
- **Overlap with sibling campaigns** — `faber-onboarding` (dev-kit payload)
  and `release-and-portable-default` (portable content) read the same release
  facts; record the interlock (below) rather than resolving it here.

## Council-4 interlock — Stage 1 planning input, NOT a Stage 0 requirement

This campaign's Authority And Durable Homes table names `faber/docs/release/`
as the coordinated product process home, and its Stage 1 produces the release
manifest schema there. `faber-onboarding`'s Stage 1/2 produces the dev-kit
payload manifest (every component, version, digest, compatibility bound,
license, destination) — the same directory. **A single routing authority for
`faber/docs/release/` must be decided at the campaigns' Stage 1 planning
before their decision outputs overlap** (dev-kit payload manifest vs release
manifest schema) — per council-4, that decision is a **Stage 1 planning
input**, named in each campaign's Stage 1 decision ledger (here:
`release-contract.md` / `release-manifest-schema.md`).

Stage 0's only duty is to record the overlap: the Stage 1 handoff (B11) lists
the `faber/docs/release/` artifacts each campaign will produce, so the Stage 1
planner takes the interlock into the council decision. Stage 0 must **not**
pre-write shared schemas or pre-empt the routing decision.

---

## Suggested closeout evidence

`stage0-baseline.md` + reconciled `process-versioning-and-deps.md` committed;
README regenerated and audit clean; unit reply cites B1–B12 by letter.
