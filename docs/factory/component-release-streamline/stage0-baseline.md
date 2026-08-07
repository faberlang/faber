# Stage 0 Baseline — Live Reconciliation Fact Set (Component Release Streamline)

**Campaign**: [component-release-streamline](CAMPAIGN.md) — Stage 0 of 10
**Delivery spec**: [delivery-stage0.md](delivery-stage0.md)
**Status**: delivered 2026-08-07
**Date-stamp**: all facts below were observed on **2026-08-07** against the
live `faber`, `radix`, `cista`, `faber-runtime`, `hosts`, `faberlang.dev`
repos and the public `faberlang/releases` GitHub surface.

**This file is the routing summary, not a competing fact set.** The canonical
reconciled fact set is
[`faber/docs/release/process-versioning-and-deps.md`](../../release/process-versioning-and-deps.md)
(reconciled this stage). Every claim below carries live source evidence
(`path` or `path:line`) and a row classification. Historical claims are dated;
historical release notes (`faber/docs/release/v*.md`, `radix/docs/release/`,
`cista/docs/release/`) are untouched.

---

## Classification legend

Every row in this inventory carries exactly one primary classification, with
notes when a step mixes surfaces:

| Class | Meaning |
| --- | --- |
| **local** | Executes on an operator-controlled machine (burgus / pharos / operator laptop) with no required public effect |
| **controlled-builder** | Executes on a managed build platform (GitHub Actions hosted runner; a future self-hosted runner) — not the operator's machine, but not a public mutation |
| **network** | Requires network access, a private clone, a token, or a remote fetch/push — no public *write* beyond the operator's own refs |
| **public mutation** | Changes externally visible public state (creates/overwrites releases/assets in `faberlang/releases`, advances `Latest`, pushes tags to origin) |

---

## 1. Side-by-side protocol table (faber / radix / cista)

Sources: `faber/AGENTS.md` release protocol (L122–141),
`radix/AGENTS.md` release protocol (L357–391), cista has **no AGENTS.md and no
protocol prose** (workflow-centric; `cista/.github/workflows/release.yml` +
`cista/docs/release/v0.1.0.md`). Workflow steps from
`faber/.github/workflows/release.yml`, `radix/.github/workflows/release.yml`,
`cista/.github/workflows/release.yml`.

### 1.1 AGENTS protocol steps

| # | Step | faber | radix | cista | Classification |
| --- | --- | --- | --- | --- | --- |
| 1 | Version bump | Bump `Cargo.toml` (`version = "X.Y.Z"`) — `faber/AGENTS.md:126` | Bulk `perl -pi -e` across `crates/*/Cargo.toml` — `radix/AGENTS.md:375-376` | No prose protocol; tag + workflow_dispatch — `cista/.github/workflows/release.yml:4-13` | **local** |
| 2 | Lockfile regen | `cargo update` — `faber/AGENTS.md:127` | `cargo update` — `radix/AGENTS.md:378` | n/a (no prose) | **local** (note: hits crates.io index unless `--offline`) |
| 3 | Locked release build | `cargo build --locked --release --bin faber` — `faber/AGENTS.md:129` | `cargo build --locked --release -p radix --bin radix` — `radix/AGENTS.md:380-381` | CI does it (`cargo build --locked --release --bin cista`) — `cista/.github/workflows/release.yml:91` | **local** (CI leg: **controlled-builder**) |
| 4 | Expensive gate | `./scripta/release-gate --locked-release-build` — `faber/AGENTS.md:131-132` | `cargo nextest run` (full workspace) — `radix/AGENTS.md:382-383` | none wired (CI builds + `--version` only) — `cista/.github/workflows/release.yml:90-94` | **local** |
| 5 | Single bump+lock commit | Required, single commit — `faber/AGENTS.md:134-135` | Required, single commit — `radix/AGENTS.md:384-385` | n/a | **local** |
| 6 | Tag | `git tag vX.Y.Z` — `faber/AGENTS.md:136` | `git tag vX.Y.Z` — `radix/AGENTS.md:386` | `git tag v0.1.0` (done 2026-08) — `cista` tags: `v0.1.0` | **local** (tag bookkeeping; push is network) |
| 7 | Push main + tag | `git push origin main && git push origin vX.Y.Z` — `faber/AGENTS.md:137` | same — `radix/AGENTS.md:387` | tag push (implied by trigger) — `cista/.github/workflows/release.yml:5-7` | **network** |
| 8 | Monitor CI | `gh run list -R faberlang/faber --limit 1` — `faber/AGENTS.md:139` | `gh run list -R faberlang/radix --workflow ci.yml --limit 5` — `radix/AGENTS.md:389` | n/a | **network** (read-only) |
| 9 | Lockfile discipline | "Never tag a commit that doesn't include the regenerated lockfile" — `faber/AGENTS.md:141-144` | same — `radix/AGENTS.md:392-395` | n/a | **local** (guardrail) |

### 1.2 Release workflow steps (all three repos)

| # | Step | faber (`release.yml`) | radix (`release.yml`) | cista (`release.yml`) | Classification |
| --- | --- | --- | --- | --- | --- |
| 1 | Trigger | tag `v[0-9]+.[0-9]+.[0-9]+` or `workflow_dispatch` with `tag` input — `:4-13` | identical — `:4-13` | identical — `:4-13` | **network** (operator pushes or calls API) |
| 2 | Resolve source tag + SemVer shape check | `:39-57` | `:39-57` | `:42-58` | **controlled-builder** |
| 3 | Checkout source at tag | `faber` @ tag — `:64-67` | `faberlang/radix` @ tag — `:57-63` | source @ tag — `:59-63` | **controlled-builder** |
| 4 | Checkout siblings at **moving default tips** | radix (private, `FABERLANG_RELEASES_TOKEN`) `:69-76`, cista `:77-81`, faber-runtime `:82-86`, hosts (`mintedgeek/hosts`) `:88-93` | none (own repo only) | none | **network** (private clone for faber) |
| 5 | Validate `Cargo.toml` version == tag | `:97-115` | `:68-86` | `:66-82` | **controlled-builder** |
| 6 | Build matrix | linux x64 + **macos-14 arm64** (Intel removed, queue note `:31-33`) | linux x64 + **macos-14 arm64** (same note `:31-33`) | linux x64 + **macos-13 x86_64** + macos-14 arm64 `:30-37` | **controlled-builder** |
| 7 | Locked release build + `--version` | `cargo build --locked --release --bin faber --target …` `:120-127` | `:89-95` | `:89-94` | **controlled-builder** |
| 8 | Package tar.gz + README + sha256 | `dist/<component>-v<v>-<target>` staging, `shasum -a 256` `:130-158`; **validates checksum file names only the archive basename** `:150-157` | same but checksum written as `shasum -a 256 "dist/${archive}" > "dist/${archive}.sha256"` — **no basename validation** `:110-120` | same as radix, **no basename validation** `:104-115` | **controlled-builder** |
| 9 | Upload workflow artifact | `:160-168` | `:124-133` | `:119-128` | **controlled-builder** |
| 10 | Publish job → `faberlang/releases` | `gh release create` or `gh release upload … --clobber` — `:169-229` (`--clobber` at `:223`) | same — `:137-192` (`--clobber` at `:186`) | same — `:132-188` (`--clobber` at `:183`) | **public mutation** |
| 11 | Readback / verify | none automated (manual: site docs / operator download) | none | none | — |

### 1.3 Publish surface facts

- All three workflows target `RELEASE_REPO: faberlang/releases` with
  component-prefixed release tags: `faber-vX.Y.Z`, `radix-vX.Y.Z`,
  `cista-vX.Y.Z` (faber `:18`, radix `:18`, cista `:18`).
- All use secret `FABERLANG_RELEASES_TOKEN` for the publish job
  (faber `:197`, radix `:166`, cista `:160`).
- **Observed public surface 2026-08-07** (`gh release list -R faberlang/releases
  --limit 100`, read-only): **45 releases** — 4 Faber (`faber-v1.1.1`,
  `v1.2.0`, `v1.3.0`, `v1.4.0`), 41 Radix (`radix-v0.32.0` … `radix-v0.79.0`).
  **`Latest` currently points at `radix-v0.79.0`** — a component release holds
  the shared repo's "Latest" signal, not the product release.
- **`cista-v0.1.0` is NOT present** (`gh release view cista-v0.1.0 -R
  faberlang/releases` → "release not found"). The cista release note claims
  "Public downloadable artifacts publish to `faberlang/releases` as
  `cista-v0.1.0` (multi-arch)" — that claim is **unfulfilled/aspirational**
  (see §7 and the stale-doc ledger §8).

---

## 2. Local script inventory (release-adjacent)

| Script | Repo | Role | What it does | Classification |
| --- | --- | --- | --- | --- |
| `scripta/release-gate` | faber | **gate** (release-only) | Full-workspace nextest closeout; `-- locked-release-build` also builds locked release binary; explicitly forbids agent use without operator request — head comment `scripta/release-gate:3-30` | **local** |
| `scripta/test` | faber | helper (progressive ladder) | Stages 1–3 (`default`/`unit`/`product`); `--check` = stage 1; never full — `scripta/test:1-30` | **local** |
| `scripta/nextest-safe` | faber | helper | macOS nextest-archive signing wrapper to avoid the loader startup wedge; `FABER_SIGNED_NEXTEST=0` bypass — `scripta/nextest-safe:1-25` | **local** |
| `scripta/check-store-only-resolve.sh` | faber | release-adjacent helper | Temp consumer resolves `norma`/`triga` from the package store only (no network); standalone, not referenced by other scripts in-tree | **local** |
| `scripta/test` | radix | **gate** / helper (ladder) | Stages 1–6 (`gate, lint, proba, unit, matrix, parity`) + `--ignored`, `--e2e`, `--release`, `--full`; `scripta/ci` aliases `--full`; `--check` = stages 1–3 — `radix/scripta/test:1-55`, `radix/AGENTS.md:355` | **local** |
| `radix/scripta/…` (75 entries) | radix | helpers | Audit/check/parity helpers; `audit-factory-goal-status.py`, `generate-factory-readme.py`, `check-*-matrix-freshness` etc. — release-adjacent only through the ladder's stage 1–6 composition | **local** |
| — | cista | **none** | No `cista/scripta/` directory exists; no local release-adjacent scripts | — |

**Faber product gate vs radix ladder split:** faber `release-gate` is the only
place full-workspace nextest is required for a product release
(`faber/AGENTS.md:131-133`); the radix ladder (`radix/scripta/test --full`)
covers compiler/corpus claims and is optional for a Faber release
(`faber/AGENTS.md:133-134`). Radix deliberately has **no** local release
helper: "Release automation belongs in the component/product GitHub workflows"
(`radix/AGENTS.md:363-365`).

---

## 3. Pin matrix facts (paths, not SHAs)

1. **`faber/core-support-manifest.txt` pins logical roots by path, not SHA.**
   Content (2026-08-07): `faber-runtime`, `radix/crates/radix-runtime-contract`,
   `hosts/crates/{host-kernel,host-native,aleator,http,consolum,processus,solum,tempus}`
   — relative to the `faberlang/` container. If a sibling path changes, the CI
   checkout steps must be updated to match (`faber/AGENTS.md:117-120,
   160-164`).
2. **`Cargo.lock` does not pin path dependencies.** `radix = { path = "../radix/crates/radix" }`
   resolves to whatever the sibling checkout holds; the locked release build
   freezes registry deps only (campaign CAMPAIGN.md "Release Object Model";
   `faber/Cargo.toml`, `faber/AGENTS.md:58-60`).
3. **Faber release CI checks out siblings at moving default-branch tips**
   (faber `release.yml:69-93`): radix (private), cista, faber-runtime, hosts.
   The same is true of the radix tag CI (`radix/.github/workflows/ci.yml`
   `tag-full` job checks out examples/norma/faber/cista at default tips —
   `ci.yml:128-150`).
4. **No build-consumed pin manifest exists.** Documentary sibling-pin tables
   exist (`faber/docs/release/v1.1.1-sibling-pins.md` — CI main tips at lock
   refresh; `faber/docs/release/v1.4.0.md` "Companion pins" — Radix `v0.79.0`
   `5bbdbbd49`, Cista `99acb1e`, faber-runtime `57493dc`, hosts `ced40f8`),
   but neither the faber nor the radix release/CI workflows consume them.
5. **Live versions** (observed 2026-08-07): Faber `Cargo.toml` `1.4.0` / source
   tag `v1.4.0` (tags: `v1.0.0`, `v1.0.0-rc.2`, `v1.1.0`, `v1.1.1`, `v1.2.0`,
   `v1.3.0`, `v1.4.0`); Radix `0.79.0` across **30 release-aligned crates**
   (31 crates; `crates/hygiene-ratchet` stays `0.1.0`) / tag `v0.79.0`; Cista
   `0.1.0` / tag `v0.1.0`.

---

## 4. Network vs offline classification

### 4.1 Network-required items (named, 2026-08-07)

| Item | Why network | Evidence |
| --- | --- | --- |
| Private radix clone in faber GHA | `actions/checkout` of `faberlang/radix` with `FABERLANG_RELEASES_TOKEN` — private source, token required | `faber/.github/workflows/release.yml:69-76` |
| `gh release create/upload` → `faberlang/releases` | Public publish + `--clobber` overwrite | faber `release.yml:199-228`, radix `:166-188`, cista `:160-183` |
| Remote tag pushes | `git push origin main && git push origin vX.Y.Z` | `faber/AGENTS.md:137`, `radix/AGENTS.md:387` |
| `cargo update` index fetch | crates.io index unless `--offline` | protocol steps (table §1.1 row 2) |
| `gh run list` / `gh release list` monitoring | read-only API | `faber/AGENTS.md:139`, `radix/AGENTS.md:389` |
| `workflow_dispatch` trigger | GitHub API | all three `release.yml:7-13` |

### 4.2 Offline-capable items (burgus / pharos, 2026-08-07)

| Item | Notes |
| --- | --- |
| Local version bump + lockfile regen | `cargo update --offline` caveat: registry cache must exist |
| Locked release build | `cargo build --locked --release --bin faber` (proof of releasability is local — campaign posture) |
| `scripta/release-gate`, `scripta/test` (faber + radix ladders) | Local gates, no CI dependency |
| Archive + checksum | packaging steps are local-capable (today only exercised in CI) |
| Tag bookkeeping | `git tag` local; push is network |
| Store-only package resolution | `scripta/check-store-only-resolve.sh` proves offline resolution |

No fully offline path exists today end-to-end: the publish leg and the private
radix clone are network-required, and no local script produces the release
artifacts outside CI.

---

## 5. Failure modes observed (with evidence)

| # | Failure mode | Evidence |
| --- | --- | --- |
| F1 | **GHA queue / platform drop** — macos-13 Intel dropped from faber + radix matrices because "GitHub-hosted queue is often unbounded" | `faber/.github/workflows/release.yml:31-33`, `radix/.github/workflows/release.yml:31-33`; campaign Problem table (operator-reported timeouts / jobs never leaving queue / actions not firing) |
| F2 | **Lockfile mistakes** — tagging a commit whose lockfile is stale makes `--locked` fail ("cannot update the lock file"); v1.1.1 pins exist because CI main tips moved and `--locked` broke | `faber/AGENTS.md:141-144`, `radix/AGENTS.md:392-395`, `faber/docs/release/v1.1.1-sibling-pins.md` |
| F3 | **Sibling path drift** — `core-support-manifest.txt` path change without CI checkout update breaks the build; hosts layout reorganized (historical host-providers-rs 7-crate layout → hosts monorepo) | `faber/AGENTS.md:117-120,160-164`; stale claims in old `process-versioning-and-deps.md` §1.4/§3.1 |
| F4 | **`--clobber` overwrite risk** — all three publish jobs re-upload existing release assets with `--clobber`; stable assets are replaceable today, which the campaign treats as a gap (stable assets must be immutable) | faber `release.yml:223`, radix `:186`, cista `:183`; campaign "Candidate, Publication, And Promotion State Machine" |
| F5 | **Cista publish claim unfulfilled** — `cista/docs/release/v0.1.0.md` claims artifacts publish to `faberlang/releases` as `cista-v0.1.0`; no such release exists | `gh release view cista-v0.1.0 -R faberlang/releases` → "release not found" (2026-08-07) |
| F6 | **Gates not wired into release CI** — faber release CI never runs `release-gate`; radix publish is not ordered after its independent tag `--full` workflow; cista release CI builds + `--version` but skips test/lint/hygiene/install/package smoke | campaign Problem table (2026-08-07); `radix/.github/workflows/ci.yml` vs `radix/.github/workflows/release.yml` are independent tag triggers |
| F7 | **Checksum-naming gap** — radix/cista `.sha256` files content names `dist/<archive>` (build path), not the downloaded archive basename; faber's workflow validates the basename but the site docs still warn consumers | radix `release.yml:117-121`, cista `:110-115`, faber `:150-157`; `faberlang.dev/src/en-US/start/install.md:34` |
| F8 | **Version/tag validation drift (historical, now aligned)** — version-validation gate previously broken for all three components (faber tag vs Cargo.toml mismatch, radix retrospective tag ladder vs `0.38.0`) | dated history in reconciled `process-versioning-and-deps.md` §2.4/§4 |

---

## 6. Tags / artifacts mutability and readback table (2026-08-07)

| Release | Source tag | Public release tag | Assets | Mutability | Readback state |
| --- | --- | --- | --- | --- | --- |
| Faber 1.4.0 | `v1.4.0` (`faber` repo) | `faber-v1.4.0` on `faberlang/releases` | 2 tar.gz + 2 `.sha256` (linux x64, macos arm64) | **Mutable today**: publish job uploads with `--clobber` on re-run (`release.yml:223`) | Manual only; site docs download and verify; faber checksum content validated basename-only in-workflow (`release.yml:150-157`) |
| Radix 0.79.0 | `v0.79.0` (`radix` repo) | `radix-v0.79.0` on `faberlang/releases` | 2 tar.gz + 2 `.sha256` | **Mutable today**: `--clobber` (`release.yml:186`) | Manual only; `.sha256` content names `dist/<archive>` — **checksum-naming gap** |
| Cista 0.1.0 | `v0.1.0` (`cista` repo) | **none observed** (`cista-v0.1.0` not found) | — | n/a | n/a — publish never executed or never surfaced |

`workflow_dispatch` path exists in all three workflows (release an existing
source tag without a new tag push): faber `release.yml:7-13`, radix `:7-13`,
cista `:7-13`.

---

## 7. Consumers of published URLs / versions / checksums / `latest` (2026-08-07)

| Consumer | What it consumes | Evidence |
| --- | --- | --- |
| `faberlang.dev` install docs | `faber-v1.4.0` tar.gz + `.sha256` direct GitHub asset URLs; release page link | `faberlang.dev/src/en-US/start/install.md:22-58` (also `index.md:60`, and zh-Hans/zh-Hant mirrors of `start/install.md`, `start/index.md`, `reference/releases.md`, `reference/design.md`) |
| `faberlang.dev` toolchain CLI doc | `faber/docs/release/v1.4.0.md` via frontmatter `sources` | `faberlang.dev/src/en-US/toolchain/cli.md:8-10` |
| `faberlang.dev` release history page | Fabers' release records | `faberlang.dev/src/en-US/reference/releases.md` |
| `ianzepp/homebrew-tap` (authoritative install surface per radix packaging) | `brew install ianzepp/tap/faber` — formula + reference pack | `radix/packaging/homebrew/README.md:3-11` |
| `radix/docs/release/shared-artifact-surface.md` | Documents shared-surface tags + `FABERLANG_RELEASES_TOKEN` contract | `radix/docs/release/shared-artifact-surface.md` |
| `faber/docs/release/v1.4.0.md` | Companion pins (Radix `v0.79.0`, Cista `99acb1e`, faber-runtime `57493dc`, hosts `ced40f8`) — consumed by site cli.md + future release records | `faber/docs/release/v1.4.0.md:63-72` |
| `cista/docs/release/v0.1.0.md` | Claims the `cista-v0.1.0` public surface (unfulfilled) | `cista/docs/release/v0.1.0.md` |
| Shared repo `Latest` signal | Currently `radix-v0.79.0` — any consumer resolving "latest Faber" from the shared repo gets a component | `gh release list -R faberlang/releases` (2026-08-07) |

---

## 8. Stale-doc disposition ledger (2026-08-07)

Dispositions: **updated+dated** (live claim corrected in place with a date),
**demoted** (claim moved to a dated history section, not rewritten),
**routed** (claim lives in a read-only/forbidden root — recorded here, fixed by
its owner in a later stage).

| Stale claim (where it lives) | Live state | Disposition |
| --- | --- | --- |
| `process-versioning-and-deps.md` §1.1: faber/radix matrices include macOS-x86_64 | faber/radix = linux x64 + macos-14 arm64 (Intel dropped); cista retains macos-13 x86_64 | **updated+dated** (reconciled doc) |
| §1.1: "No component has a non-release CI workflow" | radix has `ci.yml` (main → stage 1-4, tag → `--full`) | **updated+dated** |
| §1.4: faber release checks out six repos incl. `host-kernel-rs`/`host-native-rs`/`host-providers-rs` | Live siblings: radix (private), cista, faber-runtime, `mintedgeek/hosts` monorepo | **updated+dated** |
| §1.4: `core-support-manifest.txt` lists host-providers-rs 7-crate layout | Live manifest pins `faber-runtime`, `radix/crates/radix-runtime-contract`, `hosts/crates/{8 crates}` | **updated+dated** |
| §2.2: faber `1.1.0`/tags `v1.0.0`, `v1.0.0-rc.2`; radix `0.38.0`/74 tags; cista never tagged | faber `1.4.0`/`v1.4.0`; radix `0.79.0` (30 release-aligned crates)/`v0.79.0`; cista `0.1.0`/`v0.1.0` | **updated+dated** |
| §2.3/§2.4 + §4 B1/B2: version-validation gate broken for all three; faber 1.1.0 untagged | Versions and tags now aligned; gate works for current tags | **demoted** (dated history; the analysis is retained as history, not current fact) |
| §3.3 + §4 B6: radix `release.yml` checks out stale faber-runtime | Radix `release.yml` checks out only `faberlang/radix` | **updated+dated** (item already fixed; text dated) |
| §4 B3: "Cista has never been tagged or released" | Cista tag `v0.1.0` exists; public `cista-v0.1.0` still absent | **updated+dated** + residual routed to cista owner (publication not executed) |
| §4 B5: homebrew tap + update script "neither exists locally" | `radix/scripta/update-homebrew-faber` exists; `radix/packaging/homebrew/README.md` names `ianzepp/homebrew-tap` as authoritative | **updated+dated**; tap repo itself not observable from this workspace |
| §3.5: companion-head model ("documentary facts, not pins") | Still true; the faber release CI still checks out siblings at default tips | **retained** (still current), refreshed evidence |
| `cista/docs/release/v0.1.0.md` claims `cista-v0.1.0` publishes to `faberlang/releases` | No such release observed | **routed** (read-only root; recorded in §5 F5 and in the cista-residual of the reconciled doc — cista owner to fix) |
| `faberlang.dev/src/en-US/start/install.md:34` consumer caution about checksum paths | Current faber workflow enforces basename-only; site note is defensive/stale-ish | **routed** (faberlang.dev is a forbidden root in this stage; not edited — recorded) |

Historical release notes (`faber/docs/release/v*.md`, `radix/docs/release/v*.md`,
`cista/docs/release/v0.1.0.md`) are **untouched** per the delivery spec.

---

## 9. Stage 1 open-decision handoff (B11)

Campaign Open Questions → Stage 1 artifacts, with markings:
**answered-by-evidence** (Stage 0 fact), **carried-to-stage-1** (context
gathered, decision pending), **needs-stage-1-decision** (explicit choice).

| # | Open question | Stage 1 artifact | Marking |
| --- | --- | --- | --- |
| 1 | Version/compatibility: independent SemVer + product pins vs lockstep; prerelease/hotfix/LTS channels | `release-contract.md` | **needs-stage-1-decision** |
| 2 | Product input manifest: schema, location, update authority; pins repos only or also package/reference/locale inputs | `release-manifest-schema.md` | **answered-by-evidence** (no build-consumed pin manifest; `core-support-manifest.txt` pins paths, `Cargo.lock` doesn't pin path deps) → schema itself **needs-stage-1-decision** |
| 3 | Platform/support matrix: which targets supported/experimental/deferred; which missing leg blocks promotion | `platform-builder-matrix.md` | **answered-by-evidence** (observed matrices 2026-08-07: faber/radix linux x64 + macos-14 arm64; cista + macos-13 x86_64) → acceptance **needs-stage-1-decision** |
| 4 | Builder trust: burgus/pharos/self-hosted/hosted per target; native vs cross; second-builder comparison | `platform-builder-matrix.md` | **carried-to-stage-1** (F4/F5 network items named in §4) |
| 5 | Artifact authenticity: signed checksum manifest, signatures, platform signing, provenance, SBOM | `release-contract.md` | **needs-stage-1-decision** |
| 6 | Stable immutability: absolute no-replacement vs exceptional incident authority | `release-contract.md` + `failure-recovery-matrix.md` | **needs-stage-1-decision** (current `--clobber` is the documented gap — F4) |
| 7 | Channels/discovery: candidate namespace, shared-repo `Latest`, dev vs LTS, atomic promotion | `release-contract.md` | **needs-stage-1-decision** (evidence: `Latest` = `radix-v0.79.0` today — F5/§7) |
| 8 | Public host: keep `faberlang/releases`, alternate storage, or mirror | `release-contract.md` | **carried-to-stage-1** (current host observed; no decision needed from Stage 0) |
| 9 | Production authority: which human role tags/publishes/promotes/withdraws/revokes | `authority.md` | **needs-stage-1-decision** |
| 10 | Cista surface: binary-only component release vs crates.io library; how Faber pins/consumes | `release-contract.md` | **carried-to-stage-1** (evidence: binary-only today, no crates.io; `cista-v0.1.0` publish unfulfilled — F5) |

## 10. Council-4 interlock (recorded, not resolved)

`faber/docs/release/` is the coordinated product process home (campaign
"Authority And Durable Homes"). **Faber-onboarding** Stage 1/2 produces a
dev-kit payload manifest (every component, version, digest, compatibility
bound, license, destination) in the same directory; this campaign's Stage 1
produces the release manifest schema there. A **single routing authority for
`faber/docs/release/`** must be decided at the campaigns' Stage 1 planning
(council-4) before their decision outputs overlap. Stage 0 records the overlap
only; no shared schema is pre-written. References:
`faber/docs/factory/faber-onboarding/CAMPAIGN.md`,
`faber/docs/factory/faber-onboarding/delivery-stage0.md`.

---

## 11. Stage 0 scope confirmations (B12)

- No product code, no scripts authored, no release executed, no tag pushed,
  no workflow run, no cargo invocation, main untouched except docs.
- Write scope: this file + `faber/docs/release/process-versioning-and-deps.md`
  (reconciled) + the pre-existing uncommitted `CAMPAIGN.md` revision
  (committed per planner residual) + regenerated factory README.
- `src/package/{compile,rust_target,wasm}.rs` and `crates/exempla/*` foreign
  WIP untouched (hand-4 U6-D surface).
