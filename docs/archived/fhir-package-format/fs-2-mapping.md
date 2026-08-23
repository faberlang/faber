# FS-2: FHIR MessagePack mapping

**Status**: specified — MessagePack encoding of the FS-1 field spec
**Created**: 2026-08-18
**Goal**: [`goal.md`](goal.md)
**Field spec**: [`fs-1-field-spec.md`](fs-1-field-spec.md) (19 families; live postcard types)
**Envelope rules**: [`../../faber-messagepack-profile-v1.md`](../../faber-messagepack-profile-v1.md) (draft FCMP 1.0)
**Authority**: operator addendum
[`../fcmp-profile-1/addendum.md`](../fcmp-profile-1/addendum.md)

**This unit does not**: implement a codec, publish kind limits, register
`fhir.unit` / `fhir.package`, freeze FCMP 1.0, or switch writers. Those are
FS-3 / FS-4 and the profile freeze gate.

This document binds a `value` to the draft envelope. It is the encoding of
FS-1, not a new inventory. Field names that appear here are FS-1 serde
names (already lowercase ASCII). Enum **tags** are the FS-1 variant names
rewritten by the [tag spelling](#16-tag-spelling) rule.

Postcard-dropped live fields from FS-1 §14 are **not** MessagePack fields.
They are listed in [§20 SPEC OPEN QUESTION](#20-spec-open-question--named-postcard-gaps) for the operator losslessness ruling.

---

## 0. Completeness

Fact-family count remains **19** — the same two envelopes as FS-1
(`HirArtifact` = 13, `FhirPackage` = 6). Nested records below are encodings
of fields inside those families.

| Envelope | Postcard constant | FCMP kind (reserved) | FCMP document schema |
| --- | --- | --- | --- |
| Unit | `SCHEMA_VERSION = 3` | `fhir.unit` | `[1, 0]` (this mapping; draft) |
| Package | `PACKAGE_SCHEMA_VERSION = 1` | `fhir.package` | `[1, 0]` (this mapping; draft) |

The FCMP root `schema` versions **this mapping**. The inner
`schema_version` / `package_schema_version` fields remain postcard-carried
facts and still travel (must equal `3` / `1`). Kinds stay reserved until
finite per-kind limits are published (FCMP §6 / §10). This document does
not invent those numbers.

A standalone unit is one framed FCMP document whose `value` is the
`HirArtifact` record. A package is one framed FCMP document whose `value`
is the `FhirPackage` record. Embedded units inside a package are **nested
`HirArtifact` records**, not framed documents and not postcard byte
blobs. That is the FS-1 §10.1 encoding choice.

---

## 1. Binding rules (FCMP applied to every family)

These rules are not restated per field except where a family specializes
them. They apply to the `value` and to every nested record, enum, array,
and logical map.

### 1.1 Named fields, one encoding, strict reject

1. Records and the document root use **named string keys**. Numeric field
   IDs MUST NOT be used as an alternate representation.
2. Keys appear in **ascending lexicographic order of their UTF-8 bytes**.
   Tables in this document list fields in that emission order, not FS-1
   declaration order.
3. Unknown keys are `unknown_field`. Duplicate keys are `duplicate_key`.
   Out-of-order keys are `noncanonical`.
4. There is **one** canonical encoding per schema value. Alternate
   MessagePack opcodes for the same value (overlong integers, float32,
   present-at-default OPTIONAL, unsorted maps) are `noncanonical`.
5. Artifact admission is fail-closed. A decoder MUST NOT normalize
   noncanonical bytes into an admitted document.

### 1.2 REQUIRED, OPTIONAL, nullable

| Kind | Wire | Default | Reject |
| --- | --- | --- | --- |
| **REQUIRED** | key always present | — | absent → `missing_field` |
| **OPTIONAL** | key omitted iff the schema value equals the named default | named in the field table; never implicit null | present-and-equal-to-default → `noncanonical` |
| **REQUIRED nullable** | key always present; MessagePack `nil` means None | — | absent → `missing_field`; `nil` used for a non-nullable field → `type` |

`T?` in FS-1 is **REQUIRED nullable**: None is a legal distinct value, so
the field admits null and MUST always carry the key (FCMP §3.1 / §4.1).
A decoder MUST NOT treat a missing key as None.

OPTIONAL fields in this mapping use only these named defaults:

| Default token | Schema value |
| --- | --- |
| `false` | boolean false |
| `0` | unsigned integer zero (`u32` / `u64` as declared) |
| `[]` | empty array, empty logical map, or empty logical set |

An OPTIONAL field whose value is `true`, a non-zero integer, or a
non-empty collection MUST be present. Empty arenas are **not** OPTIONAL:
identity spaces (`interner`, `types.types`, `types.indices`) are REQUIRED
even when length is zero.

`nil` is never the encoding of an omitted OPTIONAL field.

### 1.3 Primitives

- Integers use the shortest MessagePack form for the numeric value
  (FCMP §3.2). Width and signedness are schema constraints, not opcodes.
  A value outside the declared range is `type`.
- Every IEEE carrier in this mapping (`HirLiteral::Float`,
  `JsonValue::Fractus`, `HirAnnotationValue::Float`, `TokenKind::Float`,
  `CliDefaultWire::Float`) is MessagePack float64 (`0xcb`) with canonical
  NaN bits `0x7ff8000000000000`. float32 is `noncanonical`. Finite and
  non-finite values are admitted at the codec layer; semantic "is this a
  legal Faber literal" is a document invariant, not an opcode rule.
- This float64 rule is **not** permission to crunch `f16` / `f32` tensor
  dtypes, integer widths, or buffer payloads into one float. See [§19](#19-widths-dtypeshape--type-table-value-facts).
- Strings use the shortest string header and MUST be valid UTF-8. No
  global Unicode normalization. Identifier strings in the interner are
  already NFKC; literal and comment payloads stay raw.
- Byte sequences use the shortest **binary** header. They MUST NOT be
  arrays of integers or strings.
- Arrays and maps use the shortest header for their length. The encoder
  MUST know the final length before the header.
- MessagePack extension values are forbidden (`extension`).

### 1.4 Enums

Unit variant:

```text
{ "tag": "variant_name" }
```

Payload variant (including empty-record / empty-array payloads):

```text
{ "tag": "variant_name", "value": ... }
```

- `tag` then `value` (canonical key order).
- `value` MUST be absent on a unit variant (`unknown_field` if present).
- `value` MUST be present on a payload variant (`missing_field` if absent).
- Unknown tags on a closed union are `type`. Every union in this mapping
  is closed.
- Tags are lowercase ASCII per [§1.6](#16-tag-spelling). Renaming a tag
  is a document-major change.

A single-field payload MAY be the payload encoding itself (a `u32`, a
nested enum, a record). A multi-field / tuple payload is a **named
record**, not a positional tuple, so later field addition is a documented
schema change rather than a silent position shift. Tree order of
sequences inside those records is still significant.

### 1.5 Collections

| FS-1 shape | Encoding | Order |
| --- | --- | --- |
| Tree sequence (items, statements, params, fields, tokens, …) | MessagePack array | source / lowering order; significant |
| Identity arena (`interner`, `types`, `indices`) | MessagePack array | index order is identity; MUST NOT sort |
| Logical map with string keys | MessagePack map | keys sorted lexicographically |
| Logical map with any other key | array of `[key, value]` | sorted by the key's complete canonical MessagePack bytes (FCMP §4.4) |
| Logical set | array of elements | sorted by each element's complete canonical bytes (FCMP §4.5) |
| Hash-map / hash-set flattening already stored as a vec on the postcard wire | same as logical map or set | sorted as above; compiler hash order is never wire order |

Duplicate canonical set elements or logical-map keys are `duplicate_key`.

Non-negative arena ids encode as unsigned integers. Their shortest
MessagePack forms are monotonically ordered by numeric value, so sorting
a logical map of `DefId` / `TypeId` / `Symbol` keys is numeric order.

### 1.6 Tag spelling

Schema identifiers (enum tags) are the FS-1 variant identifier rewritten:

1. Emit lowercase ASCII.
2. Insert `_` before each internal capital run (`TypeAlias` → `type_alias`,
   `GlobalInvocationX` → `global_invocation_x`, `SolumIn` → `solum_in`).
3. Keep a trailing capital run attached to the preceding digits or single
   letter only when the FS-1 name is a width or coordinate (`I8` → `i8`,
   `F16` → `f16`, `X` in `WorkgroupSizeX` → `_x`).
4. Do not translate Latin keywords (`Functio` → `functio`, not
   `function`). The `HirItemKind::Function` tag is `function` because that
   is the FS-1 variant name, not the Latin keyword.

Field names are the FS-1 serde names unchanged.

---

## 2. Shared encodings (ids, spans, closed vocabularies)

These appear in many families. They are value facts, never inferred from
the surrounding MessagePack opcode.

### 2.1 Arena and identity integers

| Schema type | Wire | Meaning |
| --- | --- | --- |
| `Symbol` | unsigned integer (`u32`) | index into `interner` |
| `TypeId` | unsigned integer (`u32`) | index into `types.types` |
| `IndexId` | unsigned integer (`u32`) | index into `types.indices` |
| `IndexVar` | unsigned integer (`u32`) | stored `Infer` slot; `next_index_var` is the allocator |
| `InferVar` | unsigned integer (`u32`) | type-inference variable |
| `HirId` | unsigned integer (`u32`) | node identity; **not** a dense arena |
| `DefId` | unsigned integer (`u32`) | definition identity |
| `HirSourceAnchorId` | unsigned integer (`u32`) | presentation owner identity |

A decoder MUST reject an overlong integer representation as
`noncanonical` and a value above `u32::MAX` as `type`. It MUST NOT treat
a MessagePack string, float, or array as an id.

Rehydration checks (fail-closed, class `invariant`, after structural
decode) are [§18](#18-arena-rehydration-encodings).

### 2.2 `Span`

REQUIRED record.

| Field | Presence | Wire |
| --- | --- | --- |
| `end` | REQUIRED | `u32` exclusive byte offset |
| `start` | REQUIRED | `u32` inclusive byte offset |

Invariant: `start <= end`. The package does not carry source bytes;
offsets still travel.

### 2.3 `NumericWidth`

Closed unit enum. **Never** a MessagePack integer-width opcode.

| FS-1 | Tag |
| --- | --- |
| `I8` `I16` `I32` `I64` | `i8` `i16` `i32` `i64` |
| `U8` `U16` `U32` `U64` | `u8` `u16` `u32` `u64` |
| `F16` `F32` `F64` | `f16` `f32` `f64` |

Example: `{ "tag": "i32" }`. Encoding `numerus<i32>` as MessagePack int32
is `type`.

### 2.4 `Primitive`

Closed unit enum. Tags: `ascii`, `textus`, `numerus`, `fractus`,
`bivalens`, `nihil`, `vacuum`, `numquam`, `ignotum`, `octeti`, `regex`,
`json`, `valor`, `instans`.

### 2.5 `InstansPrecision`

Closed unit enum. Tags: `millis`, `micros`, `nanos`.

### 2.6 `SemanticMutability`

Closed unit enum. Tags: `immutable`, `mutable`.

### 2.7 `SemanticParamMode`

Closed unit enum. Tags: `owned`, `ref`, `mut_ref`, `move`.

### 2.8 `Visibility`

Closed unit enum. Tags: `privata`, `publica`.

### 2.9 `CallablePosture`

Closed unit enum. Tags: `sync_finite`, `async_finite`, `sync_stream`,
`async_stream`.

### 2.10 `HirParamMode`

Closed unit enum. Tags: `owned`, `de`, `in`, `ex`.

### 2.11 `GenericParamKind`

Closed unit enum. Tags: `typus`, `magnitudo`.

### 2.12 `NominalKind`

Closed unit enum. Tags: `struct`, `enum`, `interface`.

### 2.13 `HirBinOp` / `HirUnOp` / `HirRefKind` / `HirRangeKind` / `HirIteraMode` / `HirScribeKind` / `HirIncDecOp` / `HirBreakableKind`

All closed unit enums.

**`HirBinOp`**: `add`, `sub`, `mul`, `div`, `mod`, `dot`, `cross`,
`tensor_product`, `hadamard`, `eq`, `not_eq`, `approx_eq`,
`approx_not_eq`, `strict_eq`, `strict_not_eq`, `lt`, `gt`, `lt_eq`,
`gt_eq`, `and`, `or`, `coalesce`, `bit_and`, `bit_or`, `bit_xor`, `shl`,
`shr`, `is`, `is_not`, `in_range`, `between`.

**`HirUnOp`**: `neg`, `not`, `bit_not`.

**`HirRefKind`**: `shared`, `mutable`.

**`HirRangeKind`**: `exclusive`, `inclusive`.

**`HirIteraMode`**: `ex`, `de`, `ab`.

**`HirScribeKind`**: `nota`, `vide`, `mone`, `scribe`.

**`HirIncDecOp`**: `inc`, `dec`.

**`HirBreakableKind`**: `fac` only.

### 2.14 `TypeParamConstraint` / `HirTypeParamConstraint`

| Variant | Tag | `value` |
| --- | --- | --- |
| `Any` | `any` | absent |
| `OneOf([TypeId])` | `one_of` | array of `TypeId` (tree / listed order) |

---

## 3. Family 1 — unit envelope (`schema_version` + `HirArtifact` record)

The `fhir.unit` `value` is this record. Nested under a package module it
is the same record (no inner frame).

Emission order (lexicographic keys):

| Field | Presence | Wire |
| --- | --- | --- |
| `analysis_stamp` | OPTIONAL default `0` | `u64` |
| `cli_program` | REQUIRED nullable | `nil` or `CliProgramWire` (§11) |
| `function_facts` | REQUIRED | `FunctionFactTableWire` (§13) |
| `gpu_builtins` | OPTIONAL default `[]` | logical map `DefId → GpuBuiltinWire` (§15) |
| `hir` | REQUIRED | `HirModule` (§5) |
| `interner` | REQUIRED | array of strings (§9) |
| `libraries` | REQUIRED | `LibraryRegistry` (§12) |
| `presentation` | REQUIRED | `HirPresentation` (§10) |
| `resolved_uses` | OPTIONAL default `[]` | array of `ResolvedUseWire` (§14) |
| `resolver` | REQUIRED | `ResolverSnapshot` (§8) |
| `schema_version` | REQUIRED | `u32`; document invariant: must equal `3` |
| `source_identity` | REQUIRED | `SourceIdentity` (§4) |
| `types` | REQUIRED | `TypeTableSnapshot` (§7) |

`schema_version ≠ 3` is `invariant` (`schema_version`).

---

## 4. Family 2 — `source_identity`

| Field | Presence | Wire |
| --- | --- | --- |
| `content_hash` | REQUIRED | string (SHA-256 hex of original source bytes) |
| `relative_path` | REQUIRED | string (project- or package-root relative) |

No Unicode normalization. No absolute build-host path.

---

## 5. Family 3 — `hir` (`HirModule` and node kinds)

### 5.1 `HirModule`

| Field | Presence | Wire |
| --- | --- | --- |
| `entry` | REQUIRED nullable | `nil` or `HirBlock` |
| `entry_annotations` | OPTIONAL default `[]` | array of `HirAnnotation` |
| `entry_args_name` | REQUIRED nullable | `nil` or `Symbol` |
| `entry_is_async` | OPTIONAL default `false` | bool |
| `items` | OPTIONAL default `[]` | array of `HirItem` (tree order) |

### 5.2 `HirItem`

| Field | Presence | Wire |
| --- | --- | --- |
| `def_id` | REQUIRED | `DefId` |
| `id` | REQUIRED | `HirId` |
| `kind` | REQUIRED | `HirItemKind` |
| `span` | REQUIRED | `Span` |

### 5.3 `HirItemKind`

| Variant | Tag | `value` |
| --- | --- | --- |
| `Function` | `function` | `HirFunction` |
| `Struct` | `struct` | `HirStruct` |
| `Enum` | `enum` | `HirEnum` |
| `Interface` | `interface` | `HirInterface` |
| `TypeAlias` | `type_alias` | `HirTypeAlias` |
| `Schema` | `schema` | `HirSchema` |
| `Constant` | `constant` | `HirConst` |
| `Import` | `import` | `HirImport` |

### 5.4 Declaration records

**`HirFunction`**

| Field | Presence | Wire |
| --- | --- | --- |
| `annotations` | OPTIONAL default `[]` | `[HirAnnotation]` |
| `body` | REQUIRED nullable | `nil` or `HirBlock` |
| `cli_args` | REQUIRED nullable | `nil` or `HirParam` |
| `err_ty` | REQUIRED nullable | `nil` or `TypeId` |
| `is_async` | OPTIONAL default `false` | bool |
| `is_fragment` | OPTIONAL default `false` | bool |
| `is_generator` | OPTIONAL default `false` | bool |
| `is_kernel` | OPTIONAL default `false` | bool |
| `is_vertex` | OPTIONAL default `false` | bool |
| `name` | REQUIRED | `Symbol` |
| `nucleum_fragment` | OPTIONAL default `false` | bool |
| `params` | OPTIONAL default `[]` | `[HirParam]` |
| `posture` | REQUIRED | `CallablePosture` |
| `ret_ty` | REQUIRED nullable | `nil` or `TypeId` |
| `test` | REQUIRED nullable | `nil` or `HirTestMetadata` |
| `type_params` | OPTIONAL default `[]` | `[HirTypeParam]` |

`NucleumRole` is not a wire field (FS-1).

**`HirTestMetadata`**: `modifiers` OPTIONAL default `[]`; `name` REQUIRED
`Symbol`; `span` REQUIRED; `suite_path` OPTIONAL default `[]` of `Symbol`.

**`HirTestModifier`**

| Variant | Tag | `value` |
| --- | --- | --- |
| `Omitte(Symbol)` | `omitte` | `Symbol` |
| `Futurum(Symbol)` | `futurum` | `Symbol` |
| `Solum` | `solum` | absent |
| `Tag(Symbol)` | `tag` | `Symbol` |
| `Temporis(i64)` | `temporis` | `i64` |
| `Metior` | `metior` | absent |
| `Repete(i64)` | `repete` | `i64` |
| `Fragilis(i64)` | `fragilis` | `i64` |
| `SolumIn(Symbol)` | `solum_in` | `Symbol` |

Retired `Requirit` is not a tag (unit schema 3).

**`HirTypeParam`**: `constraint` REQUIRED; `def_id` REQUIRED; `kind`
REQUIRED; `name` REQUIRED `Symbol`; `span` REQUIRED.

**`HirParam`**: `default` REQUIRED nullable (`nil` or `HirExpression`);
`def_id` REQUIRED; `mode` REQUIRED; `name` REQUIRED; `optional` OPTIONAL
default `false`; `span` REQUIRED; `sponte` OPTIONAL default `false`;
`ty` REQUIRED `TypeId`.

**`HirStruct`**: `annotations` OPTIONAL `[]`; `extends` REQUIRED nullable
`DefId`; `fields` OPTIONAL `[]`; `implements` OPTIONAL `[]` of `DefId`
(tree order); `methods` OPTIONAL `[]`; `name` REQUIRED; `type_params`
OPTIONAL `[]`.

**`HirField`**: `annotations` OPTIONAL `[]`; `def_id` REQUIRED; `init`
REQUIRED nullable; `is_static` OPTIONAL `false`; `name` REQUIRED; `span`
REQUIRED; `sponte` OPTIONAL `false`; `ty` REQUIRED `TypeId`.

**`HirMethod`**: `def_id` REQUIRED; `func` REQUIRED `HirFunction`;
`receiver` REQUIRED `HirReceiver`; `span` REQUIRED.

**`HirReceiver`**: unit tags `none`, `ref`, `mut_ref`, `owned`.

**`HirEnum`**: `annotations` OPTIONAL `[]`; `name` REQUIRED;
`type_params` OPTIONAL `[]`; `variants` OPTIONAL `[]`.

**`HirVariant`**: `def_id` REQUIRED; `fields` OPTIONAL `[]`; `name`
REQUIRED; `span` REQUIRED.

**`HirVariantField`**: `name` REQUIRED; `span` REQUIRED; `ty` REQUIRED
`TypeId`.

**`HirInterface`**: `methods` OPTIONAL `[]`; `name` REQUIRED;
`type_params` OPTIONAL `[]`.

**`HirInterfaceMethod`**: `availability` OPTIONAL `[]`; `err_ty` REQUIRED
nullable; `name` REQUIRED; `params` OPTIONAL `[]`; `posture` REQUIRED;
`ret_ty` REQUIRED nullable; `span` REQUIRED.

**`HirAvailability`**: only `Nondum`. Tag `nondum`. `value` record:
`reason` REQUIRED nullable `Symbol`; `target` REQUIRED nullable `Symbol`.

**`HirTypeAlias`**: `name` REQUIRED; `ty` REQUIRED `TypeId`;
`type_params` OPTIONAL `[]`.

**`HirSchema`**: `columns` OPTIONAL `[]`; `name` REQUIRED.

**`HirSchemaColumn`**: `name` REQUIRED; `nullable` OPTIONAL `false`;
`order` REQUIRED `u32`; `source` REQUIRED `Symbol`; `ty` REQUIRED
`TypeId`.

**`HirConst`**: `is_await` OPTIONAL `false`; `mutable` OPTIONAL `false`;
`name` REQUIRED; `ty` REQUIRED nullable; `value` REQUIRED
`HirExpression`.

**`HirImport`**: `items` OPTIONAL `[]`; `path` REQUIRED `Symbol`;
`visibility` REQUIRED.

**`HirImportItem`**: `alias` REQUIRED nullable `Symbol`; `def_id`
REQUIRED; `name` REQUIRED.

### 5.5 `HirAnnotation` / `HirAnnotationValue` / `Token`

**`HirAnnotation`**: `contract_id` REQUIRED nullable `DefId`; `family`
REQUIRED `Symbol`; `fields` OPTIONAL `[]`; `raw_args` OPTIONAL `[]` of
`Token`; `span` REQUIRED.

**`HirAnnotationField`**: `name` REQUIRED `Symbol`; `span` REQUIRED;
`value` REQUIRED `HirAnnotationValue`.

**`HirAnnotationValue`**

| Variant | Tag | `value` |
| --- | --- | --- |
| `Expr` | `expr` | `HirExpression` |
| `Type` | `type` | `TypeId` |
| `Symbol` | `symbol` | `Symbol` |
| `String` | `string` | `Symbol` |
| `Bool` | `bool` | bool (always present; not omit-default) |
| `Int` | `int` | `i64` |
| `Float` | `float` | float64 |
| `Nihil` | `nihil` | absent |

**`Token`**: `kind` REQUIRED `TokenKind`; `span` REQUIRED.

**`TokenKind`** is the closed `radix_lexer::TokenKind` vocabulary. Do not
invent a reduced DTO. Payload-bearing variants:

| Variant | Tag | `value` |
| --- | --- | --- |
| `Ident` | `ident` | `Symbol` |
| `Underscore` | `underscore` | `Symbol` |
| `Integer` | `integer` | `u64` |
| `Float` | `float` | float64 |
| `String` | `string` | `Symbol` |
| `AsciiString` | `ascii_string` | `Symbol` |
| `BacktickString` | `backtick_string` | `Symbol` |
| `OctetiString` | `octeti_string` | `Symbol` |
| `LineComment` | `line_comment` | `Symbol` |

`LineComment` is payload-bearing on the live enum. FS-1 abbreviated the
payload list; this mapping does not drop the `Symbol`.

Every other `TokenKind` variant is a unit tag. The complete tag list is
[Appendix A](#appendix-a--tokenkind-unit-tags).

### 5.6 `HirBlock` / statements

**`HirBlock`**

| Field | Presence | Wire |
| --- | --- | --- |
| `breakable` | REQUIRED nullable | `nil` or `HirBreakableKind` |
| `end` | REQUIRED nullable | `nil` or `HirBlockEnd` |
| `expr` | REQUIRED nullable | `nil` or `HirExpression` |
| `span` | REQUIRED | `Span` |
| `statements` | OPTIONAL default `[]` | `[HirStatement]` |

**`HirStatement`**: `id` REQUIRED `HirId`; `kind` REQUIRED
`HirStatementKind`; `span` REQUIRED.

**`HirStatementKind`**

| Variant | Tag | `value` |
| --- | --- | --- |
| `Local` | `local` | `HirLocal` |
| `Expr` | `expr` | `HirExpression` |
| `Redde` | `redde` | `nil` or `HirExpression` (nullable payload) |
| `Rumpe` | `rumpe` | absent |
| `Perge` | `perge` | absent |
| `Tacet` | `tacet` | absent |
| `IncDec` | `inc_dec` | `HirIncDec` |
| `Custodi` | `custodi` | `HirCustodi` |

`Redde`'s payload is a nullable expression: `value` is present and is
either `nil` or an expression record.

**`HirLocal`**: `def_id` REQUIRED; `init` REQUIRED nullable; `is_await`
OPTIONAL `false`; `mutable` OPTIONAL `false`; `name` REQUIRED;
`runtime_provided` OPTIONAL `false`; `ty` REQUIRED nullable.

**`HirIncDec`**: `op` REQUIRED; `target` REQUIRED `HirExpression`.

**`HirCustodi`**: `clauses` OPTIONAL `[]` of `HirCustodiClause`.

**`HirCustodiClause`**: `body` REQUIRED `HirBlock`; `cond` REQUIRED
`HirExpression`; `span` REQUIRED.

### 5.7 `HirExpression`

| Field | Presence | Wire |
| --- | --- | --- |
| `id` | REQUIRED | `HirId` |
| `kind` | REQUIRED | `HirExpressionKind` |
| `span` | REQUIRED | `Span` |
| `ty` | REQUIRED nullable | `nil` or `TypeId` |

**`HirExpressionKind`** (closed). Tuple payloads from FS-1 become named
records. Field lists are in emission (lex) order.

| Variant | Tag | `value` |
| --- | --- | --- |
| `Path` | `path` | `DefId` |
| `Literal` | `literal` | `HirLiteral` |
| `Vacua` | `vacua` | absent |
| `Binary` | `binary` | `{ lhs, op, rhs }` |
| `Unary` | `unary` | `{ expr, op }` |
| `Call` | `call` | `{ args, callee, type_args }` — `type_args` OPTIONAL `[]` of `TypeId`; `args` OPTIONAL `[]` |
| `Gradient` | `gradient` | `{ call, selected_arguments }` — `selected_arguments` OPTIONAL `[]` |
| `MethodCall` | `method_call` | `{ args, method, receiver, type_args }` — `args` / `type_args` OPTIONAL `[]` |
| `Field` | `field` | `{ name, receiver }` — `name` is `Symbol` |
| `Index` | `index` | `{ base, index }` |
| `OptionalChain` | `optional_chain` | `{ kind, receiver }` |
| `NonNull` | `non_null` | `{ kind, receiver }` |
| `Block` | `block` | `HirBlock` |
| `Si` | `si` | `{ cond, else_block, then_block, then_catch }` — `else_block` / `then_catch` REQUIRED nullable |
| `Discerne` | `discerne` | `{ arms, exhaustive, scrutinees }` — `arms` / `scrutinees` OPTIONAL `[]`; `exhaustive` OPTIONAL `false` |
| `Loop` | `loop` | `HirBlock` |
| `Dum` | `dum` | `{ body, cond }` |
| `Itera` | `itera` | `{ body, iter, mode, name, name_def }` — `name` `Symbol`; `name_def` `DefId` |
| `Intervallum` | `intervallum` | `{ end, kind, start, step }` — `step` REQUIRED nullable |
| `Assign` | `assign` | `{ lhs, rhs }` |
| `ConversioAssign` | `conversio_assign` | `{ recovery, source, target }` — `recovery` REQUIRED nullable |
| `Array` | `array` | array of `HirArrayElement` (OPTIONAL default `[]` as the payload array; an empty payload array is a legal payload and MUST be present as `[]` because it is the variant `value`, not an OPTIONAL record field) |
| `Struct` | `struct` | `{ fields, struct_def }` — `fields` OPTIONAL `[]` of `{ expr, name }`; `struct_def` `DefId` |
| `Tuple` | `tuple` | `{ elems, tys }` — `elems` OPTIONAL `[]`; `tys` REQUIRED nullable array of `TypeId` |
| `Scribe` | `scribe` | `{ args, kind }` — `args` OPTIONAL `[]` |
| `Scriptum` | `scriptum` | `{ args, template }` — `template` `Symbol`; `args` OPTIONAL `[]` |
| `ReadLine` | `read_line` | bool (always present) |
| `Praefixum` | `praefixum` | `HirExpression` |
| `Adfirma` | `adfirma` | `{ cond, message }` — `message` REQUIRED nullable |
| `Panic` | `panic` | `HirExpression` |
| `Throw` | `throw` | `HirExpression` |
| `Handled` | `handled` | `{ body, catch }` |
| `Clausura` | `clausura` | `{ body, err_ty, params, ret_ty }` — `err_ty` / `ret_ty` REQUIRED nullable; `params` OPTIONAL `[]` |
| `Cede` | `cede` | `HirExpression` |
| `Reddet` | `reddet` | `HirExpression` |
| `Tacebit` | `tacebit` | `HirExpression` |
| `Yield` | `yield` | `HirExpression` |
| `Verte` | `verte` | `{ entries, source, target }` — `entries` REQUIRED nullable array of `HirObjectField`; `target` `TypeId` |
| `Conversio` | `conversio` | `{ params, recovery, source, target }` — `params` OPTIONAL `[]` of `Symbol`; `recovery` REQUIRED nullable; `target` is `HirConversioTarget` |
| `Ad` | `ad` | `{ opener, route }` — `opener` REQUIRED nullable `HirExpression`; `route` `Symbol` |
| `Ref` | `ref` | `{ expr, kind }` |
| `Deref` | `deref` | `HirExpression` |
| `TypeCheck` | `type_check` | `{ expr, positive, ty }` — `positive` OPTIONAL `false`; `ty` `TypeId` |
| `Error` | `error` | absent |

Empty `array` / `tuple.elems` / `call.args` still need a present `value`
when they are the variant payload or a REQUIRED field. OPTIONAL applies
only to **record fields** named above, not to a unit-vs-payload choice.

**`HirCallArg`**: `expr` REQUIRED; `name` REQUIRED nullable `Symbol`;
`span` REQUIRED; `spread` OPTIONAL `false`.

**`HirCape`**: `binding_def_id` REQUIRED; `binding_name` REQUIRED;
`binding_ty` REQUIRED nullable; `body` REQUIRED `HirBlock`; `span`
REQUIRED.

**`HirArrayElement`**: `Expr` tag `expr`, `Spread` tag `spread`; each
`value` is `HirExpression`.

**`HirObjectField`**: `key` REQUIRED `HirObjectKey`; `value` REQUIRED
nullable `HirExpression`.

**`HirObjectKey`**: `Ident` / `String` / `Computed` / `Spread` → tags
`ident`, `string`, `computed`, `spread`; payload `Symbol` or
`HirExpression`.

**`HirOptionalChainKind` / `HirNonNullKind`**: `Member(Symbol)` tag
`member`; `Index(HirExpression)` tag `index`; `Call([HirCallArg])` tag
`call` (payload array; present even when empty).

**`HirConversioTarget`**: `Type(TypeId)` tag `type`; `Intervallum(HirExpression)` tag `intervallum`.

**`HirCasuArm`**: `body` REQUIRED `HirExpression`; `guard` REQUIRED
nullable; `patterns` OPTIONAL `[]` of `HirPattern`; `span` REQUIRED.

**`HirPattern`**

| Variant | Tag | `value` |
| --- | --- | --- |
| `Wildcard` | `wildcard` | absent |
| `Binding` | `binding` | `{ def_id, name }` |
| `Alias` | `alias` | `{ def_id, inner, name }` — `inner` is `HirPattern` |
| `Variant` | `variant` | `{ def_id, fields }` — `fields` OPTIONAL `[]` of `HirPattern` |
| `Literal` | `literal` | `HirLiteral` |

### 5.8 `HirLiteral` / `JsonValue`

| Variant | Tag | `value` | Notes |
| --- | --- | --- | --- |
| `Int` | `int` | `u64` | Magnitude. Negation is `unary` + `neg`. |
| `Float` | `float` | float64 | Literal `fractus` spelling. **Not** a tensor dtype. |
| `String` | `string` | `Symbol` | Interner, raw. |
| `Ascii` | `ascii` | `Symbol` | Interner, raw. |
| `Octeti` | `octeti` | `Symbol` | Interner string is the hex payload without `\|…\|`. Exact byte sequence via the interner, not an array of floats. |
| `JsonValor` | `json_valor` | `JsonValue` | |
| `Regex` | `regex` | `{ flags, pattern }` — `flags` REQUIRED nullable `Symbol`; `pattern` REQUIRED `Symbol` | |
| `Bool` | `bool` | bool (always present) | |
| `Nil` | `nil` | absent | Unit tag. Not MessagePack `nil`. |

There is no tensor-constant blob variant. Tensor **values** are ordinary
expressions; tensor **type** facts are `Type::Tensor` in the type table.

**`JsonValue`**

| Variant | Tag | `value` |
| --- | --- | --- |
| `Object` | `object` | array of `JsonMember` (source order) |
| `Array` | `array` | array of `JsonValue` |
| `String` | `string` | `Symbol` |
| `Numerus` | `numerus` | `i64` |
| `Fractus` | `fractus` | float64 |
| `Bool` | `bool` | bool |
| `Null` | `null` | absent |

JSON `null` is the unit tag `null`, never MessagePack `nil`.

**`JsonMember`**: `key` REQUIRED `Symbol`; `key_span` REQUIRED; `value`
REQUIRED `JsonValue`.

---

## 6. Family 4 — `presentation` (see §10)

Presentation is family 4 of the unit envelope (`HirArtifact.presentation`).
Its record encoding is written next to the other sidecar families in
[§10](#10-family-4--presentation--hirpresentation) so node kinds stay
contiguous. The family number is the FS-1 envelope slot, not the section
number.

---

## 7. Family 6 — `types` (`TypeTableSnapshot`)

Value facts live here: widths, tensor dtype/shape, buffer types.

| Field | Presence | Wire |
| --- | --- | --- |
| `indices` | REQUIRED | array of `IndexExpr` (index identity order) |
| `next_index_var` | OPTIONAL default `0` | `u32` |
| `types` | REQUIRED | array of `Type` (index identity order) |
| `unspecified_shape` | REQUIRED nullable | `nil` or `IndexId` |

`intern_map`, `primitives`, memos, and `first_modular_word` are not
fields (FS-1; rebuilt on load).

Required primitives must appear as `Type::Primitive` entries. Missing any
of `textus`, `numerus`, `fractus`, `bivalens`, `nihil`, `vacuum`,
`numquam`, `ignotum`, `octeti`, `regex`, `json`, `valor`, `ascii`,
`instans` is `invariant` (`required_primitive`).

### 7.1 `Type`

| Variant | Tag | `value` |
| --- | --- | --- |
| `Primitive` | `primitive` | `Primitive` |
| `Array` | `array` | `TypeId` |
| `BoundedArray` | `bounded_array` | `{ capacity, element }` — both REQUIRED; `capacity` `IndexId`; `element` `TypeId` |
| `BoundedTextus` | `bounded_textus` | `{ capacity }` `IndexId` |
| `BoundedAscii` | `bounded_ascii` | `{ capacity }` `IndexId` |
| `BoundedOcteti` | `bounded_octeti` | `{ capacity }` `IndexId` |
| `Map` | `map` | `{ key, value }` — both `TypeId` |
| `Record` | `record` | logical map `Symbol → TypeId` (array of `[symbol, type_id]`, sorted by `Symbol`) |
| `Set` | `set` | `TypeId` |
| `Promissum` | `promissum` | `TypeId` |
| `PromissumFailable` | `promissum_failable` | `{ err, ok }` — `ok` is success `T`; `err` is alternate `E` |
| `Cursor` | `cursor` | `TypeId` |
| `AsyncCursor` | `async_cursor` | `{ err, item }` |
| `Tensor` | `tensor` | `{ elem, shape }` — `elem` `TypeId` (dtype); `shape` `IndexId` |
| `Vector` | `vector` | `{ elem, shape }` |
| `Matrix` | `matrix` | `{ elem, shape }` |
| `Sparsa` | `sparsa` | `{ elem, shape }` |
| `Atomic` | `atomic` | `TypeId` |
| `Intervallum` | `intervallum` | `TypeId` |
| `SizedNumeric` | `sized_numeric` | `{ primitive, width }` — `primitive` is `numerus` or `fractus`; `width` is `NumericWidth` |
| `ModularWord` | `modular_word` | `NumericWidth` |
| `SizedInstans` | `sized_instans` | `InstansPrecision` |
| `Option` | `option` | `TypeId` |
| `Ref` | `ref` | `{ mutability, ty }` |
| `Struct` | `struct` | `DefId` |
| `Enum` | `enum` | `DefId` |
| `Interface` | `interface` | `DefId` |
| `Alias` | `alias` | `{ def_id, ty }` |
| `Func` | `func` | `FuncSig` |
| `Param` | `param` | `Symbol` |
| `Applied` | `applied` | `{ args, base }` — `args` OPTIONAL `[]` of `TypeId` |
| `Infer` | `infer` | `InferVar` |
| `InferUnion` | `infer_union` | `InferVar` |
| `Union` | `union` | array of `TypeId` (listed order) |
| `Tuple` | `tuple` | array of `TypeId` (position order) |
| `Error` | `error` | absent |

A decoder MUST reject a `tensor` / `vector` / `matrix` / `sparsa` /
`sized_numeric` encoded as a MessagePack array of numbers or as a bare
int/float opcode (`type`). See [§19](#19-widths-dtypeshape--type-table-value-facts).

**`FuncSig`**

| Field | Presence | Wire |
| --- | --- | --- |
| `err` | REQUIRED nullable | `nil` or `TypeId` |
| `is_async` | OPTIONAL default `false` | bool |
| `is_generator` | OPTIONAL default `false` | bool |
| `params` | OPTIONAL default `[]` | `[ParamType]` |
| `ret` | REQUIRED | `TypeId` |
| `type_param_constraints` | OPTIONAL default `[]` | `[TypeParamConstraint]` |
| `type_params` | OPTIONAL default `[]` | `[Symbol]` |

**`ParamType`**: `mode` REQUIRED; `optional` OPTIONAL `false`; `ty`
REQUIRED `TypeId`.

### 7.2 `IndexExpr`

| Variant | Tag | `value` |
| --- | --- | --- |
| `Literal` | `literal` | `u64` |
| `Param` | `param` | `Symbol` (`magnitudo N`) |
| `Tuple` | `tuple` | array of `IndexId` (rank-n; empty array is rank-0; payload MUST be present) |
| `Infer` | `infer` | `IndexVar` |
| `Unspecified` | `unspecified` | absent |

Shape is this arena, not a MessagePack array of generic numbers.

---

## 8. Family 8 — `resolver` (`ResolverSnapshot`)

| Field | Presence | Wire |
| --- | --- | --- |
| `ambiguous_imported_nominal_types` | OPTIONAL default `[]` | logical set of `{ kind, name }` — `kind` `NominalKind`; `name` `Symbol`; sorted by canonical key bytes |
| `file_interfaces` | OPTIONAL default `[]` | logical map `Symbol → FileInterfaceSnapshot` |
| `imported_nominal_types` | OPTIONAL default `[]` | logical map `(NominalKind, Symbol) → TypeId`. Key is the two-element array `[{ "tag": … }, symbol]`. |
| `namespace_exports` | OPTIONAL default `[]` | logical map `Symbol →` array of `Symbol` (member names sorted as a logical set) |
| `used_namespaces` | OPTIONAL default `[]` | logical set of `Symbol` |

**`FileInterfaceSnapshot`**: `exports` OPTIONAL default `[]` — logical
map `Symbol → FileExportSnapshot`.

**`FileExportSnapshot`**

| Variant | Tag | `value` |
| --- | --- | --- |
| `Function` | `function` | `FuncSig` |
| `Type` | `type` | `{ def_id, ty }` — `def_id` REQUIRED nullable; `ty` REQUIRED `TypeId` |
| `Struct` | `struct` | `{ def_id, fields, ty }` — `fields` OPTIONAL `[]` of `StructFieldSnapshot` |

No `enum` arm (FS-1 §14.3). Do not add one.

**`StructFieldSnapshot`**: `name` REQUIRED; `optional` OPTIONAL `false`;
`required` OPTIONAL `false`; `ty` REQUIRED `TypeId`.

---

## 9. Family 7 — `interner`

REQUIRED array of strings in **symbol-id order**. `Symbol(n)` is the
string at index `n`.

- MUST NOT sort. Order is identity.
- Lookup `FxHashMap` is not a field. Load rebuilds it by raw-interning
  each entry in order (`Interner::from_string_table`).
- Identifier strings are already NFKC. Literal, comment, regex, and
  octeti-hex payloads are raw. A decoder MUST NOT re-normalize.

An index `n >= interner.len()` used as a `Symbol` is `invariant`
(`symbol_bound`).

---

## 10. Family 4 — `presentation` (`HirPresentation`)

| Field | Presence | Wire |
| --- | --- | --- |
| `attachments` | OPTIONAL default `[]` | `[HirTriviaAttachment]` (owner order as stored) |
| `block_ends` | OPTIONAL default `[]` | `[HirBlockEnd]` |
| `entry_start_anchor` | REQUIRED nullable | `nil` or `HirSourceAnchorId` |
| `owners` | OPTIONAL default `[]` | `[HirTriviaOwner]` |
| `program_end` | REQUIRED | `HirModuleEnd` |

Do not invent extra owner kinds.

**`HirTriviaOwner`**: `id` REQUIRED `HirSourceAnchorId`; `kind` REQUIRED
`HirTriviaOwnerKind`; `semantic` REQUIRED nullable `HirOwnerRef`.

**`HirTriviaOwnerKind`**: unit tags `item`, `statement`, `member`,
`entry`, `block_end`, `program_end`.

**`HirOwnerRef`**: `Hir(HirId)` tag `hir`; `Def(DefId)` tag `def`.

**`HirTriviaAttachment`**: `anchor` REQUIRED; `leading` OPTIONAL `[]` of
`HirTrivia`.

**`HirTrivia`**

| Variant | Tag | `value` |
| --- | --- | --- |
| `CommentLine` | `comment_line` | `{ span, text }` — `text` is `Symbol` (verbatim `# …` payload) |
| `Newline` | `newline` | `{ span }` |

**`HirBlockEnd` / `HirModuleEnd`**: `id` REQUIRED `HirSourceAnchorId`;
`span` REQUIRED.

Presentation integrity (class `invariant`) matches live
`validate_presentation`: unique owner anchors; at most one attachment per
owner; every attachment names a registered owner; comment `Symbol`s
resolve; semantic back-refs resolve; `entry_start_anchor` names a
registered owner; structural ends are anchor-consistent.

---

## 11. Family 5 — `cli_program` (`CliProgramWire`)

Present on the unit as REQUIRED nullable (family 5). Diagnostic-only
spans and `binding_symbol` stay excluded (FS-1 F18). They are not added
here.

| Field | Presence | Wire |
| --- | --- | --- |
| `commands` | OPTIONAL default `[]` | `[CliCommandWire]` |
| `description` | REQUIRED nullable | `nil` or string |
| `entry_args` | REQUIRED | string |
| `exit` | REQUIRED nullable | `nil` or `CliExitWire` |
| `global_operands` | OPTIONAL default `[]` | `[CliOperandWire]` |
| `global_options` | OPTIONAL default `[]` | `[CliOptionWire]` |
| `mode` | REQUIRED | `CliModeWire` |
| `name` | REQUIRED | string |
| `operands` | OPTIONAL default `[]` | `[CliOperandWire]` |
| `options` | OPTIONAL default `[]` | `[CliOptionWire]` |
| `version` | REQUIRED nullable | `nil` or string |

**`CliModeWire`**: unit tags `not_cli`, `single_command`, `subcommand`.

**`CliExitWire`**: `Fixed(i64)` tag `fixed`; `Binding(string)` tag
`binding`; `Field { object, field }` tag `field` with value
`{ field, object }` (both strings); `Unsupported` tag `unsupported`.

**`CliOptionWire`**: `binding` REQUIRED string; `default` REQUIRED
nullable `CliDefaultWire`; `description` REQUIRED nullable string;
`flag` OPTIONAL `false`; `global` OPTIONAL `false`; `long` REQUIRED
nullable string; `short` REQUIRED nullable string; `ty` REQUIRED
`CliTypeWire`.

**`CliOperandWire`**: `binding` REQUIRED; `default` REQUIRED nullable;
`description` REQUIRED nullable; `global` OPTIONAL `false`; `rest`
OPTIONAL `false`; `ty` REQUIRED.

**`CliCommandWire`**: `aliases` OPTIONAL `[]` of string; `args_binding`
REQUIRED nullable string; `description` REQUIRED nullable;
`function` REQUIRED string; `function_symbol` REQUIRED `Symbol`;
`module_path` REQUIRED nullable array of string; `operands` OPTIONAL
`[]`; `options` OPTIONAL `[]`; `path` OPTIONAL `[]` of string.

**`CliTypeWire`**: unit tags `textus`, `numerus`, `fractus`, `bivalens`,
`octeti`, `ignotum`, `lista_textus`, `lista_numerus`.

**`CliDefaultWire`**: `Text(string)` tag `text`; `Integer(i64)` tag
`integer`; `Float(f64)` tag `float` (float64); `Bool(bool)` tag `bool`;
`Nil` tag `nil` (unit; not MessagePack `nil`); `Expr(string)` tag `expr`.

---

## 12. Family 9 — `libraries` (`LibraryRegistry`)

| Field | Presence | Wire |
| --- | --- | --- |
| `bindings` | OPTIONAL default `[]` | logical map `DefId → LibraryBinding` |
| `exports` | OPTIONAL default `[]` | logical map `DefId →` logical set of string |
| `items` | OPTIONAL default `[]` | logical map `DefId → LibraryItem` |
| `reexports` | OPTIONAL default `[]` | logical map `(DefId, string) → string`. Key is the two-element array `[def_id, exported_name]`. |

**`LibraryBinding`**: `identity` REQUIRED; `local_def_id` REQUIRED;
`rust_runtime_methods` OPTIONAL default `[]` (string-key MessagePack
map, keys sorted); `rust_runtime_module` REQUIRED nullable string.

**`LibraryIdentity`**: `module_path` OPTIONAL `[]` of string;
`provider` REQUIRED `LibraryProvider`.

**`LibraryProvider`**: `Builtin(string)` tag `builtin`; `Package(string)`
tag `package`.

**`LibraryItem`**: `def_id` REQUIRED; `elide_rust_decl` OPTIONAL `false`;
`exported_name` REQUIRED string; `identity` REQUIRED; `is_async`
OPTIONAL `false`; `is_failable` OPTIONAL `false`; `kind` REQUIRED
`LibraryItemKind`; `rust_runtime_type` REQUIRED nullable string.

**`LibraryItemKind`**: unit tags `interface`, `function`, `type_alias`,
`struct`, `enum`, `const`.

---

## 13. Family 10 — `function_facts` (`FunctionFactTableWire`)

| Field | Presence | Wire |
| --- | --- | --- |
| `entries` | OPTIONAL default `[]` | logical map `DefId → FunctionFactWire` |
| `entry` | REQUIRED nullable | `nil` or `FunctionFactWire` (module entry facts) |

Postcard stores `entries` as a vec of pairs. This mapping sorts it as a
logical map.

**`FunctionFactWire`**

| Field | Presence | Wire |
| --- | --- | --- |
| `call_edges` | OPTIONAL default `[]` | logical set of `DefId` |
| `captures` | OPTIONAL default `[]` | `[CaptureFactWire]` sorted by `def_id` |
| `direct_failure` | OPTIONAL default `false` | bool |
| `err_ty` | REQUIRED nullable | `nil` or `TypeId` |
| `is_async` | OPTIONAL default `false` | bool |
| `is_generator` | OPTIONAL default `false` | bool |
| `may_fail` | OPTIONAL default `false` | bool |
| `param_modes` | OPTIONAL default `[]` | logical map `DefId → SemanticParamMode` |
| `requires_await` | OPTIONAL default `false` | bool |

**`CaptureFactWire`**: `def_id` REQUIRED; `mode` REQUIRED
`CaptureModeWire`.

**`CaptureModeWire`**: unit tags `read`, `mutate`, `consume`.

---

## 14. Family 11 — `resolved_uses`

OPTIONAL default `[]` on the unit. Array of `ResolvedUseWire`, sorted by
the canonical encoding of `key` then `kind` then `enclosing` (logical
set of records; postcard hash order is not wire order).

**`ResolvedUseWire`**: `enclosing` REQUIRED nullable `DefId`; `key`
REQUIRED; `kind` REQUIRED.

**`ResolvedUseKeyWire`**: `Local(DefId)` tag `local`; `Portable(string)`
tag `portable`.

**`ResolvedUseKindWire`**: unit tags `path`, `call`, `method_call`,
`type_ref`, `import_binding`.

---

## 15. Family 13 — `gpu_builtins`

OPTIONAL default `[]` on the unit. Logical map `DefId → GpuBuiltinWire`.

**`GpuBuiltinWire`** (closed unit enum; mirrors
`radix_types::MirKernelBuiltin`):

`global_invocation_x`, `global_invocation_y`, `global_invocation_z`,
`global_invocation_id`, `workgroup_id_x`, `workgroup_id_y`,
`workgroup_id_z`, `workgroup_id`, `local_invocation_id_x`,
`local_invocation_id_y`, `local_invocation_id_z`,
`local_invocation_id`, `workgroup_size_x`, `workgroup_size_y`,
`workgroup_size_z`.

The resolver's runtime `gpu_builtins` map is not a field (FS-1). This
artifact field is the durable copy.

---

## 16. Family 12 — `analysis_stamp`

OPTIONAL `u64` default `0` on the unit. Package build zeros it so
package bytes are content-deterministic. Not a semantic program fact.

---

## 17. Package envelope — families 14–19

The `fhir.package` `value` is this record.

| Field | Presence | Wire | Family |
| --- | --- | --- | --- |
| `dependencies` | OPTIONAL default `[]` | `[PackageDependencyWire]` sorted by `name` (already unique) | 19 |
| `entry_frontmatter` | REQUIRED nullable | `nil` or string (TOML text) | 17 |
| `entry_path` | REQUIRED | string (package-root relative) | 16 |
| `identity` | REQUIRED | `PackageIdentityWire` | 15 |
| `modules` | OPTIONAL default `[]` | `[FhirPackageModule]` sorted by `path` (unique) | 18 |
| `package_schema_version` | REQUIRED | `u32`; invariant: must equal `1` | 14 |

**`PackageIdentityWire`**: `edition` REQUIRED string; `name` REQUIRED
string; `version` REQUIRED string.

**`PackageDependencyWire`**: `checksum` REQUIRED nullable string;
`lock_identity` REQUIRED string; `name` REQUIRED string; `version`
REQUIRED string. Exact coordinates only.

`package_schema_version ≠ 1` is `invariant` (`package_schema_version`).
Duplicate `modules[].path` or `dependencies[].name` is `invariant`.

### 17.1 Family 18 — `modules` (`FhirPackageModule`)

| Field | Presence | Wire |
| --- | --- | --- |
| `export_names` | OPTIONAL default `[]` | logical set of string (already sorted public names) |
| `is_entry` | OPTIONAL default `false` | bool |
| `library_imports` | OPTIONAL default `[]` | `[LibraryImportWire]` sorted by `(binding, package, module)` |
| `local_links` | OPTIONAL default `[]` | `[LocalLinkWire]` sorted by `binding` |
| `module_segments` | OPTIONAL default `[]` | array of string (path identity; do not sort) |
| `path` | REQUIRED | string (package-root relative) |
| `source_hash` | REQUIRED | string; must equal nested `unit.source_identity.content_hash` |
| `unit` | REQUIRED | **nested `HirArtifact` record** (§3) |
| `unit_schema_version` | REQUIRED | `u32`; must equal `3` and `unit.schema_version` |

`unit` is the unit mapping, not opaque postcard bytes and not a nested
FCMP frame. A decoder that sees MessagePack binary here rejects `type`.
(Legacy postcard `.fhirpkg` is a different product byte stream and is
diagnosed `legacy` at the frame, never reinterpreted.)

`source_hash ≠ unit.source_identity.content_hash` is `invariant`
(`source_hash`). `unit_schema_version ≠ unit.schema_version` is
`invariant` (`unit_schema_version`).

### 17.2 Package import links (inside family 18)

**`LocalLinkWire`**: `binding` REQUIRED string; `target` REQUIRED string
(package-root-relative module path). Decode rejects a target missing from
the module table (`invariant` `dangling_module_ref`). Load never
re-derives local imports from the filesystem.

**`LibraryImportWire`**: `binding` REQUIRED string; `module` OPTIONAL
`[]` of string; `package` REQUIRED string. No `interface_path`. No
absolute checkout path.

The adapter rule that every `library_imports.package` appears in
`dependencies` stays on the product adapter, not this format mapping.

---

## 18. Arena rehydration encodings

All ids are **unit-local**. A package is a list of units, not one shared
arena. Cross-unit references travel as module path + export name (link
table + per-unit resolver snapshots).

These checks run after structural decode and before the artifact is
admitted. Failure class is `invariant`; the payload names the row below.
They match `radix_hir_fhir::decode` / `validate_referential_integrity`.
The driver reconstruction does not re-parse; it only rebuilds derived
caches.

| Id | Arena / membership | Check |
| --- | --- | --- |
| `Symbol` | `interner` array | `id < interner.len()` |
| `TypeId` | `types.types` array | `id < types.len()`; nested `TypeId` / `IndexId` inside each `Type` in bounds |
| `IndexId` | `types.indices` array | `id < indices.len()` |
| `IndexVar` | stored on `IndexExpr::Infer` | `next_index_var` restored as stored |
| `unspecified_shape` | when not `nil` | in bounds and names `IndexExpr::Unspecified` |
| `HirId` | nodes in `hir` | membership: the id appears on a `HirItem` / `HirStatement` / `HirExpression`. **Not** a `0..N` count bound. Lowering leaves gaps. |
| `DefId` | definition sites | builtins in `[1, USER_DEF_ID_BASE)` with `USER_DEF_ID_BASE = 0x0000_1000` are valid without a table. User / synthetic ids must appear as a definition site in the HIR tree, `LibraryRegistry`, or a resolver file-interface export (`struct.def_id` or `type.def_id` when not `nil`). `DefId(0)` and `DefId(u32::MAX)` are not definition sites. |
| `HirSourceAnchorId` | `presentation.owners` | membership in owners; attachments, `entry_start_anchor`, and structural ends name a registered owner |
| `InferVar` | carried on `Type::Infer` / `InferUnion` | integer fact only; no separate arena |

`TypeId` / `HirId` / `Symbol` (and the other rows) travel as the unsigned
integers in [§2.1](#21-arena-and-identity-integers). A decoder MUST NOT
infer an id from a MessagePack container opcode, a float width, or a
string.

Resolver runtime `symbols` / `scopes` / `next_def_id` are not on the
wire (FS-1 §14.3). Load seeds an empty resolver and does not re-allocate
ids; stored ids stay as stored.

---

## 19. Widths / dtype / shape — type-table value facts

Addendum losslessness, binding here:

> Numeric width and tensor dtype/shape are HIR type-table facts. They
> MUST appear in the FHIR `value`. They MUST NOT be inferred from
> MessagePack opcodes.

### 19.1 Width

`NumericWidth` appears **only** as:

- `Type` tag `sized_numeric` with `primitive` `numerus` or `fractus` and
  `width` a `NumericWidth` tag (`numerus<W>` / `fractus<W>`);
- `Type` tag `modular_word` with a `NumericWidth` tag.

Bare `Type` tag `primitive` / `numerus` or `fractus` is a distinct arena
entry. Semantic default widths `I64` / `F64` are a load-time analysis
rule, not an omitted field.

Forbidden (all `type` or `noncanonical`, never silently accepted):

- encoding `numerus<i32>` as MessagePack int32 / int64 / uint32;
- encoding `fractus<f16>` or `fractus<f32>` as MessagePack float32 or
  float64 and calling that the dtype;
- omitting `width` because "the opcode implies it";
- using float64-only FCMP §3.3 as permission to crunch widths.

### 19.2 Tensor dtype / shape

`Type` tags `tensor`, `vector`, `matrix`, `sparsa` carry:

- `elem` — `TypeId` of the element dtype (often a `sized_numeric`
  `fractus` + `f32`, or another arena entry);
- `shape` — `IndexId` into the index arena (`IndexExpr` tree: `literal`,
  `param`, `tuple`, `infer`, `unspecified`).

Forbidden:

- a MessagePack array of floats (or ints) standing in for a tensor value
  or a shape;
- inferring rank from array header length;
- collapsing symbolic / inferred / unspecified dimensions to numbers.

### 19.3 Buffers / `octeti`

Type-table facts (dtype + dimensionality) are the `Type` tags
`primitive`/`octeti`, `bounded_octeti`, `bounded_textus`,
`bounded_ascii`, and the tensor-family tags above.

Value payloads on the tree:

- `HirLiteral` tag `octeti` — `Symbol`; interner string is the exact hex
  sequence. Keep the `Symbol` so the arena index round-trips. Do not
  lift to an anonymous binary that drops interner identity.
- `HirLiteral` tags `ascii` / `string` — text payloads as `Symbol`.
- No standalone tensor-constant blob (FS-1 §3.4).

A decoder MUST NOT interpret octeti hex or tensor contents as an array
of generic floats.

### 19.4 Worked encodings (diagnostic notation)

`numerus<i32>` as an arena entry:

```text
{ "tag": "sized_numeric", "value": { "primitive": { "tag": "numerus" }, "width": { "tag": "i32" } } }
```

`tensor<fractus<f32>, [2, 3]>` (dtype is some `TypeId` E, shape some
`IndexId` S; S names `IndexExpr::Tuple` of two `Literal`s):

```text
{ "tag": "tensor", "value": { "elem": E, "shape": S } }
```

The float64 rule applies only when a schema field's carrier is IEEE
`f64` (a `fractus` **literal**, a JSON fractus, a CLI default float). It
does not encode E.

---

## 20. SPEC OPEN QUESTION — named postcard gaps

FS-1 §14 names facts the postcard snapshot itself drops. This mapping
**does not add them as MessagePack fields**. Adding them silently would
invent facts the live wire does not carry and would hide a losslessness
decision the addendum reserved to the operator.

Postcard load already seeds defaults for these fields
(`from_analyzed` / `reconstruct_unit` / `TypeTable::snapshot` /
`Resolver::from_snapshot` / `loaded_package_to_analyzed`). Structural
equality against today's snapshot does not require them. Equivalence to a
never-snapshotted in-session `AnalyzedModule` / `AnalyzedProgram` might.

### 20.1 The ruling asked of the operator

For each named gap, choose exactly one:

**A. Carried-forward-as-absent** — add the field to the schema as
OPTIONAL (or REQUIRED nullable) with a **named default equal to today's
postcard load behavior**, and a schema marker that producers of this
mapping omit it. Decoders apply the named default. A later document
minor MAY start emitting a non-default. Absence means "postcard load
behavior," not "unknown."

**B. Known-loss** — the field stays out of the schema. Reconstruction is
defined to seed the same default as postcard load. Closing the gap later
is a new document version (minor only if the field can be OPTIONAL with
the same default; major if meaning changes) or a proof that carried
facts already reconstruct it.

Until the ruling, this mapping has **no wire keys** for the rows below
and decode reconstruction matches postcard load. That is operationally
identical to B and is **not** a silent choice of A: A requires named
schema fields this document refuses to invent.

Do not treat a grep hit in a campaign note as a field list. The rows
are FS-1 §14 only.

### 20.2 Unit — dropped by `from_analyzed` / `reconstruct_unit`

| Live field | Postcard load | A would look like | Notes |
| --- | --- | --- | --- |
| `annotation_contracts` | `AnnotationContractMetadata::default()` | OPTIONAL omitted → empty contract registry | Applications still sit on `HirAnnotation`; the registry does not travel. |
| `qualified_identities` | `QualifiedIdentityTable::default()` | OPTIONAL omitted → empty table | |
| `radix_lanes` | `RadixLaneMetadata::default()` | OPTIONAL omitted → empty | Explicit F14 exclusion. |
| `graphics_source` | `GraphicsSourceFacts::default()` | OPTIONAL omitted → empty | Explicit F14 exclusion. |
| `diagnostics` | `[]` | OPTIONAL omitted → `[]` | |
| `package_import_identities` | `None` | REQUIRED nullable always `nil` | Package links live on the package envelope. |

### 20.3 Type table — dropped by `TypeTable::snapshot`

| Live field | Postcard load | A would look like | Notes |
| --- | --- | --- | --- |
| `variant_parent: map<DefId, DefId>` | empty | OPTIONAL omitted → empty logical map | HIR enums still carry variants. Imported-enum parent edges that existed only on this map do not travel. |

Caches (`intern_map`, memos, `primitives`, `first_modular_word`) are
derived. They are not gaps and are not fields.

### 20.4 Resolver — dropped by `ResolverSnapshot`

Not on the wire; load seeds defaults. Same A/B choice per row.

- `scopes`, `symbols`, `current_scope`, `lookup_shadow`
- `next_def_id` / builtin DefId counters and builtin
  forma/scrinium/status/sermo/meus/tuus handles
- `variant_parents`, `imported_enum_variants`
- resolver-local `gpu_builtins` / `gpu_builtins_order` (artifact field
  `gpu_builtins` is the durable copy — already mapped in §15)
- `namespace_seams`, `namespace_declarations`, `schemas`
- `canonical_imported_nominal_types` / `_defs`
- file-interface `methods`, `identity`, `canonical_exports`,
  `strict_members`, `const_members`
- **visibility tiers** on namespace exports (rebuilt as `Publica`)

`FileExportSnapshot` has no `enum` arm. Imported enums travel as
`type { ty, def_id }` plus `imported_nominal_types`. Variant
constructors are not a file-interface export kind. Adding an `enum` arm
would be a new fact, not a mapping of postcard.

### 20.5 CLI

Spans and `binding_symbol` on live `CliProgram` are excluded by F18.
FS-1 says this is not a semantic-HIR gap. They are **not** in this
ruling's A/B list.

### 20.6 Package → `AnalyzedProgram`

| Live field | Postcard load | A would look like |
| --- | --- | --- |
| `AnalyzedProgramNode.file_interface` | `FileInterface::new()` (empty) | OPTIONAL omitted → empty. Per-unit `resolver.file_interfaces` still travel. |
| `expanded_library_imports[].visibility` | forced `Privata` | OPTIONAL omitted → `privata` |

Load-time `spec.package_root` / `source_root` (artifact directory) and
recomputed `ModuleId` are not postcard gaps; they are not wire facts.

### 20.7 What a ruling is not

- A ruling of A is not a license to populate the fields from something
  other than a future proven snapshot.
- A ruling of B is not a license to drop any **carried** FS-1 family.
- Either ruling leaves FS-3's proof target as: encode live postcard
  snapshot → MessagePack → decode → structurally equal snapshot →
  reconstructed program equivalent to the postcard path.

---

## 21. Document kinds, frames, and limits (not settled here)

A framed unit:

```text
FABERMP\0 + profile 1.0 + payload length
{ "kind": "fhir.unit", "schema": [1, 0], "value": <HirArtifact record> }
```

A framed package:

```text
FABERMP\0 + profile 1.0 + payload length
{ "kind": "fhir.package", "schema": [1, 0], "value": <FhirPackage record> }
```

Root keys are `kind`, then `schema`, then `value`. Envelope-prefix
limits (kind-string 64, root map 3, schema array 2, empty-set payload
256) apply before kind admission.

Registration still requires finite per-kind limits (complete frame
bytes, nesting depth, array elements, map entries, string bytes, binary
bytes, decoded-node budget, package module multiplicity). This mapping
does not publish those numbers. Until they exist the kinds remain
reserved and a product decoder MUST NOT admit them.

---

## 22. What this unit does not settle

- Reference codec and round-trip proof (FS-3).
- Writer/reader switch; postcard stays the live wire (FS-4).
- FCMP 1.0 freeze.
- Kind registration and numeric resource limits.
- The [§20](#20-spec-open-question--named-postcard-gaps) losslessness
  ruling.
- Reconstructing original author source bytes (out of goal).

---

## Appendix A — `TokenKind` unit tags

Payload-bearing tags are in §5.5. Remaining live variants are unit tags.
Spelling follows [§1.6](#16-tag-spelling).

**Keywords and markers:** `columna`, `discretio`, `fixum`, `functio`,
`genus`, `iuncta`, `implendum`, `importa`, `magnitudo`, `modulus`,
`ordo`, `proba`, `probandum`, `sit`, `schema`, `typus`, `varia`,
`abstractus`, `ceteri`, `curata`, `errata`, `exitus`, `generis`,
`iacit`, `immutata`, `interna`, `nexum`, `optiones`, `prae`, `privata`,
`protecta`, `publica`, `casu`, `ceterum`, `custodi`, `discerne`, `dum`,
`elige`, `ergo`, `fac`, `itera`, `secus`, `si`, `sic`, `sin`, `perge`,
`redde`, `rumpe`, `tacet`, `adfirma`, `cape`, `iace`, `mori`,
`requirit`, `cede`, `clausura`, `cursor`, `futura`, `fiet`, `fiunt`,
`fient`, `figendum`, `variandum`, `reddet`, `tacebit`, `brevis`, `cli`,
`descriptio`, `fragment`, `imperium`, `longum`, `nomen`, `nondum`,
`nucleum`, `operandus`, `optio`, `radix`, `ubique`, `vertex`, `falsum`,
`nihil`, `verum`, `aut`, `est`, `et`, `non`, `vel`, `ego`, `finge`,
`implet`, `sub`, `conversio`, `verte`, `mone`, `nota`, `scribe`,
`vide`, `ad`, `argumenta`, `cura`, `incipiet`, `incipit`, `meus`,
`tuus`, `de`, `ex`, `in`, `lege`, `lineam`, `omnia`, `praefixum`,
`scriptum`, `sparge`, `ut`, `ab`, `ante`, `inter`, `intra`, `per`,
`usque`, `fragilis`, `futurum`, `metior`, `omitte`, `postpara`,
`postparabit`, `praepara`, `praeparabit`, `repete`, `solum`,
`solum_in`, `tag`, `temporis`, `negativum`, `nonnihil`, `nonnulla`,
`nulla`, `positivum`, `sponte`.

**Punctuation and operators:** `l_paren`, `r_paren`, `l_brace`,
`r_brace`, `l_bracket`, `r_bracket`, `comma`, `colon`, `semicolon`,
`dot`, `arrow`, `exit_arrow`, `cup`, `at`, `plus`, `minus`, `star`,
`slash`, `percent`, `bitwise_and`, `bitwise_or`, `bitwise_xor`,
`bitwise_not`, `bitwise_shl`, `bitwise_shr`, `bang`, `question`,
`middle_dot`, `cross`, `circled_times`, `circled_dot`, `nabla`, `eq`,
`assign`, `conversio_assign`, `eq_eq`, `eq_eq_eq`, `approx_eq`,
`approx_not_eq`, `bang_eq`, `bang_eq_eq`, `lt`, `gt`, `lt_ascii`,
`gt_ascii`, `lt_eq`, `gt_eq`, `post_inc`, `post_dec`, `question_dot`,
`question_bracket`, `question_paren`, `bang_dot`, `bang_bracket`,
`bang_paren`, `dot_dot`, `ellipsis`, `therefore`, `newline`, `eof`,
`error`.

A tag not in §5.5 and not in this appendix is `type` (unknown closed
union). Adding a live lexer variant is a document-major change to this
mapping.

---

## Appendix B — family index

| # | Envelope field | Section |
| ---: | --- | --- |
| 1 | `schema_version` | §3 |
| 2 | `source_identity` | §4 |
| 3 | `hir` | §5 |
| 4 | `presentation` | §10 |
| 5 | `cli_program` | §11 |
| 6 | `types` | §7 |
| 7 | `interner` | §9 |
| 8 | `resolver` | §8 |
| 9 | `libraries` | §12 |
| 10 | `function_facts` | §13 |
| 11 | `resolved_uses` | §14 |
| 12 | `analysis_stamp` | §16 |
| 13 | `gpu_builtins` | §15 |
| 14 | `package_schema_version` | §17 |
| 15 | `identity` | §17 |
| 16 | `entry_path` | §17 |
| 17 | `entry_frontmatter` | §17 |
| 18 | `modules` (incl. nested unit + import links) | §17.1–17.2 |
| 19 | `dependencies` | §17 |
