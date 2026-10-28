# Canonical Faber — Pattern Library

A growing collection of good/bad pairs used by the `canonical-faber` review
pass: patterns to follow and patterns to avoid. "Canonical" means the
preferred form. Entries cite their evidence; the live tree wins over this
file. This file prescribes — it does not inventory current usage. Examples
use the `en` surface unless noted — render through the reviewed file's locale.

Null surface: the null **value** is `null` (la `nulla`) and the
null **type** is `none` (la `nihil`). A nullable type is `T ∪ none`; the
null test is `x is none` and its negation is `x not is none`
(la `x non est nihil`; the spelling `is not none` is
`PARSE001 est_non_spelling`). `T ∪ null` and `x is null` are
`PARSE040 null_value_in_type_position` errors. The en pack still lexes
the Latin `nihil` as a transitional value-literal alias (la word); write `null`.

Reading the claims: a `✗` marked **rejected** is a compile error today (the
code is named); a plain `✗` compiles and is only non-canonical. A snippet shown
without `main` is a function body or declaration fragment. Failable
conversions (`↦` to a width that can fail, such as `"7" ↦ int`) must be handled
in every function except `main`: `⊥ default`, or a `⇥ string` error
channel on the function; the snippets below that omit the handler sit in
`main`.

Entry shape: `id` heading, bold one-line claim, one metadata line
(`severity · Evidence: …`), optional scope, then the pair.

## Index

| id | severity | claim |
| ---- | ---- | ---- |
| type-first-decl | canon-violation | declarations are type-first `T name`, never `name: T` |
| nullable-union | canon-violation | nullability is `T ∪ none`, never an invented suffix |
| banned-cum | canon-violation | `cum` is a banned keyword |
| flat-norma-imports | canon-violation | `norma:*` imports are flat; `norma:hal/*` is retired |
| legacy-annotations | canon-violation | no `@ future` / `@ cursor` in new code |
| externa-subsidia-library-source | canon-violation | `@ externa` / `@ subsidia` are rejected (SEM018); library source uses native Faber, `ad` templates, or explicit `panic` deferral |
| legacy-scena-target | canon-violation | new MIR package work uses `fmir-*`, not `scena` |
| anonymous-object-literal | canon-violation | typed construction or JSON literal; anonymous `{ key = expr }` is rejected |
| empty-collection-type | canon-violation | empty collections carry a declared type with `empty`; the untyped `let x ← empty` is rejected |
| optional-slot-sponte | non-preferred | omittable slots use the optional marker, not `∪ none` |
| guard-ergo-redde | non-preferred | single-statement guards use the `then return` consequent (la `ergo redde`); `∴` is the closure joint only |
| conversion-directed-assign | non-preferred | `↤` when the conversion is the point of the line; recovery is `⊥ default`, never `⇥` |
| stroustrup-braces | non-preferred | opening brace on the same line |
| naming-case-regime | non-preferred | snake_case functions, PascalCase types, SCREAMING_SNAKE constants; helpers are ordinary fns, no `_` prefix |
| public-export-marker | canon-violation | cross-module exports carry `@ public` |
| greek-paper-identifiers | non-preferred | paper-facing scalar quantities use visually distinct Greek identifiers, not Latin lookalikes or Unicode subscripts |
| test-colocation-proba | non-preferred | tests live in colocated `.proba` files as `describe`/`test`/`assert` |
| typed-error-union | non-preferred | failable operations return `T ⇥ XError` over a `union` with `string message` variants |
| null-guard-narrowed-use | non-preferred | after an `is none` early-return guard on an immutable binding, plain use — the guard narrows the continuation |
| structure-over-scalar-fanout | non-preferred | pass the structured value, not a positional scalar fan-out |
| tensor-glyph-operators | non-preferred | tensor/vector ops through glyph operators (`⊗` `⊙` `⊘` `·`), not method-call spellings |
| tensor-glyph-transpose | preferred | rank-2 transpose uses postfix `ᵀ`; rank-1 is permanently declined and rank-3+ waits for a batched consumer |
| tensor-glyph-device-form | preferred | device (`@ kernel`, la `@ nucleum`) bodies spell tensor ops with the same glyphs — `out ← a · b` lowers through the tiled matmul recipe; method-call spellings in device bodies are non-preferred |
| tensor-extremum-over-hand-loop | non-preferred | shaped tensor extrema use `reduce via max from` / `reduce via min from` with caller-stated `coalesce`, not a hand-rolled counter loop and sentinel |
| infix-extrema-over-hand-rolled-max | non-preferred | pairwise or elementwise extrema use `⤒` / `⤓`, not a hand-rolled max/min branch; reductions remain `reduce via max from` / `reduce via min from` |
| promoted-equality-over-width-blind-eq | non-preferred | compatible numeric widths use lossless `≅` / `≇` — a signed negative never wraps into an unsigned maximum — not a width-blind `==` mental model or strict-tier `≡` |
| exact-list-equality-through-cast | non-preferred | strict list equality states the concrete bound with `↦`, not a loose untyped literal comparison |
| equality-ladder-tier-choice | non-preferred | choose strict `≡`, promoted-exact `≅`, or fuzzy `≈` in the operands' own joined precision by intent; do not swap tiers |
| redundant-ascription | non-preferred | no `∷` cast when the receiver is already typed and auto-conversion applies |
| dimensional-types-when-fixed | non-preferred | fixed-size data uses dimensional types (`vf32[3]`, `tf32[4]`) or the bounded `list<T, size N>` family |
| numeric-type-sugar-by-context | non-preferred | bare widths everywhere (wrapped forms are rejected); collection sugar only in numeric modules per the sugar guide |
| endian-conversion-not-manual-packing | non-preferred | `list<b8>` + `↦ u32 via Be|Le` instead of hand-rolled shift packing |
| range-loop-over-open-while | non-preferred | `for range a‥b` over open-ended index `while` — ranges are unrollable |
| generics-when-shape-allows | non-preferred | parameterize with `fn<T>` / `class<A, B>` / `union<T>` instead of duplicating per type |
| locale-pin-at-manifest | non-preferred | locale pinned once in `faber.toml [locale]`; frontmatter only to deviate |
| leaf-module-imports | non-preferred | import one-concern leaf modules; broad maps are facades, not the import surface |
| file-header-charter | non-preferred | every library file opens with a charter: what, public import, depends on, decisions |
| failure-posture-selection | non-preferred | absence → `∪ none`; distinct failure identities → typed union `⇥`; one channel per operation |
| interpolation-slots | non-preferred | slotted strings use `§` interpolation; `+` only without slots |
| type-holes-no-duplication | non-preferred | never restate the type on both sides of a binding — `_` / `∪` holes when the other side is explicit |
| labeled-tuple-elements | non-preferred | role-bearing tuple slots carry labels (`tuple<gx: f32, gw: f32>`); access by label, not index |
| named-template-holes | non-preferred | multi-hole templates name their holes (`"§{label} …"(label: value)`); single trivial hole stays `§` |
| cape-narrowest-scope | non-preferred | catch attaches to the narrowest block that can recover — not one whole-function handler |
| guard-rebind-over-rebuild | non-preferred | when the guarded value is a field place or call result, bind it to a `let` first, then guard — not `!` chains, field-by-field rebuilds, or `coalesce` sentinels |
| unary-minus-not-zero-subtraction | non-preferred | negate with unary minus (`-1`, `-x`), never `0 - 1` / `0.0 - x` |
| switch-for-equality-dispatch | non-preferred | literal-equality dispatch chains use `switch`/`case`/`default` (la `elige`/`casu`/`ceterum`), not `if x ≡ k then return` ladders |
| kernel-region-split | non-preferred | mixed host-policy/tensor-math functions split: the wrapper keeps policy and I/O; the math reuses existing `@ kernel` leaves or becomes a `@ public @ kernel` the wrapper calls (a private kernel is a helper only kernels may call) |
| field-over-trivial-getter | non-preferred | a stored class field is the public read (`m.loss`); do not wrap it as `fn loss() { return self.loss }` |
| class-field-modifier | canon-violation | every class field carries `let` / `mut` / `const` (type-level constant, with `=`); a bare `T name` field is rejected (PARSE010); `static` is retired |
| module-constant-const | canon-violation | a module constant is `const T X = e`; module-level `static` is retired and a module-level `←` binding is rejected (`let`/`mut`/`const ← …` alike) |
| true-division-glyph | non-preferred | `/` on integers is floor division; a real quotient is `÷`, never `/` into a float slot |

## type-first-decl

**Declarations are type-first `T name`; `name: T` annotations are not Faber.**

canon-violation · Evidence: declarations across `corpus/`; the colon form is rejected (`PARSE001`)

```fab
# ✗ rejected (PARSE001): colon annotation
fn greet(name: string) → string {
    return name
}

# ✓ type-first
fn greet(string name) → string {
    return name
}
```

## nullable-union

**Nullability is a union in type position; suffix `T?` and invented markers are not Faber.**

canon-violation · Evidence: `corpus/si/ergo-redde.fab`; `T?` is rejected (`PARSE012`), `∪ null` is rejected (`PARSE040 null_value_in_type_position`)

```fab
# ✗ rejected (PARSE012): invented optional suffix
fn divide(int a, int b) → int? {
    …
}

# ✗ rejected (PARSE040): the null value word in type position
fn divide(int a, int b) → int ∪ null {
    …
}

# ✓ union with the null type
fn divide(int a, int b) → int ∪ none {
    …
}
```

## banned-cum

**Any use of the `cum` keyword is a violation.**

canon-violation · Evidence: `cum` is never introduced as a keyword. No pair — the rule is absence. `cum` is in no live locale pack, so the compiler reads it as an ordinary identifier (`SEM001 unknown_identifier` at a `cum x { }` use); any hit is stale grammar.

## flat-norma-imports

**`norma:*` module paths are flat and use the English module names; the nested `norma:hal/*` shape and the old Latin module names are retired.**

canon-violation · Evidence: live `lib/norma/src/` module layout (`fs`, `text`, `json`, `console`, `process`, `time`, `math`, …); both retired shapes fail with `PKG001 unknown_library_module`

```fab
# ✗ rejected (PKG001): retired nested path
import from "norma:hal/json" json

# ✗ rejected (PKG001): retired Latin module name
import from "norma:solum" fs

# ✓ flat English norma module path
import from "norma:json" json
import from "norma:fs" fs
```

## legacy-annotations

**Do not add `@ future` / `@ cursor` annotations (la `@ futura`) in new code.**

canon-violation · Evidence: `../../references/idioms.md` General Style avoid-list. Scope: new code; existing legacy uses are migration candidates, not instant findings. The en pack spells the annotation `@ future`; the la spelling `@ futura` in an en file is `PARSE018 wrong_locale_annotation` (locales never mix). Both spellings still compile today, so the rule is style, not a compiler gate.

## externa-subsidia-library-source

**`@ externa` and `@ subsidia` do not exist: the compiler rejects both (`SEM018 annotatio_unknown_contract`, verified in en and la). Norma source (and the sibling libraries) uses native Faber, a codegen template (`ad`), or an explicit `panic` deferral naming the undispatched route.**

canon-violation · Evidence: the compiler rejects both annotations (`SEM018`); a deferral is an explicit `panic` naming the undispatched route. Scope: library source trees; a hit anywhere is stale grammar.

## legacy-scena-target

**New MIR package work targets `fmir-text` or `fmir`; hidden legacy `scena` is retired.**

canon-violation · Evidence: `faber targets` lists the live target names. Scope: manifests and build configuration (`faber.toml`), not `.fab` bodies.

## anonymous-object-literal

**Anonymous `{ key = expr }` records are rejected; use typed construction `Type { field = expr }` for computed values, or a JSON literal `{ "key": constant }` for constant data.**

canon-violation · Evidence: `../../references/idioms.md` General Style; `corpus/json/json.fab`; `corpus/genus/genus.fab` ("retired bare object literals — bare `{ }` is now a JSON valor literal"). The bare form fails at parse time (`PARSE001 json_quoted_key`: a bare `{ }` expects quoted keys and `:`).

```fab
# ✗ rejected (PARSE001): anonymous record with expression values
let cfg ← { count = 1 + 1 }

# ✓ typed construction for computed values, JSON literal for constant data
let Config cfg ← Config { count = 1 + 1 }
let json data ← { "count": 2 }
```

## empty-collection-type

**An empty collection carries a declared type with the empty initializer `empty` (la `vacua`); the untyped form `let x ← empty` is rejected (`SEM014`), and `← []` states no element type.**

canon-violation · Evidence: `corpus/lista/lista.fab` (a note there says the empty initializer requires a declared collection type); `grammar/EBNF.md` (prefer `empty` for ordinary empty collection values); `let x ← empty` is `SEM014 vacua_requires_declared_type`

```fab
# ✗ rejected (SEM014): empty needs a declared type
let items ← empty

# ✗ untyped empty literal — checks, but declares no element type
let items ← []

# ✓ declared type + empty initializer
let list<int> items ← empty
```

## optional-slot-sponte

**Omittable parameters and fields use the optional-slot marker `optional` (la `sponte`), not a nullable union — the marker expresses omission, `∪ none` expresses absence. A `∪ none` parameter must still be passed; a `∪ none` field must still be written.**

non-preferred · Evidence: `corpus/sponte/sponte.fab` (common-mistakes note); verified: `paginate()` against an `int ∪ none page` parameter is `SEM013 wrong_number_of_arguments`, and `Q { name = "x" }` against an `int ∪ none limit` field is `SEM004 missing_required_struct_field`, while both omissions pass with `optional`

```fab
# ✗ nullable union modeling an omittable argument
fn paginate(int ∪ none page) → int {
    …
}

# ✓ optional slot with default
fn paginate(int page optional coalesce 1) → int {
    …
}
```

## guard-ergo-redde

**Single-statement consequents — especially guard returns — use the compact `then return` consequent (la `ergo redde`; `∴` is rejected in en files, `PARSE026`), not a full block.**

non-preferred · Evidence: `corpus/si/ergo-redde.fab` (the locale surface of the `then return` consequent is the preferred form)

```fab
# ✗ full block for a single-statement guard
fn classify(int x) → string {
    if x < 0 {
        return "negative"
    }
    return "positive"
}

# ✓ compact consequent
fn classify(int x) → string {
    if x < 0 then return "negative"
    return "positive"
}
```

## conversion-directed-assign

**When the conversion is the point of the line and the destination is an assignable place with a known type, use `place ↤ value` (optionally `⊥ default`) instead of routing `↦` and storing `←` separately.**

non-preferred · Evidence: `../../references/idioms.md` (Conversion-Directed Assignment); `corpus/assignatio/conversio-assign.fab`; `grammar/EBNF.md` `fixum_decl`, `assignment`, `inline_default` (`inline_default ::= '⊥' unary_expr`), and the Conversion-Directed Assignment prose. `↤` never appears on `const _` or `let` bindings (`PARSE002 conversio_assign_requires_concrete_type`); `⊥` is legal only on `↤` and on a `↦` conversion. Apply `↤` only where its precondition genuinely holds — an assignable place with a known type where the conversion is the point of the line; otherwise explicit `↦` reads clearer.

```fab
# (in main — see the failable-conversion note in the preamble)
# ✗ typed declaration, explicit route + store (declaration form — most of the live family)
let int total ← "7" ↦ int

# ✓ conversion is the point of the binding — the written type is the target
let int total ↤ "7"

mut int count ← 0

# ✗ explicit route + separate store when the place fixes the target (reassignment form)
count ← "42" ↦ int

# ✓ conversion is the point of the line
count ↤ "42"
```

Recovery is the `⊥` default: `count ↤ "x" ⊥ 0` stores 0 when the conversion fails. The old `⇥ recovery` spelling is rejected (`PARSE030 recovery_uses_exit_arrow`; `⇥` is the function error channel and never takes a value). Outside `main`, an unhandled failable conversion is `SEM011 failable_conversio_requires_handler`, so write `let int total ↤ "7" ⊥ 0` or give the function a `⇥ string` channel.

`↤` binds at the same precedence as `←` and is right-associative: `a ↤ b ↤ "42"` is `a ↤ (b ↤ "42")` (EBNF `assignment`). In a right-associated chain, `⊥ default` attaches to the nearest `↤`, not the outer one (probe: `a ↤ b ↤ "x" ⊥ 7` leaves both `a` and `b` at 7).

```fab
mut int a ← 0
mut int b ← 0

# ✓ inner converts first; `⊥ 7` rides the nearest `↤` (`b ↤ ("x" ⊥ 7)`)
a ↤ b ↤ "x" ⊥ 7
```

## stroustrup-braces

**Opening brace sits on the same line as its construct; one statement per line; four-space indent.**

non-preferred · Evidence: `../../references/idioms.md` Formatting Patterns; corpus throughout. Style only — the Allman form below compiles.

```fab
# ✗ opening brace on its own line
fn greet(string name) → string
{
    return name
}

# ✓ opening brace on the same line
fn greet(string name) → string {
    return name
}
```

## naming-case-regime

**Functions and fields are `snake_case`, types `PascalCase`, module constants SCREAMING_SNAKE. A single-file helper is just another function — no `_` prefix; cross-file helpers live in a public `common` module.**

non-preferred · Evidence: a leading-underscore function name warns (`WARN030 leading_underscore`) because declarations are private by default and `@ public` is required to export, so the prefix adds nothing (a file's own calls do not care whether a callee is exported); naming regime across `lib/norma/src/text.fab`

```fab
# ✗ camelCase function, snake_case type
fn buildQuadMesh() → quad_mesh { … }

# ✗ underscore-marked "private" helper (WARN030)
fn _staged(list<f32> data, list<int> shape) → tensor.Tensor { … }

# ✓ library naming regime; ordinary helper name (default-private already hides it)
@ public
fn build_quad_mesh(int code) → data.ColoredQuadMesh { … }

fn staged(list<f32> data, list<int> shape) → tensor.Tensor { … }
```

Note: norma's Latin-stem public API names are a documented repo dialect, still snake_case. Norma source currently has no `_`-prefixed helper (`rg 'fn _[a-z]' lib/norma/src` finds none); report any hit as `WARN030` residue. One lesson stands: private-only helpers force cross-file duplication — publish one leaf helper instead of copying a private one per file.

## public-export-marker

**Every cross-module import surface carries `@ public`; unmarked declarations are module-private.**

canon-violation · Evidence: `corpus/importa/auxilium.fab` (only `@ public`/`@ internal` declarations are importable; la `@ publica`/`@ interna`); `corpus/publica/publica.fab`; `corpus/annotation-sugar/` (braced record form is canonical, bare spelling is sugar); verified: reading an unmarked function through an import is `SEM006 import_module_private`

```fab
# ✗ exported-by-intent but unmarked — importers cannot see it
fn face_code_valid(int code) → bool { … }

# ✓ explicit export marker (bare sugar form; braced `@ public { }` is the
# canonical annotation record — both accepted)
@ public
fn face_code_valid(int code) → bool { … }
```

## test-colocation-proba

**Tests live in colocated `.proba` files (`src/foo.proba` beside `src/foo.fab`), written as `describe "suite" { test "case" { assert … } }` — not inline test blocks in source.**

non-preferred · Evidence: `lib/norma/src/math.proba`, `lib/norma/src/json.proba`

```fab
# ✗ test assertions living in the library source file
@ public
fn add(float a, float b) → float {
    assert add(1.0, 2.0) ≡ 3.0
    return a + b
}

# ✓ src/math.proba (colocated, not importable, run by `faber test`)
import from "./math" math

describe "math catalog" {
    test "add is commutative on unit" {
        assert math.add(1.0, 2.0) ≡ math.add(2.0, 1.0)
    }
}
```

## typed-error-union

**Failable operations with distinct failure identities return `T ⇥ XError` over a `union` whose variants each carry a `string message`, guarded by `require … throw variant` — not bare string throws.**

non-preferred · Evidence: the `union ShapeError` + `⇥` + `require … throw variant` shape and `MetricError` (`variant Invalid { message = … }`) in library source. Known weakness: the per-variant `string message` plus a hand-written `message()` mirror states the same fact twice per union; the union/`⇥`/`require throw variant` core is the pattern, the mirror boilerplate is not required. Both forms below check today (the string form is non-canonical, not rejected).

```fab
# ✗ untyped string throw as the operation's error surface
fn numel(list<int> f) → int ⇥ string {
    require f.length() > 0 throw "empty shape"
    …
}

# ✓ typed error union, variant with message
union ShapeError {
    NegativeDimension {
        string message
    }
}

@ public { }
fn numel(list<int> f) → int ⇥ ShapeError {
    require f.length() > 0 throw variant NegativeDimension {message = "empty shape"}
    …
}
```

Plain `throw "§ …"(args)` strings remain fine inside test helpers and local plumbing; this entry governs public operation surfaces.

## null-guard-narrowed-use

**After an `is none` early-return guard on an immutable binding (parameter or `let`), continue with plain use — the guard narrows the rest of the block. A `!` assertion after the guard is redundant noise.**

non-preferred · Evidence: `crates/radix-semantic/src/passes/typecheck/control.rs` (`join_flow_states` — "diverging edges contribute nothing", the guard's continuation keeps the refinement); `corpus/assertio/nonnulla.fab` (the `!` operator itself). Verified by probe and by `faber run`: `if r is none then return -1` then `return r.a` checks and runs on a `P ∪ none` parameter; the same holds for a `const` local and a call-result bound to a `const`.

```fab
# ✗ guard, then assert non-null on every use
fn area(Quad ∪ none quad) → f32 ∪ none {
    if quad is none then return null
    return quad!.width * quad!.height
}

# ✓ the guard narrowed `quad`; plain use
fn area(Quad ∪ none quad) → f32 ∪ none {
    if quad is none then return null
    return quad.width * quad.height
}
```

The positive form works too, spelled `if quad not is none then return quad.width` (en; la `quad non est nihil`).

Limits (probed): narrowing applies to a visible immutable binding. A field place does not narrow — after `if h.p is none then return null`, `h.p.a` is `SEM004 optional_receiver_requires_chain`; bind the field to a `const` first (see `guard-rebind-over-rebuild`). A `mut` that is never reassigned narrows but draws `WARN013 unnecessary_varia`.

## structure-over-scalar-fanout

**Pass the structured value a call site already holds; exploding it into a positional scalar fan-out discards structure the code just built.**

non-preferred · Evidence: a positional fan-out discards the structure the caller just built; the repaired shape takes four `vector<f32, size 3>` corners and a `Color` instead of 18 positional scalars

```fab
# ✗ scalar fan-out — the quad and color just built are flattened away
return data.colored_quad_mesh_append(positions, colors, indices, quad.a.x, quad.a.y, quad.a.z, quad.b.x, quad.b.y, quad.b.z, quad.c.x, quad.c.y, quad.c.z, quad.d.x, quad.d.y, quad.d.z, color.r, color.g, color.b)

# ✓ pass the structured values; the callee reads fields it needs
return data.colored_quad_mesh_append(positions, colors, indices, quad.a, quad.b, quad.c, quad.d, color)
```

Rule of thumb: once a call's positional arguments exceed roughly five, prefer a typed record or the already-constructed value.

## tensor-glyph-operators

**Tensor and vector arithmetic uses canonical operators (`+` `-` `⊗` `⊙` `⊘` `·`) where an operator form is legal, not intrinsic method-call twins.**

non-preferred · Evidence: `corpus/tensor/arithmetic-elementwise.fab`, `glyph-matmul.fab`, `glyph-elementwise.fab`, `glyph-outer.fab`, `glyph-hadamard-divide.fab` (operator surfaces; `⊘` is element-wise divide at the multiplicative tier). The method twins still compile, so this is style, not a gate.

```fab
# ✓ canonical operators (probed: vf32 `+`, `-`, `⊙`, and the `⊗` outer product)
let vf32[3] a ← [1.0, 2.0, 3.0] ↦ vf32[3]
let vf32[3] b ← [4.0, 5.0, 6.0] ↦ vf32[3]
let vf32[2] p ← [1.0, 2.0] ↦ vf32[2]
let vf32[3] sum ← a + b
let vf32[3] diff ← a - b
let vf32[3] squared ← b ⊙ b
let matrix<f32, [3, 2]> outer ← a ⊗ p

# ✗ method twins where the operator form is legal
let vf32[3] method_sum ← a.added(b)
let vf32[3] method_diff ← a.subtract(b)
```

Scope: numeric/tensor code. When reviewing method-call spellings of operations that have an operator form, flag the operator exemplar as the preferred surface.

## tensor-glyph-transpose

**A rank-2 tensor transpose uses the postfix `ᵀ` glyph (`[M,N] → [N,M]`), not the non-preferred `transpose()` method twin (la `transpone`); the glyph is sugar over the existing `Transpose` plan.**

preferred · Evidence: `corpus/tensor/glyph-transpose.fab`, `grammar/EBNF.md` (Postfix tensor transpose); the method twin is `transpose`; the la spelling of it written in an en file is `SEM004 unknown_method`

```fab
# ✓ rank-2 postfix transpose
let tensor<f32, [3, 2]> output ← inputᵀ

# ✗ method twin when the postfix surface is legal
let tensor<f32, [3, 2]> output ← input.transpose()
```

The rank rule is deliberate: rank-1 is permanently declined because there is
no row/column distinction; rank-3+ is deferred until a batched-transpose
consumer owns the contract. The precedence form is `a · bᵀ ∇ [x]` →
`(a · bᵀ) ∇ [x]`; `∇` remains parse-only (a gradient expression is
`SEM010 gradient_expression_unsupported`).

## tensor-glyph-device-form

**Device (`@ kernel`) bodies spell tensor operations with the same glyphs as host code — `out ← a · b` lowers through the tiled matmul recipe; method-call spellings in device bodies are non-preferred.**

preferred · Evidence: `corpus/kernel/` and `corpus/tensor/glyph-*.fab` exemplars. Scope: device-library code; the GPU path as a whole is not a product surface today, so cite exemplars for what a kernel may contain.

```fab
# ✓ glyph body (shape of the proof exemplar)
@ kernel
@ public
fn multiply(tf32[16, 8] a, tf32[8, 16] b, mut tf32[16, 16] out) → void {
    out ← a · b
}
```

## tensor-extremum-over-hand-loop

**A shaped tensor maximum or minimum uses `reduce via max from` / `reduce via min from` with a caller-stated `coalesce` identity when the shape may be empty, not a hand-rolled counter loop with a sentinel. The operand must be a tensor (`tf32[4]`, `tensor<f32, …>`) or matrix; a `list` operand is `reducta_requires_shaped_iterable`.**

non-preferred · Evidence: `corpus/summa/max-min.fab`; `crates/radix-parser/src/max_min_disambiguation_test.rs`; `grammar/EBNF.md` `reducta_expr`. For a `list`, use `.argmax()` / `.argmin()` or the infix `⤒` / `⤓` (see the next extremum entry). A bounded-prefix row max (a `for range` walk over a valid prefix, commented as deliberate) is not a finding.

```fab
# ✗ row-max loop: a sentinel and a second counter walk the flat carrier
mut f32 maximum ← x.data.get(i * cols) coalesce 0.0
mut int j ← 0
while j < end {
    let f32 value ← x.data.get(i * cols + j) coalesce 0.0
    if value > maximum {
        maximum ← value
    }
    j ← j + 1
}

# ✓ the canonical form is binderless; the caller owns the empty identity
let f32 maximum ← reduce via max from (row ⊙ scale) coalesce 0.0

# ✓ the coordinate clause is the index-needing variant
let f32 minimum ← reduce via min from values at [i] coalesce floor
```

`max` and `min` are selected through `via` like `sum`; they are not flags on
one another. The coordinate clause is optional; use `at [...]` when a body or
group key needs the index. A fixed non-empty shape (`tf32[4]`) returns the
element type with no `coalesce`; an empty-possible source without `coalesce`
has result `T ∪ none` and the nullable result must be handled explicitly. A
transform belongs in the operand (`reduce via max from (row ⊙ scale)`).

## redundant-ascription

**`∷` is for type-changing ascription; it is redundant when the receiver is already typed and an auto-conversion is allowed — `1.0 ∷ f32` into a `const f32` is noise.**

non-preferred · Evidence: correct plain assignment is shown at `corpus/typi/sized-numerus.fab` (`let i64 signed ← narrow`).

```fab
# ✗ receiver already f32; the cast restates the declaration
let f32 tolerance ← (0.01 ∷ f32)

# ✓ auto-conversion reaches the typed place
let f32 tolerance ← 0.01
```

(Bindings inside a function or `describe` block; a module-level constant is `const f32 TOLERANCE = 0.01` (`=`, compile-time) — see `module-constant-const`.)

Keep `∷` where the receiver is untyped or no auto-conversion exists (`0x80 ∷ octet`, narrowing into an inferred place) — those ascriptions carry information.

**Equality is exact-typed (SEM010) — casts in `≡` positions are load-bearing, not redundant.** `f32 ≡ 1.5` is rejected (`equality_exact_types_required`), so `assert x ≡ (10.0 ∷ f32)` on an `f32` receiver is the correct form (verified; the bare form compiles only when widths already match, e.g. the `ratio ≡ 3.5` compare in `corpus/conversio/valor-scalaria.fab`). This entry flags `∷` in binding/argument/default positions with auto-conversion — never inside `≡` comparisons against float literals.

## dimensional-types-when-fixed

**Data whose size is a compile-time fact uses dimensional types — `vf32[3]`, `tf32[4]`, `matrix<f32, [3, 2]>` — or, for a capacity bound on a list or string, the bounded forms `list<T, size N>` / `string<size N>`; not un-sized collections. Fixed dimensions enable loop unrolling and register representation.**

non-preferred · Evidence: `corpus/tensor/bracket-access.fab` (`mut tf32[4] input ← seed.create(0.0, [4])`), `glyph-elementwise.fab` (`let vf32[3] a ← …`); sizing caveat (`matrix<T, [R,C]>` and `mf32[R,C]` are register types of the semantic and MIR layers; Rust/Go/TS package targets reject them until host representations exist)

```fab
# ✗ fixed-length data in an un-sized collection
mut list<f32> input ← seed.create(0.0, [4])

# ✓ dimensional type carries the size fact
mut tf32[4] input ← seed.create(0.0, [4])
```

**Bounded-buffer family — shipped.** `list<T, size N>` (la `lista<T, magnitudo N>`), `string<size N>` and `ascii<size N>` are the capacity-bearing forms (`grammar/EBNF.md` Generic Collections lists the bounded list form as the shipped one). Probed: `mut list<f32, size 2> a ← [1.0, 2.0, 3.0]` is rejected (`SEM010 incompatible_tensor_index`), the type works in parameter position, and `append` on a bounded list is a failable call (`SEM011 failable_call_requires_handler` without a handler). The `string<_>` / `ascii<_>` capacity-hole forms are admitted but not fully shipped (EBNF row). Bounded `map` / `set` are not implied by the family.

**Hardcoded-width variant spellings in closed host-vocabulary enums are temporary.** A graphics library's `VertexFormat` (`Float32x4`, `Uint32`, …) mirrors WebGPU `GPUVertexFormat` (itself inherited from three.js); the spellings are accepted only because component width is a runtime value generics cannot parameterize — not because they are canonical. The intent is to convert such unions to proper generics once value-parameterized generics land; the library marks them with a TEMPORARY SHAPE note, so do not flag them today, and treat new ones that mirror a host vocabulary the same way.

## numeric-type-sugar-by-context

**Width markers are bare everywhere (`list<u8>`, `i32`, `f32`, `inf`); the wrapped `list<int<u8>>` / `int<i32>` forms (la `numerus<i32>`) are `PARSE040 numeric_wrapper_retired`, not a style choice. Collection sugar (`lf32`, `tf32[2, 3]`, `mf32[4, 4]`, `lu32`) only in numeric-primary modules, chosen per module rather than per repository, with one `# STYLE:` line at the top of the file.**

non-preferred · Evidence: the policy is chosen per module, not per repository (long collection head for general Faber, sugar for numeric/tensor-primary modules; wrapped forms are retired and rejected by the compiler); exemplar `corpus/tensor/index-width.fab` (`let list<u32> origin ← [r, c]`). The policy words keep their `<W>` argument (`trapping<u8>`, `wrapping<u8>`, `saturating<u8>`; la `exactus`, `modulus`, `saturatus`) — those are not retired wrappers.

```fab
# ✗ rejected (PARSE040 numeric_wrapper_retired): retired wrapped width
mut list<int<u8>> out ← empty

# ✓ bare width marker in general code
mut list<u8> out ← empty

# ✓ full sugar in a numeric-primary module (declared once at top of file)
# STYLE: numeric sugar (tf32, mf32, sf32, lf32, lu32)
let tf32[2, 2] dense ← [[1.0, 0.0], [0.0, 2.0]] ↦ tf32[2, 2]
```

## endian-conversion-not-manual-packing

**Byte payloads are `list<b8>` (la `lista<b8>`) with typed endian conversion (`↦ u32 via Be|Le`), never hand-rolled shift-and-append packing. The reverse direction is native too: `x ↦ list<b8, size N> via Le|Be` for one number, `list<f32> ↦ list<b8> via Le|Be` or `tensor<E, shape> ↦ list<b8> via Le|Be` for a whole list or tensor (`corpus/bits/bits-tensor-bytes.fab`), and `out.extend(part)` (la `extende`) joins byte lists (`corpus/lista/methodi-extende-octeti.fab`). The hint is a `via` clause; the type-argument spelling (`↦ ascii<4, Hex>`) is rejected.**

non-preferred · Evidence: `corpus/conversio/octeti-endian.fab` (`|de ad be ef|[0‥4] ↦ u32 via Be`; endian-omitted conversion is rejected, `SEM016 octeti_to_numerus_unspecified`); `grammar/EBNF.md` (The `via` clause: hints are `Hex` `Bin` `Oct` `Be` `Le` `Bits` `Code`; `PARSE030 conversio_hint_type_argument` for the retired form); the manual form (eight `(n ⇒ k) ↦ u8` appends assembling a little-endian u64 by hand) also needs a `⊥` handler outside `main`

```fab
# ✗ manual little-endian assembly
fn header8(int n) → list<u8> {
    mut list<u8> out ← empty
    out.append((n ⇒ 0) ↦ u8 ⊥ 0)
    out.append((n ⇒ 8) ↦ u8 ⊥ 0)
    …
}

# ✗ rejected (PARSE030): retired type-argument hint
let text ← 255 ↦ ascii<4, Hex>

# ✓ typed endian conversion over a byte window; hint is a `via` clause
let u32 word ← window[0‥4] ↦ u32 via Le
let ascii<size 4> hex ← 255 ↦ ascii<size 4> via Hex

# ✓ a whole buffer or weight file at once: literal extents, hint required,
#   byte length exactly product(shape) x width; never a per-element list round trip
let tf32[2, 3] weights ← raw ↦ tf32[2, 3] via Le
```

## range-loop-over-open-while

**Index loops over a known span are `for range a‥b let i` (la `itera ab`), not open-ended `while` with a manual counter — range bounds are compile-time facts, so the loop can unroll. A constant stride is spelled `per` (en `step`): `for range a‥b step n let i`.**

non-preferred · Evidence: range bounds are compile-time facts, so the loop can unroll; `grammar/EBNF.md` `range_tail` / `itera_stmt`. Probed and run: `for range 0‥values.length() step 3 let i` checks, and `faber run` over seven elements sums indices 0, 3, 6.

```fab
# ✗ open-ended counter walk over a fixed span
mut int i ← 0
while i < values.length() {
    out ← out + values[i]
    i ← i + 1
}

# ✓ range loop; the span is declared, not maintained
for range 0‥values.length() let i {
    out ← out + values[i]
}

# ✓ constant stride is part of the range, not a hand-advanced counter
for range 0‥values.length() step 3 let i {
    out ← out + values[i]
}
```

`while` stays correct when the index advances by computed (data-dependent) strides or the bound is genuinely open (`lib/norma/src/text.fab` `count`) — that exception is accepted practice, not a violation.

## generics-when-shape-allows

**Parameterize over the type instead of duplicating a function or shape per type: `fn identity<T>(T value) → T`, `union Result<T>`, `interface Responder<T>`, `class Something<A, B>`.**

non-preferred · Evidence: `corpus/generic/generic.fab` (a generic identity function), `corpus/generic/genus.fab` (a generic `union` and a generic `interface`; grammar note: union/interface/class accept type-parameter lists). Class-generic application is grammar-admitted but corpus-thin — cite the grammar, not an exemplar, for `class<A, B>` until one lands.

```fab
# ✓ from corpus/generic/generic.fab
fn identity<T>(T value) → T {
    return value
}
```

Review direction: near-identical functions differing only in an element/field type (or hand-rolled `int`/`string` variants of one algorithm) are the finding; the generic form is the preferred shape.

## locale-pin-at-manifest

**Pin the package code locale once in `faber.toml [locale]`; per-file `+++` frontmatter only when a file deviates from the package default.**

non-preferred · Evidence: repeating the package default in every file is boilerplate; `[locale]` is the canonical manifest section and `[reader]` the legacy alias (`../../references/locales.md`; `crates/faber/src/package/manifest.rs` keeps `[reader]` only as a serde alias).

```fab
# ✗ every src file repeating the package default
+++
locale = "en"
+++
fn greet() → string { … }

# ✓ faber.toml — stated once for the whole package
[locale]
locale = "en"
```

A file that genuinely deviates (a Latin-locale exemplar inside an `en` package) keeps its frontmatter — that is the form's purpose. A standalone probe file outside a package has no manifest, so it carries its own frontmatter.

## leaf-module-imports

**Split a library into one-concern leaf modules and import the leaf (`provider:area/leaf`, alias = last path segment); broad package maps are thin facades, not the import surface.**

non-preferred · Evidence: Norma style is one concern per import path; `lib/norma/src/http/headers.fab` and siblings

```fab
# ✗ importing the broad facade and qualifying through it
import from "triga:geometry" geometry
let attr ← geometry.BufferAttribute { … }

# ✓ import the leaf that owns the concern
import from "triga:geometry/attribute" attribute
let attr ← attribute.BufferAttribute { … }
```

Aliasing: the alias is an optional trailing `as name` (`import from "triga:graph/object" object as graph_object`); the bare last-segment alias (`import from "triga:graph/object" object`) also works. An `as` alias is a free choice for disambiguation, not a compiler workaround.

## file-header-charter

**Every library file opens with a `#` charter: what the module is, its public import, what it depends on — and the rationale for real decisions. Content decides length; there is no length norm.**

non-preferred · Evidence: a header that states what the module is, its import and its dependencies orients a reader; length depends on content; `lib/norma/src/http/headers.fab` opens this way

```fab
# ✗ file starts cold
import from "triga:math" math

# ✓ charter first, then code
# Voxel face quad construction helpers.
#
# Public import: import from "triga:face" face
# Depends on: triga:math, triga:geometry/data

import from "triga:math" math
```

When a file encodes a real posture decision, record the decision and its why in the charter — that prose is the point, not padding.

## failure-posture-selection

**Choose the failure channel by the kind of failure: absence → `T ∪ none` with a guard; distinct failure identities → typed `union XError` with `⇥` and `require … throw variant`; undispatched route → explicit deferral. One operation, one channel.**

non-preferred · Evidence: the channel follows the kind of failure, not the repository; undispatched route → an explicit `panic` deferral naming the route

```fab
# ✗ absence expressed as failure — not-found is not an error
fn first(list<int> xs) → int ⇥ LookupError {
    require xs.length() > 0 throw variant LookupError {message = "empty"}
    return xs[0]
}

# ✓ absence returns the optional union
fn first(list<int> xs) → int ∪ none {
    if xs.length() ≡ 0 then return null
    return xs[0]
}
```

The inverse is equally a finding: an operation with genuinely distinct failure identities (malformed, unsupported, overflow) collapsed into a nullable return discards the failure vocabulary — use the typed union.

## interpolation-slots

**Strings with argument slots use `§` interpolation — `"… § … § …"(a, b)`; `+` concatenation only for slot-free strings.**

non-preferred · Evidence: `lib/norma/exempla/crypta-sha2/src/main.fab` (`throw "§: byte mismatch at §"(label, i)`)

```fab
# ✗ concatenating a slotted message
return "unknown digest algorithm: " + algorithm

# ✓ slot interpolation
return "unknown digest algorithm: §"(algorithm)
```

## type-holes-no-duplication

**A binding states its type on one side only. When the initializer already fixes the type, bind inferred (`let x ← …`, writable `mut _ x ← …`; `let _ x` is `PARSE001 sit_identifier_binding`); when the declaration carries the type, let the right side infer (`let list<f32> xs ← empty`). Never write the same type on both sides.**

non-preferred · Evidence: `corpus/type-hole-union/type-hole-union.fab` (canonical exemplar: `_` solves one inhabitant, `∪` accumulates members across flows, `list<∪>` learns heterogeneous elements; WARN016/WARN017 police cross-shape solves); `corpus/ego/ego.fab` (`let box ← Box { title = "first" }` — the class-literal hole form); typical offender: `let Quaternion av ← Quaternion {…}`

```fab
# ✗ type restated on both sides — the constructor already names it
let Quaternion av ← Quaternion { w = 1.0, x = 0.0, y = 0.0, z = 0.0 }

# ✓ left-inferred: the initializer fixes the type
let av ← Quaternion { w = 1.0, x = 0.0, y = 0.0, z = 0.0 }
```

The mirror direction is equally binding: when the left side carries the type, the right side stays bare — `let list<f32> xs ← empty`, `mut list<u8> out ← empty` — never `… ← empty` with an untyped place or a duplicated conversion target.

**Hole selection follows the shape (WARN016/WARN017):**

- `_` — exactly one inhabitant type flows in (literals, constructor calls, single-typed calls). A `_` solved to a multi-member union warns (WARN016).
- `∪` — the slot accumulates a member per direct flow: `mut ∪ mixed ← 1` then `mixed ← "hello"` solves `int ∪ string`. Solved monomorphically it warns (WARN017).
- `list<∪>` — heterogeneous element inference: `let list<∪> nodes ← [1, "two", 3]` solves `list<int ∪ string>`.
- A `_` over `T ∪ none` stays quiet — one optional inhabitant is not a multi-member sum.

Scope: bindings whose other side is explicit. Public signatures — parameters, returns, fields — state their types; holes there are a separate judgment, not this entry.

## labeled-tuple-elements

**A tuple whose slots carry distinct roles is labeled: `tuple<gx: f32, gw: f32>` with by-label access (`pair.gx`) and by-label destructuring (`let {gx, gw} ← pair`). Positional `.0`/`[0]` access on a role-bearing tuple hides meaning.**

non-preferred · Evidence: `corpus/literalia/labeled-elements.fab` (labeled construction, mixed labeled/unlabeled slots, label+hole composition, partial by-label binding with `rest`, identity erasure)

```fab
# ✗ role-bearing tuple read by position — the reader must remember the order
let tuple<f32, f32> grads ← tuple<f32, f32> [1.5, 0.25]
apply(grads[0], grads[1])

# ✓ labels carry the roles; mixed slots stay positional only when role-free
let tuple<gx: f32, gw: f32> grads ← tuple<gx: f32, gw: f32> [1.5, 0.25]
apply(grads.gx, grads.gw)
```

Live-surface notes from the exemplar: labels compose with holes (`tuple<loss: _, int>`); labels are identity-erased (labeled and unlabeled forms assign across); partial binding uses `let {loss, rest leftover} ← metrics`. Labels are legal only in tuple type-argument position — a label in a non-tuple generic list (`f<gx: T>(x)`, `list<gx: T>`) is a parse error. Combine with `type-holes-no-duplication`: `let pair ← tuple<gx: string, gw: int> ["hello", 42]` — the labeled constructor already names the type.

## named-template-holes

**Templates with multiple holes — or whose holes carry meaning — name them: `"§{greet} world"(greet: "Hello")`. A single trivial hole stays anonymous: `"Hello, §!"(name)`.**

non-preferred · Evidence: `corpus/literalia/named-holes.fab` (named-only application, mixed named/anonymous with positional actuals, positional actual on a named hole erases identically); `corpus/scriptum/scriptum.fab` (anonymous single-slot form)

```fab
# ✗ three anonymous holes — the call site is a guess-the-order exercise
let row ← "§ | § | §"(name, count, status)

# ✓ named holes read at both template and call site
let row ← "§{name} | §{count} | §{status}"(name: name, count: count, status: status)
```

Live-surface notes from the exemplar: mixed forms share one positional sequence (`"§{greet} §"("Hello", "World")` fills the named then the anonymous hole in order); labels erase before rendering — output is ordinary text. Reject rows (do not add these to the library): labels on ordinary calls (`f(greet: "x")`), the `=` form (`"§ world"(greet = "x")`), unknown labels.

## cape-narrowest-scope

**`catch` (la `cape`) attaches to the narrowest block that can actually recover — the failing branch, loop, or resource block — one handler per distinct recovery strategy. Wrapping an entire function body in a single `do { … } catch { … }` to reach one deep throw collapses distinct failures into one handler and pushes recovery far from the failure.**

non-preferred · Evidence: EBNF `cape_clause?` attachment points — `do` blocks, every `if`/`elif`/`else` arm, `while`, `for`, `switch` (`grammar/EBNF.md` productions `arm`, `else_arm`, `dum_stmt`, `itera_stmt`, `elige_stmt`, `fac_stmt`; the `cura` resource block is retired and has no attachment point); `corpus/cape/cape.fab`, `corpus/cape/repro.fab` (arm-attached cape emits and rustc-compiles). Both shapes below check today; the whole-body form is a style finding, not a rejection.

```fab
# ✗ one whole-body handler for three different failures
fn handle(int x) → string ⇥ string {
    do {
        if x < 0 then throw "negative input"
        step_a(x)
        step_b(x)
    }
    catch e {
        return "generic failure: §"(e)   # recovery knows nothing about which step failed
    }
}

# ✓ catch attached where recovery happens — per arm, smallest scope
fn handle(int x) → string {
    if x < 0 {
        step_a(x)
    }
    catch e {
        return "input rejected: §"(e)
    }
    else {
        step_b(x)
    }
    catch e {
        return "processing failed: §"(e)
    }
    return "ok"
}
```

Ordering facts from the grammar: on `do`-`while` (la `fac`-`dum`), `catch` binds before the trailing `while`; an arm-attached `catch` may catch `throw` (la `iace`) without an enclosing `⇥`, and a failable call (`→ T ⇥ E`) propagates to the function's own error channel without any wrapper. Report the whole-body wrapper whenever it collapses genuinely distinct recoveries into one handler.

## guard-rebind-over-rebuild

**When the nullable value is a field place or a call result, bind it to a typed `let` first and guard the binding; never rebuild the value field by field, chain `!`, or add a post-guard `coalesce` sentinel.**

non-preferred · Evidence: extends `null-guard-narrowed-use` (narrowing is binding-scoped: a field place such as `h.p` does not narrow, a `let` binding does); the green form `let FaceQuad ∪ none quad_result ← …`, `if quad_result is none then return null`, `let FaceQuad quad ← quad_result`. The rebuild shape this entry was written against (`SceneNode {name = result!.name, …}`) no longer appears in the library.

```fab
# ✗ guard a field place, then rebuild through `!`
if self.geometry is none then return null
let BufferGeometry geom ← BufferGeometry {topology = self.geometry!.topology, vertex_count = self.geometry!.vertex_count, …}

# ✗ guard, then a sentinel whose default is unreachable
if count_result is none then return 0
return count_result coalesce 0

# ✓ bind the place to a `let` once, guard the binding, use it plainly
let BufferGeometry ∪ none geometry_result ← self.geometry
if geometry_result is none then return null
return geometry_result.bounding_box()
```

Pair with `type-holes-no-duplication`: the binding states its type once, on the left.

## unary-minus-not-zero-subtraction

**Negation is unary minus — `-1`, `-89.0`, `-half_width` — never subtraction from zero (`0 - 1`, `0.0 - 89.0`, `0.0 - half_width`).**

non-preferred · Evidence: EBNF `unary_expr ::= ('-' | '¬' | 'non') unary_expr` (`grammar/EBNF.md`); `corpus/custodi/custodi.fab` (`return -1`), `corpus/intrinseca/fractus-comparatio.fab` (`let float value ← -3.75`). Both spellings compile; this is style. The original offenders (`0.0 - 89.0`, `0.0 - half_width`) no longer appear in the library; treat any remaining hit as residue.

```fab
# ✗ subtraction from zero
if pitch_degrees ≺ 0.0 - 89.0 then return 0.0 - 89.0
let f32 half_width ← 0.0 - width * 0.5

# ✓ unary minus
if pitch_degrees ≺ -89.0 then return -89.0
let f32 half_width ← -(width * 0.5)
```

Live compiler gap (re-probed): a bare negative literal in the compact consequent returning through `T ∪ none` still fails — `if x ≡ 3 then return -1` on `→ int ∪ none` is `SEM011 numeric_operand_required` — while the plain-`int` form passes. Bridge spellings: the block consequent, a typed `let` (`let int negative ← -1; return negative`), or `0 - 1`. Prefer the typed const — it keeps unary minus and states the type once. Zero-subtraction remains non-preferred everywhere else.

## switch-for-equality-dispatch

**Dispatch on a literal discriminant uses the value-branch statement — `switch x { case k { … } default { … } }` (la `elige`/`casu`/`ceterum`) — not an `if x ≡ k then return …` ladder.**

non-preferred · Evidence: `corpus/elige/` (marked canonical, with a note that it works with text, number, and other comparable types; exemplars are la-locale — render through the reviewed file's locale); en surface from the locale pack (`switch`/`case`/`default`, `locale/en/pack.toml`); re-probed (en `switch`/`case`/`default` on `int`, block and `case 1 then return …` forms, check clean); a live ladder shape is `if code ≡ 1 then return …`. Do not confuse with `match` (`discerne`), which matches closed sets — unions, enums, variants — not numeric widths or lengths.

```fab
# ✗ equality ladder over a code discriminant
if code ≡ 1 then return Color {r = 0.8, g = 0.1, b = 0.1}
if code ≡ 2 then return Color {r = 1.0, g = 0.4, b = 0.4}
…
return null

# ✓ value dispatch
switch code {
    case 1 {
        return Color {r = 0.8, g = 0.1, b = 0.1}
    }
    case 2 {
        return Color {r = 1.0, g = 0.4, b = 0.4}
    }
    default {
        return null
    }
}
```

Guard conditions stay `if` — range checks (`if code ≤ 2`), boolean predicates, and non-literal comparisons are not value dispatch. **Multi-field comparisons are also not dispatch**: a ladder that probes several distinct fields (`if n.x ≡ -1.0 … if n.y ≡ 1.0 …`) has no single discriminant and must stay `if` — converting it to `switch` is a bug, not a fix. Non-exhaustive switches without `default` silently fall through (corpus `switch` common-mistakes note) — cover the values or write the `default`.

## infix-extrema-over-hand-rolled-max

**Pairwise or elementwise extrema use `⤒` / `⤓`; a whole-tensor reduction uses `reduce via max from` / `reduce via min from` — neither needs a hand-rolled max/min branch.**

non-preferred · Evidence: `corpus/summa/minmax-infix.fab` (scalar, list, tensor, and chained clamp); `corpus/summa/max-min.fab` (reduction expressions over shaped inputs); parser precedence in `crates/radix-parser/src/expr.rs`; `grammar/EBNF.md` `additive_expr` (`⤒ ⤓` sit at the `+ -` tier). Probed: `left ⤒ right` and `values ⤒ 4 ⤓ 8` over a `list<int>` check clean.

```fab
# ✗ hand-rolled pairwise maximum and manual clamp
mut int high ← left
if right > high {
    high ← right
}
mut list<int> clamped ← values
for from clamped mut value {
    if value > 8 {
        value ← 8
    }
}

# ✓ infix extrema express the operation directly
let int high ← left ⤒ right
let list<int> clamped ← values ⤒ 4 ⤓ 8

# ✓ whole-tensor reduction keeps its dedicated head and caller identity
let f32 maximum ← reduce via max from scores coalesce 0.0
```

Use the glyphs for two-operand scalar, list, or elementwise selection and chained
clamping. Use `reduce via` when one result summarizes a whole shaped
tensor; `coalesce` remains the caller-stated identity for an empty source. A
`reduce via max from` over a plain `list` is rejected — use `.argmax()` or a `⤒` fold.

## promoted-equality-over-width-blind-eq

**Compatible numeric widths use lossless promoted-exact `≅` / `≇`; do not import a width-blind `==` mental model or force a strict `≡` comparison across widths.**

non-preferred · Evidence: `corpus/operatores/numeric-value-eq.fab`; `crates/radix-semantic/src/passes/typecheck/ops.rs` (numeric promotion join and numeric-only gate); `crates/radix-semantic/src/passes/typecheck/numeric_value_equality_test.rs`; comparisons across number families are exact. Verified: `u16 ≡ i64` is `SEM010 equality_exact_types_required`, `u16 ≅ i64` checks.

```fab
# ✗ mainstream width-blind equality is not a Faber operator
# if left == right { … }

# ✗ rejected (SEM010 equality_exact_types_required): strict equality across a deliberate width mismatch
if port ≡ count {
    print "same"
}

# ✓ compatible numeric widths join, then compare exactly
if port ≅ count {
    print "same value"
}
```

`≅` is numeric-only. It is the promoted-exact tier, not fuzzy equality and not
collection equality (`list ≅ list` is `SEM011 numeric_value_equality_operands_required`);
use `≇` for its negation. The numeric join is lossless: a signed negative never
wraps into an unsigned maximum (`−1 ≅ 18446744073709551615 ∷ u64` is false, and
the `≇` form is true).

## exact-list-equality-through-cast

**List values reach strict exactness through a typed conversion — `X ≡ ([…] ↦ list<int>)` — rather than a loose untyped literal or numeric `≅`.**

non-preferred · Evidence: `grammar/EBNF.md` `cast_expr` / `conversio_expr`; `crates/radix-semantic/src/passes/typecheck/ops.rs` (exact equality); `crates/radix-semantic/src/passes/typecheck/convert_test.rs` (typed list conversion). Verified: the loose and `≅` forms below are rejected, the cast form checks.

```fab
# ✗ rejected (SEM010 equality_exact_types_required): list literal's element type left implicit
assert actual ≡ [1, 2]

# ✗ rejected (SEM011 numeric_value_equality_operands_required): ≅ is the numeric promotion tier, not list equality
assert actual ≅ [1, 2]

# ✓ bind the comparison literal to the receiver's concrete list type
assert actual ≡ ([1, 2] ↦ list<int>)
```

The `↦` cast is load-bearing in the exactness idiom. It states the element
bound before `≡` checks the two list values; it does not turn `≅` into a
collection operator.

## equality-ladder-tier-choice

**Choose the equality tier by intent: strict same-type `≡` / `≢`, promoted-exact numeric `≅` / `≇`, or fuzzy tolerance `≈` / `≉`; do not use a neighboring glyph as a vague synonym.**

non-preferred · Evidence: the equality ladder described below; `crates/radix-semantic/src/passes/typecheck/ops.rs`; `corpus/intrinseca/fractus-approximata.fab`; integer-family `≈` follows one exact relative-1e-9 rule

```fab
# ✗ fuzzy glyph for an exact promoted-value check
assert port ≈ count

# ✓ strict when both operands already have the same type
assert same_width ≡ (42 ∷ int)

# ✓ promote compatible numeric widths, then compare exactly
assert port ≅ count

# ✓ fuzzy only when a tolerance match is intended
assert measured ≈ expected
```

The ladder is monotone: `≡` is strict, `≅` widens only compatible numeric
widths before an exact compare, and `≈`/`≉` carries the language tolerance
default. Lists and other non-numeric values do not enter the `≅` tier.

## greek-paper-identifiers

**Paper-facing scalar quantities use the visually distinct Greek identifier set — `σ` sigmoid, `γ` norm gain, `ε` tolerance/norm epsilon, `θ` RoPE base or model parameter, `τ` sampling temperature, with `α`/`β` reserved for future optimizer coefficients — never Latin lookalikes, and never Unicode subscripts.**

non-preferred · Evidence: the Faber lexer `IDENTIFIER` rule in `grammar/EBNF.md` admits these letters; the table below is a library naming convention

scope: library-owned identifier vocabulary only; this does not add grammar, operators or locale-pack spellings, and it sets no naming rule for other libraries.

```fab
# ✗ a visually confusable spelling or a Unicode-subscripted quantity
fn normalize(f32 ο, f32 subscripted_beta) → f32 {
    return ο + subscripted_beta
}

# ✓ paper notation uses a visually distinct, plain identifier
@ public { }
fn σ(f32 v) → f32 {
    return v
}

fn rmsnorm(tensor.Tensor x, tensor.Tensor γ, f32 ε) → tensor.Tensor {
    return x
}
```

The preferred table is intentionally small: `σ` is sigmoid; `γ` is normalization
gain; `ε` is tolerance or normalization epsilon; `θ` is the RoPE base or a model
parameter; `τ` is sampling temperature; and `α`/`β` are future optimizer
coefficients. The allowed visually distinct subset is `α β γ δ ε θ λ μ π σ τ φ ω Δ`.
Never use the Latin-lookalike codepoints `ο ν ρ ι κ χ υ ζ`, even though they look
like Latin letters on screen. Unicode subscripts are never used; plain digits are
canonical.

## kernel-region-split

**A function that mixes host policy (`⇥` handling, host-side containers, I/O, `source`, optional-bias branches) with tensor math is two functions: an ordinary wrapper that keeps the policy, and a device region the wrapper calls. The region reuses existing `@ kernel` leaves first; where the wrapper must call a new one, that kernel is `@ public @ kernel` — a private `@ kernel` is a helper only other kernels may call.**

non-preferred · Evidence: the kernel visibility rule (host code calling a private kernel is a check-time error) in `crates/radix-semantic/src/passes/kernel_legality.rs` (`nucleum_private_host_call`, reported as `SEM008`). `⇥`/`throw` is admitted inside a kernel, so a failable function may itself be a kernel when its body is host-free; the split below is for bodies that actually need host policy. Cite the legality pass in findings.

scope: device-library conversion of mixed host/math functions. Not session drivers (`decode_step`, `generate`).

```fab
# ✗ one function is both host policy and math, so the math never reaches a kernel
fn dense_block_static<size T, size D, size F>(tensor<f32, [T, D]> x, TypedDenseLayer<D, F> layer, f32 ε) → tensor<f32, [T, D]> ⇥ TransformerError {
    require ε > 0.0 throw variant ShapeMismatch {message = "epsilon"}
    let tensor<f32, [T, F]> gate ← x · layer.wg
    …
}

# ✗ rejected (SEM008 nucleum_private_host_call): a private kernel called from host code
@ kernel
fn dense_mlp(…) → tensor<f32, [T, D]> { … }

# ✓ wrapper keeps the policy; the math is a public kernel the wrapper calls
fn dense_block_static<size T, size D, size F>(tensor<f32, [T, D]> x, TypedDenseLayer<D, F> layer, f32 ε) → tensor<f32, [T, D]> ⇥ TransformerError {
    require ε > 0.0 throw variant ShapeMismatch {message = "epsilon"}
    return dense_mlp(x, nn.rmsnorm(x, layer.ln2, ε), layer.wg, layer.wu, layer.wd)
}

@ public
@ kernel
fn dense_mlp<size T, size D, size F>(
    tensor<f32, [T, D]> residual,
    tensor<f32, [T, D]> ln2,
    tensor<f32, [D, F]> wg,
    tensor<f32, [D, F]> wu,
    tensor<f32, [F, D]> wd
) → tensor<f32, [T, D]> {
    let tensor<f32, [T, F]> hidden_act ← nn.swiglu_hidden(ln2 · wg, ln2 · wu)
    return math.add(residual, hidden_act · wd)
}
```

The kernel-closure spelling (`(params) kernel ∴ expr`) that an earlier version of this entry taught is retired: it still checks today, but the `kernel` closure modifier is retired — a one-off region is a private `@ kernel` function called from a kernel, a closure inside a kernel needs no modifier, and host code uses `for … thread` (la `itera … filum`). Do not recommend it.

Decision tree for a conversion unit:

1. Body already all glyphs / `@ kernel` calls and host-free → mark `@ kernel`. Prefer calling existing kernel leaves over duplicating math.
2. Mixed policy + math → wrapper keeps the policy; the math is (1) existing leaves, else (2) a new `@ public @ kernel` the wrapper calls, or a private `@ kernel` when only other kernels call it.
3. One illegal nested call (host-side containers, `list.append` head loop, `source()`) → that call becomes an *argument* or its own later unit. Do not drag it into the region.
4. Bias presence is host policy: pass a zero tensor; the kernel always adds. No `if has_bias` in a kernel.
5. Session drivers, loaders, cache admit, sampling → not kernels. Record and skip.

## field-over-trivial-getter

**A stored class field is the public read; do not wrap it as a same-name zero-arg method that only returns the field.**

non-preferred · Evidence: a same-name getter that only returns a field gives one fact two spellings; field reads in `corpus/genus/genus.fab` (`print p.x`); the repaired shape is `class Metric { mut f32 loss; mut f32 accuracy }` read as `m.loss` (no `fn loss()` twins). File interfaces export struct fields, so `m.loss` typechecks across modules. Member `@ private` **is enforced**: reading a private member from outside the class is `SEM063 member_private_read` — so a getter whose field is private is a legitimate API, not a twin.

```fab
# ✗ stored facts duplicated as call-form twins
@ public
class Metric {
    mut f32 loss
    mut f32 accuracy

    fn loss() → f32 {
        return self.loss
    }

    fn accuracy() → f32 {
        return self.accuracy
    }
}

print m.loss()

# ✓ the field is the read
@ public
class Metric {
    mut f32 loss
    mut f32 accuracy
}

print m.loss
```

Keep a method when the body does work — computation, arguments, mutation, or a `⇥` channel:

```fab
# ✓ derived / operational surface stays a call
fn rank() → int {
    return self.shape.length()
}

fn get(list<int> indices) → f32 ⇥ TensorError {
    …
}
```

A mixed class is honest when stored facts are fields and derived facts are methods (`block.shape` next to `block.rank()`). Do not keep `shape()` merely so it matches `rank()`. A rename twin (`fn accepted_count() { return self.accepted }`) is the same anti-pattern unless the distinct name is the API; prefer the field name.

Not this entry: builtin collection methods (`list.length()`, `map.get(key)`), constructors / factories (`metric(...)`), or a getter over a field marked `@ private` (enforced). Existing twins are migration candidates — report as non-preferred with this citation, not as a canon-violation flood. Call-site conversion is `m.loss()` → `m.loss`; do not rewrite unrelated `.length()` / `.shape()` hits on other types.

## class-field-modifier

**Every class field carries an explicit modifier — `let` (per instance, set once in the construction literal), `mut` (per instance, reassignable), or `const T X = e` (type-level compile-time constant); a bare `T name` field is rejected.**

canon-violation · Evidence: `grammar/EBNF.md` `genus_field_decl` (`let` | `mut` per instance, `const` with `=` for the type-level constant; la `sit` | `varia`, `fixum`); `corpus/genus/genus.fab`; probe: a bare field is `PARSE010 field_modifier_missing`. Union shared fields (`field_decl`) keep the modifier optional.

```fab
# ✗ rejected (PARSE010 field_modifier_missing)
class Person {
    string name
    int age = 0
}

# ✓ explicit modifier
class Person {
    let string name
    mut int age = 0
    mut int limit optional
    const string HOME = "Rome"
}
```

Note the preceding entries' `class` snippets elide fields for brevity; a real class always writes the modifier.

## module-constant-const

**A module constant is `const T NAME = e` (la `fixum`) — the `=` form is the compile-time fact. Module-level `static` / `generis` is retired, and a module-level `←` binding is rejected.**

canon-violation · Evidence: `static` is retired and `const T x = e` (la `fixum`) is a typed compile-time constant; `grammar/EBNF.md` `fixum_decl` (`'=' const_init`); the "Common wrong guesses" section of `../../SKILL.md`. Probed: `static int X = 3` is `PARSE010 static_decl_retired`; `let f32 TOL ← 0.5` at module level is `SEM062 top_level_binding` (the run-time `const f32 TOL ← 0.5` is `PARSE010 runtime_const_retired`); `const int X = 3` and `const f32 Y = 0.5` check clean. `static`/`generis` is retired in class bodies too (`static_member_retired`; the class-level constant is the member `const T X = e`).

```fab
# ✗ rejected (PARSE010 static_decl_retired)
static int LIMIT = 3

# ✗ rejected (SEM062 top_level_binding): runtime store at module level
let f32 TOLERANCE ← 0.5

# ✓ compile-time constant (also legal inside a function body, and with `embed` / `comptime` initializers)
const int LIMIT = 3
const f32 TOLERANCE = 0.5
```

## true-division-glyph

**`/` and `%` on integers are floor operations; a real-valued quotient is `÷`. Never route an integer `/` into a float slot.**

non-preferred · Evidence: the Division prose in `grammar/EBNF.md` (`a / b` is ⌊a / b⌋, `-7 / 2` is −4, `-7 % 2` is 1; `÷` is real division with no method twin). Verified: `let f64 avg ← a / b` with `int` operands is rejected (`SEM010 integer_quotient_to_float_requires_true_division`); `a ÷ b` checks and yields `f64` for default `int`, `f32` for 8/16-bit operands.

```fab
# ✗ rejected (SEM010): integer quotient stored as a float
let f64 avg ← total / count

# ✓ real division
let f64 avg ← total ÷ count

# ✓ integer floor division stays `/`
let int pages ← total / page_size
```

Code ported from C, Java, Rust, or Go changes its results on negative operands: do not carry over truncating division.
