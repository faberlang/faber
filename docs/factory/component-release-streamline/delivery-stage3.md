# Delivery — Component Release Streamline Stage 3: Gap Scripts And Thin Component Runbooks

**Status**: ready — script-only implementation unit; local-first, dry-run only, no production release execution
**Campaign**: [component-release-streamline](CAMPAIGN.md) — Stage 3 of 10
**Unit title**: `component-release-streamline-stage3-gap-scripts`
**Control plane**: `/Users/ianzepp/work/faberlang/faber` (writes additionally to `radix/` and `cista/` scripta + runbook paths per campaign ownership)
**Owners**: faber (`faber/scripta/`, `faber/docs/release/`), radix (`radix/scripta/`, `radix/docs/release/runbook.md`), cista (`cista/scripta/`, `cista/docs/release/runbook.md`)
**Created**: 2026-08-07
**Base**: faber main `70cca44` (foreign WIP in `src/package/device/*` untouched)
**Source of truth**: CAMPAIGN Stage 3 section + gate; `process-local-first.md` §1 flow / §2 local proof / §4 leakage gate; `release-manifest-schema.md` §2–§7 (schema + update authority + consumers); `release-contract.md` §5.1 (archive naming + basename-only checksum) and §5.3 (immutability/idempotent retry); `worktree-dry-run-recipe.md` + `worktree-rehearsal-procedure.md` (Stage 2, as it lands) — the procedure these scripts automate; `stage0-baseline.md` §2 (script conventions), §3 (pin facts), §5 (F2 lockfile mistakes). This spec pins the delivery contract; it does not reopen campaign decisions.

---

## Outcome

The gap scripts land as **few standalone shell/python helpers** under the repos'
scripta conventions, closing the mechanical gaps the campaign inventory named
(`stage0-baseline.md` §2/§3/§5): bulk radix version bump, lockfile regen,
pin-file generation (machine schema + instance generator + validator),
archive+checksum, and a release-doctor preflight. Each helper has a focused unit
test. The thin radix/cista component runbooks (staged from Stage 2) land and
point at the local scripts + the shared contract. Everything is local-first and
dry-run-only: helpers emit `would-tag` / `would-upload` placeholders where a
public effect would occur, never touch credentials, and never execute a
production release.

**Batching:** split-on-boundary by repo ownership (faber / radix / cista),
script-only, no new SaaS, stdlib-only python3 (radix scripta convention).

---

## write_scope

Writes (exact paths):

**faber repo**
- **`faber/scripta/regen-lock`** (new; python3, executable) — regenerate and
  verify the faber lockfile: `cargo update` (documented `--offline` caveat),
  then assert the lock matches the manifests (`--locked`-readiness, F2). A
  `--check` mode verifies freshness without writing.
- **`faber/scripta/generate-release-manifest`** (new; python3) — pin-file
  generation: writes `faber/release-manifest.yaml` from live evidence — the
  release intent (version, channel, line), pinned source SHAs (faber tag commit
  + radix/cista/faber-runtime/hosts commits), the dev-kit payload `pinnedInputs.packs`
  rows (digests from the faber-onboarding `assemble-dev-kit` payload receipt),
  authoritative version sources, and intentional exclusions — then validates the
  instance. Rejects ad-hoc edits outside a release intent (schema §7).
- **`faber/scripta/validate-release-manifest`** (new; python3) — validates a
  `release-manifest.yaml` against the machine schema; hard stop on any
  mismatch. Must accept the faber-onboarding Stage-2 instance shape (interlock).
- **`faber/docs/release/release-manifest.schema.json`** (new) — the machine
  JSON Schema (draft 2020-12) derived exactly from `release-manifest-schema.md`
  §2–§7 decisions. **`faber/docs/release/release-manifest.example.yaml`** (new)
  — the human-readable companion example.
- **`faber/scripta/package-archive`** (new; python3) — archive + checksum for
  the faber product: wraps a payload staging dir (from the onboarding
  `assemble-dev-kit`) into `<component>-v<version>-<target-triple>.tar.gz` with
  a basename-only `.sha256` per `release-contract.md` §5.1 (`shasum -c` works on
  a downloaded set). Idempotent on identical hashes; different-hash collision
  fails closed (§5.3).
- **`faber/scripta/release-doctor`** (new; python3) — preflight, local + dry-run:
  clean tree, correct remote, version/tag alignment (Cargo.toml == candidate
  version == no existing tag), lockfile fresh, release notes present
  (`docs/release/v<version>.md`), pin manifest valid + pins match live evidence,
  dev-kit packs present (onboarding interlock), and no ambient release
  credentials in env. Emits `would-tag` / `would-upload` placeholders in a plan
  file; never proceeds past them. Worktree state is advisory-only (operator
  lifecycle per `worktree-convention.md`; no creation/disposal).
- **`radix/docs/release/runbook.md` and `cista/docs/release/runbook.md`** — no
  (these are the sibling repos' thin runbooks; see below).
- **`faber/docs/factory/component-release-streamline/delivery-stage3.md`** —
  this spec.
- **`faber/docs/factory/README.md`** — regenerated.

**radix repo**
- **`radix/scripta/bump-version`** (new; python3, executable) — bulk version
  bump across the **30 release-aligned `crates/*/Cargo.toml`** with the
  intentional exclusion (`crates/hygiene-ratchet` stays `0.1.0`,
  `process-versioning-and-deps.md` §1.5/§2.2). `--check` mode verifies alignment
  without writing; a single-commit bump+lock flow is its documented use.
- **`radix/scripta/regen-lock`** (new; python3) — radix lockfile regen +
  freshness verification (same contract as faber's).
- **`radix/docs/release/runbook.md`** (new) — the thin component runbook:
  points at the shared `faber/docs/release/release-runbook.md` contract + these
  radix-local scripts; names the radix component release path (bump → regen-lock
  → locked build → ladder `--full` at tag → tag → publish) per `radix/AGENTS.md`.

**cista repo**
- **`cista/scripta/regen-lock`** (new; python3) — cista lockfile regen +
  freshness (cista has no scripta dir today; this creates it under the same
  convention).
- **`cista/docs/release/runbook.md`** (new) — the thin component runbook:
  shared contract + cista-local steps (tag → workflow_dispatch, publish to
  `faberlang/releases`); records the unfulfilled `cista-v0.1.0` publish as a
  routed residual for the cista owner (F5).

**Unit tests** (one focused test file per helper, runnable as
`python3 <name>-test.py`, following the radix `*-test.py` convention):
- `faber/scripta/{regen-lock,generate-release-manifest,validate-release-manifest,package-archive,release-doctor}-test.py`
- `radix/scripta/{bump-version,regen-lock}-test.py`
- `cista/scripta/regen-lock-test.py`

Read-only references: `release-manifest-schema.md`, `release-contract.md`,
`process-local-first.md`, `worktree-rehearsal-procedure.md` (Stage 2, as it
lands), `stage0-baseline.md`, the three `release.yml` workflows (observed, not
edited), `faber/docs/factory/faber-onboarding/delivery-stage2.md` (payload
contract), `radix/docs/factory/worktree-convention.md`.

Forbidden roots: `faberlang.dev/`, `hosts/`, `faber-runtime/`, `examples/`,
`norma/`, `triga/` — not touched. No edits to the Stage-1 seven release docs,
`policy.md`, `process-versioning-and-deps.md`, `v*.md`, or the three `release.yml`
workflows (CI thinning is Stage 8).

---

## done_when

Objective completion — every item below must be satisfied:

- **D1.** This delivery spec is committed.
- **D2.** Every authored helper exists, is executable, follows the repo's
  scripta convention (`#!/usr/bin/env python3`, docstring, stable exit codes,
  stdlib-only), and has a focused unit test that passes.
- **D3.** `bump-version` bumps all 30 release-aligned radix crates in one pass
  with `crates/hygiene-ratchet` excluded; `--check` verifies alignment without
  writing; the test proves both.
- **D4.** `regen-lock` (faber + radix + cista) regenerates and verifies lock
  freshness with the `--offline` caveat documented; `--check` verifies without
  writing.
- **D5.** `release-manifest.schema.json` matches `release-manifest-schema.md`
  §2–§7 (no drift); `generate-release-manifest` produces an instance that
  validates; `validate-release-manifest` accepts the faber-onboarding Stage-2
  instance shape (packs rows per §6) and rejects malformed input.
- **D6.** `package-archive` produces `<component>-v<version>-<triple>.tar.gz` +
  basename-only `.sha256` such that `shasum -c` passes on a downloaded set;
  identical-hash retry is idempotent; different-hash collision fails closed.
- **D7.** `release-doctor` passes on a clean prepared candidate and fails with
  named reasons on: dirty tree, wrong remote, version/tag mismatch, stale lock,
  missing notes, missing dev-kit packs, and ambient release credentials. It
  never proceeds past `would-tag` / `would-upload` placeholders.
- **D8.** Thin runbooks committed (`radix/docs/release/runbook.md`,
  `cista/docs/release/runbook.md`) pointing at the shared contract + local
  scripts; the cista residual (F5) is recorded, not fixed.
- **D9.** Dry-run-only: no helper touches credentials, pushes tags, uploads
  assets, or writes public state; no production release executed; no new SaaS.
- **D10.** No edits to the Stage-1 seven docs, `policy.md`,
  `process-versioning-and-deps.md`, `v*.md`, or the three `release.yml`
  workflows; no cargo test/build suites run in the dev loop (Cargo discipline);
  main untouched except scripta + runbook + spec + README.

## validation

Narrow in-loop (only the narrowest checks that falsify the change):

1. Each authored helper's unit test run individually as written
   (`python3 <name>-test.py`); a failing helper is fixed and that test re-run.
2. `validate-release-manifest` against the onboarding Stage-2 instance (once
   it exists) or a fixture matching the documented shape.

Closeout — exactly **one** pass, after the last edit:

1. All authored helper unit tests pass (one run each, no re-runs).
2. `./scripta/test --check` (stage 1) from `faber/` — the equivalent cheap Faber
   ladder; the radix/cista helpers are additive and their unit tests are their
   proof. No radix ladder run, no `release-gate`.
3. Regenerate the faber factory README and audit:
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/generate-factory-readme.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`
   then `... --check`; then
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/audit-factory-goal-status.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory` — target **0 findings**.
4. Internal proof: each helper's `--help`/usage text is accurate; the schema
  JSON mirrors the documented schema section-by-section; this spec is
  date-stamped.

Script tests that need a Cargo invocation (regen-lock) use `cargo update
--offline` in a scratch fixture or mock the Cargo step — never a real workspace
lock update and never a test suite in the dev loop.

## forbids

- No production release: no tags, no pushes, no `gh release`, no `--clobber`,
  no asset mutation, no workflow runs, no `release-gate`, no radix ladder runs.
- No `cargo build`/`cargo test`/`cargo nextest run` suites in the dev loop;
  `regen-lock` tests use `--offline` scratch fixtures or mocks only (Cargo
  discipline; one closeout `--check` from faber is the only workspace touch).
- No edits to the three `release.yml` workflows (CI thinning is Stage 8) and no
  GHA changes of any kind.
- No edits to the Stage-1 seven release docs, `policy.md`,
  `process-versioning-and-deps.md`, or `v*.md`.
- No writes to `faberlang.dev/`, `hosts/`, `faber-runtime/`, `examples/`,
  `norma/`, `triga/`.
- No SaaS, no external services, no credentials stored or used; the private
  radix path is only ever invoked by the operator, and helpers never print
  private paths to public-adjacent output.
- No worktree creation/disposal (operator-managed); the doctor's worktree state
  is advisory.
- No pre-writing of Stage 4 packaging (provenance/signatures/SBOM), Stage 6
  publish/readback, or Stage 8 CI — those are later stages.
- No verification loops: one closeout pass, then done.

## risks

- **Onboarding interlock (primary):** `validate-release-manifest` must accept
  the faber-onboarding Stage-2 manifest instance, and `release-doctor` /
  `package-archive` must consume the onboarding `assemble-dev-kit` payload
  receipt/staging dir. Both deliveries cite `release-manifest-schema.md`
  §3/§4/§6; if they land in either order, the schema JSON and the instance must
  agree. The onboarding instance is the first real test fixture for the
  validator.
- **Schema drift from the Stage-1 decision:** the machine JSON must mirror the
  documented schema exactly; the internal proof (section-by-section mapping) is
  the guard, not the prose doc.
- **Scripts against a private radix tree:** `bump-version` and the radix
  helpers run against private source by the operator; helpers must not embed or
  emit private paths in receipts/logs (leakage gate,
  `process-local-first.md` §4).
- **`cargo update` index fetch:** regen-lock documents the `--offline` caveat
  (stage0-baseline §4.1) and its tests never hit the network or a real
  lockfile.
- **Cista has no scripta dir:** creating `cista/scripta/` under the shared
  convention is in-scope; keep it minimal (one helper + its test).
- **F2 lockfile mistakes:** regen-lock + doctor's freshness check directly
  target the observed failure mode; the test fixtures include a stale-lock
  negative.
- **Foreign WIP** in faber `src/package/device/*` — untouched.
- **Procedure-vs-script lag:** the Stage-2 rehearsal procedure documents
  hand-followable steps; these scripts automate them — a scripted step that
  diverges from the procedure is a defect, not an improvement (record any
  divergence in the unit reply for the Stage-2 doc owner).

---

## Suggested closeout evidence

All helpers + tests committed with their passing test output; the schema JSON
mapped to `release-manifest-schema.md` §2–§7; `validate-release-manifest`
accepting the onboarding instance shape; faber `--check` closeout passed once;
README regenerated and audit clean; unit reply cites D1–D10 by letter and names
the onboarding interlock fixtures used.
