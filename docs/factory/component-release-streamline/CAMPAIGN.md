# Campaign: Component Release Streamline (Faber · Radix · Cista)

**Status**: planned — Stage 0 live reconciliation, then Stage 1 release-contract decisions
**Created**: 2026-08-07
**Mode**: routing artifact — does not implement code or perform releases
**Control-plane location**: `/Users/ianzepp/work/faberlang/faber/docs/factory/component-release-streamline/`
**Container mirror (not a git repo)**: `/Users/ianzepp/work/faberlang/docs/campaigns/component-release-streamline/`
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
  ([`radix/docs/factory/worktree-convention.md`](../../../radix/docs/factory/worktree-convention.md) spirit: operator-managed
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

## Release Object Model

A release is not a tag, a green build, or an uploaded tarball by itself. The
campaign must define and preserve these linked objects:

```text
release intent
  → exact source identity (product tag + immutable input commits)
  → candidate manifest (version, channel, targets, builders, toolchains)
  → verified artifacts (content + hashes + signatures/provenance)
  → publication record (remote URLs + readback hashes)
  → promotion record (candidate → stable/LTS discovery)
  → release receipt (all evidence and authority decisions)
```

For a Faber product release, source identity includes the exact Faber commit and
the exact commits of every path/build input: Radix crates, Cista,
`faber-runtime`, the hosts monorepo, and any release-owned packs selected by the
onboarding/payload contract. `Cargo.lock` does not pin path dependencies.
Documentary sibling-pin tables are evidence, not enforcement. A committed,
machine-readable release manifest must be consumed by both local and CI builds.

Radix and Cista component releases remain independently versioned. A Faber
product release may pin their source commits without requiring a public Radix
or Cista binary release unless the product contract names those binaries as
runtime/user prerequisites.

## Authority And Durable Homes

The workspace container is not a Git repository. It cannot own a committed
release authority. Durable ownership is:

| Scope | Canonical home |
| --- | --- |
| Coordinated Faber product process and release manifest schema | `faber/docs/release/` |
| Faber product scripts and receipts | `faber/scripta/` and `faber/docs/release/` |
| Radix component protocol | `radix/docs/release/` with Radix-local scripts |
| Cista component protocol | `cista/docs/release/` with Cista-local scripts |
| Container layout | Linked context only; never the sole committed authority |

Separate roles must be named even when one operator fills several roles:
proposer, builder, verifier, tagger/signer, publisher, promoter, and
withdraw/revocation authority. Agents may prepare, inspect, and dry-run by
default. Production tags, public uploads, promotion, overwrite, deletion, and
revocation remain operator-authorized external effects.

## Candidate, Publication, And Promotion State Machine

```text
planned
  → prepared       exact clean commits + candidate manifest
  → built          complete required target set
  → verified       gates, archive inspection, signatures/provenance
  → tagged         immutable annotated/signed source identity
  → published      candidate/draft assets uploaded
  → read-back      public bytes re-downloaded and verified
  → promoted       installer/site/channel metadata advances last
  → withdrawn      defect recorded; superseded, never silently replaced
```

- Stable tags and assets are immutable. Current workflow `--clobber` behavior
  is a gap, not desired idempotence. A changed artifact requires a new patch
  release unless an exceptional operator-approved incident record says
  otherwise.
- Retrying the same operation with identical hashes is idempotent. A collision
  with different hashes fails closed.
- Candidate or rehearsal uploads use a draft/staging namespace and require
  explicit external-write approval. Dry-run means no credentials, tags, remote
  refs, release objects, or public metadata can change.
- Installer, website, package-index, Homebrew, and `latest` metadata advance
  only after all required artifacts pass remote readback. The shared
  `faberlang/releases` repository also needs an explicit global “Latest” rule
  so a component release cannot accidentally replace the product signal.

## Platform, Builder, And Reproducibility Contract

“Local-first” means the release process and proof do not depend on Actions. It
does not mean one laptop proves every platform. Stage 1 must define a matrix of
component, target, support tier, native/cross build, controlled builder, OS/SDK,
toolchain, signing identity, required gates, and whether absence blocks the
whole release or only that target.

Exact source commits are necessary but not sufficient for reproducibility.
Record Rust/toolchain and packaging versions, target triple, OS/SDK/linker,
features, environment policy, and normalized archive rules (order, ownership,
mode, timestamp). Decide explicitly whether the standard is byte-for-byte
rebuild equivalence or verified provenance from controlled builders. Stable
release candidates should include a second clean-builder comparison where the
selected standard requires it.

## Supply Chain And Secret Boundary

Stage 1 must settle the minimum trust contract:

- artifact/checksum signing or attestations and the user verification path;
- source/input/toolchain/builder provenance and a release manifest digest;
- action and Rust-toolchain pinning where CI remains involved;
- archive license/notice inventory and optional SBOM policy;
- macOS code signing, notarization, quarantine, and minimum OS where supported;
- least-privilege publication credentials isolated from product build steps;
- a leakage gate proving private Radix source, paths, tokens, and logs are not
  present in public archives or receipts.

SHA-256 alone detects changed bytes. It does not authenticate who built or
published them.

## Failure And Recovery Matrix

The runbook and rehearsal must cover at least:

| Failure point | Default response |
| --- | --- |
| Before tag | Abort candidate; no public state |
| One of several source tags created | Stop; record partial state; do not publish until identity is reconciled |
| Required target missing or fails | Candidate incomplete; stable promotion blocked |
| Upload interrupted / token expired | Retry only against identical manifest and hashes |
| Existing asset has different bytes | Fail closed; never automatic `--clobber` |
| Bad checksum, notes, signature, or metadata before promotion | Correct candidate/draft, re-verify |
| Defect found after promotion | Withdraw/deprecate, publish incident note, supersede with new version |
| Signing or upload credential compromised | Revoke credential, freeze promotion, inventory affected releases, publish verified recovery record |

Deletion, tag movement, and stable-asset replacement are not normal rollback.
The campaign must define whether they are ever allowed and whose explicit
authority is required.

## Implementation Workflow

1. Stage 0 reconciles the existing inventory against live repos and retires
   stale claims; it does not create a competing inventory.
2. Stage 1 records release-object, authority, version/channel, platform,
   immutability, provenance, and recovery decisions.
3. Stage 2 writes canonical repo-owned process docs from those decisions.
4. Later stages lower scripts, candidates, packaging, controlled-builder
   rehearsals, promotion, and recovery drills by repo ownership.
5. Real production releases remain operator-authorized; campaign stages
   produce side-effect-safe dry-run evidence by default.

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
| [`radix/docs/factory/worktree-convention.md`](../../../radix/docs/factory/worktree-convention.md) | Packet/worktree ops model to reuse for release rehearsal |

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
- A detailed `faber/docs/release/process-versioning-and-deps.md` inventory
  already exists but contains stale topology, versions, tags, and platform
  claims. Live state now includes Faber `1.4.0` / tag `v1.4.0`, Radix `0.79.0`
  across 30 release-aligned crates / tag `v0.79.0`, and Cista `0.1.0` / tag
  `v0.1.0`. Stage 0 reconciles this document instead of duplicating it.
- Faber release CI checks out Faber at the requested tag but Radix, Cista,
  runtime, and hosts at moving default-branch tips. Existing sibling-pin notes
  are not consumed by the build.
- Faber release CI does not run `scripta/release-gate`. Radix release publish
  is not ordered after its independent tag-triggered full-suite workflow.
  Cista release CI builds and checks `--version` but does not run its test,
  lint, hygiene, install, or package smoke surfaces.
- All three workflows keep packaging logic inline in YAML and allow published
  assets to be replaced with `gh release upload --clobber`.
- Radix and Cista checksum files currently name `dist/<archive>` rather than
  the downloaded archive basename, so ordinary consumer-layout checksum
  verification is not directly usable.
- Faber and Radix currently build Linux x86_64 + macOS arm64. Cista additionally
  builds macOS x86_64. These are observed matrices, not yet an accepted support
  policy.

## Current State

| Track | State | Next action |
| --- | --- | --- |
| Written per-repo protocols | Present but split across AGENTS, workflows, and stale/current release docs | Stage 0 reconciliation |
| Release contract / authority | Implicit and incomplete | Stage 1 decisions |
| Build-consumed product pin manifest | Missing; documentary tables exist | Stage 1 schema → Stage 3 implementation |
| Local end-to-end dry-run | Missing | Stages 3–5 |
| Worktree release candidate | Not defined | Stage 3 |
| Deterministic archive + provenance | Partial / CI-owned | Stage 4 |
| Staged immutable publication | Missing; current workflows clobber | Stage 6 |
| Recovery / withdrawal drill | Missing | Stage 7 |
| GHA optional/thin path | Not designed | Stage 8 |
| Cadence / automation | None | Stage 9 optional |

## Campaign Path

### Stage 0 — Live Baseline Reconciliation And Stale-Doc Retirement

**Status**: selected; ready for `delivery`  
**Lowers to**: delivery (docs only under this campaign dir or linked)  
**Batching**: discovery-first  
**Owners**: container docs + read-only survey of faber/radix/cista

**Outcome:** Reconcile the existing release analysis, policies, notes, AGENTS
protocols, scripts, workflows, live tags/versions, public artifact claims, and
current dependency topology into one current fact set. Do not create a second
unlinked inventory.

Inventory includes:

- Step lists from AGENTS and workflows (side-by-side table).
- Every local script that is release-adjacent (`release-gate`, radix ladder,
  any cista scripts).
- What **must** hit the network today (private clone, artifact upload, `gh`).
- What can already run offline on `burgus` / `pharos`.
- Failure modes observed (GHA queue, lockfile mistakes, sibling path drift).
- Existing release artifacts/tags and their mutability/readback state.
- Current consumers of published URLs, versions, checksums, and `latest`.
- Stale documents/claims to update, demote to history, or archive.
- Open contract decisions for Stage 1.

**Gate:** Current facts are committed under `faber/docs/release/`; every row has
live source evidence and `local`, `controlled-builder`, `network`, or `public
mutation` classification. Historical facts are dated rather than silently
rewritten. No product code required.

---

### Stage 1 — Release Contract, Authority, And Manifest Schema

**Status**: planned; depends on Stage 0  
**Lowers to**: delivery (decision artifacts; no release implementation)
**Batching**: discovery-first

**Outcome:** Accepted decisions define:

- product vs component release units and compatibility/version rules;
- development, candidate/prerelease, stable, LTS, and hotfix channels;
- machine-readable Faber input/pin manifest schema and authoritative version
  sources/exclusions for each component;
- platform/support/builder matrix and what blocks complete release;
- artifact identity, archive schema boundary, immutability, idempotent retry,
  and global `Latest` semantics in the shared release repo;
- reproducibility standard, signing/provenance/SBOM/license decisions, and
  secret/private-source boundary;
- proposer/builder/verifier/tagger/publisher/promoter/withdraw authority;
- abort, partial failure, withdrawal, revocation, and supersede semantics.

**Required artifacts:** `release-contract.md`, `release-manifest-schema.md`,
`platform-builder-matrix.md`, `authority.md`, and `failure-recovery-matrix.md`
(names flexible; all under committed repo-owned release docs).

**Gate:** Each release-changing decision is accepted, explicitly deferred, or
routed to an owner. A later planner can implement without choosing identity,
authority, channel, platform, trust, or rollback policy.

**Stop if:** product input identity, stable asset immutability, or production
authority remains unresolved. Do not write an executable release procedure
around silent defaults.

---

### Stage 2 — Canonical Repo-Owned Process And Threat Model

**Status**: planned; depends on Stage 1  
**Lowers to**: delivery (documentation)
**Batching**: docs-first

**Outcome:** `faber/docs/release/` owns the coordinated product runbook and
threat model. Radix and Cista own thin component runbooks that point to their
local scripts and the shared contract. The non-repo container has no canonical
committed document.

The process specifies prepare, verify, tag/sign, publish candidate, remote
readback, promote, withdraw/supersede, and receipt steps. It maps claims to the
required Faber, Radix, Cista, language-matrix, and clean-install gates rather
than running every expensive suite unconditionally.

**Gate:** A cold operator can identify the authority and exact next command for
component-only, Faber product, prerelease, stable, hotfix, abort, and withdrawal
paths without asking chat. The threat model covers private Radix leakage,
untrusted build inputs, credential exposure, and asset replacement.

**Authorization:** Creating worktrees and running expensive gates needs
operator intent; this stage is not silent agent background work.

---

### Stage 3 — Candidate Workspace, Pinning, And Side-Effect-Safe Harness

**Status**: planned; depends on Stages 1–2
**Lowers to**: delivery → factory per repo
**Batching**: split-on-boundary (faber / radix / cista / container)

**Outcome:** Shared-script primitives prepare an isolated release candidate from
clean exact commits, validate remote/commit identity, generate and consume the
Faber release manifest, and produce a plan/receipt without public effects.

Dry-run uses an explicit output directory, scrubbed publish credentials, no
network mutation, and an exact `would-tag` / `would-upload` manifest. Mechanical
helpers cover clean-tree/remote checks, version alignment (all 30 Radix release
crates plus intentional exclusions), lock updates, notes presence, tag absence,
and worktree creation/disposal.

**Gate:** Focused sandbox tests prove no public mutation, correct pin
consumption, dirty/wrong-remote/wrong-version rejection, deterministic plan
output, idempotent cleanup, and no ambient credential use. No expensive release
gate is required merely to test the harness.

**External-effect rule:** Creating/removing worktrees outside an approved packet,
running expensive gates, and any network write require named operator approval.

---

### Stage 4 — Deterministic Packaging, Provenance, And Verification

**Status**: planned; depends on Stage 3  
**Lowers to**: delivery → factory per repo
**Batching**: split-on-boundary (faber / radix / cista / signing)

**Outcome:** One repo-local packaging implementation per artifact class is
called by both local rehearsal and CI. It produces the Stage 1 archive schema,
normalized metadata, portable checksum manifests, provenance, signatures or
attestations as selected, licenses/notices, and machine-readable receipts.

**Gate:** Extract-and-inspect tests assert exact inventory and executable mode;
checksum verification works after downloading files into one directory;
`--version` equals the candidate version; a hermetic CLI smoke passes; private
source/paths/secrets are absent; repackage/rebuild comparison meets the selected
reproducibility standard. Workflow YAML contains orchestration, not a second
packaging definition.

### Stage 5 — Controlled-Builder Component And Product Rehearsal

**Status**: planned; depends on Stage 4
**Lowers to**: operator-authorized factory/release rehearsal
**Batching**: one named boundary run, not a verification loop

**Outcome:** One component-only candidate and one Faber product candidate build
from exact pinned inputs on the controlled builders required by the selected
platform matrix. Receipts record commands, source/input commits, dirty-state
checks, toolchains, gates, artifacts, hashes, timing, skipped claims, and no
public writes.

**Gate:** Required target set is complete; Faber release gate and any
claim-triggered Radix/language gates passed exactly once at the named boundary;
artifacts pass clean extract/run/install smoke; main branches and public refs
are untouched; missing targets are explicit blockers or accepted exclusions
under the matrix—not silently absent.

**Stop if:** private Radix cannot be built on a controlled builder without
leakage. Select a minimum trusted remote/self-hosted builder rather than falling
back to moving public GHA inputs.

---

### Stage 6 — Staged Publish, Remote Readback, And Promotion

**Status**: planned; depends on Stage 5
**Lowers to**: delivery → operator-authorized factory
**Batching**: split prepare/publish/readback/promote

**Outcome:** A candidate/draft namespace accepts already-verified artifacts
without rebuilding them. Publication validates the immutable source identity,
complete target set, hashes, notes, and signatures; remote readback downloads
and re-verifies public bytes; stable promotion and downstream installer/site
metadata happen last.

**Gate:** Same-hash retry is safe; different-hash collision fails; no stable
`--clobber`; partial upload cannot promote; public notes derive from committed
release notes; source/public tag mapping and global `Latest` policy are checked;
one operator-authorized rehearsal uses a clearly non-production namespace.

---

### Stage 7 — Failure, Withdrawal, And Recovery Drill

**Status**: planned; depends on Stage 6
**Lowers to**: operator-authorized rehearsal + documentation

**Outcome:** Inject and recover from missing matrix leg, bad checksum/signature,
duplicate identical and conflicting assets, expired token, interrupted upload,
bad notes/version/tag, post-promotion defect, and compromised credential.

**Gate:** No scenario silently replaces stable bytes or leaves discovery
metadata pointing at an incomplete release. The receipt names retained public
state, withdrawal/supersede action, human authority, and next safe operation.

---

### Stage 8 — Thin CI Or Controlled Remote Builder

**Status**: planned; depends on Stages 3–7
**Lowers to**: delivery → factory per repo

**Outcome:** CI calls the same manifest, gate, package, and verification
primitives. It consumes exact pinned inputs and either mirrors verified
artifacts, serves as an explicitly trusted platform builder, or remains a
secondary fallback. Faber publication cannot skip `release-gate`; Radix publish
cannot race independently ahead of its required full validation.

**Gate:** A branch-tip change cannot change an existing candidate; build jobs
cannot publish directly with broader credentials than needed; concurrency and
approval prevent duplicate promotion; local and CI receipts have the same
schema.

---

### Stage 9 — Regular Cadence And Optional Automation

**Status**: planned; depends on Stages 1–8; optional for campaign closeout
**Lowers to**: delivery  

**Outcome:** Manual, agent-assisted, and scheduled dry-run cadence options have
prerequisites, owners, budgets, notification, and kill switches. Automation is
dry-run-only by default and never inherits production authority.

**Gate:** Operator can select cadence without redesigning the release process.

---

### Stage 10 — Closeout And Protocol Authority

**Status**: planned; depends on mandatory Stages 0–8
**Lowers to**: delivery (docs)  

**Outcome:** AGENTS.md release sections in faber/radix/(cista) and container
layout notes point to the correct repo-owned authorities. Campaign status →
done or parked with residual list. Onboarding consumes only promoted release
metadata.

**Gate:** A cold operator completes one candidate rehearsal and one recovery
scenario without chat. All required receipts are durable. Cadence automation,
extra targets, and alternate artifact hosts may remain explicit residuals;
identity, pin enforcement, immutable publication, remote readback, and recovery
may not.

## Dependency Rules

| Situation | Route |
| --- | --- |
| Portable archive **content** (reference packs, no-Rust defaults) | `release-and-portable-default` — process campaign records pins/gates only |
| Site/install/channel version strings | Onboarding / faberlang.dev — advance only after Stage 6 promotion/readback |
| Feature work blocking green gates | Own factory campaigns; release dry-run may residual “gate red on main” |
| Need private token for GHA | Stage 1 secret boundary + Stage 8 least-privilege builder; do not make broad token access the design |
| Operator wants real tag during campaign | Explicit human authorization; not default stage gate |
| Worktree path conflicts | Operator packet lifecycle; do not auto-prune foreign worktrees |
| macOS signing/notarization | Stage 1 trust decision + controlled signer; credentials never enter general build jobs |
| Public asset is wrong after promotion | Stage 7 withdraw/supersede procedure; no silent clobber or tag move |

## First Useful Milestones

1. Stage 0 current fact set and stale-doc disposition.
2. Stage 1 accepted release contract, manifest schema, platform/authority/recovery matrices.
3. Stage 3 side-effect-safe candidate plan consuming exact input pins.
4. Stage 4 consumer-verifiable artifact + provenance receipt.
5. Stage 5 component and Faber controlled-builder rehearsals.
6. Stage 6 immutable publish/readback/promotion rehearsal.
7. Stage 7 successful failure-recovery drill.

## Acceptance Criteria (This Artifact)

- Next stage identified: **Stage 0**.
- Local-first and worktree dry-run are explicit.
- Faber / Radix / Cista all in scope with clear product vs component roles.
- Durable authority lives in Git repositories, not the non-repo container.
- Release identity, enforced pins, builder/platform trust, immutable assets,
  promotion, recovery, and external authority are explicit decision stages.
- Related portable-default and onboarding campaigns referenced, not absorbed.
- Stop conditions prevent accidental public mutation, moving-input builds,
  stable asset replacement, and GHA-only thinking.
- Ready for Stage 0 delivery without solving automation in this session.

## Validation

| Layer | How |
| --- | --- |
| Artifact | Parseable Status; stages ordered |
| Stage 0–2 | Current evidence + accepted decisions + canonical docs committed |
| Stages 3–8 | Machine-readable receipts with identity, commands, outcomes, hashes, and authority |
| Scripts | Focused sandbox tests + one named controlled-builder rehearsal; no public effect in dry-run |
| Publication | Remote download/readback matches promoted manifest and hashes |
| Recovery | Injected failure leaves no silently mutable or falsely discoverable stable release |

## Open Questions

1. **Version and compatibility:** independent SemVer with product pins, or any
   lockstep rule; how prerelease, hotfix, and LTS channels differ?
2. **Product input manifest:** exact schema, location, update authority, and
   whether it pins package/reference/locale inputs as well as source repos?
3. **Platform/support matrix:** which Faber, Radix, and Cista targets are
   supported, experimental, or deferred; which missing leg blocks promotion?
4. **Builder trust:** burgus, pharos, self-hosted runner, or hosted runner per
   target; native vs cross-build; second-builder comparison standard?
5. **Artifact authenticity:** signed checksum manifest, per-artifact signature,
   platform signing, provenance attestation, SBOM/license baseline?
6. **Stable immutability:** absolute no-replacement rule, or what exceptional
   incident authority could replace an asset?
7. **Channels and discovery:** candidate/draft namespace, shared-repo `Latest`,
   development vs LTS, and atomic installer/site promotion?
8. **Public host:** retain `faberlang/releases`, add alternate controlled
   storage, or use one as mirror?
9. **Production authority:** which human role may tag, publish, promote,
   withdraw, revoke, and supersede?
10. **Cista surface:** binary-only component release or future crates.io library
    publication, and how Faber product pins/consumes it?

Stage 0 gathers current evidence. Stage 1 records accepted choices. No
architecture-changing release default becomes policy merely because it was
unanswered.

## Stop Conditions

- Do not push production tags or overwrite release assets without explicit
  operator authorization.
- Do not require GitHub Actions green as the sole candidate proof.
- Do not merge release bump commits to main from dry-run worktrees without a
  separate accept/merge decision.
- Do not publish untagged or moving-input artifacts as stable.
- Do not promote until the complete required matrix passes remote readback.
- Do not silently rebuild a candidate from newer sibling branch tips.
- Do not use `--clobber`, move a stable tag, or delete public evidence as an
  ordinary retry/rollback mechanism.
- Do not expand scope into language features or onboarding content.
- Pause if private radix cannot be built anywhere the operator controls — route
  a need for builder placement before claiming local-first complete.
- Pause if Stage 1 cannot assign production and recovery authority or define an
  immutable product identity.

---

## Suggested Stage 0 Delivery Title

`component-release-streamline-stage0-inventory` — side-by-side protocol table,
script list, network/public effects, current tags/artifacts, stale-doc
dispositions, and Stage 1 decision inputs; pure documentation under this
campaign directory and `faber/docs/release/` as appropriate.
