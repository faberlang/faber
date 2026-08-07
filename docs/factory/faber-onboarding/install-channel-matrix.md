# Install Channel Matrix — Channel Dispositions, Trust Policy, Prefix, Lifecycle

**Status**: active — Stage 1 decision record (gate 2 + OQ8); Stage 3 implements the primary channel lifecycle
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 1 of 10
**Delivery spec**: [delivery-stage1.md](delivery-stage1.md)
**Date-stamped**: 2026-08-07
**Evidence**: [golden-path-inventory.md](golden-path-inventory.md) E1, E3, E17, E19; CAMPAIGN §Product Definition provisional choice 1/2; `faber/docs/release/policy.md` (normative version lanes — cited, not duplicated)

## Channel dispositions

| Channel | Disposition | Role | Evidence |
| --- | --- | --- | --- |
| **GitHub prebuilt archive** | **Primary** | Source of truth; versioned, checksummed release bundle per platform triple | E1 (site documents 1.4.0 archive), E3 (assets + SHA-256 verified) |
| **Verified `curl` bootstrap** | **Secondary (convenience)** | Downloads the fixed release payload; never a second release system (CAMPAIGN Stage 3 overlap rule) | E1, E3; provisional choice 1 |
| **Homebrew** | **Secondary, non-authoritative** | Presentation of the same payload/version; explicitly labeled non-authoritative; formula-lag policy (G7) | E1 (site labels non-authoritative), E19 (formula 0.38.0-era) |
| **macOS `.pkg` / `.dmg`** | **Deferred** | No artifact today; revisit at faber Stage 3 under OQ8 conditions | E3/E17 (none exists; no signing/notarization anywhere) |

**Gate decision 2:** GitHub prebuilt archive is the primary channel; Homebrew is non-authoritative (decision-ledger item 3).

## OQ8 — macOS-native value (explicitly deferred with owner)

**explicitly-deferred-with-owner.** No signed/notarized `.pkg` and no `.dmg` in this stage; the archive remains the only macOS channel for the supported slice.

- Owner: **faber-onboarding Stage 3** ("primary install channel and lifecycle") — revisit after the archive lifecycle (reinstall/upgrade/downgrade/uninstall) lands and proves the payload layout.
- Conditions (provisional choice 2): start a `.pkg` only if it adds real OS integration beyond the archive installer; a `.dmg` that merely asks the user to drag a CLI binary has no default value; native packaging must not fork the payload layout or the library model (`dev-kit-contract.md` layers).
- The signing/notarization leg is owned by `component-release-streamline` (platform-builder-matrix signing identity, sibling Stage 1) — a prerequisite, not this stage's work.

## Checksum / provenance / signing policy

1. **Checksums:** SHA-256 verified against the published checksum asset before any payload is unpacked or executed (E3). Failure → abort, no partial install.
2. **Provenance:** the archive carries a provenance manifest naming version, platform triple, source commit(s), and sibling revisions. Today the archive is `faber` + `README.txt` with no provenance/license, unsigned, sibling checkouts unpinned (E3/E17 — G8). Until Stage 2 + `component-release-streamline` land, **unsigned/unproven is a labeled residual**, not a silent "supported signed channel" claim.
3. **Signing:** platform code-signing/notarization is not required for the primary archive channel in this stage; when a signed channel exists (macOS `.pkg`, OQ8), it must pass Gatekeeper/notarization before being called supported (CAMPAIGN Stage 3 overlap rule).
4. **Bootstrap ordering:** the shell bootstrap verifies TLS source + checksum **before** any fetched script executes a payload (CAMPAIGN §Install bundle contract).

## Prefix rules

- **Unprivileged user install (default):** user-local prefix (`~/.local`, `~/.faber/…` per the Stage 3 layout decision); no sudo, no hidden writes outside the prefix + platform cache + store.
- **System install:** explicit `--prefix` under an admin-writable root; receipts record the prefix.
- **Agent installs:** explicit prefix required, non-interactive, machine-readable diagnostics, stable exit codes (CAMPAIGN §Experience model "Agent installer"; Stage 9).
- Shell/PATH changes are **explicit and reversible**: the installer states what it changes; `faber self uninstall` reverses PATH/shell changes (Stage 3).

## Lifecycle owners (named)

| Lifecycle | Owner | Notes |
| --- | --- | --- |
| **Install / reinstall (idempotent)** | faber Stage 3 | Same prefix reinstall is a no-op or clean upgrade; user projects untouched |
| **Upgrade** | faber Stage 3 (`faber self update`) | Versioned side-by-side installs allowed; version-lane policy per `faber/docs/release/policy.md` (odd dev / even LTS) |
| **Repair** | faber Stage 4 (`faber doctor` repair paths) | Names missing/incompatible layers (dev-kit contract), each with one next action |
| **Downgrade** | faber Stage 3 | Pinned reinstall of an older version; side-by-side layout makes it reversible |
| **Uninstall** | faber Stage 3 (`faber self uninstall`) | Removes only product-owned files; user projects, locks, and third-party caches survive unless the user explicitly asks to remove them (CAMPAIGN §Install bundle contract) |

Proxy, offline, and non-interactive behavior with stable exit codes are Stage 3 gates (CAMPAIGN Stage 3).

## Formula-lag policy (G7)

Homebrew formula must be labeled with its true served version; the site labels the channel non-authoritative until the formula tracks the tested release. Owner: faber Stage 3 (labeling policy) + faberlang.dev Stage 8 (site wording), routed per decision-ledger G7.
