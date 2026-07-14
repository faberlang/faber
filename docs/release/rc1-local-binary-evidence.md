# Faber RC1 Local Binary Evidence

Date: 2026-07-14
Status: local evidence only; not a public release record
Owning repo: `faber`

This note records local evidence for release control-plane and `faberlang.dev`
claim review. It does not authorize tagging, pushing, publishing, public
downloads, installation instructions, Homebrew, curl, DNS, or a
production/stable 1.0 claim.

## Current Evidence Refresh

Captured on 2026-07-14 after the Radix generated-Rust producer fix that cleared
the Triga Stage 2 compile errors.

Source provenance:

| Repo | Commit |
| --- | --- |
| `radix` | `7d32673f8136` |
| `faber` | `5d508d737e87` |

Both `radix` and `faber` were clean before the release-profile build.

Build and validation commands run in `/home/ianzepp/work/faberlang/faber`:

```sh
cargo fmt --check
timeout 180 cargo test reference_test --lib
timeout 240 cargo test --test clean_install_integration_test
timeout 600 cargo build --release
./target/release/faber --version
sha256sum target/release/faber
```

Observed results:

```text
cargo fmt --check: passed
cargo test reference_test --lib: 14 passed
cargo test --test clean_install_integration_test: 2 passed
cargo build --release: passed
./target/release/faber --version: faber 1.0.0-rc.1
sha256sum target/release/faber: 666e0563532ff48659051b2210db09e11aad52690bf6e6e043153a5664170345  target/release/faber
```

## Proven Locally

- A release-profile Faber binary can be built from this checkout.
- `./target/release/faber --version` reports:

  ```text
  faber 1.0.0-rc.1
  ```

- The observed local SHA-256 for that release-profile binary was:

  ```text
  666e0563532ff48659051b2210db09e11aad52690bf6e6e043153a5664170345  target/release/faber
  ```

- Reference-pack version compatibility handles prerelease/build metadata for
  `1.0.0-rc.1`.
- Clean-install core-support proofs pass locally for minimal and native-provider
  packages.

## Validation Commands

The following commands passed in `/home/ianzepp/work/faberlang/faber`:

```sh
cargo fmt --check
timeout 180 cargo test reference_test --lib
timeout 240 cargo test --test clean_install_integration_test
timeout 600 cargo build --release
./target/release/faber --version
sha256sum target/release/faber
```

Observed results:

```text
cargo test reference_test --lib: 14 passed
cargo test --test clean_install_integration_test: 2 passed
cargo build --release: passed
./target/release/faber --version: faber 1.0.0-rc.1
sha256sum target/release/faber: 666e0563532ff48659051b2210db09e11aad52690bf6e6e043153a5664170345
```

## Allowed Claim Wording

- "A local Faber 1.0.0-rc.1 release-profile binary can be built from the
  current local source checkout."
- "The local RC1 binary reports `faber 1.0.0-rc.1`."
- "Clean-install core-support proofs pass locally for minimal and
  native-provider packages."

## Prohibited Claim Wording

Do not claim any of the following from this evidence:

- public release availability;
- pushed tag or GitHub Release;
- public artifact download;
- install route, Homebrew, curl, or one-command install;
- public source-build support;
- production readiness or stable final `1.0`;
- public package registry availability.

## Remaining Gates

Before `faberlang.dev` or release materials can claim RC1 public availability,
the release owner still needs:

- explicit Mind/operator approval for tag, publication, and install route;
- a clean release-control checkout for the scripted release lane;
- a named public artifact location and checksum/manifest policy;
- website export/leakage checks and placeholder digest closure;
- operator authorization for any external publication surface.
