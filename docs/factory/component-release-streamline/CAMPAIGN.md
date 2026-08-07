# Campaign: Component Release Streamline (Faber · Radix · Cista)

**Status**: planned — draft/maintain complete; ready for Stage 0 inventory delivery
**Created**: 2026-08-07
**Mode**: routing artifact — does not implement code or perform releases
**Control-plane location**: `/Users/ianzepp/work/faberlang/docs/campaigns/component-release-streamline/`
**Product surface**: `faber` (language/product release)
**Component surfaces**: `radix`, `cista` (own versions; pinned by Faber when product ships)
**Related repos**: `faber-runtime`, `hosts` (assembled into Faber via
`core-support-manifest.txt`; not independent public release campaigns unless
stages invent one)
**Slug**: `component-release-streamline`
**Audience**: operator / release engineer / future automation (agent or human)

---

## Summary

Make **shipping Faber, Radix, and Cista regular and boring**: one clear process,
dry-runnable in **isolated worktrees**, with as much as possible done **locally**
(build, gate, package, checksum, version/tag bookkeeping) so a release does not
depend on GitHub Actions queues, flaky runners, or publish timeouts.

Today each repo has **partial** protocol text (AGENTS.md) and **tag-triggered
GitHub release workflows** that push artifacts toward `faberlang/releases`.
Local helpers exist for gates (`faber/scripta/release-gate`, radix ladder) but
there is **no single end-to-end dry-run**, no multi-repo pin matrix that is
exercised offline, and no operator checklist that is known-good without CI.

This campaign **defines and then implements** that structure. It does not
perform a production release by itself.

```text
Inventory live protocols + scripts + GHA
  → Write one RELEASE process (local-first)
  → Dry-run in worktrees (no public tag/push required)
  → Script gaps (bump, pin, package, checksum, notes)
  → Optional: schedule / agent-assisted regular release
  → GHA reduced to optional publish mirror (or replaced)
```

## Problem

| Friction | Evidence (2026-08-07) |
| --- | --- |
| Protocols are prose, not dry-runnable | `faber/AGENTS.md`, `radix/AGENTS.md` list bump → lock → build → gate → tag → push → **monitor CI** |
| Radix deliberately has no local release helper | AGENTS: release automation “belongs in GitHub workflows” so “release” means public artifacts — fights local-first preference |
| Faber product release is multi-repo | `release.yml` checks out radix (private token), cista, faber-runtime, hosts; `core-support-manifest.txt` pins sibling paths, not necessarily git SHAs |
| Gates are expensive and uneven | Faber: `./scripta/release-gate` (full nextest); Radix: tag runs `./scripta/test --full`; Cista: workflow-centric |
| Artifact truth lives on GitHub | Workflows target `RELEASE_REPO: faberlang/releases`; site install docs point at those assets |
| GHA reliability is a hard constraint | Matrix already dropped macos-13 Intel citing unbounded queue; operator reports timeouts / jobs never leaving queue / actions not firing |
| Related work is siloed | `faber/docs/factory/release-and-portable-default` owns portable **product content** of a release, not the multi-component **process** |
| No worktree dry-run | Factory worktree convention exists for feature packets; not wired as a release rehearsal lane |

Rough process means every release is tribal knowledge, high stress, and blocked
when GitHub is unhealthy — exactly when you want a local path.

## Desired End State

1. **One process document** (or small set) an operator can follow cold:
   version rules, order of component vs product tags, pin matrix, gates,
   artifact layout, publish steps, rollback/abort.
2. **Local-first path**: version bump, lockfile, build, gate, archive,
   checksums, and release notes can complete **without** GitHub Actions
   succeeding. Public upload may still use `gh`/API or a non-GHA host, but
   the **proof of releasability** is local.
3. **Worktree dry-run**: a documented recipe (and eventually a script) that
   creates or reuses worktrees for `faber` / `radix` / `cista` (and any
   required siblings), runs the process up to “would tag / would upload,”
   and leaves main untouched.
4. **Scripts fill gaps** where prose is error-prone (bulk radix version bump,
   lock regenerate, pin file generation, archive+checksum, “release doctor”
   preflight). Prefer few scripts over a second platform.
5. **GHA is optional or thin**: either (a) local artifacts + manual/scripted
   publish to `faberlang/releases` (or successor), or (b) GHA only mirrors
   already-built local artifacts. Tag-push-as-only-build-path is retired as
   the sole happy path.
6. **Cadence-ready**: process is short enough for regular releases; further
   full automation is a later stage, not a prerequisite for first regular
   manual release.

**Not** required for campaign closeout: fully unattended weekly releases,
replacing Git hosting, or finishing portable-default product content
(that remains `release-and-portable-default`).

## Development Posture

- **Process before automation.** Document and dry-run first; script second;
  schedule last.
- **Local proof is authority.** A release is “ready” when local gates and
  local packages pass; CI is corroboration or publish, not the only truth.
- **Worktrees for rehearsal.** Never dry-run version bumps on shared main
  without isolation; follow factory worktree discipline
  (`radix/docs/factory/worktree-convention.md` spirit: operator-managed
  packets under `faberlang/worktrees/`).
- **Three surfaces, one pin story.** Radix and Cista may tag independently;
  a **Faber product** release always records exact radix/cista/(runtime/hosts)
  revisions used.
- **Do not invent a fourth product.** faber-runtime and hosts ride Faber
  assembly unless a stage proves they need public tags.
- **Prefer stdlib / shell / existing scripta** over new SaaS.
- **No production tag from a half-finished stage.** Dry-run stages stop
  before `git push --tags` unless the operator explicitly authorizes a real
  release outside this campaign’s default path.
- **Honesty about remaining cloud steps.** If something must hit the network
  (upload, private radix checkout on a builder), name it; do not pretend
  full air-gap if private deps exist.

## Implementation Workflow

1. Stage 0 inventories current AGENTS protocols, workflows, scripts, and
   pain points — no new automation yet.
2. Stage 1 writes the process (RELEASE.md or equivalent) and the worktree
   dry-run procedure as **docs only**.
3. Later stages lower to delivery specs in `faber`, `radix`, `cista` (and
   container docs) for scripts and optional CI changes.
4. Real production releases remain operator-authorized; campaign stages
   produce dry-run evidence by default.

## Scope Routing

| In this campaign | Out of scope |
| --- | --- |
| Multi-repo release **process** and dry-run | Language features, parity campaigns |
| Local build/gate/package/checksum/tag bookkeeping | Fixing all GHA platform bugs at GitHub |
| Worktree rehearsal layout | Day-to-day factory feature worktrees |
| Thinning or replacing tag-only GHA build | Mandatory public registry (`cista.dev`) |
| Pin matrix for Faber product | Portable FHIR defaults content (sibling delivery) |
| Optional scheduled/agent release later | Marketing/site copy except “how we release” notes |

### Related artifacts (references, not subordinates)

| Artifact | Role |
| --- | --- |
| `faber/AGENTS.md` — Release protocol | Current product steps |
| `radix/AGENTS.md` — Release protocol | Current component steps; local-helper stance to revisit |
| `cista/.github/workflows/release.yml` + `cista/docs/release/` | Current component publish |
| `faber/.github/workflows/release.yml`, `radix/.github/workflows/release.yml` | Tag → multi-platform build → `faberlang/releases` |
| `faber/scripta/release-gate` | Expensive product gate (release-only) |
| `radix/scripta/test` ladder | Component validation / tag full suite |
| `faber/docs/factory/release-and-portable-default/delivery.md` | **What** a good Faber archive must contain (portable first-run); this campaign owns **how** components ship |
| `faber/docs/factory/faber-onboarding/CAMPAIGN.md` | Consumes released archives; does not define ship process |
| `radix/docs/factory/worktree-convention.md` | Packet/worktree ops model to reuse for release rehearsal |

## Batching And Split Policy

- **discovery-first** for Stage 0.
- **docs-first** for Stage 1 (process + dry-run procedure).
- **split-on-boundary** by repo ownership when implementing scripts
  (`faber` vs `radix` vs `cista` vs container docs).
- **batch-by-default** only for mechanical multi-repo version bumps once one
  script pattern is proven.
- Do not batch “fully automated weekly release” into the first dry-run stage.

## Ground Truth Researched (2026-08-07)

- **Faber protocol:** bump `Cargo.toml` → `cargo update` → locked release build
  → `./scripta/release-gate --locked-release-build` → single bump+lock commit →
  `vX.Y.Z` tag → push main+tag → monitor GHA.
- **Radix protocol:** bulk `crates/*/Cargo.toml` bump → update lock → locked
  build → nextest → single commit → tag → push; main CI stage 1–4, tag full.
  Explicit: no local `scripta/release`.
- **Cista:** tag/workflow_dispatch → matrix build (linux + two macOS) →
  artifacts to `faberlang/releases` as `cista-vX.Y.Z`; notes under
  `cista/docs/release/`.
- **Faber GHA:** needs private radix token + sibling checkouts; matrix is
  linux x64 + macos-14 arm64 (Intel host dropped for queue reasons).
- **Portable release content** is a separate planning delivery; process
  streamline must not wait on it, but Faber product dry-run should eventually
  call into its gates when they exist.

## Current State

| Track | State | Next action |
| --- | --- | --- |
| Written per-repo protocols | Present in AGENTS / workflows | Stage 0 unified inventory |
| Local end-to-end dry-run | Missing | Stage 1–2 |
| Worktree release packet | Not defined | Stage 1 |
| Local archive + checksum | Partial / CI-owned | Stage 3 |
| Pin matrix file (product) | Implicit via checkouts | Stage 3 |
| GHA optional path | Not designed | Stage 4 |
| Cadence / automation | None | Stage 5 |

## Campaign Path

### Stage 0 — Inventory: Protocols, Scripts, Workflows, Pain

**Status**: selected; ready for `delivery`  
**Lowers to**: delivery (docs only under this campaign dir or linked)  
**Batching**: discovery-first  
**Owners**: container docs + read-only survey of faber/radix/cista

**Outcome:** A single inventory of:

- Step lists from AGENTS and workflows (side-by-side table).
- Every local script that is release-adjacent (`release-gate`, radix ladder,
  any cista scripts).
- What **must** hit the network today (private clone, artifact upload, `gh`).
- What can already run offline on `burgus` / `pharos`.
- Failure modes observed (GHA queue, lockfile mistakes, sibling path drift).
- Open decisions for Stage 1 (see Open Questions).

**Gate:** Inventory committed; no product code required; explicit “local-first
vs GHA-required” column per step.

---

### Stage 1 — RELEASE Process Document + Worktree Dry-Run Spec

**Status**: planned; depends on Stage 0  
**Lowers to**: delivery (documentation)  
**Batching**: docs-first  

**Outcome:**

1. **`RELEASE.md`** (name flexible) at a single discoverable root — recommend
   `faberlang/docs/release/RELEASE.md` or per-repo `docs/release/PROCESS.md`
   with a top-level index. Content: version policy, order of operations
   (component-only vs product), pin matrix, gates, artifact naming, abort
   rules, who may tag.
2. **Worktree dry-run spec**: directory layout under
   `faberlang/worktrees/release-rehearsal-<date-or-slug>/` (or similar),
   which remotes/branches, which commands, **stop before public push**,
   how to dispose of the packet.
3. **Local-first principle** written as invariant: proof of releasability does
   not require Actions green.

**Gate:** An operator (or agent) can rehearse the process on paper without
asking chat; dry-run stop conditions are unambiguous.

**Stop if:** Version identity rules for multi-repo product cannot be decided
(file need to operator with a default: Faber product version independent;
components free; product records pins).

---

### Stage 2 — First Worktree End-to-End Dry-Run (Evidence)

**Status**: planned; depends on Stage 1  
**Lowers to**: factory / operator session (execution of dry-run; minimal code)  
**Batching**: discovery-first  

**Outcome:** At least one full dry-run for **one** component surface and one
**product-shaped** path (Faber with pinned siblings), in worktrees, producing
a receipt: commands run, gate results, would-be tag names, artifact paths,
time taken, GHA steps skipped.

**Gate:**

- Receipt committed under this campaign dir (`receipts/` or delivery closeout).
- Main branches clean; no accidental public tags from the dry-run.
- List of script gaps filed for Stage 3 (mechanical only).

**Authorization:** Creating worktrees and running expensive gates needs
operator intent; this stage is not silent agent background work.

---

### Stage 3 — Scripts And Checklists To Close Gaps

**Status**: planned; depends on Stage 2  
**Lowers to**: delivery → factory per repo  
**Batching**: split-on-boundary (faber / radix / cista / container)

**Outcome:** Small scripts or checklist automation for high-error steps, e.g.:

- Preflight doctor (clean tree, tool versions, sibling paths present).
- Version bump helpers (especially radix multi-crate).
- Pin manifest generation for Faber product (git SHAs of radix/cista/hosts/…).
- Local package: tar/zip + sha256 + notes template.
- Optional: “dry-run release” driver that orchestrates worktree steps.

Prefer extending `scripta/` over new frameworks. Update AGENTS release
sections to point at the scripts and local-first path (including reversing
radix’s “GHA-only release automation” claim if Stage 1 decided local-first).

**Gate:** Stage 2 dry-run re-run is shorter or less error-prone; docs match
scripts; no required GHA for dry-run success.

---

### Stage 4 — Publish Path Without Depending On GHA Build

**Status**: planned; depends on Stage 3  
**Lowers to**: delivery → factory  
**Batching**: split-on-boundary (upload host vs workflow edit)

**Outcome:** A supported path:

```text
local build + gate + package + checksum
  → (optional) upload artifacts to faberlang/releases or alternate host
  → tag/push source repos for history
```

GitHub Actions either:

- **A)** builds nothing required (mirror/attach only), or  
- **B)** remains a fallback builder explicitly labeled secondary, or  
- **C)** is removed from the critical path with operator-approved residual.

**Gate:** One rehearsal upload of **non-production** or carefully versioned
artifacts without waiting on matrix queue — or documented equivalent on
pharos/local hosting if releases repo is optional for rehearsal.

**Stop if:** Private radix build **cannot** be done on operator machines —
then Stage 4 documents the minimum remote builder (pharos, self-hosted runner)
instead of public GHA.

---

### Stage 5 — Regular Cadence And Optional Automation

**Status**: planned; depends on Stages 1–4  
**Lowers to**: delivery  
**Batching**: discovery-first  

**Outcome:** Written cadence options (e.g. manual monthly, agent-assisted
checklist, fully automated) with prerequisites and kill switches. Optional:
scheduled local job or fleet/Mind task that runs dry-run only.

**Gate:** Operator can choose a cadence without redesigning process; full
automation is optional, not blocking “regular releases.”

---

### Stage 6 — Closeout And Protocol Authority

**Status**: planned; depends on Stages 1–5 as selected  
**Lowers to**: delivery (docs)  

**Outcome:** AGENTS.md release sections in faber/radix/(cista) and container
RELEASE index all point to the same authority. Campaign status → done or
parked with residual list. Onboarding campaign can cite the process for
“where binaries come from.”

**Gate:** Poker-face: a cold operator follows RELEASE.md once successfully
(dry-run or real, operator-chosen).

## Dependency Rules

| Situation | Route |
| --- | --- |
| Portable archive **content** (reference packs, no-Rust defaults) | `release-and-portable-default` — process campaign records pins/gates only |
| Site install version strings | Onboarding / faberlang.dev — update after real release, not inside dry-run |
| Feature work blocking green gates | Own factory campaigns; release dry-run may residual “gate red on main” |
| Need private token for GHA | Prefer Stage 4 local/pharos path; do not expand GHA secrets as goal |
| Operator wants real tag during campaign | Explicit human authorization; not default stage gate |
| Worktree path conflicts | Operator packet lifecycle; do not auto-prune foreign worktrees |

## First Useful Milestones

1. Stage 0 inventory table (local vs network per step).  
2. Stage 1 RELEASE.md + worktree dry-run recipe.  
3. Stage 2 receipt: one successful isolated dry-run.  
4. Stage 3 scripts: bump/pin/package doctor.  
5. Stage 4: ship path that survives GHA being down.

## Acceptance Criteria (This Artifact)

- Next stage identified: **Stage 0**.
- Local-first and worktree dry-run are explicit.
- Faber / Radix / Cista all in scope with clear product vs component roles.
- Related portable-default and onboarding campaigns referenced, not absorbed.
- Stop conditions prevent accidental public tags and GHA-only thinking.
- Ready for Stage 0 delivery without solving automation in this session.

## Validation

| Layer | How |
| --- | --- |
| Artifact | Parseable Status; stages ordered |
| Stage 0–1 | Docs committed; checklist complete |
| Stage 2+ | Receipts with commands and outcomes |
| Scripts | Dry-run re-run without GHA build |

## Open Questions

1. **Single RELEASE.md root** — container `docs/release/` vs per-repo only?  
2. **Version coupling** — independent SemVer always, or lockstep radix/faber?  
3. **Artifact host if not GitHub** — still `faberlang/releases` via local `gh`,
   or pharos/object storage?  
4. **Who may cut production tags** — operator only vs agent with policy?  
5. **Minimum platforms for “release”** — match current matrix (linux + mac
   arm64) or expand?  
6. **Pharos as builder** — in-scope for Stage 4 or residual?

**Defaults if unanswered by Stage 1:** (1) container index + per-repo thin
protocol; (2) independent versions + product pin file; (3) keep
`faberlang/releases` via local upload first; (4) operator-only production tags;
(5) linux x64 + macos arm64; (6) document pharos option, implement if Stage 0
says local mac/linux insufficient for private radix.

## Stop Conditions

- Do not push production tags or overwrite release assets without explicit
  operator authorization.
- Do not require GitHub Actions green as the sole Stage 2 success criterion.
- Do not merge release bump commits to main from dry-run worktrees without a
  separate accept/merge decision.
- Do not expand scope into language features or onboarding content.
- Pause if private radix cannot be built anywhere the operator controls — route
  a need for builder placement before claiming local-first complete.

---

## Suggested Stage 0 Delivery Title

`component-release-streamline-stage0-inventory` — side-by-side protocol table,
script list, network dependencies, GHA failure modes, open decisions; pure
documentation under this campaign directory.
