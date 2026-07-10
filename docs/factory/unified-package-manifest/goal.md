# Goal: Unified Faber Package Manifest

**Status**: active — Phases 1–2 and Phase 4 binding verification complete; Phase 3 backend build graph remains open
**Created**: 2026-07-08
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/unified-package-manifest/`
**Primary surface**: `faber.toml` schema, package discovery, library install,
source-library resolution, backend library builds, target-specific binding
manifests.

---

## Summary

Make `faber.toml` the repo-level manifest for every Faber package shape:
applications, source libraries, backend-compiled libraries, and Faber facades
over native backend dependencies.

The manifest should describe package identity once, then declare what the
package produces:

- a binary application (`kind = "bin"`);
- a Faber source library (`kind = "lib"`);
- a Faber library that can be built for one or more backend targets;
- a Faber API facade implemented by target-specific native bindings.

This goal replaces ad hoc stdlib/provider assumptions with a common package
format. The Faber language surface remains the source-level contract between
packages; backend artifacts are built or linked intentionally based on the
selected target.

## Invariant

`faber.toml` is the package authority for both applications and libraries.
Faber source declares provider/module APIs; manifests declare package layout,
artifact kind, target support, dependencies, and backend binding maps.

## Desired Manifest Shape

Current binary manifests remain valid:

```toml
[package]
name = "demo"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
target = "rust"
```

The existing optional `[reader]` table (`locale`, `pack`) remains valid and is
orthogonal to library package metadata; it is omitted from the examples below.

Source and backend-buildable library packages use the same file:

```toml
[package]
name = "norma"
version = "0.1.0"
edition = "2026"

[library]
provider = "norma"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
```

Faber facades over existing Rust crates add target-specific implementation
metadata:

```toml
[package]
name = "jsonx"
version = "0.1.0"
edition = "2026"

[library]
provider = "jsonx"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[target.rust]
bindings = "bindings/rust.toml"

[target.rust.dependencies]
serde = "1"
serde_json = "1"
```

Dense backend maps should live outside `faber.toml`:

```toml
# bindings/rust.toml
[functions."jsonx:json/solve.solve"]
symbol = "crate::shim::solve"

[functions."jsonx:json/pange.pange"]
symbol = "crate::shim::pange"

[shim]
path = "rust/shim.rs"
```

The current binding key grammar is
`provider:module/path.function`. Changing it requires an explicit clean-break
design; Faber source remains the API contract and backend manifests remain
implementation data.

## Goals

- Extend `faber.toml` into one package format for binaries and libraries.
- Keep existing binary manifests source-compatible.
- Add library package metadata:
  - provider name;
  - source root;
  - library artifact kind;
  - supported backend targets.
- Teach `faber install` to accept Git URLs, clone a repo, parse top-level
  `faber.toml`, and install by manifest provider/package identity.
- Teach provider resolution to read installed package manifests instead of
  assuming `src`.
- Prepare the build graph model where an application target selects matching
  backend artifacts for every Faber dependency.
- Move `externa`/`subsidia`-style implementation linkage out of Faber source
  into target-specific manifests.

## Non-goals

- Reintroducing `@ externa` or `@ subsidia` as source annotations.
- Building a full registry, lockfile, or semver solver in the first phases.
- Compiling every library dependency to Rust in Phase 1.
- Designing complete Rust ABI/shim generation in Phase 1 or Phase 2.
- Solving non-Rust backend binding formats before the Rust model is clear.

## Implementation Phases

### Phase 1 — Library-Aware `faber.toml`

Status: complete (2026-07-09). Faber manifests now accept library package
metadata with `[library].provider`, `build.kind = "lib"`, and
`build.targets`; binary manifests require `[paths].entry` and keep singular
`build.target`. Validation rejects unknown fields and invalid provider/package
names. Gates passed:
`timeout 120 cargo test --lib manifest -- --format terse` and
`timeout 120 cargo test --lib discover -- --format terse`.

Add the manifest schema and validation needed to represent library packages.

Deliverables:

- Add optional `[library]` with `provider`.
- Extend `[build]` to accept `kind = "bin" | "lib"`.
- Add `targets = ["rust", ...]` for libraries while keeping `target = "rust"`
  for binaries.
- Make `[paths].entry` required for `bin` and optional for `lib`.
- Validate provider/package names with the same segment policy used by import
  providers.
- Keep unknown fields rejected.
- Add tests for current binary manifests, source-library manifests, invalid
  provider names, missing binary entries, and library manifests without entry.

Gate:

```bash
timeout 120 cargo test --lib manifest
timeout 120 cargo test --lib discover
```

### Phase 2 — Git URL Install and Manifest-Based Provider Roots

Status: complete (2026-07-09). `faber install` now accepts library names and
Git/path sources, clones into a temporary checkout, requires a top-level
installable library `faber.toml`, installs under the declared provider, rejects
conflicting existing installs by package identity or remote, and provider
resolution reads the installed manifest's `[paths].source`. Consumer proof:
`commands::install_test::install_git_path_library_and_consume_non_default_source_root`
installs a local Git library whose source root is `interfaces/` and compiles an
application importing `altmath:math/add` from that installed package. Negative
proof:
`package::tests::library_resolver_reports_installed_manifest_missing_source_root`
checks missing installed source-root diagnostics, and
`commands::install_test::install_rejects_existing_provider_with_different_remote_or_identity`
checks conflicting installs. Gates passed:
`timeout 120 cargo test install` and
`timeout 120 cargo test --lib library_resolver`.

Make `faber install` install any Faber library repo that declares a valid
top-level `faber.toml`.

Deliverables:

- Accept `faber install <library>` and `faber install <git-url>`.
- Clone URL installs into a temporary checkout first.
- Require top-level `faber.toml`.
- Require `[build].kind = "lib"` for installable source libraries.
- Install/update under `$FABER_LIBRARY_HOME/<provider>`.
- Fail if the target directory exists with a different remote or conflicting
  provider identity.
- Resolve provider modules from the installed package's `[paths].source`, not a
  hard-coded `src`.
- Keep direct setup diagnostics for missing home, invalid manifest, missing
  source root, missing provider repo, and missing module.

Gate:

```bash
timeout 120 cargo test install
timeout 120 cargo test --lib library_resolver
```

### Phase 3 — Backend Library Build Graph

Status: open. Phase 4 can verify a native-binding library in isolation, but
application builds do not yet consume its generated/backend library artifact,
shim, and target dependencies through a package graph.

Compile Faber library dependencies for the same backend target as the
application and link generated backend artifacts intentionally.

Deliverables:

- Represent dependency package graph separately from source import graph.
- For `faber build --target rust` of a binary package, require every runtime
  Faber library dependency to support Rust.
- Emit Rust library crates for Faber library packages.
- Link the generated application crate to generated dependency crates through
  Cargo path dependencies.
- Stop selecting Cargo runtime dependencies by sniffing emitted Rust text
  (e.g. `generated_code_needs_tokio` / `generated_code_needs_faber` in
  `package/cargo.rs` and `package/cmd.rs`); drive that selection from the
  manifest and package dependency graph instead. Faber library linkage is
  already annotation/import-graph based (`has_rust_subsidia` reading
  `@ subsidia` in `package/library.rs`), so this phase extends that model
  across generated library crate boundaries rather than replacing norma
  scanning — no production `rust_code.contains("norma::")` exists today.
- Decide how native Faber bodies, codegen templates, and `ad` routes export
  across generated Rust library crate boundaries.

Gate:

```bash
timeout 120 cargo test --lib package
timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix -- backend_smoke
```

### Phase 4 — Target Binding Manifests

Status: complete for the binding-manifest verification surface (2026-07-09).
Faber manifests now accept `[target.rust]` with `bindings` and target-specific
dependency pins. `faber verify-library --target rust <package>` loads
`bindings/rust.toml`, validates binding rows against real Faber function
declarations, requires bindings for declarations without Faber bodies, checks
Rust shim source presence, and reports explicit binding diagnostics. This
provides the SQLite package contract gate without implementing the Phase 3
generated-library build graph. Gates passed:
`timeout 120 cargo test --lib binding_manifest -- --format terse`,
`timeout 120 cargo test cli_parses_verify_library_subcommand -- --format terse`,
and `timeout 120 cargo test --lib manifest -- --format terse`.

Replace source-level implementation linkage annotations with target-specific
binding manifests loaded like reader packs.

Deliverables:

- Define `bindings/rust.toml` schema for externally implemented declarations.
- Load binding manifests from `[target.rust].bindings`.
- Validate every declaration without a Faber body has a selected-target binding.
- Validate every binding row points to a real Faber declaration.
- Support Rust shim source inclusion and declared Cargo dependencies in the
  verification probe; application build linkage remains Phase 3.
- Fail clearly during verification when a selected target has missing bindings.
- Add `faber verify-library --target rust` validation.

Gate:

```bash
timeout 120 cargo test --lib binding
timeout 120 cargo test --lib package
```

## Open Questions

- Should library packages use `[build].targets` only, or should binary packages
  eventually also accept `targets`?
- Should `[dependencies]` live in `faber.toml` now, or remain deferred until the
  package-store/lockfile direction is ready?
- How much Rust shim generation should the compiler own versus requiring
  explicit shim files?

Resolved in Phase 4: a Faber function declaration without a body means its body
is supplied by the selected target binding, and binding rows use
`provider:module/path.function` keys.

## Validation

At goal closeout:

```bash
timeout 120 cargo test --lib manifest
timeout 120 cargo test --lib library_resolver
timeout 120 cargo test install
```

Broaden to sibling radix `./scripta/test` once Phase 3 or Phase 4 changes package
build behavior that needs compiler gates.
