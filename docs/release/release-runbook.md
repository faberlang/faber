# Faber release runbook (coordinated operator runbook)

**Status:** active — EL-6 rewrite (per-lane-e2e-validation; tag-only-on-green)
**Date-stamped:** 2026-08-11
**Purpose:** the cold-operator runbook for the local-first release process —
authority role + exact next command + gates + stop conditions for every
release path, without asking chat.
**Source of truth:** the seven Stage-1 decision docs in this directory
(`release-contract.md`, `release-manifest-schema.md`, `platform-builder-matrix.md`,
`authority.md`, `failure-recovery-matrix.md`, `process-local-first.md`,
`worktree-dry-run-recipe.md`), plus the per-lane e2e grid
(`docs/factory/per-lane-e2e-validation/`). This runbook cites and sequences
those decisions; it does not re-decide.
**Thin component runbooks:** radix/cista thin runbooks under
`radix/docs/release/` and `cista/docs/release/` are **staged to Stage 3**
(owner: component-release-streamline Stage 3), because they "point to their
local scripts" and no scripts exist until Stage 3. This runbook names the
exact component commands so the campaign Stage 2 gate holds without them.

> **Live-state caveat (2026-08-11):** versions, tags, and workflow shapes cited
> below are the reconciled baseline (`process-versioning-and-deps.md` §2.2,
> `stage0-baseline.md`, observed workflows). Re-verify any fact that would
> change a command before executing it.

---

## 0. Release posture (tag only on a green main)

**Tag only on a main tip the lane grid (or a CI re-run on the exact commit)
declared green.** Standing evidence lives in
`docs/factory/per-lane-e2e-validation/grid-status.md` (Tier 1 report-only
grid, EL-5). If the grid has not yet declared the tip green, re-run CI (or
the relevant per-lane commands) on that exact commit and record the green
result before tagging. Never tag a tip whose grid status is red, skipped, or
unknown.

**Local release is short:** bump → pinned-sibling lock regen → commit → tag →
push. The slow correctness work moved to the nightly per-lane grid on
pharos; local release is minutes of bookkeeping, not a day of e2e on the
dev machine.

```bash
# 0. Green-main gate (operator; read-only)
#    Confirm grid-status.md (or CI re-run on this exact commit) is green.
# 1. Prepare on a pin packet (worktree-rehearsal-procedure.md), never main tree:
#    edit Cargo.toml version = "X.Y.Z"
#    update release-manifest.yaml pins + packs
./scripta/regen-lock --pinned-siblings          # ONE command (L2 + F2)
# 2. Single bump+lock commit (local to the release branch / main tip)
git add Cargo.toml Cargo.lock release-manifest.yaml docs/release/vX.Y.Z.md
git commit -m "release: faber vX.Y.Z (version bump + lockfile + manifest pins)"
# 3. Cheap stage-1 gates on the exact tip about to be tagged (L3)
./scripta/test                                  # faber stage 1
# 4. Tag + push as SEPARATE commands (L4)
git tag -a vX.Y.Z -m "Faber vX.Y.Z"
git push origin main
git push origin vX.Y.Z
```

**One-command pinned-sibling rehearsal** (extends `scripta/regen-lock`):

| Command | Effect |
| --- | --- |
| `./scripta/regen-lock --pinned-siblings` | Verify `../radix`, `../cista`, `../faber-runtime`, `../hosts` match `release-manifest.yaml` source pins, then `cargo update` + freshness check. Writes only the `--root` (pin-packet) lock. |
| `./scripta/regen-lock --pinned-siblings --check` | Pin-match + freshness only; never writes, never runs cargo. |

Run it **inside a pin packet** (siblings detached at the pinned SHAs per
`worktree-rehearsal-procedure.md`). A pin mismatch is a hard stop — that is
the L2 trap: regenerating the lock against drifted local siblings produces a
lock CI cannot build. The command is worktree-scoped (`--root`); it never
mutates another checkout's lock.

`release-gate` remains the expensive product closeout (operator-only at a
real release boundary). It is **not** part of the short local path above and
is forbidden as an agent default.

---

## 1. The flow (contract level)

The local-first flow, with each step's classification (`process-local-first.md`
§1), the exact operator command, the gate that gates it, and the dry-run stop
point (`worktree-dry-run-recipe.md` §3–§5). Steps 6–8 are operator-authorized
external effects (`authority.md` §2) and are never reached in a rehearsal.

| # | Step | Classification | Exact command(s) | Gate | Dry-run stop point |
| --- | --- | --- | --- | --- | --- |
| 0 | **Green-main gate** | **local** (read-only) | confirm `grid-status.md` (or CI re-run on the exact tip) is green for the commit about to be tagged (§0) | tag only on a main tip the lane grid / CI re-run declared green | tip green recorded; nothing public |
| 1 | **Prepare** | **local** (lockfile index fetch = **network** unless `--offline`) | version bump (`Cargo.toml`; radix: bulk `perl -pi -e` across `crates/*/Cargo.toml`); update `faber/release-manifest.yaml` pins + packs (§2.2); **`./scripta/regen-lock --pinned-siblings`** inside the pin packet (one-command L2+F2; never `cargo update` on the main tree); draft release notes | manifest validation + pin-vs-live check (`release-manifest-schema.md` §5, §7); pin match is compatibility proof, not existence | pin matrix + draft plan generated; nothing public |
| 2 | **Local proof** | **local** | `cargo build --locked --release --bin faber` **against the pinned siblings** (pin packet); optional `./scripta/release-gate --locked-release-build` at a real product boundary only; component gates §2.3; archive + basename-only `.sha256`; leakage scan (`process-local-first.md` §4) | locked build on pins; leakage scan clean; release-gate only when the product closeout is in scope | gates pass, leakage scan clean |
| 3 | **Tag** | **local**; push = **network** | single bump+lock commit; cheap stage-1 gates on the exact tip (L3); `git tag -a vX.Y.Z -m "<Component> vX.Y.Z"`; push main then push tag as **separate** commands (L4) | never tag a stale lockfile; tag == manifest version; **tag only on a green main tip** (§0) | **`would-tag`** — plan lists the annotated tag and its commit; no tag created |
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

**Faber surface release** — the faber repo on its own version. Post-EL-6 the
short path is bump → regen-lock (pinned siblings) → commit → tag → push on a
green main tip (`faber/AGENTS.md` Release protocol):

```bash
# 0. Green-main gate (operator; read-only) — §0
#    grid-status.md (or CI re-run on this exact commit) must be green.

# 1. Prepare (proposer; local; INSIDE the pin packet, never the main tree)
#    edit Cargo.toml version = "X.Y.Z"
#    update faber/release-manifest.yaml pins (see §2.2); draft notes in docs/release/vX.Y.Z.md
./scripta/regen-lock --pinned-siblings          # one-command pinned-sibling rehearsal

# 2. Local proof (builder; local; against the pinned siblings)
cargo build --locked --release --bin faber
#    product closeout (operator-only real release boundary, not the short path):
#    ./scripta/release-gate --locked-release-build
#    optional compiler/corpus claims: ../radix/scripta/test --full and/or --e2e

# 3. Tag (tagger/signer; operator; push = network; SEPARATE commands — L4)
git add Cargo.toml Cargo.lock release-manifest.yaml docs/release/vX.Y.Z.md
git commit -m "release: faber vX.Y.Z (version bump + lockfile + manifest pins)"
./scripta/test                                  # stage-1 on the exact tip (L3)
git tag -a vX.Y.Z -m "Faber vX.Y.Z"
git push origin main
git push origin vX.Y.Z

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

## 6. Incident lessons — faber 1.6.0 / radix 0.81.0 (2026-08-10)

Pre-campaign capture of what broke on the 2026-08-10 release pair and the
fix-forward gate each failure maps to. **Superseded in protocol by EL-6**
(this rewrite): tag-only-on-green (§0), one-command
`./scripta/regen-lock --pinned-siblings` (L2), and the nightly per-lane grid
(EL-5). Keep the rows as the evidence base; the gates below remain the
minimum whenever the grid is unavailable.

### L1 — Carried-forward sibling pins can exist yet be incompatible

`release-manifest.yaml` carried the rc.1-era faber-runtime (`10d48ea`) and
hosts (`e066ee0a`) pins into the 1.6.0 manifest. Both SHAs resolved
(`git cat-file -e` passes), but neither satisfied current faber: the pinned
faber-runtime lacked the `model_widen`/`model_format` modules (E0432) and the
pinned hosts lacked `program_graph_hash` on `DeviceExecutionReceipt` (E0609).
The pinned release build failed; the local build had been green all session
only because local siblings were newer than the pins.

**Gate:** pin verification is **compatibility**, not existence. The pinned
locked build (`cargo build --locked --release --bin faber` against the pin
packet, rehearsal §3 step 3) is the proof; `git cat-file -e` is a necessary
pre-check only. When carrying forward the last record's companion pins,
diff each sibling's pinned SHA against the SHA the last green build actually
used — a carried pin that is *older* than the proven pairing is stale.

### L2 — The lockfile must be generated against the pins, not local siblings

The 1.6.0 `Cargo.lock` was regenerated with `cargo update` in the main tree,
resolving against local (newer) siblings. CI checks out the **pinned**
siblings, so the `--locked` build failed there ("cannot update the lock file
because --locked was passed") even though the local locked build was green.
The committed lock was in fact correct for the corrected pins — the CI
failure was pin staleness, not lock drift — but the two can silently disagree.

**Gate:** regenerate the lock **inside the pin packet** with the one-command
rehearsal — `./scripta/regen-lock --pinned-siblings` — never `cargo update`
in the main tree. The rehearsed lock is the one that commits. The command
hard-stops on a pin mismatch so a drifted sibling layout cannot silently
produce a CI-failing lock.

### L3 — Verify the exact tag tip before creating the tag

The radix `v0.81.0` tag was created on a commit whose stage-1 gate was red
(stale factory README: `docs(factory)` goals landed after the last green run
without regenerating the README). CI reported it post-tag; fixing it required
a post-tag commit and, on faber, tag surgery. The faber tag had to be
force-moved twice — operator-only, hook-blocked, and confusing under the
shared-workspace git policy.

**Gate:** run the cheap stage-1 gates (`./scripta/test --check` /
`generate-factory-readme.py --check` + the goal-status audit) on the **exact
commit about to be tagged**, after the bump+lock commit, before `git tag -a`.
A release tip must be green before the tag exists, because moving a pushed tag
is not ordinary rollback.

### L4 — Keep release git commands granular (shared-workspace policy)

A single chained command containing a blocked destructive op (`git push
--force`) was denied **wholesale** by the shared-workspace git policy — the
`git add`, commit, tag, and main push in the same chain never ran, silently.
The "commit happened" assumption then produced a tag pointing at the old
commit and an "everything up-to-date" force-push.

**Gate:** release git writes run as **separate commands** — `git add` +
`git commit`, then `git tag -a`, then `git push origin main`, then the tag
push — so a blocked op fails loudly and atomically, and its absence is
visible. Never infer that earlier commands in a chain ran because the final
error names a later one.

### L5 — Linux leg when bypassing CI (local container build)

The `x86_64-unknown-linux-gnu` artifact was built on burgus in an amd64
container (the rc.1 recipe): `docker run --rm --platform linux/amd64 -v
<pin-packet>:/work -w /work/faber -e CARGO_TARGET_DIR=/work/target-linux
amd64/ubuntu:24.04 bash -lc '… build-essential pkg-config curl … rustup
--default-toolchain 1.97.1 … cargo build --locked --release --bin faber'`.
Two gotchas: `--platform linux/amd64` is required on Apple Silicon (the plain
image has no arm64 manifest), and with `CARGO_TARGET_DIR` set the binary is at
`$CARGO_TARGET_DIR/release/faber`, not `./target/release/faber`. Verify the
artifact with `file` (ELF 64-bit x86-64) since the binary cannot run on macOS.

### L6 — Slow gates surfaced latent defects at release time

The e2e harnesses — excluded from the dev cycle because they are slow — were
the first thing to run the `conversio-assign` exemplar across backends and
found three real codegen bugs (go `strconv` import, ts typed-init, swift
double-eval) plus a forma round-trip non-idempotency. That is the core
failure this campaign fixes: nothing validates between releases, so release
time is discovery time. See the `per-lane-e2e-validation` goal for the model.

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
  rehearsal form of this runbook (pin packet + one-command regen-lock).
- `release-checklist.md` — one-line-per-step operator checklist.
- `threat-model.md` — the release-process failure/abuse surfaces.
- `docs/factory/per-lane-e2e-validation/grid-status.md` — standing green/red
  for main (EL-5); the tag-only-on-green gate input.
- `faber/scripta/regen-lock --pinned-siblings` — one-command pinned-sibling
  lock rehearsal (EL-6).
- `faber/AGENTS.md` / `radix/AGENTS.md` Release protocol sections — the
  short-form protocol this runbook operationalizes.
- the three `release.yml` workflows — controlled-builder build after tag.
