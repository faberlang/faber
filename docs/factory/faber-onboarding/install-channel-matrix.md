# Install Channel Matrix — Channel Dispositions, Trust Policy, Prefix, Lifecycle

**Status**: active — Stage 1 decision record (gate 2 + OQ8); Stage 3 A6 OQ8 revisit closed **explicitly-deferred-with-owner** (2026-08-11)
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 1 of 10; Stage 3 A6 decision
**Delivery spec**: [delivery-stage1.md](delivery-stage1.md); Stage 3 unit A6 in [delivery-stage3.md](delivery-stage3.md)
**Date-stamped**: 2026-08-07 (Stage 1); OQ8 A6 revisit 2026-08-11
**Evidence**: [golden-path-inventory.md](golden-path-inventory.md) E1, E3, E17, E19; CAMPAIGN §Product Definition provisional choice 1/2; `faber/docs/release/policy.md` (normative version lanes — cited, not duplicated); Stage 3 archive lifecycle A1–A5 landed; `faber/docs/release/platform-builder-matrix.md` (macOS signing identity — none today)

## Channel dispositions

| Channel | Disposition | Role | Evidence |
| --- | --- | --- | --- |
| **GitHub prebuilt archive** | **Primary** | Source of truth; versioned, checksummed release bundle per platform triple | E1 (site documents 1.4.0 archive), E3 (assets + SHA-256 verified); Stage 3 A1–A5 lifecycle |
| **Verified `curl` bootstrap** | **Secondary (convenience)** | Downloads the fixed release payload; never a second release system (CAMPAIGN Stage 3 overlap rule) | E1, E3; provisional choice 1; A1 install-faber |
| **Homebrew** | **Secondary, non-authoritative** | Presentation of the same payload/version; explicitly labeled non-authoritative; formula-lag policy (G7) | E1 (site labels non-authoritative), E19 (formula 0.38.0-era) |
| **macOS `.pkg` / `.dmg`** | **Deferred** (not supported) | No artifact; no "supported" label; A6 OQ8 revisit keeps deferral — see OQ8 section | E3/E17 (none exists); platform-builder-matrix signing gap; provisional choice 2 |

**Gate decision 2:** GitHub prebuilt archive is the primary channel; Homebrew is non-authoritative (decision-ledger item 3). Native macOS packaging is not a supported channel.

## OQ8 — macOS-native value (Stage 3 A6 revisit — explicitly deferred with owner)

**Disposition: explicitly-deferred-with-owner** (Stage 3 unit A6, 2026-08-11). No `.pkg`, no `.dmg`, no native packaging implementation in this campaign stage. The **GitHub prebuilt archive** (plus the verified `curl` bootstrap convenience path for the same payload) remains the **only** macOS install channel for the supported slice. An unsigned native channel is **never** labeled supported.

### A6 revisit basis (archive lifecycle A1–A5 landed)

Stage 1 deferred OQ8 to Stage 3 after the primary-channel lifecycle landed. That lifecycle is now product-owned on the archive path:

| Unit | Lifecycle capability proven on the archive channel |
| --- | --- |
| A1 | Verified bootstrap installer (`scripta/install-faber`) — checksum before unpack, user-local prefix, non-interactive |
| A2 | Reinstall idempotency + `faber self update` |
| A3 | Downgrade policy (fail-closed on incompatible packs) |
| A4 | Repair policy + typed install-side failure classes |
| A5 | `faber self uninstall` — product-owned only; user projects/caches survive by default |

### Conditions re-checked (provisional choice 2 — not met)

1. **Real OS integration:** start a `.pkg` only if it adds real OS integration **beyond** the archive installer. A1–A5 already cover user-local prefix, PATH/shell edits, receipts, reinstall/upgrade/downgrade/repair/uninstall on the **same logical layout** (`dev-kit-contract.md` layers). No concrete OS-integration need (e.g. privileged system service, App bundle UX required by a product surface, MDM-only distribution) is named that the archive cannot meet.
2. **`.dmg`:** a disk image that merely asks the user to drag a CLI binary still has **no default value**.
3. **Layout invariant:** native packaging must **not** fork the payload layout or the library model. Any future native path must install the **same logical layout** as the archive (CAMPAIGN Stage 3 overlap rule).
4. **Gatekeeper / notarization (overlap rule restated):** any signed/native channel introduced later must pass **Gatekeeper and notarization** **before** being called supported. The primary archive channel does not require platform code-signing in this stage; **unsigned/unproven remains a labeled residual (G8)**, never a silent "supported signed channel" claim.
5. **Signing identity:** the signing/notarization leg is owned by **`component-release-streamline`** (`platform-builder-matrix.md` / release-contract — controlled signer; credentials never in general build jobs). **None exists today** (recorded gap). This unit does **not** invent a signing identity.

### Owners (no blank owner cell)

| Leg | Owner | Role |
| --- | --- | --- |
| **Controlled signing identity + notarization path** | `component-release-streamline` | Prerequisite for any future signed `.pkg` (or other signed native channel) |
| **Product reopen / acceptance of a native channel** | faber-onboarding | Reopen OQ8 only when **both** (1) a controlled signer exists **and** (2) a concrete OS-integration need the archive cannot meet is named; record accepted or re-defer with owner; never ship an unsigned "supported" native channel |
| **Stage 3 A6 (this revisit)** | closed | Decision only — no `.pkg`/`.dmg` artifact |

### What is not claimed

- No `.pkg` or `.dmg` artifact is produced or planned by this unit.
- macOS `.pkg` / `.dmg` is **not** a supported install channel.
- Homebrew remains secondary and non-authoritative (separate G7/R2 track).

## Checksum / provenance / signing policy

1. **Checksums:** SHA-256 verified against the published checksum asset before any payload is unpacked or executed (E3). Failure → abort, no partial install.
2. **Provenance:** the archive carries a provenance manifest naming version, platform triple, source commit(s), and sibling revisions. Today the archive is `faber` + `README.txt` with no provenance/license, unsigned, sibling checkouts unpinned (E3/E17 — G8). Until Stage 2 + `component-release-streamline` land, **unsigned/unproven is a labeled residual**, not a silent "supported signed channel" claim.
3. **Signing:** platform code-signing/notarization is **not** required for the primary **archive** channel. When a signed **native** channel exists (future macOS `.pkg` under OQ8 reopen), it **must pass Gatekeeper/notarization before being called supported** (CAMPAIGN Stage 3 overlap rule). An unsigned native package must never carry a "supported" label.
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
