# Release process, versioning, and repo interdependencies

**Status:** authoritative COO-scope analysis (head-cpo)
**Scope:** the full faberlang multi-repo ecosystem — release workflows,
versioning policy, dependency graph, and breakage inventory
**Companion docs:** [`policy.md`](policy.md) (normative release lane policy),
[`v1.0.0.md`](v1.0.0.md), [`v1.0.0-rc.2.md`](v1.0.0-rc.2.md)

---

## 1. Current release process (as-built)

### 1.1 Workflow inventory

| Component | `release.yml` | Trigger | Build matrix | Publish target |
| --- | --- | --- | --- | --- |
| **faber** | ✅ `.github/workflows/release.yml` | tag push `v*.*.*` or `workflow_dispatch` | linux-x86_64, macOS-x86_64, macOS-arm64 | `faberlang/releases` as `faber-v*.*.*` |
| **cista** | ✅ `.github/workflows/release.yml` | tag push `v*.*.*` or `workflow_dispatch` | linux-x86_64, macOS-x86_64, macOS-arm64 | `faberlang/releases` as `cista-v*.*.*` |
| **radix** | ✅ `.github/workflows/release.yml` | tag push `v*.*.*` or `workflow_dispatch` | linux-x86_64, macOS-x86_64, macOS-arm64 | `faberlang/releases` as `radix-v*.*.*` |
| **faber-runtime** | ❌ | — | — | — |
| **host-kernel-rs** | ❌ | — | — | — |
| **host-native-rs** | ❌ | — | — | — |
| **host-providers-rs** | ❌ | — | — | — |
| **triga** | ❌ (pure Faber source library) | — | — | — |
| **norma** | ❌ (pure Faber source stdlib) | — | — | — |
| **examples** | ❌ | — | — | — |

Three components have release workflows. Seven do not. No component has a
non-release CI workflow (no test/build/lint on push).

### 1.2 Tag-trigger flow (all three workflows)

```
operator pushes vX.Y.Z tag (or workflow_dispatch with tag)
  → resolve source tag, validate SemVer shape
  → checkout source repo at tag
  → [faber only] checkout sibling repos at default branch
  → validate Cargo.toml version == tag version
  → build --locked --release for each matrix target
  → package binary + SHA-256 + README into tar.gz
  → upload workflow artifact
  → publish job: gh release create/upload to faberlang/releases
    with component-prefixed tag (e.g. faber-v1.0.0)
```

### 1.3 Shared publish surface

All three workflows publish to `faberlang/releases` — a single shared public
release repo. Component prefixes distinguish artifacts:

```text
faber-v1.0.0    → Faber CLI binary + checksums
radix-v0.38.0   → Radix compiler CLI binary + checksums
cista-v0.1.0    → Cista package-store CLI binary + checksums
```

The publish step uses `FABERLANG_RELEASES_TOKEN` secret scoped to create/update
releases in the shared repo.

### 1.4 Faber's multi-repo build (critical constraint)

The faber release workflow is unique: it checks out **six sibling repos** at
their default branch (not pinned to any version):

```yaml
# faber/.github/workflows/release.yml
- faberlang/radix          (private, token)
- faberlang/faber-runtime
- faberlang/host-kernel-rs
- faberlang/host-native-rs
- faberlang/host-providers-rs
```

These siblings are required because:

1. **Cargo path dependency**: `radix = { path = "../radix/crates/radix" }` in
   faber's `Cargo.toml`.
2. **build.rs core-support assembly**: faber's `build.rs` reads
   `core-support-manifest.txt` and bundles source from four host repos into a
   compressed archive embedded in the binary at compile time:

```text
# core-support-manifest.txt
faber-runtime
host-kernel-rs
host-native-rs
host-providers-rs/Cargo.toml
host-providers-rs/crates/aleator
host-providers-rs/crates/http
host-providers-rs/crates/consolum
host-providers-rs/crates/processus
host-providers-rs/crates/solum
host-providers-rs/crates/tempus
```

3. **Radix release workflow** also checks out `faber-runtime`, but this is
   **stale** — radix no longer depends on faber-runtime (see §3.3).

### 1.5 Hygiene ratchet

Both faber and cista vendor a `hygiene-ratchet` dev-dependency
(`crates/hygiene-ratchet`). Radix has its own workspace-level copy. This enforces
code-hygiene invariants during tests but is not part of the release flow.

---

## 2. Versioning approach

### 2.1 Policy framework

[`policy.md`](policy.md) defines a **major-parity** system for the Faber product
line:

- **Odd majors** = development lines (evolving contracts). Faber 1.x is the
  first public development line.
- **Even majors** = language-locked LTS lines. Faber 2 is the first planned LTS.
- `latest` channel must not blur development with LTS.

This policy applies to the **coordinated Faber product line** (CLI, compiler
contract, ReaderPack, standard packages, ABI/wire). It does not directly govern
the Radix compiler version or the Cista package manager version, which have
their own independent version sequences.

### 2.2 Per-component version state

| Component | Cargo.toml | Latest git tag | Tags total | Match? | Published? |
| --- | --- | --- | --- | --- | --- |
| **faber** | `1.1.0` | `v1.0.0` | 2 (`v1.0.0`, `v1.0.0-rc.2`) | ❌ Cargo.toml ahead | source tag only |
| **radix** | `0.38.0` | `v0.75.0` | 74 (`v0.7.0`–`v0.75.0`) | ❌ Cargo.toml behind | source tags only |
| **cista** | `0.1.0` | (none) | 0 | ❌ no tags | never released |
| **faber-runtime** | `0.1.0` | (none) | 0 | n/a | never released |
| **host-kernel-rs** | `0.1.0` | (none) | 0 | n/a | never released |
| **host-native-rs** | `0.1.0` | (none) | 0 | n/a | never released |
| **host-providers-rs** | workspace (7 crates, all `0.1.0`) | (none) | 0 | n/a | never released |

### 2.3 What "1.1" means

The operator says "release 1.1." In practice this touches **three independent
version sequences**:

- **Faber product** (`faber` crate): Cargo.toml is already at `1.1.0`
  (commit `31dc245`), but no `v1.1.0` git tag exists. The release workflow
  cannot trigger. To ship "Faber 1.1," a `v1.1.0` tag must be created.
- **Radix compiler** (`radix` crate): Cargo.toml is stuck at `0.38.0`. The
  74 tags (`v0.7.0`–`v0.75.0`) are **retrospective source-history markers**,
  not release-version indicators (see
  `radix/docs/release/retrospective-minor-tags.md`). They were created
  post-hoc to mark development epochs, not to trigger releases. The Cargo.toml
  version has not been bumped since commit `7c5d2557c` ("Release v0.38.0"),
  which predates tags `v0.39.0` through `v0.75.0`.
- **Cista package manager** (`cista` crate): Cargo.toml is `0.1.0`, never
  tagged, never released. No version coordination exists between Cista and
  Faber.

**Key insight:** "Release 1.1" is a Faber-only statement. It does not imply a
coordinated Radix or Cista release. The Radix compiler that Faber 1.1 builds
against is whatever HEAD of the Radix default branch is at build time — there is
no version pin.

### 2.4 The version-validation gate (and why it breaks)

All three release workflows include a validation step that checks the tag
version against the Cargo.toml `version` field:

```bash
if [[ "$crate_version" != "${version_from_tag}" ]]; then
  echo "tag does not match Cargo.toml version" >&2
  exit 1
fi
```

This gate is correct by design but currently broken for all three components:

- **Faber**: `v1.0.0` tag exists but Cargo.toml is `1.1.0` → tag-trigger fails.
  No `v1.1.0` tag exists → tag-push can't fire at all.
- **Radix**: Cargo.toml is `0.38.0`. Any tag > `v0.38.0` fails validation.
  Tags `v0.39.0`–`v0.75.0` cannot release.
- **Cista**: No tags exist → no release has ever been triggered.

---

## 3. Interdependencies between repos

### 3.1 Dependency graph

```text
faber (1.1.0)
├── radix ← path dep: ../radix/crates/radix
├── [build.rs] core-support assembly ←
│   ├── faber-runtime
│   ├── host-kernel-rs
│   ├── host-native-rs
│   └── host-providers-rs/
│       ├── crates/aleator
│       ├── crates/http
│       ├── crates/consolum
│       ├── crates/processus
│       ├── crates/solum
│       └── crates/tempus
└── [dev] hygiene-ratchet (workspace-internal)

radix (0.38.0)
├── (no faberlang path deps — fully independent)
└── [dev] hygiene-ratchet (workspace-internal: ../hygiene-ratchet)

cista (0.1.0)
└── (no faberlang path deps — fully independent)
    [dev] hygiene-ratchet (workspace-internal: crates/hygiene-ratchet)

faber-runtime (0.1.0)
├── (no external dependencies)
└── workspace member: hosts/llvm
    └── faber-runtime (path dep: ../..)

host-kernel-rs (0.1.0)
└── faber-runtime ← path dep: ../faber-runtime

host-native-rs (0.1.0)
├── faber-runtime ← path dep: ../faber-runtime
└── host-kernel-rs ← path dep: ../host-kernel-rs

host-providers-rs (workspace, 7 crates, all 0.1.0)
└── (internal cross-crate deps; provider-contracts is the shared contract crate)
```

### 3.2 Release order

If a coordinated release were desired:

1. **faber-runtime** (no deps) — ships first; everything else may reference it.
2. **host-kernel-rs** (depends on faber-runtime).
3. **host-native-rs** (depends on faber-runtime + host-kernel-rs).
4. **host-providers-rs** (7 sub-crates; some depend on faber-runtime patterns).
5. **radix** (independent — can ship anytime).
6. **faber** (depends on radix + all host repos via build.rs) — ships last.
7. **cista** (fully independent — ships anytime).

In practice, **only faber, radix, and cista have release workflows**. The host
and runtime repos are consumed as path dependencies and have no independent
release surface. Their "release" is whatever commit is at HEAD when faber's
workflow checks them out.

### 3.3 Radix independence — proven

**Assessment: Radix is fully independent from all sibling repos.**

Evidence:

- **Cargo.toml**: `crates/radix/Cargo.toml` has zero path dependencies. All
  dependencies are external crates (ariadne, thiserror, rustc-hash,
  unicode-ident, unicode-normalization, clap, serde, serde_json, rand,
  getrandom, uuid, toml, wait-timeout, wasm-encoder, wat, wasmtime).
- **Commit history**: the faber-runtime dependency was deliberately eliminated:
  - `de80b63cf` — "refactor(dep): own runtime contract in Radix;
    faber-runtime to dev-only"
  - `ce6030dd4` — "refactor(dep): eliminate faber-runtime dev-dep; own
    FrameStatus in runtime_contract"
- **Stale CI**: the Radix `release.yml` still checks out `faber-runtime` as a
  sibling, but this checkout is now unnecessary — it was not removed when the
  dependency was eliminated. **This is a cleanup item, not a blocker.**

### 3.4 Cista independence — proven

Cista is fully standalone: no path dependencies on any faberlang repo. Its
release workflow checks out only its own source. It can be released independently
at any time.

### 3.5 The companion-head coordination model

The `v1.0.0.md` and `v1.0.0-rc.2.md` release notes record **companion
release-lane heads** — the exact commit hashes of sibling repos at the time the
Faber release was validated:

```text
Radix compiler/host:     247d50785
faber-runtime / LLVM:    b6d1ad3
Host-kernel:             4e6c657
Host-native:             d2d7d4d20
Host providers:          0720a2c
Cista package store:     5bf7a53
Triga geometry/graphics: bbace0d
Examples corpus:         128a40e
```

These are **documentary facts**, not release pins. The faber CI workflow does
**not** pin to these commits — it checks out siblings at default-branch HEAD.
This means a Faber tag release may build against companion repos that have
drifted past their validated companion-head. This is an unmanaged risk.

---

## 4. Broken / incomplete parts inventory

### B1. Faber 1.1.0 has no release tag

**Severity: blocks "release 1.1"**

Cargo.toml was promoted to `1.1.0` (commit `31dc245`, "release: promote Faber to
1.1.0") but no `v1.1.0` git tag was created. The release workflow triggers on
tag push (`v*.*.*`) — without a tag, no release can fire. Using
`workflow_dispatch` with the existing `v1.0.0` tag would fail the version
validation step (`1.0.0` != `1.1.0`).

**Fix**: `git tag v1.1.0 && git push origin v1.1.0` (after validating the build).

### B2. Radix Cargo.toml version disconnected from tags

**Severity: blocks all Radix releases past v0.38.0**

The Cargo.toml version (`0.38.0`) has not been bumped since commit `7c5d2557c`
("Release v0.38.0"). Meanwhile, 37 additional tags (`v0.39.0`–`v0.75.0`) were
created as retrospective source-history markers. The release workflow's version
validation would reject any tag > `v0.38.0` because `0.38.0` != `0.75.0`.

The retrospective tags document (`radix/docs/release/retrospective-minor-tags.md`)
explicitly states these are "marker-only" and "not evidence that binaries were
built or published." This is honest documentation but creates a trap: pushing any
of these tags to trigger the release workflow will fail.

**Fix**: decide whether to (a) bump Cargo.toml to match the next intended release
tag and abandon the retrospective ladder for release purposes, or (b) keep the
retrospective ladder as source markers and pick a new Cargo.toml version for the
next actual release.

### B3. Cista has never been tagged or released

**Severity: no published Cista artifact exists**

A release workflow exists and is structurally correct, but zero tags have ever
been created. Cista has never been released in any form.

**Fix**: validate the build, then tag `v0.1.0` and push.

### B4. No CI for runtime, host, stdlib, or examples repos

**Severity: silent regressions in build-time dependencies**

`faber-runtime`, `host-kernel-rs`, `host-native-rs`, `host-providers-rs`,
`norma`, `triga`, and `examples` have no GitHub Actions workflows at all — not
even test/build CI. These repos are consumed as path dependencies by the Faber
build. A regression in any of them silently breaks Faber builds with no early
warning.

The faber CI workflow checks these repos out at default-branch HEAD during
release, so whatever state they're in at release time is what ships — including
broken state.

**Fix**: add minimal CI (build + test) to each repo, or at minimum to the
repos directly in the faber build path (faber-runtime, host-kernel-rs,
host-native-rs, host-providers-rs).

### B5. Homebrew formula stale / absent

**Severity: no install path for end users**

The Homebrew template at `radix/packaging/homebrew/README.md` references
`ianzepp/homebrew-tap` (`brew install ianzepp/tap/faber`) and an update script
at `scripta/update-homebrew-faber`. Neither the tap repo nor the script exists
locally in the workspace. Campaign notes mention a version lag (0.38.0 vs 1.1.0).

The faber release policy explicitly states v1.0.0 makes "no Homebrew formula"
claim. No binary releases exist to point a formula at. The Homebrew install path
is aspirational, not operational.

**Fix**: after a successful binary release exists in `faberlang/releases`, create
or update the tap formula with correct checksums and version.

### B6. Radix release workflow checks out stale faber-runtime

**Severity: wasted CI time, misleading workflow**

The Radix `release.yml` checks out `faber-runtime` as a sibling, but Radix no
longer depends on it (dependency eliminated in `ce6030dd4`). The checkout is
dead weight.

**Fix**: remove the `faber-runtime` checkout step from Radix's `release.yml`.

### B7. No companion repo version pinning

**Severity: release reproducibility gap**

The faber release workflow checks out six sibling repos at default-branch HEAD.
There is no mechanism to pin companion repos to validated commits. The
companion-head table in `v1.0.0.md` is documentary only — it records what was
validated but does not enforce it in CI.

A Faber tag release at time T builds against whatever companion HEAD is at time
T, which may include unvalidated changes. Two builds of the same Faber tag at
different times can produce different binaries.

**Fix**: either pin companion repos in the release workflow (checkout at a known
commit per release) or accept that faber releases always bundle latest-HEAD
companions and document this explicitly.

### B8. No release checklist or automation script

**Severity: process knowledge is tacit**

No documented checklist exists for "operator says release X.Y → exact steps."
The release policy defines what a release means but not how to execute one. Tag
creation, version bumping, companion-head recording, and publish verification
are all manual, undocumented steps.

**Fix**: create a release runbook (§5 below provides the target operating model).

---

## 5. Target operating model

### 5.1 Release checklist: "operator says release Faber X.Y.Z"

```text
1. PREPARE
   a. Ensure faber/Cargo.toml version = X.Y.Z
   b. Ensure Cargo.lock agrees
   c. Run local gate: cargo test --all --locked && cargo clippy --all
   d. Record companion-head commits for the release note

2. TAG
   a. git tag -a vX.Y.Z -m "Faber X.Y.Z"
   b. git push origin vX.Y.Z
   → tag push triggers release.yml automatically

3. VERIFY (after CI completes)
   a. Check faberlang/releases has faber-vX.Y.Z with all expected platform archives
   b. Download one archive plus its `.sha256` into one directory, run `(cd <download-dir> && shasum -a 256 -c <file>.sha256)`, and run binary `--version`
   c. The `.sha256` line must name only the archive basename (for example: `<64 hex>  faber-vX.Y.Z-aarch64-apple-darwin.tar.gz`), not a CI path like `dist/<archive>`
   d. Faber 1.1.1 checksums were published before this basename-only workflow fix; do not re-tag 1.1.1 without Mind/operator direction.
   e. If Homebrew: update tap formula with checksums

4. DOCUMENT
   a. Write docs/release/vX.Y.Z.md with companion heads and evidence
   b. Commit and push release note
```

### 5.2 Release checklist: "operator says release Radix X.Y.Z"

```text
1. ALIGN VERSION
   a. Bump crates/radix/Cargo.toml version = X.Y.Z
   b. Ensure Cargo.lock agrees

2. TAG
   a. git tag -a vX.Y.Z -m "Radix X.Y.Z"
   b. git push origin vX.Y.Z

3. VERIFY
   a. Check faberlang/releases has radix-vX.Y.Z
   b. Download, verify checksum, run --version
```

### 5.3 Release checklist: "operator says release Cista X.Y.Z"

Same as Radix — standalone build, no sibling checkouts needed.

### 5.4 Recommended ordered fixes

| # | Fix | Priority | Effort | Blocks releases? |
| --- | --- | --- | --- | --- |
| 1 | Tag `v1.1.0` for Faber and push | P0 | 1 tag | **Yes — unblocks "release 1.1"** |
| 2 | Decide Radix version strategy (bump Cargo.toml or reset tag ladder) | P1 | decision + 1 commit | **Yes — unblocks Radix releases** |
| 3 | Remove stale faber-runtime checkout from Radix release.yml | P2 | 1 workflow edit | No |
| 4 | Add build CI to faber-runtime + host repos | P2 | 4 workflow files | No (prevents silent breakage) |
| 5 | Tag `v0.1.0` for Cista and push | P3 | 1 tag | No (first Cista release) |
| 6 | Add companion-repo pinning to faber release workflow | P3 | workflow edit | No (reproducibility) |
| 7 | Create Homebrew tap formula after binary release exists | P4 | formula + tap repo | No (install path) |
| 8 | Write faber `scripta/update-homebrew-faber` script | P4 | 1 script | No |

---

## 6. Evidence

### 6.1 Commands and paths inspected

```text
# Workflow files
faber/.github/workflows/release.yml
cista/.github/workflows/release.yml
radix/.github/workflows/release.yml

# Release policy and notes
faber/docs/release/policy.md
faber/docs/release/v1.0.0.md
faber/docs/release/v1.0.0-rc.2.md
faber/docs/release/rc1-local-binary-evidence.md
radix/docs/release/shared-artifact-surface.md
radix/docs/release/retrospective-minor-tags.md

# Cargo.toml version sources
faber/Cargo.toml                        → version = "1.1.0"
radix/crates/radix/Cargo.toml           → version = "0.38.0"
cista/Cargo.toml                        → version = "0.1.0"
faber-runtime/Cargo.toml               → version = "0.1.0"
host-kernel-rs/Cargo.toml              → version = "0.1.0"
host-native-rs/Cargo.toml              → version = "0.1.0"

# Dependency manifests
faber/Cargo.toml                        → radix = { path = "../radix/crates/radix" }
faber/core-support-manifest.txt         → 4 host repos, 7 sub-crates
faber/build.rs                          → core-support assembly via build.rs

# Git tags
faber:    v1.0.0, v1.0.0-rc.2           (2 tags; no v1.1.0)
radix:    v0.7.0 … v0.75.0              (74 tags; Cargo.toml stuck at 0.38.0)
cista:    (none)
faber-runtime: (none)
host-kernel-rs: (none)
host-native-rs: (none)
host-providers-rs: (none)
```

### 6.2 Key commit evidence

```text
faber 31dc245  "release: promote Faber to 1.1.0"  (Cargo.toml bumped, no tag)
faber b771357  "ci(release): add GitHub Actions release workflow"

radix 7c5d2557c  "Release v0.38.0"               (last Cargo.toml version bump)
radix de80b63cf  "own runtime contract in Radix; faber-runtime to dev-only"
radix ce6030dd4  "eliminate faber-runtime dev-dep; own FrameStatus"
```

### 6.3 Version validation gate (from release.yml)

```bash
crate_version="$(awk '/^\[package\]/{f=1;next}/^\[/&&f{exit}f&&/^version/{gsub(/"/,"",$3);print $3;exit}' Cargo.toml)"
if [[ "$crate_version" != "${version_from_tag}" ]]; then
  echo "tag $tag does not match Cargo.toml version $crate_version" >&2
  exit 1
fi
```

### 6.4 Radix version/tag drift

```text
git tag --sort=-v:refname | head -5    → v0.75.0 v0.74.0 v0.73.0 v0.72.0 v0.71.0
crates/radix/Cargo.toml version        → 0.38.0
git show v0.38.0:Cargo.toml version    → 0.35.0  (tag ≠ Cargo.toml even at tag time)
git show v0.75.0:Cargo.toml version    → 0.38.0  (Cargo.toml unchanged since v0.38.0-era)
```
