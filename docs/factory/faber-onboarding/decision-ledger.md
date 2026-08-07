# Decision Ledger — Faber Onboarding Stage 1: Dev Kit And Distribution Contract

**Status**: active — Stage 1 decision records accepted (delivery `f77180e`); Stage 2 delivery lowered ([delivery-stage2.md](delivery-stage2.md)), ready for delivery
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 1 of 10
**Delivery spec**: [delivery-stage1.md](delivery-stage1.md) (commit `f77180e`)
**Evidence base**: [golden-path-inventory.md](golden-path-inventory.md) (commit `36e6880d`), evidence E1–E20, lie list G1–G12, §8 handoff
**Evidence date**: 2026-08-07 (same as the Stage 0 inventory; live-file claims re-verified at this date)
**Date-stamped**: 2026-08-07

Every decision row carries a marking — `accepted` | `explicitly-deferred-with-owner` | `routed` — plus the §8 handoff marking and the Stage-0 evidence cited. No silent default. Owner cells are never blank. This stage writes nothing to `faber/docs/release/` (see item 1).

---

## 1. Council-4 routing authority for `faber/docs/release/` — FIRST DECISION ITEM

**Decision: accepted.** `faber/docs/release/` is governed by a **single routing authority — the coordinated product release process contract** (`component-release-streamline`'s `release-contract.md` + `release-manifest-schema.md`). The faber-onboarding dev-kit payload manifest (this campaign's Stage 2) is authored as the **"dev-kit payload section" of that single schema** (release-owned packs: launcher, core support, reference/locale packs, libraries), never as a parallel document. Onboarding Stage 1 writes nothing to `faber/docs/release/`; its payload-shape decisions (`dev-kit-contract.md`, `package-and-lock-contract.md`) define **content** only, which Stage 2 encodes into the shared schema under the process contract's review.

- **Wording parity:** this ledger's wording matches `component-release-streamline/delivery-stage1.md` (Council-4 interlock, "Default (shared with the sibling Stage-1 spec…)") and that campaign's Stage-1 ledger, so the two ledgers agree.
- **Non-overlap by construction:** this stage = decision records in this campaign dir; sibling Stage 1 = the seven release docs under `faber/docs/release/`.
- **Evidence:** `delivery-stage1.md` §Council-4 interlock; `golden-path-inventory.md` §8 (council-4 recorded, not resolved at Stage 0); `component-release-streamline/delivery-stage1.md` §Council-4 interlock.
- **Consequence recorded:** `platform-matrix.md` here (user platform slice) is distinct from the sibling's `platform-builder-matrix.md` (release production matrix); the distinction is recorded in `platform-matrix.md`, not merged.

---

## 2. Open Questions OQ1–OQ9

| OQ | Question | Stage-0 mark (§8) | Stage-1 disposition | Evidence cited |
| --- | --- | --- | --- | --- |
| OQ1 | Canonical payload representation | needs-stage-1-decision | **accepted** — multi-file versioned dev-kit archive (four-layer payload); embedded core-support materialization is a layer-delivery mechanism, not the whole contract. See `dev-kit-contract.md`. | E3, E12; CAMPAIGN §Product Definition |
| OQ2 | Norma model | needs-stage-1-decision | **accepted** — Norma = release-owned platform package (bundled, version-locked, seeded into store, pinned in lock); Triga = ordinary explicit dependency. See `package-and-lock-contract.md`. | E13, E14, G5; provisional choice 3 |
| OQ3 | Portable lock and restore | needs-stage-1-decision | **accepted** — lock identity is relocatable content identity + provider coordinates; restore = `faber restore [--project <path>]`. Absolute paths are local receipts only. See `package-and-lock-contract.md`. | E13, E15; provisional choice 4 |
| OQ4 | Git/registry bootstrap | needs-stage-1-decision | **accepted** — verified release asset is the primary bootstrap source; exact-revision Git pins acceptable; registry `name@version` deferred to `cista-dev-registry` (not blocking). See `package-and-lock-contract.md`. | E7, E13; provisional choice 5; gate 1 |
| OQ5 | Default newcomer execution | answered-by-evidence (state) + needs-stage-1-decision (choice) | **accepted** — default target = portable FHIR→FMIR, effective when released; until then the documented default is the released execution path with explicit prerequisites. See gate 3 below. | E4, E10; release-and-portable-default stages 3/6 |
| OQ6 | Dependency graph | carried-to-stage-1 | **accepted** — manifest declares direct deps; lock records the resolved transitive closure with content identity + compatibility bounds; package manifest carries transitive metadata. See `package-and-lock-contract.md`. | E14; provisional choice 4 |
| OQ7 | Init language surface | answered-by-evidence | **accepted** — default scaffold stays Latin `Salve, munde!`; locale-parameterized init **explicitly deferred with owner** `default-en-locale` (related campaign) + onboarding Stage 8 documentation. | E1, E4 |
| OQ8 | macOS-native value | carried-to-stage-1 | **explicitly-deferred-with-owner** — no `.pkg`/`.dmg` in this stage; owner = faber-onboarding Stage 3 (revisit after primary channel lifecycle lands), conditions per provisional choice 2; signing/notarization leg owned by `component-release-streamline`. See `install-channel-matrix.md`. | E3, E17 |
| OQ9 | Deferred platforms | answered-by-evidence + needs-stage-1-decision (slice) | **accepted** — supported slice = macOS arm64 + Linux x86_64; Windows and macOS Intel are named residuals (no artifact, no clean-room worker). See `platform-matrix.md`. | E3, E17; gate 3 |

---

## 3. The three gate decisions

### Gate decision 1 — Is a public registry required?

**Accepted: a public registry is NOT required for the golden path.** The local-store mechanism is self-contained (`--store` → `CISTAE_HOME` → `~/.faber/cistae`, E13). An **immutable verified bootstrap source is selected (OQ4)** so Norma/Triga acquisition does not depend on a live `cista.dev` endpoint. No blocking dependency on `cista-dev-registry`; that campaign stays a sibling. A need/mail to `cista-dev-registry` is routed only if a later stage proves a registry is required (CAMPAIGN §Dependency rules).

### Gate decision 2 — Which install channel is primary?

**Accepted: GitHub prebuilt archive is the primary channel; Homebrew is non-authoritative.** Evidence: the GitHub prebuilt archive is the only channel with a current artifact and verified checksums (E1, E3); Homebrew is explicitly non-authoritative on the site and the observed formula is 0.38.0-era (E19). Verified `curl` bootstrap is a convenience installer for the same release payload, never a second release system (CAMPAIGN Stage 3 overlap rule). Homebrew is a secondary presentation of the same payload/version, labeled non-authoritative with a formula-lag policy (G7 → faber Stage 3). macOS-native packaging is deferred (OQ8). See `install-channel-matrix.md`.

### Gate decision 3 — Default execution target for newcomers

**Accepted: the default newcomer execution target is the portable FHIR→FMIR path, aligned with `release-and-portable-default`'s portable gates and the site's prerequisites page — effective when released.** The released 1.4.0 executes via Rust/Cargo (E4/E10); the portable default exists on main, unreleased (E10). Until the portable path is released and its clean-room `no-rust` profile passes (owned by `release-and-portable-default`, stages 3/6), the documented default is the **released** execution path with its prerequisites stated explicitly (install docs + `faber doctor` say so before `run`). **No unreleased no-rust claim is made as current proof** (E4/E10 discipline, delivery-stage1.md §risks). This is a contract decision only; implementation and proof are owned by faber Stage 5 + `release-and-portable-default`.

---

## 4. Distribution contract — G1–G6 routing table

Route type: **own** = this campaign's own stage (faber control plane); **need/mail** = routed to a sibling campaign/repo. Owner cells are never blank.

| Gap | Evidence | Severity | Owner repo | Target stage/unit | Route type |
| --- | --- | --- | --- | --- | --- |
| G1 `faber explain SEM001` (site verify step) fails clean-room | E4, E8 | blocking | faber | Stage 2 "canonical payload assembly + manifest" (ship reference/locale packs; resolver reads installed location) + Stage 4 "first-run bootstrap + doctor" (doctor names missing packs) | own |
| G2 `faber explain <term>` / `--list` fails clean-room (incl. dev-repo walk-up false green) | E4, E9, E5 | blocking | faber | Stage 2 (reference pack shipped; remove dev-repo walk-up fallback in `resolve_reference_root`) + Stage 4 (doctor diagnostic) | own |
| G3 Site "First package check" (`faber check examples/ai-workbench/packages/faber-ai`) fails clean-room | E6, E16 | blocking | faber + faberlang.dev | faber Stage 2 (reader pack distribution — `la` pack ships) + faber Stage 5 (first-hour check/run on clean install) | own + need/mail to faberlang.dev Stage 8 (site example-check step re-verified against the tested release) |
| G4 Hello `faber run` needs Cargo, undocumented | E4, E10 | blocking | faber + release-and-portable-default + faberlang.dev | faber Stage 5 (default execution target) + `release-and-portable-default` (portable default release + clean-room no-rust proof, its stages 3/6) | own + need/mail to `release-and-portable-default`; need/mail to faberlang.dev Stage 8 (prerequisites page states Cargo when Rust is the documented default) |
| G5 Norma/Triga not obtainable through a product command | E7, E13, E14 | blocking | faber + cista | faber Stage 6 (Norma platform availability — release-owned platform package seeding) + faber Stage 7 (Triga acquisition via the OQ4 verified bootstrap) + cista (need: `faber install` accepts verified release-asset and exact-revision pins; interface-only install must not probe rustc — aligned with `release-and-portable-default` Stage 4) | own + need/mail to cista |
| G6 Multi-locale start track stale (6/7 locales at 1.2.0) | E2 | blocking (locale outcome) | faberlang.dev + faber | faberlang.dev Stage 8 (site locale parity at the tested release) + faber Stage 8 (CLI locale docs/parity) + faber Stage 2 (packs make non-default locales runnable) | need/mail to faberlang.dev + own |

**Stop-if honored:** no `faber install triga`-style claim is routed as a supported path without an immutable verified acquisition source (OQ4 decided; Stage 7 gate; CAMPAIGN §Stop conditions / Stage 7 "Stop if"). The G5 cista need is exactly that hardening.

## 5. Residuals G7–G12 — routed or documented with owners

| Gap | Evidence | Severity | Owner repo | Target stage/unit | Route type |
| --- | --- | --- | --- | --- | --- |
| G7 Installed binary drift (PATH 1.3.0 vs site 1.4.0; formula 0.38.0-era) | E19 | residual | faber + faberlang.dev | faber Stage 3 (formula-lag labeling policy on the Homebrew channel) + faberlang.dev Stage 8 (site syncs from release metadata) | own + need/mail to faberlang.dev |
| G8 Release archive not a self-consistent product artifact (no provenance/license, unsigned, sibling checkouts unpinned) | E3, E17 | residual | faber + component-release-streamline | faber Stage 2 (dev-kit payload manifest section — content, provenance, license) + `component-release-streamline` (shared `release-manifest-schema.md` + provenance/authenticity — sibling Stage 1) | own + sibling interlock |
| G9 Locks not portable / reproducible (absolute paths, version drift) | E13, E15 | residual | faber + cista | faber Stage 1 (package-and-lock-contract.md — this unit) + cista (need: lock writer emits relocatable content identity, not absolute paths) + faber Stage 7 (relocation/offline replay proof) | own + need/mail to cista |
| G10 CLI locale flags fail clean-room (reader packs monorepo-only) | E10, E20 | residual | faber | Stage 2 (pack distribution — same fix as G1) + Stage 8 (CLI locale docs) | own |
| G11 Install path not container-verified | E18 | residual | faber + faberlang.dev | faber Stage 10 (continuous honesty CI) + faberlang.dev Stage 8 (container-verified quickstart row) | own + need/mail to faberlang.dev |
| G12 Platform slice not explicit | E3, E17 | residual | faber | Stage 1 (platform-matrix.md — this unit) + Stage 3 (per-platform channel/lifecycle) + Stage 10 (per-platform clean-room evidence) | own |

---

## 6. Internal proof

- Every decision row above cites Stage-0 evidence (E#/G#) or a live file. ✓
- Every G1–G6 owner is repo + stage/unit, never blank. ✓
- No `faber/docs/release/` path appears in this commit; this stage writes nothing there (item 1). ✓
- Ledger date-stamped 2026-08-07. ✓
- No Stage-2 payload manifest pre-written; no `release-manifest-schema.md` content pre-written (sibling owns the schema). ✓
