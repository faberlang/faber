# Platform Matrix — Supported Slice, Named Clean-Room Profiles, Residuals

**Status**: active — Stage 1 decision record (OQ9/G12); per-platform evidence accrues from Stage 2 on, consolidated in Stage 10
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 1 of 10
**Delivery spec**: [delivery-stage1.md](delivery-stage1.md)
**Date-stamped**: 2026-08-07
**Evidence**: [golden-path-inventory.md](golden-path-inventory.md) E3, E17; CAMPAIGN §Experience model + §Named clean-room profiles

**Scope distinction (recorded, not merged):** this `platform-matrix.md` is the **user platform slice** (what newcomers can run Faber on). The sibling `component-release-streamline/platform-builder-matrix.md` is the **release production matrix** (which builders/toolchains/signing identities produce artifacts). Two distinct matrices; neither absorbs the other.

## OQ9 — Supported slice: macOS arm64 + Linux x86_64

**Accepted.** The supported first slice is **macOS arm64 and Linux x86_64** — exactly the platforms with a released artifact and a release CI worker (E3/E17). **Windows and macOS Intel are named residuals**: no release artifact and no clean-room worker exist (macOS-13 Intel was removed from CI citing unbounded GitHub-hosted queue, E17). A platform moves from residual to supported only when a release artifact **and** a clean-room worker exist (delivery-stage1 D8).

## Platform rows

| Platform | Shell | Arch | Install prefix | Required system tools | Signature/checksum policy | Default execution target | Unsupported/residual status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **macOS arm64** | zsh (default) | aarch64 | user-local `~/.local`/`~/.faber` (unprivileged default; explicit `--prefix` for system/agent) | None beyond the kit (launcher + core support materialized); Cargo only when the Rust target is explicitly selected and while it remains the released default (gate 3) | SHA-256 verified (E3); unsigned labeled residual until signing lands (G8) | Portable FHIR→FMIR when released; until then the released path with prerequisites stated | **Supported** |
| **Linux x86_64** | bash/sh (minimal container) | x86_64 | same prefix rules | Minimal documented system prerequisites only (clean-room `linux-x64-minimal`); glibc-based mainstream distros; other Linux variants residual | SHA-256 verified (E3); unsigned labeled residual until signing lands (G8) | Portable FHIR→FMIR when released; until then the released path with prerequisites stated | **Supported** (glibc mainstream); other variants **residual** |
| **macOS Intel** | zsh | x86_64 | — | — | — | — | **Residual** — no artifact (E17); no clean-room worker |
| **Windows** | — | — | — | — | — | — | **Residual** — no artifact, no worker (E3/E17) |

Every row records shell, arch, prefix, tools, signature/checksum policy, default execution target, and residual status (delivery-stage1 D8). The default-execution-target cell is consistent with gate decision 3 and `release-and-portable-default`'s portable gates — no unreleased no-rust claim is made as current proof.

## Named clean-room profiles

Per CAMPAIGN §Named clean-room profiles. Each profile records (at proof time, Stages 2–10): dev-kit manifest digest, allowed network endpoints, exact PATH, environment variables removed, expected filesystem changes, and positive **plus** negative outcomes. A locally built binary is not a published artifact proof.

| Profile | Platform row | Required isolation and proof | Owner |
| --- | --- | --- | --- |
| `macos-arm64-fresh` | macOS arm64 | Fresh macOS user; isolated HOME/store; no repos, overrides, or ambient credentials; published signed/checksummed artifact | faber Stage 2 (payload) + Stage 10 (evidence) |
| `linux-x64-minimal` | Linux x86_64 | Minimal supported container; isolated HOME/PATH/store; only documented system prerequisites | faber Stage 2 + Stage 10 |
| `no-rust` | macOS arm64 / Linux x86_64 | Cargo/rustc absent; the selected portable `check`/`run` claim must pass or docs must not make it | `release-and-portable-default` (stages 3/6) + faber Stage 5 |
| `offline-restored` | macOS arm64 / Linux x86_64 | Network denied after one authenticated/verified fetch; unchanged lock + populated store reproduce the build | faber Stage 7 (relocation/offline proof) |
| `agent-noninteractive` | macOS arm64 / Linux x86_64 | Explicit prefix; no prompts; stable exit codes and JSON diagnostics; cleanup receipt checked | faber Stage 9 + Stage 3 |

## Residual policy and routing

- **Windows, macOS Intel, non-mainstream Linux variants** stay named residuals until a release artifact and a clean-room worker exist (G12; decision-ledger item 5).
- Residual → supported transitions are proposed at the stage that adds the artifact (faber Stage 3/10) and gated on Stage 10 clean-room evidence; macOS-native packaging additionally follows OQ8 (deferred, `install-channel-matrix.md`).
- The clean-room evidence consolidation for both supported platforms is the faber Stage 10 "continuous honesty" gate (separate evidence per platform; CAMPAIGN Stage 10 gate).
