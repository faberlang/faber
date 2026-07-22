# Faber CLI

Public user-facing CLI for the Faber programming language. Path-depends on
the private Radix compiler (`../radix/crates/radix`) and public Cista
package store (`../cista`). The `build.rs` assembles a core-support archive
from sibling repos listed in `core-support-manifest.txt`.

## Layout

```text
src/                    CLI + package pipeline
crates/exempla/         end-to-end test harness
crates/hygiene-ratchet/ production code hygiene budgets
tests/                  integration tests (emit, hygiene)
build.rs                core-support assembler (reads core-support-manifest.txt)
core-support-manifest.txt  sibling repo paths relative to faberlang container
```

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
4. Verify: `cargo test` passes (or known-ignored tests only).
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
