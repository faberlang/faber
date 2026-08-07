# Faber release manifest schema

**Status:** accepted — Stage 1 decision record (component-release-streamline)
**Date-stamped:** 2026-08-07
**Resolves:** campaign Open Question 2 (product input manifest: schema,
location, update authority, pinned inputs, exclusions)
**Companion:** [`release-contract.md`](release-contract.md) (release contract,
channels, identity, authority context), `process-versioning-and-deps.md`
(canonical fact set), `../factory/faber-onboarding/package-and-lock-contract.md`
(sibling: package **semantics** — the distinction below is recorded, not
merged).

> This document defines the schema and its governing decisions. **No instance
> file is authored here** and no implementation exists yet — the instance is
> produced and consumed by Stage 3 (harness) and Stage 4 (packaging). Both
> local and CI builds consume the same committed manifest by design.

---

## 1. Purpose

A committed, machine-readable **input/pin manifest** that a release intent
locks to. It exists because today there is **no build-consumed pin manifest**:
`core-support-manifest.txt` pins logical paths (not SHAs), `Cargo.lock` does
not pin path dependencies, and the faber release CI checks out siblings at
moving default-branch tips (`process-versioning-and-deps.md` §3.5, §4 B7;
`stage0-baseline.md` §3). Two builds of the same Faber tag at different times
can produce different binaries — the manifest closes that gap.

The manifest is the enforcement mechanism behind the release object model
(CAMPAIGN "Release Object Model"): exact source identity → candidate manifest
→ verified artifacts → publication → promotion → receipt.

## 2. Schema name and location

**Decision (accepted):**

| Item | Decision |
| --- | --- |
| Schema | **`release-manifest.schema.json`** (JSON Schema draft 2020-12), human-readable companion example `release-manifest.example.yaml` |
| Instance file | **`faber/release-manifest.yaml`** at the faber repo root, committed alongside `core-support-manifest.txt` |
| Consumers | **Both local and CI builds by design** — local `scripta/` release helpers and the CI checkout/build steps resolve the same file |
| Validation | The instance validates against the schema at prepare time (Stage 3) and in CI; a manifest that fails validation is a hard stop |

Schema documents live under `faber/docs/release/`; the **instance** lives in
the repo root because it is build input, not documentation.

## 3. Top-level structure

```yaml
schemaVersion: "1"            # schema revision; instance format
manifestName: faber-release
releaseIntent:
  component: faber             # faber | radix | cista
  version: "1.5.0"
  channel: stable              # development|candidate|stable|lts|hotfix (release-contract.md §4)
  line: "1.x"                  # explicit major line per policy.md
pinnedInputs:
  source: [ ... ]              # §4 — exact source commits
  packs: [ ... ]               # §6 — dev-kit payload section (release-owned packs)
versionSources: [ ... ]        # §5 — authoritative version sources
exclusions: [ ... ]            # §5 — intentional exclusions
publication:                   # §7 — identity, host, latest rule
  releaseTag: faber-v1.5.0
  host: faberlang/releases
  advancesGlobalLatest: true
```

Every release-changing field is derived from the decisions in
`release-contract.md`; the manifest records, it does not re-decide.

## 4. Pinned inputs — source repos

**Decision (accepted):** the manifest pins **source repos at exact immutable
commits** (tagged commit SHA, or the explicit commit where no tag exists):

| Input | Identity to pin | Why |
| --- | --- | --- |
| `faber` | own source tag commit `vX.Y.Z` | the release itself |
| `radix` | commit SHA (private repo; source-pin only — never leaks source) | path dependency `radix = { path = "../radix/crates/radix" }` (`process-versioning-and-deps.md` §3.1) |
| `cista` | commit SHA | standalone; pinned for provenance (`release-contract.md` §8) |
| `faber-runtime` | commit SHA | core-support assembly (`core-support-manifest.txt`) |
| `hosts` | commit SHA | core-support assembly (`core-support-manifest.txt`) |

The current documentary companion pins (`v1.4.0.md` "Companion pins") become
**enforced** here: CI and local builds resolve the pinned SHAs instead of
default-branch tips. Radix and cista component releases pin their own source
tag; a Faber product release additionally pins all five inputs.

## 5. Authoritative version sources and intentional exclusions

**Decision (accepted):**

- **Authoritative version sources** are the component manifests, exactly as
  the version-validation gates already enforce: `faber/Cargo.toml`,
  radix release-aligned `crates/*/Cargo.toml`, `cista/Cargo.toml`
  (`process-versioning-and-deps.md` §2.2, §6.1). The tag must equal the
  manifest version.
- **Intentional exclusions:** `crates/hygiene-ratchet` (in each of faber,
  radix, cista workspaces) stays at `0.1.0` and is **not** release-aligned
  (`process-versioning-and-deps.md` §1.5, §2.2). Repos without an
  independent release surface (`norma`, `triga`, `examples`, `faber-runtime`,
  `hosts`) have no version row — they appear only as pinned source inputs
  (§4) or pack payloads (§6).
- A mismatch between manifest version, source tag, and component manifest is a
  hard stop at prepare time (Stage 3) and in CI.

## 6. The dev-kit payload section (faber-onboarding Stage-2 target)

Per the council-4 routing-authority decision (`release-contract.md` §1), the
faber-onboarding dev-kit payload manifest is authored as **a section of this
single schema** — the `pinnedInputs.packs` list — never as a parallel
document.

Each release-owned pack row carries the payload facts the onboarding campaign
defined: **component, version, digest, compatibility bound, license,
destination**.

```yaml
pinnedInputs:
  packs:
    - name: launcher            # e.g. the faber binary archive
      component: faber
      version: "1.5.0"
      digest: sha256:...        # of the released artifact
      compatibility: "... major-lane / ABI bound ..."
      license: MIT
      destination: ...          # install/prefix path decided by onboarding
    - name: core-support        # core-support archive (build.rs assembly)
      component: faber
      digest: sha256:...
      ...
    - name: reference-pack      # reference/locale packs (release-owned)
      component: reference
      version: "..."
      digest: sha256:...
      license: ...
      destination: ...
    - name: library-pack        # released libraries pinned for the dev kit
      ...
```

The **shape** (pack taxonomy, discovery, core-vs-optional rules) is defined by
the sibling campaign's `dev-kit-contract.md` (Stage 1, content only);
Stage 2 of that campaign **encodes** the payload into this section under this
process contract's review. This schema owns the release-pin meaning of the
rows; it does not re-define dev-kit content semantics.

**Distinction recorded, not merged:** this document (release **pins**) is
distinct from the sibling `package-and-lock-contract.md` (package **semantics**
— Norma model, portable lock identity, restore command, dependency-graph
placement). The manifest records *what is released and pinned*; the package
contract defines *how packages resolve and lock*.

## 7. Update authority

**Decision (accepted):** the manifest is updated **only as part of the release
prepare step** under the authority model in `authority.md`:

- **Proposer** drafts the manifest change with the release intent (version,
  channel, pinned source SHAs, pack selection).
- **Verifier** checks every pin against live evidence (tag SHAs, version
  sources §5, pack digests) before the commit.
- **Tagger/signer** commits the manifest with the bump+lock single commit for
  the release (`faber/AGENTS.md:134-135`).
- **Ad-hoc edits outside a release intent are rejected** — the manifest is a
  freeze artifact, not a living config file.

## 8. Relationship to the schema consumers

- **Local builds** (burgus/pharos, dry-run worktrees) resolve the manifest at
  prepare time; the dry-run recipe (`worktree-dry-run-recipe.md`) rehearses
  its generation and consumption with no public effect.
- **CI builds** (faber `release.yml`) resolve the same committed manifest and
  check out the pinned SHAs instead of default tips. Stage 8 makes CI consume
  the manifest through the same primitives (thin CI / controlled builder).

## 9. References

- `release-contract.md` — contract, channels, identity, authority context.
- `process-versioning-and-deps.md` §3.5 / §4 B7 — the pin gap this closes.
- `stage0-baseline.md` §3, B11 OQ2 — evidence for the missing manifest.
- `../factory/faber-onboarding/{delivery-stage1.md, dev-kit-contract.md,
  package-and-lock-contract.md}` — payload-shape owners (read-only here).
