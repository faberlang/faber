# Faber release contract

**Status:** accepted — Stage 1 decision record (component-release-streamline)
**Date-stamped:** 2026-08-07
**Campaign:** [component-release-streamline](../factory/component-release-streamline/CAMPAIGN.md) — Stage 1 of 10
**Unit:** `component-release-streamline-stage1-release-contract`
**Governs:** every release-changing contract for the coordinated Faber product
line and the radix / cista component surfaces
**Source of truth:** `policy.md` (normative lane/channel policy),
`process-versioning-and-deps.md` (canonical reconciled fact set),
`../factory/component-release-streamline/stage0-baseline.md` §9 B11 handoff
and §1–§8 facts. This contract **decides**; it does not re-inventory.

> This document is a decision record, not a second fact inventory. Current
> as-built facts (protocols, matrices, versions, public surface) live in
> `process-versioning-and-deps.md` and `stage0-baseline.md` and are cited, not
> duplicated. Historical release notes (`v*.md`) are untouched.

---

## 1. Council-4 routing authority for `faber/docs/release/` — first decision

**Decision: accepted** (first item in the Stage-1 ledger; wording shared with
the `faber-onboarding` Stage-1 ledger so the two campaigns agree).

`faber/docs/release/` is governed by a **single routing authority — the
coordinated product release process contract** (this campaign's
`release-contract.md` + `release-manifest-schema.md`). The faber-onboarding
dev-kit payload manifest (its Stage 2) is authored as the **"dev-kit payload
section" of this single schema** (release-owned packs: launcher, core support,
reference/locale packs, libraries), never as a parallel document. Onboarding
Stage 1 writes nothing to `faber/docs/release/`; its payload-shape decisions
define **content** only, which Stage 2 encodes into this schema under this
process contract's review.

The two campaigns' Stage-1 outputs are non-overlapping by construction: this
stage = the seven release docs under `faber/docs/release/` (this file, the
manifest schema, the platform/builder matrix, authority, failure/recovery,
local-first process, and the worktree dry-run recipe); sibling Stage 1 =
decision records in its campaign directory.

**Evidence:** `delivery-stage1.md` "Council-4 interlock" (this campaign,
default wording); `../factory/faber-onboarding/delivery-stage1.md` "Council-4
interlock" (identical default); `stage0-baseline.md` §10 (interlock recorded,
not resolved).

---

## 2. Stage-1 decision ledger

Every release-changing decision below is `accepted` |
`explicitly-deferred-with-owner` | `routed`. No silent default. Evidence cites
`stage0-baseline.md` facts/rows (`F#`, `§N`) or a live `path:line`.

| # | Decision | Marking | Evidence | Resolved in |
| --- | --- | --- | --- | --- |
| L0 | Council-4 routing authority for `faber/docs/release/` = the coordinated product release process contract (`release-contract.md` + `release-manifest-schema.md`); dev-kit payload manifest is a **section** of this schema, never a parallel doc | **accepted** | `stage0-baseline.md` §10; this spec "Council-4 interlock"; sibling `delivery-stage1.md` interlock (identical wording) | §1 (this file) + `release-manifest-schema.md` §6 |
| OQ1 | Version/compatibility: independent SemVer per component + product pins in the release manifest; prerelease/hotfix/LTS channels reconciled with `policy.md` odd/even major lanes | **accepted** | B11 OQ1 (`needs-stage-1-decision`); `policy.md:15-28,62-70`; reconciled §2.1–2.2 | §3, §4 |
| OQ2 | Product input manifest: schema `release-manifest.yaml` at the faber repo root; pins source repos + release-owned pack inputs (dev-kit payload section); update authority = the release process; both local and CI consume by design | **accepted** (schema/decisions; instance implementation is Stage 3) | B11 OQ2 (`answered-by-evidence` — no build-consumed pin manifest today; `core-support-manifest.txt` pins paths, `Cargo.lock` doesn't pin path deps) + `needs-stage-1-decision` (schema); reconciled §3.5, §4 B7 | `release-manifest-schema.md` |
| OQ3 | Platform/support matrix: faber/radix linux x86_64 + macOS arm64 supported; cista additionally macOS x86_64 supported; macOS x86_64 (faber/radix) + Windows deferred; missing **supported** leg blocks the whole release, deferred legs never block | **accepted** | B11 OQ3 (`answered-by-evidence` — observed matrices 2026-08-07) + acceptance decision; `stage0-baseline.md` §1.1–1.2, F1 | `platform-builder-matrix.md` |
| OQ4 | Builder trust: burgus/pharos (operator-controlled) + GHA hosted runners (controlled); native builds; second clean-builder comparison required for stable/LTS product releases; reproducibility standard = verified provenance from controlled builders | **accepted** | B11 OQ4 (`carried-to-stage-1`); `stage0-baseline.md` §4; CAMPAIGN "Platform, Builder, And Reproducibility Contract" | `platform-builder-matrix.md` §3, §5 |
| OQ5 | Artifact authenticity: SHA-256 checksum manifest (basename-only content) + detached Ed25519 signature over the manifest; provenance receipt + SBOM for stable/LTS product releases; macOS signing where a controlled signer exists (none today — recorded gap) | **accepted** | B11 OQ5 (`needs-stage-1-decision`); F7 (checksum naming), CAMPAIGN "Supply Chain And Secret Boundary" | §7 |
| OQ6 | Stable immutability: absolute no-replacement for stable assets; idempotent retry on identical hashes; fail-closed on hash collision; exceptional incident authority only for security/legal takedown, operator-authorized, incident record | **accepted** | B11 OQ6 (`needs-stage-1-decision`; F4 `--clobber` is the documented gap) | §6 + `failure-recovery-matrix.md` |
| OQ7 | Channels/discovery: candidate/draft namespace; shared-repo global `Latest` reserved for promoted product releases (component releases never advance it); atomic installer/site promotion after remote readback | **accepted** | B11 OQ7 (`needs-stage-1-decision`; today `Latest` = `radix-v0.79.0` — §7 F5/§7 gap); `policy.md:68-69` | §4, §5 |
| OQ8 | Public host: retain `faberlang/releases` as the single public host; alternate/mirror storage explicitly deferred (no mirror now) | **accepted** (retain); mirror **explicitly-deferred-with-owner** (this campaign's later stages / cista-dev-registry) | B11 OQ8 (`carried-to-stage-1`); `stage0-baseline.md` §7 (consumers) | §8 |
| OQ9 | Production authority: proposer/builder/verifier/tagger/signer/publisher/promoter/withdraw roles; production authority operator-owned; agents may prepare/inspect/dry-run by default | **accepted** | B11 OQ9 (`needs-stage-1-decision`); CAMPAIGN "Authority And Durable Homes" | `authority.md` |
| OQ10 | Cista surface: binary-only component release today (CLI archives); crates.io library publication deferred; Faber pins the cista source revision in the release manifest, not a binary prerequisite | **accepted** (binary-only); crates.io **explicitly-deferred-with-owner** (cista owner); unfulfilled `cista-v0.1.0` publish **routed** (cista owner) | B11 OQ10 (`carried-to-stage-1`; F5) | §9 |

**Gate:** a later planner can implement Stage 2 (runbook) / Stage 3 (harness)
without choosing identity, authority, channel, platform, trust, or rollback
policy.

---

## 3. Release units and version rules (OQ1)

### 3.1 Product vs component release units

| Unit | What it is | Version sequence | Public surface |
| --- | --- | --- | --- |
| **Faber product release** | The coordinated language/toolchain line: CLI, compiler contract, ReaderPack, standard packages, ABI/wire (`policy.md:4-8`) | Faber `X.Y.Z`, governed by `policy.md` major-parity lanes (odd = development, even = LTS) | `faber-vX.Y.Z` on `faberlang/releases` |
| **Radix component release** | The compiler library+CLI, independently versioned (`policy.md` does not govern it — reconciled §2.1) | `0.Y.Z` (30 release-aligned crates, one version) | `radix-vX.Y.Z` |
| **Cista component release** | The package-store CLI/lib, standalone (`process-versioning-and-deps.md` §3.4) | `0.Y.Z` | `cista-vX.Y.Z` (tag `v0.1.0` exists; public release absent — routed, see §9) |
| **faber-runtime / hosts** | Assembled into the product via `core-support-manifest.txt`; no independent release surface (`process-versioning-and-deps.md` §1.1, §3.2) | — | never released standalone |

**Decision (accepted):** components keep **independent SemVer**; a Faber
product release **pins** the exact source commits of every build/path input
(radix, cista, faber-runtime, hosts — see `release-manifest-schema.md` §4).
No lockstep rule between component versions. A product release does **not**
require a public radix or cista binary release unless the product contract
names those binaries as user prerequisites (currently it does not — cista is
standalone; radix is a private-source path dependency). The current
companion-head model (`v1.4.0.md` "Companion pins") becomes **enforced pins**
in the manifest — documentary tables are no longer the mechanism
(`process-versioning-and-deps.md` §3.5, §4 B7).

**Evidence:** B11 OQ1; reconciled §2.1–2.3, §3.5; `v1.4.0.md` companion pins.

### 3.2 Version validation rule

`Cargo.toml` version must equal the source tag version for every release;
the version-validation gate is enforced (all three `release.yml`s reject a
mismatch — reconciled §2.2). The lockfile at the tagged commit must match the
manifests (`faber/AGENTS.md:141-144`; `radix/AGENTS.md:392-395`) — a stale
lockfile makes `--locked` fail (F2).

---

## 4. Channels and discovery (OQ7)

Channels are **reconciled with `policy.md`**, not invented in parallel.
`policy.md` names the lane structure (odd major = development, even major =
LTS) and the rule that `latest` must not blur a development line with an LTS
line (`policy.md:62-70`). The channel model below layers release-lifecycle
channels onto those lanes.

| Channel | Meaning | Version shape | Policy basis | Discovery rule |
| --- | --- | --- | --- | --- |
| **development** | The odd-major line; place for planned evolution and intentional breaking changes within its documented boundary | odd `X.Y.Z`; also `-dev` notes | `policy.md:15-18,62-65` | Never stable-default; may be the only active line |
| **candidate / prerelease** | Pre-GA evidence releases (historic precedent: `v1.0.0-rc.2`) | `X.Y.Z-rc.N` | `policy.md` lock-transition gate context | Published in the candidate/draft namespace (§4.1); never `Latest`, never site stable |
| **stable** | GA release of the current line (dev line or LTS line) | `X.Y.Z` | `policy.md:28` ("development line may still evolve") | Site stable default for its line; may hold global `Latest` only when the promotion rule allows (§4.2) |
| **LTS** | Even-major language-locked line with its own compatibility boundary, support window, and end-of-support notice | even `X.Y.Z` | `policy.md:15-28,66-69` + lock-transition gate (`policy.md:75-89`) | Holds global `Latest` when active; never blurred with the development line |
| **hotfix / maintenance** | Bug/portability/security fixes on the locked line, or maintenance on the dev line | patch bump on the target line | `policy.md:50-58` | Same as its line's stable discovery |

**Decision (accepted):** every release record identifies its major line
explicitly (per `policy.md:68-69`). A locked line publishes its stated support
window and end-of-support notice in its release record
(`policy.md:70-71`); this contract does not invent a duration.

### 4.1 Candidate / draft namespace

Candidates and rehearsal uploads use a non-production namespace (draft
releases or `candidate-*` release tags) and require explicit external-write
approval (`stage0-baseline.md` §4 classification; CAMPAIGN "Candidate,
Publication, And Promotion State Machine"). Dry-run means **no credentials, no
tags, no remote refs, no release objects, no public metadata** change.

### 4.2 Global `Latest` semantics

- The shared `faberlang/releases` repo's global `Latest` is **reserved for
  the latest promoted Faber product release** (`faber-vX.Y.Z`).
- **Component releases never advance the global `Latest`.** Radix and cista
  releases are created with `--latest=false`; a component's own latest is
  discoverable from its component-prefixed tags, never from the shared
  signal.
- When an LTS line is active, `Latest` points at the LTS line's latest; when
  no LTS line is active, `Latest` may point at the development line's latest
  GA. It never shows a development line as LTS or vice versa.
- **Current state is a recorded gap:** as of 2026-08-07 `Latest` =
  `radix-v0.79.0` (`stage0-baseline.md` §7; reconciled §1.3). Correction is
  **routed** to this campaign's Stage 6 (promotion) — the first promoted
  product release re-points `Latest`, and Stage 8's thin-CI publish enforces
  `--latest=false` for components.

### 4.3 Atomic promotion

Installer, website, package-index, Homebrew, and `latest` metadata advance
**only after** all required artifacts pass remote readback
(CAMPAIGN "Candidate, Publication, And Promotion State Machine"). Promotion
is the last step; it is operator-authorized (`authority.md`).

---

## 5. Artifact identity, archive schema boundary, immutability, idempotent retry

### 5.1 Artifact identity

| Element | Rule |
| --- | --- |
| Source identity | Annotated source tag `vX.Y.Z` + the manifest's pinned input commits (`release-manifest-schema.md` §4) |
| Public release identity | Component-prefixed tag: `faber-vX.Y.Z` / `radix-vX.Y.Z` / `cista-vX.Y.Z` (`radix/docs/release/shared-artifact-surface.md:5-10`) |
| Archive name | `<component>-v<version>-<target-triple>.tar.gz` (current workflow shape, reconciled §1.2) |
| Checksum file | `<archive>.sha256`, content naming **only the downloaded archive basename** (faber enforces today — `faber/.github/workflows/release.yml:151-155`; radix/cista write `dist/<archive>` paths — F7) |

**Decision (accepted):** every `.sha256` file names only the archive basename
so `shasum -c` works on a downloaded set. The radix/cista naming gap (F7) is
**routed** to the Stage 4 packaging implementation (portable checksum
manifests) and recorded in the radix/cista workflow owners' residual list.

### 5.2 Archive schema boundary

The archive **content/layout schema** (what a good Faber archive contains:
reference pack, reader data, bundled support, documentation, self-consistency,
clean-room gate) is owned by the sibling
`release-and-portable-default` delivery, and the release-owned pack **payload**
is the dev-kit payload section of the manifest schema. This contract owns
**how components ship**: identity, naming, immutability, checksum content
rules, provenance, and the publication/promotion sequence. The three documents
do not duplicate one another (`release-and-portable-default/delivery.md`;
`release-manifest-schema.md` §6).

### 5.3 Stable immutability (OQ6)

- Stable tags and assets are **immutable**. The current `--clobber` behavior
  (`faber/.github/workflows/release.yml:223`, `radix/...:186`,
  `cista/...:183`) is the documented gap (F4) and is **retired** as a design:
  a changed artifact requires a new patch release unless an exceptional
  incident record says otherwise (see `failure-recovery-matrix.md` §3).
- **Idempotent retry:** retrying the same operation with identical hashes is
  safe. A collision with different hashes **fails closed**.
- Tag movement, deletion, and stable-asset replacement are never ordinary
  rollback; their only exceptional path is in `failure-recovery-matrix.md` §3.

---

## 6. Authenticity and provenance (OQ5)

**Decision (accepted)** — the minimum trust contract:

| Layer | Decision | Scope |
| --- | --- | --- |
| Checksum manifest | SHA-256 manifest, content names basename-only (§5.1) | every component and the product |
| Checksum signature | **Detached Ed25519 signature over the checksum manifest** (signing key held by the tagger/signer role on an operator-controlled machine; never on a hosted runner or in public-mutation job secrets) | every component and the product |
| Per-artifact signature | optional, not required (the signed manifest authenticates the set) | — |
| Provenance receipt | machine-readable receipt: source pins, builder(s), toolchain/SDK versions, target triples, hashes, timestamps, gate outcomes | required for stable/LTS product releases; recorded for components |
| Platform signing | macOS code signing/notarization **where a controlled signer exists**; credentials never enter general build jobs. **None exists today** — recorded gap, `routed` to Stage 4/controlled-signer work | macOS arm64 artifacts |
| SBOM / license | Cargo-lock-derived license/notice inventory + optional SBOM policy; notices always shipped | required for stable/LTS product releases, optional for components |
| User verification path | download archive + `.sha256` into one directory → `shasum -c` → verify the detached signature | documented in Stage 2 runbook |

**Note:** SHA-256 alone detects changed bytes; it does not authenticate who
built or published them — that is what the signature + provenance receipt add
(CAMPAIGN "Supply Chain And Secret Boundary").

---

## 7. Public host (OQ8)

**Decision (accepted):** retain `faberlang/releases` as the **single public
host**. Consumers verified as of 2026-08-07: `faberlang.dev` install docs
(direct asset URLs + checksums), the toolchain CLI doc, the release-history
page, and `ianzepp/homebrew-tap` (`stage0-baseline.md` §7). No mirror is
added now.

**Explicitly deferred with owner:** alternate controlled storage or a mirror
host is owned by this campaign's later stages (Stage 8 thin-CI / controlled
remote builder) and the separate `cista-dev-registry` campaign — a change of
host must preserve the archive contract (per `release-and-portable-default`
open question 6). Nothing about the release identity, checksum, or provenance
contract is host-specific; the same contract applies to any successor host.

---

## 8. Cista surface (OQ10)

**Decision (accepted):** cista remains a **binary-only component release**
today — CLI archive(s) with checksums, published as `cista-vX.Y.Z`.

- **crates.io library publication:** `explicitly-deferred-with-owner`
  (owner: cista). A library surface would be a new public contract with its
  own packaging/verification path; it is not required by any current product
  contract.
- **Product pinning:** a Faber product release pins the **cista source
  revision** (commit SHA) in the release manifest — a source pin, not a
  binary prerequisite (`release-manifest-schema.md` §4).
- **Recorded gap routed:** the unfulfilled `cista-v0.1.0` public-release claim
  (F5; `cista/docs/release/v0.1.0.md` vs no observed release) is **routed**
  to the cista owner to reconcile before the next cista release.

---

## 9. References

- `policy.md` — normative lane/channel policy (cited, never duplicated).
- `process-versioning-and-deps.md` — canonical reconciled fact set (cited).
- `stage0-baseline.md` — routing summary, facts F1–F8, B11 handoff, §10.
- `../factory/faber-onboarding/delivery-stage1.md` — sibling interlock wording.
- `../factory/release-and-portable-default/delivery.md` — archive **content**
  authority (what a good archive contains).
- `radix/docs/release/shared-artifact-surface.md` — shared-surface tags +
  token contract.
- `authority.md`, `release-manifest-schema.md`, `platform-builder-matrix.md`,
  `failure-recovery-matrix.md`, `process-local-first.md`,
  `worktree-dry-run-recipe.md` — this stage's companion decisions.
