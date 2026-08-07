# Release-process threat model

**Status:** active — Stage 2 delivery (component-release-streamline)
**Date-stamped:** 2026-08-07
**Purpose:** the release-process failure/abuse surfaces for the coordinated
Faber product line and the radix / cista component surfaces, each with the
**decided control** from the Stage-1 docs.
**Scope:** exactly the four classes the campaign Stage 2 gate names — private
Radix leakage, untrusted build inputs, credential exposure, and asset
replacement. No new policy is invented here; every control maps to an accepted
Stage-1 decision (`release-contract.md`, `release-manifest-schema.md`,
`platform-builder-matrix.md`, `authority.md`, `failure-recovery-matrix.md`,
`process-local-first.md`, `worktree-dry-run-recipe.md`).

---

## 1. Threat model summary

| # | Class | Adversary / failure | Primary impact | Decided control (source) |
| --- | --- | --- | --- | --- |
| T1 | **Private Radix leakage** | private source, paths, tokens, or logs reach public artifacts/receipts/CI logs | private-source disclosure; token theft | leakage gate (`process-local-first.md` §4), extended to receipts + pin matrix (§2) |
| T2 | **Untrusted build inputs** | moving default-branch tips, unverified pins, rogue toolchain/builder | wrong/malicious bytes released as Faber | exact pinned commits (`release-manifest-schema.md` §4), version sources + exclusions (§5), verified provenance (`platform-builder-matrix.md` §3–§4) |
| T3 | **Credential exposure** | signing key or publish token leaks into build envs, logs, or rehearsal envs | forge artifacts / unauthorized publish | least privilege + operator-only secrets (`authority.md` §1–§2, `release-contract.md` §6), no secrets in rehearsal (`worktree-dry-run-recipe.md` §4), compromise path (`failure-recovery-matrix.md` §4) |
| T4 | **Asset replacement** | stable asset overwritten or collided (`--clobber`), tag move, partial upload promoted | consumers install silently-replaced or forged bytes | stable immutability + idempotent retry + fail-closed collision (`release-contract.md` §5.3), no silent `--clobber`/tag move (`failure-recovery-matrix.md` §3), readback-before-promotion (§3) |

---

## 2. T1 — Private Radix leakage

**Surface.** Radix is a private repo consumed as a path dependency
(`radix = { path = "../radix/crates/radix" }`,
`process-versioning-and-deps.md` §3.1). The faber release workflow checks out
private Radix with `FABERLANG_RELEASES_TOKEN` (`faber/.github/workflows/release.yml:69-76`).
Public artifacts, receipts, and CI logs are the exposure surface.

**Abuse / failure scenarios.**

| Scenario | Effect |
| --- | --- |
| A staged archive or receipt embeds private source paths (`/Users/ianzepp/…`, `../radix/…` absolute build paths) | private topology disclosure in a public artifact |
| A receipt or pin table carries private-repo source *content*, markers, or the private clone URL | private-source leakage beyond a path string |
| `FABERLANG_RELEASES_TOKEN` or any token/credential appears in a public log, receipt, or archive | credential theft → unauthorized publish/forge |
| CI logs are published as release evidence | build logs (which can contain paths, env, warnings with source snippets) become public |
| The checksum manifest does not cover exactly the staged artifacts | unsigned/unlisted files can ship, or files outside the staged dir get uploaded |

**Decided control** — the private-radix leakage gate (`process-local-first.md`
§4), extended to receipts and the pin matrix:

1. **Only bytes in the staged dist directory + generated receipts are ever
   uploaded — never build trees** (`process-local-first.md` §4). Everything a
   publisher uploads must come from `dist/` / `out/`, never from a build tree.
2. **The scan rejects:** private source paths (`/Users/ianzepp/…`,
   `../radix/…` absolute build paths), private-repo markers, embedded
   tokens/credentials, and secret URLs. Run it on the staged dist + receipts
   before any public upload; in the rehearsal it runs on `out/`.
3. **CI logs are never published as release evidence** — monitoring output
   (`gh run list`, build logs) is observation, not release evidence.
4. **The signed checksum manifest covers exactly the staged artifacts** —
   basename-only content so consumers can verify a downloaded set
   (`release-contract.md` §5.1, §6).
5. **The pin matrix and receipts leak nothing:** Radix appears as a
   **source-pin only** — commit SHA + the pinned version, never source content,
   never the private clone path, never the token used to clone it
   (`release-manifest-schema.md` §4; rehearsal procedure §3–§4).
6. **Rehearsal environment carries no tokens:** the private checkout is
   rehearsed with an operator-provided local clone or documented as a network
   step — never by placing a private token in the rehearsal env
   (`worktree-dry-run-recipe.md` §4).

**Stop condition:** a leakage-scan hit blocks the upload (and blocks the
rehearsal's `would-upload` plan) until the offending bytes are removed and the
scan re-runs clean. If private Radix cannot be built on a controlled builder
without leakage, stop and route a need for builder placement
(CAMPAIGN "Stop Conditions"; `platform-builder-matrix.md` §3).

---

## 3. T2 — Untrusted build inputs

**Surface.** The release source identity: what exact commits, toolchain, and
builder produced the bytes. The current gap is that faber release CI checks
out siblings at moving default-branch tips (`process-versioning-and-deps.md`
§3.5; `stage0-baseline.md` §3 F4) and `Cargo.lock` does not pin path
dependencies — two builds of the same tag at different times can differ
(`release-manifest-schema.md` §1).

**Abuse / failure scenarios.**

| Scenario | Effect |
| --- | --- |
| A sibling is checked out at a moving default tip instead of a pinned commit | the "same" release is built from different inputs over time — undetectable drift |
| A pin is not verified against live evidence (wrong/forged SHA in the manifest) | a release built from unapproved or malicious source |
| `Cargo.toml` version, source tag, and manifest version disagree | identity mismatch; a wrong artifact published under the release's identity |
| Toolchain/builder provenance is unrecorded | no way to attribute who built or published what; SHA-256 alone authenticates nothing (`release-contract.md` §6 note) |
| An intentional-exclusion row is missed (e.g. `crates/hygiene-ratchet`) | a crate that must stay `0.1.0` gets bumped/released |

**Decided control** — exact pinned inputs + verified provenance
(`release-manifest-schema.md` §4–§5, `platform-builder-matrix.md` §3–§4):

1. **Exact immutable commits for every build/path input** — faber (own tag
   commit), radix, cista, faber-runtime, hosts (`release-manifest-schema.md`
   §4). Both local and CI resolve the same committed manifest
   (`release-manifest-schema.md` §8).
2. **Verifier checks every pin against live evidence before the commit** —
   tag SHA vs remote, version sources (§5), pack digests (§6);
   `release-manifest-schema.md` §7. A pin that fails verification is a hard
   stop.
3. **Authoritative version sources** are the component manifests; the tag must
   equal the manifest version; intentional exclusions are named
   (`release-manifest-schema.md` §5). Mismatch = hard stop at prepare and in CI.
4. **Verified provenance from controlled builders** — the receipt records
   exact source pins, toolchain/SDK versions, target triple, environment
   policy, and normalized archive rules (`platform-builder-matrix.md` §4);
   native builds only (§3). Byte-for-byte equivalence is a recorded non-gate
   aspiration, not a blocker.
5. **Second clean-builder comparison** for stable/LTS product releases — one
   rebuild of the same pinned inputs on a different controlled builder with
   receipts compared (`platform-builder-matrix.md` §3).
6. **No gate runs more than once per release boundary**
   (`process-local-first.md` §3).

**Stop condition:** a supported leg with no reachable controlled builder, or a
manifest that fails validation, stops the release
(`platform-builder-matrix.md` §3; `release-manifest-schema.md` §2).

---

## 4. T3 — Credential exposure

**Surface.** The release signing key (Ed25519 over the checksum manifest) and
the publish token (`FABERLANG_RELEASES_TOKEN`). Today the publish jobs carry
the token as a workflow secret and use `--clobber`
(`faber/.github/workflows/release.yml:197-228`).

**Abuse / failure scenarios.**

| Scenario | Effect |
| --- | --- |
| The signing key lives on a hosted runner or in public-mutation job secrets | a runner compromise can sign arbitrary manifests |
| The publish token is present in a build job or rehearsal env | build/rehearsal exposure can publish without operator action |
| A token or key value appears in logs/receipts/archives | immediate credential theft (also T1) |
| Broad-token access becomes the design (any agent/runner can push) | unauthorized or accidental public mutation |

**Decided control** — least privilege + operator-owned secrets
(`authority.md` §1–§2, `release-contract.md` §6,
`worktree-dry-run-recipe.md` §4):

1. **Signing key held by the tagger/signer role on an operator-controlled
   machine — never on a hosted runner, never in public-mutation job secrets**
   (`release-contract.md` §6).
2. **Publish credentials are least-privilege publication identities isolated
   from product build steps** (CAMPAIGN "Supply Chain And Secret Boundary");
   tagger/signer and publisher roles are operator-only (`authority.md` §1).
3. **No secrets in build or rehearsal environments, logs, or receipts** — a
   dry-run renders `would-sign` / `would-upload` placeholders naming the
   secret *holder*, never a value (`worktree-dry-run-recipe.md` §4;
   `process-local-first.md` §4).
4. **Agent delegation of tagger/publisher/promoter is explicit-deferred** —
   per-release explicit operator authorization only (`authority.md` §5).
5. **Compromised-credential path** (named, mandatory): revoke immediately
   (operator + credential owner), freeze promotion, inventory affected
   releases, publish a verified recovery record, route unverifiable releases
   to withdrawal/supersede (`failure-recovery-matrix.md` §4).

**Stop condition:** any credential value in a build/rehearsal env, log, or
receipt is a leakage-gate failure (T1) and triggers the compromise path if the
value was real.

---

## 5. T4 — Asset replacement

**Surface.** Stable release assets and tags in the shared `faberlang/releases`
repo. Today all three publish jobs re-upload existing assets with `--clobber`
(F4 — `faber/.github/workflows/release.yml:223`, radix `:186`, cista `:183`);
shared-repo `Latest` currently points at a component release
(`stage0-baseline.md` §1.3).

**Abuse / failure scenarios.**

| Scenario | Effect |
| --- | --- |
| A stable asset is re-uploaded with different bytes (`--clobber`) | consumers silently install changed bytes under an immutable-looking version |
| Same-hash retry is treated as unsafe or different-hash retry is treated as safe | idempotency lost, or a collision silently replaces bytes |
| A partial upload is promoted | discovery points at an incomplete release |
| A component release advances the shared `Latest` | the product signal is blurred by a component (`stage0-baseline.md` §7) |
| Stable tag moved or deleted as "rollback" | public evidence destroyed; consumers lose the version's identity |

**Decided control** — immutability + fail-closed collision
(`release-contract.md` §5.3, `failure-recovery-matrix.md` §1–§3):

1. **Stable tags and assets are immutable**; `--clobber` behavior is retired
   as a design (`release-contract.md` §5.3). A changed artifact requires a new
   patch release.
2. **Idempotent retry on identical hashes**; **fail-closed on a collision with
   different hashes** (`release-contract.md` §5.3) — the differing asset is
   inspected, never overwritten.
3. **No partial promotion** — installer/site/package-index/Homebrew/`Latest`
   advance only after all required artifacts pass remote readback
   (`release-contract.md` §4.3).
4. **Global `Latest` reserved for the latest promoted Faber product release;**
   component releases publish with `--latest=false` (`release-contract.md`
   §4.2; the current `radix-v0.79.0` `Latest` is a recorded gap corrected by
   the first promoted product release).
5. **Deletion, tag movement, and stable-asset replacement are never ordinary
   rollback** — the only exceptional path is the incident path: security/legal
   trigger, withdraw/revocation authority, committed incident record,
   discovery updated only in the same operation
   (`failure-recovery-matrix.md` §3).
6. **Same-hash retry is safe; different-hash collision fails closed**
   (failure table, `failure-recovery-matrix.md` §1).

**Stop condition:** any attempted upload whose bytes differ from an existing
asset stops and goes to inspection — never `--clobber`. A rehearsal's
`would-upload` plan must show identical-hash idempotency or fail-closed
language.

---

## 6. Controls that span all four classes

| Control | Classes it serves | Source |
| --- | --- | --- |
| **Operator-only external effects** (tag, publish, promote, withdraw) | T1, T3, T4 | `authority.md` §2 |
| **No silent `--clobber` / no tag move / no stable replacement** | T4, T3 | `release-contract.md` §5.3, `failure-recovery-matrix.md` §3 |
| **Rehearsal = zero public effect, no credentials, `would-*` stops** | T1, T3, T4 | `worktree-dry-run-recipe.md` §1–§5 |
| **Leakage gate before any upload** | T1, T3 | `process-local-first.md` §4 |
| **Exact pins + verified provenance** | T2 | `release-manifest-schema.md` §4–§5, `platform-builder-matrix.md` §4 |
| **Incident record for every exceptional action** | T1, T3, T4 | `failure-recovery-matrix.md` §3–§4 |

---

## 7. References

- `process-local-first.md` §4 — leakage gate (minimum enforced control).
- `release-contract.md` §5.3 (immutability), §6 (authenticity/provenance),
  §4 (channels/Latest).
- `release-manifest-schema.md` §4–§5 (pins, version sources), §7 (update
  authority).
- `platform-builder-matrix.md` §3–§4 (builder trust, provenance).
- `authority.md` §1–§2 (roles, operator-owned production effects), §5
  (deferrals).
- `failure-recovery-matrix.md` §1–§4 (failure table, immutability exceptions,
  compromised credentials).
- `worktree-dry-run-recipe.md` §4 (scrubbed credentials).
- CAMPAIGN "Supply Chain And Secret Boundary".
