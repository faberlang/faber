# Faber CLI

Public user-facing CLI for the Faber programming language. Path-depends on
the private Radix compiler (`../radix/crates/radix`) and public Cista
package store (`../cista`). The `build.rs` assembles a core-support archive
from sibling repos listed in `core-support-manifest.txt`.

## Layout

```text
src/                    CLI + package pipeline
src/postprocess.rs      Faber-owned generated target-source format/lint helpers
crates/exempla/         corpus / language e2e harness (workspace member)
crates/hygiene-ratchet/ production code hygiene budgets
tests/                  integration tests (emit, run, hygiene, …)
scripta/test            cheap progressive ladder (agent default)
scripta/release-gate    EXPENSIVE full-workspace closeout (release only)
build.rs                core-support assembler (reads core-support-manifest.txt)
core-support-manifest.txt  sibling repo paths relative to faberlang container
```

## Ownership boundaries

- Radix lowers Faber into target artifacts. It does not run target-specific
  formatters, linters, package builds, Cargo builds, or executable hosts.
- Faber owns user/product workflow: package build/run/test, host launch, and
  optional generated target-source postprocessing.
- `faber build --format/--linter` and `faber emit --format/--linter` are
  implemented in `src/postprocess.rs`. Supported tools are best-effort and
  host-installed: `rustfmt`, `cargo clippy --fix` for generated Rust, `gofmt`,
  `prettier`/`deno fmt`, and `biome`/`eslint`.
- `crates/exempla/src/postprocess.rs` is harness-owned for e2e toolchain checks.
  Do not move those helpers back into Radix.
- Faber mirrors Radix target features at the product crate level. Default
  builds enable `full-targets`; smaller installs can use
  `--no-default-features --features hir-rust` or another explicit `hir-*` /
  `mir-*` set. Keep Faber feature names aligned with Radix feature names.

`faber format` is author-source formatting. It remains distinct from generated
target-source postprocessing. Longer-term Faber/Forma consolidation is tracked
as factory work: Faber should own user-facing formatting policy, with rule
slugs/options rather than a single all-or-nothing surface enum.

## Test law (cheap-first — no exceptions)

### Agents must not proactively run expensive surfaces

| Surface | Command | When |
| --- | --- | --- |
| **Default (agents)** | `./scripta/test` or `cargo test -p faber --lib` | Every implementation loop |
| **Named test** | `cargo test -p faber <name>` | Fixing one failure |
| **Megasuite (`package::tests`)** | `cargo test -p faber --lib package::tests` | Explicit need for package_test |
| **Product integration** | `cargo test -p faber` | Explicit need for `tests/*` |
| **Full workspace** | `cargo test --workspace` | **Release prep or operator said so** |
| **Language matrices / e2e** | `../radix/scripta/test --stage 5-6` / `--e2e` | **Release / explicit only** |

**Forbidden for agents without an explicit operator request:**

- `./scripta/release-gate`
- `cargo test --workspace` (pulls exempla + every binary)
- `cargo test -p faber` as a “safety sweep” (product integration is explicit opt-in)
- Re-running the full suite after every single-test fix

After fixing one failure: re-run **that test** (or stage 1). Do **not** run
release-gate “to make sure nothing else broke” unless the operator asked for
release/full closeout.

### What `cargo test -p faber --lib` means here

The megasuite is mounted as `package::tests` **inside** the lib crate, so
`--lib` is the honest cheap surface (there are no profile filters to exclude
it):

- package `faber`, library tests only
- **includes** the `package::tests` megasuite (mounted in the lib)
- **excludes** `tests/*` integration binaries
- **excludes** `crates/exempla`

Surfaces:

| Surface | Cargo test form |
| --- | --- |
| `default` (agents) | `cargo test -p faber --lib` |
| megasuite | `cargo test -p faber --lib package::tests` |
| product | `cargo test -p faber` (lib + integration) |
| full workspace | `cargo test --workspace` |

### Faber ladder (`./scripta/test`)

Progressive stages (default = stage 1 only):

1. **default** — hygiene + `cargo test -p faber --lib`
2. **unit** — `cargo test -p faber --lib package::tests` (slow; the megasuite)
3. **product** — `cargo test -p faber` (slow; spawns CLI / nested cargo)

There is **no** progressive stage for full workspace or exempla. That is
`./scripta/release-gate` only.

### Radix ladder (compiler + language proofs)

The sibling radix ladder remains the compiler gate: `../radix/scripta/test`
from `radix/`. Stages 5–6 and `--e2e` are language closeout (expensive). They
are not the Faber agent default either.

- During compiler work: `--check` or `--stage 1-4` as appropriate.
- Language closeout: `--stage 1-6` / `--e2e` only when explicitly needed or
  for release.

## CI dependencies

The release workflow (`.github/workflows/release.yml`) checks out sibling
repos to mirror the local `faberlang/` layout:

- `faberlang/radix` → `../radix` (private, needs `FABERLANG_RELEASES_TOKEN`)
- `faberlang/cista` → `../cista`
- `faberlang/faber-runtime` → `../faber-runtime`
- `mintedgeek/hosts` → `../hosts` (public monorepo)

If `core-support-manifest.txt` changes its sibling paths, the CI checkout
steps must be updated to match.

## Release protocol (Faber)

CI uses `cargo build --locked`. The lockfile must match `Cargo.toml` at the
tagged commit, or the build fails. Follow this exact order:

1. Bump version in `Cargo.toml` (`version = "X.Y.Z"`).
2. Run `cargo update` to regenerate `Cargo.lock`.
3. Verify: `cargo build --locked --release --bin faber` passes.
4. Verify expensive product suite: **`./scripta/release-gate --locked-release-build`**
   (or `./scripta/release-gate` if the release binary was already built).
   This is the only place a full-workspace `cargo test --workspace` is
   required for a release.
   Optional language closeout: `../radix/scripta/test --full` and/or `--e2e`
   when the release includes compiler/corpus claims.
5. **Single commit** containing both the version bump and the regenerated
   `Cargo.lock`. Do not commit them separately.
6. Tag that commit: `git tag vX.Y.Z`.
7. Push: `git push origin main && git push origin vX.Y.Z`.
8. Monitor CI: `gh run list -R faberlang/faber --limit 1`.

**Never** tag a commit that doesn't include the regenerated lockfile. The tag
freezes the exact source CI will build; a stale lockfile makes `--locked`
fail with "cannot update the lock file."

### CI build script

`build.rs` reads `core-support-manifest.txt` and assembles a `core-support`
archive from sibling repos. The paths are relative to the `faberlang/`
container (parent of the `faber/` crate). The manifest currently targets:

```
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

If the hosts monorepo layout changes, update both `core-support-manifest.txt`
and the CI checkout step in `.github/workflows/release.yml`.
