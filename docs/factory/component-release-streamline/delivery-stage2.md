# Delivery — Component Release Streamline Stage 2: Runnable Release Process, Threat Model, And Worktree Rehearsal

**Status**: ready — docs-and-procedure unit; no scripts, no worktrees, no release execution
**Campaign**: [component-release-streamline](CAMPAIGN.md) — Stage 2 of 10
**Unit title**: `component-release-streamline-stage2-runnable-process-worktree-rehearsal`
**Control plane**: `/Users/ianzepp/work/faberlang/faber`
**Owners**: `faber/docs/release/` (repo-owned release docs); radix/cista thin runbooks staged to Stage 3 (recorded below)
**Created**: 2026-08-07
**Base**: faber main `70cca44` (clean except foreign WIP in `src/package/device/*`)
**Source of truth**: CAMPAIGN Stage 2 section + gate, Release Object Model, Authority And Durable Homes, the seven Stage-1 docs (`release-contract.md`, `release-manifest-schema.md`, `platform-builder-matrix.md`, `authority.md`, `failure-recovery-matrix.md`, `process-local-first.md`, `worktree-dry-run-recipe.md`), and `stage0-baseline.md` facts F1–F8 / §4 classifications. This spec pins the delivery contract; it does not reopen campaign decisions.

---

## Outcome

The decided release process becomes **cold-operator-runnable**: the coordinated
product runbook and threat model land under `faber/docs/release/`, and the
worktree dry-run recipe (`worktree-dry-run-recipe.md`) is **operationalized as a
documented runnable procedure** — exact commands, the operator checklist, and
the pin-matrix generation steps — with zero public effect.

- **Runbook:** a cold operator can identify the authority and the exact next
  command for component-only, Faber product, prerelease, stable, hotfix, abort,
  and withdrawal paths without asking chat (CAMPAIGN Stage 2 gate).
- **Threat model:** covers private Radix leakage, untrusted build inputs,
  credential exposure, and asset replacement.
- **Rehearsal procedure:** the dry-run recipe as a runnable procedure — worktree
  packet create/reuse/dispose (operator-managed per
  `radix/docs/factory/worktree-convention.md`), pin-matrix generation by
  hand-followable steps, `would-tag` / `would-upload` stop points, scrubbed
  credentials, and the receipt schema.
- **Operator checklist:** one-page, command-by-command, with classifications and
  stop points.
- **Boundary:** documentation and procedure only. Scripts are Stage 3. Thin
  component runbooks (`radix/docs/release/`, `cista/docs/release/`) are staged
  to Stage 3 because they "point to their local scripts" (CAMPAIGN Stage 2
  outcome) and no scripts exist until Stage 3 — recorded as a staging decision
  with owner (Stage 3). The campaign Stage 2 gate is still met: the shared
  runbook names the exact next commands for component-only paths.

**Batching:** docs-first; all artifacts under the repo-owned release docs home
(`faber/docs/release/`), none in the non-repo container.

---

## write_scope

Writes (faber repo only):

- **`faber/docs/release/release-runbook.md`** (new) — the coordinated operator
  runbook. For each path (component-only faber / radix / cista, Faber product,
  prerelease/candidate, stable, LTS, hotfix, abort, withdrawal/supersede): the
  authority role that may act (`authority.md`), the exact next command(s), the
  gates that gate that path (`process-local-first.md` §3), and the stop
  conditions. Steps 1–8 of the local-first flow (`process-local-first.md` §1)
  each carry their classification (`local` / `controlled-builder` / `network` /
  `public mutation`) and the dry-run stop point. Consumes the dev-kit payload
  manifest instance (`faber/release-manifest.yaml`) in the prepare step.
- **`faber/docs/release/threat-model.md`** (new) — the four classes from the
  campaign Stage 2 gate: private Radix source/path/token/log leakage (the
  leakage gate in `process-local-first.md` §4, extended to receipts and the
  pin matrix), untrusted build inputs (pinned SHAs, toolchain, builder
  provenance per `platform-builder-matrix.md`), credential exposure (least
  privilege, no secrets in build/rehearsal envs or logs), and asset replacement
  (immutability + fail-closed collision per `release-contract.md` §5.3).
- **`faber/docs/release/worktree-rehearsal-procedure.md`** (new) — the
  operationalized rehearsal: the runnable form of `worktree-dry-run-recipe.md`.
  Exact commands for packet naming (`release-dry-run-<version>`), member
  creation/reuse/disposal (operator-managed; no auto-prune of foreign
  worktrees; existing worktrees `exact-output-transfer` and
  `test-lifecycle-split` are foreign), detached pinned members at the manifest's
  exact commits, the explicit `out/` dir, scrubbed credentials, and the
  `would-tag` / `would-upload` plan + receipt steps.
- **`faber/docs/release/release-checklist.md`** (new) — the operator checklist:
  one line per flow step (prepare → local proof → tag → controlled-builder →
  package/checksum/sign → publish candidate → readback → promote →
  withdraw/supersede), with classification, the authority gate for each public
  step, and the stop points.
- **Pin-matrix generation procedure** — a numbered section of
  `worktree-rehearsal-procedure.md`: how the operator produces the candidate pin
  matrix by hand from live evidence before Stage 3 scripts exist — verify each
  source tag SHA against the remote, resolve sibling commits, list the dev-kit
  payload packs + digests from the onboarding manifest instance, and record the
  result in the candidate plan. Stage 3's `generate-release-manifest` automates
  this same procedure.
- **`faber/docs/factory/component-release-streamline/delivery-stage2.md`** —
  this spec.
- **`faber/docs/factory/README.md`** — regenerated (doc-count change).

Read-only references (cite, never modify):

- The seven Stage-1 docs under `faber/docs/release/` (incl.
  `worktree-dry-run-recipe.md` — this delivery operationalizes it, does not
  rewrite it), `policy.md`, `process-versioning-and-deps.md`, historical
  `v*.md`.
- `faber/docs/factory/faber-onboarding/{delivery-stage2.md, dev-kit-contract.md}` —
  the dev-kit payload manifest instance and pack contract the pin matrix must
  include (read-only; non-overlap).
- `radix/docs/factory/worktree-convention.md` — packet layout + lifecycle.
- `faber/AGENTS.md`, `radix/AGENTS.md` release protocols; the three
  `release.yml` workflows (evidence, not to edit).
- `faber/.github/workflows/release.yml` etc. — observed; not touched.

Forbidden roots: `radix/`, `cista/`, `hosts/`, `faber-runtime/`,
`faberlang.dev/` — survey and cite only (thin component runbooks are staged to
Stage 3, not authored here). No edits to the Stage-1 seven docs, `policy.md`,
`process-versioning-and-deps.md`, or `v*.md` — cite and date-stamp, never
rewrite.

---

## done_when

Objective completion — every item below must be satisfied:

- **D1.** This delivery spec is committed.
- **D2.** `release-runbook.md` committed: a cold operator can identify the
  authority and exact next command for component-only (faber / radix / cista),
  Faber product, prerelease, stable, hotfix, abort, and withdrawal paths without
  asking chat. Every flow step carries its classification and stop point.
- **D3.** `threat-model.md` committed: covers private Radix leakage, untrusted
  build inputs, credential exposure, and asset replacement, each with the
  decided control from the Stage-1 docs.
- **D4.** `worktree-rehearsal-procedure.md` committed: the recipe is runnable
  step-by-step with exact commands — packet layout, member lifecycle
  (operator-managed, no auto-prune), detached pinned members, `out/`, scrubbed
  credentials, `would-tag` / `would-upload` stop points, and the receipt schema.
- **D5.** Pin-matrix generation is a numbered, hand-followable procedure
  (source tag SHA verification, sibling commit resolution, dev-kit pack +
  digest listing from the onboarding manifest instance) that Stage 3 scripts
  will automate.
- **D6.** `release-checklist.md` committed: one line per flow step with
  classification, authority gate, and stop point; usable cold.
- **D7.** The runbook's prepare step consumes the dev-kit payload manifest
  instance (`faber/release-manifest.yaml`) and the pin matrix includes its
  `pinnedInputs.packs` rows — the onboarding interlock is documented, not
  duplicated.
- **D8.** Thin component runbooks are recorded as staged to Stage 3 with owner
  (streamline Stage 3), and the shared runbook names the exact component
  commands so the campaign Stage 2 gate holds without them.
- **D9.** No script authored, no worktree created/removed, no release executed,
  no tag pushed, no workflow run, no cargo invocation, main untouched except the
  four new docs + this spec + README.
- **D10.** The Stage-1 seven docs, `policy.md`, `process-versioning-and-deps.md`,
  and `v*.md` are untouched; every command in the new docs cites the protocol or
  Stage-1 doc it implements.

## validation

Docs-only. At closeout, exactly **one** pass:

1. Regenerate the faber factory README and audit:
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/generate-factory-readme.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`
   then `... --check`; then
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/audit-factory-goal-status.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory` — target **0 findings**.
2. Internal proof: one cold-operator walkthrough of a named path (e.g. a Faber
   product dry-run) produces a concrete command sequence and a `would-tag` /
   `would-upload` plan with no public effect; every row cites live evidence or a
   Stage-1 doc; the docs are date-stamped; the rehearsal procedure names
   `faberlang/worktrees/release-dry-run-<version>/` and treats existing
   worktrees as foreign.

No cargo builds, no ladder runs, no `release-gate`, no workflow execution, no
worktree creation.

## forbids

- No product code; **no scripts authored** (scripts are Stage 3).
- **No worktree creation/removal/sync** — the procedure documents the
  operator-managed lifecycle; nothing executes it.
- No release execution: no tags, no pushes, no `gh release`, no `--clobber`,
  no asset mutation, no workflow runs, no cargo builds, no `release-gate`, no
  ladder runs.
- No edits to `radix/`, `cista/`, `hosts/`, `faber-runtime/`,
  `faberlang.dev/` (thin runbooks are Stage 3).
- No edits to the Stage-1 seven docs, `policy.md`,
  `process-versioning-and-deps.md`, or historical `v*.md`.
- No rewriting `worktree-dry-run-recipe.md` — the new procedure operationalizes
  it and cites it.
- No ambient credentials or private tokens; never run the private-radix GHA
  path.
- No pre-writing of `release-manifest.schema.json`, the manifest generator, or
  `package-archive` (Stage 3).
- No verification loops: one closeout pass, then done.

## risks

- **Onboarding interlock:** the pin-matrix procedure and the runbook's prepare
  step must consume the faber-onboarding payload manifest instance without
  owning it. If the onboarding Stage 2 instance shape drifts from
  `release-manifest-schema.md` §3/§4/§6, the procedure must still name the
  packs/digests rows — both specs cross-reference the same schema section.
- **Procedures cite nonexistent scripts:** no Stage-3 script exists yet; the
  runbook and rehearsal procedure must document hand-followable steps (exact
  commands that work today) and mark the scripted replacements as Stage 3 —
  never require a script that does not exist.
- **Cold-operator claim:** the runbook must be self-contained on authority +
  next command (citing, not requiring the reader to traverse all seven Stage-1
  docs); the walkthrough in validation is the proof.
- **Worktree operator lifecycle:** existing worktrees (`exact-output-transfer`,
  `test-lifecycle-split`) and any release-dry-run packets are foreign; the
  procedure never auto-prunes and never merges a rehearsal branch to main.
- **Foreign WIP** in faber `src/package/device/*` — untouched.
- **Live drift** — tags/versions may change after the 2026-08-07 baseline;
  date-stamp and re-verify claims that would change a command.
- **Threat-model completeness creep** — four classes only, each mapping to a
  decided control; no new policy invented in this stage.

---

## Suggested closeout evidence

Four new docs under `faber/docs/release/` committed; README regenerated and
audit clean; unit reply cites D1–D10 by letter, names the pin-matrix procedure
and the staging decision for the thin component runbooks, and includes one
cold-operator walkthrough of a named path.
