# Delivery: FCMP profile 1 — freeze-path residuals, then gated reference + FHIR switch

**Assignment**: Vivi task `5c143bf6` (Mind → planner, 2026-08-18) — lower registered goal `gol_58147dc9ef95b704`
**Planner**: planner (handle `5c143bf6`); no packet; evidence from main checkouts only
**Goal**: [`goal.md`](goal.md) (`gol_58147dc9ef95b704`) — **Status** still `planned` / draft; U-1 / U-2 / U-2a complete; not frozen
**Amendment**: [`amendment.md`](amendment.md) (U-2a fold authority; do not reopen unit 2)
**Protocol**: [`../../faber-messagepack-profile-v1.md`](../../faber-messagepack-profile-v1.md) @ faber `02327f9` (identity + prefix 64/3/2/256 folded; Status still draft)
**Related**: radix RTR [`radix-top-level-recomposition/goal.md`](../../../radix/docs/factory/radix-top-level-recomposition/goal.md); FHIR format goal still unwritten
**Baselines at lowering**: faber `0fe3a00`; radix `973e2a80b`
**Status**: planned — delivery lowered 2026-08-18; implementation closed on freeze; two freeze-path Hands are dispatchable now
**This artifact is planning only.** No product code, no freeze, no unit 3 dispatch.

The registered goal lives in **faber**, not radix. Mind's spawn path `radix/docs/factory/fcmp-profile-1/` is a stale locator. This spec is committed beside the goal.

---

## 1. Goal-check summary

| Field | Value |
| --- | --- |
| goal_path | `faber/docs/factory/fcmp-profile-1/goal.md` (`gol_58147dc9ef95b704`) |
| evaluator mode | cold self-pass against live protocol, live FHIR codec, live RTR types, CTO receipts |
| intended consumer | delivery (this spec) → Mind files Hands; Head + operator own freeze |
| verdict | **READY** (gated) |

**Reasoning.** The desired end state is one byte-deterministic FCMP frame plus canonical MessagePack, admitted only by strict decoders, with FHIR as the first consumer. Architecture for the *profile* is locked in the protocol file after U-2a. The two CTO freeze holes (optional/default identity, envelope-prefix limits) are folded (`02327f9`). The §14 re-check (`0dc52153`) returned `record_risk` and is **not** a freeze. Remaining admitted work is Hand-lowerable as a gated graph: two freeze-path docs units now, then a Head/operator freeze, then the reference codec, then FHIR DTOs/TS, then the writer/reader switch. Implementation Hands must not start while Status is draft.

**Key points.**

- Units 1, 2, 2a are complete. Do not reopen them.
- RTR2 (`AnalyzedProgram` graph, `55a7b72a9` / merge `ddc0810f1`) and RTR3b (`radix-package` → `radix-program`, `eb24be9ff` / merge `a8a9055ee`) are **landed**. The goal's "implementation conflicts with RTR in flight" sentence is stale. The RTR half of the unit-3 gate is satisfied. The crate-home gate is open.
- Freeze is still closed. CTO `0dc52153`: `policy.md` does not carry the FCMP current-plus-two window. Amendment residuals still unnamed in the protocol: kind-reuse as BCP-14 `MUST NOT`, shared error-class taxonomy, reserved fixture kind `fcmp.test`.
- Live FHIR is still postcard + serde (`radix-hir-fhir` `decode.rs` / `package.rs`; `SCHEMA_VERSION = 3`; `PACKAGE_SCHEMA_VERSION = 1`). Not rmp-serde. Not FCMP.
- No FHIR schema may be invented here (goal non-goal). Units 4–5 stay in this goal's admitted table but are entry-gated on an as-yet-unwritten FHIR format goal that publishes concrete finite limits.
- Crate-home default is recorded below so Hands do not choose.

**Blocking gaps:** none at the planning boundary. Gated items: operator freeze after PRE units + Head §14 re-check; FHIR format goal (unwritten) before U-4.

**Recommended next step.** Mind files **U-PRE-1** and **U-PRE-2** (write-disjoint, parallel). After both land, file Head §14 re-check (still not a freeze), then wait for an explicit operator freeze. Do **not** file U-3A until Status is frozen.

---

## 2. Interpreted unit

Lower the remaining admitted FCMP profile-1 scope into Hand-sized units:

1. Close the freeze-path residuals named by CTO `0dc52153` and `amendment.md` (policy window; kind-reuse; error-class table; `fcmp.test`).
2. Re-check §14 and take a **separate** operator freeze decision. That decision is not a Hand and is not implied by this spec.
3. After freeze: a strict Rust reference codec plus the §11.1 generic profile vectors (goal unit 3).
4. After that and after FHIR schemas publish finite limits: format-owned FHIR DTOs, document vectors, and an independent TypeScript codec (goal unit 4).
5. Switch FHIR write/read to FCMP only and reject postcard with a structured diagnostic (goal unit 5).

This lowering does **not** freeze the profile, invent FHIR schemas, swap postcard for rmp-serde, change FMIR, build a generic framework before a second consumer, or reopen units 1–2a.

---

## 3. Normalized spec

**End state (admitted, whole goal).** Faber durable artifacts use one FCMP encoding. A strict decoder admits only canonical bytes. FHIR unit/package are the first registered kinds, each with published finite limits. A TypeScript implementation re-encodes every positive fixture byte-identically and rejects shared negatives with the shared error class. Postcard FHIR is rejected closed. Profile Status is frozen only by an explicit operator decision after a post-PRE §14 re-check.

**Split boundary.** The goal's unit-3/4/5 rows are themes, not Hands. This spec keeps those ids as phase groups and lowers them to one-logical-change Hands.

```text
{U-PRE-1 ‖ U-PRE-2}
  -> GATE-FREEZE  (Head §14 re-check + operator freeze; not a Hand)
    -> U-3A → U-3B → U-3C → U-3D → U-3E     # goal unit 3
      -> {U-4A ‖ U-4B} → U-4C → U-4D       # goal unit 4; extra gate: FHIR schemas published
        -> U-5A → U-5B                     # goal unit 5
```

**Locked decisions (Hands do not reopen).**

1. **Optional vs default = (a).** Already in protocol §3.1 / §4.1. Encoders omit defaults. Present-at-default is `noncanonical`. `nil` is never the omitted-optional encoding.
2. **Prefix limits 64 / 3 / 2 / 256.** Already in §1 / §2 / §6. Checked from MessagePack headers before allocation. Root keys in canonical order so `value` is not allocated to learn `kind`.
3. **Kind strings MUST NOT be reused.** Already implied by §10 ("never reused"). U-PRE-2 makes it BCP-14.
4. **Error-class taxonomy** is the table in §3.1 of this spec. Extracted from existing MUST-reject rules. U-PRE-2 folds it into the protocol. Hands do not invent new class names.
5. **Fixture kind `fcmp.test`.** Reserved in §10 by U-PRE-2. Product decoders MUST NOT admit it while reserved. U-3E may register it for the conformance decoder only, with published finite limits.
6. **Crate home.** New workspace crate `radix-fcmp` under `radix/crates/radix-fcmp`. No HIR / `AnalyzedProgram` / compiler-struct deps. Workspace **member**, **not** a default-member (reached by `cargo test -p radix-fcmp`, same membership posture as `radix-program`). No `rmp-serde`. Prefer a primitive / hand-written MessagePack writer; a high-level serializer is allowed only when every byte is conformance-proven (protocol §8 / §12).
7. **Registry home.** `faber/docs/faber-messagepack-profile-v1.md` §10 remains the registry until a second *product* kind is registered. Do not invent a second registry format.
8. **Shared fixtures.** `faber/docs/fcmp/vectors/` (created by U-3E / U-4C). Rust and TS load the same bytes. They MUST NOT share encoder source.
9. **TS home.** New `faber/fcmp/` conformance package. Not `@faber/runtime` (that package is generated-TS helpers).
10. **FHIR DTO shape.** `fhir.unit` follows the post-RTR2 module contract (`ModuleId` = `{package, segments}` in `radix-program/src/artifact_plan.rs`). `fhir.package` follows post-RTR2/RTR3b `AnalyzedProgram` (`spec`, `entry: Option<ModuleId>`, `roots`, `nodes: BTreeMap<ModuleId, AnalyzedProgramNode>`, `imports`, diagnostics). Current `HirArtifact` / vector-era postcard envelopes are migration input, not DTO templates. Concrete field lists and per-kind limits come from the FHIR format goal, not this spec.
11. **No dual writer, no silent postcard fallback, no indefinite compatibility decoder** (protocol §13).
12. **Profile versions ≠ product semver.** Current-plus-two is a per-kind FCMP window. U-PRE-1 makes `policy.md` carry that cost.

### 3.1 Error-class table (lock; U-PRE-2 folds this)

Structured decoder errors carry `class` as one of these lowercase ASCII identifiers. Rust and TS share the identifiers. Each row is a protocol MUST-reject already written in §1–§7 / §10; this table names them.

| `class` | Meaning (already required) |
| --- | --- |
| `truncated` | payload shorter than declared length, or unexpected end (§1.4) |
| `trailing_bytes` | bytes after the declared payload (§1.4) |
| `bad_magic` | preamble is not `FABERMP\0` (§1.8) |
| `profile_unsupported` | profile major/minor the decoder does not accept (§7.1) |
| `payload_limit` | declared length above the enabled-kind / empty-set cap (§1.5–6, §6) |
| `kind_string_limit` | `kind` string header longer than 64 bytes (§6) |
| `root_map` | root map count ≠ 3, or keys not `kind` then `schema` then `value` (§2, §6) |
| `schema_array` | `schema` array length ≠ 2, or elements not unsigned 16-bit (§2, §6) |
| `kind_unregistered` | unknown kind, or reserved-but-unregistered kind admitted (§2.5, §10) |
| `schema_unsupported` | document schema major/minor the decoder does not accept (§7.2) |
| `noncanonical` | any alternate encoding of a legal schema value, including present-at-default OPTIONAL, overlong primitives, float32, unsorted keys (§3, §5.10) |
| `duplicate_key` | duplicate map key (§3.7) |
| `unknown_field` | field not in the admitted schema (§4.1) |
| `missing_field` | REQUIRED field absent (§4.1) |
| `type` | wrong primitive, or `nil` where null is not legal (§3.1, §5.8) |
| `utf8` | string payload is not valid UTF-8 (§3.4) |
| `extension` | MessagePack extension / timestamp (§3.8) |
| `limit` | a kind-specific resource limit; payload names the violated limit (§6) |
| `overflow` | length/budget arithmetic overflow (§6) |
| `legacy` | known non-FCMP product bytes (postcard FHIR) named for a diagnostic (§1.8, §13) |

Do not add classes in implementation. If a reject has no row, stop and route a protocol amend.

---

## 4. Repo-aware baseline (verified 2026-08-18)

Authority order: live source + CTO receipts → protocol file → goal / amendment prose.

**Protocol (faber `02327f9`, still draft).**

- Status line: "draft 1 — amendments folded; protocol review re-check required before implementation or freeze."
- Identity (a) and prefix 64/3/2/256 are in §1, §2, §3.1, §4.1, §6, §11.1.
- §10: "Kind strings are never reused" (not yet `MUST NOT`). Reservations: `fhir.unit`, `fhir.package` only. No `fcmp.test`. No error-class table.
- §14 freeze list is still the review gate. Implementation MUST NOT begin while those choices are treated as serializer defaults, and the draft MUST NOT be declared frozen until §14 is re-checked after amendments.

**CTO receipts.**

- `5a4e974a` `correct_before_next_phase`: the two holes U-2a closed.
- `0dc52153` `record_risk`: holes closed in protocol; freeze still blocked because `radix/docs/release/faber/policy.md` does not carry the FCMP current-plus-two window; do not freeze, implement, reopen unit 2, or dispatch unit 3.

**Release policy (radix, live).** `docs/release/faber/policy.md` states that a locked line needs *a* support window. It never names FCMP, MessagePack, current-plus-two, or per-kind document majors. Protocol §7.1 / §7.3 cite this file as the place that must carry the FCMP window. The citation is currently a hole.

**RTR (radix, live — goal text is stale).**

- `AnalyzedProgram` / `AnalyzedProgramNode` live in `radix/crates/radix-program/src/analyze.rs` (`spec`, `entry`, `roots`, `nodes: BTreeMap<ModuleId, …>`, `imports`, `diagnostics`, `library_resolver`).
- `ModuleId` is `{package: PackageId, segments: Vec<String>}` in `radix/crates/radix-program/src/artifact_plan.rs`. One `ModuleId`; do not mint a second.
- RTR goal Status: RTR0–RTR3c landed; next is RTR4 (CLI extract). Not an FCMP blocker.

**Live FHIR wire (radix, still postcard).**

- `crates/radix-hir-fhir/src/decode.rs`: `postcard::to_allocvec` / `from_bytes`.
- `crates/radix-hir-fhir/src/package.rs`: `PACKAGE_SCHEMA_VERSION = 1`; per-unit `SCHEMA_VERSION = 3` in `artifact.rs`.
- Crate depends on `radix-hir`, `radix-lexer`, `radix-types`, `postcard`, `serde`. That is the compiler-struct wire the protocol forbids as the public contract.
- No `rmp` / `rmpv` / `rmp-serde` workspace dep.

**No FCMP implementation exists.** Workspace grep for `FABERMP` / FCMP codec hits only the protocol and this goal. LLVM `fcmp` IR is unrelated.

**TS surface.** `faber/runtime/typescript` is `@faber/runtime` generated-code helpers. It is not a MessagePack stack and is not the independent FCMP implementation.

**Foreign dirt.** faber main clean. radix main has unrelated `M docs/design/README.md` — out of this write scope; do not touch.

---

## 5. Stage graph (Hand units)

Every unit carries the eight campaign-rule-2 fields. `closeout_command` names a target that **already exists** or that **this unit creates**; the "Exists?" column says which.

```text
U-PRE-1 ‖ U-PRE-2
        └─► GATE-FREEZE (Head + operator; not filed as a Hand)
                └─► U-3A → U-3B → U-3C → U-3D → U-3E
                                    └─► U-4A ‖ U-4B   [+ FHIR-schema gate]
                                          └─► U-4C → U-4D → U-5A → U-5B
```

### GATE-FREEZE (not a Hand)

After U-PRE-1 and U-PRE-2 land, Mind files `head-cto` for a §14 re-check against the folded protocol **and** the updated `policy.md`. Disposition `proceed` is still not a freeze. Freeze is an operator decision recorded on the protocol Status line and the goal Status line. Implementation Hands stay unfiled until that Status says frozen.

### U-PRE-1 — `policy.md` carries the FCMP current-plus-two window

| Field | Value |
| --- | --- |
| `outcome` | `radix/docs/release/faber/policy.md` states the per-kind FCMP current-plus-two document-major support window as a release-record cost, separate from Faber product semver and from the odd/even LTS lane. Ownership of that cost is named. A draft major does not count as stable. Legacy postcard FHIR is not FCMP major zero. |
| `write_scope` | `radix/docs/release/faber/policy.md` only |
| `first_failing_oracle` | `rg -n -i 'fcmp\|current-plus-two\|messagepack' docs/release/faber/policy.md` in the radix checkout — today: no matches (verified 2026-08-18). |
| `closeout_command` | `rg -n -i 'FCMP|current-plus-two' docs/release/faber/policy.md` (cwd: radix). **Exists:** `rg` and `policy.md` both exist. |
| `expected_observed_result` | At least one hit that names FCMP and current-plus-two as a per-kind release-record cost; odd/even LTS language unchanged; no freeze of FCMP 1.0 claimed. |
| `est_basis` | `rivus-docs-note` |
| `stop_condition` | Stop if the edit would freeze FCMP, change odd/even lane rules, or invent a support duration the operator has not approved (policy already refuses invented durations). |
| `depends_on` | none |

### U-PRE-2 — protocol residuals: kind-reuse, `fcmp.test`, error-class table

| Field | Value |
| --- | --- |
| `outcome` | Fold three already-locked residuals into `faber/docs/faber-messagepack-profile-v1.md` only: (1) §10 "Kind strings are never reused" becomes `Kind strings MUST NOT be reused`; (2) reserve `fcmp.test` in the §10 table as a generic-vector fixture kind, reserved not registered; (3) add the §3.1 error-class table as the shared `class` vocabulary and point §11.3 at it. Status stays draft. Not a freeze. |
| `write_scope` | `faber/docs/faber-messagepack-profile-v1.md` only |
| `first_failing_oracle` | `rg -n 'MUST NOT be reused|fcmp\.test|^\\| `truncated`' docs/faber-messagepack-profile-v1.md` in the faber checkout — today: no `fcmp.test`, no `MUST NOT be reused`, no error-class table (verified 2026-08-18). |
| `closeout_command` | `rg -n 'MUST NOT be reused|fcmp\.test|truncated|kind_unregistered' docs/faber-messagepack-profile-v1.md` (cwd: faber). **Exists:** `rg` and the protocol file both exist. |
| `expected_observed_result` | All four patterns present; Status line still draft; `fhir.unit` / `fhir.package` still reserved; no freeze language; no implementation. |
| `est_basis` | `rivus-docs-note` |
| `stop_condition` | Stop if the fold would freeze, add a class not in §3.1, register `fcmp.test`, or reopen identity / prefix text. |
| `depends_on` | none (parallel with U-PRE-1; different repo) |

### U-3A — `radix-fcmp` crate + 20-byte frame

| Field | Value |
| --- | --- |
| `outcome` | Create workspace crate `radix-fcmp` (member, not default-member) with encode/decode of the 20-byte preamble (`FABERMP\0`, profile major/minor, payload length) and exact end-of-frame. Reject `bad_magic`, `truncated`, `trailing_bytes`, `profile_unsupported`, empty-set `payload_limit` (>256 when no kind enabled). No root map yet. No HIR deps. No `rmp-serde`. |
| `write_scope` | `radix/crates/radix-fcmp/**` (new), `radix/Cargo.toml` (member list only; do not add to `default-members`), `radix/Cargo.lock` if membership requires it |
| `first_failing_oracle` | `cargo test -p radix-fcmp` in radix — today: no such package (verified: crate absent). Record that red. |
| `closeout_command` | `cargo test -p radix-fcmp --lib` (cwd: radix). **Creates:** this unit creates the crate and the frame tests the command runs. |
| `expected_observed_result` | Package exists; frame tests pass; no crate in `default-members`; `git grep rmp-serde crates/radix-fcmp` empty; exit 0. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if crate-dag / workspace law forbids a new member without a separate membership unit (route that unit; do not hide the crate inside `radix-hir-fhir`). Stop if the encoder needs compiler types. **Entry gate:** protocol Status is frozen (GATE-FREEZE). |
| `depends_on` | GATE-FREEZE (U-PRE-1 + U-PRE-2 + Head §14 re-check + operator freeze) |

### U-3B — root map + envelope-prefix limits

| Field | Value |
| --- | --- |
| `outcome` | Decode/encode the root map with prefix limits: kind-string ≤ 64, map exactly 3, schema array exactly 2, canonical key order, empty-set payload cap already in U-3A. Header-only rejects: `kind_string_limit`, `root_map`, `schema_array`. Do not allocate `value` to learn `kind`. |
| `write_scope` | `radix/crates/radix-fcmp/**` |
| `first_failing_oracle` | After U-3A: a 65-byte `kind` / map-count≠3 / schema-len≠2 fixture is not rejected with the locked class — record the failing test name this unit adds. |
| `closeout_command` | `cargo test -p radix-fcmp --lib` (cwd: radix). **Creates:** this unit adds the prefix-limit tests. |
| `expected_observed_result` | Header-only negatives fail with `kind_string_limit` / `root_map` / `schema_array`; value is not allocated on those paths; exit 0. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if a reject requires reading past the MessagePack header, or if a new error class appears. |
| `depends_on` | U-3A |

### U-3C — canonical primitives

| Field | Value |
| --- | --- |
| `outcome` | Canonical primitives: shortest integers, float64-only + canonical NaN `0x7ff8000000000000`, shortest string/binary/array/map headers, UTF-8, string map keys sorted by UTF-8 bytes, unique keys, no extensions. Classes: `noncanonical`, `utf8`, `duplicate_key`, `extension`, `type`. |
| `write_scope` | `radix/crates/radix-fcmp/**` |
| `first_failing_oracle` | After U-3B: float32 / overlong int / unsorted keys / extension / invalid UTF-8 are not rejected as the locked classes — record the new failing tests. |
| `closeout_command` | `cargo test -p radix-fcmp --lib` (cwd: radix). **Creates:** this unit adds the primitive canonicality tests. |
| `expected_observed_result` | Listed negatives fail with the locked classes; positive shortest-form integers/floats/strings/maps re-encode byte-identically; exit 0. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if NaN/infinity policy would need a FHIR schema (protocol leaves that to the owning schema; this unit only encodes IEEE bits and canonical NaN). |
| `depends_on` | U-3B |

### U-3D — schema shapes + omit-defaults identity

| Field | Value |
| --- | --- |
| `outcome` | Records (named string keys, omit-when-default, present-at-default → `noncanonical`, `nil` distinct), tagged unions (`tag` / `tag`+`value`), tuples, logical maps/sets with canonical sort. Classes: `noncanonical`, `missing_field`, `unknown_field`, `type`. |
| `write_scope` | `radix/crates/radix-fcmp/**` |
| `first_failing_oracle` | After U-3C: a present-at-default OPTIONAL fixture is accepted, or `nil` is treated as omitted-optional — record the new failing tests. |
| `closeout_command` | `cargo test -p radix-fcmp --lib` (cwd: radix). **Creates:** this unit adds the shape/identity tests. |
| `expected_observed_result` | Omit-default pair behaves as protocol (a); present-at-default is `noncanonical`; `nil` stays `type` unless the field admits null and is REQUIRED; exit 0. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if the implementation normalizes noncanonical bytes on the admission path. |
| `depends_on` | U-3C |

### U-3E — generic profile vectors (§11.1) + optional `fcmp.test` registration

| Field | Value |
| --- | --- |
| `outcome` | Land the §11.1 generic vector suite as shared bytes under `faber/docs/fcmp/vectors/generic/` and rust tests that encode/decode them. Include the U-2a pair (omitted optional vs present-at-default), kind>64, empty-set payload>256, root≠3, schema≠2. If the suite needs a kind, register `fcmp.test` with published finite limits for the conformance decoder only; product decoders still must not admit it unless that registration is explicit and limited. SHA-256 of the complete frame is the content identity. |
| `write_scope` | `radix/crates/radix-fcmp/**`, `faber/docs/fcmp/vectors/generic/**` (new), protocol §10 registration row only if `fcmp.test` is registered |
| `first_failing_oracle` | `ls faber/docs/fcmp/vectors/generic` — today: directory absent. |
| `closeout_command` | `cargo test -p radix-fcmp` (cwd: radix) and `ls docs/fcmp/vectors/generic` (cwd: faber). **Creates:** this unit creates the vector directory and the suite. |
| `expected_observed_result` | Every §11.1 bullet has a fixture; positives re-encode byte-identically with recorded SHA-256; negatives fail with the locked class; exit 0. Goal unit 3 is then complete. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if a required vector needs a FHIR field or a new error class. |
| `depends_on` | U-3D |

### U-4A — `fhir.unit` format-owned DTO + finite limits

| Field | Value |
| --- | --- |
| `outcome` | Format-owned `fhir.unit` wire DTO + conversion boundary from the post-RTR2 module record (`ModuleId`, schema-defined contents, import edges). Publish concrete finite limits and register the kind in §10. Never serialize `HirArtifact` / compiler structs as the public wire. |
| `write_scope` | `radix/crates/radix-fcmp/**` and/or a thin `radix-hir-fhir` conversion module that depends on `radix-fcmp` (not the reverse); `faber/docs/faber-messagepack-profile-v1.md` §10 registration row |
| `first_failing_oracle` | No `fhir.unit` registration and no FCMP unit DTO exist (live wire is postcard `HirArtifact`). |
| `closeout_command` | `cargo test -p radix-fcmp` plus the focused DTO test this unit adds. **Creates:** those DTO tests. |
| `expected_observed_result` | Kind registered with finite limits; DTO round-trip is FCMP bytes; `git grep postcard` in the new DTO path is empty; compiler structs are not the wire type. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | **Entry gate:** a FHIR format goal has published the concrete unit schema and finite limits. Stop and do not invent fields if that publication is missing. Stop if the DTO copies `HirArtifact`. |
| `depends_on` | U-3E; FHIR format goal schema publication |

### U-4B — `fhir.package` format-owned DTO + finite limits

| Field | Value |
| --- | --- |
| `outcome` | Format-owned `fhir.package` wire DTO against `AnalyzedProgram` facts: explicit roots, optional entry, stable module records, import edges, library identities, graph-specific limits. Register the kind. Vector-era `FhirPackage` postcard envelope is migration input only. |
| `write_scope` | same crate home as U-4A; protocol §10 registration row |
| `first_failing_oracle` | No `fhir.package` FCMP DTO (live envelope is postcard `PACKAGE_SCHEMA_VERSION = 1`). |
| `closeout_command` | `cargo test -p radix-fcmp` plus the focused package-DTO test this unit adds. **Creates:** those tests. |
| `expected_observed_result` | Kind registered with graph limits; DTO carries roots/entry/`ModuleId` keys; no postcard in the new path. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | **Entry gate:** FHIR format goal has published the package schema and finite limits. Stop if the DTO copies `FhirPackage` field order as authority. |
| `depends_on` | U-3E; FHIR format goal schema publication |

U-4A ‖ U-4B are write-disjoint if one Hand owns unit DTO files and the other owns package DTO files. Serialize if they share one new module file.

### U-4C — FHIR document vectors (§11.2)

| Field | Value |
| --- | --- |
| `outcome` | Shared document fixtures under `faber/docs/fcmp/vectors/fhir/`: smallest valid unit and package, representative positives with bytes + SHA-256, negatives for every structural/referential class, provenance record. |
| `write_scope` | `faber/docs/fcmp/vectors/fhir/**` (new), rust tests in `radix/crates/radix-fcmp/**` that load those files |
| `first_failing_oracle` | `ls faber/docs/fcmp/vectors/fhir` — today: absent. |
| `closeout_command` | `cargo test -p radix-fcmp` (cwd: radix). **Creates:** this unit creates the fhir vector dir and the loader tests. |
| `expected_observed_result` | §11.2 bullets covered; rust re-encodes positives byte-identically; negatives match locked classes. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if a fixture requires an unpublished schema field. |
| `depends_on` | U-4A, U-4B |

### U-4D — independent TypeScript decoder/encoder (§11.3)

| Field | Value |
| --- | --- |
| `outcome` | Independent TS implementation in `faber/fcmp/` that decodes and re-encodes every positive generic + FHIR fixture byte-identically and rejects every shared negative with the same `class`. No shared encoder source with Rust. |
| `write_scope` | `faber/fcmp/**` (new). May *read* `faber/docs/fcmp/vectors/**`. Must not edit rust crates. |
| `first_failing_oracle` | `ls faber/fcmp` — today: absent. |
| `closeout_command` | the test command this unit adds under `faber/fcmp/` (record the exact command in the receipt; likely a small node-free or documented runner — **do not** introduce Bun). **Creates:** this unit creates the package and its test entry. |
| `expected_observed_result` | §11.3 items 2–4 hold for the shared fixtures; repeated runs are byte-identical; goal unit 4 is then complete. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if the TS stack would need a new runtime language policy, or if it copies rust encoder code. Faber Agents.md: Cargo and `scripta/` only — not Bun or Node as a *product* toolchain; a bounded conformance runner is allowed if recorded. If no acceptable runner exists, stop and route a need rather than inventing a Node-based product path. |
| `depends_on` | U-4C |

### U-5A — FHIR writer switch to FCMP only

| Field | Value |
| --- | --- |
| `outcome` | FHIR writers emit FCMP only. No dual writer. No postcard encode on the product write path. |
| `write_scope` | FHIR write call sites (today `radix-hir-fhir` `encode` / `encode_package` and their callers under `radix-program` / `radix` / `faber` as found by grep at implementation time). Do not keep a postcard writer "for tests" on the product path. |
| `first_failing_oracle` | Product write still calls `postcard::to_allocvec` (live in `decode.rs` / `package.rs`). |
| `closeout_command` | focused rust tests this unit adds on the write path plus `rg -n 'postcard::to_allocvec' crates/radix-hir-fhir/src` (cwd: radix) showing product encode gone or confined to a named legacy-reject test helper. **Creates:** the writer-switch tests. |
| `expected_observed_result` | New artifacts are FCMP (magic `FABERMP\0`); no product dual writer. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if a caller still needs postcard for FMIR (FMIR is out of scope; do not touch it). |
| `depends_on` | U-4D |

### U-5B — FHIR reader switch + postcard `legacy` reject

| Field | Value |
| --- | --- |
| `outcome` | FHIR readers admit FCMP only. Postcard bytes fail closed with structured class `legacy` (and a diagnostic code). No silent reinterpret. No compatibility decoder. |
| `write_scope` | FHIR read call sites (`decode` / `decode_package` and callers); diagnostic catalog row if a new code is required |
| `first_failing_oracle` | Product read still `postcard::from_bytes` and accepts current fixtures. |
| `closeout_command` | focused rust tests this unit adds: one positive FCMP load, one postcard fixture → `legacy`. **Creates:** those tests. |
| `expected_observed_result` | FCMP loads; postcard rejects `legacy`; goal unit 5 and the goal invariant are then complete. |
| `est_basis` | `compiler-surface-feature` |
| `stop_condition` | Stop if rejection would be a string-match test against English prose (assert `class` / diagnostic `code` + `issue`). Stop if FMIR postcard is pulled into scope. |
| `depends_on` | U-5A |

---

## 6. Implementation work (Mind pointers)

Mind files Hands from these ids. This planner does not file or spawn them.

| id | write_scope (short) | done_when | dispatchable? |
| --- | --- | --- | --- |
| `U-PRE-1` | `radix/docs/release/faber/policy.md` | policy names FCMP current-plus-two | **yes now** |
| `U-PRE-2` | `faber/docs/faber-messagepack-profile-v1.md` | MUST NOT reuse + `fcmp.test` reserved + error-class table; still draft | **yes now** (‖ PRE-1) |
| `GATE-FREEZE` | protocol + goal Status lines (operator / Head) | Status frozen after §14 re-check | after PRE; **not a Hand** |
| `U-3A` | `radix/crates/radix-fcmp/**`, workspace member | frame codec + crate exists | after freeze |
| `U-3B` | `radix/crates/radix-fcmp/**` | prefix limits header-only | after 3A |
| `U-3C` | `radix/crates/radix-fcmp/**` | primitive canonicality | after 3B |
| `U-3D` | `radix/crates/radix-fcmp/**` | shapes + omit-defaults | after 3C |
| `U-3E` | `radix-fcmp` + `faber/docs/fcmp/vectors/generic/` | §11.1 suite green | after 3D |
| `U-4A` | DTO + §10 `fhir.unit` | unit kind registered with limits | after 3E **and** FHIR schema pub |
| `U-4B` | DTO + §10 `fhir.package` | package kind registered with limits | after 3E **and** FHIR schema pub |
| `U-4C` | `faber/docs/fcmp/vectors/fhir/` + rust loaders | §11.2 suite | after 4A+4B |
| `U-4D` | `faber/fcmp/**` | §11.3 TS gate | after 4C |
| `U-5A` | FHIR write call sites | FCMP-only writer | after 4D |
| `U-5B` | FHIR read call sites | postcard → `legacy` | after 5A |

---

## 7. Checkpoints and gates

| Gate | When | Owner | Content |
| --- | --- | --- | --- |
| **SG-P** | after U-PRE-1 ‖ U-PRE-2 | Mind accepts docs | policy names the window; protocol has MUST NOT / `fcmp.test` / class table; Status still draft |
| **SG-F** | after Head §14 re-check + operator freeze | operator | protocol Status frozen; goal Status updated; **still not** unit 3 until this stamp exists |
| **SG-3** | after U-3E | Mind | generic vectors green in rust; goal unit 3 done |
| **SG-4** | after U-4D | Mind | FHIR kinds registered; TS §11.3 green; goal unit 4 done |
| **SG-5** | after U-5B | Mind | FHIR e2e on FCMP only; postcard `legacy`; goal complete |

**Batching / split decision.** Two docs Hands in parallel, then a freeze gate, then a serial codec spine (U-3A–E), then DTO fan-out (4A ‖ 4B) only after schemas exist, then TS, then write/read switch. Do not bag 3A–E into one Hand (frame, prefix, primitives, shapes, and the vector suite are different families). Do not bag 5A+5B (writer vs fail-closed reader).

**Lane-owned gates (named once, not copied onto children).** Lint / `./scripta/test` / merge own workspace compile and stage-1–4. Hands do not run `./scripta/test --full`, `--stage`, or workspace `cargo test`.

**Release posture.** `defer-release`. This is a wire-format contract. A Faber product release that ships FCMP FHIR is a later release-boundary item (protocol §7.1 / §13). Completing this spec does not itself cut a version.

---

## 8. Validation

**Hand sanity** = each unit's `closeout_command` only.

**Lane-owned** (not on Hands): `./scripta/check-factory-goal-status` after goal Status updates; radix `./scripta/test` stage 1 when factory docs move; no e2e Metal/CUDA.

**Existence rule (memo `06c99530`).** Every `closeout_command` either already exists (PRE-1/2: `rg` + named files) or is created by that same unit (U-3A creates `-p radix-fcmp`; later rust units add tests in that crate; U-3E/U-4C create vector dirs; U-4D creates `faber/fcmp`; U-5 creates switch tests). No unit names a test function that this spec pretends already exists.

---

## 9. Companion skill plan

| Skill | When |
| --- | --- |
| `correctness` | U-3 / U-4 / U-5 codecs and fail-closed rejects |
| `clean-break` | U-5 postcard removal (compat guilty until required; protocol already forbids dual writer) |
| `red-green` | each rust/TS unit: first_failing_oracle before the edit |
| `faber` | locale/diagnostic identity on U-5B (`code` + `issue`, not English fragments) |

---

## 10. Open questions (defaults recorded; none blocks U-PRE-1)

1. **Operator freeze after SG-P.** Default: leave draft until the operator stamps freeze. This spec does not freeze.
2. **FHIR format goal is unwritten.** Default: U-4A/U-4B stay gated; do not invent schemas. Forging that goal is a separate planner assignment, not a silent expansion of this one.
3. **TS conformance runner.** Default: U-4D records the runner it adds. If the only option is a new Node product path, stop (Faber tooling law).
4. **`radix-fcmp` default-member?** Default: **no** (lock 6). Revisit only if U-5 needs it on the default compile path.
5. **Diagnostic code for `legacy`.** Default: add a structured code in the same unit as U-5B; do not reuse a postcard-internal error as the product identity.

---

## 11. Scope closure

Admitted goal units 1 and 2 / 2a stay complete. Admitted remaining units 3, 4, and 5 are all in this graph (as U-3A–E, U-4A–D, U-5A–B) plus the freeze-path residuals the CTO named as blockers. Nothing is narrowed, deferred out of the goal, or marked optional. U-4/U-5 wait on an unwritten FHIR schema publication — that is an entry gate, not a scope cut. Completing U-PRE-* is not goal completion. Completing U-3E is not FHIR completion.

---

## 12. First implementation frontier

**U-PRE-1** and **U-PRE-2** are the only dispatchable Hands. After both land: Head §14 re-check, then operator freeze, then U-3A. Do not file U-3A against a draft Status.

---

*Planning artifact only. 13 Hand units (U-PRE-1, U-PRE-2, U-3A–E, U-4A–D, U-5A–B) plus GATE-FREEZE. Verified against faber `0fe3a00` and radix `973e2a80b` on 2026-08-18.*
