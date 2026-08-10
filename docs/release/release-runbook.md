# Faber release runbook (coordinated operator runbook)

**Status:** active — Stage 2 delivery (component-release-streamline)
**Date-stamped:** 2026-08-07
**Purpose:** the cold-operator runbook for the local-first release process —
authority role + exact next command + gates + stop conditions for every
release path, without asking chat.
**Source of truth:** the seven Stage-1 decision docs in this directory
(`release-contract.md`, `release-manifest-schema.md`, `platform-builder-matrix.md`,
`authority.md`, `failure-recovery-matrix.md`, `process-local-first.md`,
`worktree-dry-run-recipe.md`). This runbook cites and sequences those
decisions; it does not re-decide.
**Thin component runbooks:** radix/cista thin runbooks under
`radix/docs/release/` and `cista/docs/release/` are **staged to Stage 3**
(owner: component-release-streamline Stage 3), because they "point to their
local scripts" and no scripts exist until Stage 3. This runbook names the
exact component commands so the campaign Stage 2 gate holds without them.

> **Live-state caveat (2026-08-07):** versions, tags, and workflow shapes cited
> below are the reconciled baseline (`process-versioning-and-deps.md` §2.2,
> `stage0-baseline.md`, observed workflows). Re-verify any fact that would
> change a command before executing it.

---

## 1. The flow (contract level)

The local-first flow, with each step's classification (`process-local-first.md`
§1), the exact operator command, the gate that gates it, and the dry-run stop
point (`worktree-dry-run-recipe.md` §3–§5). Steps 6–8 are operator-authorized
external effects (`authority.md` §2) and are never reached in a rehearsal.

| # | Step | Classification | Exact command(s) | Gate | Dry-run stop point |
| --- | --- | --- | --- | --- | --- |
| 1 | **Prepare** | **local** (lockfile index fetch = **network** unless `--offline`) | version bump (`Cargo.toml`; radix: bulk `perl -pi -e` across `crates/*/Cargo.toml`); `cargo update`; update `faber/release-manifest.yaml` pins + packs (§2.2); draft release notes | manifest validation + pin-vs-live check (`release-manifest-schema.md` §5, §7) | pin matrix + draft plan generated; nothing public |
| 2 | **Local proof** | **local** | `cargo build --locked --release --bin faber` then `./scripta/release-gate --locked-release-build` (`faber/AGENTS.md:126-133`); component gates §2.3; archive + basename-only `.sha256`; leakage scan (`process-local-first.md` §4) | release gate + version/tag gate + locked build; leakage scan clean | gates pass, leakage scan clean |
| 3 | **Tag** | **local**; push = **network** | single bump+lock commit; `git tag -a vX.Y.Z -m "<Component> vX.Y.Z"`; `git push origin main && git push origin vX.Y.Z` | never tag a stale lockfile (`faber/AGENTS.md:141-144`); tag == manifest version | **`would-tag`** — plan lists the annotated tag and its commit; no tag created |
| 4 | **Controlled-builder build** | **controlled-builder** (private radix clone in faber GHA = **network**) | tag push triggers `.github/workflows/release.yml`; monitor `gh run list -R faberlang/faber --limit 1` | matrix legs per `platform-builder-matrix.md` §2 (missing **supported** leg blocks); stable/LTS adds second-builder comparison §3 | **`would-upload`** plan lists the expected matrix legs; nothing runs |
| 5 | **Package + checksum + sign** | **local** (mirror in CI) | archive per matrix; `(cd dist && shasum -a 256 "<archive>" > "<archive>.sha256")` with **basename-only** content (§5.1); detached Ed25519 signature over the checksum manifest (`release-contract.md` §6); provenance/SBOM for stable/LTS product releases | `.sha256` basename check; signed manifest covers exactly the staged artifacts (`process-local-first.md` §4) | **`would-sign`** — plan names the secret *holder*, never a value |
| 6 | **Publish candidate** | **public mutation** (operator only) | draft/candidate namespace upload (§2.5); today: manual `gh release create --draft` (workflow path rejects rc tags) | explicit external-write approval (`authority.md` §2); same hashes as readback target | **`would-upload`** — plan lists the release objects + assets; nothing uploaded |
| 7 | **Readback** | **network** (read-only) | `gh release download <release-tag> --repo faberlang/releases --dir out/readback`; `shasum -c` on the downloaded set; verify detached signature | all required artifacts pass remote readback before promotion (`release-contract.md` §4.3) | documented as the pre-promotion gate; rehearsal stops before |
| 8 | **Promote** | **public mutation** (operator only) | advance candidate → stable; installer/site/package-index/Homebrew/`Latest` metadata **last** (`release-contract.md` §4.2–4.3) | readback green; global `Latest` rule (§2.2/§2.4); no partial promotion | documented as the post-release gate; rehearsal stops before |

Every row above implements `process-local-first.md` §1; the dry-run recipe
(`worktree-dry-run-recipe.md` §3) is the runnable form, operationalized in
`worktree-rehearsal-procedure.md`.

---

## 2. Release paths

For each path: the authority role(s) that may act (`authority.md` §1), the
exact next command(s), the gates that gate that path (`process-local-first.md`
§3), and the stop conditions.

| Path | Authority | Exact next command (cold) | Gate | Stop condition |
| --- | --- | --- | --- | --- |
| **Faber product release** (§2.2) | proposer → builder → verifier → tagger/signer → publisher → promoter | `cargo update` … `./scripta/release-gate --locked-release-build` … `git tag -a vX.Y.Z` … `gh release create faber-vX.Y.Z` | version/tag + locked build + release-gate + matrix §2 + readback | no tag on stale lockfile; no `--clobber`; no Latest before readback |
| **Component-only faber** (§2.1) | proposer → tagger/signer → publisher | same commands minus the product-manifest ceremony | version/tag + locked build + release-gate | same; no Latest promotion without the product form |
| **Radix component** (§2.1) | proposer → builder → tagger/signer → publisher | bulk `perl -pi -e` bump → `cargo build --locked --release -p radix --bin radix` → `git tag -a v0.Y.Z` → `gh release create radix-v0.Y.Z --latest=false` | version/tag + locked build + ladder `--full` at tag | no tag on stale lockfile; publish ordered after full validation (F6); never advance Latest |
| **Cista component** (§2.1) | proposer → builder → tagger/signer → publisher | bump `Cargo.toml` → `cargo build --locked --release --bin cista` → `git tag -a v0.Y.Z` → `gh release create cista-v0.Y.Z --latest=false` | version/tag + smoke (`--version`); test/lint/hygiene/install/package smoke = recorded gap F6 (routed) | reconcile F5 (`cista-v0.1.0` unfulfilled) before next release; never advance Latest |
| **Prerelease / candidate** (§2.5) | proposer → builder → verifier; publish needs explicit external-write approval | local proof + manual `gh release create --draft` (workflow rejects `-rc.N`) | local proof (§1 steps 1–2) | never `Latest`, never site stable |
| **Stable** (§2.4) | full flow; promotion operator-only | §1 steps 1–8 | full matrix + second-builder comparison (stable/LTS product) + readback | no promotion on missing supported leg; no `--clobber` |
| **LTS** (§2.6) | full flow + withdraw/revocation for end-of-support | even-major version; §1 steps 1–8 | lock-transition gate (`policy.md:75-89`) + matrix + readback | support window + end-of-support notice in the release record |
| **Hotfix** (§2.7) | full flow on the target line | patch bump on the target line; §1 steps 1–8 | line's own gates; same as its line's stable | never silently replace stable bytes; supersede via new patch |
| **Abort** (§3) | proposer/verifier decide; operator confirms no push | `git tag -d vX.Y.Z` for **never-pushed** local tags; record partial state | — | never delete or move a pushed tag |
| **Withdrawal / supersede** (§4) | withdraw/revocation operator role | incident note + deprecate/withdraw record + new patch release | incident record + readback of the superseding release | stable bytes stay in place; discovery moves to supersede (§2) |

### 2.1 Component-only releases (faber surface / radix / cista)

**Authority:** proposer, builder, verifier may act locally and by default;
tagger/signer and publisher are **operator-only** (`authority.md` §1–§2).

**Faber surface release** — the faber repo on its own version, the path today's
protocol already implements (`faber/AGENTS.md:122-139`):

```bash
# 1. Prepare (proposer; local)
#    edit Cargo.toml version = "X.Y.Z"; then:
cargo update
#    update faber/release-manifest.yaml pins (see §2.2); draft notes in docs/release/vX.Y.Z.md

# 2. Local proof (builder; local)
cargo build --locked --release --bin faber
./scripta/release-gate --locked-release-build        # only full-workspace gate required
#    optional, only for compiler/corpus claims:
#    ../radix/scripta/test --full   and/or  --e2e

# 3. Tag (tagger/signer; operator; push = network)
git add Cargo.toml Cargo.lock release-manifest.yaml docs/release/vX.Y.Z.md
git commit -m "release: faber vX.Y.Z (version bump + lockfile + manifest pins)"
git tag -a vX.Y.Z -m "Faber vX.Y.Z"
git push origin main && git push origin vX.Y.Z

# 4. Controlled builder (network)
gh run list -R faberlang/faber --limit 1

# 5. Package + checksum + sign (tagger/signer; local; mirrors release.yml:130-158)
#    archive name <component>-v<version>-<triple>.tar.gz; .sha256 content names the basename only

# 6. Publish candidate (publisher; operator only; public mutation)
#    draft/candidate namespace first (§2.5), then promote (§2.4)
```

**Radix component release** — independent version `0.Y.Z`; 30 release-aligned
crates, `crates/hygiene-ratchet` intentionally excluded
(`release-manifest-schema.md` §5; `process-versioning-and-deps.md` §2.2):

```bash
# 1. Prepare (proposer; local)
#    bulk bump per radix/AGENTS.md:375-376, e.g.:
perl -pi -e 's/^version = "0\.79\.0"$/version = "0.80.0"/' crates/*/Cargo.toml
cargo update

# 2. Local proof (builder; local)
cargo build --locked --release -p radix --bin radix   # radix/AGENTS.md:380-381
#    full workspace at the release boundary (auditor-owned): cargo test --workspace;
#    the tag run of ci.yml runs ./scripta/test --full (radix/AGENTS.md:388-389)

# 3. Tag (tagger/signer; operator; push = network)
git add -A && git commit -m "release: radix v0.Y.Z (bulk bump + lockfile)"
git tag -a v0.Y.Z -m "Radix v0.Y.Z"
git push origin main && git push origin v0.Y.Z

# 4. Controlled builder / monitor (network, read-only)
gh run list -R faberlang/radix --workflow ci.yml --limit 5

# 5. Publish (publisher; operator only; public mutation) — component rules:
#    release tag radix-v0.Y.Z with --latest=false (release-contract.md §4.2)
gh release create radix-v0.Y.Z dist/* --repo faberlang/releases \
  --latest=false --title "Radix v0.Y.Z" --notes-file notes.md
```

**Gates:** version/tag gate (`radix/.github/workflows/release.yml:68-86`),
locked build, radix ladder `--full` at tag. **Stop:** never tag a stale
lockfile (`radix/AGENTS.md:392-395`); publish is ordered after the tag's full
validation — the current independence of the two tag triggers is the recorded
gap F6, closed at Stage 8.

**Cista component release** — independent version `0.Y.Z`; binary-only surface
(`release-contract.md` §8):

```bash
# 1. Prepare (proposer; local)
#    edit cista/Cargo.toml version = "0.Y.Z"; then:
cargo update

# 2. Local proof (builder; local)
cargo build --locked --release --bin cista
target/release/cista --version                          # smoke gate (today)
#    test/lint/hygiene/install/package smoke surfaces = recorded gap F6,
#    routed to the cista owner / Stage 8 (process-local-first.md §3)

# 3. Tag (tagger/signer; operator; push = network)
git add -A && git commit -m "release: cista v0.Y.Z (version bump + lockfile)"
git tag -a v0.Y.Z -m "Cista v0.Y.Z"
git push origin main && git push origin v0.Y.Z

# 4. Monitor (network, read-only)
gh run list -R faberlang/cista --limit 1

# 5. Publish (publisher; operator only; public mutation) — component rules:
gh release create cista-v0.Y.Z dist/* --repo faberlang/releases \
  --latest=false --title "Cista v0.Y.Z" --notes-file notes.md
```

**Gates:** version/tag gate (`cista/.github/workflows/release.yml:66-82`),
cista smoke. **Stop:** reconcile the unfulfilled `cista-v0.1.0` public-release
claim (F5, routed to the cista owner) before the next release; never advance
the shared `Latest`.

### 2.2 Faber product release (coordinated; consumes the release manifest)

The coordinated language/toolchain release: faber's own version **plus** the
committed release manifest as the enforced pin story (`release-manifest-schema.md`
§1, §4). This is the form that pins all five source inputs (faber, radix,
cista, faber-runtime, hosts) and the dev-kit payload packs.

- **Prepare step (interlock with faber-onboarding):** the manifest instance
  `faber/release-manifest.yaml` (repo root) is produced by the faber-onboarding
  `assemble-dev-kit` (`faber/docs/factory/faber-onboarding/delivery-stage2.md`),
  which encodes the dev-kit payload as `pinnedInputs.packs` rows of the single
  schema (`release-manifest-schema.md` §6). The runbook **consumes** that
  instance; it never owns or duplicates it. While the onboarding Stage 2
  instance is in flight, prepare records the pack rows as `pending` against
  the §6 shape — never a parallel manifest.
- **Pin verification:** every pin is verified against live evidence before the
  bump commit (verifier role, `release-manifest-schema.md` §7); the
  hand-followable pin-matrix procedure is `worktree-rehearsal-procedure.md`
  §4; Stage 3's `generate-release-manifest` automates it.
- **Publish identity:** `faber-vX.Y.Z` on `faberlang/releases`; global
  `Latest` is reserved for the latest **promoted** product release
  (`release-contract.md` §4.2).
- **Promotion:** installer/site/package-index/Homebrew/`Latest` advance only
  after all required artifacts pass remote readback (`release-contract.md`
  §4.3); stable/LTS product releases add the second clean-builder comparison
  (`platform-builder-matrix.md` §3).

Exact next commands are the faber surface commands in §2.1 with the manifest
steps: after `cargo update`, update `release-manifest.yaml` pins (source §4 +
packs §6), verify them, and commit the manifest in the single bump+lock commit
(`release-manifest-schema.md` §7).

**Consumer smoke-test gate (U3, the P0 guard):** after packaging, every
dev-kit archive must pass the consumer smoke test in a clean prefix (fresh
HOME, minimal PATH, no siblings):

```bash
# Local proof (checklist step 2) — per triple, against the packaged archive:
python3 scripta/smoke-test-release-archive \
  --archive dist/faber-vX.Y.Z-<triple>.tar.gz \
  --version X.Y.Z --triple <triple>
# P0 guard on the previous bare-binary shape (must be REJECTED, pack-error class):
python3 scripta/smoke-test-release-archive \
  --archive dist/faber-vX.Y.Z-<triple>.tar.gz \
  --version X.Y.Z --triple <triple> \
  --expect-fail-class pack-error   # exit 0 only if rejected with pack-error
```

CI runs the smoke test against the packaged archive after the "Package
artifact" step (fails fast before upload). Readback (checklist step 7) is
authoritative: download the public bytes, `shasum -c`, then smoke the
downloaded archive — either explicitly or via the readback mode
(`--download-into out/readback`, which downloads + `shasum -c` + smokes).
Promotion (step 8) is blocked until readback smoke is green.

### 2.3 Gates that gate each path

`process-local-first.md` §3 is the gate mapping:

| Surface | Gate | When |
| --- | --- | --- |
| faber product | `./scripta/release-gate --locked-release-build` | every faber release |
| faber compiler/corpus claims | `../radix/scripta/test --full` / `--e2e` | optional, when claims are included |
| radix component | ladder `./scripta/test --full` at tag; `--stage 1-4` on main | every radix release |
| cista component | smoke (build + `--version`); F6 gap routed | every cista release |
| clean-install / portable | sibling portable gates | called by faber product dry-run **when they exist** |

No gate runs more than once per release boundary
(`process-local-first.md` §3).

### 2.4 Stable

The default release path: §1 steps 1–8, full matrix
(`platform-builder-matrix.md` §2 — a missing **supported** leg blocks the whole
release), second-builder comparison for stable/LTS **product** releases (§3),
remote readback green before promotion (§1 step 7), immutable stable assets
(no `--clobber`, `release-contract.md` §5.3).

### 2.5 Prerelease / candidate

- Version shape `X.Y.Z-rc.N`, channel `candidate`, discovery in the
  candidate/draft namespace (`release-contract.md` §4, §4.1).
- **Today there is no automated candidate build:** the release workflows'
  tag regex `^v[0-9]+\.[0-9]+\.[0-9]+$` rejects `-rc.N` tags
  (`faber/.github/workflows/release.yml:39-57`, radix `:39-57`, cista `:42-58`).
  The candidate path is therefore: local proof (§1 steps 1–2) + manual publish
  of local artifacts into a **draft** release in the candidate namespace
  (`gh release create --draft ...`), with explicit external-write approval.
  The draft/candidate namespace and its automation are Stage 6
  (CAMPAIGN Stage 6).
- **Stop:** candidates never advance `Latest` and never become site stable
  (`release-contract.md` §4).

### 2.6 LTS

- Even-major line per `policy.md:15-28`; the lock-transition gate
  (`policy.md:75-89`) governs entering it. Holds global `Latest` **when
  active** (`release-contract.md` §4.2).
- The release record names the line's support window and end-of-support notice
  (`release-contract.md` §4); the contract does not invent a duration.
- Flow: §1 steps 1–8 with the matrix + second-builder comparison + readback
  requirements of §2.4.

### 2.7 Hotfix / maintenance

- Patch bump on the target line (dev or locked), per `policy.md:50-58`; same
  discovery rules as its line's stable (`release-contract.md` §4).
- **Stop:** a hotfix is a **new patch release** — never an in-place
  replacement of a stable asset (`release-contract.md` §5.3;
  `failure-recovery-matrix.md` §2).

---

## 3. Abort

`failure-recovery-matrix.md` §1 is the decided semantics:

| Failure point | Default response | Exact action |
| --- | --- | --- |
| **Before tag** | abort candidate | Delete/never-create local candidate state (local commit can be dropped — it was never pushed); no public state existed; safe to restart from a new prepare |
| **One of several source tags created** | stop; record partial state | Record which tags exist and which are authoritative (verifier action); publish nothing until source identity is reconciled |

For a **never-pushed** local tag, `git tag -d vX.Y.Z` is safe cleanup. A tag
already pushed is public state: **never delete or move it** (immutability,
`release-contract.md` §5.3; tag movement is never ordinary rollback).

---

## 4. Withdrawal and supersede

Authority: the **withdraw/revocation** operator role (`authority.md` §1).

1. Publish an **incident note** naming the release and the defect.
2. **Withdraw or deprecate** the release record: removal from *discovery*
   (draft/candidate state, or a stable release flagged withdrawn/deprecated).
   Stable bytes are not silently deleted (`failure-recovery-matrix.md` §2).
3. **Supersede** with a new patch release; discovery metadata (installer/site/
   `Latest`) moves to the superseding release only after it passes all gates
   + remote readback.
4. **Compromised credential** (signing or upload): revoke immediately (operator
   + credential owner), freeze promotion, inventory affected releases,
   publish a verified recovery record (`failure-recovery-matrix.md` §4).

---

## 5. References

- `process-local-first.md` — the flow §1, gates §3, leakage gate §4.
- `release-contract.md` — channels §4, immutability §5.3, authenticity §6.
- `release-manifest-schema.md` — pins §4, version sources §5, packs §6, update
  authority §7.
- `authority.md` — role table + operator-owned production effects.
- `platform-builder-matrix.md` — matrix legs + builder trust.
- `failure-recovery-matrix.md` — abort/withdraw/supersede/compromise.
- `worktree-dry-run-recipe.md` + `worktree-rehearsal-procedure.md` — the
  rehearsal form of this runbook.
- `release-checklist.md` — one-line-per-step operator checklist.
- `threat-model.md` — the release-process failure/abuse surfaces.
- `faber/AGENTS.md:122-139`, `radix/AGENTS.md:375-395`, the three `release.yml`
  workflows — the protocol steps this runbook operationalizes.
