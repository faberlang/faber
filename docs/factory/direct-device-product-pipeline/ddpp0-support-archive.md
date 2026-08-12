# DDPP0 Support Archive — LLVM support-archive ABI/content identity + clean-install/core-support/CI/release implications

**Unit**: DDPP0-U8 (lane E — support archive). **Date**: 2026-08-08. **Repo**: faber (control plane).
**Status**: frozen by DDPP0-U8.
**Write scope**: this file only; `faber-runtime/` read-only; no product code; no cargo.
**Frozen here**: the `faber-host-llvm` support-archive ABI version + SHA-256 content
identity, the rebuild / no-last-good-reuse / fail-closed policy, the distinct
support-archive identity domain (cross-referenced to `ddpp0-contract.md`
§IdentityDomains), and the clean-install / core-support / CI / release
implications including the C5 migration-note requirement for DDPP8.

**Authority order** (campaign §Repo-Aware Baseline): live source/tests and live
`faber targets` → accepted artifact schemas + hardware receipts → **this phase's
frozen contracts** → campaign prose. This artifact is the detailed policy of the
support-archive identity domain (row 6 of the §IdentityDomains table in
`ddpp0-contract.md` and the campaign §Artifact identity table) and of the
DDPP3/DDPP8 support-archive gates; it cross-references, never duplicates, the
other DDPP0 artifacts.

---

## SupportArchive

**Frozen**: the LLVM support archive is the static host runtime linked into
faber LLVM products. Its product-side contract is:

- **Archive**: `faber-runtime/hosts/llvm` (`faber-host-llvm` crate,
  `crate-type = ["rlib", "staticlib"]`, dependency `faber-runtime`), producing
  `libfaber_host_llvm.a`. Consumer: `radix/crates/faber/src/package/llvm_host.rs`
  (`ensure_llvm_runtime_archive` builds the archive from the
  `faber-runtime` support sources and links it; the archive identity is
  recorded in the link manifest).
- **Canonical archive bytes**: the deterministic output of the pinned
  toolchain at the pinned support-source revision. The archive build must be
  reproducible (deterministic archive/ranlib flags); run-to-run byte variance
  is treated as identity failure, not tolerated variance.
- **ABI version**: an explicit, single-authority ABI version carried by the
  `faber-host-llvm` crate and embedded in / attributed to the archive
  metadata. It is recorded in the link manifest and the composite manifest
  and is a canonicalization input to the content receipt (§IdentityDomains
  domain rule 3 — ABI-version inputs are normative to the hashes).
- **SHA-256 content receipt**: `support-archive-content-sha256` over the
  **canonical archive bytes** only (the produced `libfaber_host_llvm.a`
  bytes; nothing else). It is a distinct digest from `content_sha256` of
  device artifacts (those cover device payload bytes) and from the
  core-support payload digest (below). No FNV and no mtime-based proxy may
  substitute for it.

Identity pair (frozen): **`support-archive-abi-version`** +
**`support-archive-content-sha256`**. This pair is the support-archive ABI /
content identity domain, distinct from the other five identity domains in
`ddpp0-contract.md` §IdentityDomains (row 6: authority = "Faber build plus
Hosts-owned support source"; migration rule = "versioned ABI plus SHA-256
content receipt; stale last-good archive reuse forbidden"), and is
cross-referenced from §FnvRemoval (domain rule 6). The pair must not be
conflated with `content_sha256` / `packet_sha256` of artifacts or with the
`execution_descriptor_hash` of call/region descriptors.

### Domain rules (normative for this artifact)

1. **The ABI version and the content receipt are independent, both mandatory.**
   A matching ABI version with a mismatched receipt, or a matching receipt
   attributed to the wrong ABI version, is an identity mismatch.
2. **Digest over canonical bytes only.** The receipt covers the canonical
   archive bytes; transport spellings (paths, archive names, base64 or hex
   spellings of the bytes) are never hashed in place of the bytes they carry
   (§CanonicalEncoding round-trip invariant applies by analogy).
3. **No proxy identity.** File existence, path identity, mtime
   ("newest source file is not newer than the archive", the current
   `llvm_host.rs` staleness check) and crate presence are **not** identity.
   The only accepted verification is ABI version + content receipt over
   canonical bytes.
4. **Cross-surface invariant.** Any surface that embeds, links, caches, or
   reuses the archive (link manifest, composite manifest, core-support
   payload, platform cache, release manifest) verifies the same pair; a
   mismatch anywhere fails closed.

---

## RebuildPolicy

**Frozen**:

1. **Faber rebuilds the support archive from the selected support sources.**
   The selected support sources are the pinned `faber-runtime/hosts/llvm`
   sources at the pinned revision (release pinning per
   `release-manifest.yaml` `pinnedInputs.source`), built by the pinned
   toolchain. There is no checked-in, prebuilt, or downloaded archive: every
   product build produces the archive from sources and verifies it.
2. **Stale last-good archive reuse is forbidden.** No silent reuse of a
   previously produced archive on rebuild failure, on missing sources, or on
   identity mismatch. In particular the current last-good fallback in
   `radix/crates/faber/src/package/llvm_host.rs` (rebuild failure → "reusing existing
   archive" warning) is removed; it is the exact behavior this freeze
   prohibits. "Reuse" requires a verified ABI version + content receipt, not
   a fallback path.
3. **Rebuild failure fails closed.** If the support crate cannot be found,
   cargo cannot be executed, the archive is not produced, or the produced
   bytes do not match the declared receipt — the build fails with an
   `llvm_host_runtime_archive_unavailable`-class diagnostic. No warning-and-
   continue.
4. **Identity mismatch fails closed.** If the produced archive's ABI version
   or `support-archive-content-sha256` does not match the declared identity,
   linking fails. Fail-closed applies at every consumer (faber link time,
   core-support materialization, platform-cache admission, release
   verification).
5. **Determinism is a requirement, not an option.** The archive build uses
   deterministic flags; if a toolchain or source change produces byte
   variance that is not an intended content change, the correct response is
   a pinned-toolchain/source fix, never a widened acceptance rule.

DDPP3 gate (campaign §DDPP3): "support-archive ABI/content identity is
recorded and stale archive fallback is unreachable" — this artifact is the
contract that makes that gate mechanically checkable. DDPP8 gate (campaign
§DDPP8): "`src/package/llvm_host.rs` contains no last-good/stale
runtime-archive fallback and every linked support archive has verified
ABI/content identity" — this artifact is that gate's normative source.

---

## CleanInstall

**Frozen — clean-install implications**:

- A clean install (fresh machine or fresh CI runner) must be able to produce
  and verify the support archive from sources: the faberlang container layout
  with the sibling support sources checked out, and the pinned toolchain, are
  prerequisites. Missing sources or toolchain fail closed — they never fall
  back to a cached or embedded archive.
- The LLVM support-archive source crate rides inside the `faber-runtime`
  root of the core-support payload (see §CoreSupport); a clean install that
  receives core-support receives the support sources. The archive itself is
  **not** part of the core-support payload — the payload carries sources and
  other roots; the archive is produced at build time from those sources.
- The platform cache is content-hash-keyed (release-manifest `core-support`
  pack "materialized to the platform cache (content-hash-keyed)"). Cache
  admission for any archived/cached support artifact uses the ABI version +
  content receipt pair, never a path or a "known good" marker.

---

## CoreSupport

**Frozen — core-support implications**:

- **`core-support-manifest.txt` entries.** The current manifest roots
  `faber-runtime` (covering `faber-runtime/hosts/llvm` as a subpath),
  `radix/crates/radix-runtime-contract`, and
  `hosts/crates/{host-kernel,host-native,aleator,http,consolum,processus,solum,tempus}`.
  The support-archive source crate is the `faber-runtime` root's `hosts/llvm`
  subpath today. Two consequences: (a) the support-archive source rides the
  `faber-runtime` manifest root, so the manifest's rerun/watch scope already
  covers it; (b) at DDPP8, when `faber-runtime` is deleted, the
  support-archive source must either gain its own manifest entry or move with
  its accepted owner — the manifest must never silently drop the support
  source. The manifest is the source-of-truth list of what the build assembles
  into the core-support payload; any change to the support archive's origin
  is recorded there first.
- **`faber/build.rs` assembly + `.sha256`.** `build.rs` assembles
  `core-support.tar.zst`, writes `core-support.sha256` (archive digest) and
  `core-support.files.sha256` (per-file digests), and exports
  `FABER_CORE_SUPPORT_SHA256`; it reruns on manifest, root, and file changes.
  The **core-support payload digest is distinct from the support-archive
  content receipt**: the payload digest covers the assembled archive of all
  roots; the support-archive receipt covers the canonical
  `libfaber_host_llvm.a` bytes. Both verify; one does not substitute for the
  other. Build-time verification of the produced archive (ABI version +
  receipt) happens in the faber link path (`llvm_host.rs`), not in the
  core-support assembly.
- `build.rs` is on the DDPP8 deletion-gate surface (§DeletionGate): it must
  not reference `faber-runtime` after deletion, and the support-archive
  production/verification path it feeds must survive the move.

---

## CI

**Frozen — CI implications**:

- **Sibling checkouts.** The release workflow
  (`.github/workflows/release.yml`) checks out `radix`, `cista`,
  `faber-runtime`, and `hosts` as siblings under one parent so the CI
  workspace mirrors the local faberlang/ container layout required by
  `build.rs` and by `llvm_host.rs`'s `../faber-runtime` path. Any CI job
  that builds a faber LLVM host product must have the selected support
  sources checked out at the pinned revision. A missing or wrong-revision
  support source fails the build closed — CI never falls back to a cached or
  committed archive.
- **Verification step.** Release CI verifies the support-archive ABI version
  + content receipt as part of the build/package step (the same
  `llvm_host.rs` verification); the produced `.a`'s receipt is recorded in
  the link manifest, and the package step's checksum is over the packaged
  artifact, not a substitute for the receipt.
- **Capability truth.** `faber targets` continues to report host×device
  capability truth; support-archive identity is part of what the LLVM host
  product row claims, not a separate marketing claim.

---

## Release

**Frozen — release implications**:

- **Release-manifest schema + examples.** The single release-manifest schema
  (`faber/docs/release/release-manifest-schema.md` +
  `release-manifest.schema.json`, first instance `release-manifest.yaml`,
  example `release-manifest.example.yaml`) pins support sources under
  `pinnedInputs.source` (e.g. `faber-runtime` at `10d48ea…`, `hosts` at
  `e066ee…`). The schema **gains the support-archive identity pair** — the
  pinned support sources, the declared ABI version, and the declared
  `support-archive-content-sha256` — as fields of the manifest's pinned
  inputs / packs, and the example manifest records a worked instance. A
  release whose support sources changed but whose manifest did not update the
  identity pair is misdeclared and rejected.
- **Generated Cargo manifests.** Generated package manifests (the
  materialized `support.faber_runtime()` path-link to `faber-runtime` and
  generated Cargo manifests produced by the package path —
  `src/package/cargo.rs`) must reference the versioned support sources and
  carry the support-archive identity pair so generated packages verify the
  support archive they link. Generated manifests that omit or stale the pair
  fail verification (no silent acceptance of a "known" archive).
- **Release notes.** Support-archive ABI version and content receipt changes
  are recorded in the release notes of the release that carries them (the
  per-version notes in `faber/docs/release/`), so consumers can correlate a
  product to its support archive identity.

---

## DeletionGate

**Frozen — DDPP8 deletion-gate references**. The DDPP8 gate surface list is
frozen by DDPP0-U4 §DeletionRule (Cargo.toml, Cargo.lock,
`core-support-manifest.txt`, `build.rs`, generated Cargo manifests, CI sibling
checkouts, release notes, stale-archive fallback). This artifact adds the
support-archive-specific readings of that surface:

| Surface | DDPP8 check (support-archive reading) |
| --- | --- |
| `Cargo.toml` / `Cargo.lock` | no path/version dep on `faber-runtime`/`faber-host-llvm`; the support-archive source rides its accepted owner |
| `core-support-manifest.txt` | the support-archive source entry is present under its new owner or explicitly relocated; never silently dropped |
| `build.rs` | no `faber-runtime` reference; core-support assembly + `.sha256` still cover the support source |
| generated Cargo manifests | support-archive identity pair present; no `faber-runtime` reference |
| CI sibling checkouts | the `faber-runtime` sibling checkout is removed; the new support source sibling is pinned and verified |
| release notes | migration + identity-pair changes recorded for the release |
| stale-archive fallback | `llvm_host.rs` contains no last-good/stale fallback; every linked support archive has verified ABI/content identity (campaign §DDPP8 gate) |
| forwarding crate | no compatibility/forwarding crate preserves the old ownership surface (U4 §DeletionRule; C4) |

The support-archive identity pair survives the DDPP8 move: moving the
support source to its accepted owner does **not** reset or redefine the ABI
version or receipt policy; the pair is a property of the support archive's
content, not of its repo home.

---

## MigrationNote

**Frozen — C5 migration-note requirement (recorded for DDPP8)**:

Before any DDPP8 release work begins, a **migration note** must be produced
covering, at minimum:

1. **Deletion** — what `faber-runtime` deletion means for the LLVM support
   archive (source relocation, `faber-host-llvm` destination per
   DDPP0-U4 §GeneratedRustSupport / §DeletionRule), and the deletion-gate
   surface table above.
2. **Support surfaces** — every consumer of the support archive (faber link
   path, core-support, generated manifests, CI, release verification) and how
   each migrates.
3. **Feature/toolchain changes** — any feature-gate changes (U7
   `ddpp0-feature-isolation.md`) and pinned-toolchain changes that affect how
   the archive is produced and verified.
4. **`core-support-manifest`** — the manifest entries and build.rs assembly
   changes required (see §CoreSupport).
5. **Sibling checkouts** — the CI/release sibling-checkout layout change and
   its verification (see §CI).
6. **No facade** — the migration must not introduce a facade, forwarding
   crate, route alias, or compatibility shim that preserves the old ownership
   surface (U4 §DeletionRule; C4: "faber-runtime is not a rename").

The migration note is a DDPP8 **prerequisite artifact**, not a deliverable of
this unit; this freeze only records the requirement and its coverage list.

---

## Cross-references

- Support-archive identity domain: `ddpp0-contract.md` §IdentityDomains row 6
  + domain rule 6; campaign §Artifact identity table row 6; cross-referenced
  from §FnvRemoval.
- Stale-archive prohibition: campaign §DDPP3 (support-archive ABI version +
  content digest; rebuild from selected support sources; fail closed; no
  silent last-good reuse) and campaign §DDPP8 gate (no last-good/stale
  fallback; verified ABI/content identity).
- DDPP8 gate surface: `ddpp0-contract.md` §DeletionRule (DDPP0-U4).
- Feature isolation: `ddpp0-feature-isolation.md` (DDPP0-U7) — the
  generated-Rust support crate non-pull rule (no transitive device/Hosts)
  keeps the support archive from being a device-runtime backdoor.
- Deletion wording: DDPP0-U4 §DeletionRule records "no universal runtime
  owner" (not "no runtime"); this artifact's migration note follows the same
  wording discipline (C5).
