# Release process, versioning, and repo interdependencies

**Status:** authoritative COO-scope analysis (head-cpo), **reconciled 2026-08-07**
(Stage 0 of the `component-release-streamline` campaign)
**Scope:** the full faberlang multi-repo ecosystem — release workflows,
versioning policy, dependency graph, and breakage inventory
**Companion docs:** [`policy.md`](policy.md) (normative release lane policy),
`v1.0.0.md`, `v1.0.0-rc.2.md`, `v1.1.1-sibling-pins.md`, `v1.3.0.md`,
`v1.4.0.md`, `v1.5.0-dev-notes.md`

> **Reconciliation banner (2026-08-07).** This document is the canonical
> reconciled fact set for release process/versioning/topology. Live facts below
> were re-verified against the repos and the public `faberlang/releases`
> surface on **2026-08-07**. Stale claims from the prior revision are preserved
> as **dated history** in the sections marked *Historical record* — never
> silently rewritten. The routing summary and per-row
> local/controlled-builder/network/public-mutation classification live in
> `docs/factory/component-release-streamline/stage0-baseline.md`.

---

## 1. Current release process (as-built, 2026-08-07)

### 1.1 Workflow inventory

| Component | `release.yml` | Trigger | Build matrix (observed 2026-08-07) | Publish target |
| --- | --- | --- | --- | --- |
| **faber** | ✅ `.github/workflows/release.yml` | tag push `v*.*.*` or `workflow_dispatch` with tag input | linux-x86_64, macOS arm64 (macos-14; **Intel dropped** — GitHub queue) | `faberlang/releases` as `faber-v*.*.*` |
| **cista** | ✅ `.github/workflows/release.yml` | tag push `v*.*.*` or `workflow_dispatch` | linux-x86_64, macOS-x86_64 (macos-13), macOS arm64 (macos-14) | `faberlang/releases` as `cista-v*.*.*` |
| **radix** | ✅ `.github/workflows/release.yml` | tag push `v*.*.*` or `workflow_dispatch` | linux-x86_64, macOS arm64 (macos-14; Intel dropped) | `faberlang/releases` as `radix-v*.*.*` |
| **radix `ci.yml`** | ✅ separate workflow | push to `main` → `./scripta/test --stage 1-4`; tag push → `./scripta/test --full` | ubuntu | — (validation only) |
| **faber-runtime** | ❌ | — | — | — |
| **hosts** (monorepo) | ❌ | — | — | — |
| **triga** | ❌ (pure Faber source library) | — | — | — |
| **norma** | ❌ (pure Faber source stdlib) | — | — | — |
| **examples** | ❌ | — | — | — |

Three components have release workflows; radix also has a validation `ci.yml`.
The runtime/host/stdio/example repos are consumed as path sources and have no
independent release surface.

Matrix source evidence: `faber/.github/workflows/release.yml:26-37`,
`radix/.github/workflows/release.yml:26-37`, `cista/.github/workflows/release.yml:26-38`,
`radix/.github/workflows/ci.yml:19-24,40-49,110-118`.

### 1.2 Tag-trigger flow (all three release workflows)

```
operator pushes vX.Y.Z tag (or workflow_dispatch with tag)
  → resolve source tag, validate SemVer shape
  → checkout source repo at tag
  → [faber only] checkout sibling repos at default-branch tips:
      radix (private, FABERLANG_RELEASES_TOKEN), cista, faber-runtime,
      hosts (mintedgeek/hosts monorepo)
  → validate Cargo.toml version == tag version
  → build --locked --release for each matrix target
  → package binary + SHA-256 + README into tar.gz
  → upload workflow artifact
  → publish job: gh release create/upload to faberlang/releases
    with component-prefixed tag (e.g. faber-v1.4.0)
```

Sibling checkout evidence: `faber/.github/workflows/release.yml:69-93`.

### 1.3 Shared publish surface

All three workflows publish to `faberlang/releases` — a single shared public
release repo. Component prefixes distinguish artifacts:

```text
faber-v1.4.0    → Faber CLI binary + checksums
radix-v0.79.0   → Radix compiler CLI binary + checksums
cista-v0.1.0    → Cista package-store CLI binary + checksums (tag exists;
                  NO release observed on the public surface as of 2026-08-07)
```

The publish step uses `FABERLANG_RELEASES_TOKEN` secret scoped to create/update
releases in the shared repo.

**Observed public surface (2026-08-07, `gh release list -R faberlang/releases
--limit 100`):** 45 releases — 4 Faber (`faber-v1.1.1`, `faber-v1.2.0`,
`faber-v1.3.0`, `faber-v1.4.0`) and 41 Radix (`radix-v0.32.0` …
`radix-v0.79.0`). The shared repo's **`Latest` points at `radix-v0.79.0`** — a
component release currently holds the product's discovery signal. No
`cista-v*` release exists.

### 1.4 Faber's multi-repo build (critical constraint)

The faber release workflow checks out **sibling repos at their default-branch
tip** (not pinned to any version):

```yaml
# faber/.github/workflows/release.yml
- faberlang/radix          (private, token: FABERLANG_RELEASES_TOKEN)
- faberlang/cista
- faberlang/faber-runtime
- mintedgeek/hosts         (public hosts monorepo)
```

These siblings are required because:

1. **Cargo path dependency**: `radix = { path = "../radix/crates/radix" }` in
   faber's `Cargo.toml`.
2. **build.rs core-support assembly**: faber's `build.rs` reads
   `core-support-manifest.txt` and bundles source from sibling paths into a
   compressed archive embedded in the binary at compile time. The manifest
   pins **logical roots by path, not by SHA** (2026-08-07):

```text
# faber/core-support-manifest.txt
faber-runtime
radix/crates/radix-runtime-contract
hosts/crates/host-kernel
hosts/crates/host-native
hosts/crates/aleator
hosts/crates/http
hosts/crates/consolum
hosts/crates/processus
hosts/crates/solum
hosts/crates/tempus
```

If `core-support-manifest.txt` changes its sibling paths, the CI checkout
steps must be updated to match (`faber/AGENTS.md:117-120, 160-164`).

3. **Radix's own release workflow** checks out only `faberlang/radix` — it no
   longer checks out any sibling (the historical faber-runtime checkout was
   removed; see §3.3 dated history).

### 1.5 Hygiene ratchet

Faber, cista, and radix each carry a workspace-local `hygiene-ratchet`
dev-dependency. It enforces code-hygiene invariants during tests but is not
part of the release flow.

---

## 2. Versioning approach

### 2.1 Policy framework (still normative)

[`policy.md`](policy.md) defines a **major-parity** system for the Faber
product line:

- **Odd majors** = development lines (evolving contracts). Faber 1.x is the
  first public development line.
- **Even majors** = language-locked LTS lines. Faber 2 is the first planned LTS.
- `latest` channel must not blur development with LTS.

This policy applies to the **coordinated Faber product line** (CLI, compiler
contract, ReaderPack, standard packages, ABI/wire). It does not directly govern
the Radix compiler version or the Cista package manager version, which have
their own independent version sequences.

### 2.2 Per-component version state (observed 2026-08-07)

| Component | Cargo.toml | Latest git tag | Tags total | Match? | Public release on `faberlang/releases`? |
| --- | --- | --- | --- | --- | --- |
| **faber** | `1.4.0` | `v1.4.0` | 7 (`v1.0.0`, `v1.0.0-rc.2`, `v1.1.0`, `v1.1.1`, `v1.2.0`, `v1.3.0`, `v1.4.0`) | ✅ | ✅ `faber-v1.4.0` (also `v1.1.1`–`v1.3.0`) |
| **radix** | `0.79.0` (30 release-aligned crates; `crates/hygiene-ratchet` stays `0.1.0`) | `v0.79.0` | `v0.7.0` … `v0.79.0` | ✅ | ✅ `radix-v0.79.0` (41 component releases observed) |
| **cista** | `0.1.0` | `v0.1.0` | 1 | ✅ | ❌ **none observed** (tag exists; publish not surfaced) |
| **faber-runtime** | `0.1.0` | (none) | 0 | n/a | never released |
| **hosts** (monorepo) | workspace crates | (none) | 0 | n/a | never released |

Version-gate alignment is now in force for all three components: each
`release.yml` rejects a tag whose version differs from `Cargo.toml` (faber
`:97-115`, radix `:68-86`, cista `:66-82`).

### 2.3 What "release X.Y" means (current)

A Faber product release (`1.4.0`, tag `v1.4.0`) is a statement about the Faber
crate alone. Radix (`0.79.0` / `v0.79.0`) and Cista (`0.1.0` / `v0.1.0`)
version independently. The Radix revision a Faber build consumes is whatever
the faber release CI checks out of the private radix sibling at default-tip —
**there is still no version pin** (see §3.5).

#### Historical record (dated 2026-08-07)

The prior revision of this document (§2.3) described the same decoupling using
stale numbers: "Faber 1.1.0 (Cargo.toml ahead, no `v1.1.0` tag)", "Radix stuck
at `0.38.0` while 74 retrospective tags `v0.7.0`–`v0.75.0` marked development
epochs (`radix/docs/release/retrospective-minor-tags.md`)", and "Cista never
tagged". All three have since been aligned to the current state in §2.2. The
retrospective-tags mechanism remains documented in
`radix/docs/release/retrospective-minor-tags.md` (marker-only tags are "not
evidence that binaries were built or published").

---

## 3. Interdependencies between repos (current topology, 2026-08-07)

### 3.1 Dependency graph

```text
faber (1.4.0)
├── radix ← path dep: ../radix/crates/radix
├── [build.rs] core-support assembly ← core-support-manifest.txt pins paths:
│   ├── faber-runtime
│   ├── radix/crates/radix-runtime-contract
│   └── hosts/crates/{host-kernel, host-native, aleator, http, consolum,
│                    processus, solum, tempus}
└── [dev] hygiene-ratchet (workspace-internal)

radix (0.79.0, 30 release-aligned crates + hygiene-ratchet 0.1.0)
├── (no faberlang path deps — fully independent)
└── [dev] hygiene-ratchet (workspace-internal)

cista (0.1.0)
└── (no faberlang path deps — fully independent)
    [dev] hygiene-ratchet (workspace-internal: crates/hygiene-ratchet)

faber-runtime (0.1.0)
└── (runtime types for generated code; assembled into faber via the manifest)

hosts (monorepo; no CI)
└── host-kernel, host-native, aleator, http, consolum, processus, solum,
    tempus — path-deps consumed by the faber core-support archive
```

#### Historical record (dated 2026-08-07)

The prior revision listed the four sibling repos `faber-runtime`,
`host-kernel-rs`, `host-native-rs`, `host-providers-rs` (with a 7-sub-crate
`host-providers-rs` workspace) as separate repos. The host surface has since
consolidated into the **`mintedgeek/hosts` monorepo**, and
`radix/crates/radix-runtime-contract` joined the core-support manifest. The old
release-order analysis ("faber-runtime ships first … faber ships last") is
superseded: faber, radix, and cista are the only repos with release surfaces.

### 3.2 Release order (current)

In practice, **only faber, radix, and cista have release workflows**. The
runtime and host repos are consumed as path sources and have no independent
release surface. Their "release" is whatever commit is at HEAD when faber's
workflow checks them out — this remains an unmanaged moving input.

### 3.3 Radix independence — current

Radix is fully independent from all sibling repos: `crates/radix/Cargo.toml`
has no faberlang path dependencies. The historical elimination of the
faber-runtime dependency (`de80b63cf`, `ce6030dd4`) stands. The historical
stale-CI item — radix `release.yml` checking out `faber-runtime` — has been
**fixed**: the current `radix/.github/workflows/release.yml` checks out only
`faberlang/radix` (own repo, `:57-63`).

### 3.4 Cista independence — current

Cista is fully standalone: no path dependencies on any faberlang repo. Its
release workflow checks out only its own source. It can be released
independently at any time; the `v0.1.0` source tag exists, but **no public
`cista-v0.1.0` release has been observed** on `faberlang/releases`
(2026-08-07) — the publication leg has not surfaced despite the release note
claim (see §4 B3).

### 3.5 Companion-head coordination model

Faber release notes record **companion release-lane heads** — the exact commit
hashes of sibling repos at the time the release was validated. Current example
(`faber/docs/release/v1.4.0.md`, "Companion pins (local path layout)"):
Radix `v0.79.0` (`5bbdbbd49`), Cista `99acb1e`, faber-runtime `57493dc`, hosts
`ced40f8`. The earlier `v1.1.1-sibling-pins.md` records CI main tips at the
lock refresh.

These are **documentary facts, not release pins.** The faber release CI does
**not** pin to these commits — it checks out siblings at default-branch tips.
A Faber tag release may therefore build against companion repos that have
drifted past their validated companion-head. **This remains an unmanaged
risk** (campaign Open Question 2 → Stage 1 `release-manifest-schema.md`).

---

## 4. Broken / incomplete parts inventory (reconciled 2026-08-07)

### Resolved items (dated history)

#### B1. Faber 1.1.0 had no release tag — RESOLVED

The prior revision documented "Cargo.toml promoted to 1.1.0 with no `v1.1.0`
tag, blocking tag-triggered release." Resolved by subsequent releases: source
tags now exist through `v1.4.0` and public releases `faber-v1.1.1` …
`faber-v1.4.0` are live.

#### B2. Radix Cargo.toml version disconnected from tags — RESOLVED

The prior revision documented `0.38.0` vs a 74-tag retrospective ladder
(`v0.7.0`–`v0.75.0`) with no matching Cargo.toml. Resolved: 30 release-aligned
crates now carry `0.79.0` and tag `v0.79.0` matches; the version-validation
gate passes. The retrospective marker-tag mechanism is historical
(`radix/docs/release/retrospective-minor-tags.md`).

#### B6. Radix release workflow checked out stale faber-runtime — RESOLVED

Fixed in the current workflow: `radix/.github/workflows/release.yml` checks
out only `faberlang/radix`. (Dated 2026-08-07; see §3.3.)

### Open items (current)

#### B3. Cista tag exists; public release not observed — OPEN

Cista `v0.1.0` tag and `cista/docs/release/v0.1.0.md` exist, and the note
claims artifacts "publish to `faberlang/releases` as `cista-v0.1.0`
(multi-arch)". As of 2026-08-07 **no `cista-v0.1.0` release exists** on the
public surface (`gh release view cista-v0.1.0 -R faberlang/releases` →
"release not found"). Either the publish was never executed or it was reverted;
the release note claim is **stale/aspirational**. Routed to the cista owner
(not fixed in this stage — read-only root).

#### B4. No CI for runtime, host, stdlib, or examples repos — OPEN

`faber-runtime`, `hosts`, `norma`, `triga`, and `examples` have no GitHub
Actions workflows. They are consumed as path sources by the Faber build; a
regression in any of them silently breaks Faber builds with no early warning.
Radix's `ci.yml` covers its own validation only.

#### B5. Homebrew install surface — PARTIALLY OPEN

`radix/packaging/homebrew/README.md` names `ianzepp/homebrew-tap`
(`brew install ianzepp/tap/faber`) as the authoritative install surface, and
`radix/scripta/update-homebrew-faber` exists (the prior revision's claim that
the script "does not exist" is stale — dated 2026-08-07). The tap repository
itself is not observable from this workspace; formula freshness vs
`faber-v1.4.0` is unverified.

#### B7. No companion repo version pinning — OPEN

The faber release workflow checks out siblings at default-branch tips; there is
no mechanism to pin companion repos to validated commits. Companion-head
tables are documentary only (`v1.1.1-sibling-pins.md`, `v1.4.0.md`). Two
builds of the same Faber tag at different times can produce different binaries.
This is the campaign's pin-matrix gap (Stage 1 `release-manifest-schema.md`).

#### B8. No single end-to-end release runbook/dry-run — OPEN

Tag creation, version bumping, companion-head recording, artifact publish, and
publish verification remain manual, split across AGENTS prose, workflows, and
this document. This campaign (Stages 1–10) owns the local-first runbook and
dry-run.

#### B9. Checksum-naming gap — OPEN (radix/cista)

Radix and cista workflows write checksum content naming the build path
`dist/<archive>` (`shasum -a 256 "dist/${archive}" > "dist/${archive}.sha256"`),
so ordinary `shasum -c` on a downloaded archive basename fails. Faber's
workflow validates basename-only content (`faber/.github/workflows/release.yml:150-157`).
The site install docs carry a defensive consumer note
(`faberlang.dev/src/en-US/start/install.md:34`).

#### B10. Release CI does not run the expensive gates — OPEN

Faber release CI never runs `./scripta/release-gate`. Radix publish
(`release.yml`) is independent of and not ordered after the tag-triggered
`--full` validation (`ci.yml`). Cista release CI builds + `--version` but skips
its test, lint, hygiene, install, and package-smoke surfaces.

#### B11. Stable-asset immutability gap — OPEN

All three publish jobs upload with `gh release upload … --clobber` (faber
`:223`, radix `:186`, cista `:183`), so published assets are replaceable
today. The campaign treats this as a gap (stable tags/assets must be immutable;
campaign "Candidate, Publication, And Promotion State Machine" and Stop
Conditions).

---

## 5. Target operating model

The prior revision's target operating model remains the campaign's Stage 1–2
direction (local-first proof, dry-run, companion-head recording). Updated
checklist shells for the current releases:

### 5.1 "Operator says release Faber X.Y.Z" (current protocol)

```text
1. PREPARE
   a. Ensure faber/Cargo.toml version = X.Y.Z
   b. Regenerate Cargo.lock (cargo update); lockfile must match the tag
   c. Local gate: ./scripta/release-gate --locked-release-build
      (or ./scripta/release-gate if already built) — the only full-workspace
      cargo test required for a release (faber/AGENTS.md:131-133)
   d. Record companion-head commits for the release note (§3.5)
2. TAG
   a. git tag vX.Y.Z (annotated)
   b. git push origin main && git push origin vX.Y.Z
   → tag push triggers release.yml
3. VERIFY (after CI completes)
   a. Check faberlang/releases has faber-vX.Y.Z with all expected platform archives
   b. Download one archive plus its .sha256 into one directory; verify hash
   c. Faber's workflow validates checksum files name only the archive basename
   d. Run binary --version
4. DOCUMENT
   a. Write docs/release/vX.Y.Z.md with companion heads and evidence
   b. Commit and push release note
```

### 5.2 "Operator says release Radix X.Y.Z" (current protocol)

```text
1. ALIGN VERSION
   a. Bulk bump all release-aligned crates/*/Cargo.toml to X.Y.Z
      (30 crates at 0.79.0 today; hygiene-ratchet excluded)
   b. cargo update; ensure Cargo.lock agrees
2. TAG
   a. git tag vX.Y.Z; git push origin main && git push origin vX.Y.Z
3. VERIFY
   a. ci.yml tag job runs ./scripta/test --full (independent workflow)
   b. Check faberlang/releases has radix-vX.Y.Z; download + verify checksum
      (mind the dist/<archive> checksum naming gap — compare the hash field,
      not the path)
```

### 5.3 "Operator says release Cista X.Y.Z" (current protocol)

Same shape as Radix — standalone build, no sibling checkouts. Note the
unfulfilled `cista-v0.1.0` public claim (§4 B3) must be reconciled before the
next Cista release.

---

## 6. Evidence

### 6.1 Commands and paths inspected (2026-08-07)

```text
# Workflow files
faber/.github/workflows/release.yml
radix/.github/workflows/release.yml
radix/.github/workflows/ci.yml
cista/.github/workflows/release.yml

# Release policy and notes
faber/docs/release/policy.md
faber/docs/release/v1.0.0.md
faber/docs/release/v1.0.0-rc.2.md
faber/docs/release/v1.1.1-sibling-pins.md
faber/docs/release/v1.3.0.md
faber/docs/release/v1.4.0.md
faber/docs/release/v1.5.0-dev-notes.md
faber/docs/release/rc1-local-binary-evidence.md
radix/docs/release/shared-artifact-surface.md
radix/docs/release/retrospective-minor-tags.md
cista/docs/release/v0.1.0.md

# Version sources (observed 2026-08-07)
faber/Cargo.toml                        → version = "1.4.0"
radix/crates/*/Cargo.toml               → 30 crates at "0.79.0"; hygiene-ratchet 0.1.0
cista/Cargo.toml                        → version = "0.1.0"
faber-runtime/Cargo.toml               → version = "0.1.0"

# Dependency / pin manifests
faber/Cargo.toml                        → radix = { path = "../radix/crates/radix" }
faber/core-support-manifest.txt         → path pins (faber-runtime, radix-runtime-contract, hosts/crates/*)

# Git tags (observed 2026-08-07)
faber:    v1.0.0, v1.0.0-rc.2, v1.1.0, v1.1.1, v1.2.0, v1.3.0, v1.4.0
radix:    v0.7.0 … v0.79.0
cista:    v0.1.0

# Public surface (observed 2026-08-07, read-only)
gh release list -R faberlang/releases --limit 100   → 45 releases (4 Faber, 41 Radix; Latest = radix-v0.79.0)
gh release view cista-v0.1.0 -R faberlang/releases → "release not found"
```

### 6.2 Consumers of published URLs/versions/checksums/`latest`

| Consumer | Consumes | Evidence |
| --- | --- | --- |
| `faberlang.dev` install docs | `faber-v1.4.0` tar.gz + sha256 asset URLs | `faberlang.dev/src/en-US/start/install.md:22-58`, `index.md:60` (+ zh-Hans/zh-Hant mirrors) |
| `faberlang.dev` toolchain CLI doc | `faber/docs/release/v1.4.0.md` (frontmatter sources) | `faberlang.dev/src/en-US/toolchain/cli.md:8-10` |
| `ianzepp/homebrew-tap` | formula + reference pack | `radix/packaging/homebrew/README.md:3-11` |
| Shared repo `Latest` | discovery signal (today: `radix-v0.79.0`) | `gh release list -R faberlang/releases` |

### 6.3 Historical key commits (dated evidence)

```text
faber 31dc245   "release: promote Faber to 1.1.0"   (historical; superseded by 1.4.0 line)
faber b771357   "ci(release): add GitHub Actions release workflow"

radix 7c5d2557c  "Release v0.38.0"               (historical version baseline)
radix de80b63cf  "own runtime contract in Radix; faber-runtime to dev-only"
radix ce6030dd4  "eliminate faber-runtime dev-dep; own FrameStatus"
```

---

## 7. Stale-doc disposition summary (2026-08-07)

| Stale claim | Live state | Disposition |
| --- | --- | --- |
| faber/radix matrices include macOS-x86_64 | faber/radix: linux x64 + macos-14 arm64; cista keeps macos-13 x86_64 | updated+dated (§1.1) |
| "No component has a non-release CI workflow" | radix has `ci.yml` | updated+dated (§1.1) |
| Six sibling repos incl. `host-{kernel,native,providers}-rs` | radix (private), cista, faber-runtime, `hosts` monorepo | updated+dated (§1.4) |
| `core-support-manifest.txt` host-providers-rs 7-crate layout | manifest pins `hosts/crates/*` + radix-runtime-contract paths | updated+dated (§1.4) |
| faber 1.1.0 / radix 0.38.0 / cista untagged | faber 1.4.0 / radix 0.79.0 / cista 0.1.0, tags aligned | updated+dated (§2.2, §4) |
| Version-validation gate broken for all three | Gate passes for current tags | demoted to dated history (§2.3, §4) |
| Radix release.yml checks out stale faber-runtime | Removed | updated+dated (§3.3, §4 B6) |
| Cista "never tagged or released" | Tag `v0.1.0` exists; public release absent | updated+dated + routed residual (§4 B3) |
| `scripta/update-homebrew-faber` "does not exist" | Script exists in radix/scripta | updated+dated (§4 B5) |

Historical release notes (`faber/docs/release/v*.md`, `radix/docs/release/v*.md`,
`cista/docs/release/v0.1.0.md`) are untouched.
