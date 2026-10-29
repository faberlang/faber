# Faber Idioms

Use this file when the task is about writing Faber naturally rather than merely legally. Default to application code, especially command-line applications and packages, before compiler-internal concerns.

Locale convention: prose names constructs by their en spellings (`class`, `const`, `return`);
the stable Latin key appears as a short `(la ...)` gloss where a reader will meet it in the corpus,
in diagnostics, or in `faber explain`.
Every fenced code block below is written in the **en** reader surface and was checked with
`faber check` or `faber run`; a file with no `+++` frontmatter and no manifest `[locale]` reads as
en, and a Latin file needs `+++ locale = "la" +++`. Spellings come from
`locale/en/pack.toml` and `locale/la/pack.toml`; never mix them in one file.

Primary stylistic authorities:

- `corpus` (frontmatter `explains` names the explain terms a file owns)
- `grammar/EBNF.md` for legality
- `crates/radix`
- `crates/faber/src` for user CLI and package workflow
- `lib/norma/src` for public `norma:*` style

## General Style

Faber code is explicit, mechanical, and keyword-forward.

Prefer:

- construct-leading keywords
- type-first declarations
- small direct blocks
- vocabulary already present in `corpus` and stdlib
- visible control flow over compressed cleverness
- `then return` (la `ergo redde`) for compact single-statement returns; `∴` is the closure joint only
- `const T X = e` (la `fixum T X = e`) for a module constant, `=` because the value is a compile-time fact; run-time names are `let` (write-once, la `sit`), `mut` (writable, la `varia`) and `ref` (re-pointable read-only, la `de`), never `const`
- bare width markers (`i32`, `u8`, `f32`, `inf`) in every type position
- the parameter's access marker at the call: `bump(mut a)`, `eat(own a)`, `dup(copy a)` (la `in` / `own` / `copy`), optional `ref`; a bare argument to a marked parameter is `SEM057 call_marker_missing`
- parameters take no `let`/`mut`/`const`: `own` and `copy` parameters are writable by the callee, `mut` writes the caller's value, unmarked and `ref` are read-only; `own` takes only a whole local binding or a fresh value (`eat(own make())`), and `a` is dead after `eat(own a)` (`SEM050 use_after_own`); to give away a field write `dup(copy b.xs)` instead
- a comment on its own line: `#` must be the first non-whitespace token, and `code # note` is `LEX007 inline_hash_after_code`
- `async` / `generator` / `async_generator` (la `fiet` / `fiunt` / `fient`) for new async and stream code
- the long type spellings (`list<f32>`, `tensor<f32, [2, 2]>`) in general code; the width sugar (`lf32`, `tf32[2, 2]`, bare `f32`) belongs in numeric and tensor code. Match the surrounding file

Avoid:

- inventing synonyms
- JavaScript or Rust habits pasted through a Latin thesaurus
- hiding meaning behind dense helper layers
- mixing stylistic dialects in one file
- sugaring every type with `tf32` / `lf32` outside numeric and tensor code
- nullable or optional syntax that is not in `EBNF.md` (`T?`, prefix `if` (la `si`) on types, etc.); the null type is `T ∪ none` in en and `T ∪ nihil` in la, and the null value is `null` (la `nulla`)
- the retired wrapped numerics `int<i32>` / `float<f32>` (la `numerus<i32>` / `fractus<f32>`; `PARSE040 numeric_wrapper_retired`); write `i32`, `f32`
- `static` (la `generis`) anywhere: a module-level `static` is `PARSE010 static_decl_retired` and a class member `static` is `static_member_retired`; a type-level constant is a class member `const T X = e`
- the retired binding words: en `var` (now `mut`, `PARSE010 var_retired`) and a run-time `const T x ← e` (now `let`, `PARSE010 runtime_const_retired`; la `fixum T x ← e` becomes `sit`)
- `@ future` / `@ cursor` (la `@ futura`) in new code
- anonymous `{ key = expr }` objects — use JSON `{ "key": constant }` or `Type { field = expr }`

## CLI Application Style

For command-line applications, prefer:

- one clear `main` or `main args args` entry surface (la `incipit` / `incipit argumenta args`)
- command functions with typed parameters and direct names
- explicit validation before work begins
- predictable `write`, `warn`, `debug`, or `print` output (la `scribe`, `mone`, `vide`, `nota`) instead of hidden side effects; `print` of a `class` (la `genus`) shows its construction literal (`Click { x = 1, y = "hello" }`), and formatting goes through `"Total: §"(value ¶ ".2")`; `sink ⇇ payload` is the typed-sink alternative that adds no formatting
- a failable `↦` needs `⊥ default`, a catch, or an enclosing `⇥ E`; `main` is exempt and panics
- small modules imported through canonical `import` forms (la `importa`)
- concrete error paths using `do` / `catch`, `throw`, and `panic` (la `fac` / `cape`, `iace`, `mori`)

Before using CLI annotations or argument-binding forms, verify current support in:

- `corpus/cli/cli.fab`
- `crates/faber/src/cli/`
- `crates/faber/src/commands/`
- live compiler / driver code

`faber check` on `corpus/cli/cli.fab` is the proof that the current annotation spellings are legal.

`faber run` is interpreted execution, always (single file, package, manifest, or
archive) through the MIR runner; faber does not execute compiled artifacts.
Build with `faber build -t <target>`; device packages execute through the host
driver. Treat compiled-target execution as unsupported unless the host driver
explicitly implements it.

## Package Style

Package-aware work uses:

- `faber.toml` for package authority; the accepted sections are the fields of `FaberManifest` in `crates/faber/src/package/manifest.rs` (`[package]`, `[paths]`, `[build]`, `[locale]`, `[library]`, `[device]`, `[product]`, `[target.*]`, `[dependencies]`, `[dispatch]`, `[check]`; unknown keys are rejected)
- optional `+++` frontmatter on `.fab` files for `locale`, `group`, `sectio`, `[probanda]`, and package-facing metadata
- `faber init`, `faber build`, `faber check`, `faber run`, `faber test`

Do not invent package manifest keys. Read a working package first, for example `lib/norma/exempla/error-conversio/` (`faber check` on the directory), the fixtures under `corpus-product/`, and `crates/faber/src/package/`.

## Stdlib Style

Collection primitives (en `string`, `int`, `float`, `list<T>`, `map<K,V>`, `set<T>`; la `textus`, `numerus`, `fractus`, `lista<T>`, `tabula<K,V>`, `copia<T>`) are compiler-owned.

User-facing `norma:*` modules live in `lib/norma/src/` (flat, English-spelled module names: `norma:fs`, `norma:text`, `norma:json`, `norma:console`, `norma:time`; `lib/norma/src/*.fab` is the module list). Optional libraries live in their own repos and are not universal stdlib. Generated-code runtime types live under `runtime/`.

When calling methods such as `myList.append(x)` (la `appende`), check the live library source and the relevant Radix lowering path before assuming whether the method is native Faber, `call`-routed (la `ad`), deferred with `panic` (la `mori`), or runtime-backed.

## Naming Rhythm

The codebase commonly uses:

- Latin or Latinized nouns for data and types
- verbs for actions and transforms
- descriptive compound names in compiler internals such as `FuncDecl`, `HirProgram`, `parse_func_decl`

When writing user-facing Faber examples, keep names short and semantically sharp:

- `name`, `word`, `person`, `event`, `item`, `mem`

Optimize for semantic distinctness under repeated model use, not literal English-to-Latin translation.

When writing compiler-adjacent code, follow established names in `crates/` for AST nodes, parser helpers, semantic passes, HIR/MIR nodes, and codegen modules.

## Formatting Patterns

From exempla:

- opening brace on the same line (Stroustrup)
- one statement per line
- four-space indentation
- every class field declares exactly one of `let` / `mut` / `const` (la `sit` / `varia` / `fixum`), written `<modifier> <type> <name>`: `let` is per instance and set once in the construction literal, `mut` is per instance and reassignable, `const T X = e` is the type-level compile-time constant read as `Person.X` (a store is `SEM020 assignment_to_const_member`; `ref` on a field is `ref_field_refused`); an unmarked field is `PARSE010 field_modifier_missing`
- default field values written with `=`
- explicit `return` (la `redde`) for returned values inside blocks

```fab
class Person {
    mut string name
    mut int age = 0
    const string city = "Rome"
}

fn name() → string {
    return "Mark Aurelius"
}
```

`faber field-modifier-codemod <PATH>` migrates an old file whose fields have no modifier.

## Conversion-Directed Assignment (`↤`)

Prefer `place ↤ value` when the right side needs the `↦` route to reach the
place's type and the conversion is the point of the line:

```fab
main {
    mut int n ← 0
    n ↤ "42"
    let int total ↤ "7"
    n ↤ "x" ⊥ 0
    mut int a ← 0
    mut int b ← 0
    a ↤ b ↤ "42"
    a ↤ b ↤ "x" ⊥ 0
    print n, total, a, b
}
```

`↤` binds at the same precedence as `←` and is right-associative
(`grammar/EBNF.md`, "Conversion-directed assignment"): `a ↤ b ↤ "42"` is
`a ↤ (b ↤ "42")`. The failure default is `⊥ value` (`inline_default`), never
`⇥` (`⇥ 0` is `PARSE030 recovery_uses_exit_arrow`: `⇥` names an error type).
In a right-associated chain the `⊥` attaches to the nearest `↤`: after
`a ↤ b ↤ "x" ⊥ 5` both `a` and `b` are 5.

Keep ordinary `←` for plain stores, and keep explicit `↦` for conversions
whose destination is not an assignable place. `⊥` is legal only after a
failable call, a `↦ T`, or a `↤`; after ordinary `←` it is
`PARSE030 default_requires_failable`. Typed `let` / `mut` (la `sit` / `varia`) initializers
accept `↤` (`fixum_decl` in the EBNF). `↤` never appears on `mut _`, inferred `let`,
or untyped destructuring: they have no concrete destination and reject
`PARSE002 conversio_assign_requires_concrete_type`. Outside `main` an
unhandled failable `↤` / `↦` needs `⊥`, a catch, or an enclosing `⇥ E`.
A convert hint is a `via` clause, never a type argument: `"ff" ↦ i32 via Hex`.

## Exact Big Integers: Opt In With `inf`

Nothing becomes `inf` by default. Declare a slot `inf` (or convert an operand `↦ inf`) where an exact
unbounded result is the point — a factorial, a 64-bit difference that must not trap, a number past `u64`
— and narrow back with a checked `↦` when the value has to fit a bounded slot:

```fab
fn factorial(inf n) → inf {
    if n ≤ 1 then return 1
    return n * factorial(n - 1)
}

main {
    let u64 a ← 5
    let u64 b ← 7
    # u64 - u64 itself is i64 (and a checked store into u64 would trap), so opt in with ↦ inf
    let inf delta ← (a ↦ inf) - b
    let inf big ← factorial(30)
    print delta
    print big
    # narrow back: 30! does not fit u64, so the default 0
    print big ↦ u64 ⊥ 0
}
```

Teaching exemplars: `corpus/operatores/inf.fab` (the whole surface), `inf-conversio.fab`
(`list<b8> via Be`/`Le`, `value`), `approx-integers.fab` (the exact integer `≈` rule). `inf` is host-only
(not a tensor element, not in a kernel or an AIR lane). Swift, Haskell, LLVM and Wasm have no unbounded
carrier and fail closed by name (`inf_target_unsupported`, `llvm_target_inf_unsupported`,
`mir_wasm_unsupported`); see "The unbounded integer `inf`" in `grammar/EBNF.md` before promising a
target. Never write `∞` for it: `∞` is the float infinity.

## Numbers: Exact Values, Checked Stores

Short form of the "Numeric model" in `grammar/EBNF.md`:

- Arithmetic computes the exact value; the **slot** applies its policy at the store. A bare marker
  (`u8`, `i32`) traps when the value does not fit; `wrapping<u8>` (la `modulus<u8>`) reduces;
  `saturating<u8>` (la `saturatus<u8>`) clamps. Policy words keep their `<W>`; the bare marker is the
  canonical width spelling. A range refines the policy: `saturating<u32, 0‥8>` clamps into 0 to 7,
  `trapping<i32, -2‥3>` traps outside -2 to 2, `wrapping<u32, 0‥5>` wraps around 5 slots (`‥` half open,
  `…` inclusive, both ends written, `∞` for no bound); the range applies at the store only.
- `/` floors on integers and `%` takes the divisor's sign (`-7 / 2` is -4, `-7 % 2` is 1); `÷` is true
  division and yields a float. Writing `let f64 avg ← a / b` with integer operands is
  `integer_quotient_to_float_requires_true_division`; write `a ÷ b`.
- Crossing number families (integer, `d64`, float) always needs `↦`.

```fab
main {
    let u8 a ← 200
    let u8 b ← 100
    let _ sum ← a + b
    let int q ← -7 / 2
    let f64 r ← 7 ÷ 2
    print sum, q, r
}
```

## Exempla Comment Headers

Runnable exempla (`corpus/<term>/<name>.fab`, one file each, plus an optional `<name>.expected`
stdout sidecar) open with `+++` TOML frontmatter and a `#` banner:

```fab
+++
term = "itera"
kind = "keyword"
category = "control-flow"
canonical = true
summary = "Cartesian walk of two ranges with two binders."
syntax = "for range <a>‥<b>, <c>‥<d> let <i>, <j> { }"
related = ["ex", "de", "apud"]
locale = "en"
+++
# =============================================================================
# <name> — one-line description.
# =============================================================================
#
# What this teaches:
#   • ...
#
# Common mistakes:
#   • ...
#
# See also: ...
# =============================================================================
```

The frontmatter `term` and `related` values are Latin registry keys, so `itera` is en `for`, `ex` is `from`,
`de` is `ref`, and `apud` is `at`. The frontmatter also carries `grammar = [...]` (EBNF production names) and feeds `faber explain`; the
`locale` key is mandatory for a non-default surface. Older files still carry `GRAMMAR:` / `EXPECTED OUTPUT:` /
`BACKEND:` comment blocks; new exempla use the shape above and put expected stdout in the `.expected` sidecar.

## Semantic Tone

Preserve visible distinctions:

- `let` versus `mut` versus `ref` versus `const` (la `sit` / `varia` / `de` / `fixum`): write-once name, writable name, re-pointable read-only name, compile-time constant
- `class` versus `interface` (la `genus` / `implendum`)
- `enum` versus `union` (la `ordo` / `discretio`)
- `switch` versus `match` (la `elige` / `discerne`): `match` matches a closed set only (variants of a `union`/`enum`, or members of a union), and a pattern outside it is `SEM010 discerne_pattern_not_in_closed_set` (`case u32` over an `int`, `case list<int, size 4>` over a `list<int>`); an open scrutinee uses `switch`, and an integer's width or a length is asked with `is` or `.length()` in an `if`
- `ref` versus `mut` versus `from` (la `de` / `in` / `ex`)
- `T ∪ none` (la `T ∪ nihil`) versus `optional` (la `sponte`) slots
- `async` / `generator` / `async_generator` (la `fiet` / `fiunt` / `fient`) versus ordinary functions
- JSON `{ "key": constant }` versus `Type { field = expr }`

Do not flatten these into generic “type”, “class”, or “match” language without naming the actual Faber construct.

## Practical Rule

When you need authentic Faber:

1. read one small exemplar under `corpus/`
2. read `EBNF.md` for legality
3. read one nearby Radix implementation file if the feature is compiler-relevant
4. reuse the repository’s exact vocabulary and ordering

If your draft could plausibly be “Latin-flavored pseudo-code” rather than repo-authentic Faber, it is not done.

## Tensor and vector glyph operators

Canonical tensor/vector arithmetic goes through glyph operators, **not**
method-call spellings (`a.matmul(b)` is the non-preferred spelling; prefer
`a · b`):

| Glyph | Operation | Notes |
| --- | --- | --- |
| `·` | dot / matmul | **dispatches by rank**: rank-2 `·` rank-2 is matrix multiplication (inner-dim contract); vector `·` vector is the dot product |
| `⊗` | outer product | builds a matrix from two vectors (register matrices have no literal construction surface — this is it) |
| `⊙` | elementwise | Hadamard product; operands unify shapes at typecheck |
| `⊘` | elementwise divide | Hadamard divide; operands require identical shapes at typecheck |
| `×` | cross product | 3-vector cross |
| `ᵀ` | postfix transpose | rank-2 tensor only in v1; `[M,N] → [N,M]`, with rank-1 permanently declined because there is no row/column distinction (`glyph_transpose_operands_unsupported`) and rank-3+ deferred until a batched-transpose consumer; lowers through the existing `Transpose` plan |

The glyphs act on `tensor` / `vector` / `matrix` values (`vf32[3]`, `tf32[4]`, `matrix<f32, [2, 3]>`):

```fab
main {
    let vf32[3] a ← [1.0, 2.0, 3.0] ↦ vf32[3]
    let vf32[3] b ← [4.0, 5.0, 6.0] ↦ vf32[3]
    let _ dot ← a · b
    let _ outer ← a ⊗ b
    let _ had ← a ⊙ b
    let _ quot ← a ⊘ b
    let _ cross ← a × b
    let _ gram ← outer · outerᵀ
    print dot, had, quot, cross, gram
}
```

Scalar operators also lift elementwise over a `tensor` (`+ -` against a scalar or equal shape, `*`, `/`,
`%`, `÷` by a scalar, `⤒ ⤓`, the comparisons, `✓ ✗` selects), so `a + b` and `a / 2.0` on `tf32[3]` need
no loop. `tensor * tensor` is rejected (`numeric_operands_required`): write `⊙` for the elementwise
product. See "Tensor lifting" in `grammar/EBNF.md`.

## Iteration and reduction — pick the construct, not the loop

Before writing any loop, check whether a dedicated construct states the intent:
- Element walk with coordinates → `for from x at [i]` (la `itera … apud`).
  Never hand-index with a counter loop when `at [...]` exists.
- Lockstep walk of N aligned lists → `for from a, b let x, y`
  (la `itera ex a, b sit x, y`). N sources carry N name binders; one
  optional shared `at [i]` names the common index and never grows with N.
  Lengths must be equal — statically known lengths are compared at check and
  dynamic lengths abort before the first body visit (fail-closed, never
  truncated to the shorter list). Binders are names in v1; patterns stay on
  the single-source form. Authority: `corpus/itera/lockstep-n2.fab`,
  `lockstep-n3.fab`, `lockstep-at-index.fab`; declines in
  `lockstep-decline.fab`. Never keep aligned lists together with a hand-written
  counter — the lockstep series exists exactly for that index-free shape.
- Cartesian walk of N declared ranges → `for range a‥b, c‥d let i, j`
  (en). N ranges yield the full output-space product in leftmost-outermost
  row-major order: the first binder is the outer loop, the last the inner.
  Use it when the product itself is the declared output space (grids,
  pair enumeration, coordinate sweeps); keep `for from a, b let x, y`
  for aligned zip walks over containers — ranges multiply, sources align.
  Reductions stay explicit (`reduce via sum` and its sibling operators); the range walk only
  visits coordinates. Authority: `corpus/itera/range-product-n2.fab`,
  `range-product-n3.fab`; declines and semantics in
  `range-product-decline.fab` and `range-product-semantics.fab`.
- Sum/reduce over an axis → `reduce via sum from` (la `reducta via summa ex`).
  A body returns one term per element:
  `reduce via sum from a at [i] let s { return s }`
  (exemplar `corpus/summa/summa-en-no-thread.fab`); `thread f` selects
  the distributed kernel-plan path inside a kernel entry. Never hand-accumulate.
- Minimum/maximum over a shaped tensor → `reduce via min from` / `reduce via
  max from` (la `reducta via minima` / `reducta via maxima`). The
  canonical form is bodiless — `reduce via max from values` on a statically
  non-empty `tf32[4]` is a plain `f32`
  (`corpus/summa/extrema-no-coalesce.fab`) — and an expression operand
  states its empty-source identity: `reduce via max from (row ⊙ scale)
  coalesce 0.0`. Without it, the nullable result needs handling. A reduction
  body can use `at [...]` coordinates. Empty-possible shapes return `T ∪ none`;
  state the caller's identity with `coalesce` (`corpus/summa/max-min.fab`).
- Best index over a list → use the twin method instead of a hand-rolled scan.
  Library code now calls the twin directly (`logits.argmax() coalesce 0`);
  the scan it replaced looked like this:

  **Before — hand-rolled `_max_index` loop:**

  ```fab
  fn _max_index(list<f32> xs) → int {
      mut int best ← 0
      mut f32 best_val ← xs.get(0) coalesce 0.0 ∷ f32
      for range 1‥xs.length() let i {
          let f32 v ← xs.get(i) coalesce 0.0 ∷ f32
          if v ≻ best_val {
              best_val ← v
              best ← i
          }
      }
      return best
  }
  ```

  **After — dedicated best-index method:**

  ```fab
  let int best ← xs.argmax() coalesce 0
  ```

  `argmax()` and `argmin()` share the first-index tie law: among equal
  winners the least index wins. The same law is the parallel-combine law:
  partials pair as `(value desc, index asc)`, so sequential scan and
  tree-combine agree. An empty source yields the null (`int ∪ none`), so
  compose `coalesce` (la `vel`) at the call site; never use a `−1` sentinel.
  NaN behavior is aligned with `reduce via max from`'s implemented fold: NaN
  never wins while a number participates, and an all-NaN source keeps index 0.
  `argmin()` mirrors `argmax()` for the least value.
- Mathematical max identity for empty-possible input →
  `reduce via max from xs coalesce -∞`
  (la `reducta via maxima ex xs vel -∞`). Use `-∞`, not a runtime `0.0`:
  zero changes the result for all-negative data, while `max(x, -∞) = x`.
  `coalesce` is the caller-supplied fallback for the nullable empty reduction,
  not an invented reduction identity. For finite/NaN detection, read the bit
  fields through `x ↦ b32 ↦ u32` (`corpus/scalar/finite-nan.fab`), not
  `x ≢ x`.
- Dot/matmul/outer/elementwise → the tensor glyphs (`·` `⊗` `⊙` `⊘` `×`, see the
  table above). Never hand-compose nested loops for these — including in
  device bodies, where the glyphs lower through plan recipes.

### Error-conversion seams: delete administrative wrappers

Each direct source-to-destination mapping is one named standalone conversion
function enrolled in the shared table. At an administrative boundary, declare
the conversion function and propagate the failable call; do not repeat the
same reconstruction in every wrapper. (The historical in-union arm form is
retracted under schema 7 — a union is pure fields plus variants.)

**Bad — wrapper family that replays the boundary mapping:**

```fab
fn inspect_before() → string ⇥ BoundaryError {
    do {
        return read_wire()
    }
    catch err {
        throw variant WireFailure {message = err.message, inner = err}
    }
}
# WireError and BoundaryError are the unions declared in the Good block below.
```

**Good — one declared conversion function plus direct propagation:**

```fab
@ conversion
fn from_wire_error(WireError err) → BoundaryError {
    return variant WireFailure {message = err.message, inner = err}
}

union BoundaryError {
    @ shared
    string message

    WireFailure { WireError inner }
}

fn inspect_after() → string ⇥ BoundaryError {
    return read_wire()
}
```

The function's whole-source parameter binds the source value. In English,
name it `err`: `self` is reserved for the method receiver (`ego` in the
canonical registry), so `self.message` is not a conversion binding outside a
method. In Latin, `ego` is the reserved receiver spelling and `self` is not
reserved by the `la` pack, but `err` remains the cross-locale teaching
spelling.

This rule is only for administrative alias/re-export boundaries. At a trust
boundary, keep an explicit `match err` in the conversion-function body and
account for the source variants deliberately. The lookup is direct and
fail-closed: a missing pair reports `failable_call_conversion_absent`, never
an auto-composed chain. One row per concrete pair; duplicates report both
spans, builtin rows cannot be shadowed, the declaring module must own an end
of the pair, and the body is a pure re-presentation. The live pair is
exercised by `lib/norma/exempla/error-conversio/` (`faber check` on the directory).

## Container walks preserve the bound

`for range 0‥rows` is the wrong construct when `rows` was re-derived from the
extent of the bounded or shaped container that the body then indexes. This is
a **LOST STATIC FACT**, not a style preference: the range form severs the loop
from the thing it iterates. The stride math is no longer mechanically
verifiable, and an out-of-bounds access stays writable when `get(...) coalesce …`
turns a failed lookup into a fallback value.

The anti-pattern has this shape (a hand-built extent plus stride indexing):

**Bad — re-derived extent plus stride indexing:**

```fab
fn sum_bad(list<f32> data, list<int> shape) → f32 {
    let int rows ← shape.get(0) coalesce 1
    let int cols ← shape.get(1) coalesce 1
    mut f32 total ← 0.0
    for range 0‥rows let i {
        for range 0‥cols let j {
            total ← total + (data.get(i * cols + j) coalesce 0.0 ∷ f32)
        }
    }
    return total
}
```

**Good — walk the container and keep any condition in the body:**

```fab
fn walk_good(matrix<f32, [2, 3]> grid) → void {
    for from grid at [r, c] let cell {
        if r ≤ c {
            print r, c, cell
        }
    }
}
```

The good form keeps the container, its extent, and its coordinates in one
construct. `corpus/itera/apud-rank2-matrix.fab` is the
rank-2 walk authority, and `corpus/itera/apud-index-causal.fab`
spells out that the condition belongs in the body and the loop walks the full
grid. `corpus/itera/apud-decline.fab` records the matching
fail-closed boundaries: the `at` coordinate count must equal the iterable rank
(`itera_apud_arity_mismatch`, which also rejects a ragged `list<list<T>>`
walked with two coordinates), a `set` (la `copia`) has no index to name
(`itera_apud_requires_indexed_iterable`), and generic dimensions decline at MIR
when they are not concrete at lowering.

This rule is specifically for range bounds that re-derive a container extent
and then manually index that same container. `for range` remains correct for a
genuinely count-like loop — repeat N times, protocol steps, or a numeric
sequence with no container being walked. LLM training priors push toward the
range-plus-index form because most languages lack this container iteration; in
Faber, the container walk **is** the default. When several index-aligned lists
walk together, the same rule reaches the lockstep series
(`for from a, b let x, y`) instead of a shared counter.

Mechanical enforcement is partial: a literal
index past a static bound is `SEM011 index_provably_out_of_bounds` at check
time, and a dynamic index past a bounded or exact-shape receiver is a hard
runtime error. Re-deriving an extent and indexing by arithmetic is still a
cultural rule, not a checked one.

A hand-rolled counter loop, manual accumulator, or guarded full-tensor scan in
place of one of these is non-idiomatic.

- Host exemplars: `corpus/tensor/glyph-matmul.fab`, `glyph-outer.fab`,
  `glyph-elementwise.fab`, `glyph-hadamard-divide.fab`, `glyph-transpose.fab`
  (rank-dispatch WHY notes and expected outputs).
- **Device form**: glyph operators are legal in `@ kernel` (la `@ nucleum`) bodies
  and lower through the plan recipes — `out ← a · b` is the glyph spelling of
  the tiled matmul recipe
  (likewise `⊗` `⊙` `×` and `ᵀ`). Kernel bodies are checked at
  `faber check` time for the host-free subset with named `nucleum_*`
  diagnostics (`corpus/kernel/legality-reject.fab`). Tensor/matrix iteration
  coordinates in a kernel use `i32`; host iteration coordinates remain `int`
  (AIR retains its constant-extent narrowing). Kernel reduction coordinates,
  `count` results and `argmax`/`argmin` coordinates use the same `i32` carrier
  (including tuple, optional and grouped results); host reductions keep `int`. Kernel launch requirements
  prove the coordinate extent and exact index arithmetic fit; an unproven
  tensor access still carries its runtime bounds guard and WARN033. A kernel may call
  kernels, and a private kernel is not callable from host code. The GPU path is
  not a product surface today, so prefer a corpus exemplar over a claim about
  execution.
- GEMV is the rank-1 case: `(N,K) · (K,) → (N,)` reads naturally as
  `output ← weights · input`.

## Highlight layer: constructs beyond the core scan

This section is intentionally selective. It points at features that are easy
to miss in a scan of the core declarations or that have an active library
consumer. The grammar and corpus remain the authorities; these are authoring
reminders, not a second EBNF.

### Shared fields on tagged unions (`@ shared`, la `@ commune`)

- **What:** A shared-field region on a tagged union declares one field carried
  by every variant, so a union value can use direct member access such as
  `e.message`.
- **Why:** It avoids a handwritten match wrapper for a field that is present on
  every error shape. Variant arms still bind it, shared fields first:
  `case ElementMismatch let message, expected, actual { … }`.
- **Exemplar:** `corpus/commune/commune.fab` (`locale = "en"`) uses
  `@ shared` on `ShapeError.message`, matches it in `case` arms, and reads
  `e.message` directly.
- **Wrong form:** Do not redeclare the shared field inside a variant, and do not
  put the annotation on an ordinary genus field (`SEM018 commune_on_non_union`). The current parser rejects
  the redeclaration with `PARSE001:union_variant_redeclares_shared_field`.

### Generic class and call-site application

- **What:** A generic class puts its type parameter in fields and methods, then
  applies that parameter in a type such as `Box<tf32[4]>`; explicit function
  call-site arguments use the same angle-bracket slot, such as
  `identitas<string>("creata")`.
- **Why:** The declaration and the application are separate surfaces. A class
  scan can show `<T>` without making it obvious where a concrete type or a
  compile-time size is supplied.
- **Exemplar:** `lib/norma/src/deque.fab` is a live `class Deque<T>` with `T` fields
  and methods; `corpus/functio/generic-call-type-args.fab` checks an
  explicit call-site type argument. The compact checked probe for the same
  shape is `class Box<T>` with `Box<tf32[4]>`.
- **Wrong form:** `tf32` is a shape-bearing tensor sugar, not a scalar. A bare
  `Box<tf32>` is rejected in the current check (`expression_type_mismatch`), so give a
  shape witness such as `Box<tf32[4]>`. `size` (la `magnitudo`) parameters
  follow the type parameters (`<T, size N>`, `max_dynamic<size Figura>` in
  `corpus/summa/max-min.fab`); an applied natural-number argument is a
  capacity fact, not a width marker. A call passes an explicit size behind
  the same word, as `axis` is passed: `mk<int, size 4>(xs)`,
  `tensor.construct<int, size [4]>(xs)`; a bare natural there is
  `SEM057 size_marker_required`.

### Kernel fragments versus graphics fragments

- **What:** The role is spelled `@ kernel fragment` in the English reader
  surface (la `@ nucleum fragment`, the stable registry spelling). It is a device-composition helper, while
  `@ fragment` is the graphics fragment stage.
- **Why:** A kernel fragment is not a dispatch entry. It has no entry ABI,
  reflection identity, standalone symbol, or runtime address; it is composed
  into an `@ kernel` entry.
- **Exemplar:** the surface spelling is checked by
  `corpus/annotation-sugar/nucleum-fragment.fab` (la `@ nucleum { fragment = verum }`;
  its prose still calls the role a "fragment-shader entry", which the tests below contradict).
  The fragment role and its call matrix are covered
  by the behavioral corpus in
  `crates/radix-module/src/mir/fragment_corpus_test.rs` and
  `fragment_instantiation_test.rs`. The en spelling `@ kernel fragment` parses.
- **Wrong form:** Do not use graphics `@ fragment`, call an entry from a
  fragment, make a fragment async/failable, create a cycle, or leave a
  fragment call/symbol after composition. The existing tensor glyph and
  sum/thread guidance above still governs device arithmetic; this section does
  not replace it.

### Tagged-union construction and case binding

- **What:** Construct a payload variant with `variant Tag { field = value }`
  and bind that payload in a match arm with `case Tag let field { … }`.
- **Why:** The `variant` marker distinguishes tagged-union construction from
  genus construction, JSON, and an anonymous object; the case binder exposes
  payload fields without a second unpacking helper.
- **Exemplar:** `corpus/unio/unio.fab` checks `variant Numerus { v = 7 } ∷ Forma`
  and `case Numerus let v { … }` under `match`; the shared-field form is also exercised by
  `corpus/commune/commune.fab`. A `union` with several payload variants separates them
  with commas.
- **Wrong form:** Do not write `Type { … }` when the target is a tagged variant,
  and do not treat `∪` as a set operator. `∪` in a type alias is an inline value
  union; the declared `union` (la `discretio`) is the tagged form.

### Recoverable `require` (la `requirit`)

- **What:** `require condition throw error` (la `requirit … iace …`)
  enters the function's declared `⇥ E` channel when the condition is false.
- **Why:** It is the recoverable twin of `assert` (la `adfirma`): the caller can
  catch or propagate the typed error instead of taking the fatal path. This
  is a real library idiom.
- **Exemplar:** `corpus/meta/requirit.fab` checks the success path, the
  catchable false path, and the required `⇥ GuardError` declaration.
- **Wrong form:** Do not use `require` in a function with no alternate exit,
  and do not substitute fatal `assert` when the API promises recovery.

### Reject (la `reice`)

- **What:** `reject condition throw error` (la `reice … iace …`) enters
  the function's declared `⇥ E` channel when the condition is **true** — the
  boolean opposite of `require`; `require not (p) throw e` is spelled
  `reject p throw e`.
- **Why:** It removes hand-rolled negation at guard sites; compound
  `require not A or B` forms become `reject (A and not (B))` by De Morgan.
- **Exemplar:** `corpus/meta/reice.fab`.
- **Wrong form:** Do not read `reject` as `require` — it fails when the
  condition holds; and like `require`/`throw`, it is a compile error in a
  `⇥`-less function.

### Labeled tuple values (`tuple`, la `iuncta`)

- **What:** A tuple may label type slots (`tuple<gx: string, gw: int>`),
  construct positionally, and read a labeled member (`pair.gx`) or a literal
  position (`pair[0]`).
- **Why:** Labels make multi-value API results readable without changing tuple
  identity or giving up positional access.
- **Exemplar:** `corpus/literalia/labeled-elements.fab` checks labels,
  construction, holes, by-label destructuring, rest binding, and identity
  erasure.
- **Wrong form:** Do not use `.0`, label a non-`tuple` type argument, or put
  the union hole `∪` in an element slot. Use a binary cup inside the slot when
  the element itself is a union.

### Precision-tagged instants (`instant`, la `instans`)

- **What:** `instant`, `instant<ms>`, `instant<us>`, and `instant<ns>` are the
  absolute-time carrier and its precision contract in the English surface.
- **Why:** Precision is part of the conversion contract: use `↦` to parse,
  narrow, widen, or emit the wire form, and keep sub-second equality explicit.
- **Exemplar:** `corpus/instans/instans.fab` checks wall-clock precision,
  offset normalization, TOML provenance, and cross-precision conversion; the
  Norma `time`, `toml`, and `value` modules consume the same carrier.
- **Wrong form:** Do not use `∷` for datetime parsing or treat `instant<ms>`
  as a formatting alias. Static ascription and runtime conversion are distinct.

### Annotation locale sealing

- **What:** Annotation spellings are selected by the file's reader locale just
  like keywords and types. In an English file the registered device annotation
  is `@ kernel`, not `@ nucleum` (the la spelling).
- **Why:** An annotation from another pack must fail closed instead of silently
  becoming a different compiler role. The current binary reports
  `PARSE018:wrong_locale_annotation` for the mixed English form.
- **Exemplar:** English device sources such as
  `corpus/vector/kernel.fab` use `@ kernel`
  (a la package would use `@ nucleum`); `references/locales.md` records the locale-diagnosis route.
- **Kernel tiers:** `@ kernel` takes an optional tier word, `@ kernel relaxed` or
  `@ kernel unsafe` (la `@ nucleum laxus` / `intutus`, provisional); `unsafe`
  includes `relaxed`, and the default tier is IEEE-correct (`corpus/kernel/tier.fab`).
  Tier words follow the file's locale like every other keyword.
- **Wrong form:** Do not copy `@ nucleum` (la) into an `en` file, or mix `@ shared`
  with a locale's other annotation spellings. Resolve the pack before judging
  an annotation; see `references/locales.md`.

The tensor glyph table and the iteration/reduction section immediately above
are existing highlights, not grammar to repeat here. Keep their cross-links
when adding a library-specific example.
