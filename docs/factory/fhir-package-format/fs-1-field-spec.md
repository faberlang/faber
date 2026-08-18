# FS-1: FHIR package field spec

**Status**: specified — inventory of every postcard-carried fact family as
named MessagePack fields
**Created**: 2026-08-18
**Goal**: [`goal.md`](goal.md)
**Authority**: operator addendum
[`../fcmp-profile-1/addendum.md`](../fcmp-profile-1/addendum.md)
**This unit does not**: invent a DTO, choose MessagePack opcodes, implement a
codec, or switch writers. Encoding rules are FS-2.

Live postcard types are the source of truth. Field names below are the serde
field names on those types. A MessagePack `value` that cannot reconstruct
today's `HirArtifact` → `AnalyzedModule` path (and the package envelope →
`AnalyzedProgram` path) is a lossy export, not a FHIR schema.

Schemas in force on the live wire:

| Envelope | Constant | Value |
| --- | --- | --- |
| Unit | `radix_hir_fhir::SCHEMA_VERSION` | `3` |
| Package | `radix_hir_fhir::PACKAGE_SCHEMA_VERSION` | `1` |

Decode is fail-closed on both. There is no compatibility decoder for earlier
schemas.

---

## Completeness

This spec is a complete projection of the live postcard snapshot:

- crate `radix-hir-fhir` (`HirArtifact`, `FhirPackage`, wire DTOs)
- crate `radix-hir` (`HirModule`, node kinds, `HirPresentation`,
  `LibraryRegistry`)
- crate `radix-types` (`TypeTableSnapshot`, `Type`, `IndexExpr`, `FuncSig`,
  widths)
- crate `radix-lexer` (`Symbol`, `Interner`, `Token`)
- driver reconstruction: `radix_module::hir::artifact::reconstruct_unit` →
  `AnalyzedModule`; `radix_module::hir::package::load_package` →
  `LoadedHirPackage`; Faber adapter
  `faber::package::fhir::loaded_package_to_analyzed` → `AnalyzedProgram`

Anything the postcard snapshot itself drops today is named in
[Named gaps](#named-gaps). Nothing in that list is silently invented as a
MessagePack field.

**Fact-family count: 19** — one per serde field on the two envelopes
(`HirArtifact` = 13, `FhirPackage` = 6). Nested records below are the
complete field lists inside those families, not extra top-level families.

---

## Stance (addendum, binding here)

- Numeric **width** and tensor **dtype/shape** are type-table **value** facts.
  They appear as named fields. They are never inferred from MessagePack
  opcodes.
- FCMP "float fields use float64" is a rule for an IEEE `f64` carrier. It is
  not permission to crunch `f32` / `f16` tensors, integer widths, or buffer
  payloads into one float.
- Buffer payloads are an exact byte sequence plus schema-stated dtype and
  dimensionality. Not an array of generic floats.
- Comments already stored on `HirPresentation` travel with the unit.
- The package does not round-trip original author source bytes. It
  round-trips analyzed HIR.

---

## 1. Unit envelope — `HirArtifact`

Record. MessagePack fields (serde names, declaration order):

| Field | Type | Role |
| --- | --- | --- |
| `schema_version` | `u32` | First wire field. Must equal `3`. |
| `source_identity` | `SourceIdentity` | Content hash + package-relative path. No absolute build-host path. |
| `hir` | `HirModule` | Lowered items + entry. |
| `presentation` | `HirPresentation` | Line-start comments, legal attachments, structural ends. Inseparable from `hir`. Never stored on `HirModule`. |
| `cli_program` | `CliProgramWire?` | Present when the unit declares `incipit argumenta`. |
| `types` | `TypeTableSnapshot` | Type + index arenas. Widths, tensor dtype/shape, buffer types live here. |
| `interner` | `[string]` | Ordered string table. `Symbol(u32)` is an index into this vec. |
| `resolver` | `ResolverSnapshot` | Import / file-interface surface. |
| `libraries` | `LibraryRegistry` | Library-import provenance. |
| `function_facts` | `FunctionFactTableWire` | Effect / capture / call-graph facts. |
| `resolved_uses` | `[ResolvedUseWire]` | Resolved definition uses. |
| `analysis_stamp` | `u64` | Monotonic in-process stamp. Package build zeros this. |
| `gpu_builtins` | `[(DefId, GpuBuiltinWire)]` | Kernel builtin assignments. |

Live constructor: `radix_module::hir::serialize::from_analyzed`.
Live decode: `radix_hir_fhir::decode` then
`radix_module::hir::artifact::reconstruct_unit`.

### 1.1 `SourceIdentity`

| Field | Type |
| --- | --- |
| `content_hash` | `string` (SHA-256 hex of original source bytes) |
| `relative_path` | `string` (project- or package-root relative) |

---

## 2. Items — `HirModule`

Record. The item family is the analyzed compilation unit's declaration
surface plus one optional executable entry.

| Field | Type |
| --- | --- |
| `items` | `[HirItem]` |
| `entry` | `HirBlock?` (`incipit` or implicit top-level statements; not a package root) |
| `entry_annotations` | `[HirAnnotation]` |
| `entry_args_name` | `Symbol?` (`incipit argumenta <name>`) |
| `entry_is_async` | `bool` (`incipiet`) |

### 2.1 `HirItem`

| Field | Type |
| --- | --- |
| `id` | `HirId` |
| `def_id` | `DefId` |
| `kind` | `HirItemKind` |
| `span` | `Span` |

### 2.2 `HirItemKind` (closed)

| Variant | Payload |
| --- | --- |
| `Function` | `HirFunction` (boxed) |
| `Struct` | `HirStruct` |
| `Enum` | `HirEnum` |
| `Interface` | `HirInterface` |
| `TypeAlias` | `HirTypeAlias` |
| `Schema` | `HirSchema` |
| `Constant` | `HirConst` |
| `Import` | `HirImport` |

Every variant below is postcard-carried. Field lists are complete.

### 2.3 Declaration payloads

**`HirFunction`**

| Field | Type |
| --- | --- |
| `name` | `Symbol` |
| `type_params` | `[HirTypeParam]` |
| `params` | `[HirParam]` |
| `cli_args` | `HirParam?` |
| `ret_ty` | `TypeId?` |
| `err_ty` | `TypeId?` |
| `body` | `HirBlock?` |
| `posture` | `CallablePosture` (`SyncFinite` / `AsyncFinite` / `SyncStream` / `AsyncStream`) |
| `is_async` | `bool` |
| `is_generator` | `bool` |
| `is_kernel` | `bool` (`@ nucleum` entry) |
| `nucleum_fragment` | `bool` (`@ nucleum fragment`) |
| `is_vertex` | `bool` |
| `is_fragment` | `bool` |
| `test` | `HirTestMetadata?` |
| `annotations` | `[HirAnnotation]` |

`NucleumRole` (`Entry` / `Fragment`) is derived from the two bits; it is not
its own wire field.

**`HirTestMetadata`**: `name: Symbol`, `suite_path: [Symbol]`,
`modifiers: [HirTestModifier]`, `span: Span`.

**`HirTestModifier`**: `Omitte(Symbol)`, `Futurum(Symbol)`, `Solum`,
`Tag(Symbol)`, `Temporis(i64)`, `Metior`, `Repete(i64)`, `Fragilis(i64)`,
`SolumIn(Symbol)`. (Retired `Requirit` is gone; that removal is why unit
schema is 3.)

**`HirTypeParam`**: `def_id`, `kind: GenericParamKind` (`Typus` /
`Magnitudo`), `name`, `span`, `constraint: HirTypeParamConstraint`
(`Any` / `OneOf([TypeId])`).

**`HirParam`**: `def_id`, `name`, `ty: TypeId`, `mode: HirParamMode`
(`Owned` / `De` / `In` / `Ex`), `optional: bool`, `sponte: bool`,
`default: HirExpression?`, `span`.

**`HirStruct`**: `name`, `type_params`, `annotations`, `fields: [HirField]`,
`methods: [HirMethod]`, `extends: DefId?`, `implements: [DefId]`.

**`HirField`**: `def_id`, `name`, `ty`, `is_static`, `sponte`, `annotations`,
`init: HirExpression?`, `span`.

**`HirMethod`**: `def_id`, `func: HirFunction`, `receiver: HirReceiver`
(`None` / `Ref` / `MutRef` / `Owned`), `span`.

**`HirEnum`**: `name`, `type_params`, `annotations`, `variants: [HirVariant]`.

**`HirVariant`**: `def_id`, `name`, `fields: [HirVariantField]`, `span`.

**`HirVariantField`**: `name`, `ty`, `span`.

**`HirInterface`**: `name`, `type_params`, `methods: [HirInterfaceMethod]`.

**`HirInterfaceMethod`**: `availability: [HirAvailability]`, `name`,
`params`, `ret_ty`, `err_ty`, `posture`, `span`.

**`HirAvailability`**: `Nondum { target: Symbol?, reason: Symbol? }`.

**`HirTypeAlias`**: `name`, `type_params`, `ty`.

**`HirSchema`**: `name`, `columns: [HirSchemaColumn]`.

**`HirSchemaColumn`**: `name`, `source`, `ty`, `nullable: bool`,
`order: u32`.

**`HirConst`**: `name`, `ty: TypeId?`, `value: HirExpression`, `mutable`,
`is_await`.

**`HirImport`**: `path: Symbol`, `visibility: Visibility` (`Privata` /
`Publica`), `items: [HirImportItem]`.

**`HirImportItem`**: `def_id`, `name`, `alias: Symbol?`.

**`HirAnnotation`**: `family: Symbol`, `fields: [HirAnnotationField]`,
`raw_args: [Token]`, `contract_id: DefId?`, `span`.

**`HirAnnotationField`**: `name`, `value: HirAnnotationValue`, `span`.

**`HirAnnotationValue`**: `Expr(HirExpression)`, `Type(TypeId)`,
`Symbol(Symbol)`, `String(Symbol)`, `Bool(bool)`, `Int(i64)`, `Float(f64)`,
`Nihil`.

**`Token`**: `kind: TokenKind`, `span`. `TokenKind` is the closed lexer
vocabulary. Payload-bearing variants: `Ident(Symbol)`, `Underscore(Symbol)`,
`Integer(u64)`, `Float(f64)`, `String(Symbol)`, `AsciiString(Symbol)`,
`BacktickString(Symbol)`, `OctetiString(Symbol)`. Remaining variants are
unit tags matching `radix_lexer::TokenKind` (keywords, punctuation,
sentinels). Do not invent a reduced token DTO.

---

## 3. Node kinds — statements, expressions, patterns

These are the rest of the `hir` family. They travel because
`HirModule` / `HirBlock` serde them directly.

### 3.1 `HirBlock`

| Field | Type |
| --- | --- |
| `statements` | `[HirStatement]` |
| `expr` | `HirExpression?` (boxed) |
| `span` | `Span` |
| `breakable` | `HirBreakableKind?` (`Fac` only) |
| `end` | `HirBlockEnd?` (presentation structure; not a statement) |

### 3.2 `HirStatement`

`id: HirId`, `kind: HirStatementKind`, `span`.

**`HirStatementKind`**: `Local(HirLocal)`, `Expr(HirExpression)`,
`Redde(HirExpression?)`, `Rumpe`, `Perge`, `Tacet`, `IncDec(HirIncDec)`,
`Custodi(HirCustodi)`.

**`HirLocal`**: `def_id`, `name`, `ty: TypeId?`, `init: HirExpression?`,
`mutable`, `is_await`, `runtime_provided`.

**`HirIncDec`**: `target: HirExpression`, `op: HirIncDecOp` (`Inc` / `Dec`).

**`HirCustodi`**: `clauses: [HirCustodiClause]`
(`cond`, `body: HirBlock`, `span`).

### 3.3 `HirExpression`

`id: HirId`, `kind: HirExpressionKind`, `ty: TypeId?`, `span`.

**`HirExpressionKind`** (closed, every variant postcard-carried):

| Variant | Payload |
| --- | --- |
| `Path` | `DefId` |
| `Literal` | `HirLiteral` |
| `Vacua` | unit |
| `Binary` | `(HirBinOp, HirExpression, HirExpression)` |
| `Unary` | `(HirUnOp, HirExpression)` |
| `Call` | `(HirExpression, [TypeId], [HirCallArg])` |
| `Gradient` | `{ call, selected_arguments: [HirExpression] }` |
| `MethodCall` | `(HirExpression, Symbol, [TypeId], [HirCallArg])` |
| `Field` | `(HirExpression, Symbol)` |
| `Index` | `(HirExpression, HirExpression)` |
| `OptionalChain` | `(HirExpression, HirOptionalChainKind)` |
| `NonNull` | `(HirExpression, HirNonNullKind)` |
| `Block` | `HirBlock` |
| `Si` | `{ cond, then_block, then_catch: HirCape?, else_block: HirBlock? }` |
| `Discerne` | `{ scrutinees, arms: [HirCasuArm], exhaustive: bool }` |
| `Loop` | `HirBlock` |
| `Dum` | `(HirExpression, HirBlock)` |
| `Itera` | `(HirIteraMode, DefId, Symbol, HirExpression, HirBlock)` |
| `Intervallum` | `{ start, end, step?, kind: HirRangeKind }` |
| `Assign` | `(HirExpression, HirExpression)` |
| `ConversioAssign` | `{ target, source, recovery? }` |
| `Array` | `[HirArrayElement]` (`Expr` / `Spread`) |
| `Struct` | `(DefId, [(Symbol, HirExpression)])` |
| `Tuple` | `([HirExpression], [TypeId]?)` |
| `Scribe` | `(HirScribeKind, [HirExpression])` |
| `Scriptum` | `(Symbol, [HirExpression])` |
| `ReadLine` | `bool` |
| `Praefixum` | `HirExpression` |
| `Adfirma` | `(HirExpression, HirExpression?)` |
| `Panic` | `HirExpression` |
| `Throw` | `HirExpression` |
| `Handled` | `{ body: HirBlock, catch: HirCape }` |
| `Clausura` | `([HirParam], TypeId?, TypeId?, HirExpression)` |
| `Cede` | `HirExpression` (legacy carrier) |
| `Reddet` | `HirExpression` |
| `Tacebit` | `HirExpression` |
| `Yield` | `HirExpression` |
| `Verte` | `{ source, target: TypeId, entries: [HirObjectField]? }` |
| `Conversio` | `{ source, target: HirConversioTarget, params: [Symbol], recovery? }` |
| `Ad` | `{ route: Symbol, opener? }` |
| `Ref` | `(HirRefKind, HirExpression)` |
| `Deref` | `HirExpression` |
| `TypeCheck` | `{ positive: bool, expr, ty: TypeId }` |
| `Error` | unit |

Supporting enums / records (all carried):

- `HirCallArg`: `name: Symbol?`, `spread: bool`, `expr`, `span`
- `HirCape`: `binding_def_id`, `binding_name`, `binding_ty: TypeId?`, `body`, `span`
- `HirScribeKind`: `Nota` / `Vide` / `Mone` / `Scribe`
- `HirObjectField`: `key: HirObjectKey`, `value: HirExpression?`
- `HirObjectKey`: `Ident(Symbol)` / `String(Symbol)` / `Computed(HirExpression)` / `Spread(HirExpression)`
- `HirOptionalChainKind` / `HirNonNullKind`: `Member(Symbol)` / `Index(HirExpression)` / `Call([HirCallArg])`
- `HirIteraMode`: `Ex` / `De` / `Ab`
- `HirConversioTarget`: `Type(TypeId)` / `Intervallum(HirExpression)`
- `HirRangeKind`: `Exclusive` / `Inclusive`
- `HirBinOp`: `Add`, `Sub`, `Mul`, `Div`, `Mod`, `Dot`, `Cross`, `TensorProduct`, `Hadamard`, `Eq`, `NotEq`, `ApproxEq`, `ApproxNotEq`, `StrictEq`, `StrictNotEq`, `Lt`, `Gt`, `LtEq`, `GtEq`, `And`, `Or`, `Coalesce`, `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr`, `Is`, `IsNot`, `InRange`, `Between`
- `HirUnOp`: `Neg` / `Not` / `BitNot`
- `HirRefKind`: `Shared` / `Mutable`
- `HirCasuArm`: `patterns: [HirPattern]`, `guard?`, `body: HirExpression`, `span`
- `HirPattern`: `Wildcard` / `Binding(DefId, Symbol)` / `Alias(DefId, Symbol, HirPattern)` / `Variant(DefId, [HirPattern])` / `Literal(HirLiteral)`

### 3.4 `HirLiteral`

| Variant | Payload | Notes |
| --- | --- | --- |
| `Int` | `u64` | Magnitude. Negation is `Unary(Neg, …)`. |
| `Float` | `f64` | IEEE carrier for a *literal* `fractus` spelling, not a tensor dtype. |
| `String` | `Symbol` | Unicode textus payload (interner, raw). |
| `Ascii` | `Symbol` | ASCII payload (interner, raw). |
| `Octeti` | `Symbol` | Exact byte-buffer payload. Interner string is the hex payload without `|…|` delimiters (`TokenKind::OctetiString`). |
| `JsonValor` | `JsonValue` | Compile-time JSON tree. |
| `Regex` | `(Symbol, Symbol?)` | Pattern + optional flags. |
| `Bool` | `bool` | |
| `Nil` | unit | |

**`JsonValue`**: `Object([JsonMember])` / `Array([JsonValue])` /
`String(Symbol)` / `Numerus(i64)` / `Fractus(f64)` / `Bool(bool)` / `Null`.

**`JsonMember`**: `key_span: Span`, `key: Symbol`, `value: JsonValue`.

There is **no** dedicated tensor-constant buffer variant on `HirLiteral`.
Tensor *values* are ordinary HIR expressions; tensor *type* facts are
`Type::Tensor` (and kin) in the type table. Named as a gap if a future DTO
wants a standalone tensor blob: postcard does not have one.

### 3.5 `Span`

| Field | Type |
| --- | --- |
| `start` | `u32` (inclusive byte offset) |
| `end` | `u32` (exclusive byte offset) |

Spans are diagnostic / source-map aids into the original source buffer. The
package does not carry those source bytes. Offsets still travel.

---

## 4. Type table — `TypeTableSnapshot`

Record. This is the `types` family. **Value facts live here.**

| Field | Type |
| --- | --- |
| `types` | `[Type]` — arena; `TypeId(u32)` is the index |
| `indices` | `[IndexExpr]` — arena; `IndexId(u32)` is the index |
| `unspecified_shape` | `IndexId?` — must point at `IndexExpr::Unspecified` when set |
| `next_index_var` | `u32` — next `IndexVar` allocator |

Not on the snapshot (rebuilt on load; not gaps of *semantic* fact except
where named below):

- `intern_map` (hash-cons) — rebuilt first-occupancy from `types`
- `primitives` map — rebuilt by scanning `Type::Primitive` entries; load
  rejects a snapshot missing any required primitive
- `equals_memo` / `assignable_memo` / `find_equal_memo` — empty on load
- `first_modular_word` — recomputed from the first `Type::ModularWord`

**Required primitives** (must appear as `Type::Primitive` entries):
`Textus`, `Numerus`, `Fractus`, `Bivalens`, `Nihil`, `Vacuum`, `Numquam`,
`Ignotum`, `Octeti`, `Regex`, `Json`, `Valor`, `Ascii`, `Instans`.

### 4.1 `Type` (closed)

| Variant | Payload |
| --- | --- |
| `Primitive` | `Primitive` |
| `Array` | `TypeId` (`lista<T>`) |
| `BoundedArray` | `{ element: TypeId, capacity: IndexId }` |
| `BoundedTextus` | `{ capacity: IndexId }` |
| `BoundedAscii` | `{ capacity: IndexId }` |
| `BoundedOcteti` | `{ capacity: IndexId }` |
| `Map` | `(TypeId, TypeId)` |
| `Record` | `map<Symbol, TypeId>` |
| `Set` | `TypeId` |
| `Promissum` | `TypeId` |
| `PromissumFailable` | `(TypeId, TypeId)` |
| `Cursor` | `TypeId` |
| `AsyncCursor` | `(TypeId, TypeId)` |
| `Tensor` | `(TypeId, IndexId)` — **dtype + shape** |
| `Vector` | `(TypeId, IndexId)` |
| `Matrix` | `(TypeId, IndexId)` |
| `Sparsa` | `(TypeId, IndexId)` |
| `Atomic` | `TypeId` |
| `Intervallum` | `TypeId` |
| `SizedNumeric` | `(Primitive, NumericWidth)` — **width** |
| `ModularWord` | `NumericWidth` |
| `SizedInstans` | `InstansPrecision` (`Millis` / `Micros` / `Nanos`) |
| `Option` | `TypeId` |
| `Ref` | `(SemanticMutability, TypeId)` (`Immutable` / `Mutable`) |
| `Struct` | `DefId` |
| `Enum` | `DefId` |
| `Interface` | `DefId` |
| `Alias` | `(DefId, TypeId)` |
| `Func` | `FuncSig` |
| `Param` | `Symbol` |
| `Applied` | `(TypeId, [TypeId])` |
| `Infer` | `InferVar` (`u32` newtype) |
| `InferUnion` | `InferVar` |
| `Union` | `[TypeId]` |
| `Tuple` | `[TypeId]` |
| `Error` | unit |

**`FuncSig`**: `type_params: [Symbol]`,
`type_param_constraints: [TypeParamConstraint]` (`Any` / `OneOf([TypeId])`),
`params: [ParamType]`, `ret: TypeId`, `err: TypeId?`, `is_async`,
`is_generator`.

**`ParamType`**: `ty: TypeId`, `mode: SemanticParamMode`
(`Owned` / `Ref` / `MutRef` / `Move`), `optional: bool`.

### 4.2 Numeric width (value fact)

`NumericWidth` is a closed enum: `I8`, `I16`, `I32`, `I64`, `U8`, `U16`,
`U32`, `U64`, `F16`, `F32`, `F64`.

It appears on the wire **only** as:

- `Type::SizedNumeric(Primitive::Numerus, width)` for `numerus<W>`
- `Type::SizedNumeric(Primitive::Fractus, width)` for `fractus<W>`
- `Type::ModularWord(width)` for modular words

Bare `Type::Primitive(Numerus)` / `Type::Primitive(Fractus)` are distinct
arena entries. Semantic default widths are `I64` / `F64`
(`NumericWidth::default_for`). Those defaults are a load-time / analysis
rule, not an omitted field: the snapshot stores the primitive variant
itself.

A MessagePack encoder must emit the `SizedNumeric` / `NumericWidth` tags.
It must not collapse `numerus<i32>` into a MessagePack int32, or
`fractus<f16>` into a MessagePack float.

### 4.3 Tensor dtype / shape (value fact)

`Type::Tensor(elem, shape)`:

- `elem: TypeId` — element dtype (often `SizedNumeric(Fractus, F32)` etc.)
- `shape: IndexId` — index-arena handle for `Figura`

Same pair form on `Vector`, `Matrix`, `Sparsa`.

**`IndexExpr`** (index arena):

| Variant | Payload |
| --- | --- |
| `Literal` | `u64` |
| `Param` | `Symbol` (`magnitudo N`) |
| `Tuple` | `[IndexId]` (rank-n; empty tuple is rank-0) |
| `Infer` | `IndexVar` |
| `Unspecified` | unit (legacy `tensor<T>` without `Figura`) |

Shape is this arena, not a MessagePack array of generic numbers. Symbolic
and inferred dimensions stay `Param` / `Infer` / `Unspecified`.

### 4.4 Buffers / `octeti`

Type-table facts (dtype + dimensionality):

- `Primitive::Octeti` — unbounded byte buffer
- `BoundedOcteti { capacity: IndexId }` — `octeti<N>`
- `BoundedTextus` / `BoundedAscii` — bounded text buffers
- `Tensor` / `Vector` / `Matrix` / `Sparsa` — typed buffers whose dtype is
  `elem` and whose dimensionality is `shape`

Value payloads on the HIR tree:

- `HirLiteral::Octeti(Symbol)` — exact hex payload in the interner
- `HirLiteral::Ascii` / `String` — text payloads
- no standalone tensor-constant blob (see §3.4)

A MessagePack mapping must carry buffer bytes as binary (or an equivalent
exact byte sequence) **plus** the type-table dtype/shape. It must not
decode octeti or tensor contents as an array of floats.

---

## 5. Interner / `Symbol` arena

`HirArtifact.interner` is `[string]` in symbol-id order.

- `Symbol(u32)` = index into that vec.
- Identifier strings are already NFKC-canonical.
- Literal and comment payloads are interned **raw** (`Interner::intern_raw`):
  no re-normalization on load.
- The lookup `FxHashMap<String, Symbol>` is not serialized. Load rebuilds it
  via `Interner::from_string_table` (`intern_raw` each entry).

---

## 6. Imports

Three postcard surfaces, all required.

### 6.1 `ResolverSnapshot`

| Field | Type |
| --- | --- |
| `namespace_exports` | `[(Symbol, [Symbol])]` — binding → public member names |
| `file_interfaces` | `[(Symbol, FileInterfaceSnapshot)]` |
| `used_namespaces` | `[Symbol]` |
| `imported_nominal_types` | `[((NominalKind, Symbol), TypeId)]` |
| `ambiguous_imported_nominal_types` | `[(NominalKind, Symbol)]` |

**`NominalKind`**: `Struct` / `Enum` / `Interface`.

**`FileInterfaceSnapshot`**: `exports: [(Symbol, FileExportSnapshot)]`.

**`FileExportSnapshot`**:

| Variant | Fields |
| --- | --- |
| `Function` | `FuncSig` |
| `Type` | `{ ty: TypeId, def_id: DefId? }` |
| `Struct` | `{ def_id: DefId, ty: TypeId, fields: [StructFieldSnapshot] }` |

**`StructFieldSnapshot`**: `name: Symbol`, `ty: TypeId`, `optional: bool`,
`required: bool`.

Load (`Resolver::from_snapshot`) rebuilds runtime maps and marks every
exported name `VisibilityTier::Publica`. See [Named gaps](#named-gaps).

### 6.2 `LibraryRegistry` (on the unit)

| Field | Type |
| --- | --- |
| `bindings` | `map<DefId, LibraryBinding>` |
| `items` | `map<DefId, LibraryItem>` |
| `exports` | `map<DefId, set<string>>` |
| `reexports` | `map<(DefId, string), string>` |

**`LibraryBinding`**: `local_def_id`, `identity: LibraryIdentity`,
`rust_runtime_module: string?`, `rust_runtime_methods: map<string, string>`.

**`LibraryIdentity`**: `provider: LibraryProvider` (`Builtin(string)` /
`Package(string)`), `module_path: [string]`.

**`LibraryItem`**: `def_id`, `identity`, `exported_name: string`,
`kind: LibraryItemKind` (`Interface` / `Function` / `TypeAlias` / `Struct` /
`Enum` / `Const`), `is_failable`, `is_async`, `rust_runtime_type: string?`,
`elide_rust_decl`.

Live postcard serializes the `FxHashMap` / `FxHashSet` shapes via serde
maps. A MessagePack mapping must make those maps deterministic (sorted
keys). That is an FS-2 encoding rule, not a new fact.

### 6.3 HIR import items

`HirItemKind::Import` (§2.3) is the source-level import declaration
retained on the tree. It is not a substitute for `ResolverSnapshot` or
`LibraryRegistry`.

---

## 7. Facts

### 7.1 `FunctionFactTableWire`

| Field | Type |
| --- | --- |
| `entries` | `[(DefId, FunctionFactWire)]` |
| `entry` | `FunctionFactWire?` (module entry facts) |

**`FunctionFactWire`**: `direct_failure: bool`, `may_fail: bool`,
`is_async: bool`, `is_generator: bool`, `requires_await: bool`,
`call_edges: [DefId]`, `captures: [CaptureFactWire]`,
`param_modes: [(DefId, SemanticParamMode)]`, `err_ty: TypeId?`.

**`CaptureFactWire`**: `def_id: DefId`, `mode: CaptureModeWire`
(`Read` / `Mutate` / `Consume`).

Live `FxHashSet` / `FxHashMap` fact tables flatten to these vecs on the
wire.

### 7.2 `ResolvedUseWire`

| Field | Type |
| --- | --- |
| `key` | `ResolvedUseKeyWire` (`Local(DefId)` / `Portable(string)`) |
| `kind` | `ResolvedUseKindWire` (`Path` / `Call` / `MethodCall` / `TypeRef` / `ImportBinding`) |
| `enclosing` | `DefId?` |

Load rebuilds `ResolvedUseIndex` lookup maps from this vec.

### 7.3 `gpu_builtins`

`[(DefId, GpuBuiltinWire)]`.

**`GpuBuiltinWire`**: `GlobalInvocationX` / `Y` / `Z` / `Id`,
`WorkgroupIdX` / `Y` / `Z` / `WorkgroupId`,
`LocalInvocationIdX` / `Y` / `Z` / `LocalInvocationId`,
`WorkgroupSizeX` / `Y` / `Z`.

Mirrors `radix_types::MirKernelBuiltin`. The resolver's own runtime
`gpu_builtins` map is **not** on `ResolverSnapshot`; the artifact field is
the durable copy.

### 7.4 `analysis_stamp`

`u64`. In-process monotonic stamp (`AnalysisStamp::from_persisted` on
load). Package `build_package` forces `0` so package bytes are content-
deterministic. The stamp is not a semantic fact of the program.

---

## 8. Presentation — `HirPresentation`

Record. Schema v2 addition; unit schema 3 still requires it. Semantic nodes
stay trivia-blind: the sidecar back-references `HirId` / `DefId`.

| Field | Type |
| --- | --- |
| `owners` | `[HirTriviaOwner]` |
| `attachments` | `[HirTriviaAttachment]` |
| `block_ends` | `[HirBlockEnd]` |
| `program_end` | `HirModuleEnd` |
| `entry_start_anchor` | `HirSourceAnchorId?` |

**`HirSourceAnchorId(u32)`** — dedicated namespace, never a `HirId`.

**`HirTriviaOwner`**: `id: HirSourceAnchorId`, `kind: HirTriviaOwnerKind`,
`semantic: HirOwnerRef?`.

**`HirTriviaOwnerKind`** (closed legal attachment positions):

| Variant | Owner |
| --- | --- |
| `Item` | top-level `HirItem` |
| `Statement` | `HirStatement` |
| `Member` | `HirField` / `HirMethod` |
| `Entry` | explicit `incipit` |
| `BlockEnd` | authored executable block end |
| `ProgramEnd` | module structural end |

Adding a legal comment position means extending this enum along with
syntax, lowering, render, and tests. Do not invent extra owner kinds in
the MessagePack schema.

**`HirOwnerRef`**: `Hir(HirId)` (item or statement) / `Def(DefId)` (member).

**`HirTriviaAttachment`**: `anchor: HirSourceAnchorId`,
`leading: [HirTrivia]`.

**`HirTrivia`**:

| Variant | Fields |
| --- | --- |
| `CommentLine` | `{ text: Symbol, span }` — line-start `# …` payload, verbatim via the interner |
| `Newline` | `{ span }` — boundary newline in lexer/parser order |

Comments travel. Canonical emit from a loaded unit must remain as capable
as emit from in-session HIR.

**`HirBlockEnd` / `HirModuleEnd`**: `id: HirSourceAnchorId`, `span`.

Integrity (live `validate_presentation`): owner anchors unique; each owner
at most one attachment; every attachment names a registered owner;
comment `Symbol`s resolve in the interner; semantic back-refs resolve in
the HIR / Def registries; `entry_start_anchor` names a registered owner;
structural ends are anchor-consistent.

---

## 9. CLI program — `CliProgramWire`

Optional unit family (`cli_program`). Diagnostic-only spans and
`binding_symbol` references are excluded by design (F18) and are **not**
named as semantic gaps.

| Field | Type |
| --- | --- |
| `name` | `string` |
| `entry_args` | `string` |
| `mode` | `CliModeWire` (`NotCli` / `SingleCommand` / `Subcommand`) |
| `version` | `string?` |
| `description` | `string?` |
| `global_options` | `[CliOptionWire]` |
| `global_operands` | `[CliOperandWire]` |
| `options` | `[CliOptionWire]` |
| `operands` | `[CliOperandWire]` |
| `commands` | `[CliCommandWire]` |
| `exit` | `CliExitWire?` (`Fixed(i64)` / `Binding(string)` / `Field { object, field }` / `Unsupported`) |

**`CliOptionWire`**: `binding`, `ty: CliTypeWire`, `short?`, `long?`,
`description?`, `global`, `default: CliDefaultWire?`, `flag`.

**`CliOperandWire`**: `binding`, `ty`, `rest`, `description?`, `global`,
`default?`.

**`CliCommandWire`**: `path: [string]`, `module_path: [string]?`,
`function: string`, `function_symbol: Symbol`, `args_binding: string?`,
`aliases: [string]`, `description?`, `options`, `operands`.

**`CliTypeWire`**: `Textus` / `Numerus` / `Fractus` / `Bivalens` / `Octeti`
/ `Ignotum` / `ListaTextus` / `ListaNumerus`.

**`CliDefaultWire`**: `Text(string)` / `Integer(i64)` / `Float(f64)` /
`Bool(bool)` / `Nil` / `Expr(string)`.

---

## 10. Package envelope — `FhirPackage`

Record. Separate schema from the unit. First wire field is the package
version prefix.

| Field | Type |
| --- | --- |
| `package_schema_version` | `u32` — must equal `1` |
| `identity` | `PackageIdentityWire` |
| `entry_path` | `string` (package-root relative) |
| `entry_frontmatter` | `string?` (TOML text; format crate is TOML-free) |
| `modules` | `[FhirPackageModule]` — sorted by `path`, unique |
| `dependencies` | `[PackageDependencyWire]` — sorted by `name`, unique |

**`PackageIdentityWire`**: `name`, `version`, `edition` (all `string`).

**`PackageDependencyWire`**: `name`, `version`, `lock_identity`,
`checksum: string?`. Exact coordinates only; acquisition is the store's
job.

File extension of the envelope: `fhirpkg`.

### 10.1 `FhirPackageModule`

| Field | Type |
| --- | --- |
| `path` | `string` (package-root relative, e.g. `src/main.fab`) |
| `module_segments` | `[string]` |
| `is_entry` | `bool` |
| `export_names` | `[string]` (sorted public names) |
| `local_links` | `[LocalLinkWire]` — **package import links** |
| `library_imports` | `[LibraryImportWire]` — **package import links** |
| `source_hash` | `string` (must equal embedded unit `SourceIdentity.content_hash`) |
| `unit_schema_version` | `u32` (manifest copy; must equal unit `SCHEMA_VERSION`) |
| `unit` | `bytes` — postcard-encoded `HirArtifact` (nested unit codec) |

A MessagePack package mapping may embed the unit as a nested `HirArtifact`
record instead of opaque bytes. That is an FS-2 encoding choice. The
**facts** are the unit's 13 families plus this module row.

---

## 11. Package import links

### 11.1 Local links — `LocalLinkWire`

| Field | Type |
| --- | --- |
| `binding` | `string` |
| `target` | `string` (package-root-relative module path) |

Explicit link table. Load never re-derives local imports from the
filesystem. Decode rejects a target missing from the module table
(`DanglingModuleRef`). Sorted by `binding` at package build.

### 11.2 Library imports — `LibraryImportWire`

| Field | Type |
| --- | --- |
| `binding` | `string` |
| `package` | `string` |
| `module` | `[string]` |

No `interface_path`. No absolute checkout path. Sorted by
`(binding, package, module)` at package build.

The Faber adapter requires every `library_imports.package` to appear in
`dependencies`. That check is on the product adapter, not the format
crate.

---

## 12. Post-RTR module / `AnalyzedProgram` graph

The portable projection of the multi-unit graph is the package envelope
plus each unit's `HirArtifact`. Live reconstruction:

```
FhirPackage
  → decode_package (format crate)
  → reconstruct_unit per module (same as standalone load)
  → LoadedHirPackage
  → loaded_package_to_analyzed (Faber adapter)
  → AnalyzedProgram
```

### 12.1 `LoadedHirPackage` (decoded envelope)

`identity`, `entry_path`, `entry_frontmatter`, `modules: [LoadedHirModule]`,
`dependencies`. Each `LoadedHirModule` carries the module-row fields plus
`unit: AnalyzedModule`.

### 12.2 `AnalyzedModule` reconstruction (`reconstruct_unit`)

| Live field | Source |
| --- | --- |
| `analysis_stamp` | `analysis_stamp` |
| `interner` | `interner` via `Interner::from_string_table` |
| `types` | `types` via `TypeTable::from_snapshot` |
| `resolver` | `resolver` via `Resolver::from_snapshot` |
| `hir` | `hir` |
| `presentation` | `presentation` |
| `cli_program` | `cli_program` (wire → live) |
| `libraries` | `libraries` |
| `function_facts` | `function_facts` (wire → live) |
| `resolved_uses` | `resolved_uses` (wire → live index) |
| `gpu_builtins` | `gpu_builtins` (wire → live) |

Fields forced to empty / default on load (postcard does not carry them):
see [Named gaps](#named-gaps).

### 12.3 `AnalyzedProgram` reconstruction (`loaded_package_to_analyzed`)

| Live field | Rebuilt from |
| --- | --- |
| `spec.entry` | `package_root` + `entry_path` |
| `spec.package_root` / `source_root` | load-time artifact directory (not on the wire) |
| `spec.manifest_backed` | `true` |
| `spec.templates` | empty |
| `entry` | the module with `is_entry` |
| `roots` | `[entry]` or all module ids |
| `nodes[id].path` | `package_root` + module `path` |
| `nodes[id].module_segments` | `module_segments` |
| `nodes[id].is_entry` | `is_entry` |
| `nodes[id].analysis` | reconstructed `AnalyzedModule` |
| `nodes[id].export_names` | `export_names` |
| `nodes[id].namespace_exports` | `{ binding → target.export_names as Publica }` from `local_links` |
| `nodes[id].expanded_library_imports` | `library_imports` |
| `imports` (ModuleGraph) | `local_links` + expanded library imports |
| `entry_frontmatter` | parse `entry_frontmatter` TOML |
| `diagnostics` | empty |
| `library_resolver` | empty (`LibraryResolver::with_optional_home(None)`) |

`ModuleId` is recomputed as `ModuleId::for_spec(spec, module_segments)`.
Unit-local `DefId` / `HirId` / `TypeId` / `Symbol` are **never**
package-global. Cross-unit references travel as module path + export name
(the link table + per-unit resolver snapshots).

---

## 13. Arena rehydration rules

All of these ids are unit-local. A package is a list of units, not one
shared arena.

### 13.1 `Symbol(u32)`

- Arena: `HirArtifact.interner`.
- Valid iff `id < interner.len()`.
- Load: `Interner::from_string_table` preserves indices (raw intern, no
  NFKC pass).
- Comments, octeti hex, ascii/textus payloads, regex, identifiers all
  share this table.

### 13.2 `TypeId(u32)`

- Arena: `TypeTableSnapshot.types`.
- Valid iff `id < types.len()`.
- Load copies the vec in order; hash-cons map rebuilt first-occupancy.
- Nested `TypeId` / `IndexId` inside `Type` entries must stay in bounds
  (`TypeTable::from_snapshot` rejects otherwise).

### 13.3 `IndexId(u32)` / `IndexVar(u32)`

- Arena: `TypeTableSnapshot.indices`.
- `unspecified_shape`, when set, must be in bounds and name
  `IndexExpr::Unspecified`.
- `next_index_var` is restored as stored.

### 13.4 `HirId(u32)`

- Identity of an annotatable HIR node (`HirItem.id`, `HirStatement.id`,
  `HirExpression.id`).
- **Not** a dense 0..N arena. Lowering leaves gaps after error recovery.
- Valid iff the id appears on a node in the deserialized `HirModule` tree
  (membership, not a count bound).
- Presentation `HirOwnerRef::Hir` must name a collected id.

### 13.5 `DefId(u32)`

- Identity of a named definition (items, params, locals, pattern bindings,
  imports, fields, methods, library bindings).
- Ranges: builtins `(0, USER_DEF_ID_BASE)` with
  `USER_DEF_ID_BASE = 0x0000_1000` are always valid and are **not**
  deserialized as a table; they are compiler constants.
- User / synthetic ids are valid iff they appear as a definition site in
  the HIR tree, `LibraryRegistry`, or a resolver file-interface export
  (`Struct.def_id` or `Type { def_id: Some(...) }`).
- `DefId(0)` and `DefId::INVALID` (`u32::MAX`) are not definition sites.
- Resolver runtime `symbols` / `scopes` / `next_def_id` are **not** on the
  wire. Load seeds an empty resolver and does not re-allocate ids; existing
  ids in HIR / snapshots stay as stored.

### 13.6 `HirSourceAnchorId(u32)`

- Dedicated presentation namespace.
- Valid iff it appears in `presentation.owners`.
- Attachments, `entry_start_anchor`, and structural ends must name a
  registered owner.

### 13.7 Referential integrity

`radix_hir_fhir::decode` runs `validate_referential_integrity` after
postcard decode and before returning the artifact. The driver
reconstruction does not re-parse; it only rebuilds derived caches. A
MessagePack decoder must apply the same membership / bounds checks
fail-closed.

---

## 14. Named gaps

Postcard itself drops these live `AnalyzedModule` / `AnalyzedProgram` /
resolver / type-table fields. They are **not** MessagePack fields in this
spec. A replacement DTO may only omit them if reconstruction from carried
facts is proven equivalent (addendum §4). Until then they are gaps.

### 14.1 Unit — dropped by `from_analyzed` / `reconstruct_unit`

| Live field | Load behavior | Notes |
| --- | --- | --- |
| `annotation_contracts` | `AnnotationContractMetadata::default()` | Applications still sit on `HirAnnotation`; the contract registry does not travel. |
| `qualified_identities` | `QualifiedIdentityTable::default()` | |
| `radix_lanes` | `RadixLaneMetadata::default()` | Explicit F14 exclusion. |
| `graphics_source` | `GraphicsSourceFacts::default()` | Explicit F14 exclusion. |
| `diagnostics` | `[]` | |
| `package_import_identities` | `None` | Package links live on the **package** envelope, not the unit. |

### 14.2 Type table — dropped by `TypeTable::snapshot`

| Live field | Load behavior |
| --- | --- |
| `variant_parent: map<DefId, DefId>` | empty. HIR enums still carry variants; imported-enum parent edges that existed only on this map do not travel. |

Caches (`intern_map`, memos, `primitives`, `first_modular_word`) are
derived, not semantic gaps.

### 14.3 Resolver — dropped by `ResolverSnapshot` (F5 / CTM E.2 / CXO F4)

Not on the wire; load seeds defaults:

- `scopes`, `symbols`, `current_scope`, `lookup_shadow`
- `next_def_id` / builtin DefId counters and builtin forma/scrinium/status/sermo/meus/tuus handles
- `variant_parents`, `imported_enum_variants`
- resolver-local `gpu_builtins` / `gpu_builtins_order` (artifact field is the durable copy)
- `namespace_seams`, `namespace_declarations`, `schemas`
- `canonical_imported_nominal_types` / `_defs`
- file-interface `methods`, `identity`, `canonical_exports`, `strict_members`, `const_members`
- **visibility tiers** on namespace exports (rebuilt as `Publica`)

`FileExportSnapshot` has no `Enum` arm. Imported enums travel as
`Type { ty, def_id }` plus `imported_nominal_types`. Variant constructors
are not a file-interface export kind.

### 14.4 CLI

Spans and `binding_symbol` on the live `CliProgram` are excluded by F18.
Not a semantic-HIR gap.

### 14.5 Package → `AnalyzedProgram`

| Live field | Load behavior |
| --- | --- |
| `AnalyzedProgramNode.file_interface` | `FileInterface::new()` (empty). Per-unit `ResolverSnapshot.file_interfaces` still travel. |
| `expanded_library_imports[].visibility` | forced `Privata` |
| `expanded_library_imports[].import_span` | `Span::default()` |
| `expanded_library_imports[].module` path | synthesized `package_root/libs/<package>` (not a stored interface path) |
| `namespace_exports` tiers | `Publica` |
| `PackageSpec.templates` | empty |
| `PackageSpec.package_root` | load-time directory, not a portable fact |
| `diagnostics` | empty |
| `library_resolver` | empty |

### 14.6 No standalone tensor-constant buffer family

`HirLiteral` has `Octeti(Symbol)` for byte buffers. It has no
`Tensor { dtype, shape, bytes }` payload. Tensor constants, if any, are
HIR expressions plus `Type::Tensor` dtype/shape. A MessagePack schema must
not invent a tensor-blob field unless a later unit proves it as a complete
projection of a new live fact.

### 14.7 Source bytes

The package stores `content_hash` / `source_hash`, not the author `.fab`
bytes. By design (addendum: package round-trips analyzed HIR, not author
source).

---

## 15. Reconstruction checklist (losslessness metric)

A FHIR MessagePack `value` is complete for this spec when decode yields:

1. Structurally equal `HirArtifact` (all 13 families, including every
   `HirItemKind` / `HirStatementKind` / `HirExpressionKind` / `Type` /
   `HirTrivia` variant listed above).
2. Structurally equal `FhirPackage` envelope (all 6 families, including
   both import-link tables).
3. `reconstruct_unit` equivalent to the postcard path (same arenas, same
   presentation, same facts).
4. `loaded_package_to_analyzed` equivalent to the postcard path (same
   module table, same local/library links, same per-unit artifacts).
5. Numeric widths and tensor dtype/shape present as type-table value
   fields, never inferred from opcodes.
6. Comments present on `HirPresentation` so canonical emit from the load
   remains as capable as in-session emit.

Proof of (1)–(6) is FS-3. Writer switch is FS-4.

---

## Source map

| Topic | Live type / function |
| --- | --- |
| Unit envelope | `radix_hir_fhir::HirArtifact` |
| Unit codec | `radix_hir_fhir::{encode, decode}` |
| Package envelope | `radix_hir_fhir::FhirPackage` |
| Package codec | `radix_hir_fhir::{encode_package, decode_package}` |
| Emit | `radix_module::hir::serialize::from_analyzed` |
| Unit load | `radix_module::hir::artifact::{load_fhir, reconstruct_unit}` |
| Package load | `radix_module::hir::package::{build_package, load_package}` |
| Program graph | `faber::package::fhir::loaded_package_to_analyzed` |
| Integrity | `radix_hir_fhir::validate::validate_referential_integrity` |
