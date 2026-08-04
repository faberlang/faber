# Delivery: reproducible Faber releases and portable defaults

**Status**: planning — implementation not started  
**Owner**: Faber product release surface (`faber/`)  
**Date**: 2026-08-02  
**Scope**: Faber release artifacts, release validation, first-run documentation,
and removal of implicit Rust/Cargo requirements from portable Faber workflows.

## Interpreted Unit

Make a downloaded Faber binary trustworthy for a user who has no sibling source
checkout and no Rust/Cargo installation. A release must be assembled from known
component revisions, carry or correctly locate every runtime/reference asset it
claims to provide, and pass a clean-room first-run test.

The product must retain Rust as an explicit optional backend, but portable Faber
workflows must not select or probe Rust merely because the user asked to build,
run, test, inspect, or package a Faber program. FHIR is the default portable
package interchange format. FMIR is the execution artifact derived from loaded
FHIR; it is not the semantic package source. S-expression output remains a
separate text/interop artifact contract.

The public `cista.dev` registry/server is a deferred companion project. This
delivery may define the client and release handoff it needs, but does not create
the `cista.dev` repository, Railway service, DNS, production credentials, public
upload path, or registry website.

## Normalized Spec

### Required outcomes

1. A release tag identifies the exact Faber, Radix, Cista, runtime, host, and
   reference/corpus revisions used to build the binary.
2. The release archive is self-consistent: version, checksum, reference pack,
   reader data, bundled support, and documentation all agree with the release.
3. A clean-room release gate runs the actual archive with a temporary `HOME`, a
   minimal `PATH`, no sibling repositories, and no Rust/Cargo unless the test
   explicitly selects the Rust backend.
4. A new package can use the portable FHIR path without Rust:

   ```text
   faber init app
   faber check app
   faber build app       # writes a FHIR package artifact
   faber run app         # loads FHIR, lowers to FMIR, runs in-process
   ```

5. Rust remains available through an explicit target or manifest selection and
   continues to have its own toolchain-dependent gate.
6. Interface-only package installation and portable package resolution do not
   invoke `rustc`, `cargo`, or a target-native compiler.
7. The site documents the tested release and separates portable execution,
   native compilation, artifact emission, and hosted package discovery.

### Explicit non-goals

- Creating or deploying the `cista.dev` server and website.
- Making every language target executable.
- Treating FHIR as executable by itself. Portable execution loads FHIR and
  lowers it to FMIR in-process.
- Preserving an implicit Rust default for newly scaffolded packages merely for
  compatibility. Existing manifests that explicitly select Rust remain Rust
  projects until deliberately migrated.

## Repo-Aware Baseline

### Release infrastructure

The Faber release workflow currently checks out sibling repositories without
per-component refs, builds with Cargo, and archives only the `faber` executable
and a short README. The release archive therefore has no explicit reference
pack or source-provenance manifest. See
[`faber/.github/workflows/release.yml`](../../../.github/workflows/release.yml) and
[`faber/AGENTS.md`](../../../AGENTS.md).

Faber's `build.rs` assembles core support from sibling paths listed in
[`core-support-manifest.txt`](../../../core-support-manifest.txt), so sibling
revision pinning is a release invariant rather than a convenience.

The public site hardcodes the user-facing release version in its source and is
currently capable of drifting from the published binary. The site is a
companion consumer of release metadata, not the authority for component
selection.

### Portable target infrastructure

The live target surface already contains useful pieces:

- `fmir-text` and `fmir` build source-independent execution images and run them
  through the in-process FMIR host without Cargo. Under the new package
  contract, these are derived execution artifacts rather than the default
  semantic package format.
- `fmir-bin` creates a native runner and currently requires Cargo for that
  wrapper.
- `fhir` serializes analyzed HIR for a `.fab` file through the Radix emit path;
  it is not currently a Faber package surface. Package support is scoped in
  [`Radix FHIR package delivery`](../../../radix/docs/factory/hir-artifact-fhir/package-fhir-delivery.md).
- `sexp` emits Racket/S-expression text for inspection or a separate parser;
  it is not currently a Faber package build/run surface.

Current product defaults still point at Rust:

- [`faber/src/commands/init.rs`](../../../src/commands/init.rs) scaffolds
  `build.target = "rust"`.
- [`faber/src/commands/mod.rs`](../../../src/commands/mod.rs) falls back to Rust
  for `faber build` when no target is selected.
- [`faber/src/cli/mod.rs`](../../../src/cli/mod.rs) defaults `faber run` to Rust;
  package execution then follows the Rust/Cargo path.
- [`cista/src/commands/install.rs`](../../../../cista/src/commands/install.rs)
  and its Rust target metadata path inspect `rustc` during package install,
  including interface-only installs.

The current package compiler supports FMIR image builds, but package target
manifests accept only the currently implemented package routes. FHIR and Sexp
are therefore not interchangeable with FMIR by changing one default string.

Existing factory work already covers important seams, including interpreted
execution and FMIR library imports. This delivery must consume those goals and
re-test their release binary behavior; it must not recreate a parallel runtime
or resolver.

## Stage Graph

### Stage 0 — Contract freeze and release inventory

Define the release manifest, artifact inventory, portable-default policy, and
target vocabulary before implementation.

Gate: a reviewed table distinguishes source revision, compiler/reference data,
package/runtime assets, portable execution, native compilation, and site
metadata. No target is called “portable” merely because it emits text.

### Stage 1 — Pinned component release assembly

Add a release-owned manifest or equivalent generated evidence that pins the
exact sibling revisions used by a Faber release. Make CI check out those
revisions, validate version/tag relationships, assemble core support, and emit
provenance alongside the archive.

Gate: rebuilding the same release tag resolves the same component revisions and
produces an artifact whose provenance is inspectable without source checkout.

### Stage 2 — Self-contained artifact packaging

Package the reference/reader assets required by the documented CLI, remove
absolute builder paths, and define the archive layout. Keep the archive small,
but do not make first-run behavior depend on undeclared paths or development
checkouts.

Gate: `--version`, `explain`, `init`, and diagnostic rendering pass from an
extracted archive under a temporary home.

### Stage 3 — Portable FHIR package default and FMIR execution

Make FHIR the default package artifact for newly initialized packages and for
implicit portable builds. Make `faber run` load the FHIR package and lower it
to FMIR in-process. Keep `--target fmir` as an explicit derived execution
artifact, and keep `--target rust` and explicit Rust manifests as the native
route. Resolve explicit user selection before any fallback so a Rust request
still fails honestly when its toolchain is absent.

The exact CLI decision must include whether `faber run` reads the package
manifest target when no CLI target is supplied or uses a portable default for
all package runs. The implementation must not silently reinterpret an existing
explicit Rust manifest.

Gate: a new package builds a FHIR package and runs through FHIR → FMIR with no
`cargo`, `rustc`, Cargo-generated crate, or sibling checkout visible in the
environment.

### Stage 4 — Toolchain-independent package installation

Split Cista package installation into metadata/interface acquisition and
target-native artifact preparation. An interfaces-only or FMIR-compatible
package must not inspect Rust. Native Rust binding validation/build remains an
explicit later step selected by the consumer target.

Gate: install a source library into an isolated store with no Rust executable in
`PATH`, create or update the project lock, resolve its interfaces, and run a
portable consumer where the package contract permits it.

### Stage 5 — Portable library and target parity

Run the portable acceptance matrix against Norma and Triga through the FHIR
package path. Repair or mark unsupported any library/module whose check path
passes but whose FHIR load or FMIR execution does not. Keep native Rust checks
as a separate matrix rather than silently using them as proof of portable
support.

Gate: the matrix records package source, lockfile, target, artifact, runtime
requirements, and output for each supported exemplar.

### Stage 6 — Release gate, promotion, and site synchronization

Run the exact published archive through the full clean-room matrix, publish
artifacts only after it passes, and synchronize the install/hello/package pages
from the same release metadata. Public promotion remains separate from source
tagging but cannot proceed on a stale or unproven archive.

Gate: the download URL, checksum, version, archive contents, CLI behavior, and
site instructions all refer to one release identity.

## Implementation Work

This is intentionally split into bounded factory units rather than one broad
cross-repository implementation.

| Unit | Owner | Primary surface | Result |
| --- | --- | --- | --- |
| Release provenance and archive contract | Faber | `faber/.github`, release scripts, archive layout | Pinned sibling revisions, provenance, reference assets, reproducible archive |
| Portable CLI defaults | Faber | `faber/src/cli`, `commands`, package dispatch | FHIR-first build, FHIR→FMIR run behavior; explicit Rust route |
| Target contract alignment | Radix + Faber | target matrix, FHIR emit, Sexp/FMIR package boundaries | Product-visible target truth and no false portability claims |
| No-Rust package install | Cista + Faber | install metadata, store/lock integration | FHIR/interface installs do not probe Rust |
| Library proof matrix | Norma + Triga + Faber | manifests, exemplars, release smoke fixtures | Portable package claims backed by build/run evidence |
| Site release handoff | `faberlang.dev` + Faber | install/hello/package docs and generated metadata | First-run docs match the tested archive |

The `cista.dev` server is a separate future unit. Its handoff from this work is
the client contract, fixture registry, package provenance format, and a list of
hosted assumptions that must become true before the public package path is
advertised.

## Checkpoints And Gates

### Release gates

- Stable SemVer tag matches the Faber package version.
- Every sibling revision is explicit and recorded.
- `cargo build --locked --release` is still required only to build the release
  binary, not to use the released portable target.
- Archive checksum covers the exact published archive.
- Reference/reader data is present and contains no builder-local absolute path.
- Temporary-home smoke tests use the extracted archive, not a local build.
- Promotion to `faberlang/releases` happens only after all target rows pass.

### Portable gates

- No-Rust `faber init → check → build → run` passes for a fresh package.
- `faber build` writes a FHIR package and `faber run` loads it, lowers to FMIR,
  and runs with a minimal `PATH`.
- `faber build --target fmir` remains available as an explicit derived image
  route and works with a minimal `PATH`.
- The portable path does not create or invoke a Cargo-generated crate.
- Package installation and lockfile resolution work without `rustc` when the
  package contract is interface/FMIR-only.
- Explicit `--target rust` produces a clear toolchain diagnostic when Rust is
  absent.
- FHIR is documented as the semantic package artifact; FMIR is documented as
  its execution derivative; S-expression remains a separate output contract.

### Batching / Split Decision

Split on the release-versus-compiler boundary:

1. Release provenance and clean-room archive gate.
2. Portable default target and no-Rust CLI behavior.
3. Cista interface-only install behavior.
4. Library/website acceptance and release promotion.

Stages 1 and 2 can begin after Stage 0 independently, but Stage 4 cannot close
until the portable target and install contracts are stable. The `cista.dev`
server remains deferred and is not a blocker for local/fixture registry proof.

### Release decision

Completion triggers **release-prep**, not automatic release. The release
workflow must first consume the new provenance and smoke gates, then produce a
candidate archive and evidence packet for operator promotion.

## Validation

Use cheap checks first, then the exact artifact:

1. Static workflow/config validation and archive-layout tests.
2. Faber unit/CLI tests for target selection and no-Rust dispatch.
3. Cista install/lock tests with `rustc` hidden from `PATH`.
4. Faber portable package tests using `fmir-text` and `fmir`.
5. Norma/Triga consumer matrix, including negative unsupported cases.
6. Build a release candidate from pinned sibling revisions.
7. Extract that candidate into a clean temporary home and run the end-to-end
   command matrix with minimal `PATH`.
8. Verify checksums, provenance, public release assets, and site links.

The final evidence must distinguish:

- compiler/build-tool requirements used by CI to create the Faber binary;
- user requirements for portable Faber execution;
- requirements of an explicitly selected native backend;
- requirements of a future hosted Cista registry.

## Companion Skill Plan

- `faber`: update target/default/package guidance after live behavior changes.
- `factory`: execute each bounded unit with review and commit gates.
- `polish`: run over changed primary Rust/workflow/docs files at unit closeout.
- `zombie-docs`: verify install, hello, package, and target pages against the
  final archive behavior.
- `cista.dev` future campaign: resume from
  [`cista/docs/factory/cista-dev-registry/CAMPAIGN.md`](../../../../cista/docs/factory/cista-dev-registry/CAMPAIGN.md)
  after the client/release boundary is stable.

## Open Questions

1. Should `faber build` with no explicit target always emit FHIR, or should it
   remain manifest-directed after initialization? Default: implicit portable
   builds emit FHIR; explicit manifest targets retain their meaning.
2. Should implicit `faber run` choose the manifest target, or should package run
   always load FHIR and derive FMIR unless `--target rust` is explicit? Default:
   explicit Rust manifests remain Rust; otherwise portable run loads FHIR.
3. Which FHIR package container and extension should the Faber product adopt?
   The Radix package delivery recommends a distinct package name from a
   single-unit `.fhir` artifact.
4. Should S-expression output remain `faber emit` only, or gain a package archive
   contract with a separate parser/runtime owner?
5. Which reference/corpus data is release-owned and which is package-owned?
6. Is `faberlang/releases` the permanent public binary boundary, or should a
   future release service replace it while preserving the archive contract?
7. Which Norma/Triga modules are promised in the first portable release gate?
