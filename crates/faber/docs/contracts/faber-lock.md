# `faber.lock` file contract

Durable contract for the `faber.lock` file format: the file the Cista package
manager writes and the Faber product CLI reads. Recorded from the Stage 2
contract freeze (faber-radix-recomposition, S2-U1), 2026-08-12. This file is
the private-workspace home of the schema note; the public Faber landing and
the Cista writer mirror this content (before either side ships a new required
field).

## Field set — frozen baseline

The file contract is the **field set as it exists today**: **12
`LockedPackage` fields** (+ 1 `packages` container on `FaberLock`),
field-identical across both live structs: the Faber reader
(`crates/faber/src/package/lockfile.rs`) and the Cista writer
(`cista/src/faber_lock.rs` @ `9355288`).

| # | TOML key | Rust field (both sides) | Semantics (from the reader's consumers + writer's builder) |
| --- | --- | --- | --- |
| 1 | `name` | `name: String` | Locked package name; the lock index key and `faber.toml [dependencies]` join key |
| 2 | `version` | `version: String` | Exact pinned version; `faber.toml` pins are compared against it (`dependency_version_mismatch`) |
| 3 | `source` | `source: String` | Install source descriptor (writer emits `path:<abs>`); informational on the read side |
| 4 | `package_root` | `package_root: String` | Absolute package root under the store; validated as a directory (`locked_package_root_missing`) |
| 5 | `kind` | `kind: String` | `source` / `lib` (compiled into the app graph) vs prebuilt kinds (artifact required) |
| 6 | `target_language` | `target_language: String` | Target language of the installed build (e.g. `rust`) |
| 7 | `target_triple` | `target_triple: String` | Target triple of the installed build |
| 8 | `target_manifest` | `target_manifest: String` | Absolute `cista.toml` path in the store target dir; file-checked for prebuilt kinds (`locked_target_manifest_missing`) |
| 9 | `interface_root` | `interface_root: String` | Absolute interfaces dir under the store; directory-checked (`locked_interface_root_missing`) |
| 10 | `artifact` | `artifact: String` | Absolute artifact path; empty for interfaces-only packages; file-checked for prebuilt kinds (`locked_artifact_missing`) |
| 11 | `crate` | `crate_name: String` (serde `rename = "crate"`) | Rust crate name for the installed package |
| 12 | `rustc` | `rustc: String` | Rustc version used to build the locked artifact |
| — | `package` | `packages: Vec<LockedPackage>` (serde `rename = "package"`, `#[serde(default)]`) | The `[[package]]` container on `FaberLock` (the +1 container field; not a `LockedPackage` field) |

## Both-struct cross-check

| Check | Faber reader (`crates/faber/src/package/lockfile.rs`) | Cista writer (`cista/src/faber_lock.rs` @ `9355288`) | Match |
| --- | --- | --- | --- |
| `LockedPackage` field set | `name`, `version`, `source`, `package_root`, `kind`, `target_language`, `target_triple`, `target_manifest`, `interface_root`, `artifact`, `crate_name`, `rustc` — **12 fields** | identical set and order — **12 fields** | **identical** |
| `FaberLock` container | `packages: Vec<LockedPackage>` (`#[serde(default, rename = "package")]`) | same (`#[serde(default, rename = "package")]`) | **identical** |
| `deny_unknown_fields` | both structs | both structs | **identical** |
| `crate` serde rename | `#[serde(rename = "crate")] pub crate_name` | same | **identical** |
| `artifact` defaulting | field **required** in TOML (no `#[serde(default)]`) | `#[serde(default)]` | **compatible margin** — writer tolerates its own older records; writer always serializes `artifact`, so the reader never observes it missing |
| Derives | `Debug, Clone, Deserialize` (reader-only) | `Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize` (writer-side) | **compatible** — field set is the contract, not the Rust type |

## Rule set

- **Cista is the schema evolution lead.** New required fields ship from the
  Cista writer side first; the Faber reader follows.
- **The Faber reader stays `deny_unknown_fields`-compatible.** A lock the
  writer produces must always deserialize with the reader's strict schema.
  This is what makes "no new required field without a documented note" a hard
  rule, not a convention.
- **A documented note precedes any new required field from either side.** The
  note lives in Cista (and the public Faber landing) before either side ships
  the field.
- **No field and no `schema_version` are added in Stage 2.** Default
  recorded: an explicit `schema_version` field is **recommended but not
  required while the field set stays stable**.
- **Faber never writes `faber.lock`**; the product keeps its
  Deserialize-only reader and its diagnostics vocabulary
  (`missing_faber_lock`, `dependency_missing_from_lock`,
  `dependency_version_mismatch`, `locked_package_root_missing`, …) unchanged.

## Change procedure

A new required field ships in this order:

1. **Documented note first.** A note is published in Cista (and the public
   Faber landing) before either side ships the field.
2. **Cista ships first.** The new field lands in the Cista writer side
   first; the Faber reader follows.
3. **Reader stays `deny_unknown_fields`-compatible.** A lock the writer
   produces must always deserialize with the reader's strict schema.
4. **`schema_version` when the set moves.** An explicit `schema_version`
   field is recommended (not required) while the field set stays stable;
   the field set is the contract, not a version number.

## Stage 2 record (faber-radix-recomposition)

The field set is **unchanged** by the recomposition cut: **12 `LockedPackage`
fields + the `packages` container**, field-identical on both sides. **No
field and no `schema_version` are added now.**
