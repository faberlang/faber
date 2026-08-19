# Delivery: fhir-package-format FS-3 — reference MessagePack codec + round-trip proof vs postcard

**Assignment**: Vivi task `c52b9139` (Mind → planner, 2026-08-19) — goal-check then FS-3 delivery lowering
**Planner**: planner (handle `c52b9139`); no packet; evidence from main checkouts only
**Goal**: [`goal.md`](goal.md) — **Status** `planned`; minted `f52ad1e`
**Inputs landed**: FS-1 field spec `3dfc219` (19 families); FS-2 mapping `93f5228` (MessagePack encoding, draft `value` bound)
**Protocol**: [`../../faber-messagepack-profile-v1.md`](../../faber-messagepack-profile-v1.md) — draft FCMP 1.0 (frame §1, root §2, primitives §3, shapes §4, admission §5/§5.1, limits §6, registry §10, conformance §11)
**Baselines at lowering**: faber `4e4b62b` (main, clean); radix main (clean; ahead 174)
**Status**: planned — FS-3 lowered here into 14 Hand units; **FS-4 (writer/reader switch) stays unminted**
**This artifact is planning only.** No product code, no codec, no writer switch, no freeze.

---

## 1. Goal-check summary

| Field | Value |
| --- | --- |
| goal_path | `faber/docs/factory/fhir-package-format/goal.md` |
| evaluator mode | cold self-pass against live radix/faber code, landed spec commits, protocol text |
| intended consumer | Mind — FS-3 Hand dispatch (unit graph §5) |
| verdict | **READY** for FS-3 lowering (goal-check on goal + FS-1/FS-2 inputs) |

**Reasoning.** FS-1 and FS-2 are complete, committed projections of the live
postcard wire, and every code path they name verifies against live radix/faber
source (§4 baseline). The proof oracles FS-3 needs already exist as postcard
harnesses that a MessagePack path can mirror byte-for-byte: structural
equality (`PartialEq` on `HirArtifact` and `DecodedFhirPackage`), the 5×5
codegen round-trip matrix, locale/canonical render transparency, and
fail-closed `validate_referential_integrity`. No FCMP/MessagePack code exists
yet anywhere in radix, so FS-3 starts from a clean, uncontested surface.

**Key points.**

- FS-2 deliberately left three things to FS-3: the codec, the round-trip
  proof, and finite per-kind limits publication (FS-2 §21/§22). All three are
  in this delivery.
- FS-2 §20 (operator A/B losslessness ruling on named postcard gaps) stays
  open and does **not** gate FS-3: §20.7 fixes FS-3's proof target as
  postcard-snapshot equivalence under either ruling.
- Kinds `fhir.unit` / `fhir.package` stay **reserved** until fcmp-profile-1
  FC-R1 registers them; FS-3's decoder is the reference/conformance path, not
  a product admission (that is FS-4).

**Blocking gaps:** none.

**Recommended next step.** Mind files Hands in §5 order: F3-A → F3-B →
{F3-C…F3-H} parallel → F3-I → F3-J → F3-K → {F3-L, F3-M} parallel → F3-N.

---

## 2. Interpreted unit

Lower FS-3 of the goal's lowering sketch — *"Reference codec + round-trip vs
postcard snapshot"* — into Hand-sized units: a canonical MessagePack codec for
the 19 FS-1 families under the FS-2 mapping and the draft FCMP envelope,
proven equivalent to the live postcard path, plus the finite per-kind limits
publication and golden document vectors that unblock fcmp-profile-1's
FC-R1/FC-R2/FC-R3.

## 3. Normalized spec

**End state (FS-3 whole).** `radix-hir-fhir` hosts a second codec beside
postcard: framed FCMP documents (`FABERMP\0`, root `{kind, schema, value}`)
whose `value` is the FS-2 record for `HirArtifact` (unit) and `FhirPackage`
(package). Encode → decode yields a structurally equal snapshot
(`PartialEq`), reconstructed programs match the postcard path byte-for-byte
(codegen matrix + locale render transparency + package `DecodedFhirPackage`
equality), golden document vectors with SHA-256 manifest + provenance exist
under `faber/docs/fcmp/vectors/fhir/`, and finite per-kind limits are
published in this goal dir and enforced by the decoder. Postcard remains the
only live wire (FS-4 unminted).

**Split boundary.**

```text
F3-A frame+primitives+error classes
  -> F3-B shared schema vocab (§2)
       -> {F3-C hir | F3-D types+interner | F3-E presentation | F3-F resolver+libraries | F3-G facts | F3-H cli}   [parallel, disjoint modules]
            -> F3-I unit envelope + fail-closed admission
                 -> F3-J package codec (nested unit records)
                      -> F3-K per-kind limits publication (doc + constants + enforcement)
                           -> {F3-L unit equivalence | F3-M package equivalence}   [parallel]
                                -> F3-N golden document vectors + conformance manifest
```

**Locked decisions (Hands do not reopen).**

1. **Crate home: `radix-hir-fhir`.** The codec lands as a `msgpack` module
   tree inside the existing format crate — the crate already owns the
   wire/schema contract for exactly these types, both codecs must stay
   schema-locked, and `validate_referential_integrity` is `pub(crate)` and
   shared (decode.rs:73 precedent). fcmp-profile-1's delivery point 6 defers
   crate home to this spec: **FC-R2's conformance tests ride
   `cargo test -p radix-hir-fhir`**. No new crate, no generic `radix-fcmp`
   (addendum: never dispatch a generic codec; protocol Abstract: no generic
   framework before a second consumer).
2. **Hand-rolled codec, no new dependencies.** Canonical admission (one
   encoding per schema value, strict reject, sorted keys) cannot come from
   `rmp-serde`; the encoder/decoder is written against the msgpack primitive
   forms directly. `postcard`/`serde` stay for the postcard path. Prefer
   stdlib; no new external deps without a Mind-routed need.
3. **FS-2 is the encoding law.** Field names, presence kinds (REQUIRED /
   OPTIONAL named defaults / REQUIRED-nullable), tag spelling (§1.6), key
   order (lexicographic), collection ordering (§1.5), and the §19
   width/dtype/shape prohibitions are transcribed, not reinvented. A
   discrepancy between FS-2 and a discoverable postcard fact is a stop +
   Mind route (spec defect), not a silent codec choice.
4. **Error classes are exactly protocol §5.1.** No new classes; a reject with
   no row routes a protocol amend through fcmp-profile-1 custody.
5. **Frame is limit-table-driven.** Prefix limits (kind-string 64, root map
   3, schema array 2, no-enabled-kind 256) are enforced from F3-A. Per-kind
   finite limits are published once (F3-K) and then wired as the two FHIR
   kinds' enabled table; before F3-K only test-local finite caps exist.
6. **`legacy` diagnosis**: postcard FHIR bytes seen at the frame are
   diagnosed `legacy` (§1.8, §13), never reinterpreted.
7. **Repo split.** Code + tests land in `radix`; published artifacts (limits
   doc, vectors, manifest) land in `faber` docs. No `faber` product-source
   writes in FS-3.

## 4. Repo-aware baseline (verified 2026-08-19)

Authority order: live files → git receipts → goal/spec prose.

| Claim | Evidence |
| --- | --- |
| FS-1 / FS-2 landed | faber `3dfc219`, `93f5228` (goal dir has goal.md + fs-1 + fs-2 only; no prior delivery.md) |
| Unit schema constants | `radix-hir-fhir/src/artifact.rs:366` `SCHEMA_VERSION = 3`; `package.rs:32` `PACKAGE_SCHEMA_VERSION = 1` |
| Postcard codec fns | `decode.rs:17/41` `encode`/`decode`; `package.rs:253/278` `encode_package`/`decode_package` |
| Snapshot constructor | `radix-module/src/hir/serialize.rs:24` `from_analyzed` |
| Reconstruction | `radix-module/src/hir/artifact.rs:274` `reconstruct_unit` (`pub(crate)`); `artifact.rs:260` `load_fhir`; `radix-module/src/hir/package.rs:193` `load_package` |
| Faber adapter | `radix/crates/faber/src/package/fhir.rs:74` `loaded_package_to_analyzed` (`pub(crate)`) |
| Integrity validation | `radix-hir-fhir/src/validate.rs:860` `validate_referential_integrity` (`pub(crate)`, reused by decode) |
| Structural equality oracles | `PartialEq` on wire DTOs (`artifact.rs:52+`) and `DecodedFhirPackage`/`DecodedFhirPackageModule` (`package.rs:126/138`); `LoadedHirPackage` holds `AnalyzedModule` (no `PartialEq`) → package equivalence asserts at wire level + reconstruction oracles |
| Codegen matrix oracle | `radix-module/tests/hir_artifact_round_trip.rs` — 5 fixtures × 5 targets byte-for-byte + 2 F23 exclusions with stable issue codes; fixtures at `tests/fixtures/hir_artifact/*.fab` |
| Render transparency oracle | `radix-module/tests/fhir_locale_roundtrip.rs` — loaded-via-FHIR unit renders byte-identically to direct render across locales |
| Adversarial precedent | `radix-module/tests/hir_artifact_adversarial.rs` |
| Feature gate | `radix` facade feature `hir-fhir` gates the format crate path (`Cargo.toml` members/features) |
| No FCMP/msgpack code exists | workspace grep `FABERMP` → docs only; no `rmp`/msgpack dep in any radix `Cargo.toml` |
| Sibling-repo test include precedent | `radix-hir-lean/tests/project_layout.rs:18` (`include_str!("../../../lean/lakefile.toml")`); corpus paths via `exempla` crate env-overridable resolvers |
| fcmp-profile-1 gates | its goal/delivery: FC-R1 waits on FS-3 **limits publication**; FC-R2 waits on the FS-3 **codec** (crate home named here); FC-R3 waits on FC-R2 + **FHIR fixtures** (F3-N output); GATE-FREEZE-2 after FC-R1 |
| Kinds reserved | protocol §10 table: `fhir.unit`/`fhir.package` "reserved; schema and limits not published" |
| Goal Status line stale | says "not lowered" — true for FS-3/FS-4, but predates FS-1/FS-2 landing; Mind updates at closeout |

## 5. Hand unit graph

Units carry the campaign fields. `sanity` is the unit's own narrow check;
lane gates are named once in §7. All units: `read_scope` unrestricted within
radix/faber checkouts; `integrable: yes` (each lands compiling with its own
focused tests green; family modules are wired as `pub` through the `msgpack`
module tree until F3-I finalizes the export surface).

### F3-A — MessagePack primitives, FCMP frame, error classes

| Field | Value |
| --- | --- |
| `id` | `F3-A` |
| `outcome` | New `radix-hir-fhir/src/msgpack/` module tree: canonical primitive writer/reader (protocol §3 shortest forms, float64 + canonical NaN, binary headers, no extensions), the 20-byte frame reader/writer (§1: magic, profile 1.0, payload length, truncation/trailing reject), root-document admission (§2: `kind`→`schema`→`value` canonical key order), envelope-prefix limits enforced from headers before allocation (§6: 64/3/2/256), a limit-table type for enabled kinds (per-kind entries arrive at F3-K), `legacy` diagnosis for postcard-prefixed bytes, and a structured error type whose classes are exactly §5.1. |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` (new) + `crates/radix-hir-fhir/src/lib.rs` (module decl) |
| `done_when` | Focused tests green (`cargo test -p radix-hir-fhir msgpack`): frame round-trips an opaque canonical payload; every §5.1 class reachable at primitive/frame level has a rejecting test (truncated, trailing_bytes, bad_magic, profile_unsupported, payload_limit, kind_string_limit, root_map, schema_array, kind_unregistered, schema_unsupported, noncanonical, utf8, extension, overflow, legacy); the error enum admits no class outside §5.1 |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` (cwd: radix) |
| `depends_on` | none |
| `non_goals` | No schema-family encoding (F3-B…); no FHIR kind entries in the limit table; no `fcmp.test` admission (FC-R2's test-only concern) |
| `risk` | low — self-contained byte layer with the protocol as oracle |
| `integrable` | yes |

### F3-B — shared schema vocab (FS-2 §2)

| Field | Value |
| --- | --- |
| `id` | `F3-B` |
| `outcome` | `msgpack/shared` (naming free to the Hand): arena/identity integers as unsigned with overlong→`noncanonical` and >u32→`type` (§2.1); `Span` record with `start <= end` (§2.2); closed unit enums `NumericWidth`, `Primitive`, `InstansPrecision`, `SemanticMutability`, `SemanticParamMode`, `Visibility`, `CallablePosture`, `HirParamMode`, `GenericParamKind`, `NominalKind`, `HirBinOp`, `HirUnOp`, `HirRefKind`, `HirRangeKind`, `HirIteraMode`, `HirScribeKind`, `HirIncDecOp`, `HirBreakableKind`; `TypeParamConstraint`/`HirTypeParamConstraint` (`any`/`one_of`). Tag tables transcribed from FS-2 §2 verbatim. |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` |
| `done_when` | Focused tests green: every §2 enum round-trips each variant; unknown tag → `type`; a test asserts the full tag set per closed enum matches the FS-2 §2 tables (guards drift) |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-A` |
| `non_goals` | No record families (F3-C…); no limits |
| `risk` | low |
| `integrable` | yes |

### F3-C — `hir` family codec (FS-2 §5)

| Field | Value |
| --- | --- |
| `id` | `F3-C` |
| `outcome` | Encode/decode of the `HirModule` tree per FS-2 §5: `HirModule`, `HirItem`/`HirItemKind` (8 variants), all declaration records (§5.4 incl. test metadata/modifiers, params, struct/enum/interface/schema/const/import), annotations + annotation values + `Token`/`TokenKind` (payload-bearing §5.5 + unit tags per Appendix A), blocks/statements (§5.6), the full `HirExpressionKind` closed set as named records (§5.7, tuple payloads → named records), call args/cape/array elements/object keys/patterns (§5.7), literals + `JsonValue` (§5.8). Tree-order sequences preserve order; identity arenas are never sorted. |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` |
| `done_when` | Focused tests green: representative value for every `HirItemKind`, `HirStatementKind`, `HirExpressionKind`, `HirPattern`, `HirLiteral`, `JsonValue`, and payload-bearing `TokenKind` variant round-trips `PartialEq`-equal; strict rejects proven for unknown tag, missing `value` on payload variant, present `value` on unit variant, out-of-order keys, duplicate keys, present-at-default OPTIONAL |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-B` |
| `non_goals` | Type-table/interner encoding (F3-D); envelope assembly (F3-I); no reduced token DTO |
| `risk` | medium — largest single surface (~50 expression variants, mutually recursive; cannot split without non-integrable halves); drift risk against FS-2 tables is the flagged failure mode |
| `integrable` | yes |

### F3-D — `types` + `interner` families (FS-2 §7, §9)

| Field | Value |
| --- | --- |
| `id` | `F3-D` |
| `outcome` | Encode/decode of `TypeTableSnapshot` (`types`, `indices`, `unspecified_shape`, `next_index_var`), the closed `Type` set (§7.1) incl. `sized_numeric`/`modular_word` width facts and tensor-family `{elem, shape}`, `FuncSig`/`ParamType`, `IndexExpr` (§7.2), and the `interner` string array in symbol-id order (§9: never sorted, never re-normalized). Required-primitive presence is an admission invariant named here and enforced at F3-I. |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` |
| `done_when` | Focused tests green: every `Type` and `IndexExpr` variant round-trips `PartialEq`-equal; `numerus<i32>`-as-int-opcode and `fractus<f32>`-as-float-opcode rejects (`type`) proven per §19.1; tensor-as-float-array and rank-from-header rejects per §19.2; interner order preserved byte-exactly (raw payloads incl. octeti hex) |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-B` |
| `non_goals` | Rebuilt caches (hash-cons, primitives map) — not wire facts; hir tree (F3-C) |
| `risk` | low |
| `integrable` | yes |

### F3-E — `presentation` family (FS-2 §10)

| Field | Value |
| --- | --- |
| `id` | `F3-E` |
| `outcome` | Encode/decode of `HirPresentation`: owners/owner kinds (6 tags, no inventions), `HirOwnerRef`, attachments, `HirTrivia` (`comment_line` with verbatim `Symbol` text, `newline`), block/program ends, `entry_start_anchor`. Integrity invariants (unique anchors, one attachment per owner, registered-owner references) enforced at F3-I admission; this unit proves field-level round-trip. |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` |
| `done_when` | Focused tests green: a presentation with all six owner kinds, both trivia variants, and structural ends round-trips `PartialEq`-equal; comment `Symbol`s keep interner identity (§19.3); unknown owner kind → `type` |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-B` |
| `non_goals` | Cross-family integrity (F3-I); new owner kinds |
| `risk` | low |
| `integrable` | yes |

### F3-F — `resolver` + `libraries` families (FS-2 §8, §12)

| Field | Value |
| --- | --- |
| `id` | `F3-F` |
| `outcome` | Encode/decode of `ResolverSnapshot` (namespace exports, file interfaces with `FileExportSnapshot` 3-arm closed set — no `enum` arm, imported/ambiguous nominal types, used namespaces) and `LibraryRegistry` (bindings/items/exports/reexports as deterministic logical maps/sets per §1.5: string-key maps sorted; non-string keys as sorted `[key, value]` arrays; `(DefId, string)` reexport keys as two-element arrays). |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` |
| `done_when` | Focused tests green: a snapshot exercising every `FileExportSnapshot` arm, `NominalKind`, and `LibraryItemKind` round-trips `PartialEq`-equal; two insertion orders of the same `FxHashMap`-backed registry encode to identical bytes (compiler hash order never wire order); unsorted map → `noncanonical`; duplicate logical key → `duplicate_key` |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-B` |
| `non_goals` | Resolver runtime state (FS-1 §14.3 gaps — not wire facts); `enum` export arm |
| `risk` | low |
| `integrable` | yes |

### F3-G — facts bundle families (FS-2 §13–§16)

| Field | Value |
| --- | --- |
| `id` | `F3-G` |
| `outcome` | Encode/decode of `function_facts` (`FunctionFactTableWire` entries as sorted logical map + nullable `entry`; `FunctionFactWire` with call_edges/captures/param_modes orderings per §13), `resolved_uses` (§14 sort by canonical key bytes), `gpu_builtins` (§15 logical map; all 16 closed tags), and `analysis_stamp` (§16 OPTIONAL default `0`). |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` |
| `done_when` | Focused tests green: a fact table with entry facts, captures in all three modes, and every `GpuBuiltinWire` tag round-trips `PartialEq`-equal; postcard vec-of-pairs → sorted logical map encoding is deterministic across two source orders; stamp `0` omitted (OPTIONAL) vs non-zero present both proven |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-B` |
| `non_goals` | Facts semantics; resolver-local gpu_builtin maps (not wire facts) |
| `risk` | low |
| `integrable` | yes |

### F3-H — `cli_program` family (FS-2 §11)

| Field | Value |
| --- | --- |
| `id` | `F3-H` |
| `outcome` | Encode/decode of `CliProgramWire` and its record tree: options/operands/commands, `CliModeWire`, `CliExitWire` (4 arms), `CliTypeWire` (8 tags), `CliDefaultWire` (6 arms incl. float64 float and unit `nil` tag — never MessagePack `nil` for the `Nil` default). Diagnostic spans / `binding_symbol` stay excluded (F18). |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` |
| `done_when` | Focused tests green: a program exercising every mode/exit/type/default arm round-trips `PartialEq`-equal; `CliDefaultWire::Nil` as unit tag vs MessagePack `nil` misuse (`type`) both proven |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-B` |
| `non_goals` | Live `CliProgram` spans/binding_symbol (excluded by design) |
| `risk` | low |
| `integrable` | yes |

### F3-I — unit envelope + fail-closed admission (FS-2 §3–§4)

| Field | Value |
| --- | --- |
| `id` | `F3-I` |
| `outcome` | Wire families 1–13 into the `fhir.unit` document: framed encode/decode of the `HirArtifact` record (lexicographic key emission per §3) + `source_identity` (§4); public `encode_msgpack(&HirArtifact)` / table-parameterized decode; admission = strict key order/duplicates/unknowns during decode, `schema_version == 3` else `invariant`, required-primitives presence (`required_primitive`), and reuse of `validate_referential_integrity` after structural decode (same fail-closed contract as postcard `decode`, decode.rs:41–73 precedent). Finalizes the `msgpack` export surface in `lib.rs`. |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` + `crates/radix-hir-fhir/src/lib.rs` |
| `done_when` | Focused tests green: a synthetic artifact from existing `test_support` round-trips through full frame → `PartialEq`-equal `HirArtifact`; re-encode of the decode is byte-identical (canonical form); `schema_version` tamper → `invariant`; missing required primitive → `invariant`; a referential-integrity violation constructed at the msgpack layer is rejected with the same behavior as the postcard adversarial path |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-C`, `F3-D`, `F3-E`, `F3-F`, `F3-G`, `F3-H` |
| `non_goals` | Product/driver integration (F3-L); package envelope (F3-J); kind registration |
| `risk` | medium — admission completeness is the FS-3 honesty bar; mirrors live decode semantics exactly |
| `integrable` | yes |

### F3-J — package codec, families 14–19 (FS-2 §17)

| Field | Value |
| --- | --- |
| `id` | `F3-J` |
| `outcome` | Framed `fhir.package` encode/decode: envelope record (§17), `FhirPackageModule` rows with **nested `HirArtifact` records** at `unit` (binary there → `type`), local links / library imports (§17.2), dependencies, identity; cross-invariants (`package_schema_version == 1`, `source_hash == unit.source_identity.content_hash`, `unit_schema_version == unit.schema_version == 3`, duplicate module path / dependency name, `dangling_module_ref`); module/dependency sort order at emission. |
| `write_scope` | radix `crates/radix-hir-fhir/src/msgpack/**` + `crates/radix-hir-fhir/src/lib.rs` (exports) |
| `done_when` | Focused tests green: a multi-module synthetic package round-trips to `PartialEq`-equal `DecodedFhirPackage`; each listed cross-invariant has a rejecting test with its named payload; nested-unit-as-binary and postcard-bytes-at-frame (`legacy`) both rejected |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` |
| `depends_on` | `F3-I` |
| `non_goals` | Adapter dependency checks (product adapter owns `library_imports ⊆ dependencies`); writer switch |
| `risk` | medium — cross-invariant set must match `decode_package` behavior exactly |
| `integrable` | yes |

### F3-K — per-kind limits publication + enforcement

| Field | Value |
| --- | --- |
| `id` | `F3-K` |
| `outcome` | Publish finite per-kind limits for `fhir.unit` and `fhir.package` covering every protocol §6 category (complete frame bytes; nesting depth; array elements; map entries; string bytes; binary bytes; decoded-node/allocation budget; kind-specific multiplicities incl. package modules and embedded units) as `faber/docs/factory/fhir-package-format/fs-3-limits.md` (new), grounded in a recorded measurement sweep over the reachable corpus (existing round-trip fixtures + exempla/product-corpus via postcard-equivalent snapshots through the new encoder); wire the two FHIR entries into the F3-A limit table so the codec's FHIR entry points decode under them. |
| `write_scope` | faber `docs/factory/fhir-package-format/fs-3-limits.md` (new); radix `crates/radix-hir-fhir/src/msgpack/**` (limit constants + wiring + measurement test) |
| `done_when` | `fs-3-limits.md` lists a finite number for every §6 bullet × both kinds with the measurement provenance; the constants in code equal the doc; a decode exceeding each enforced limit fails `limit` with the violated limit named; `cargo test -p radix-hir-fhir msgpack` green |
| `sanity` | `cargo test -p radix-hir-fhir msgpack` + `ls docs/factory/fhir-package-format/fs-3-limits.md` (cwd: faber) |
| `depends_on` | `F3-J` (measurement needs unit + package encode) |
| `non_goals` | No protocol §10 registration (fcmp-profile-1 FC-R1 transcribes these numbers — **this unit is FC-R1's gate**); no freeze language; numbers are measured, not aspirational |
| `risk` | medium — limits too tight break real corpus, too loose evade §6 intent; measurement sweep must record maxima honestly |
| `integrable` | yes |

### F3-L — unit equivalence vs postcard path

| Field | Value |
| --- | --- |
| `id` | `F3-L` |
| `outcome` | Driver-level proof mirroring the postcard harnesses through the msgpack path: a thin `load_fhir_msgpack`-style seam in `radix-module::hir::artifact` (msgpack decode + existing `reconstruct_unit`); new `hir_artifact_round_trip_msgpack` test reproducing the 5 fixtures × 5 targets byte-for-byte matrix (same F23 exclusion contract, stable issue codes, library-import fixture first) and a `fhir_locale_roundtrip_msgpack` test reproducing locale/render transparency (loaded render == direct render); structural equality of msgpack-decoded vs postcard-decoded `HirArtifact` on the same fixtures. |
| `write_scope` | radix `crates/radix-module/src/hir/artifact.rs` (seam fn only) + `crates/radix-module/tests/*msgpack*.rs` (new; may include existing `tests/fixtures/hir_artifact/*.fab` read-only) |
| `done_when` | All 25 matrix cells green through the msgpack path (23 byte-for-byte + 2 exclusion-reason round-trips); locale renders byte-identical; `HirArtifact` equality holds per fixture; runs under the F3-K limit table |
| `sanity` | `cargo test -p radix-module --features hir-fhir msgpack` (cwd: radix) |
| `depends_on` | `F3-I`, `F3-K` |
| `non_goals` | Package-level equivalence (F3-M); no CLI/writer surface changes; no codegen changes |
| `risk` | medium — any inequality here is an FS-2/codec defect, not a test-tuning target; failures route back to the owning family unit |
| `integrable` | yes |

### F3-M — package equivalence vs postcard path

| Field | Value |
| --- | --- |
| `id` | `F3-M` |
| `outcome` | Package-level proof: build a real multi-module `FhirPackage` (existing driver package-build path or inline multi-module sources), encode both ways, assert `DecodedFhirPackage` `PartialEq` equality (msgpack vs postcard decode), and assert reconstructed-module equivalence via the same codegen oracle on each module; `LoadedHirPackage` equivalence at wire fields (its `AnalyzedModule` has no `PartialEq` — reconstruction equivalence stands in); re-encode byte-identity of the decoded package. |
| `write_scope` | radix `crates/radix-module/src/hir/package.rs` (seam fn only, if needed) + `crates/radix-module/tests/*msgpack*package*.rs` (new) |
| `done_when` | `DecodedFhirPackage` equality holds on ≥1 real package; every module's codegen output matches the postcard-loaded path byte-for-byte; runs under the F3-K limit table |
| `sanity` | `cargo test -p radix-module --features hir-fhir msgpack` |
| `depends_on` | `F3-J`, `F3-K` (parallel with F3-L — disjoint files) |
| `non_goals` | `faber` binary/integration surface (FS-4); adapter-internal checks beyond parity with the postcard path |
| `risk` | low-medium — oracle already exists postcard-side |
| `integrable` | yes |

### F3-N — golden document vectors + conformance manifest

| Field | Value |
| --- | --- |
| `id` | `F3-N` |
| `outcome` | FHIR document vectors per protocol §11.2 under `faber/docs/fcmp/vectors/fhir/` (new): smallest valid unit + package documents; representative documents exercising every field and closed enum variant across the 19 families; canonical bytes + SHA-256 per positive in a manifest; language-independent decoded semantic expectations; negatives for every structural/referential class reachable in the FHIR schema; provenance record naming schema (`fs-2-mapping.md`, draft `[1,0]`) and encoder version. A conformance test in `radix-hir-fhir` loads the shared bytes (sibling-include precedent: `radix-hir-lean/tests/project_layout.rs:18`), asserts positives re-encode byte-identically with manifest SHA match, and negatives reject with the expected §5.1 class. |
| `write_scope` | faber `docs/fcmp/vectors/fhir/**` (new, incl. manifest); radix `crates/radix-hir-fhir/tests/**` or in-crate test module (conformance test only) |
| `done_when` | Every §11.2 bullet has a fixture for both kinds; conformance test green (byte-identical re-encode, SHA match, per-class rejects); manifest carries provenance; **this unit is FC-R3's fixture gate** (with FC-R2 riding the codec) |
| `sanity` | the conformance test command this unit adds (record exact command in receipt); `ls docs/fcmp/vectors/fhir` non-empty (cwd: faber) |
| `depends_on` | `F3-L`, `F3-M` (fixtures generated only from the proven-equivalent codec) |
| `non_goals` | Generic §11.1 vectors (fcmp-profile-1 FC-R2 owns those); TS codec (FC-R3); no `fcmp.test` fixtures |
| `risk` | medium — "every field and variant" coverage is the §11.2 bar; generation must be reproducible (fresh-process identity, §11.3 item 4) |
| `integrable` | yes |

## 6. Mind pointer table

| id | write_scope (short) | done_when | dispatchable after |
| --- | --- | --- | --- |
| `F3-A` | radix-hir-fhir `msgpack/**` (frame+primitives+errors) | §5.1-class rejects + frame round-trip green | now |
| `F3-B` | radix-hir-fhir `msgpack/**` (§2 vocab) | §2 enum/tag tables round-trip green | F3-A |
| `F3-C` | radix-hir-fhir `msgpack/**` (hir tree) | every node-kind variant round-trips + strict rejects | F3-B |
| `F3-D` | radix-hir-fhir `msgpack/**` (types+interner) | §19 width/dtype/shape rejects proven | F3-B |
| `F3-E` | radix-hir-fhir `msgpack/**` (presentation) | all owner kinds + trivia round-trip | F3-B |
| `F3-F` | radix-hir-fhir `msgpack/**` (resolver+libraries) | deterministic maps; hash-order independence | F3-B |
| `F3-G` | radix-hir-fhir `msgpack/**` (facts bundle) | facts/resolved_uses/gpu/stamp round-trip | F3-B |
| `F3-H` | radix-hir-fhir `msgpack/**` (cli_program) | every CLI arm round-trips | F3-B |
| `F3-I` | radix-hir-fhir (envelope+admission+exports) | synthetic unit frame round-trip + invariants | F3-C…F3-H |
| `F3-J` | radix-hir-fhir (package codec) | nested-unit package round-trip + cross-invariants | F3-I |
| `F3-K` | faber `fs-3-limits.md` + radix constants | finite §6 limits × 2 kinds, enforced | F3-J |
| `F3-L` | radix-module seam + msgpack matrix tests | 25-cell codegen matrix + render parity green | F3-I, F3-K |
| `F3-M` | radix-module package seam + tests | `DecodedFhirPackage` equality + module parity | F3-J, F3-K |
| `F3-N` | faber `docs/fcmp/vectors/fhir/**` + conformance test | §11.2 coverage + SHA manifest green | F3-L, F3-M |

**fcmp-profile-1 dependency notes (task-mandated).** FC-R1 gates on
**F3-K** (limits publication; transcribes `fs-3-limits.md` into protocol
§10 — registration without limits MUST fail). FC-R2 gates on the **FS-3
codec** (F3-J landed; crate home `radix-hir-fhir`, locked decision 1) and is
write-disjoint from FC-R1. FC-R3 gates on FC-R2 **plus FHIR fixtures**, i.e.
**F3-N**. GATE-FREEZE-2 follows FC-R1 minimum. None of those units is minted
here; this delivery only names their gates.

**FS-4 stays unminted.** The writer/reader switch (postcard rejected as
`legacy` at the product surface, `.fhirpkg` produced in MessagePack) is a
separate future lowering; nothing in FS-3 changes any live writer, reader,
CLI surface, or `faber` product source.

## 7. Checkpoints and lane-owned validation

| Gate | When | Owner | Content |
| --- | --- | --- | --- |
| **SG-1** | after F3-I | Mind | unit document round-trips + admission parity with postcard decode |
| **SG-2** | after F3-K | Mind | limits finite, measured, enforced; FC-R1 unblocked |
| **SG-3** | after F3-L + F3-M | Mind | equivalence proofs green (matrix, render, package) |
| **SG-4** | after F3-N | Mind | vectors + manifest complete; FC-R3 unblocked; goal closeout (Status line + ledger receipts; FS-3 done, FS-4 explicitly remaining) |

**Lane-owned (named once):** lint/test/merge own workspace compile, stages
1–4, and `./scripta/check-factory-goal-status` after the goal Status-line
update at closeout. Hands run only their unit `sanity`. The FCMP vectors dir
and `fs-3-limits.md` are faber-main docs; merges are path-limited docs
commits.

## 8. Validation

Hand sanity = each unit's `sanity` command only. Existence rule: every
command either exists today (`cargo test -p <crate> [filter]`, `ls`, `rg`) or
is created by that same unit (conformance test entry at F3-N, measurement
test at F3-K). Red in F3-L/F3-M is a real codec/spec defect: route to the
owning family unit; never tune the oracle.

## 9. Open questions (defaults recorded)

1. **FS-2 §20 operator A/B ruling** — open, operator-owned, does not gate
   FS-3 (§20.7 fixes the proof target). Default: FS-3 proves against the
   postcard snapshot; the ruling re-opens only schema text, not the codec
   proof.
2. **Module naming** — default `msgpack` (it is the FHIR MessagePack codec
   of FS-2; "FCMP" names the profile). Mind may rename before F3-A dispatch;
   after F3-A it is stable.
3. **Goal Status line** — stale ("not lowered", pre-FS-1/FS-2). Mind updates
   at SG-4 closeout (or earlier); not a Hand.
4. **Vector include path** — default sibling-relative include from
   `radix-hir-fhir` tests to `faber/docs/fcmp/vectors/fhir/` (lean
   precedent). If packets make the sibling unreliable, Mind routes an
   exempla-style env-overridable resolver need; do not copy bytes into two
   repos.

## 10. Scope closure

Nothing admitted by the goal is narrowed: FS-1 (landed `3dfc219`) and FS-2
(landed `93f5228`) carry receipts; FS-3 is lowered whole here (codec +
proof + limits + vectors = F3-A…F3-N); FS-4 is explicitly **unminted, not
dropped** — the goal's completion contract still contains it, and its
lowering is Mind's next planner dispatch after FS-3 lands. fcmp-profile-1's
FC-R1/FC-R2/FC-R3/GATE-FREEZE-2 remain that goal's units; this delivery
names their gates only. The §20 losslessness ruling stays with the operator.

---

*Planning artifact only. 14 Hand units (F3-A…F3-N), dependency-ordered, six
of them parallel on disjoint modules; FS-4 unminted. Verified against faber
`4e4b62b` and radix main on 2026-08-19.*
