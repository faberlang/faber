# GOAL: FHIR package format — the HIR cargo spec

**Status**: planned — goal-checked 2026-08-23 (planner) against radix 333177658 / faber b4ae10c tips; P3-lowered below; awaiting Mind dispatch
**Created**: 2026-08-18
**Campaign:** `—` (standalone; fcmp-profile-1 becomes a dependency of this goal, not the reverse)
**Source:** operator addendum 2026-08-18 — the product is HIR package serialization; the envelope is a framing detail around this schema
**Repos:** `radix` (codec + wire: `radix-hir-fhir`; loader/reconstruction: `radix-module/src/hir/package.rs`; writer: `radix-program/src/fhir.rs`), `faber` (format docs + product adapter `crates/faber/src/package/fhir.rs`)
**Related:**
- [`../fcmp-profile-1/addendum.md`](../fcmp-profile-1/addendum.md) — the authority this goal executes
- [`../../docs/faber-messagepack-profile-v1.md`](../../docs/faber-messagepack-profile-v1.md) — envelope rules; stay draft until this schema binds a `value`

---

## Invariant

A FHIR package load rehydrates the analyzed program with **no semantic
confusion**: items, types, imports, facts, and presentation that HIR carries.
Numeric width and tensor dtype/shape are type-table facts in the `value`, never
inferred from MessagePack opcodes. If a format DTO cannot reconstruct today's
`HirArtifact` → `AnalyzedModule` path, it is not a FHIR schema — it is a lossy
export.

## Problem

Live code already saves/loads analyzed HIR in postcard (`HirArtifact`
encode/decode, `FhirPackage` `.fhirpkg`, driver reconstruction of
`AnalyzedModule`). The missing contract is how that snapshot is written in
MessagePack so decode yields a structurally equal artifact and the same
analyzed program. The prior plan froze an envelope around an undefined
`value`; the operator blocked that sequencing.

## Ground truth re-verified (2026-08-23, radix 333177658 / faber b4ae10c)

Every load-bearing claim above holds at tip; corrections and pinning:

- **Unit artifact schema is v5, not v4.** `radix-hir-fhir/src/artifact.rs`
  `SCHEMA_VERSION = 5` — ratcheted 2026-08-23 by EOT-3W (79c36d9fb):
  unreleased `HirExpressionKind::Transfer` (EOT-3M) and `FirstMatch`
  (AFM-2) shifted serde ordinals in the directly-serde'd
  `HirModule` payload. The v4→v5 ratchet landed **after this goal was
  minted** (2026-08-18). The field spec and MessagePack mapping are pinned
  to the v5 wire; no compatibility decoders exist (fail closed,
  `UnsupportedSchemaVersion`).
- **Package envelope schema is v1** (`PACKAGE_SCHEMA_VERSION = 1`,
  `package.rs`); separate version gate from the embedded unit; embedded
  units are postcard bytes (`FhirPackageModule.unit: Vec<u8>`) with a
  manifest copy of the unit schema version and a source-hash consistency
  check.
- **Fact families confirmed in `HirArtifact` (v5):** `schema_version`,
  `source_identity` (content hash + relative path), `hir`
  (`radix_hir::HirModule`, direct serde), `presentation`
  (`HirPresentation`: trivia owners/attachments, `block_ends`,
  `program_end`, `entry_start_anchor`), `cli_program` (`CliProgramWire`),
  `types` (`TypeTableSnapshot`: `types`/`indices`/`unspecified_shape`/
  `next_index_var` — widths and `Type::Tensor(TypeId, IndexId)` live here
  as type-table facts), `interner` (`Vec<String>` — `Symbol` rehydration),
  `resolver` (`ResolverSnapshot`), `libraries` (`LibraryRegistry`),
  `function_facts` (`FunctionFactTableWire`), `resolved_uses`
  (`Vec<ResolvedUseWire>`), `analysis_stamp`, `gpu_builtins`
  (`Vec<(DefId, GpuBuiltinWire)>`).
- **Arena/id rehydration families:** `TypeId` (type-table snapshot
  rebuild), `DefId` (carried inline in facts/HIR), `Symbol` (string
  interner `Vec<String>`), `HirId(u32)` (inline in the HIR tree — HIR is a
  tree, not a node arena), `HirSourceAnchorId` (presentation sidecar).
- **Package import links travel:** `LocalLinkWire` (explicit link table;
  loader never re-derives from filesystem; `DanglingModuleRef` on miss),
  `LibraryImportWire` (binding/package/module only, no absolute paths),
  `PackageDependencyWire`, `PackageIdentityWire`.
- **Reconstruction spans three crates, not one:**
  `radix-module/src/hir/package.rs` `load_package` rebuilds
  `AnalyzedModule` arenas; `radix-program/src/fhir.rs`
  `build_package_fhir` is the writer; faber's
  `crates/faber/src/package/fhir.rs` `load_package_fhir` +
  `loaded_package_to_analyzed` rehydrates `AnalyzedProgram` from the
  envelope link table (fresh process, no parser/resolver/analyzer pass).
- **No MessagePack/rmp code exists in radix yet** — the codec units below
  are greenfield; postcard 1.1.3 is the only codec dependency in
  `radix-hir-fhir`.

## Proposal

Specify the cargo first:

1. **Field spec** — every `HirArtifact` / `FhirPackage` fact family as named
   MessagePack, including `TypeId` / `HirId` / `Symbol` arena rehydration.
   Live postcard types are the source of truth until a replacement is proven
   equivalent.
2. **MessagePack encoding of that spec** — FCMP envelope rules applied to it.
3. **Reference codec + round-trip proof** — encode live artifact → decode →
   structurally equal snapshot → reconstructed program equivalent to the
   postcard path.
4. **Writer/reader switch** — then, and only then, reject postcard as legacy.

A DTO rewrite is allowed only as a complete projection of the current
analyzed-HIR snapshot (post-RTR module / `AnalyzedProgram` graph included):
node kinds, presentation, type widths, package import links all travel.

## Non-goals

- Envelope framing rules (owned by fcmp-profile-1, which stays draft and
  becomes this goal's dependency).
- FMIR / executable image changes.
- Round-tripping author source bytes (canonical-emit stability already proves
  HIR is the semantic core; the package round-trips analyzed HIR).
- Dual writers or silent fallback: postcard stays the live wire until the
  proven switch.

## Delivery (P3 lowering — 2026-08-23)

Theme: make a MessagePack FHIR `value` real, prove it against the postcard
path, then switch. Ordered unit graph; one logical change per unit. Lint
(stages 1–2), test (stages 3–4 / broad), and merge integration are
lane-owned gates, not child units. Postcard stays the live wire until
FPF-5.

| Field | FPF-1 field spec | FPF-2 unit codec | FPF-3 package codec | FPF-4 equivalence proof | FPF-5 switch |
| --- | --- | --- | --- | --- | --- |
| outcome | Inventory + named-MessagePack field spec for every v5 fact family | MessagePack encode/decode of `HirArtifact` per spec | MessagePack encode/decode of `FhirPackage` (nested unit values) | Loaded-via-MessagePack program ≡ loaded-via-postcard program | Default writer/reader flip; postcard rejected as legacy |
| write_scope | `faber/docs/factory/fhir-package-format/` (new `spec.md` + this goal) | `radix/crates/radix-hir-fhir/` (new msgpack module + `Cargo.toml` rmp dep + crate tests) | `radix/crates/radix-hir-fhir/` (package msgpack codec + tests) | `radix/crates/radix-module/src/hir/` (+ `package_test.rs` fixture comparing both codecs) | `radix/crates/radix-hir-fhir/`, `radix/crates/radix-module/src/hir/package.rs`, `radix/crates/radix-program/src/fhir.rs`, `radix/crates/faber/src/package/` |
| done_when | Every field family listed in Ground truth has a named key, MessagePack type rule, and rehydration rule (incl. `TypeId`/`DefId`/`Symbol`/`HirId`/anchor arenas, widths/tensor/buffer-as-type-facts, presentation, link tables); spec cites the defining type for each row | For every artifact exercised by `artifact_test.rs` fixtures: msgpack encode → decode → `assert_eq` on the snapshot, mirroring the postcard proof; named fields on the wire (not compact arrays); no width/dtype fact encoded via float opcodes | Round-trip on package fixtures incl. multi-module link tables, dependency table, entry checks; embedded units carried as nested named values with both version gates preserved; structural equality vs pre-encode `FhirPackage` | A test builds a real analyzed unit/package, encodes both ways, loads both through `load_package`, and proves reconstructed `AnalyzedModule`/`LoadedHirPackage` equality plus canonical-Faber emit equality from the loaded HIR | `.fhirpkg` write path emits MessagePack; postcard bytes rejected with a legacy error (no fallback decode); version/magic ratchet per OQ-1 |
| depends_on | — | FPF-1 | FPF-2 | FPF-3 | FPF-4 |
| sanity | Spec rows cross-checkable against `radix-hir-fhir/src/{artifact,package}.rs` (read-only) | `cargo test -p radix-hir-fhir --lib` | `cargo test -p radix-hir-fhir --lib` | `cargo test -p radix-module --lib hir` | `cargo test -p radix-hir-fhir --lib && cargo test -p radix-module --lib hir` |
| non_goals | Envelope framing rules (fcmp-profile-1 owns); no code | No writer switch; postcard untouched as live wire; no envelope | No envelope freeze; no CLI flags | No emit/locale features beyond equality proof | No dual writer, no silent fallback, no FMIR change |
| risk | low (docs) | medium — direct-serde `HirModule` tree is the large mapping surface; enum-ordinal drift risk is exactly why the spec pins v5 | low — thin over FPF-2 | medium — equivalence must catch semantic (not just structural) drift | medium-high — wire break; ratchet + fail-closed rejection |
| integrable | yes | yes | yes | yes | yes (single commit, both codecs touched atomically) |

Notes for Mind: FPF-2 is the long pole (whole-tree mapping); FPF-1 is the
critical-path artifact the operator addendum demanded and unblocks FPF-2/3
in parallel with fcmp-profile-1 draft work. All units stay inside the
`hir-fhir` feature; no `--stage`/`--e2e`/`--full` closeout on children.

### Lane-owned validation (named once)

- Lint lane: stages 1–2 after merge.
- Test lane: stages 3–4; faber `package` e2e touches after FPF-5.
- Merge gate: FPF-5 must land as one commit (codec flip across all four
  write scopes); every earlier unit is independently integrable.

## Validation

Round-trip proof per the addendum: loaded HIR emits canonical Faber as capable
as in-session HIR; structural equality on the snapshot; program equivalence to
the postcard path.

## Release posture

Ships with a Faber cut; not standalone.

## Open questions (for Mind ruling)

1. **FPF-5 rejection mechanics (blocking for FPF-5 only).** Default: ratchet
   `PACKAGE_SCHEMA_VERSION` → 2 with a MessagePack-marked prefix so v1
   postcard bytes fail closed as `UnsupportedPackageSchemaVersion` (renamed
   legacy in the error text). Alternative: new magic bytes. Needs ruling
   before FPF-5 dispatch.
2. **Envelope binding for fcmp-profile-1.** Once FPF-2 lands, the FCMP draft
   can bind `value = fhir unit schema v5`. Mind decides when the profile
   freeze is scheduled relative to FPF-3/4 (addendum says a framing profile
   may stay draft until the first consumer schema exists).
3. **Nested-unit vs opaque-bytes in the MessagePack envelope.** Default
   (encoded in FPF-3 done_when): embedded units are nested named values, not
   opaque nested bytes, so the whole package is one MessagePack document.
   Confirm or overrule at FPF-3 dispatch.
