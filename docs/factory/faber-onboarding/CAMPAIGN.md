# Campaign: Faber Onboarding And First-Run Experience

**Status**: planned — draft/maintain complete; ready for Stage 0 inventory delivery
**Created**: 2026-08-07
**Mode**: routing artifact — does not implement code directly
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Working repos** (as stages require): `faber`, `cista`, `faberlang.dev`,
`norma`, `triga`, `examples`, `hosts` (browser demos only), optional `radix`
(locale packs / release assembly only — not a user-facing install step)
**Slug**: `faber-onboarding`
**Audience**: new human developers and agent installers discovering Faber from
the public website or social promotion

---

## Summary

Coordinate a **smooth path from public instructions to a working local Faber
setup**: install the CLI, create or open a project, check/build/run code, and
use the libraries people actually need (**Norma** standard library, **Triga**
graphics library), in **English and other site/code locales**.

This campaign owns **user/developer experience and honesty of the path**, not
the full package-registry product and not the compiler. Package storage,
registry transport, release packaging, and site generation are **references and
dependencies** — they supply pieces; this campaign decides what a newcomer must
succeed at, measures the gaps, and routes fixes to the right owner.

```text
Website (faberlang.dev start track)
  → install CLI (honest archive / formula)
  → verify (`faber --version`, explain)
  → first program (init / hello)
  → portable check / build / run
  → install or resolve Norma + Triga
  → multi-locale (docs + code/diagnostic locales)
  → “I can keep building” (projects, examples, failure UX)
```

## Problem

Promotion without a reliable first hour fails. Current reality (2026-08-07):

| Surface | What exists | Gap for newcomers |
| --- | --- | --- |
| **Website start track** | Multi-locale `start/install`, `hello`, examples, projects under `faberlang.dev/src/<locale>/start/` | Install docs can lag releases; container-verified install CI is residual; Homebrew may serve older compilers |
| **CLI** | `faber init`, `check`, `build`, `run`, `script`, `install`, locale flags | Portable no-Rust path is planned, not the universal default story; install UX is Cista-composition shaped |
| **Cista store** | Client contract closed (phases A–G); `$CISTAE_HOME` | Public `cista.dev` registry/server **not** required for local store; end-user “get Norma/Triga” story is incomplete |
| **Norma / Triga** | Source repos; developers use monorepo / `FABER_LIBRARY_HOME` | Released binary users do not have a one-command “install stdlib + Triga” path that is docs-verified |
| **Locales** | Site locales (en-US, zh-Hans, ar, …); code/diagnostic locales on CLI | Code-default English campaign is separate; onboarding must not assume Latin-only or monorepo layout |
| **Release honesty** | Prebuilt archives on GitHub releases | Clean-room first-run gate is planned under release-and-portable-default, not closed |

Without one campaign, install docs, CLI defaults, package install, library
distribution, and multi-locale wording drift independently — and every public
push re-exposes the same broken first hour.

## Desired End State

A person (or agent) who only has the **public website** and a normal machine
can, without a sibling monorepo checkout and without undocumented env vars:

1. **Install** a current `faber` binary from the documented primary path and
   prove it with `faber --version` (and a tiny non-build check such as
   `faber explain`).
2. **Hello** — create or paste a minimal package, then `check` and **run** it
   successfully on the **documented default** execution path.
3. **Project** — understand package layout (`faber.toml`, entry, lock) enough
   to open an example and modify it.
4. **Libraries** — install or otherwise obtain **Norma** (stdlib) and **Triga**
   through a **product command** (or documented equivalent) and successfully
   `importa` them in a package; fail closed with actionable errors if a
   library is missing.
5. **Locales** — follow the start track in at least **English + one other site
   locale**; run `check`/`run` with a non-default **diagnostic** and/or **code**
   locale without dead ends in docs.
6. **Honesty** — every command on the golden path is either container-verified
   against a clean room or explicitly labeled residual (never silent lie).

**Not** required for campaign closeout: full `cista.dev` public registry,
Homebrew as sole authority, graphics GPU host product completeness, or default
English code-locale migration (those remain sibling campaigns).

## Development Posture

- **UX first, ownership second.** Stages name user outcomes; implementations
  lower to `faber`, `cista`, `faberlang.dev`, `norma`, or `triga` as needed.
- **Clean room is truth.** Prefer temporary `HOME`, minimal `PATH`, no sibling
  repos, no Rust unless the stage explicitly selects a Rust backend.
- **Cista is a dependency, not the campaign.** Package-store client and future
  `cista.dev` registry are **references**. This campaign may *require* store
  behaviors or docs; it does not reopen closed client phases or replace the
  public-registry campaign.
- **Docs and product ship together.** Website install/hello pages and CLI
  defaults change in the same stage gate when either would otherwise lie.
- **Locale is first-class, not a late translation pass.** Site track parity and
  CLI locale flags are stage gates, not polish.
- **Fail closed, explain next.** Missing libraries, wrong install channel, or
  unsupported host must produce a message a newcomer can act on.
- **No monorepo assumption in public docs.** `FABER_LIBRARY_HOME` and sibling
  checkouts are developer-layout; end-user onboarding must not require them.

## Implementation Workflow

1. Stage 0 inventories the golden path and records lies (zombie-docs style).
2. Each later stage lowers to one or more delivery specs in the owning repo(s).
3. Factory implements only after delivery; website copy updates with product.
4. Stage gates prefer a **scripted clean-room walkthrough** over narrative alone.
5. Do not implement `cista.dev` server work here — file needs against
   `cista-dev-registry` if the path requires a public registry.

## Scope Routing

| In this campaign | Out of scope (own elsewhere) |
| --- | --- |
| Golden-path definition and measurement | Compiler feature work (radix factory goals) |
| Install channel honesty (archives, formula lag labels) | Building the `cista.dev` server/ops product |
| First package create/check/run UX | Full graphics/WebGPU product parity (Triga 80/HV) |
| Norma + Triga **acquisition** for newcomers | Norma/Triga API design and library completeness |
| Multi-locale start track + CLI locale first-run | Default English code-locale pivot (`default-en-locale`) as a whole |
| Coordination with release clean-room gate | Full release versioning protocol (rides release-and-portable-default) |
| Agent-facing install skill honesty | Marketing posts / social calendars |

### Reference campaigns and goals (not subordinates)

| Artifact | Role relative to this campaign |
| --- | --- |
| [`cista/docs/factory/cista-package-store/goal.md`](../../../cista/docs/factory/cista-package-store/goal.md) | **Reference** — closed client/store contract; live `cista.dev` proof operator-gated |
| [`cista/docs/factory/cista-dev-registry/CAMPAIGN.md`](../../../cista/docs/factory/cista-dev-registry/CAMPAIGN.md) | **Reference** — future public registry/server; do not block first-run on full registry if a local/git/path install path works |
| [`release-and-portable-default/delivery.md`](../release-and-portable-default/delivery.md) | **Sibling dependency** — clean-room release archive, no-Rust portable `init/check/build/run` |
| [`faberlang.dev` site-implementation](../../../faberlang.dev/docs/factory/site-implementation/CAMPAIGN.md) | **Sibling** — site generator and start track; residual container-verified install CI |
| [`radix/.../default-en-locale`](../../../radix/docs/factory/default-en-locale/CAMPAIGN.md) | **Related** — default code locale `en`; onboarding may document both Latin and English surfaces without owning the pivot |
| Triga Hello Voxel / Three.js 80 | **Consumer pressure** — graphics demos after Triga is installable; not onboarding stage 0 |

## Batching And Split Policy

- **discovery-first** for Stage 0 (single inventory + lie list).
- **split-on-boundary** when ownership crosses `faber` CLI vs `cista` store vs
  website vs library packaging.
- **batch-by-default** for multi-locale **doc** parity once English golden path
  is locked (same structure, locale-specific wording).
- Do not batch “implement public registry” into library install stages.

## Ground Truth Researched (2026-08-07)

Inspected:

- `faberlang.dev/src/en-US/start/install.md` — prebuilt archives for Faber
  1.4.0; Homebrew explicitly non-authoritative / lagging; first package check
  clones `examples` and runs `faber check`.
- `faberlang.dev/src/*/start/` — parallel start tracks in en-US, zh-Hans,
  zh-Hant, ar, hi, vi, th-TH.
- `faber/src/commands/{init,install,run}.rs` — `init` scaffolds Latin hello;
  `install` composes Cista store (path / registry pin / git with `cista.toml`);
  `run` dual-path interpret vs compile.
- `cista` factory README — package-store client closed; `cista.dev` draft
  campaign separate; no unblocked live registry implementation unit.
- `release-and-portable-default` — planning delivery for clean-room portable
  default path (FHIR/FMIR); explicitly defers public `cista.dev` server.
- `CONTENT-PLAN.md` (site) — install honesty and container-verified quickstart
  called out as known risks.

## Current State

| Track | State | Next action |
| --- | --- | --- |
| Golden path definition | Implicit across start pages + READMEs | Stage 0 inventory |
| Clean-room install proof | Residual / planned (site + release delivery) | Align Stage 1 with release-and-portable-default |
| Hello / init UX | Works in monorepo; Latin default scaffold | Stage 2 |
| Norma acquisition (end user) | Dev layout / incomplete product install story | Stage 3 |
| Triga acquisition (end user) | Same | Stage 3 |
| Multi-locale docs | Site locales present; drift risk high | Stage 4 after English path locks |
| CLI locale first-run | Flags exist; docs thin | Stage 4 |
| Agent install skills | `static/agents` + skills exist | Stage 5 honesty pass |

## Campaign Path

### Stage 0 — Golden Path Inventory And Lie List

**Status**: selected; ready for `delivery`  
**Lowers to**: delivery  
**Batching**: discovery-first  
**Owners**: faber (control plane) + read-only survey of cista, faberlang.dev,
norma, triga, examples

**Outcome:** One written golden path (commands + expected outcomes) from
“opened install page” to “ran a package that imports Norma” and a second
optional branch “imports Triga.” Catalog every current lie, monorepo
assumption, missing binary, and locale dead end with owner.

**Gate:**

- Path is step-numbered and runnable by a cold reader.
- Each step cites live file or command evidence (or `unknown`).
- Explicit split: **must work without monorepo** vs **developer-only**.
- Open decisions listed (registry required or not; which install channel is
  primary; default execution target for newcomers).

**Stop if:** primary install channel cannot be named without inventing a
release process — then route a need to release-and-portable-default first.

---

### Stage 1 — Install Channel Honesty And Clean-Room CLI

**Status**: planned; depends on Stage 0  
**Lowers to**: delivery → factory (faber + faberlang.dev; release sibling)  
**Batching**: split-on-boundary (docs vs release packaging vs CLI)

**Outcome:** The documented primary install path installs a binary that matches
the documented version story. A clean-room script (or CI job) proves
`faber --version` and a minimal non-monorepo check without sibling repos.

**Gate:**

- Install page(s) match the artifact under test.
- Formula/manager lag is labeled or fixed — never preferred when stale.
- Clean-room proof is linked from the stage closeout (may reuse
  release-and-portable-default machinery).

**Overlap rule:** If release packaging is blocked, document residual and still
fix the **documented** channel; do not invent a second unofficial path in
marketing copy.

---

### Stage 2 — First Hour: Init, Check, Run

**Status**: planned; depends on Stage 1  
**Lowers to**: delivery → factory (faber + faberlang.dev)  
**Batching**: batch-by-default for hello/init docs once CLI defaults lock

**Outcome:** From install, a newcomer creates a package (`faber init` or
documented scaffold), checks it, and runs it on the **default product path**
without Cargo mental model or monorepo env.

**Gate:**

- `hello` / `projects` pages match actual CLI (no stale `faber.toml` shapes).
- Default run path is one story (direct run vs build-exec clearly split).
- Failure modes for missing entry/manifest are actionable.
- Clean-room walkthrough green for hello.

---

### Stage 3 — Libraries: Norma And Triga For Newcomers

**Status**: planned; depends on Stage 2  
**Lowers to**: delivery → factory (faber, cista, norma, triga, docs)  
**Batching**: split-on-boundary (packaging vs CLI vs docs); Norma and Triga may
parallelize after one install pattern exists

**Outcome:** Documented commands install or pin **Norma** and **Triga** for a
project that is **not** a monorepo checkout. A minimal package
`importa`s each library and `check`s (and runs where support exists).

**Gate:**

- One primary pattern (e.g. `faber install …` / lock / path pin) named on the
  site; monorepo `FABER_LIBRARY_HOME` is advanced-only.
- Missing library fails with install/next-step guidance.
- Triga graphics demos are **optional** follow-ons; Stage 3 only requires
  library resolution and compile/check, not WebGPU product closeout.
- References Cista store semantics without requiring `cista.dev` if git/path
  distribution is the interim honest path — residual must say so.

**Stop if:** no honest distribution channel exists for Norma/Triga without a
registry — file residual to package-store / release; do not document a fake
`cista.dev` fetch.

---

### Stage 4 — Multi-Locale Onboarding

**Status**: planned; depends on Stages 1–2 (Stage 3 library steps translated
when stable)  
**Lowers to**: delivery → factory (faberlang.dev + faber CLI docs/help)  
**Batching**: batch-by-default across site locales after English lock

**Outcome:** Start track is usable in **English and at least one additional
site locale** with the same golden path. CLI docs cover `--locale` /
`--diagnostic-locale` (and manifest `[locale]`) for first-run diagnosis.
Code examples either stay Latin with explanation or offer an English surface
per product policy — no silent mix.

**Gate:**

- Locale parity checklist: install, hello, library install steps.
- At least one non-English site locale passes the same clean-room commands
  (commands are language-agnostic; **copy and screenshots** are locale-owned).
- Diagnostic locale demo works in clean room.

**Overlap rule:** Do not absorb `default-en-locale` campaign; if English code
default flips later, re-verify this stage.

---

### Stage 5 — Agent And Human Install Surfaces

**Status**: planned; depends on Stages 1–3  
**Lowers to**: delivery → factory (faberlang.dev agents pages, skills, help)  
**Batching**: batch-by-default for skill/doc sync

**Outcome:** Agent-facing install instructions and human start track tell the
**same** golden path. Skills and `static/agents` do not recommend monorepo-only
steps as the default.

**Gate:**

- Diff of agent vs human install path is empty or explicitly dual-mode.
- One clean-room agent prompt/script succeeds against the primary path.

---

### Stage 6 — Continuous Honesty Gate

**Status**: planned; depends on Stages 1–4  
**Lowers to**: delivery → factory (CI in faber and/or faberlang.dev)  
**Batching**: discovery-first for CI placement

**Outcome:** A recurring clean-room job fails the build or a release gate when
install/hello/library steps regress. Optional: zombie-docs check on start pages
against live CLI help.

**Gate:**

- CI or release-gate hook documented and owned.
- On failure, residual names the lying page or command.

## Dependency Rules

| Situation | Route |
| --- | --- |
| Clean-room binary/assets missing | Stage 1 + `release-and-portable-default` |
| Public registry required for library install | Need/mail to `cista-dev-registry`; interim honest git/path path if possible |
| Package store bug (path safety, lock) | Fix in `cista` under package-store ownership; onboarding stage waits or documents residual |
| Compiler cannot run portable package | Radix/faber product bug — not an onboarding copy fix alone |
| Triga API incomplete | Triga campaigns; Stage 3 still requires **install + import** of what exists |
| Default code locale English | `default-en-locale`; Stage 4 documents current surface only |
| Marketing wants “one curl \| sh” | Only if Stage 1 gate owns that channel |

## First Useful Milestones

1. **Stage 0 closeout** — shared lie list; no more arguing about what the path is.
2. **Clean-room hello** — Stages 1–2 green without monorepo.
3. **Norma import without monorepo** — Stage 3 partial.
4. **Second site locale parity** — Stage 4 partial.
5. **CI honesty** — Stage 6.

## Acceptance Criteria (Campaign Artifact)

- Next stage to lower is identified (Stage 0).
- Cista campaigns are referenced without becoming this campaign’s backlog.
- Desired end state is user-outcome shaped, not “implement registry.”
- Multi-locale and library install are first-class stages.
- Stop conditions prevent fake docs and premature `cista.dev` dependency.
- Ready for **Stage 0 delivery** without further discovery meetings.

## Validation

| Layer | How |
| --- | --- |
| Artifact | Status line parseable; stages ordered; owners named |
| Stage 0 | Inventory doc committed under this directory or linked delivery |
| Stages 1–6 | Clean-room script or CI evidence per stage gate |
| Docs | zombie-docs style: commands on pages exist on CLI |

## Open Questions

1. **Primary install channel for promotion:** GitHub release archive only, or
   also Homebrew when version-matched?
2. **Norma/Triga distribution interim:** vendored release assets, git pins,
   path installs, or wait for `cista.dev`?
3. **Default newcomer execution:** always portable FHIR/FMIR, or keep Rust as
   default when present?
4. **Init language surface:** Latin `Salve, munde!` vs English scaffold vs
   locale-parameterized init?
5. **Windows:** explicit residual or Stage 1 platform?

Defaults if unanswered by Stage 0 closeout: (1) release archive primary;
(2) honest git/path or release-bundled libs until registry exists; (3) portable
path as documented default for newcomers; (4) document Latin scaffold + note;
(5) macOS arm64 + Linux x64 only until residual filed.

## Stop Conditions

- Pause if Stage 0 cannot name a primary install channel without inventing
  releases.
- Pause before Stage 3 if no honest Norma/Triga acquisition exists — do not
  publish “`faber install triga` works” without evidence.
- Pause if a stage would require production `cista.dev` credentials, DNS, or
  public upload — that is `cista-dev-registry` / ops, not this campaign.
- Pause if clean-room proof is replaced by “works on my monorepo.”

---

## Suggested Stage 0 Delivery Title

`faber-onboarding-stage0-golden-path-inventory` — single markdown inventory +
lie list + open decisions table; no product code required.
