# Delivery — Faber Onboarding Stage 1: Dev Kit And Distribution Contract

**Status**: ready — single decision-contract Hand unit; docs only, no product code
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 1 of 10
**Unit title**: `faber-onboarding-stage1-distribution-contract`
**Control plane**: `/Users/ianzepp/work/faberlang/faber`
**Owners**: faber control plane, with cista/release/library reviewers (read-only survey; no writes outside this campaign dir)
**Created**: 2026-08-07
**Source of truth**: the Stage 1 section, Product Definition (incl. the six provisional product choices), Open Questions, the three gate decisions, and the Stage-0 handoff (golden-path-inventory.md §8 D7, lie list §5 G1–G12, evidence E1–E20). This spec pins the delivery contract; it does not reopen campaign decisions.

---

## Outcome

Accepted decision records that resolve every Stage-0 handoff item into a
distribution contract a later planner can build against without choosing
architecture. The unit produces **decision records only** — it routes G1–G6
fixes to owners, it does not implement any of them.

Decisions made (each `accepted` | `explicitly-deferred-with-owner` | `routed`
— no silent default):

1. **Council-4 routing authority for `faber/docs/release/`** — the first
   decision item (see interlock below). Must be recorded before any other
   decision because it governs where this campaign's later-stage artifacts
   land.
2. The **9 Open Questions** (OQ1–OQ9) resolved per the §8 handoff markings.
3. The **3 gate decisions** finalized (D4 below).
4. The **distribution contract**: which owner repo fixes each blocking gap
   (G1–G6) at which target stage/unit, and which routes become needs/mail to
   sibling campaigns.

**Gate:** another planner can specify the canonical payload without choosing
architecture. Absolute developer paths are not a portable lock design. Norma
and Triga have distinct stated lifecycles. Upgrade, repair, and uninstall have
owners. The macOS-native channel is selected or explicitly deferred.

## write_scope

Five artifacts, **all under `faber/docs/factory/faber-onboarding/`**:

- **`dev-kit-contract.md`** — required/optional payload and discovery rules
  (OQ1). The four dev-kit layers with deterministic locations, no hidden env
  vars, a diagnostic naming missing/incompatible layers, and the
  core-vs-optional taxonomy. Every layer has an owner.
- **`install-channel-matrix.md`** — every channel (prebuilt archive, verified
  curl bootstrap, Homebrew, future macOS `.pkg`/`.dmg`) with a
  primary/secondary/deferred disposition (gate decision 2 + OQ8),
  checksum/provenance/signing policy, prefix rules, and upgrade / repair /
  downgrade / uninstall owners.
- **`package-and-lock-contract.md`** — Norma model (OQ2), portable lock and
  restore identity + the command that materializes the locked closure (OQ3),
  Git/registry bootstrap source and pin rule (OQ4), dependency-graph placement
  (OQ6), and relocation / offline / update / integrity / compatibility
  semantics. Absolute developer paths are local receipts, not the portable
  lock design.
- **`platform-matrix.md`** — the supported platform slice (OQ9/G12) and the
  named clean-room profiles (campaign §"Named clean-room profiles"). Each
  platform row records shell, architecture, install prefix, required system
  tools, signature/checksum policy, default execution target, and
  unsupported/residual status.
- **`decision-ledger.md`** — the single ledger: council-4 routing-authority
  decision (first item), all 9 Open Questions with markings, the 3 gate
  decisions with dispositions, and the G1–G6 distribution-contract routing
  table with owner repo + target stage/unit + route type. G7–G12 residuals
  are routed or documented with owners.

**No writes to `faber/docs/release/`** — the dev-kit payload manifest is
Stage 2, and per council-4 it must be authored as a defined section of the
single release-manifest schema under the routing authority decided in this
stage (see interlock). This stage records the decision only.

Read-only references (cite, never modify):

- `golden-path-inventory.md` — E1–E20 evidence, G1–G12 lie list, §8 handoff.
- `CAMPAIGN.md` — provisional product choices (consequence of reopening any
  must be recorded).
- `faber/docs/factory/release-and-portable-default/delivery.md` — sibling:
  portable FHIR/FMIR default and no-rust clean-room gates (constrains the
  default-execution-target decision; this stage does not own that delivery).
- `faber/docs/release/policy.md` — normative version-lane/channel policy
  (odd = development, even = LTS); cited, not duplicated.
- `faber/docs/factory/component-release-streamline/{CAMPAIGN.md,delivery-stage1.md}` —
  sibling Stage-1 scope; the routing-authority wording must match (non-overlap
  check only; read-only).
- `cista/`, `norma/`, `triga/`, `examples/`, `faberlang.dev/`, `hosts/`,
  `radix/` — survey and cite only (store contract, package manifests, example
  locks, start-track pages).

Forbidden roots: `cista/`, `norma/`, `triga/`, `examples/`, `hosts/`,
`radix/`, `faberlang.dev/`, `faber/docs/release/` — survey and cite only. No
new product code anywhere.

## done_when

Objective decision completeness — every item below must be satisfied:

- **D1.** The five artifacts (`dev-kit-contract.md`, `install-channel-matrix.md`,
  `package-and-lock-contract.md`, `platform-matrix.md`, `decision-ledger.md`)
  are committed under this campaign dir.
- **D2.** The **council-4 routing-authority decision is the first item** in
  `decision-ledger.md`: the single routing authority for `faber/docs/release/`
  is named (default: the coordinated product release process contract —
  `component-release-streamline`'s `release-contract.md` +
  `release-manifest-schema.md`), the Stage-2 dev-kit payload manifest is
  defined as a section of that single schema (not a parallel document), and
  this stage writes nothing to `faber/docs/release/`. The wording matches the
  sibling campaign's Stage-1 ledger.
- **D3.** All 9 Open Questions are resolved, each marked
  `answered-by-evidence` | `decided` | `explicitly-deferred-with-owner` |
  `routed` with the §8 evidence cited. No silent default. OQ5's choice, OQ7's
  locale-parameterization question, and OQ9's slice are decided or explicitly
  deferred with an owner.
- **D4.** The 3 gate decisions are finalized: (1) **public registry NOT
  required** for the golden path — no blocking dependency on `cista-dev-registry`;
  an immutable verified bootstrap source is selected (OQ4); (2) **GitHub
  prebuilt archive is the primary channel**; Homebrew is non-authoritative;
  (3) **default execution target for newcomers** is finalized and consistent
  with `release-and-portable-default`'s portable gates and the site
  prerequisites page — no unreleased no-rust claim is made as current proof.
- **D5.** `dev-kit-contract.md` defines the canonical payload representation
  (OQ1): the four dev-kit layers, deterministic locations, discovery rules, a
  diagnostic that names missing layers, and a core-vs-optional taxonomy with
  an owner per layer.
- **D6.** `install-channel-matrix.md` lists every channel with
  primary/secondary/deferred disposition, checksum/provenance/signing policy,
  prefix rules, and named owners for upgrade, repair, downgrade, and uninstall.
  macOS `.pkg`/`.dmg` is selected or explicitly deferred (OQ8).
- **D7.** `package-and-lock-contract.md` decides the Norma model (OQ2),
  portable lock identity + restore command (OQ3), Git/registry bootstrap
  source + pin rule (OQ4), dependency-graph placement (OQ6), and
  relocation/offline/update/integrity/compatibility semantics. Absolute
  developer paths appear only as local receipts.
- **D8.** `platform-matrix.md` names the supported slice and the named
  clean-room profiles; every row records shell, arch, prefix, tools,
  signature/checksum policy, default execution target, and residual status.
  Windows and macOS Intel are named residuals unless a release artifact and
  clean-room worker exist.
- **D9.** The distribution contract (in the ledger) routes every blocking gap
  **G1–G6** to an owner repo + target stage/unit + route type (own stage vs
  need/mail to a sibling campaign). **G7–G12** residuals are routed or
  documented with owners. Owner cells are never blank. No `faber install
  triga`-style claim is routed as a supported path without an immutable
  verified acquisition source (Stop-if).
- **D10.** No product code; no writes outside this campaign dir; no
  `faber/docs/release/` writes; no Stage-2 payload manifest pre-written; no
  release execution.

## validation

Docs-only. At closeout, exactly **one** pass:

1. Regenerate the faber factory README (the campaign dir gains files, so the
   generated README's per-goal doc counts and bucket change):
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/generate-factory-readme.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`
   then `... --check`.
2. Goal-status audit, target **0 findings**:
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/audit-factory-goal-status.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`.
3. Internal proof: every decision row cites Stage-0 evidence (E#/G#) or a
   live file; every G1–G6 owner is repo + stage/unit, never blank; no
   `faber/docs/release/` path appears in the commit; the ledger is
   date-stamped.

No cargo builds, no ladder runs, no site build — this stage is docs-only.

## forbids

- No product code in `faber`/`cista`/`norma`/`triga`/`radix`/`hosts`.
- No edits outside this campaign dir (`cista`, `norma`, `triga`, `examples`,
  `hosts`, `radix`, `faberlang.dev` are read-only references).
- **No `faber/docs/release/` writes** — the routing authority decision and the
  single-schema rule govern the payload manifest's Stage-2 landing; this stage
  records the decision only.
- No Stage-2 artifact pre-written (payload manifest is Stage 2) and no
  `release-manifest-schema.md` content pre-written (sibling campaign owns the
  schema).
- No release execution: no tags, no pushes, no `gh release`, no asset
  mutation, no cargo builds, no `release-gate`, no ladder runs.
- No reopening of closed Cista phases; no blocking dependency on
  `cista-dev-registry`.
- No unreleased clean-room/no-rust claim presented as current proof (E4/E10
  discipline from Stage 0).
- No site rebuild, no `faberlang.dev` changes.
- No verification loops: one closeout pass, then done.

## risks

- **Reopening a provisional product choice without recording the consequence**
  — the campaign's six provisional choices are defaults, not decisions; any
  replacement must record why, with evidence.
- **Default-execution-target decision collides with
  `release-and-portable-default`** — released 1.4.0 runs via Cargo (E4/E10);
  the portable default exists on main, unreleased. If the decision cannot
  name a released execution path, the unit must route a need rather than
  fabricate a portable claim.
- **Overlap with `component-release-streamline` Stage 1** — both campaigns'
  Stage-1 outputs converge on `faber/docs/release/`. This stage writes nothing
  there and matches the sibling ledger's routing-authority wording (D2);
  the `platform-matrix.md` here (user platform slice) is distinct from the
  sibling's `platform-builder-matrix.md` (release production matrix) — the
  distinction is recorded, not merged.
- **Lock/lifecycle contract has no owner** — Stop-if: if the package/lock
  model or the core-vs-optional taxonomy cannot be owned, the unit routes the
  gap; it does not lower installer or library implementation around an
  unresolved contract.
- **Foreign WIP in faber `src/package/*` and `corpus/`** — uncommitted
  unrelated changes; do not touch (same note as Stage 0).
- **Site/CLI drift since the inventory date** — decisions cite the 2026-08-07
  evidence state and are date-stamped; re-verify claims that would change a
  decision.
- **Triga version drift** — live metadata `0.2.0` vs example locks `0.1.0` +
  absolute paths (E15): record as drift evidence constraining the bootstrap
  decision, do not fix examples.
- **Evidence overreach** — label a row `unknown` over asserting from stale
  docs.

## Council-4 interlock — routing authority is the FIRST decision item

The Stage-0 handoff (§8) records that `faber/docs/release/` is the named
landing spot for both this campaign's Stage-2 dev-kit payload manifest
(component, version, digest, compatibility bound, license, destination) and
`component-release-streamline`'s Stage-1 release manifest schema. **A single
routing authority for `faber/docs/release/` is a Stage-1 planning input that
must be decided before the outputs overlap** (council-4). It is therefore the
**first decision item** in this Stage-1 spec and in `decision-ledger.md`.

**Default (shared with the sibling Stage-1 spec, so the two ledgers agree):**
`faber/docs/release/` is governed by a **single routing authority — the
coordinated product release process contract** (`component-release-streamline`'s
`release-contract.md` + `release-manifest-schema.md`). The faber-onboarding
dev-kit payload manifest (Stage 2) is authored as the **"dev-kit payload
section" of that single schema** (release-owned packs: launcher, core support,
reference/locale packs, libraries), never as a parallel document. Onboarding
Stage 1 writes nothing to `faber/docs/release/`; its payload-shape decisions
(`dev-kit-contract.md`, `package-and-lock-contract.md`) define **content**
only, which Stage 2 encodes into the shared schema under the process contract's
review.

The ledger records the accepted decision, and the two campaigns' Stage-1
outputs are non-overlapping by construction: this stage = decision records in
this campaign dir; sibling Stage 1 = the five release docs + process doc +
dry-run recipe under `faber/docs/release/`.

---

## Suggested closeout evidence

Five artifacts committed under the campaign dir; README regenerated and audit
clean; unit reply cites D1–D10 by letter and names the routing-authority
decision wording used.
