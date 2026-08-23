# FPF-1: FHIR package format field spec (unit schema v5)

**Status**: specified — named-MessagePack field spec for every v5 fact family
**Created**: 2026-08-23
**Unit**: FPF-1 in [`goal.md`](goal.md) (P3 lowering, artifact `afcc33d`)
**Authority**: operator addendum
[`../fcmp-profile-1/addendum.md`](../fcmp-profile-1/addendum.md);
goal Ground-truth section (re-verified 2026-08-23 at radix `333177658` /
faber `b4ae10c`)
**Supersedes the schema pinning of** [`fs-1-field-spec.md`](fs-1-field-spec.md)
(written at unit schema v3) — that inventory's per-node field lists remain a
useful cross-reference, but its version claims and family table are no longer
authoritative. This document is the v5 authority FPF-2/FPF-3 implement
against.

**This unit does not**: implement a codec, choose final FCMP envelope framing
(fcmp-profile-1 owns it; OQ-2 binds only after FPF-2), ratchet any version
constant, or switch writers. Those are FPF-2/FPF-3/FPF-4/FPF-5.

---

## 0. Schemas in force

| Envelope | Constant | Value | Defining type |
| --- | --- | --- | --- |
| Unit artifact | `radix_hir_fhir::SCHEMA_VERSION` | `5` | `artifact.rs` (`SCHEMA_VERSION`; v5 = `Transfer`/`FirstMatch` serde-ordinal shift, 79c36d9fb) |
| Package envelope | `radix_hir_fhir::PACKAGE_SCHEMA_VERSION` | `1` | `package.rs` (`PACKAGE_SCHEMA_VERSION`) |

Decode is fail-closed on both (`UnsupportedSchemaVersion`,
`UnsupportedPackageSchemaVersion`); no compatibility decoders exist. The
MessagePack value inherits exactly these two gates: the version is a **named
field read first**, not inferred from framing.

## 1. Wire rules (binding on FPF-2/FPF-3)

1. **Named fields, not compact arrays.** Every record below is a MessagePack
   map (`map<N>`) whose keys are the serde field names of the defining type
   (already lowercase ASCII). Positional/tuple encoding is out of spec.
2. **Versioning.** `schema_version` (unit) and `package_schema_version`
   (envelope) are `uint` fields, first-read on decode; mismatch fails closed
   before any other field is interpreted.
3. **No semantic facts via float opcodes.** Numeric widths, tensor
   dtype/shape, and buffer element types are type-table **value** facts under
   named keys (see §5). Never encoded as, or recovered from, MessagePack
   float opcodes. FCMP "float fields are float64" applies only to genuine
   IEEE `f64` values.
4. **Enums** encode as a map `{ "tag": <variant name string>, ...payload
   fields }`. Unit payloads carry their variant names; ordinal drift (the v5
   ratchet cause) is thereby neutralized on the MessagePack wire, but the
   version gate stays because postcard remains the live wire until FPF-5.
5. **Options** encode as the bare payload when `Some`, `nil` when `None`.
6. **Integers** encode as MessagePack `uint`/`int` matching the Rust width
   (`u32`→uint, `i64`→int, `u64`→uint). **Bools** as `bool`; **strings** as
   `str` (UTF-8); **byte payloads** as `bin`.
7. **Determinism.** Where the defining type's writer canonically sorts
   (module table by `path`, dependencies by `name`, `local_links` by
   `binding`), the MessagePack encoder emits the same canonical order.

## 2. Unit artifact — `HirArtifact` (13 families)

Defining type: `radix-hir-fhir/src/artifact.rs` `HirArtifact`. All 13 serde
fields are named top-level keys of the unit `value` map.

| # | Named key | Defining type | MessagePack type rule | Rehydration rule |
| --- | --- | --- | --- | --- |
| 1 | `schema_version` | `HirArtifact::schema_version: u32` | `uint`; must equal `5` or fail closed | Checked before payload decode; gates rows 2–13 |
| 2 | `source_identity` | `SourceIdentity` (artifact.rs) | map `{ content_hash: str, relative_path: str }` | Verbatim; `content_hash` is cross-checked against the package manifest `source_hash` (§4) |
| 3 | `hir` | `radix_hir::HirModule` (direct serde) | map of the `HirModule` serde fields; nested HIR nodes as nested maps per rule §1.1/§1.4 | Rebuilds the HIR tree as-is; `HirId` values inline (§6.4), `Symbol`/`TypeId` as indices (§6) |
| 4 | `presentation` | `radix_hir::HirPresentation` (nodes.rs) | map `{ owners: [HirTriviaOwner], attachments: [HirTriviaAttachment], block_ends: [HirBlockEnd], program_end: HirModuleEnd, entry_start_anchor: uint? }` | Rebuilt as the presentation sidecar, inseparable from `hir`; `entry_start_anchor` is an `HirSourceAnchorId` into the anchor arena (§6.5) |
| 5 | `cli_program` | `Option<CliProgramWire>` (artifact.rs) | `nil` or map of `CliProgramWire` serde fields (options/operands/commands as named-key maps; `CliDefaultWire`/`CliExitWire`/`CliTypeWire`/`CliModeWire` enums per §1.4) | Rehydrates the CLI contract when the unit declares `incipit argumenta`; else absent |
| 6 | `types` | `TypeTableSnapshot` (radix-types types.rs) | map `{ types: [Type], indices: [IndexExpr], unspecified_shape: uint?, next_index_var: uint }` — see §5 | Rebuilds the type + index arenas; `TypeId`/`IndexId` become valid indices again in declaration order (§6.1) |
| 7 | `interner` | `Vec<String>` (artifact.rs) | array of `str`, in order | Rebuilds the string interner; every `Symbol` index elsewhere resolves through it (§6.3) |
| 8 | `resolver` | `ResolverSnapshot` (artifact.rs) | map `{ namespace_exports, file_interfaces, used_namespaces, imported_nominal_types, ambiguous_imported_nominal_types }`, each list-of-named-maps (`Symbol` as index, `TypeId` as index) | Rebuilds import surface only; scopes/shadow/builtin caches are excluded and recomputed on load (CTM E.2, CXO F4) |
| 9 | `libraries` | `radix_hir::LibraryRegistry` | map of the `LibraryRegistry` serde fields | Rehydrates library-import registry verbatim |
| 10 | `function_facts` | `FunctionFactTableWire` (artifact.rs) | map `{ entries: [ { def_id: uint, facts: FunctionFactWire } ], entry: FunctionFactWire? }`; `FunctionFactWire` fields (`direct_failure`, `may_fail`, `is_async`, `is_generator`, `requires_await`, `call_edges: [uint]`, `captures`, `param_modes`, `err_ty: uint?`) as named keys | Rehydrates effect/capture/call-graph facts keyed by `DefId`; `err_ty` resolves through row 6 |
| 11 | `resolved_uses` | `Vec<ResolvedUseWire>` (artifact.rs) | array of maps `{ key, kind, enclosing }` (`key` = `Local(def_id)`/`Portable(str)` enum per §1.4) | Rehydrates resolved-use table verbatim |
| 12 | `analysis_stamp` | `u64` (artifact.rs) | `uint` | Carried verbatim; package build may zero it |
| 13 | `gpu_builtins` | `Vec<(DefId, GpuBuiltinWire)>` (artifact.rs) | array of maps `{ def_id: uint, builtin: str-tagged enum }` | Rehydrates GPU builtin assignments keyed by `DefId` |

## 3. Package envelope — `FhirPackage` (7 families)

Defining type: `radix-hir-fhir/src/package.rs` `FhirPackage`. Postcard carries
`FhirPackageModule.unit` as opaque bytes; per OQ-3 default (confirm at FPF-3
dispatch), the MessagePack value carries embedded units as **nested named
values** — the whole package is one MessagePack document.

| # | Named key | Defining type | MessagePack type rule | Rehydration rule |
| --- | --- | --- | --- | --- |
| 1 | `package_schema_version` | `FhirPackage::package_schema_version: u32` | `uint`; first-read; must equal `1` | Fail-closed version prefix gate, before envelope decode |
| 2 | `identity` | `PackageIdentityWire` (package.rs) | map `{ name: str, version: str, edition: str }` | Verbatim |
| 3 | `entry_path` | `FhirPackage::entry_path: String` | `str` (package-root-relative) | Must name a module in row 5 with consistent `is_entry` |
| 4 | `entry_frontmatter` | `Option<String>` | `nil` or `str` (TOML text; format crate stays TOML-free) | Parsed by the caller on load |
| 5 | `modules` | `Vec<FhirPackageModule>` (package.rs) | array of `FhirPackageModule` maps (§3.1), sorted by `path`, unique | Each module's nested `unit` value rehydrates per §2; manifest `source_hash` and `unit_schema_version` cross-checked |
| 6 | `dependencies` | `Vec<PackageDependencyWire>` (package.rs) | array of maps `{ name, version, lock_identity, checksum: str? }`, sorted by `name`, unique | Satisfied from store/cache by the loader; unsatisfiable fails `MissingDependencyArtifact` |
| 7 | *(per-module link tables)* | `LocalLinkWire`, `LibraryImportWire` (package.rs) | see §3.1 | see §3.1 |

### 3.1 `FhirPackageModule`

| Field | Defining type | MessagePack type rule | Rehydration rule |
| --- | --- | --- | --- |
| `path` | `String` | `str`, package-root-relative | Canonical module key; sorted/unique gate |
| `module_segments` | `Vec<String>` | array of `str` | Derived path segments; verbatim |
| `is_entry` | `bool` | `bool` | Exactly one module may flag entry, and it must equal `entry_path` |
| `export_names` | `Vec<String>` | array of `str`, sorted | Source-free public surface for package codegen assembly |
| `local_links` | `Vec<LocalLinkWire>` | array of maps `{ binding: str, target: str }`, sorted by `binding` | **Explicit link table** — the loader never re-derives local imports from the filesystem; a target absent from the module table fails `DanglingModuleRef` |
| `library_imports` | `Vec<LibraryImportWire>` | array of maps `{ binding: str, package: str, module: [str] }` | Binding/package/module only — no `interface_path`, no absolute paths |
| `source_hash` | `String` | `str` (SHA-256 hex) | Must equal the unit's `source_identity.content_hash` or fail `MismatchedContentHash` |
| `unit_schema_version` | `u32` | `uint` | Manifest copy for fast fail with module attribution; must equal `SCHEMA_VERSION` (5) |
| `unit` | `Vec<u8>` (postcard) → **nested unit value** (MessagePack, OQ-3 default) | map per §2 | Decoded through the unit gates (§2 row 1) then reconstructed |

## 4. Reconstruction span (who rehydrates what)

- `radix-module/src/hir/package.rs` `load_package`: rebuilds `AnalyzedModule`
  arenas (type table, interner, resolver snapshot → runtime types) from the
  decoded envelope + nested units.
- `radix-program/src/fhir.rs` `build_package_fhir`: writer side; canonical
  ordering of modules/dependencies/links is established here.
- `crates/faber/src/package/fhir.rs` `load_package_fhir` +
  `loaded_package_to_analyzed`: rehydrates `AnalyzedProgram` from the link
  table in a fresh process — no parser/resolver/analyzer pass.

## 5. Type-table value facts (widths, tensor, buffer)

Defining type: `radix-types/src/types.rs` (`TypeTableSnapshot`, `Type`,
`IndexExpr`). These facts live **inside family §2 row 6**, as named fields of
`Type` values — never as opcode-level encodings:

| Fact | Defining type | MessagePack type rule | Rehydration rule |
| --- | --- | --- | --- |
| Numeric width | `Type::…(NumericWidth)` variants (e.g. `ModularWord(width)`) | enum map with a named width payload (`uint`-typed width fields, never float) | Re-interned into the rebuilt type arena |
| Tensor | `Type::Tensor(TypeId, IndexId)` (types.rs:1342) | enum map `{ tag: "Tensor", element: uint (TypeId), shape: uint (IndexId) }` — dtype is the element `TypeId`, shape an `IndexId` into `indices` | Element and shape indices resolve inside the rebuilt snapshot; structural identity preserved |
| Tensor family | `Type::Vector/Matrix/Sparsa(TypeId, IndexId)`; `TensorFamilyTypeSpec`/`TensorFamilyKind` | same shape: named element + shape index fields | Same as tensor |
| Buffer-as-type-facts | buffer-carrying `Type` variants (dtype/dimensionality as type-table facts) | named dtype/dimension fields; payload bytes, when carried, are `bin` with schema-stated dtype — never a generic float array | Exact byte sequence + stated dtype round-trip |
| Index arena | `TypeTableSnapshot::indices: Vec<IndexExpr>`; `unspecified_shape: Option<IndexId>`; `next_index_var: u32` | array of `IndexExpr` named maps; `unspecified_shape` as `uint?`; `next_index_var` as `uint` | Index arena rebuilt in order; fresh index variables continue from `next_index_var` |

## 6. Arena / id rehydration families

| Id | Defining type | Carried as | Rehydration rule |
| --- | --- | --- | --- |
| `TypeId(u32)` | `radix-types/src/types.rs:51` | `uint` index into the `types` snapshot array (§2 row 6) | The type-table snapshot rebuilds the arena in declaration order; indices are valid again without renumbering |
| `IndexId(u32)` | `radix-types/src/index.rs:16` | `uint` index into `indices` | Same, against the index arena |
| `DefId(u32)` | `radix-types/src/def_id.rs:21` | `uint`, inline in facts/HIR/resolver fields | Unit-local; carried verbatim (never package-global — package wire discipline, package.rs) |
| `Symbol(u32)` | `radix-lexer/src/token.rs:64` | `uint` index into the `interner` string table (§2 row 7) | The interner `Vec<String>` rebuilds symbol→string; every `Symbol` on the wire resolves through it |
| `HirId(u32)` | `radix-hir/src/nodes.rs:51` | `uint`, inline in HIR nodes (`hir` family) | HIR is a tree, not a node arena — ids rehydrate with the tree verbatim, no arena rebuild |
| `HirSourceAnchorId(u32)` | `radix-hir/src/nodes.rs:1469` | `uint`, in the `presentation` sidecar (`entry_start_anchor`, trivia anchors) | Rehydrates with the presentation sidecar, inseparable from `hir` |

## 7. Excluded from the wire (rebuilt, not carried)

Per frozen field set and cache discipline (CTM E.2, CXO F4, F14): resolver
scopes/shadow map/lookup caches, `gpu_builtins` runtime caches and order
mirrors, `radix_lanes`, `graphics_source`, spans on diagnostic-only CLI
fields, `binding_symbol` references. These are never invented as MessagePack
fields; the loader rebuilds them.

## 8. Losslessness oracle (binds FPF-2/3/4)

A MessagePack unit/package value is conformant only if encode→decode yields a
structurally equal `HirArtifact`/`FhirPackage` snapshot and the reconstructed
`AnalyzedModule`/`LoadedHirPackage` equals the postcard path's, including
canonical-Faber emit from the loaded HIR. Anything less is a lossy export,
not a FHIR schema.
