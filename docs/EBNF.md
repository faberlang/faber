# Faber Language Specification

Formal grammar for the Faber programming language. This file is the canonical
grammar and spec-commentary surface for the public language; the compiler
(Radix) implements it. The rendered, localized grammar is published on
[the documentation site](https://faberlang.dev/en-US/reference/grammar.html).

Documentation contract: runnable language reference programs live in the public
sibling [`examples/corpus/`](../../examples/corpus/) with optional `+++`
frontmatter (`term`, `syntax`, `related`, …); the generated manifest is
[`examples/corpus/index.toml`](../../examples/corpus/index.toml). `faber
explain` loads the exempla reference pack from disk. Prefer the language corpus
+ EBNF for new reference work.

---

## Program Structure

Faber source files are raw text peeled by the driver before lexing. Optional TOML
frontmatter is not part of the token grammar. Within Faber syntax, spaces,
tabs, and newlines are trivia unless a production explicitly names `NEWLINE`.
Canonical forms are safe to compress onto one line. Any line-sensitive syntax is
explicitly sugar; a compressor must expand it when a lossless canonical mapping
exists, and otherwise preserve its boundary or reject compression. Line comments
remain line-oriented trivia and must be removed or relocated safely by a compressor.

```ebnf
fabFile       := frontmatter? program
frontmatter   := FRONTMATTER_DELIMITER NEWLINE TOML_LINES FRONTMATTER_DELIMITER NEWLINE?
program       := statement*
statement     := annotation* statementCore
statementCore := importDecl | bindingDecl | funcDecl | genusDecl | implendumDecl
               | typeAliasDecl | enumDecl | discretioDecl
               | ifStmt | whileStmt | iteraStmt
               | eligeStmt | discerneStmt | guardStmt | curaStmt | facBlockStmt
               | returnStmt | breakStmt | continueStmt | noopStmt | throwStmt
               | assertStmt | requiritStmt | outputStmt | incipitStmt | incipietStmt
               | extractStmt | probandumDecl | probaStmt | blockStmt
               | incDecStmt | exprStmt
bindingDecl   := varDecl | sitDecl | arrayDestruct | objectDestruct
exprStmt      := expression
blockStmt     := '{' statement* '}'
```

Uppercase names are lexical terminals. `FRONTMATTER_DELIMITER` is a line whose
trimmed content is exactly `+++`; `TOML_LINES` is the possibly empty sequence of
complete TOML lines before the closing delimiter. `NON_NEWLINE_TOKEN` means one
ordinary source token other than a newline. `ANNOTATION_NAME` and
`ANNOTATION_FIELD_NAME` are identifier spellings in annotation-owned contexts;
they include spellings that are keywords in other contexts. `NO_NEWLINE` is a
zero-width constraint requiring adjacent grammar parts to remain on the same
logical line.

### File frontmatter (`+++`)

When present, frontmatter must open on **line 1** with exactly `+++`. A later line
that trims to exactly `+++` ends the block. Bytes after the closing delimiter are
the Faber `program`. An empty body (whitespace only) is a valid empty program.

Frontmatter is parsed as a generic TOML document in the compiler driver — not
parsed as Faber statements. Authors may attach arbitrary metadata keys; tooling
reads known keys such as `group`, `sectio`, and `[probanda]` via accessors.
`faber` package tooling consumes those package keys. Package authority for
`[package]`, `[paths]`, and `[build]` remains `faber.toml`; conflicting
frontmatter values are rejected in package mode.

Example:

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

incipit {}
```

Line-start `§` file directives were removed. Put file metadata in `+++`
frontmatter instead. Inside quoted strings, `§` remains the string-template hole
(see **Call and Member Access** below).

### Comma separator law

Every comma position is either required or forbidden. Optional commas do not
exist.

**Item lists** — homogeneous entries inside a bounded header (`lista` literals,
call arguments, parameters, type argument lists, figura lists, field-init
lists, `ordo` members, `discretio` variant lists, JSON members and array
elements, annotation / import / nucleum fields, output statement lists) —
require a comma between adjacent items and forbid one after the last.

**Declaration blocks** — self-annotating declarations (statements, `genus`
members, `implendum` methods, `discretio` payload fields) — contain no commas.
Entries are trivia-delimited.

---

## Declarations

### Variables

```ebnf
varDecl      := ('fixum' | 'varia') typeAnnotation IDENTIFIER (('←' expression) | ('↤' assignment inlineRecovery?))?
awaitVarDecl := ('figendum' | 'variandum') typeAnnotation IDENTIFIER '←' expression
sitDecl      := 'sit' IDENTIFIER ('←' expression)?
arrayDestruct := ('fixum' | 'varia') arrayPattern '←' expression
objectDestruct := ('fixum' | 'varia') objectPattern '←' expression
```

- `fixum` = immutable binding (write-once): it may be declared without an
  initializer and assigned exactly once later, then frozen. `varia` = mutable
  binding (reassignable), like `let`.
- `figendum` / `variandum` await a `promissum<T>` or `promissum<T ⇥ E>`, bind
  the resolved `T`, and propagate a compatible alternate `E`.
- Use `_` as the type annotation when the initializer determines the type: `fixum _ name ← value`
- `sit name ← value` is sugar for `fixum _ name ← value` (inferred immutable local)
- `sit name` (no initializer) is sugar for `fixum _ name` — the inferred deferred
  immutable. Assign exactly once before any read.
- Typed `fixum`/`varia` initializers accept `↤` (`fixum numerus x ↤ "42"`):
  the written type is the conversion destination, then the binding is
  initialized. `figendum`/`variandum` keep `←`; `fixum _`, `sit`, and untyped
  destructuring reject `↤` (no concrete destination type).
- Deferred init: `fixum numerus x` or `sit x` declares an uninitialized immutable
  slot that must be assigned exactly once before any read; a second assignment is
  rejected. The definite-assignment pass (semantic Phase 3a) enforces this.

### Functions

```ebnf
funcDecl     := 'functio' IDENTIFIER genericParams? '(' paramList ')' funcModifier* callablePosture? returnClause? alternateExitClause? blockStmt
paramList    := (parameter (',' parameter)*)?
genericParams := '<' genericParam (',' genericParam)* '>'
genericParam  := IDENTIFIER | 'magnitudo' IDENTIFIER
callTypeArgs  := '<' typeAnnotation (',' typeAnnotation)* '>'
parameter    := 'ceteri'? typeAnnotation IDENTIFIER 'sponte'? ('ut' IDENTIFIER)? ('vel' expression)?
funcModifier := 'argumenta' IDENTIFIER | 'curata' IDENTIFIER ('ut' IDENTIFIER)? | 'errata' IDENTIFIER | 'exitus' (IDENTIFIER | NUMBER) | 'immutata' | 'iacit' | 'optiones' IDENTIFIER
callablePosture := 'fiet' | 'fiunt' | 'fient'
returnClause := '→' typeAnnotation
alternateExitClause := '⇥' typeAnnotation
stmtBodyJoint  := 'ergo'
clausuraJoint  := '∴'
clausuraExpr   := compactClausuraExpr | legacyClausuraExpr
compactClausuraExpr := clausuraSignature clausuraJoint (expression | closureFacBlock)
clausuraSignature := (clausuraParam | '(' clausuraParams? ')') returnClause? alternateExitClause?
closureFacBlock := 'fac' blockStmt catchClause?
legacyClausuraExpr := 'clausura' clausuraParams? ('→' typeAnnotation)? (':' expression | blockStmt)
clausuraParams := clausuraParam (',' clausuraParam)*
clausuraParam  := typeAnnotation IDENTIFIER
```

- Return syntax: `→` declares the normal success type. A bodyful function with no `→` is effect-only (`vacuum`) and must not contain `redde`. A statement-bodied closure (`fac { ... }` or legacy block body) must also spell `→ T` before it can use `redde`; expression-bodied closures may infer their result from the expression.
- Recoverable alternate-exit syntax: `⇥` declares the error-channel type. It can appear after `→ T` or alone on an effect-only failable function or closure. A closure body that uses an escaping `iace` must declare its own `⇥ E`; it cannot inherit the enclosing function's error channel. A local `fac { ... } cape err { ... }` may catch `iace` without an enclosing `⇥`. A failable function call (`→ T ⇥ E`) inside a `⇥`-declaring function propagates to the function's alternate exit without a `fac`/`cape` wrapper, mirroring how bare `↦` conversio and `iace` throws already behave; the call lowers to Rust `?`. A closure must still declare its own `⇥` to propagate a failable call — the enclosing function's error channel does not cross the closure boundary.
- Parameter access markers live in the type position: `de`/`ref` (read), `in`/`mut` (mutate), `own` (consume), and `copy` (duplicate then own). The retired parameter-prefix slot is not part of the grammar; `ex`/`from` remains the import/iteration/extraction token identity.
- Post-name marker: `sponte` (voluntary/optional provision)
- `ceteri` marks rest parameter
- `curata NAME ('ut' LOCAL)?` declares an allocator requirement; `LOCAL` is the function-body alias.
- Ordinary `functio` declarations and genus methods require bodies. Signature-only methods belong in `implendum`.
- `errata NAME` is a legacy runtime-injected `ignotum` local, and `iacit` is a legacy marker with no current semantic effect. Neither declares the typed alternate-exit contract. New failable APIs should use `⇥ E`; whether either legacy modifier should survive is unresolved.
- `ergo` is the compact **statement-body** joint only (one-statement `si`/`dum`/`casu`/… arms).
- `∴` is the compact **clausura** joint only. The two are not aliases.
- Compact closure block bodies must use `fac { ... }`; a closure-local `fac` body may attach `cape`, but cannot use postfix `dum`.

### Classes

```ebnf
genusDecl    := 'abstractus'? 'genus' IDENTIFIER genericParams? ('sub' IDENTIFIER)? ('implet' IDENTIFIER (',' IDENTIFIER)*)? '{' genusMember* '}'
genusMember  := annotation* (fieldDecl | methodDecl)
fieldDecl    := 'generis'? 'nexum'? typeAnnotation IDENTIFIER 'sponte'? ('=' expression)?
methodDecl   := 'functio' IDENTIFIER genericParams? '(' paramList ')' funcModifier* callablePosture? returnClause? alternateExitClause? blockStmt
```

### Annotations

```ebnf
annotation            := nucleumAnnotation | bracedAnnotation | annotationSugar
annotationName        := ANNOTATION_NAME
bracedAnnotation      := '@' annotationName '{' annotationFieldList? '}'
annotationFieldList   := annotationField (',' annotationField)*
annotationField       := ANNOTATION_FIELD_NAME '=' (expression | typeAnnotation)
annotationSugar       := '@' annotationName NON_NEWLINE_TOKEN* NEWLINE
nucleumAnnotation     := nucleumSugar | nucleumBraced
nucleumSugar          := '@' 'nucleum' nucleumModifier? NEWLINE
nucleumBraced         := '@' 'nucleum' '{' nucleumFieldList? '}'
nucleumModifier       := 'fragment'
nucleumFieldList      := nucleumField (',' nucleumField)*
nucleumField          := 'fragment' '=' ('verum' | 'falsum')
```

`@ nucleum fragment` is a modifier on the `nucleum` annotation (sugar or
braced `fragment = verum` / `falsum`), not a fused annotation name and not the
graphics `@ fragment` stage. Standalone `@ fragment` is unchanged.

Braced annotation records (`@ futura { }`, `@ optio { binding = verbose, ... }`)
are canonical and compression-safe. Unbraced annotations are line-sensitive,
non-compression-safe sugar that consumes through `NEWLINE`; the newline is part
of this sugar grammar, not a general Faber statement separator. A compressor may
rewrite promoted families only when their named-field mapping is known. It must
otherwise preserve the line break or reject compression. Promoted sugar and
braced forms lower to the same `HirAnnotation` records. Unpromoted positional
families preserve raw arguments and do not yet have a lossless braced expansion.

The current Radix parser still accepts only a fixed token subset in unbraced
payloads and ends them with declaration-boundary heuristics rather than `NEWLINE`.
Those are implementation mismatches with this specification, not alternate
language rules.

**Annotation contracts:** `@ annotatio` (optionally `@ annotatio { target = functio }`)
marks a top-level `genus` as a compile-time annotation contract. Ordinary genera
are not annotation schemas. Applications use `@ ContractName { field = constant }`
and resolve through local declarations or imported file-interface exports.
Resolved applications lower to `HirAnnotation` with `contract_id: Some(DefId)`
and constant field values. v1 attachment target is `functio` only; payload
scalars are `textus`, `numerus`, `fractus`, and `bivalens` (optional via
`sponte` or `T ∪ nihil`). No compiler-owned `@ web` / controller / route families.

**JSON genera:** `@ json` on a `genus` is a compiler-owned data-model contract,
not a generic annotation schema. Fields must be JSON-safe (`textus`, `ascii`,
`numerus`, `fractus`, `bivalens`, `instans`, `nihil`, `lista<T>`,
`tabula<textus, T>`, nullable `T ∪ nihil`, or another `@ json genus`). Field
metadata `@ json { nomen = "wire_name" }` changes the emitted object key used by
`value ↦ valor`, `value ↦ json`, and `json ↦ Genus`; JSON text remains a Norma
wire operation such as `json.pange(value ↦ json)`.

- `@ radix` is reserved for compiler-owned metadata. The historical
  morphology-stem meaning is retired; morphology remains a source naming
  discipline, not compiler-generated conjugation. Accepted directive forms are
  `@ radix lane "air"` / `"mir"` / `"hir-direct"` on top-level functions for
  explicit compiler-lane routing; unsupported lane/target combinations reject
  with diagnostics instead of being ignored.
- `@ verte` defines codegen transformation (method name or template)
- `@ nondum [TARGET] ["REASON"]` marks a declaration as present in an interface but unavailable for the target
- `@ cli "NAME"` marks an `incipit` entry as a CLI program
- `@ imperium "NAME"` marks a function as a CLI command entry point
- `@ optio NAME ...` defines a CLI option; use `typus bivalens` for boolean flags
- `@ operandus [ceteri] TYPE NAME ...` defines a CLI positional argument
- `@ futura` marks a function as async (legacy — prefer `fiet` posture word)
- `@ cursor` marks a function as generator (legacy — prefer `fiunt` posture word)
- Callable posture words (`fiet`/`fiunt`/`fient`) are recognized in the signature
  slot after modifiers and before `→`/`⇥`/body; bare means synchronous finite
- `@ publica` marks a declaration for the file's importable (export) surface; `@ interna` marks it package-internal (same-package importable only); `@ privata` is an explicit module-private marker. Unmarked top-level declarations are module-private by default; a declaration mixing distinct visibility tiers is rejected with `SEM019` (`conflicting_visibility`)
- `@ protecta` is reserved and rejected with a semantic diagnostic; it has no package, subclass, or sibling-file visibility meaning

- `sub` = extends, `implet` = implements
- `generis` = static, `nexum` = bound/property

### Interfaces

```ebnf
implendumDecl   := 'implendum' IDENTIFIER genericParams? '{' implendumMethod* '}'
implendumMethod := annotation* 'functio' IDENTIFIER '(' paramList ')' funcModifier* callablePosture? returnClause? alternateExitClause?
```

`implendum` is the **contract** construct: signature-only methods for `implet`
(gerundive of *implere* — that which must be fulfilled). Import namespaces are
`.fab` file boundaries; exported declarations live at file top level.

### Type Aliases

```ebnf
typeAliasDecl := 'typus' IDENTIFIER genericParams? '=' typeAnnotation
```

### Enums

```ebnf
enumDecl   := 'ordo' IDENTIFIER '{' enumMember (',' enumMember)* '}'
enumMember := IDENTIFIER ('=' ('-'? NUMBER | STRING))?
```

### Tagged Unions

```ebnf
discretioDecl := 'discretio' IDENTIFIER genericParams? '{' variant (',' variant)* '}'
variant       := IDENTIFIER ('{' variantFields '}')?
variantFields := (typeAnnotation IDENTIFIER)*
```

Variant lists are an item list: comma required between variants, forbidden
after the last. Payload fields inside a variant are a declaration block
(genus-style, no commas).

### Identifier Naming

Faber has no globally reserved words. Keyword ownership is contextual per
spelling: a keyword claims only its owning grammar slot. Every user-chosen
name slot accepts every keyword spelling — declaration names, parameters,
members, binding targets (`fixum`/`varia`/`sit` patterns and captures),
import aliases, and loop/iteration bindings. Type-name slots stay out.

Outside a spelling's owning contexts, that spelling may be an `IDENTIFIER`.
An owning context may itself be effectively global when its production
applies everywhere a statement or expression may begin. Builtin claims
(`lege`/`lineam`/`scriptum`/`vacua`, and the scribe family in
statement-initial position) are defaults, not reservations: a user binding
of the same surface spelling wins.

Radix still emits globally reserved tokens for some spellings and selectively
reinterprets them as identifiers. That is transitional implementation behavior;
it does not replace the contextual language rule above.

Mixed-case lower-initial names are syntactically accepted but not
Faber-preferred for language, stdlib, host routes, or compiler-owned intrinsic APIs.
Prefer one word. If one word cannot carry the meaning, use snake_case only in
rare cases. If neither shape works, the method probably does not belong in the
core surface unless it is critical. Stdlib encode/decode uses the
mechanical verb trio `pange` / `solve` / `tempta` across modules — see
`docs/stdlib/stdlib-mechanical-verbs.md`. The public text library is
`norma:chorda` — see `docs/stdlib/chorda-methods.md`.

### Imports

```ebnf
importDecl     := importRecord | importSugar
importRecord   := 'importa' '{' importFieldList? '}'
importFieldList := importField (',' importField)*
importField    := importSourceField | importVisibilityField | importNameField
                | importAliasField | importWildcardField
importSourceField := 'ex' '=' STRING
importVisibilityField := 'visibilitas' '=' visibility
importNameField := 'nomen' '=' IDENTIFIER
importAliasField := 'ut' '=' IDENTIFIER
importWildcardField := 'omnia' '=' IDENTIFIER

importSugar    := 'importa' 'ex' STRING visibility? (namedImport | wildcardImport)?
visibility    := 'publica'
namedImport   := IDENTIFIER ('ut' IDENTIFIER)?
wildcardImport := '*' 'ut' IDENTIFIER
```

Example:

```fab
importa ex "hono" Hono
importa ex "hono" Context
# No marker: no re-export.
importa ex "norma:chorda"
importa { ex = "norma:json/solve", ut = solve_mod }
importa ex "norma:consolum" consolum
# Kernel manifest glob.
importa ex "faber:*" faber
importa ex "lodash" * ut _
# Re-export.
importa ex "./types" publica User
```

The `privata` import marker was removed (VM-U3); an import without a marker
does not re-export, and `publica` is the re-export marker. Missing named binding
defaults to the
last import path segment when it is a valid, non-conflicting identifier. If the
inferred name is invalid or collides with an existing top-level binding, spell an
explicit `nomen` or `ut` binding.

`importa ex "faber:*" faber` is kernel-specific sugar: the glob lives
inside the import path string and expands the released binary's kernel manifest
into `faber.<module>.<verb>` calls. It is not a wildcard re-export and does not create a runtime aggregate value.

---

## Types

```ebnf
typeAnnotation := ownedType ('∪' ownedType)*
ownedType      := ('de' | 'in' | 'own' | 'copy')? baseType
baseType       := holeType | functionType | widthTypeSugar | qualifiedType typeArguments? | '(' typeAnnotation ')'
holeType       := '_' | '∪'
qualifiedType  := IDENTIFIER ('.' IDENTIFIER)*
typeArguments  := '<' typeArgument (',' typeArgument)* '>'
typeArgument   := labeledTypeArgument | typeAnnotation | NATURAL | '[' figuraList? ']'
labeledTypeArgument := IDENTIFIER ':' typeAnnotation
widthTypeSugar := WIDTH_MARKER | LISTA_WIDTH_SUGAR
                | (TENSOR_WIDTH_SUGAR | SPARSA_WIDTH_SUGAR | VECTOR_WIDTH_SUGAR) shapeSuffix?
                | MATRIX_WIDTH_SUGAR shapeSuffix
shapeSuffix    := '[' figuraList? ']'
figura         := '_' | NATURAL | IDENTIFIER | '[' figuraList? ']'
figuraList     := figura (',' figura)*
functionType   := '(' typeList? ')' '→' typeAnnotation alternateExitClause?
typeList       := typeAnnotation (',' typeAnnotation)*
```

- Declaration parameters (`genericParams`) and applied arguments (`typeArguments`) are distinct grammar categories. Applied arguments admit nested types and static `figura` values. `typeArguments` still admits `NATURAL`.
- Applied `NATURAL` arguments are `magnitudo` capacity facts, not width markers. Proposed (not shipped) bounded forms use that slot: `lista<T, N>`, `textus<N>`, `ascii<N>`, `octeti<N>`. Width-marker families such as `numerus<i32>` stay the separate `widthTypeSugar` production below.
- A second applied argument on a `↦` target (`numerus<W, Hex>`, `numerus<W, Be>`) is a convert-slot hint, not a type identity, not a width marker, and not a keyword. Live text-parse hints are `Hex` / `Bin` / `Oct`. `Be` / `Le` occupy that same Hex slot for endian unpack. `typeArguments` is unchanged: these are ordinary `IDENTIFIER` arguments interpreted by conversio, not new `baseType` productions.
- Type arguments admit the hole forms: `lista<∪>` infers a heterogeneous element union and `tabula<K, ∪>` a heterogeneous value union; `lista<_>` keeps the monomorphic single-inhabitant hole.
- Explicit generic call-site lists use the same `typeArguments` production: `id<_>(x)` is a type hole (equivalent to omitted `id(x)` for a one-param callee), and mixed lists such as `both<_, textus>(a, b)` are legal. Arity stays exact (`both<_>` is still one argument). `∪` in that list is rejected (`explicit_union_type_arg_unsupported`): a callee type param is a monomorphic witness slot.
- `labeledTypeArgument` is the optional label prefix on `iuncta` type arguments only (`iuncta<gx: f32, T>`; mixed labeled/unlabeled legal). A label in a non-`iuncta` list (`f<gx: T>(x)`, `lista<gx: T>`) is a parse error. Absence is the only unlabeled form; there is no `_: T` spelling. Keyword spellings are legal labels under the contextual law (`iuncta<fixum: A>`).
- Labels are unique within one tuple type.
- Labels are erased from type identity: `iuncta<gx: A, B> ≡ iuncta<A, B>` for assignment, `≡`/`↦`, unify, and every emitter.
- Bracket index on a tuple requires a literal integer (`i[0]`); every element is reachable by position, labeled or not. Non-literal index expressions stay rejected. Positions are brackets only — no `.0`.
- Member-by-label (`i.gx`) requires that label to be present on the receiver's `iuncta` annotation.
- `iuncta` element slots admit `_` (monomorphic hole, solved element-wise from the single position witness) and reject `∪`. A wanted union element is declared with binary cup (`iuncta<f32, textus ∪ nihil>`). `lista<∪>` / `tabula<K, ∪>` keep heterogeneous-union behavior. Labels compose with holes (`iuncta<loss: _, T>`).
- Arrays are written `lista<T>` (unbounded, shipped). Postfix `T[]` is not accepted. `lista<T, N>` is a proposed (not shipped) bounded form; see Generic Collections.
- `de`/`in` mark ownership (borrow/mut-borrow) on the immediately following union member. Parenthesize when grouping must be explicit.
- Two hole kinds share the `holeType` production. `_` is the monomorphic hole ("infer exactly one inhabitant type"); the standalone `∪` is the union hole ("infer a finite multi-member union"). Both are legal wherever a base type is: bindings, returns, params, fields, and type arguments (`lista<∪>`, `tabula<K, ∪>`, `→ ∪`).
- **Lone-`∪` rule:** a `∪` hole consumes the whole type expression — any following `∪` is a parse error (`A ∪ ∪`, `∪ B` rejected, issue `unexpected_cup_after_union_hole`). `_` keeps today's behavior and may still appear as a binary-cup member (`_ ∪ B`).
- **Binary-cup disambiguation:** `∪` between two non-hole types remains the inline value-union operator (`A ∪ B`, nullable `T ∪ nihil`); the hole reading applies only when `∪` stands alone in a base-type position.
- Inline union `T ∪ U` (cup) for ad-hoc value unions; `T ∪ nihil` is the canonical nullable type form (lowers to Option<T>).
- Unions are parsed as a flat member list; duplicates and `nihil`-only cases are diagnosed in semantic lowering.
- `sponte` is a declaration marker (post-name on params/fields), never a prefix on types.
- Qualified type paths such as `terminus.Terminus` name a type through an
  imported namespace binding. The prefix must resolve to a namespace; the final
  segment must resolve to a type-bearing declaration.

Function types enable higher-order function signatures:

```fab
functio filtrata((T) → bivalens pred) → lista<T>
functio compose((A) → B f, (B) → C g) → (A) → C
functio apply((numerus) → numerus ⇥ textus op, numerus n) → numerus ⇥ textus
```

### Primitive Types

| Faber      | Meaning |
| ---------- | ------- |
| `textus`   | Unicode string |
| `textus<N>` | proposed — not shipped; bounded Unicode string; `N` is a `magnitudo` / `NATURAL` capacity, not a width marker. `textus<_>` is the capacity hole (infer `N`). |
| `ascii`    | ASCII-only string |
| `ascii<N>` | proposed — not shipped; bounded ASCII string; `N` is a `magnitudo` / `NATURAL` capacity, not a width marker. `ascii<_>` is the capacity hole (infer `N`). |
| `forma`    | captured template + params |
| `numerus`  | integer (default `i64`) |
| `modulus<W>` | unsigned modular word; arithmetic wraps modulo 2^W |
| `fractus`  | float (default `f64`) |
| `bivalens` | boolean |
| `nihil`    | null |
| `vacuum`   | void |
| `numquam`  | never |
| `ignotum`  | unknown |
| `octeti`   | bytes |
| `octeti<N>` | proposed — not shipped; bounded byte buffer; `N` is a `magnitudo` / `NATURAL` capacity, not a width marker. `octeti<_>` is the capacity hole (infer `N`). |

Bare `textus` / `ascii` / `octeti` remain the unbounded productions. The
proposed (not shipped) forms `textus<N>`, `ascii<N>`, and `octeti<N>` take
one `magnitudo` / `NATURAL` applied argument. That `N` is capacity, not a
width marker and not a language-wide default. `_` in that slot (`ascii<_>`,
`textus<_>`, `octeti<_>`, `lista<T, _>`) is a capacity hole: the form stays
bounded, and `N` is inferred from a same-family bounded witness. Bare
`ascii` is not a hole.

Sized primitives accept one optional **width marker** (not a user type parameter):

| Family | Markers | Invalid example |
| ------ | ------- | --------------- |
| `numerus<W>` | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64` | `numerus<f32>` → use `fractus<f32>` |
| `fractus<W>` | `f16`, `f32`, `f64` | `fractus<i32>` → use `numerus<i32>`; `bf16` is deferred |
| `modulus<W>` | `u8`, `u16`, `u32`, `u64` | `modulus<i32>` → signed widths are not modular words |

Bare `numerus` / `fractus` remain shorthand for `numerus<i64>` / `fractus<f64>`.
`numerus<_>`, `fractus<_>`, `modulus<_>`, and `instans<_>` are marker holes:
the family stays identity and only the width/precision is inferred from a
same-family witness (exact marker, no lattice widening). Unsolved `_` is an
error, never the bare default. Convert-hint holes (`numerus<u32, _>`) are
not this form.

`modulus<W>` is a distinct semantic family: arithmetic does not mix implicitly
with `numerus<W>`, while explicit same-width conversion remains available.
Literals must be in `0..=2^W-1` (for `modulus<u64>` up to
`18446744073709551615`). Shift counts are themselves modular: `x ⇐ W` is a
full wrap. Cross-width modular arithmetic is rejected.

### Generic Collections

| Faber          | Meaning  |
| -------------- | -------- |
| `lista<T>`     | array    |
| `lista<T, N>`  | proposed — not shipped; bounded array; `N` is a `magnitudo` / `NATURAL` capacity, not a width marker. `lista<T, _>` is the capacity hole (infer `N`). |
| `tabula<K,V>`  | map      |
| `copia<T>`     | set      |
| `promissum<T>` | promise  |
| `cursor<T>`    | iterator |
| `tensor<T, Figura>` | dense homogeneous buffer with static shape `Figura`; numeric methods require numeric element types |
| `vector<T, N>` | register-class numeric vector with static width `N` (single dimension, not buffer-backed) |
| `matrix<T, [R, C]>` | register-class numeric matrix with exactly two static dimensions (not buffer-backed and not a tensor alias) |
| `atomic<T>` | storage-sensitive atomic cell; v1 accepts `i32` / `u32` elements only and access must go through atomic methods |
| `sparsa<T, Figura>` | sparse homogeneous buffer with static shape `Figura`; omitted coordinates equal zero; numeric methods require numeric element types |

A `figura` is `_`, a natural number, a size identifier, or a bracketed list of nested figura values; empty `[]` is rank-0. Bare `tensor<T>` is incomplete — use `tensor<T, []>` for rank-0 or `tensor<T, _>` to infer shape.

`vacua` for `tensor<T, []>` produces a rank-0 tensor (one default-initialized element slot).
`vacua` for `sparsa<T, Figura>` (any shape) produces an all-zero sparse tensor with no stored entries.
`matrix<T, Figura>` requires exactly two dimensions; bare `matrix<T>` and one- or three-axis matrix shapes are rejected.
`atomic<T>` requires `T` to be `i32` or `u32` in v1. Atomic cells are not interchangeable with their element type; use `load`, `store`, `exchange`, and `compare_exchange` receiver methods.
Construct multi-dimensional tensors via `crea` / `structa` / `↦`.
`Type(...)` is not a construction form: `vector<f32, 4>(...)`, `matrix<f32, [2, 2]>(...)`, `tensor<f32, [2, 2]>(...)`, and scalar forms such as `numerus("42")` are rejected. Use `value ↦ Type`, named library constructors, or `Genus { field = value }` records.

Tensor index/shape intrinsic slots (`accipe`, `ponde`, `forma`, `crea`, `structa`) accept integer lists that fit the canonical `lista<numerus>` / `&[i64]` runtime boundary at call sites (e.g. `lista<u32>` for GPU thread ids; not `lista<u64>`). This is a structural exception scoped to those slots — it does not widen the signed↔unsigned numeric lattice (see Index vector parameter policy in `tensor-intrinsics.md`).

Value unions use inline `T ∪ U` (nullable: `T ∪ nihil`). The standalone `∪` hole infers a multi-member union; `_` infers a single inhabitant (see `docs/design/type-hole-union.md`). Tagged unions use `discretio`.
`copia.unio()` is a set method, not a type constructor.

### Type Sugar

Explicit long forms such as `numerus<u32>` and `lista<numerus<u32>>` are the
canonical spellings. Type sugar is an ergonomic alternate spelling for numeric
and collection types. It is **type-position only** and **semantically identical**
to the long form — the compiler treats both the same. This is the single
canonical reference for sugar; the rest of the specification uses long form.

Sugar combines a width marker with an optional one-letter family prefix. Width
markers are `i8`/`i16`/`i32`/`i64` (signed), `u8`/`u16`/`u32`/`u64` (unsigned),
and `f16`/`f32`/`f64` (float). A bare width marker (no prefix) sugars the scalar
numeric type; a family prefix sugars a collection of that width. In the grammar,
`WIDTH_MARKER` is a bare marker; `LISTA_WIDTH_SUGAR`, `TENSOR_WIDTH_SUGAR`,
`SPARSA_WIDTH_SUGAR`, `VECTOR_WIDTH_SUGAR`, and `MATRIX_WIDTH_SUGAR` are that
marker prefixed with `l`, `t`, `s`, `v`, and `m`, respectively.

| Sugar | Long form | Bracket rule |
| ----- | --------- | ------------ |
| `i8` … `u64`, `f16`/`f32`/`f64` | `numerus<W>`, `fractus<W>` | none (bare marker) |
| `lf32`, `lu32`, `li64`, … | `lista<f32>`, `lista<u32>`, `lista<i64>`, … | none |
| `tf32`, `tf32[2, 3]`, `ti64[N]` | `tensor<f32, _>`, `tensor<f32, [2, 3]>`, `tensor<i64, [N]>` | optional `Figura` |
| `sf32`, `sf32[2, 3]`, `si64[N]` | `sparsa<f32, _>`, `sparsa<f32, [2, 3]>`, `sparsa<i64, [N]>` | optional `Figura` |
| `vf32`, `vf32[4]`, `vu32[3]` | `vector<f32, _>`, `vector<f32, 4>`, `vector<u32, 3>` | optional single width |
| `mf32[4, 4]`, `mf16[2, 2]`, `mu32[3, 3]` | `matrix<f32, [4, 4]>`, `matrix<f16, [2, 2]>`, `matrix<u32, [3, 3]>` | **required**, two dimensions |

Bracket shapes: `[]` is rank-0, `[2, 3]` is a fixed shape, and no bracket infers
the shape (`_`). Matrix requires exactly two dimensions. Sugar never uses `<>`.
For non-width element types (e.g. `tensor<textus, [3]>`), use the full form.

Sugar is reserved in type syntax only — value identifiers named `tf32`, `lf32`,
etc. are unchanged.

`modulus<W>` has no sugar; write `modulus<u32>` in full.

**Spelling preference (author convention, not grammar):** general Faber code
tends toward long form for readability; numeric/tensor-primary modules may
prefer sugar. Choose per module or file.

---

## Control Flow

### Conditionals

```ebnf
ifStmt     := 'si' expression arm ('sin' ifStmt | elseClause)?
elseClause := 'secus' elseArm
arm        := (blockStmt | stmtBodyJoint statement) catchClause?
elseArm    := (blockStmt | stmtBodyJoint statement) catchClause?
```

- `si` = if, `sin` = else-if, `secus` = else
- `ergo` for one-statement bodies, including `ergo redde`, `ergo iace`, `ergo mori`, and `ergo tacet` (`∴` is not accepted here)
- `tacet` for explicit no-op (from musical notation: "it is silent")

### Loops

```ebnf
whileStmt  := 'dum' expression (blockStmt | stmtBodyJoint statement) catchClause?
iteraStmt  := 'itera' (('ex' | 'de') expression | 'ab' expression) ('fixum' | 'varia') IDENTIFIER (blockStmt | stmtBodyJoint statement) catchClause?
```

- `dum` = while
- `itera ex...fixum`/`itera ex...varia` = for-of (values)
- `itera de...fixum`/`itera de...varia` = for-in (keys)
- `itera ab range fixum/varia i` = range iteration (e.g. `itera ab 0‥10 per 2 fixum i { nota i }`; `per` belongs to the range expression)

### Switch/Match

```ebnf
eligeStmt    := 'elige' expression '{' eligeCase* defaultCase? '}' catchClause?
eligeCase    := 'casu' expression (blockStmt | stmtBodyJoint statement)
defaultCase  := 'ceterum' (blockStmt | stmtBodyJoint statement)
```

### Pattern Matching

```ebnf
discerneStmt := 'discerne' 'omnia'? discriminants '{' variantCase* defaultCase? '}'
discriminants := expression (',' expression)*
variantCase  := 'casu' patterns (blockStmt | stmtBodyJoint statement)
patterns     := pattern ((',' | 'et') pattern)*
pattern      := '_' | literal | (IDENTIFIER patternBind?)
patternBind  := ('ut' IDENTIFIER) | (('fixum' | 'varia') patternBinding (',' patternBinding)*)
patternBinding := IDENTIFIER ('ut' IDENTIFIER)?
```

### Guards

```ebnf
guardStmt   := 'custodi' '{' guardClause+ '}'
guardClause := 'si' expression (blockStmt | stmtBodyJoint statement)
```

### Resource Management

```ebnf
curaStmt    := 'cura' STRING ('fixum' | 'varia') typeAnnotation IDENTIFIER blockStmt catchClause?
```

### Destructuring Extraction

```ebnf
extractStmt   := 'ex' expression ('fixum' | 'varia') extractFields
extractFields := extractField (',' extractField)* (',' restField)? | restField
extractField  := IDENTIFIER ('ut' IDENTIFIER)?
restField     := 'ceteri' IDENTIFIER
```

### Control Transfer

```ebnf
returnStmt   := 'redde' expression?
returnAwaitStmt := 'reddet' expression
awaitDiscardStmt := 'tacebit' expression
yieldStmt    := 'cede' expression
breakStmt    := 'rumpe'
continueStmt := 'perge'
noopStmt     := 'tacet'
```

- `reddet` awaits a compatible promise and returns its success value from a
  `fiet` function.
- `tacebit` awaits a compatible promise to completion and discards any success
  value.
- `cede` is statement-initial yield from `fiunt` / `fient`; it is not an
  expression-form await.

---

## Error Handling

```ebnf
throwStmt         := bareThrow | guardedThrowSugar
bareThrow         := ('iace' | 'mori') expression
guardedThrowSugar := ('iace' | 'mori') expression NO_NEWLINE 'si' expression
catchClause       := 'cape' IDENTIFIER blockStmt
assertStmt        := 'adfirma' expression ('mori' expression)?
requiritStmt      := 'requirit' expression 'iace' expression
```

- `cape` attaches to the structured forms whose productions name `catchClause`: conditional arms, `dum`, `itera`, `elige`, `cura`, and `fac`. It does not attach to arbitrary bare blocks.
- Use the explicit do block when a standalone block needs a handler: `fac { ... } cape err { ... }`.
- `iace` = throw (recoverable), `mori` = panic (fatal).
- A same-line `si <expr>` guard on `iace` and `mori` is line-sensitive parser sugar: `iace val si cond` desugars to `si cond { iace val }` at parse time. Its canonical, compression-safe spelling is the expanded `si` block. A source compressor must expand this sugar before removing line breaks; the guarded shorthand remains under language review.
- `adfirma` is a runtime invariant check. It desugars conceptually to `mori "msg" si !cond`, with the positive condition kept in source form and the inversion applied during lowering. The optional particle is `mori` (en `panic`): `adfirma cond mori msg` / `assert cond panic msg`. Bare `adfirma cond` stays legal. An `adfirma` failure is fatal and uncatchable by `cape` (it lowers to a panic, not a `Result`-channel error); in test context the harness isolates each `proba` so a failed assertion ends that test without ending the suite.
- `requirit` is the recoverable require statement (en surface `require … throw …`), the typed-error-channel twin of `adfirma`. `requirit cond iace err` desugars to `si non (cond) { iace err }` at lowering; the thrown value enters the function's `⇥ E` channel and is catchable by `cape`/`fac`, unlike `adfirma` (fatal). A `requirit` statement in a `⇥`-less function is a compile error, same as `iace`. The particle is `iace` (en `throw`) and is required.

---

## Expressions

### Operators (by precedence, lowest to highest)

```ebnf
expression := assignment
assignment := ternary ('←' assignment | '↤' assignment inlineRecovery?)?
incDecStmt := place ('↑' | '↓')
place      := call  (* semantic analysis requires an assignable target *)
ternary    := or (('?' expression ':' | 'sic' expression 'secus') ternary)?
or         := and (('aut') and)*
and        := equality (('et') equality)*
equality   := comparison equalityTail*
equalityTail := ('≡' | '≠' | '≈' | '≉' | 'est' | 'non' 'est') comparison
comparison := bitwiseOr (('≺' | '≻' | '≤' | '≥' | 'intra' | 'inter') bitwiseOr)*
# Ordering operators use the canonical Unicode glyphs `≺`/`≻`/`≤`/`≥`;
# membership uses Latin keywords `intra`/`inter` (Faber prose identity).
# Glyph aliases such as `∈` are not in the active contract.
bitwiseOr  := bitwiseXor ('∨' bitwiseXor)*
bitwiseXor := bitwiseAnd ('⊻' bitwiseAnd)*
bitwiseAnd := shift ('∧' shift)*
shift      := range (('⇐' | '⇒') range)*
range      := additive rangeTail?
rangeTail  := ('‥' | '…' | 'ante' | 'usque') additive ('per' additive)?
additive   := multiplicative (('+' | '-') multiplicative)*
# Glyph products (`·` `×` `⊗` `⊙`) bind at the multiplicative level with `*`,
# left-associative. `·` inner product dispatches dot/matvec/matmul by rank.
multiplicative := coalesce (('*' | '/' | '%' | '·' | '×' | '⊗' | '⊙') coalesce)*
# `vel` is local nullable elimination (`T ∪ nihil vel T → T`), not logical `aut`.
# It binds tighter than arithmetic so `prefix + item vel ""` is `prefix + (item vel "")`.
# `velRhs` greedily consumes a following range tail, so `a vel b‥c` is `a vel (b‥c)`.
coalesce   := unary ('vel' velRhs)*
velRhs     := unary velRangeTail?
velRangeTail := ('‥' | '…' | 'ante' | 'usque') unary ('per' unary)?
unary      := ('-' | '¬' | 'non') unary | fingeExpr | cast
# `∇` is a structural POSTFIX gradient-selection operator, never a prefix unary
# form. It binds in the expression postfix fold after calls have been formed,
# and its optional selector bracket is consumed as part of the gradient suffix
# before ordinary postfix continuation resumes. The concrete parser accepts an
# ordinary expression as the left side; semantic analysis must require it to
# resolve to an eligible differentiable forward call.
gradientExpr := call ('∇' gradientSelection?)?
gradientSelection := '[' gradientPlace (',' gradientPlace)* ']'
gradientPlace := expression  (* semantic analysis requires a direct call argument place *)
cast       := gradientExpr ('∷' typeAnnotation | conversio)*
conversio        := '↦' typeAnnotation inlineRecovery?
inlineRecovery   := '⇥' unary
```

**Conversion-directed assignment (`↤` / conversio-assign):** `place ↤ value`
evaluates the right side, converts it to the statically known type of the left
place through the existing `↦` route, then assigns. It binds at the same
precedence as `←` and is right-associative; `⇥ inlineRecovery` is **legal only
on `↤`** — a `⇥` recovery after ordinary `←` is rejected, and in a
right-associated `↤` chain the recovery attaches to the nearest `↤`. The
operator is preserved verbatim through syntax and emission; it is never
rewritten to `←` or `↦`. Typed `fixum`/`varia` initializers accept `↤`
(convert to the written type, then initialize); `fixum _`, `sit`, and untyped
destructuring have no concrete destination and are rejected.

`est` and `non est` inspect an existing value; they never convert it. Core type
spellings on the right perform runtime variant/type tests, while `nihil`,
`verum`, `falsum`, and ordinary value expressions use the value-test path. Radix
currently recognizes type targets through a fixed core-type vocabulary. Extending
that recognition to arbitrary declared types is a separate language decision.
Use `≡` / `≠` for structural value equality and `↦` for runtime conversion.

Retired predicate keywords are not prefix unary syntax. Use `expr est verum`,
`expr est falsum`, `expr est nihil`, `expr non est nihil`, `expr ≺ 0`, or
`expr ≻ 0`.

**Static type ascription (`∷` / verte):**

The `∷` glyph (U+2237, "proportion") explicitly ascribes a target type to an expression. Use it when the source expression already exists and the compiler needs a static target shape:

- Primitive/alias → cast (no runtime effect): `data ∷ textus` → TypeScript: `(data as string)`
- Built-in collection → target-shaped collection value: `[1, 2, 3] ∷ lista<numerus>`
- Variant expression → enum/interface target ascription: `finge Click { x = 10 } ∷ Event`

Prefer typed construction for ordinary `genus` values and `vacua` for ordinary empty collection values:

```fab
fixum _ point ← Point { x = 10 }
fixum lista<numerus> xs ← vacua
```

Only the `∷` glyph is accepted as the postfix static type-ascription operator. The Latin forms `qua`, `innatum`, and `novum` were aliases and have been removed (see verte-alias-clean-break).

**Runtime conversion (`↦` / conversio):**

The `↦` glyph (U+21A6, "rightwards arrow from bar") is the runtime value conversion operator. Unlike `∷` (compile-time cast), this performs actual parsing/conversion that can fail:

- `"22" ↦ numerus` → Rust: `"22".parse::<i64>().unwrap()`
- `"bad" ↦ numerus ⇥ 0` → Rust: `"bad".parse::<i64>().unwrap_or(0)`
- `42 ↦ textus` → Rust: `42.to_string()`

The second type argument of a `↦` target is the convert-hint slot. `Hex` / `Bin` / `Oct` / `Be` / `Le` are convert hints in that slot, not keywords and not new `baseType` productions. Target support is not a grammar production (see Target Support).

- `"ff" ↦ numerus<i32, Hex>` — shipped; text parse at radix 16 (`Bin` = 2, `Oct` = 8). Hex/Bin/Oct text parse is unchanged by endian hints.
- `octeti[lo‥hi] ↦ numerus<W, Be>` / `… ↦ numerus<W, Le>` — endian unpack of an exact-width window (`W` is `i16` / `i32` / `i64` / `u16` / `u32` / `u64`; window length 2 / 4 / 8). Shipped on rust, the MIR runner, Go, and TypeScript. TypeScript `i64`/`u64` stay fail-closed (JS number is not exact). English `int<W, Be>` is the same form. `octeti` itself has no endian; `bytes ↦ numerus<u32>` without `Be`/`Le` stays rejected. A short window fails (no pad).
- `n ↦ octeti<N, Be>` / `… ↦ octeti<N, Le>` — proposed (not shipped); write convert after `octeti<N>` (`N` ∈ {2, 4, 8}). `Be`/`Le` stay Hex-slot hints, not a second capacity.

Inline failure recovery uses `⇥` immediately after the conversio target (`↦ T ⇥ recovery-expr`). The unparenthesized recovery operand is a unary-precedence expression; parenthesize arithmetic, coalescing, ternary, or assignment recovery expressions. The recovery value must have type `T`.

Using `vel` as conversio recovery is rejected with a migration diagnostic. `vel` is local nullable elimination only (`x vel y`, parameter defaults) — not logical `aut`. A parenthesized conversio result may still combine with `vel` as ordinary defaulting.

### Call and Member Access

```ebnf
call          := primary (callSuffix | memberSuffix | optionalSuffix | nonNullSuffix)*
callSuffix    := callTypeArgs? '(' argumentList ')'
memberSuffix  := '.' IDENTIFIER | '[' expression ']'
optionalSuffix := '?.' IDENTIFIER | '?[' expression ']' | '?(' argumentList ')'
nonNullSuffix := '!.' IDENTIFIER | '![' expression ']' | '!(' argumentList ')'
argumentList  := (argument (',' argument)*)?
argument      := 'sparge'? expression
```

### String And Template Literals

Faber uses **delimiter semantics**: each quote form means a different source shape.
They are not interchangeable synonyms.

| Form | Type | Role |
| --- | --- | --- |
| `'...'` | `ascii` | fixed machine tokens; no `§`; no `(...)` |
| `"..."` | `textus` | short Unicode line strings; `(...)` renders |
| `«...»` | `textus` | block/multiline Unicode; `(...)` renders |
| `` `...` `` | `forma` | captured templates; `(...)` captures |
| `{ ... }` | `json` | compile-time object-rooted JSON document (`:` inside) |
| `\|...\|` | `octeti` | compile-time hex bytes |
| `"..." ↦ regex` | `regex` | compiled pattern from text conversion |
| `[ ... ]` | `lista<T>` | Faber list (not JSON array, not bytes) |

`§` (U+00A7) is a template hole in Unicode forms (`"`, `«`, `` ` ``). It cannot
appear in `ascii` literals.

**Rendered templates** (`textus`): `"..."(...)` and `«...»(...)` lower to
`scriptum("...", args...)`.

**Captured templates** (`forma`): `` `...`(args) `` captures template text and
parameters without rendering. Safe for bound SQL/URL payloads; do not use
`«...»(...)` for that job.

Block `textus` uses guillemets `«...»`. The heavy quotation-mark
pair is retired (too visually close to `"` in many fonts).

Implementation status (2026-06-30):

- Shipped: `"..."`, `«...»` block `textus`, `'...'` → `ascii`, `` `...` `` → `forma`, `|...|` → `octeti`, `{ ... }` → `json`, and text/ascii `↦ regex`.
- Pending factory delivery: slash-delimited `/.../` regex literals.

Inline block example:

```fab
fixum _ tag ← «inline»
```

Multiline block example (newline after opening `«`):

```fab
fixum _ blob ← «
    select id, email
    from accounts
»
```

Captured template example:

```fab
fixum _ q ← `select * from accounts where id = §`(accountId)
```

Octeti hex literal example:

```fab
fixum _ sig ← |de ad be ef|
fixum _ hello ← |48 65 6c 6c 6f|
```

### Format-Template Application

String literal call syntax is the canonical source form for format-template application:

```fab
"status: § (§)"(sample_status(), "ok")
"status: §1 (§0)"("ok", sample_status())
```

This lowers to the compiler's `scriptum("...", args...)` form. Use the string-template form in ordinary source; reserve `scriptum(...)` for explicit desugaring examples and compiler-facing documentation.

For `textus`, bracket indexing is Unicode-scalar based:

```fab
# Produces "§".
"Salve, §!"[7]
# Produces "hello".
"hello world"[0‥5]
# Produces "hello world".
"hello world"[0 usque 10]
# Produces "ace".
"abcdef"[0‥6 per 2]
```

Text slices accept the full range form, including `per`.

For `lista<T>`, bracket indexing is a single-element access. The index must be
one integer; range slices are not accepted (use `sectio(start, end)` for a
copied range):

```fab
# Element at position i.
xs[i]
# Write element at position i.
xs[i] ← v
```

Lista bracket access is **plain**, not nullable: it returns the bare element
`T` and traps on out-of-bounds. This differs from `tensor`, whose bracket read
is `accipe` sugar and returns `T ∪ nihil`. For nullable list access, use
`xs.accipe(i) → T ∪ nihil` with `vel`.

For `tensor<T, Figura>`, bracket indexing is sugar over the tensor intrinsic
surface:

```fab
# vector.accipe([id])
vector[id]
# vector.ponde([id], v)
vector[id] ← v
# grid.accipe([r, c])
grid[[r, c]]
# grid.ponde([r, c], v)
grid[[r, c]] ← v
```

Reads return `T ∪ nihil`, matching `accipe`; use `vel` or another ordinary
option-handling form before arithmetic. Rank-1 tensors accept scalar integer
indices that fit the tensor `i64` runtime boundary (`u64` is rejected).
Rank-N tensors use a list-shaped index expression such as `[[r, c]]` or a
bound `lista<integer>` value. `grid[r, c]` is not syntax; `memberSuffix` still
contains exactly one `expression` between brackets.

For `octeti`, bracket indexing is a byte or an exclusive window:

```fab
# One byte → numerus<u8>. O(1). Traps on out-of-bounds.
buf[i]
# Exclusive window → octeti. Fully in bounds or fail (no short slice, no pad).
buf[lo‥hi]
```

The index must be an integer or a range. A compile-time-provable out-of-range
index on an octeti literal (`|de ad be ef|[0‥5]`) is a structured reject.
Runtime out-of-bounds traps — the same trapping model as lista bracket access,
not textus short-slice. Lista `[lo‥hi]` stays rejected.

`octeti` is the endian host. Parse byte windows on the buffer
(`buf[lo‥hi] ↦ numerus<W, Be|Le>`). Cross to a list once, for element work,
via `octeti ↦ lista<numerus<u8>>` (representation change only; other element
types fail closed). The reverse `lista<numerus<u8>> ↦ octeti` is live. Do not
detour through `valor`. Lists stay for element work, not endian windows.

### Primary Expressions

`vacua` is a contextual empty-collection marker (identifier form, not a reserved keyword).
Use it with an explicit collection type: `fixum lista<numerus> xs ← vacua` or `fixum tensor<fractus<f32>, []> t ← vacua`.

```ebnf
literal := NUMBER | STRING | ASCII_STRING | BACKTICK_STRING | OCTETI_STRING
         | 'verum' | 'falsum' | 'nihil'
primary := IDENTIFIER | literal | 'ego'
         | arrayLiteral | jsonLiteral | typedConstructor | iunctaExpr
         | adExpr | clausuraExpr | praefixumExpr | scriptumExpr | legeExpr
         | '(' expression ')'
adExpr    := 'ad' ASCII_STRING adOpener?
adOpener  := '(' expression ')'
arrayLiteral := '[' argumentList? ']'
iunctaExpr := 'iuncta' typeArguments '[' argumentList? ']'
# Bare `{ ... }` is a JSON document literal. Keys are quoted JSON strings separated
# by `:`; values are JSON constants. Anonymous Faber objects (`{ key = expr }`)
# are retired (literal-family Stage 6). Genus construction uses `typedConstructor`.
jsonLiteral := '{' (jsonMember (',' jsonMember)*)? '}'
jsonMember  := STRING ':' jsonValue
typedConstructor := typeAnnotation '{' fieldList? '}'
fieldList := fieldInit (',' fieldInit)*
fieldInit := ('sparge' expression) | (fieldKey '=' expression) | IDENTIFIER
fieldKey := IDENTIFIER | STRING | '[' expression ']'
# JSON values: constants only (no Faber expressions, no variable references).
jsonValue := jsonObject | jsonArray | jsonString | jsonNumber | 'true' | 'false' | 'null'
jsonObject := '{' (jsonMember (',' jsonMember)*)? '}'
jsonArray  := '[' (jsonValue (',' jsonValue)*)? ']'
jsonString := STRING
# Numerus when no decimal point or exponent is present; otherwise Fractus.
jsonNumber := NUMBER
```

`STRING` includes short strings delimited by `"` and block strings delimited by
`«` and `»`. `'...'` (`ascii`) and backtick
`` `...` `` (`forma`) are separate literal forms (see String And Template
Literals above).

A bare `{ ... }` now produces an object-rooted JSON document of type `json`:
`{ "name": "Alice", "age": 30, "active": true }`. Keys are quoted JSON strings
separated by `:`; values are JSON constants only. Duplicate keys are an error
(second occurrence). Ascribing to `tabula<K,V>` lowers a real constant map.
Use `↦ valor` for explicit widening to the broad dynamic carrier. Genus/variant
construction `Type { field = expr }` uses the Faber `=` grammar unchanged.

### Special Expressions

```ebnf
fingeExpr     := 'finge' qualifiedIdent ('{' fieldList '}')? ('∷' typeAnnotation)?
qualifiedIdent := IDENTIFIER ('.' IDENTIFIER)*
praefixumExpr := 'praefixum' (blockStmt | '(' expression ')')
scriptumExpr  := 'scriptum' '(' STRING (',' expression)* ')'
legeExpr      := 'lege' 'lineam'?
```

`scriptum` and `lege`/`lineam` are builtin claims that resolve to a user binding
when the surface spelling is bound in scope (parameter, local, function, or any
in-scope definition); otherwise they are the builtin. The same binding-wins rule
applies to `scriptum`'s paren-claimed form and to the `vacua` empty-collection
marker: builtin claims are defaults, not reservations.

`finge` variant construction accepts a qualified variant path
(`finge pkg.Bonum { … }`), so an imported union's variants construct through
the import alias, and the `∷` cast is a full type annotation
(`∷ pkg.Exitus`) exactly as the general postfix ascription (uvf-u3).

`∷` remains the general postfix ascription in `cast`. Rendered text templates
(`STRING '(' argumentList ')'`) and captured `forma` templates
(`BACKTICK_STRING '(' argumentList ')'`) use the ordinary call suffix. Regex
construction uses the ordinary conversio grammar: `(STRING | ASCII_STRING) '↦'
'regex'`.

Slash-delimited regex literals are not active grammar yet. `/` lexes as the
division operator, while `//` and `/* ... */` are rejected as invalid comments.
Use `"..." ↦ regex` for compiled regex values.

---

## Patterns

```ebnf
objectPattern  := '{' patternProperty (',' patternProperty)* '}'
patternProperty := 'ceteri'? IDENTIFIER ('ut' IDENTIFIER)?
arrayPattern   := '[' arrayPatternElement (',' arrayPatternElement)* ']'
arrayPatternElement := '_' | 'ceteri'? IDENTIFIER
```

---

## Diagnostics

```ebnf
outputStmt := ('nota' | 'vide' | 'mone' | 'scribe') expression (',' expression)*
```

The scribe family (`nota`/`vide`/`mone`/`scribe` — en `print`/`debug`/`warn`/`write`)
claims the statement-initial position only when **not** immediately followed by
`(`. `nota expr` is the output statement; a statement-initial `nota(...)` is an
expression statement whose callee is the identifier `nota` — a user function
call, never the intrinsic.

- `nota` = neutral diagnostic note, `vide` = debug/inspect, `mone` = warn
- `scribe` is a diagnostic channel spelling; use current stdlib methods for real output

### Comments

Faber accepts **line comments only**: `#` through end of line. The `#` must be the
first non-whitespace token on the logical line (optional leading ASCII spaces or
tabs only — other Unicode space separators are not skipped by the lexer).
A `#` that follows any other token on the same line is a **lex error** with the
message `# comments must start a line; move this comment above the code`.

Valid line-start comments attach forward as `leading_trivia` on the following
statement or declaration (see comment-preservation). `#` inside string literals,
`ascii` literals, `forma` templates, and other delimited literals is **not** a
comment.

---

## Entry Points

```ebnf
entryHeader  := ('argumenta' IDENTIFIER)? ('exitus' expression)?
incipitStmt  := 'incipit' entryHeader blockStmt
incipietStmt := 'incipiet' entryHeader blockStmt
```

- `incipit` = sync entry, `incipiet` = async entry.
- `argumenta` binds parsed command-line arguments; `exitus` supplies the process exit expression. Their order is fixed by `entryHeader`.

---

## Testing

```ebnf
probandumDecl := 'probandum' STRING probaModifier* '{' probandumBody '}'
probandumBody := (praeparaBlock | probandumDecl | probaStmt)*
probaStmt     := 'proba' STRING probaModifier* blockStmt
probaModifier := 'omitte' STRING | 'futurum' STRING | 'solum' | 'tag' STRING
              | 'temporis' NUMBER | 'metior' | 'repete' NUMBER | 'fragilis' NUMBER
              | 'solum_in' STRING
praeparaBlock := ('praepara' | 'praeparabit' | 'postpara' | 'postparabit') 'omnia'? blockStmt
```

---

## CLI Framework

CLI metadata uses the ordinary reachable `annotation* statementCore` grammar.
The promoted `cli`, `imperium`, `optio`, and `operandus` families validate their
own named-field schemas after parsing.

Faber supports building CLI applications with automatic argument parsing and help generation.

### CLI Entry Point

```fab
@ cli "faber"
@ optio verbose longum "verbose" typus bivalens
incipit argumenta args {
    # CLI framework automatically parses arguments
}
```

### CLI Options and Arguments

```fab
@ imperium "deploy"
@ optio target brevis "t" longum "target" typus textus descriptio "Deployment target"
@ optio verbose brevis "v" longum "verbose" typus bivalens descriptio "Enable verbose output"
@ operandus textus file descriptio "File to deploy"
functio deploy() argumenta args {
    # Arguments automatically parsed and passed
}
```

---

## Capability Calls

Expression-form `ad` is the only supported `ad` surface. Legacy typed
`ad "route" (args) → T { }` and statement-level stream blocks
`ad 'route' { meus/tuus … }` are rejected at parse time.

The active `adExpr` production is defined under **Primary Expressions**. Its
ordinary postfix `conversio` materializes the resulting conversation handle.

- Route: `ASCII_STRING` (`'solum:lege'`), not double-quoted `STRING`.
- Opener: optional single `expression` → Request `data` as `valor`.
- **Expression `ad`**: blockless; evaluates to a `sermo` conversation handle.
  Use postfix `↦ T` (materialization), assign to `sermo`, or open live directional
  views: `s.meus<T>()` (outbound `da` / `fini`) and `s.tuus<T>()` (inbound
  `accipe` / `cursor` / `exhauri` / `fini`). Iterate inbound content frames with
  `s.tuus<T>().cursor()`, not direct `itera ex s.tuus<T>()`.
- **Removed (parse error):** legacy typed `ad "route"` and block `meus`/`tuus` arms.
- Types: compiler-owned `scrinium`, `status`; opaque `sermo` conversation handle.
- `sermo ↦ T` materializes inbound frames into one value of type `T` using
  the type-directed collector for `T`.

See [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md).

---

## Collection Operations

The former `ab` collection pipeline DSL is retired. Collection filtering,
slicing, and aggregation are expressed through ordinary
`textus`/`lista`/`tabula`/`copia` methods and closures instead of a
grammar-level query expression. `textus`, `numerus`, `fractus`, `lista<T>`,
`tabula<K,V>`, and `copia<T>` are compiler-owned core types; their method
surfaces are not Norma declarations.

`prima` and `ultima` are ordinary method names, not transform keywords. `ubi` is
not active collection syntax.

`ex` is used for iteration (`itera ex items fixum x`) and imports (`importa ex "path"`).

---

## Fac Block

```ebnf
facBlockStmt := 'fac' blockStmt catchClause? ('dum' expression)?
```

- `fac { ... }` is the explicit `do` block and executes its body once.
- `fac { ... } dum condition` is the post-test loop form; postfix `dum` attaches only to `fac`, not arbitrary preceding blocks.
- `cape` is an attachment shared by several structured forms, not a semantic mode owned by `fac`. A plain `fac` is often used when an otherwise unattached block needs a local handler: `fac { ... } cape err { ... }`.

---

## Target Support

Target support is **not** part of the grammar — this file defines only the
language. For which grammar each compilation target lowers, and the runtime
policy around it, see:

- [`EBNF_MATRIX.md`](EBNF_MATRIX.md) — generated grammar×target lowerability matrix (the official rows).
- [`docs/design/target-capability-matrix.md`](docs/design/target-capability-matrix.md) — runtime/contract policy (erase/warn/defer), pipeline routing, per-target contracts.

---

## Keyword Reference

| Category            | Faber                         | Meaning             |
| ------------------- | ----------------------------- | ------------------- |
| **Declarations**    | `discretio`                   | tagged union        |
|                     | `fixum`                       | const               |
|                     | `functio`                     | function            |
|                     | `genus`                       | class               |
|                     | `implendum`                   | interface contract  |
|                     | `magnitudo`                   | size/index generic parameter (in `<>` lists) |
|                     | `ordo`                        | enum                |
|                     | `sit`                         | inferred immutable local |
|                     | `sponte`                      | optional declaration slot (post-name) |
|                     | `typus`                       | type alias          |
|                     | `vacua`                       | contextual empty collection marker |
|                     | `varia`                       | let                 |
| **Control Flow**    | `si` / `sin` / `secus`        | if / else-if / else |
|                     | `custodi`                     | guard               |
|                     | `discerne`                    | pattern match       |
|                     | `dum`                         | while               |
|                     | `elige` / `casu`              | switch / case       |
|                     | `fac`                         | explicit do block / post-test loop |
|                     | `itera ex...fixum`            | for-of (values)     |
|                     | `itera de...fixum`            | for-in (keys)       |
|                     | `itera ab...fixum`            | range iteration     |
|                     | `perge`                       | continue            |
|                     | `redde`                       | return              |
|                     | `rumpe`                       | break               |
|                     | `tacet`                       | no-op (silence)     |
|                     | `ergo`                        | compact one-statement body joint |
|                     | `∴`                           | compact clausura joint only |
| **Error Handling**  | `cape`                        | structured local handler |
|                     | `adfirma`                     | assert              |
|                     | `requirit`                    | require (recoverable) |
|                     | `iace`                        | throw               |
|                     | `iacit`                       | legacy marker; no current semantic effect |
|                     | `mori`                        | panic               |
| **Async**           | `@ futura`                    | async annotation (legacy; prefer `fiet`) |
|                     | `@ cursor`                    | generator annotation (legacy; prefer `fiunt`) |
|                     | `fiet`                        | async finite posture |
|                     | `fiunt`                       | sync stream posture |
|                     | `fient`                       | async stream posture |
|                     | `figendum`                    | await-bind immutable |
|                     | `variandum`                   | await-bind mutable |
|                     | `reddet`                      | await-return |
|                     | `tacebit`                     | await-discard |
|                     | `cede`                        | yield (fiunt/fient only) |
| **Endpoints**       | `ad`                          | capability call expression |
| **Boolean**         | `verum`                       | true                |
|                     | `aut`                         | or                  |
|                     | `et`                          | and                 |
|                     | `falsum`                      | false               |
|                     | `non`                         | not                 |
|                     | `vel`                         | local nullable defaulting |
| **Objects**         | `ego`                         | this/self           |
|                     | `finge`                       | construct variant   |
| **Type Shape**      | `∷` | static type ascription / compile-time cast |
| **Type Conversion** | `↦ target`                    | runtime value conversion |
|                     | `↦ T ⇥ expr`                  | conversio with inline recovery of type `T` |
|                     | `↦ numerus`                   | parse to integer    |
|                     | `↦ fractus`                   | parse to float      |
|                     | `↦ textus`                    | convert to string   |
|                     | `↦ bivalens`                  | convert to boolean  |
| **Bitwise**         | `∧` / `∨` / `⊻` / `¬`         | and/or/xor/not      |
|                     | `⇐` / `⇒`                     | left/right shift    |
| **Diagnostics**     | `nota`                        | neutral note        |
|                     | `mone`                        | warn                |
|                     | `scribe`                      | diagnostic channel  |
|                     | `vide`                        | debug/inspect       |
---

## Critical Syntax Rules

1. **Type-first parameters**: `functio f(numerus x)` NOT `functio f(x: numerus)`
2. **Type-first declarations**: `fixum textus name` NOT `fixum name: textus`
3. **Iteration loops**: `itera ex/de collection fixum/varia item { }` or `itera ab range fixum/varia item { }` (verb-first, source, then binding)
4. **Parentheses around conditions are valid but not idiomatic**: prefer `si x ≻ 0 { }` or `si flag est verum { }` over `si (x ≻ 0) { }`
5. **Scribe-family keywords claim statement-initial position only when not followed by `(`** — `nota x` is the output statement; a statement-initial `nota(x)` is a call to the identifier `nota`
