# `faber.lock` file contract

Durable contract for the `faber.lock` file format: the file the Cista package
manager writes and the Faber product CLI reads. Recorded from the Stage 2
contract freeze (faber-radix-recomposition, S2-U1), 2026-08-12. This file is
the private-workspace home of the schema note; the public Faber landing and
the Cista writer mirror this content (before either side ships a new required
field).

## Field set — frozen baseline

The file contract is the **field set as it exists today**: **13
`LockedPackage` fields** (12 required + the optional `content_sha256`;
+ 1 `packages` container on `FaberLock`),
field-identical across both live structs: the Faber reader
(`radix/crates/radix-program/src/lockfile.rs`) and the Cista writer
(`cista/src/faber_lock.rs` `LockedPackage` @ `bc384b4`).

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
| 13 | `content_sha256` | `content_sha256: String` (writer, `#[serde(default, skip_serializing_if = "String::is_empty")]`) / `content_sha256: Option<String>` (reader, `#[serde(default)]`) | Optional. SHA-256 over the installed package snapshot; normative stream defined in the recorded stage decision below. Writer records it at install; reader validates shape and fails closed on a malformed value (`locked_content_sha256_malformed`); absence means the lock predates the field |
| — | `package` | `packages: Vec<LockedPackage>` (serde `rename = "package"`, `#[serde(default)]`) | The `[[package]]` container on `FaberLock` (the +1 container field; not a `LockedPackage` field) |

## Both-struct cross-check

| Check | Faber reader (`radix/crates/radix-program/src/lockfile.rs`) | Cista writer (`cista/src/faber_lock.rs` `LockedPackage` @ `bc384b4`) | Match |
| --- | --- | --- | --- |
| `LockedPackage` field set | `name`, `version`, `source`, `package_root`, `kind`, `target_language`, `target_triple`, `target_manifest`, `interface_root`, `artifact`, `crate_name`, `rustc`, `content_sha256` — **13 fields** (12 required + optional `content_sha256`) | identical set and order — **13 fields** | **identical** |
| `FaberLock` container | `packages: Vec<LockedPackage>` (`#[serde(default, rename = "package")]`) | same (`#[serde(default, rename = "package")]`) | **identical** |
| `deny_unknown_fields` | both structs | both structs | **identical** |
| `crate` serde rename | `#[serde(rename = "crate")] pub crate_name` | same | **identical** |
| `artifact` defaulting | field **required** in TOML (no `#[serde(default)]`) | `#[serde(default)]` | **compatible margin** — writer tolerates its own older records; writer always serializes `artifact`, so the reader never observes it missing |
| `content_sha256` shape | `Option<String>` + `#[serde(default)]`; shape-validated in `lock_index` (fail closed) | `String` + `#[serde(default, skip_serializing_if = "String::is_empty")]` | **compatible** — writer omits the field when empty; reader defaults it absent and rejects a malformed value |
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
  **Superseded 2026-09-21 (gate-1, need S-D3):** the optional
  `content_sha256` field is added by the recorded stage decision below; the
  `schema_version` recommendation stands.
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

## Recorded stage decision — `content_sha256` (2026-09-21, gate-1 S-D3)

Supersedes the Stage-2 "No field" freeze for exactly one field:
`content_sha256` (field 13) is added as an **optional** field.
`deny_unknown_fields` stays on both structs. Both directions are tolerant by
construction: a lock without the field reads identically on both sides, the
writer omits the field when it is empty, and the reader defaults it absent.
A reader build that has not picked up the field rejects the row (its
`deny_unknown_fields` is unchanged), which is the documented note-before-ship
rule doing its job. No `schema_version` is added. Writer surface: cista
`src/faber_lock.rs` (`LockedPackage`, `staged_content_sha256`,
`locked_from_install`). Reader surface:
`radix/crates/radix-program/src/lockfile.rs` (`LockedPackage`, `lock_index`).

### Digest definition (normative)

`content_sha256` is the lowercase-hexadecimal SHA-256 of the package
snapshot's **canonical content stream**, defined as:

1. Enumerate every **regular file** under the installed package root — the
   store directory named by the row's `package_root`, identical to the
   staged snapshot the writer commits. Directories contribute nothing; an
   entry that is neither a regular file nor a directory fails the install.
2. Key each file by its `/`-separated path relative to that root; sort the
   keys by **raw byte order** (not locale collation).
3. Concatenate, for each file in that order: the relative path's UTF-8
   bytes, one `0x00` byte, the file's byte length as an **unsigned 64-bit
   big-endian** integer, and the file's bytes. Nothing else enters the
   stream.

The writer computes the digest from the staged snapshot immediately before
committing it to the store and records it in the row it writes for that
install. The reader performs **shape validation only** — a present value
must be exactly 64 lowercase hexadecimal characters, else
`locked_content_sha256_malformed` fails the read closed — and never re-hashes
the store at compile time (a separate cost ruling). Absence of the field
means the lock predates the field. An install-time consumer gate,
compile-time verification, and any registry-side digest are separate
decisions; all of them bind to the stream defined here, and no cheaper
per-file manifest hash substitutes for it.
