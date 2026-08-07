# Platform / builder / reproducibility matrix

**Status:** accepted — Stage 1 decision record (component-release-streamline)
**Date-stamped:** 2026-08-07
**Resolves:** campaign Open Questions 3 (platform/support matrix) and 4
(builder trust, native vs cross, second-builder comparison)
**Evidence basis:** observed matrices 2026-08-07 (`stage0-baseline.md`
§1.1–1.2; reconciled `process-versioning-and-deps.md` §1.1), failure mode F1
(GHA queue / Intel drop).

> This matrix governs **release production**. The user-facing *platform slice*
> (shell, install prefix, default execution target, clean-room profiles) is the
> sibling `faber-onboarding` `platform-matrix.md` — the distinction is
> recorded, not merged.

---

## 1. Support tiers

| Tier | Meaning | Missing-leg effect |
| --- | --- | --- |
| **supported** | A release must ship this target for the owning component; gates apply | blocks the whole release |
| **experimental** | May ship when green; no release-blocking obligation | never blocks; recorded when absent |
| **deferred** | Intentionally not in the matrix (recorded decision/dated reason) | never blocks |

**Decision (accepted):** a missing **supported** leg blocks the whole release
of that component; experimental/deferred legs never do. The current
`--clobber`/matrix gaps are not silently ignored — a supported leg with no
builder is a stop condition (see §4).

## 2. Matrix — faber / radix / cista

Observed as-built matrices (`stage0-baseline.md` §1.1–1.2) adopted as the
Stage-1 acceptance:

| Component | Target | Tier | Native/cross | Builder | OS / SDK + toolchain | Signing | Required gates | Missing leg blocks |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **faber** | linux x86_64 | **supported** | native | burgus (local proof) + GHA ubuntu hosted runner (controlled); pharos = second builder | Linux; pinned Rust toolchain (`cargo build --locked`) | unsigned (no platform signing on Linux) | `./scripta/release-gate --locked-release-build` + version/tag gate + locked build (`faber/AGENTS.md:126-133`) | whole faber release |
| **faber** | macOS arm64 | **supported** | native | GHA macos-14 hosted runner (controlled) + Apple Silicon burgus (local proof) | macOS SDK; pinned Rust toolchain | macOS code signing/notarization **where a controlled signer exists — none today (recorded gap)** | same + signing gate when signer exists | whole faber release |
| **faber** | macOS x86_64 | **deferred** | — | — | Intel dropped 2026-08-07 — GHA queue (F1) | — | — | never |
| **faber** | Windows x86_64 | **deferred** | — | — | not in scope (no artifact, no builder) | — | — | never |
| **radix** | linux x86_64 | **supported** | native | pharos (local proof) + GHA ubuntu hosted runner (controlled) | Linux; pinned Rust toolchain | unsigned | radix ladder `./scripta/test --full` at tag (or `--stage 1-4` at main) + locked build (`radix/AGENTS.md:378-390`) | whole radix release |
| **radix** | macOS arm64 | **supported** | native | GHA macos-14 hosted runner (controlled) + local Apple Silicon | macOS SDK; pinned Rust toolchain | macOS signing **where a controlled signer exists — none today** | same | whole radix release |
| **radix** | macOS x86_64 | **deferred** | — | — | Intel dropped (F1) | — | — | never |
| **radix** | Windows x86_64 | **deferred** | — | — | not in scope | — | — | never |
| **cista** | linux x86_64 | **supported** | native | GHA ubuntu hosted runner (controlled) + local | Linux; pinned Rust toolchain | unsigned | cista smoke (build + `--version` today; test/lint/hygiene/install/package smoke surfaces are a recorded gap F6, routed to cista owner / Stage 8) | whole cista release |
| **cista** | macOS x86_64 | **supported** | native | GHA macos-13 hosted runner (controlled; Intel leg retained for cista) | macOS SDK; pinned Rust toolchain | macOS signing where a signer exists — none today | same | whole cista release |
| **cista** | macOS arm64 | **supported** | native | GHA macos-14 hosted runner (controlled) | macOS SDK; pinned Rust toolchain | same | same | whole cista release |
| **cista** | Windows x86_64 | **deferred** | — | — | not in scope | — | — | never |

**Facts carried from the fact set (cited, not re-verified here):** faber/radix
build linux x86_64 + macOS arm64; cista additionally macOS x86_64;
`cista-v0.1.0` has no observed public release (`stage0-baseline.md` §6, §5 F5).

## 3. Builder trust (OQ4)

| Builder | Kind | Role in the process |
| --- | --- | --- |
| **burgus** (local laptop) | operator-controlled machine | primary **local** proof builder: locked build, gates, archive, checksum, leakage scan (`stage0-baseline.md` §4.2 offline-capable items) |
| **pharos** (home server) | operator-controlled machine | second builder for stable/LTS comparison; heavier/parallel local builds |
| **self-hosted runner** | future managed builder | **deferred-with-owner** (this campaign Stage 8) — no self-hosted runner exists today |
| **GHA hosted runner** | managed build platform (controlled-builder class) | current controlled builder for the matrix legs; CI is corroboration or publish, **not the only truth** (CAMPAIGN "Development Posture") |

**Decision (accepted):**
- **Native builds only.** Cross-compilation is not used to satisfy a supported
  leg; each supported target builds on a matching-OS native builder. (A future
  cross path needs its own provenance evidence — not a Stage-1 decision.)
- **Second clean-builder comparison:** required for **stable/LTS product
  releases** — one rebuild of the same pinned inputs on a *different*
  controlled builder (e.g. pharos for linux, burgus for macOS arm64) with
  receipts compared (hashes + `--version`). Recommended, not required, for
  component releases.
- A supported leg with **no reachable controlled builder** is a stop condition
  — route a need for builder placement before claiming local-first complete
  (CAMPAIGN "Stop Conditions").

## 4. Reproducibility standard (OQ4)

**Decision (accepted):** the binding standard is **verified provenance from
controlled builders** — each release receipt records exact source pins,
toolchain/SDK versions, target triple, environment policy, and normalized
archive rules. **Byte-for-byte rebuild equivalence is a recorded non-gate
aspiration**, not a gate: environment/toolchain drift makes it unreliable as a
release blocker, and the second-builder comparison (§3) provides the
cross-builder check that matters.

Archive normalization rules (order, ownership, mode, timestamp) are
implemented by the Stage 4 packaging unit; this matrix records the standard
they serve.

## 5. Decision ledger for this artifact

| # | Decision | Marking | Evidence |
| --- | --- | --- | --- |
| OQ3 | Support tiers + matrix rows above; missing **supported** leg blocks the whole release | **accepted** | B11 OQ3; observed matrices `stage0-baseline.md` §1.1–1.2; F1 (Intel drop) |
| OQ4 | Builder trust per §3; native-only; second clean-builder comparison for stable/LTS product releases; verified-provenance standard | **accepted** | B11 OQ4 (`carried-to-stage-1`); `stage0-baseline.md` §4; CAMPAIGN "Platform, Builder, And Reproducibility Contract" |
| OQ4 | Self-hosted runner, macOS signing, Windows/macOS-x86_64 (faber/radix) legs | **explicitly-deferred-with-owner** (this campaign later stages / cista owner / controlled-signer work) | F1; §2 rows |

## 6. References

- `release-contract.md` — contract, identity, authenticity context.
- `process-versioning-and-deps.md` §1.1 — as-built matrices.
- `stage0-baseline.md` §1.1–1.2, §4, F1 — evidence.
- `../factory/faber-onboarding/platform-matrix.md` — user-platform slice
  (distinct; read-only here).
