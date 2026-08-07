# Delivery — Component Release Streamline Stage 1: Release Contract, Authority, And Manifest Schema

**Status**: ready — single decision-contract Hand unit; docs only, no product code
**Campaign**: [component-release-streamline](CAMPAIGN.md) — Stage 1 of 10
**Unit title**: `component-release-streamline-stage1-release-contract`
**Control plane**: `/Users/ianzepp/work/faberlang/faber`
**Owners**: `faber/docs/release/` (repo-owned release docs), with radix/cista/faberlang.dev reviewers (read-only survey)
**Created**: 2026-08-07
**Source of truth**: the Stage 1 section, Release Object Model, Authority And Durable Homes, Candidate/Publication/Promotion State Machine, Platform/Builder/Reproducibility Contract, Supply Chain And Secret Boundary, Failure And Recovery Matrix, and the Stage-0 handoff (stage0-baseline.md §9 B11, facts §1–§8). This spec pins the delivery contract; it does not reopen campaign decisions.

---

## Outcome

Accepted decisions that define every release-changing contract so a later
planner can implement (Stage 2 runbook, Stage 3 harness, Stage 4 packaging)
without choosing identity, authority, channel, platform, trust, or rollback
policy. The unit produces **decision records and process/recipe documentation
only** — it does not perform, script, or rehearse a release.

Decisions made (each `accepted` | `explicitly-deferred-with-owner` | `routed`
— no silent default):

1. **Council-4 routing authority for `faber/docs/release/`** — the first
   decision item (see interlock below). This campaign is the process/schema
   owner; its own Stage-1 docs are the first documents under the authority it
   records.
2. The **10 Open Questions** (OQ1–OQ10) resolved per the §9 handoff mapping
   into the five target docs.
3. The **release contract**: product vs component release units and
   version/compatibility rules, channels, artifact identity and immutability,
   authenticity/provenance, public host, cista surface.
4. The **manifest schema**: machine-readable Faber input/pin manifest.
5. The **platform/builder matrix**: targets, support tiers, builders, gates,
   and what blocks promotion.
6. The **authority model**: proposer/builder/verifier/tagger/signer/publisher/
   promoter/withdraw roles.
7. The **failure/recovery matrix**: abort, partial failure, withdrawal,
   revocation, supersede.
8. The **local-first process doc** and the **worktree dry-run recipe**
   (documentation; Stage 3 scripts the recipe).

**Gate:** each release-changing decision is accepted, explicitly deferred, or
routed to an owner. A later planner can implement without choosing identity,
authority, channel, platform, trust, or rollback policy.

## write_scope

Seven artifacts, **all under `faber/docs/release/`**:

- **`release-contract.md`** — product vs component release units +
  version/compatibility rules (OQ1); channels (development /
  candidate-prerelease / stable / LTS / hotfix) reconciled with the normative
  `policy.md` odd/even major-lane rule (OQ1, OQ7); artifact identity, archive
  schema boundary, immutability, idempotent retry, global `Latest` semantics
  (OQ6, OQ7); authenticity: signed checksum manifest / per-artifact signature
  / platform signing / provenance / SBOM (OQ5); public host decision (OQ8);
  cista surface (OQ10). Includes the **Stage-1 decision ledger** section (all
  10 OQs + council-4 item with markings) so the gate "accepted / deferred /
  routed, no silent default" is auditable.
- **`release-manifest-schema.md`** — machine-readable Faber input/pin manifest
  (OQ2): schema, location, update authority; pins source repos and, as
  decided, package/reference/locale inputs; authoritative version sources and
  intentional exclusions (e.g. `crates/hygiene-ratchet`); consumed by both
  local and CI builds by design (schema only, no implementation).
- **`platform-builder-matrix.md`** — faber/radix/cista target rows with
  support tier (supported/experimental/deferred), native vs cross build,
  controlled builder (burgus / pharos / self-hosted / hosted runner), OS/SDK
  and toolchain, signing identity, required gates, and which missing leg
  blocks the whole release vs only that target (OQ3, OQ4).
- **`authority.md`** — proposer/builder/verifier/tagger/signer/publisher/
  promoter/withdraw roles (OQ9); production authority is operator-authorized;
  agents may prepare, inspect, and dry-run by default; withdrawal and
  revocation authority named.
- **`failure-recovery-matrix.md`** — abort / partial-failure / withdrawal /
  revocation / supersede semantics and the immutability exceptions (OQ6);
  resolves the campaign's Failure And Recovery table; no silent `--clobber`,
  tag move, or stable-asset replacement; compromised-credential path named.
- **`process-local-first.md`** — the decided local-first process flow: step
  order, per-step classification (`local` / `controlled-builder` / `network` /
  `public mutation`), the local proof of releasability, gate mapping (faber
  `release-gate`, radix ladder, cista smoke), and the private-radix leakage
  gate. Contract-level process; the full operator runbook + threat model is
  Stage 2.
- **`worktree-dry-run-recipe.md`** — the decided rehearsal procedure: worktree
  layout under `faberlang/worktrees/` (factory worktree discipline), steps up
  to `would-tag` / `would-upload`, scrubbed credentials, no public mutation,
  explicit output directory. Documentation only; Stage 3 scripts this recipe.

No edits to historical notes (`faber/docs/release/v*.md`), `policy.md`, or the
reconciled `process-versioning-and-deps.md` — decision docs cite them.

Read-only references (cite, never modify):

- `stage0-baseline.md` — the B11 open-decision handoff, §1–§8 fact set.
- `faber/docs/release/process-versioning-and-deps.md` — canonical reconciled
  fact set (cite, do not duplicate).
- `faber/docs/release/policy.md` — normative version-lane/channel policy.
- `faber/AGENTS.md`, `radix/AGENTS.md` release protocols; the three
  `release.yml` workflows; `core-support-manifest.txt`.
- `faber/docs/factory/release-and-portable-default/delivery.md` — sibling:
  **what** a good archive contains; this stage owns **how** components ship.
- `radix/docs/factory/worktree-convention.md` — rehearsal-layout reference.
- `faber/docs/factory/faber-onboarding/{delivery-stage1.md,dev-kit-contract.md,package-and-lock-contract.md}`
  (as they land) — sibling payload-content decisions; the routing-authority
  wording must match (non-overlap check only; read-only).

Forbidden roots: `radix/`, `cista/`, `hosts/`, `faber-runtime/`,
`faberlang.dev/` — survey and cite only; their protocol/docs are later stages.

## done_when

Objective decision completeness — every item below must be satisfied:

- **B1.** All seven artifacts (`release-contract.md`,
  `release-manifest-schema.md`, `platform-builder-matrix.md`, `authority.md`,
  `failure-recovery-matrix.md`, `process-local-first.md`,
  `worktree-dry-run-recipe.md`) are committed under `faber/docs/release/`.
- **B2.** The **council-4 routing-authority decision is the first item** in
  `release-contract.md`'s decision ledger: the single routing authority for
  `faber/docs/release/` is named (default: this coordinated product release
  process contract — `release-contract.md` + `release-manifest-schema.md`),
  the faber-onboarding dev-kit payload manifest (its Stage 2) is defined as a
  section of this single schema (not a parallel document), and the two
  campaigns' Stage-1 outputs are non-overlapping. The wording matches the
  sibling campaign's Stage-1 ledger.
- **B3.** All 10 Open Questions are resolved in the target docs with the §9
  mapping; every release-changing decision is `accepted` |
  `explicitly-deferred-with-owner` | `routed`, no silent default.
- **B4.** `release-contract.md` covers: product vs component release units +
  version rules (OQ1); channels reconciled with `policy.md` (OQ1/OQ7);
  artifact identity, archive schema boundary, immutability + idempotent retry,
  global `Latest` (OQ6/OQ7); authenticity: signed checksum / signatures /
  provenance / SBOM (OQ5); public host (OQ8); cista surface (OQ10).
- **B5.** `release-manifest-schema.md` names the schema, its location, its
  update authority, the pinned inputs (source repos and, as decided,
  package/reference/locale inputs), and the intentional exclusions (OQ2).
  Both local and CI builds consume it by design.
- **B6.** `platform-builder-matrix.md` has a row per faber/radix/cista target
  with support tier, native/cross, builder, toolchain/SDK, signing, gates, and
  whether a missing leg blocks the whole release or only that target
  (OQ3/OQ4).
- **B7.** `authority.md` names proposer/builder/verifier/tagger/signer/
  publisher/promoter/withdraw roles; production authority is operator-owned;
  agents may prepare/inspect/dry-run by default (OQ9).
- **B8.** `failure-recovery-matrix.md` resolves abort / partial-failure /
  withdrawal / revocation / supersede semantics (OQ6 exceptions); no silent
  `--clobber` or tag move; compromised-credential path named.
- **B9.** `process-local-first.md` states the decided local-first flow with
  per-step `local` / `controlled-builder` / `network` / `public mutation`
  classification, the local proof of releasability, the gate mapping, and the
  private-radix leakage gate.
- **B10.** `worktree-dry-run-recipe.md` documents the rehearsal procedure
  (layout, `would-tag`/`would-upload` stop points, scrubbed credentials, no
  public mutation) as the recipe Stage 3 scripts. No script is authored here.
- **B11.** Historical release notes, `policy.md`, and the reconciled
  `process-versioning-and-deps.md` are untouched; the new docs cite them
  without duplicating the fact set.
- **B12.** No product code, no scripts authored, no worktree created, no
  release executed, no tag pushed, no workflow run, main untouched except
  docs.

## validation

Docs-only. At closeout, exactly **one** pass:

1. Regenerate the faber factory README (the campaign dir and `faber/docs/release/`
   change):
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/generate-factory-readme.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`
   then `... --check`.
2. Goal-status audit, target **0 findings**:
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/audit-factory-goal-status.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`.
3. Internal proof: every decision row cites live evidence (stage0-baseline
   §/F# or a file:line) and carries a marking; the routing-authority wording
   matches the faber-onboarding ledger; every doc is date-stamped.

No cargo builds, no ladder runs, no `release-gate`, no workflow execution,
no worktree creation — this stage is docs-only.

## forbids

- No product code; **no scripts authored** (scripts are Stage 3).
- **No worktree creation** (the recipe is documentation; the harness is
  Stage 3).
- No release execution: no tags, no pushes, no `gh release`, no `--clobber`,
  no asset mutation, no workflow runs, no cargo builds, no `release-gate`, no
  ladder runs.
- No edits to `radix/`, `cista/`, `hosts/`, `faber-runtime/`,
  `faberlang.dev/` (read-only references).
- No edits to `faber/docs/release/v*.md`, `policy.md`, or the reconciled
  `process-versioning-and-deps.md` — cite and date-stamp, never rewrite.
- No Stage-2 runbook/threat-model implementation and no Stage-3 harness
  content beyond the decided recipe; no pre-writing of the faber-onboarding
  payload manifest (that is its Stage 2, authored as a section of this
  schema).
- No ambient credentials or private tokens; never run the private-radix GHA
  path.
- No verification loops: one closeout pass, then done.

## risks

- **Reconcile, not rewrite** — the seven new docs must not become a second
  inventory; they cite `process-versioning-and-deps.md` as the fact set.
- **Overlap with `faber-onboarding`** — both Stage-1 outputs converge on
  `faber/docs/release/`. The routing-authority decision (first ledger item)
  and the sibling's identical wording keep them non-overlapping; this
  campaign's `release-manifest-schema.md` (release pins) is distinct from the
  sibling's `package-and-lock-contract.md` (package semantics) — the
  distinction is recorded, not merged.
- **Channels clash with `policy.md`** — `policy.md` already names odd/even
  major lanes and `latest` rules; the channel decision reconciles with it
  instead of inventing a parallel channel model.
- **Stop-if: product input identity, stable asset immutability, or production
  authority unresolved** — the unit must not write an executable release
  procedure around silent defaults; it routes the unresolved item as a need.
- **Live drift** — tags/versions may change between the 2026-08-07 baseline
  and this stage; date-stamp and re-verify claims that would change a
  decision.
- **Foreign WIP in faber `src/package/*` and `corpus/`** — uncommitted
  unrelated changes; do not touch.
- **GHA-related claims** — `--clobber` and checksum-naming claims were
  re-verified 2026-08-07 (F4/F7); re-verify at decision time rather than
  carrying them forward blind.
- **Worktree path conflicts** — the recipe must follow the factory worktree
  convention and never auto-prune foreign worktrees (operator packet
  lifecycle).

## Council-4 interlock — routing authority is the FIRST decision item

This campaign's Authority And Durable Homes table names `faber/docs/release/`
as the coordinated product process home, and this Stage 1 produces the release
manifest schema there. `faber-onboarding`'s Stage 1/2 produces the dev-kit
payload manifest (every component, version, digest, compatibility bound,
license, destination) — the same directory. **A single routing authority for
`faber/docs/release/` is a Stage-1 planning input that must be decided before
the outputs overlap** (council-4). It is therefore the **first decision item**
in this Stage-1 spec and in `release-contract.md`'s decision ledger.

**Default (shared with the sibling Stage-1 spec, so the two ledgers agree):**
`faber/docs/release/` is governed by a **single routing authority — the
coordinated product release process contract** (this campaign's
`release-contract.md` + `release-manifest-schema.md`). The faber-onboarding
dev-kit payload manifest (its Stage 2) is authored as the **"dev-kit payload
section" of this single schema** (release-owned packs: launcher, core support,
reference/locale packs, libraries), never as a parallel document. Onboarding
Stage 1 writes nothing to `faber/docs/release/`; its payload-shape decisions
define **content** only, which Stage 2 encodes into this schema under this
process contract's review.

The ledger records the accepted decision, and the two campaigns' Stage-1
outputs are non-overlapping by construction: this stage = the seven docs under
`faber/docs/release/`; sibling Stage 1 = decision records in its campaign dir.

---

## Suggested closeout evidence

Seven artifacts committed under `faber/docs/release/`; README regenerated and
audit clean; unit reply cites B1–B12 by letter and names the routing-authority
decision wording used.
