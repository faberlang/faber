# Campaign: Faber Onboarding And First-Run Experience

**Status**: planned — Stage 0 inventory, then Stage 1 distribution-contract decisions
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

## Product Definition: What The Dev Kit Is

The campaign treats the **Faber dev kit** as an installed product contract, not
as a synonym for the `faber` executable. A conforming installation has four
layers whose ownership and lifecycle must be visible:

| Layer | Required content | Lifecycle rule |
| --- | --- | --- |
| **Launcher** | `faber` CLI and version metadata | Installed, upgraded, and removed as one versioned product |
| **Core support** | Runtime/host material needed by the documented default `check`/`run` path | Version-compatible with the launcher; no sibling checkout discovery |
| **Reference and locale packs** | Data required by `faber explain` and the documented code/diagnostic locales | Shipped and versioned with the launcher, or fetched by an explicit verified bootstrap step |
| **Faber libraries** | Norma as the selected platform/core library; optional packages such as Triga | Resolved through a declared package contract, not an ambient monorepo path |

“Install succeeded” therefore means more than copying one binary. The selected
install channel must put every required layer in a deterministic location,
teach Faber how to find it without hidden environment variables, and expose a
diagnostic that names missing or incompatible layers.

### Provisional product choices for planning

These are campaign defaults. Stage 1 must confirm or replace them with evidence;
planners should not reopen them without recording the consequence.

1. **Primary channel:** a versioned, checksummed Faber release bundle is the
   source of truth. `curl` may be a convenience installer for that bundle.
   Homebrew and a future native macOS installer are secondary presentations of
   the same payload and version, not independent products.
2. **macOS packaging:** start with a signed/notarized `.pkg` only if it adds real
   OS integration beyond the archive installer. A `.dmg` that merely asks the
   user to drag a CLI binary has no default value. Native packaging must not
   fork the payload layout or library model.
3. **Core vs optional:** Norma is part of the compatible platform set for a
   Faber release. Triga is an ordinary explicit dependency. Bundling Triga in a
   macOS installer must not make it implicitly importable or platform-specific.
4. **Third-party identity:** source imports keep stable provider coordinates
   such as `triga:geometria`; project manifests declare a package name and
   version; `faber.lock` records the exact resolved source, content identity,
   target/interface roots, and compatibility metadata.
5. **Acquisition before a public registry:** exact Git revision or verified
   release-asset installs are acceptable bootstrap channels. Mutable Git
   branches, absolute developer paths, and an unproven `cista.dev` endpoint are
   not acceptable golden-path lock sources.
6. **Execution:** the first-run default must work with the prerequisites named
   on the install page. If Cargo remains required for that path, the installer
   and `faber doctor` must say so before `run`; otherwise the selected portable
   path must prove that Cargo is absent.

## Experience Model And Supported Matrix

Stage planning must cover these users separately. Passing one row does not
imply the others pass.

| User | Starting state | Required first success |
| --- | --- | --- |
| **Learner** | Browser, shell, no Rust, no Faber repos | Install, diagnose, init, check, and run Hello |
| **Faber app developer** | Normal Git/toolchain access | Add a pinned library, reproduce from lock, work offline after fetch |
| **Faber library author** | Source package with `cista.toml` | Validate, install locally, and test as a project dependency |
| **Contributor** | Full sibling workspace | Use explicit developer overrides without leaking them into user docs or locks |
| **Agent installer** | Non-interactive shell | Verify checksum, install to an explicit prefix, emit machine-readable diagnostics, uninstall cleanly |

The first supported platform slice is provisionally **macOS arm64 and Linux
x86_64**. Each platform row records shell, architecture, install prefix,
required system tools, signature/checksum policy, default execution target,
and unsupported/residual status. Windows and macOS Intel remain named residuals
until a release artifact and clean-room worker exist.

## Installation And Package Contracts To Settle

Stage 1 must produce decision records for each contract below. “Use Cista” is
not itself an answer; Cista provides mechanisms while this campaign selects the
newcomer-facing behavior.

### Install bundle contract

- Exact bundle contents and a versioned on-disk layout for `bin/`, `share/`,
  core support, reference data, locales, receipts, and licenses.
- Prefix rules for system installs and unprivileged user installs.
- Shell/PATH changes, whether they are automatic, and how they are reversed.
- Checksums, provenance, code signing/notarization, TLS download source, and
  failure behavior before any fetched script executes a payload.
- Idempotent reinstall, upgrade, downgrade, side-by-side versions, and
  uninstall. User projects, locks, and third-party caches survive uninstall
  unless the user explicitly asks to remove them.
- Proxy, offline, and non-interactive behavior with stable exit codes.

### Library package contract

- Canonical package identity (`name`, exact version, source origin, immutable
  revision/content digest) and its relation to import provider coordinates.
- Required package contents: `cista.toml`, Faber interfaces/source, target
  artifacts or compile policy, license/provenance, and compatibility bounds.
- The rule for Norma: bundled platform package, seeded Cista package, or normal
  locked dependency. Pick one semantic model even if installers prefetch it.
- The rule for Triga and third parties: explicit `[dependencies]` entry plus a
  reproducible lock; installation must not grant ambient import access.
- Store ownership and discovery: `$CISTAE_HOME` may select a store, but project
  compilation consumes the lock contract. Absolute paths may be local install
  receipts; portable committed locks need a relocatable source/content identity
  or a deterministic re-resolution rule.
- Transitive dependencies, target variants, Faber compatibility, duplicate
  versions, update selection, yanks/revocation, cache garbage collection, and
  offline replay.
- Trust policy for Git and registry packages: immutable pins, checksum/signature
  verification, archive traversal/symlink defenses, and no install-time code
  execution unless a later explicit contract permits it.

### Product commands and diagnostics

Planning must decide whether these are `faber` commands, delegated `cista`
commands, or both with one canonical UX:

```text
faber doctor [--json]
faber install <package-source> [--project <path>]
faber update [<package>]
faber package inspect|list|remove ...
faber self update
faber self uninstall
```

Names are provisional. The required outcomes are not: inspect the installed
kit, explain prerequisites, add and lock a dependency, restore a project from
its lock, update intentionally, report provenance, and remove product-owned
files without deleting user work.

## Implementation Workflow

1. Stage 0 inventories the golden path and records lies (zombie-docs style).
2. Stage 1 selects the dev-kit, distribution, and library package contracts.
3. Each later stage lowers to one or more delivery specs in the owning repo(s).
4. Factory implements only after delivery; website copy updates with product.
5. Stage gates prefer a **scripted clean-room walkthrough** over narrative alone.
6. Do not implement `cista.dev` server work here — file needs against
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
| [`component-release-streamline/CAMPAIGN.md`](../component-release-streamline/CAMPAIGN.md) | **Sibling** — how Faber/Radix/Cista are cut and published; onboarding consumes the artifacts |
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

Material live findings planners must carry forward:

- The release archive currently contains only `faber` and `README.txt`. Locale
  packs, reference data, Norma, Triga, an installer receipt, and sibling-source
  provenance are absent. Core-support source is embedded separately inside the
  binary and materialized into a platform cache.
- Release CI builds Linux x86_64 and macOS arm64. It checks out sibling repos at
  their default branches, so a Faber tag does not yet pin the complete build
  provenance.
- `faber explain` and code-locale discovery still have disk/build-layout paths
  that the published binary-only archive does not supply as a relocatable pack.
- `faber install` composes Cista for local `cista.toml`, a Git URL, or an exact
  `name@version` from a filesystem registry. Git install has no required
  revision/checksum pin. Bare names fail closed. Remote HTTPS fetch is not yet
  composed into this Faber path.
- Cista installs interfaces under `$CISTAE_HOME` (default
  `~/.faber/cistae`) and rewrites `faber.lock`. Faber builds consume the lock;
  they do not discover or repair from the store. Current locks record absolute
  package/interface/artifact paths and are therefore machine/store-location
  specific.
- Regular source-package manifests do not yet express the general transitive
  graph needed for a third-party ecosystem. Native target packages currently
  use a Rust compile-at-install path; pure Faber interface packages should not
  inherit that prerequisite accidentally.
- Norma and Triga already have Cista source-package manifests. Norma is
  described as an implicit platform default; Triga is an explicit dependency.
  Live Triga package metadata is `0.2.0`, while some examples/locks still pin
  `0.1.0` and absolute workspace paths. Those examples are drift evidence, not
  a release-ready distribution proof.

## Current State

| Track | State | Next action |
| --- | --- | --- |
| Golden path definition | Implicit across start pages + READMEs | Stage 0 inventory |
| Dev-kit/distribution contract | Not yet selected | Stage 1 decision records |
| Clean-room install proof | Residual / planned (site + release delivery) | Stages 2–3 with release-and-portable-default |
| Hello / init UX | Works in monorepo; Latin default scaffold | Stage 5 |
| Norma acquisition (end user) | Dev layout / incomplete product install story | Stage 6 |
| Triga acquisition (end user) | Same; current example locks include absolute paths | Stage 7 |
| Multi-locale docs | Site locales present; drift risk high | Stage 8 after English path locks |
| CLI locale first-run | Flags exist; docs thin | Stage 8 |
| Agent install skills | `static/agents` + skills exist | Stage 9 honesty pass |

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

### Stage 1 — Dev Kit And Distribution Contract

**Status**: planned; depends on Stage 0  
**Lowers to**: delivery (decision artifacts only)  
**Owners**: faber control plane, with cista/release/library reviewers  
**Batching**: discovery-first; no installer implementation in this stage

**Outcome:** Accepted decisions define the product payload, installed layout,
component taxonomy, compatibility/version rules, library identity and lock
model, supported platform slice, channel priority, trust policy, and lifecycle.

**Required artifacts:**

- `dev-kit-contract.md` — required/optional payload and discovery rules.
- `install-channel-matrix.md` — archive, verified bootstrap, Homebrew, and
  macOS `.pkg`/`.dmg` feasibility with primary/secondary/deferred disposition.
- `package-and-lock-contract.md` — Norma, Triga, third-party, store, lock,
  relocation, offline, update, integrity, and compatibility semantics.
- `platform-matrix.md` and named clean-room profiles.
- Decision ledger: accepted, explicitly deferred, or routed; no silent default.

**Gate:** Another planner can specify the canonical payload without choosing
architecture. Absolute developer paths are not a portable lock design. Norma
and Triga have distinct stated lifecycles. Upgrade, repair, and uninstall have
owners. The macOS-native channel is selected or explicitly deferred.

**Stop if:** the package/lock model or core-vs-optional taxonomy has no owner.
Do not lower installer or library implementation around an unresolved contract.

---

### Stage 2 — Canonical Payload Assembly And Manifest

**Status**: planned; depends on Stage 1  
**Lowers to**: delivery → factory (faber + release assembly; library owners as selected)  
**Batching**: split-on-boundary; one payload format across channels

**Outcome:** Release automation produces one versioned dev-kit payload and a
machine-readable manifest of every component, version, digest, compatibility
bound, license, and installed destination. Today’s binary-only archive is
evidence, not presumed to satisfy this outcome.

**Gate:** Extracting the payload at an arbitrary prefix supports `faber
--version`, reference/locale lookup, and the selected core-support/Norma model
without siblings or environment overrides. Tampered or version-skewed content
fails before use. Payload construction has an early clean-room CI proof.

---

### Stage 3 — Primary Install Channel And Lifecycle

**Status**: planned; depends on Stage 2  
**Lowers to**: delivery → factory (faber + faberlang.dev + release sibling)  
**Batching**: split-on-boundary (installer, release, docs)

**Outcome:** The primary channel installs the canonical payload. It supports a
user-local prefix and non-interactive use, reports what changed, and has tested
reinstall, upgrade, repair, downgrade policy, and uninstall behavior.

**Gate:** Checksums/provenance are verified before install; PATH/shell changes
are explicit and reversible; uninstall removes only product-owned files; user
projects and dependency caches survive by default; install pages name the exact
artifact/version under test. Formula lag is labeled or fixed.

**Overlap rule:** A shell bootstrap downloads a fixed release payload; it is not
a second release system. Native macOS packaging must install the same logical
layout and pass Gatekeeper/notarization policy before being called supported.

---

### Stage 4 — First-Run Bootstrap And Doctor

**Status**: planned; depends on Stage 3  
**Lowers to**: delivery → factory (faber + cista integration as selected)  
**Batching**: one diagnostic contract; render human and JSON output separately

**Outcome:** A newcomer can prove the installed kit is coherent before opening
a project. A doctor-equivalent surface reports launcher and component versions,
prefix/channel when known, default execution target, required host tools,
Norma availability, store/cache roots, locale state, and repair steps.

**Gate:** Human and stable machine-readable modes cover healthy install,
missing Cargo/toolchain where relevant, missing/corrupt component, incompatible
version, unwritable prefix/store, offline state, and unsupported host. Each
failure names one next action and exits nonzero.

---

### Stage 5 — First Hour: Init, Check, Run

**Status**: planned; depends on Stage 4  
**Lowers to**: delivery → factory (faber + faberlang.dev)  
**Batching**: batch-by-default for hello/init docs once CLI defaults lock

**Outcome:** From install, a newcomer creates a package, checks it, and runs it
on the documented default product path with no monorepo state and no unlisted
toolchain prerequisite.

**Gate:** `hello` / `projects` pages match actual CLI and manifest shape;
direct/portable/native execution is one clear story; missing entry, manifest,
or prerequisite diagnostics are actionable; the named clean-room Hello path is
green with both positive and negative evidence.

---

### Stage 6 — Norma Platform Availability

**Status**: planned; depends on Stages 2, 4, and 5  
**Lowers to**: delivery → factory (faber + cista + norma + docs)  
**Batching**: split-on-boundary (package format, bootstrap, resolver, docs)

**Outcome:** A fresh kit can import and run a minimal `norma:*` program through
the exact platform-package model selected in Stage 1. The resolved Norma
version and integrity are inspectable and reproducible.

**Gate:** No `FABER_LIBRARY_HOME`, sibling checkout, or ambient fallback;
upgrade compatibility is enforced; missing/corrupt Norma yields a repair path;
the package snapshot excludes tests/docs/build outputs while preserving module
paths, source frontmatter, licenses, and selected locale metadata.

---

### Stage 7 — Triga And Third-Party Dependency Acquisition

**Status**: planned; depends on Stages 1 and 5; may start after Stage 6 proves
the shared package format  
**Lowers to**: delivery → factory (faber + cista + triga + docs)  
**Batching**: prove Triga first, then generalize only from the second package

**Outcome:** An app declares an exact compatible Triga dependency, acquires it
from the selected immutable bootstrap source, writes a reproducible lock, and
checks a `triga:math` import without the monorepo. The same contract is ready
for an unrelated third-party source package.

**Gate:** Project relocation works; two versions coexist for different
projects; offline rebuild works after fetch; a missing package gives one
actionable acquisition command; tampered content and incompatible targets fail
closed; lock update changes only the requested dependency closure. Absolute
developer paths are limited to explicit local-development locks/receipts.

**Stop if:** only mutable Git branches, machine-local paths, or an unproven
registry can satisfy the flow. Route the missing distribution capability; do
not publish a fake `faber install triga` story.

---

### Stage 8 — Multi-Locale Onboarding

**Status**: planned; depends on Stages 3–7 as their text stabilizes  
**Lowers to**: delivery → factory (faberlang.dev + faber CLI docs/help)  
**Batching**: batch-by-default across site locales after English lock

**Outcome:** Start track is usable in English and at least one additional site
locale with the same golden path. CLI docs cover code locale, diagnostic
locale, manifest selection, and package-owned locale limits without silent mix.

**Gate:** Locale parity covers install, doctor, Hello, Norma, and Triga; at
least one non-English site locale passes the same commands; diagnostic locale
works in clean room. Package-owned API locale transport is proven or named as a
specific deferred capability, never implied by package installation.

**Overlap rule:** Do not absorb `default-en-locale`; re-verify after that
campaign changes defaults.

---

### Stage 9 — Agent And Human Install Surfaces

**Status**: planned; depends on Stages 3–7  
**Lowers to**: delivery → factory (site agents pages, skills, CLI help)  
**Batching**: batch-by-default for skill/doc sync

**Outcome:** Human and agent instructions use the same artifacts and package
contract. Agent mode adds explicit prefix, non-interactive behavior, stable
exit codes, checksum verification, JSON diagnosis, and no hidden prompts.

**Gate:** The path diff is empty or deliberately dual-mode; one clean-room
agent script succeeds and one corrupt/unavailable dependency case fails with
machine-readable evidence.

---

### Stage 10 — Continuous Honesty And Promotion Gate

**Status**: planned; coverage begins in Stage 2 and consolidates here  
**Lowers to**: delivery → factory (CI in faber and/or faberlang.dev)  
**Batching**: discovery-first for durable CI ownership

**Outcome:** Recurring clean-room jobs fail release or docs promotion when the
canonical supported path regresses. Website promotion names a dev-kit manifest
and release whose full path passed.

**Gate:** macOS arm64 and Linux x86_64 have separate evidence where supported;
jobs reset HOME, PATH, store/cache, library overrides, repository access, and
credentials; network allowlists and artifact digests are recorded; failures
name the lying page, command, or component. Optional channels/locales remain
clearly labeled, but unavailable primary artifact, broken default run, missing
Norma, and non-actionable dependency failure are forbidden residuals.

## Dependency Rules

| Situation | Route |
| --- | --- |
| Clean-room binary/assets missing | Stages 2–3 + `release-and-portable-default` |
| Public registry required for library install | Need/mail to `cista-dev-registry`; use only an immutable, verified interim source selected by Stage 1 |
| Package store bug (path safety, lock) | Fix in `cista` under package-store ownership; onboarding stage waits or documents residual |
| Compiler cannot run portable package | Radix/faber product bug — not an onboarding copy fix alone |
| Triga API incomplete | Triga campaigns; Stage 7 still requires **install + import** of what exists |
| Default code locale English | `default-en-locale`; Stage 8 documents current surface only |
| Marketing wants “one curl \| sh” | Only when Stage 3 installs and verifies the canonical Stage 2 payload |

## First Useful Milestones

1. **Stage 0 closeout** — shared lie list; no more arguing about what the path is.
2. **Contract lock** — Stage 1 leaves no installer/package architecture for an
   implementation planner to invent.
3. **Canonical payload + install** — Stages 2–4 prove product coherence.
4. **Clean-room hello** — Stage 5 green without monorepo or hidden tools.
5. **Norma import without monorepo** — Stage 6.
6. **Pinned Triga restore** — Stage 7 proves relocation and offline replay.
7. **Second site locale parity** — Stage 8.
8. **CI honesty** — Stage 10.

## Acceptance Criteria (Campaign Artifact)

- Next stage to lower is identified (Stage 0).
- Cista campaigns are referenced without becoming this campaign’s backlog.
- Desired end state is user-outcome shaped, not “implement registry.”
- Multi-locale and library install are first-class stages.
- Dev-kit contents, core-vs-optional taxonomy, install lifecycle, package
  identity, portable locking, provenance, and supported platforms are explicit
  decision outputs before implementation.
- Stop conditions prevent fake docs and premature `cista.dev` dependency.
- Ready for **Stage 0 delivery** without further discovery meetings.

## Validation

| Layer | How |
| --- | --- |
| Artifact | Status line parseable; stages ordered; owners named |
| Stage 0 | Inventory doc committed under this directory or linked delivery |
| Stages 1–10 | Decision receipt or clean-room script/CI evidence per stage gate |
| Docs | zombie-docs style: commands on pages exist on CLI |

### Named clean-room profiles

| Profile | Required isolation and proof |
| --- | --- |
| `macos-arm64-fresh` | Supported fresh macOS user; isolated HOME/store; no repos, overrides, or ambient credentials; published signed/checksummed artifact |
| `linux-x64-minimal` | Minimal supported container; isolated HOME/PATH/store; only documented system prerequisites |
| `no-rust` | Cargo/rustc absent; selected portable `check`/`run` claim must pass or docs must not make it |
| `offline-restored` | Network denied after one authenticated/verified fetch; unchanged lock and populated store reproduce the build |
| `agent-noninteractive` | Explicit prefix; no prompts; stable exit codes and JSON diagnostics; cleanup receipt checked |

Every profile records the dev-kit manifest digest, allowed network endpoints,
exact PATH, environment variables removed, expected filesystem changes, and
positive plus negative outcomes. A locally built binary is not a published
artifact proof.

## Open Questions

1. **Canonical payload representation:** multi-file dev-kit archive, embedded
   single binary with materialization, or another layout?
2. **Norma model:** release-owned platform content, a seeded exact Cista
   package, or a normal explicit dependency with product shorthand?
3. **Portable lock and restore:** which logical identity replaces absolute
   store paths, and which command materializes the locked closure?
4. **Git/registry bootstrap:** exact Git revision plus digest, verified release
   asset, or wait for a proven remote registry path?
5. **Default newcomer execution:** always portable FHIR/FMIR, or keep Rust as
   default when present?
6. **Dependency graph:** where transitive metadata, conflict rules, target
   variants, and compatibility bounds live?
7. **Init language surface:** Latin `Salve, munde!` vs English scaffold vs
   locale-parameterized init?
8. **macOS-native value:** does a signed/notarized `.pkg` materially improve
   first contact enough to own credentials, receipts, upgrade, and uninstall?
9. **Deferred platforms:** minimum macOS, macOS Intel, Linux variants, and
   Windows status?

Stage 0 gathers evidence. Stage 1 records decisions. Provisional product choices
above guide planning, but no architecture-changing question closes merely due
to silence.

## Stop Conditions

- Pause if Stage 1 cannot define one canonical payload and package/lock model.
- Pause before Stage 6 if no coherent Norma platform model exists.
- Pause before Stage 7 if no immutable, verified Triga acquisition exists — do not
  publish “`faber install triga` works” without evidence.
- Pause if a stage would require production `cista.dev` credentials, DNS, or
  public upload — that is `cista-dev-registry` / ops, not this campaign.
- Pause if clean-room proof is replaced by “works on my monorepo.”

---

## Suggested Stage 0 Delivery Title

`faber-onboarding-stage0-golden-path-inventory` — single markdown inventory +
lie list + evidence table feeding the named Stage 1 decision artifacts; no
product code required.
