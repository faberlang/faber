# Delivery — Faber Onboarding Stage 0: Golden Path Inventory And Lie List

**Status**: ready — single discovery-first Hand unit; docs only, no product code
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 0 of 10
**Unit title**: `faber-onboarding-stage0-golden-path-inventory`
**Control plane**: `/Users/ianzepp/work/faberlang/faber`
**Owners**: faber (control plane) + read-only survey of `cista`, `faberlang.dev`, `norma`, `triga`, `examples`, optional `radix` (locale packs / release assembly only)
**Created**: 2026-08-07
**Source of truth**: the Stage 0 section, Desired End State, Problem table, Product Definition, and Ground Truth Researched sections of `CAMPAIGN.md`. This spec pins the delivery contract; it does not reopen campaign decisions.

---

## Outcome

One written golden path — a step-numbered sequence of commands with expected
outcomes from "opened install page" to "ran a package that imports Norma",
plus a second optional branch "imports Triga" — with every current lie,
monorepo assumption, missing binary, and locale dead end cataloged with an
owner. The inventory is the evidence input that Stage 1 distribution-contract
decisions consume. Stage 0 decides nothing; it records facts, gaps, and
routing.

The inventory must answer the campaign gate: is the path runnable by a cold
reader, with each step citing live file or command evidence (or `unknown`),
with an explicit split between "must work without monorepo" and
"developer-only", and with the open decisions listed?

## write_scope

Primary artifact (the only write outside evidence gathering):

- **`faber/docs/factory/faber-onboarding/golden-path-inventory.md`** — one
  markdown file: the step-numbered golden path, the desired-end-state coverage
  matrix, the lie list, the surface survey, the open-decisions handoff, and a
  dated evidence index. Single file recommended (discovery-first batching);
  splitting into evidence tables is allowed only if the file links them.

Read-only references (cite, never modify):

- `faberlang.dev/src/{en-US,zh-Hans,zh-Hant,ar,hi,vi,th-TH}/start/*.md` — site
  start track, all seven locale dirs; `commands.md`, `hello.md`,
  `examples.md`, `projects.md`, `index.md` (separate site repo; read-only).
- `faber/src/commands/{init,install,run}.rs` and the `explain` / doctor
  surfaces — CLI evidence citations (file or `file:line`).
- `cista` repo — store client contract, `$CISTAE_HOME` default, lock rewrite
  behavior (read-only).
- `norma/`, `triga/` — Cista source-package manifests and live package
  metadata (read-only).
- `examples/` — example locks as drift evidence (read-only; record drift, do
  not fix).
- `radix/` — locale/reference packs and release assembly only (read-only).
- `faber/docs/factory/release-and-portable-default/delivery.md` — sibling
  dependency for clean-room/portable default claims (read-only).
- GitHub releases artifacts for `faberlang/releases` — observation only;
  record names, versions, and digests as found.
- Live read-only commands for evidence (e.g. `faber --version`,
  `faber explain`, `faber --help`) are permitted. No build, no mutation.

Forbidden roots: `faberlang.dev/`, `cista/`, `norma/`, `triga/`, `examples/`,
`radix/` — survey and cite only. No new product code anywhere.

## done_when

Objective inventory completeness — every item below must be satisfied:

- **D1.** `golden-path-inventory.md` committed under this campaign dir.
- **D2.** Golden path is step-numbered and runnable by a cold reader, from
  "opened install page" through install → verify (`faber --version` + a tiny
  non-build check such as `faber explain`) → hello/init → `check` → `run` →
  Norma import, with an optional Triga branch. Every step has: command(s),
  expected outcome, live evidence (file path or command) or the literal
  `unknown`, and a tag `must-work-without-monorepo` vs `developer-only`.
- **D3.** Desired-end-state coverage matrix: each of the **6 Desired End State
  outcomes** (install+prove, hello check/run, project layout, Norma+Triga
  acquisition, multi-locale, honesty) has a **current-state row, a gap, and an
  owner**. Owner = owning repo + stage (e.g. "faber Stage 5", "cista /
  routed need", "faberlang.dev Stage 8") — never blank.
- **D4.** Lie list: every lie, monorepo assumption, missing binary, and locale
  dead end cataloged, each with owner and severity (blocks golden path vs
  residual).
- **D5.** Surface survey: all 6 Problem-table surfaces (website start track,
  CLI, cista store, Norma/Triga distribution, locales, release honesty) have a
  current-state row with live evidence. All seven site locale dirs are
  surveyed for the locales row.
- **D6.** Release-honesty row names what the current published archive
  actually contains (observed) vs what the Dev Kit product definition
  requires, labeled verified vs residual. No "works on my monorepo" claim is
  allowed as release-proof.
- **D7.** Open-decisions handoff: each of the campaign's 9 Open Questions is
  marked `answered-by-evidence` | `carried-to-stage-1` | `needs-stage-1-decision`.
  The three gate decisions are named explicitly: is a public registry
  required, which install channel is primary, what is the default execution
  target for newcomers. This list is the Stage 1 decision-input handoff.
- **D8.** Stop-if check recorded: whether the primary install channel could be
  named without inventing a release process. Expected result: yes (GitHub
  prebuilt archive primary; Homebrew explicitly non-authoritative), so no need
  is routed. If it could not be named, a need was routed to mind →
  `release-and-portable-default` **instead of fabricating an answer**.
- **D9.** No Stage 1 artifact is pre-written (decision records are Stage 1's).

## validation

Docs-only. At closeout, exactly **one** pass:

1. Regenerate the faber factory README (the campaign dir gains files, so the
   generated README's per-goal doc counts change):
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/generate-factory-readme.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`
   then `... --check`.
2. Goal-status audit, target **0 findings**:
   `python3 /Users/ianzepp/work/faberlang/radix/scripta/audit-factory-goal-status.py --factory-root /Users/ianzepp/work/faberlang/faber/docs/factory`.
3. Internal proof: every inventory row cites live evidence or `unknown`;
   step commands are verified to exist on the CLI (zombie-docs rule); the
   inventory is date-stamped.

No cargo builds, no ladder runs, no site build — this stage is docs-only.

## forbids

- No product code in `faber`/`cista`/`norma`/`triga`/`radix`.
- No edits outside this campaign dir (`faberlang.dev`, `cista`, `norma`,
  `triga`, `examples`, `radix` are read-only references).
- No site rebuild, no `faberlang.dev` changes.
- No release execution: no tags, no pushes, no `gh release`, no asset
  mutation, no `cargo build`, no `release-gate`, no ladder runs.
- No pre-writing of Stage 1 contracts (dev-kit, install-channel matrix,
  package-and-lock contract, platform matrix, decision ledger).
- No claim "this works on my monorepo" as release-ready proof.
- No second unlinked inventory; `golden-path-inventory.md` is the single
  artifact.
- No ambient credentials or private tokens; radix is read-only reference.
- No verification loops: one closeout pass, then done.

## risks

- **Site/CLI drift during inventory** — every row date-stamped and cited to
  the exact file (or `file:line`) observed at inventory time; never prose
  alone.
- **Sibling campaigns move the facts** — `component-release-streamline` and
  `release-and-portable-default` change release facts; record observations at
  inventory date and note the interlock (below), do not chase their WIP.
- **Uncommitted CAMPAIGN.md revision** — the faber working tree carries an
  uncommitted revision of `faber-onboarding/CAMPAIGN.md`; this spec follows
  the working-tree text. The executor cites repo files, not the WIP.
- **Foreign WIP in faber `src/package/*`** — uncommitted unrelated changes;
  do not touch.
- **Locale parity scope creep** — 7 site locales exist; survey all, fix none
  (fixing is Stage 8).
- **Triga version drift** — live metadata `0.2.0` vs examples/locks pinning
  `0.1.0` + absolute workspace paths; record as drift evidence, do not fix
  examples.
- **Evidence overreach** — prefer labeling a row `unknown` over asserting
  from stale docs.

## Council-4 interlock — Stage 1 planning input, NOT a Stage 0 requirement

`faber/docs/release/` is the durable home named by
`component-release-streamline` for the coordinated product release process and
manifest schema, and is the natural landing spot for this campaign's Stage 2
dev-kit payload manifest (machine-readable manifest of every component,
version, digest, compatibility bound, license, destination). Both campaigns'
Stage 1 decision outputs therefore converge on the same directory. **A single
routing authority for `faber/docs/release/` must be decided at the campaigns'
Stage 1 planning before their decision outputs overlap** (dev-kit payload
manifest vs release manifest schema) — per council-4, that decision is a
**Stage 1 planning input**, named in each campaign's Stage 1 decision ledger.

Stage 0's only duty is to record the overlap: list the `faber/docs/release/`
artifacts each campaign will produce at Stage 1, so the Stage 1 planner takes
the interlock into the council decision. Stage 0 must **not** pre-write shared
schemas or pre-empt the routing decision.

---

## Suggested closeout evidence

`golden-path-inventory.md` committed; README regenerated and audit clean;
unit reply cites D1–D9 by letter.
