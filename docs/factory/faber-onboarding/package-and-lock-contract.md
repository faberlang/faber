# Package And Lock Contract — Norma Model, Portable Lock, Bootstrap, Semantics

**Status**: active — Stage 1 decision record (OQ2/OQ3/OQ4/OQ6); implementation lands in faber Stages 6–7 + cista needs
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 1 of 10
**Delivery spec**: [delivery-stage1.md](delivery-stage1.md)
**Date-stamped**: 2026-08-07
**Evidence**: [golden-path-inventory.md](golden-path-inventory.md) E7, E13, E14, E15; CAMPAIGN §Library package contract; provisional choices 3/4/5

## OQ2 — Norma model: **release-owned platform package**

**Accepted.** One semantic model, even though installers may prefetch it:

- **Norma is release-owned platform content** — part of the compatible platform set for a Faber release (provisional choice 3). It ships with the dev-kit payload (dev-kit layer 4), is **seeded into the store** by the install/bootstrap step, and is recorded in `faber.lock` as a pinned package whenever a project uses it.
- Norma is **not** a normal explicit dependency that a newcomer must discover and fetch; the store's `PLATFORM_DEFAULT_PACKAGES = ["norma"]` behavior (E13) becomes "release-owned seeding" instead of "discovered from a registry". No ambient monorepo path (`FABER_LIBRARY_HOME` stays developer-only).
- Live manifest: `norma` `0.1.0`, `faber_min = "0.38.0"`, rust `mode = "compile"` (E14). The rust compile-at-install prerequisite is a known gap — interface/portable installs must not probe rustc (routed: cista + `release-and-portable-default` Stage 4; G5).
- **Triga and third parties:** ordinary explicit `[dependencies]` entries plus a reproducible lock. Installing them grants **no ambient import access** (provisional choice 3/4). Stage 7 proves `importa ex "triga:…"` with an exact compatible pin.

## OQ3 — Portable lock and restore

**Accepted.**

- **Lock identity:** `faber.lock` records, per package: **provider coordinate** (e.g. `triga:geometria`), **name + exact version**, **source origin**, **immutable content identity** (exact Git revision pin or content digest), and **compatibility metadata** (`faber_min` bounds, target/interface roots) (provisional choice 4). This identity is **relocatable** — no absolute paths.
- **Absolute developer paths are local receipts only:** explicit local-development locks/receipts may carry machine paths (E15's vivilite relative `../sqlite` and triga absolute `/Users/ianzepp/…` examples); committed/portable locks use content identity or a deterministic re-resolution rule (E13/E15 — G9). The 0.1.0-vs-0.2.0 example drift is recorded drift evidence, not a release-ready distribution proof (E15).
- **Restore command:** `faber restore [--project <path>]` materializes the locked closure from the store, falling back to the verified bootstrap source on a first fetch, and failing closed with one actionable message if a package cannot be acquired. This is the command named in the campaign's product-command list; exact CLI shape is implementation (Stage 7).

## OQ4 — Git/registry bootstrap source and pin rule

**Accepted.**

| Source | Disposition | Pin rule |
| --- | --- | --- |
| **Verified release asset** (GitHub prebuilt archive) | **Primary bootstrap source** | Checksum-verified release asset; exact artifact + digest (E3) |
| **Git** | Acceptable (explicit) | **Exact revision + content digest required**; mutable branches are not acceptable golden-path lock sources (provisional choice 5). Today's `faber install` git path is unpinned by design (E13) — hardening is routed (G5 cista need) |
| **Registry `name@version`** | **Deferred** | Needs a proven remote registry; owned by `cista-dev-registry` (sibling). Not blocking; no unproven `cista.dev` endpoint as a golden-path source |

Bare names keep failing closed with an actionable pin error (E7). **Gate decision 1:** a public registry is not required — the immutable verified bootstrap source above is selected.

## OQ6 — Dependency-graph placement

**Accepted.**

- **Project manifest** (`faber.toml`): declares **direct** dependencies (`[dependencies]`, e.g. `sqlite = "0.1.0"` per E16).
- **Lock** (`faber.lock`): records the **full resolved transitive closure** — provider coordinates, exact versions, content identity, target/interface roots, compatibility bounds.
- **Package manifest** (`cista.toml`): carries the package's own transitive metadata, declared version, and compatibility bound (`faber_min`, E14). Regular source-package manifests today do not express a general transitive graph (E14 — third-party ecosystem unexpressed); that expression is a cista-store evolution routed with G5/G9.
- **Conflict/coexistence:** exact pins; two versions may coexist for different projects (Stage 7 gate). Update selection: `faber update [<package>]` changes only the requested dependency closure.
- **Target variants:** per-package target manifests; rust `mode = "compile"` is an explicit consumer choice, not an install-time universal (E14).

## Semantics

- **Relocation:** a project directory moves freely; the lock stays valid because identity is content-based, not path-based. Proof: Stage 7 gate.
- **Offline:** after one verified fetch populates the store, `faber restore`/`check`/`run` replay from the populated store without network (CAMPAIGN `offline-restored` profile).
- **Update:** intentional via `faber update`; lock changes only the requested closure; yanks/revocation follow the store contract (CAMPAIGN §Library package contract).
- **Integrity:** content digests verified at fetch and at use; tampered content fails closed before import. Trust policy: immutable pins, checksum/signature verification, archive traversal/symlink defenses, and **no install-time code execution** unless a later explicit contract permits it (CAMPAIGN §Library package contract; `release-and-portable-default` Stage 4 separates metadata/interface acquisition from target-native preparation).
- **Compatibility:** `faber_min`-style bounds enforce launcher↔package compatibility; incompatible versions fail with a repair/upgrade message (Stage 6/7 gates; `faber/docs/release/policy.md` standard-package compatibility).
- **Store ownership/discovery:** `$CISTAE_HOME` may select a store, but project compilation consumes the **lock contract** — never ambient discovery from the store (CAMPAIGN §Library package contract; E13).

## Interlock

- Payload content for release-owned packs (Norma, reference/locale) lands in the shared release-manifest schema section (decision-ledger item 1); this doc defines the package/lock **semantics**, `release-manifest-schema.md` (sibling) owns the release **pins** — the distinction is recorded, not merged.
- G5/G9 routing: see decision-ledger items 4–5.
