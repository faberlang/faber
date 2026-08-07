# Dev Kit Contract — Required And Optional Payload And Discovery Rules

**Status**: active — Stage 1 decision record (OQ1); Stage 2 encodes this content into the shared release-manifest schema
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 1 of 10
**Delivery spec**: [delivery-stage1.md](delivery-stage1.md)
**Date-stamped**: 2026-08-07
**Evidence**: [golden-path-inventory.md](golden-path-inventory.md) E3, E12; CAMPAIGN §Product Definition

## Decision: canonical payload representation (OQ1)

**Accepted.** The canonical dev-kit payload is a **versioned multi-file dev-kit archive** — one release bundle per platform triple, containing all four dev-kit layers. It is *not* "one binary that discovers everything from a build path".

- The 2026-08-07 release archive ships the launcher only (`faber` + `README.txt`, E3); core support is embedded in the binary and materialized to the platform cache (E12). That is **evidence of a working layer-delivery mechanism**, not the whole contract.
- The contract: every layer has a deterministic installed location, is discoverable **without hidden environment variables**, and a diagnostic names any missing or incompatible layer.
- The archive is the release-time representation; the **installed layout** is the on-disk contract below. Stage 2 encodes archive + layout into the "dev-kit payload section" of the shared release-manifest schema (`faber/docs/release/`, governed per decision-ledger item 1). This doc defines content only.

## The four dev-kit layers

| Layer | Required content | Installed location (deterministic) | Lifecycle rule |
| --- | --- | --- | --- |
| **1. Launcher** | `faber` CLI + version metadata | `<prefix>/bin/faber` | Installed, upgraded, removed as one versioned product |
| **2. Core support** | Runtime/host material needed by the documented default `check`/`run` path (faber-runtime, radix-runtime-contract, hosts crates) | Materialized to the platform cache (content-hash-keyed, immutable) — today `~/Library/Caches` / `~/.cache` (E12); the stage-2 manifest records the materialization rule | Version-compatible with the launcher; no sibling-checkout discovery |
| **3. Reference and locale packs** | Data required by `faber explain` and the documented code/diagnostic locales | `<prefix>/share/faber/reference` and `<prefix>/share/faber/locale/<locale>/` (or equivalent relocatable layout per the manifest) | Shipped and versioned with the launcher; fetched by an explicit verified bootstrap step only if not shipped |
| **4. Faber libraries** | Norma (release-owned platform package); optional packages (Triga, third parties) | Resolved through the package contract into the store (`$CISTAE_HOME` / `~/.faber/cistae`), recorded in `faber.lock` | Resolved through a declared package contract, never an ambient monorepo path |

### Layer owners

- **Launcher** — faber (release assembly, Stage 2 payload; lifecycle faber Stage 3 "primary install channel").
- **Core support** — faber Stage 2 (payload packaging + manifest section), content assembled from `faber-runtime`/`hosts` via `core-support-manifest.txt` (E12/E17); faber Stage 4 (doctor verifies materialization).
- **Reference and locale packs** — faber Stage 2 (packs shipped in payload; resolver reads installed location, E8/E9), faber Stage 4 (doctor names missing packs). Packs originate in the radix tree (`radix/stdlib/locale/*`, E20) and are release-owned once shipped.
- **Faber libraries** — faber Stage 6 (Norma platform availability) + Stage 7 (Triga/third-party acquisition); cista (store + lock contract) via the G5 need.

## Discovery rules

1. **No hidden environment variables.** The launcher locates every layer from (a) the install prefix recorded in the installer receipt, then (b) platform-default locations. `FABER_REFERENCE_ROOT` (E9) remains an **explicit, documented developer-only override** — never required, never ambient.
2. The installed layout must resolve `faber --version`, reference/locale lookup, and the selected core-support/Norma model **without siblings or environment overrides** (CAMPAIGN Stage 2 gate).
3. Locale packs are resolved from the installed `<prefix>/share/faber/locale/<locale>/` — the `CARGO_MANIFEST_DIR`-relative baked path (E8) and dev-repo walk-up (E5/E9) are removed for installed binaries (G1/G2/G10 owners).
4. Core support is materialized by content hash (immutable cache, E12); a doctor check verifies it (Stage 4; G11/G12 side).

## Diagnostic: naming missing or incompatible layers

A doctor-equivalent surface (faber Stage 4, `faber doctor`) reports per layer: present / missing / incompatible, with **one next action** per failure and a nonzero exit. Failure classes: missing launcher metadata, missing/corrupt core-support materialization, missing/incompatible reference or locale pack, missing Norma platform package, unwritable prefix/store. This satisfies CAMPAIGN "Fail closed, explain next".

## Core-vs-optional taxonomy

| Class | Layers | Missing = |
| --- | --- | --- |
| **Core (required)** | Launcher, core support, reference/locale packs | Broken install → repair path (doctor), never a silent fallback |
| **Optional (platform-recommended)** | Norma (release-owned platform package) | Actionable acquisition/repair message; imports of `norma:*` fail closed with a next action (G5 → Stage 6) |
| **Optional (ordinary dependencies)** | Triga and all third-party packages | Explicit `[dependencies]` entry + lock; install never grants ambient import access (provisional choice 3/4) |

Every layer has an owner (table above). The taxonomy is owned by faber Stage 2 (payload) and Stage 6/7 (library acquisition); the package/lock half lives in `package-and-lock-contract.md`.

## Interlock

- Payload content lands in Stage 2 as a **section of the single release-manifest schema** (`component-release-streamline` `release-manifest-schema.md`), not a parallel document (decision-ledger item 1).
- `platform-matrix.md` records which platform rows consume this layout.
