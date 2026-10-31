# Faber Grammar

Use this file when the task is about syntax, grammar, or whether a construct is legal Faber.

Primary source:

- `grammar/EBNF.md`

Secondary source:

- `corpus/`

This file is a compact map. Quote the live EBNF section before claiming a construct exists.

## Locale spellings (read this first)

Faber's reader surfaces are **sealed per locale**: one file, one language.
**`en` is the default reader**, and this file spells everything in it: a file
with no locale frontmatter (and no package pin) reads as en. An en file may
state `locale = "en"` (most exempla do) and uses the English rows of
`locale/en/pack.toml`; the other packs sit beside it in `locale/`. The
compiler's canonical grammar names, and the Latin (`la`) locale spellings, are
Latin (`itera`, `summa`, `dum`, `si`), so a Latin file must say
`locale = "la"` in its `+++` frontmatter (or the package must pin it). Mixing
spellings inside one file is always a defect. Any Latin sample needs
`locale = "la"` to run as written.

The en forms writers most often need (all probed against the built binary):

| en spelling | Canonical (la) | Use |
| --- | --- | --- |
| `for from x` / `for ref m` / `for range a‥b step k` | `itera ex x` / `itera de m` / `itera ab a‥b per k` | iterate values / map keys / integer range |
| `for from … at [i, j]` | `itera … apud [i, j]` | coordinate-binding walk of a tensor/matrix/list/map (binds indices; tensor rank must match) |
| `reduce via sum from x at [i]? thread f let s { … }` | `reducta via summa ex x apud? filum f sit s { … }` | device sum reduction (see below) |
| `while` | `dum` | pre-test loop (`do … while` = post-test; la `fac … dum`) |
| `if` / `elif` / `else` | `si` / `sin` / `secus` | conditionals |
| `then` | `ergo` | one-statement body joint |
| `let` / `mut` / `ref` / `const` | `sit` / `varia` / `de` / `fixum` | run-time bindings (write-once / writable / re-pointable read-only) and the compile-time constant (`const` / la `fixum` takes `=` only) |
| `fn` / `main` / `print` | `functio` / `incipit` / `nota` | declaration, entry, output |
| `class` / `interface` / `implements` | `genus` / `implendum` / `implet` | classes |
| `union` / `enum` / `variant` | `discretio` / `ordo` / `finge` | tagged unions, enums |
| `match` / `switch` / `case` / `default` | `discerne` / `elige` / `casu` / `ceterum` | dispatch |
| `do … catch` / `guard` | `fac … cape` / `custodi` | handler block, guards |
| `coalesce` / `null` / `none` | `vel` / `nulla` / `nihil` (type) | nullable default, null value, null type |

**Canonical exemplars to copy, never transliterate:** en-locale iteration lives
in `corpus/itera/` (see `apud-rank1.fab`, `apud-rank3.fab` and the en
files there), the device reduction in `corpus/summa/summa-en.fab`
(`reduce via sum from … at [i] thread f let s { … }` inside a kernel entry), and tensor
glyph arithmetic in `corpus/tensor/glyph-*.fab`. When writing en Faber, open the nearest
exemplar first and match its spellings exactly.

## Core Shape

Programs are statement sequences after optional `+++` TOML frontmatter peeled by the driver.

Major statement families include:

- declarations: `fn`, `class`, `interface`, `type`, `enum`, `union`
- imports: `import`; file name: `module` (below)
- flow and control: `if`, `while`, `for`, `switch`, `match`, `guard`, `do`/`catch`
- imperative forms: `return`, `break`, `continue`, `throw`, `panic`, `assert`, `write`, `warn`, `debug`, `print` (la `redde`, `rumpe`, `perge`, `iace`, `mori`, `adfirma`, `scribe`, `mone`, `vide`, `nota`)
- entry and test forms: `main`, `async_main`, `test`, `describe` (la `incipit`, `incipiet`, `proba`, `probandum`)
- inferred locals: `let`

## File Frontmatter

Optional `+++` TOML on line 1 is peeled by the driver before lexing; it is not token grammar. Tooling reads known metadata such as `group`, `sectio`, `[probanda]`, and package keys consumed by `faber`.

Line-start `§` file directives were removed. Inside quoted strings, `§` remains the string-template hole.

## Module declaration and top-level constants

`module NAME` (la `regio NAME`) optionally names a file. Legal only as the file's very first
declaration, before any import or statement, at most once. Contextual: `module` is claimed
only in that leading position, so it stays an ordinary identifier everywhere else.

A module-level constant is a top-level `const T X = e` (la `fixum T X = e`): `=`, not `←`, because the
value is compile-time (folded, `comptime`, or an embed) and must fit `T`; `const _ X = e` infers the
type; `@ public` exports it. A top-level `let`/`mut` with `←` stays `SEM062 top_level_binding`,
and a run-time `const T x ← e` (la `fixum T x ← e`) is `PARSE010 runtime_const_retired`. The
retired module-level `static` (la `generis`) declaration is a parse error: `PARSE010 static_decl_retired`
(`keyword` names the spelling used) — write `const` instead; `static` is also retired
as a `class` member (see the field modifiers below). `embed "path"` (la `insere "path"`) is a build-time file embed, read once at
build time, package-relative path, no `..` escape; `string` requires valid UTF-8, `list<b8>` reads raw
bytes. `embed` is contextual: claimed in expression position only when directly followed by a
string literal. It is admitted only as the whole initializer of any `=` constant (module level or
block level) or `class` field default under any modifier; elsewhere `SEM061 insere_outside_constant`.
`comptime { … }` (la `praefixum { … }`) is legal as the direct initializer of every `=` binding; its body
stands alone: no captures of enclosing parameters, locals or `self` (`praefixum_captures_local`), no
type parameters (`praefixum_type_parameter`). There is no `faber const-codemod`: the migration command
is gone, not a thing to run.

Unmarked top-level declarations are module-private by default; `@ public` exports, `@ internal`
is package-internal-only, `@ private` is an explicit private marker (la `publica`, `interna`, `privata`).
A leading-underscore name is a warning (`WARN030`), not an error.

## Canonical Declaration Forms

Variables (`let` is write-once, la `sit`; `mut` is writable, la `varia`):

```fab
main {
    # let: write-once
    # mut: writable
    let string name ← "Mark"
    mut int index ← 0
    # inferred forms write the `_` type slot; `let inferred ← 1` is PARSE010 binding_type_slot_required
    let _ inferred ← 1
    mut _ total ← 0
    let list<string> xs ← empty
    # deferred: assign exactly once before any read
    let int late
    # ref: re-pointable read-only name over a place (la de)
    ref list<string> cur ← xs
}
```

`let _ name` with no initializer is the inferred deferred form (assign it exactly once before reading it); `let name` without a type slot is `PARSE010 binding_type_slot_required`. `let`/`mut T x ↢ future` and
`let x ↢ future` are the await-directed initializers (same as `await_const T x ← future`, la `figendum`; `↢` is not an
expression operator and cannot target an existing place).

Functions:

```fab
fn greet() {
    write "Hello"
}

fn double(int n) → int {
    return n * 2
}
```

Parameter and field markers:

- ownership prefixes `ref` / `mut` / `own` / `copy` (la `de` / `in` / `own` / `copy`) appear in type position (`owned_type ::= ('de' | 'in' | 'own' | 'copy')? base_type` in the EBNF, which uses the Latin spellings), not as parameter prefixes — the parameter prefix slot is retired
- call-site markers repeat the parameter's marker on the argument: a `mut` / `own` / `copy` parameter is called with `bump(mut a)`, `eat(own a)`, `dup(copy a)`; `ref` is optional (`argument ::= template_argument | call_marker? 'sparge'? expression`). It applies to every source-signature function and to a method's arguments (`obj.run(mut src)`, no receiver marker), and function-value calls use the modes in the function type; built-in/intrinsic methods are exempt. `SEM057` `call_marker_missing` / `call_marker_mismatch` / `call_marker_on_read_only`; a temporary to `mut` is `SEM020 mut_argument_not_writable`; `own`/`copy` take a fresh value (`eat(own make())`)
- parameter writes follow the marker, never a binding word (parameters take no `let`/`mut`/`const`): unmarked and `ref` are read-only (`SEM020`), `mut` writes the caller's value, `own` and `copy` are always writable by the callee (method call, field store, reassignment, element store; an `own` tensor may be rebound whole, a `mut` tensor may not). Only a whole local binding may be passed as `own`: field/element, unmarked parameter, module constant, or `for` binder is `SEM057 own_argument_not_local_binding` / `own_argument_unmarked_parameter` / `own_argument_module_constant` / `own_argument_loop_binder` (`copy b.xs` is fine and keeps `b` alive); after `eat(own a)` a later read of `a` is `SEM050 use_after_own` (also `moved_in_loop`, `moved_name_captured_by_closure`; `own`/`copy` belong on the value side of a binding, `let b ← own a`, and a binding-position marker is `PARSE010 binding_marker_moved_to_value`)
- `rest` (la `ceteri`) — rest parameter
- `optional` (la `sponte`) — optional/voluntary declaration slot (post-name)
- `as` (la `ut`) — alias
- `coalesce` (la `vel`) — default expression on a declaration slot
- generic parameters live in `<>` after the name: `fn f<T, size N>(…)`, type parameters first,
  `size` (la `magnitudo`) size parameters after them; there is no `prae typus` parameter form
- explicit generic arguments at a call echo the kind the signature declares: an explicit size
  carries the `size` marker (la `magnitudo`), `mk<int, size 4>(xs)` for `fn mk<T, size N>`,
  `tensor.construct<int, size [4]>(xs)` for a whole shape (`size [2, 3]`, naturals only), and
  `f<size n>(x)` for an in-scope integer. The list binds by marker (a `size` argument binds the
  next `size` parameter, an unmarked one the next type parameter), and the marker is required:
  a bare `mk<int, 4>(xs)` is `SEM057 size_marker_required`, `size` on a callee with no `size`
  parameter is `size_marker_unexpected` (`corpus/generic/call-size-args.fab`,
  `corpus/conversio/lista-tensor-explicit-shape.fab`)

Recoverable errors:

- `→ T` — success return type
- `⇥ E` — alternate error-channel type
- `do { ... } catch err { ... }` — explicit do-block with a local handler

`coalesce` is also local nullable elimination in expression position (`T ∪ none coalesce T → T`). It is not logical `or` (la `aut`).

## Conversion-Directed Assignment (`↤`)

`place ↤ value` evaluates the right side, converts it to the statically known
type of the left place through the existing `↦` route, then assigns. Same
precedence and right-associativity as `←`. The
operator is preserved verbatim through syntax and emission — never rewritten
to `←` or `↦`. A `⊥ default` (see **Format (`¶`) and default (`⊥`)** below) is legal only on
`↤` and attaches to the nearest `↤` in a right-associated chain.

```fab
main {
    mut int count ← 0
    mut int a ← 0
    mut int b ← 0
    # string → int conversion, then store
    count ↤ "42"
    # typed initializer: convert to the written type
    let int typed ↤ "7"
    # ⊥ default stored only on conversion failure (retired: `⇥ 0`)
    count ↤ "x" ⊥ 0
    # right-associated: inner converts first
    a ↤ b ↤ "42"
}
```

Rejected: `mut _ x ↤ …` (no concrete destination type), `let x ↤ …` (`PARSE002 conversio_assign_requires_concrete_type`), and
`await_const`/`await_var` (la `figendum`/`variandum`) with `↤` (awaited bindings keep `←`).

## Numeric Model

Teaching exemplar: `corpus/operatores/numerus-widening.fab`.

- **Exact values, checked stores.** Integer arithmetic computes the exact result; the type of an expression is its range type (`u8 + u8` is `u16`, `u8 - u8` is `i16`; at 64 bits the sign is kept, so `u64 - u64` is `i64`). A value is checked only where it lands in a typed slot (declaration, assignment, field, argument, `return`, element). A trapping slot traps with "N does not fit in `T` (…)"; `wrapping<W>` reduces; `saturating<W>` clamps once at the store. An expression with no slot (`print a + b`) never traps for size. An intermediate of bounded operands outside [−2⁶³, 2⁶⁴ − 1] traps (`2.power(64)` traps, `2.power(63)` is exact); an operand typed `inf` has no such bound (see **The unbounded integer `inf`** below). An implicit trap never enters `⇥`; explicit `↦ T` is the recoverable form.
- **Untyped constants** are exact values that join the other operand's range (`x + 10` is the same operation at every `x`); in an annotated slot a constant must fit at compile time; `_` slots take the range type (or `int` for constants alone).
- **Division.** `/` floors, `%` takes the divisor's sign (`-7 / 2` is `-4`, `-7 % 2` is `1`); `÷` is true division (`f32` for an 8/16-bit widest operand, else `f64`; `7 ÷ 2` is `3.5`).
- **Families.** Implicit integer widening is lossless only, and `u64` has no implicit widening: a typed `u64` with a typed signed operand is a compile error (fix with `↦`); constants are exempt. Crossing integer / decimal (`d64`) / float needs `↦`; comparisons across families are exact and need none (`i64 ≺ f64` is legal).
- **Bits.** On numbers shifts are unmasked (`x ⇐ n` is `x·2ⁿ`); `¬x` is `-x - 1`; for fixed-width complement or hashing use `wrapping<W>`. The raw-bits markers `b8 b16 b32 b64` are not numbers: each operation acts at the type's own width (`0xF0 ⇐ 4` on a `b8` is `0x00`, `¬0xF0` is `0x0f`, a shift by N or more gives zeros, a negative count is an error); a bN has equality, unsigned-pattern ordering and `∧ ∨ ⊻ ¬ ⇐ ⇒` only, with explicit `↦` to numbers.
- **Float to integer.** A float or `d64` source needs a rounding choice: `x ↦ int via Floor|Ceil|HalfEven|Trunc`; the bare `x ↦ int` is `SEM016 float_to_integer_requires_rounding`, and `⊥ default` / `⇥` follow the `via` as for any failable conversion (`corpus/conversio/float-to-int-via.fab`). The glyph `⌊x⌉` (nearest) is the short form of `via HalfEven`.
- **Local constant.** `const T x = e` is a compile-time local (evaluated by the compiler, must fit `T`); a runtime store is `let T x ← e`, and `const T x ← e` is `PARSE010 runtime_const_retired`.
- **AIR lane.** In an `@ radix lane "air"` function every integer store and operation must be proven to fit by the range rule; otherwise it is a compile error (declare a wider slot or use `↦ T ⊥ default`).
- **The unbounded integer `inf`** (teaching exemplar `corpus/operatores/inf.fab`, with `inf-conversio.fab`, `inf-json.fab`). The bare marker `inf` is an integer with no bound — opt in by declaring a slot `inf` or converting an operand `↦ inf`; nothing becomes `inf` by default, and a lone literal of any length defaults to `int` (a literal past `u64` needs an `inf` slot or operand, else `SEM010 numerus_literal_out_of_range`). `inf`, `trapping<inf>`, `wrapping<inf>`, `saturating<inf>` (la `exactus`/`modulus`/`saturatus`) all name the same type (the wrapped `int<inf>` is retired, `PARSE040`); the marker is ASCII in every locale, with no prefixed sugar (`linf` is an identifier). `inf` is the implicit lossless target of every integer width including `u64` (`let inf x ← u` needs no `↦`) and widens into nothing; `u64 - u64` is still `i64` — opt in with `(a ↦ inf) - b`. On `inf`: `+ - * ⇐ power` never trap, `/` `%` floor, `∧ ∨ ⊻ ¬` use infinite two's complement, a zero divisor or negative shift count/exponent still traps, comparisons against every integer width, `d64` and float are exact, and an `inf` is a valid `map` key. Narrowing back is a checked conversion: `x ↦ u64 ⊥ 9` (a value that does not fit takes the default; without `⊥`/handler it is `failable_conversio_requires_handler`), `↦ wrapping<W>` reduces and `↦ saturating<W>` clamps (never fail); `↦ float` rounds and a magnitude past the float range is `inf`/`-inf`; a float or `d64` → `inf` needs an explicit rounding hint (`via Floor`, `Ceil`, `HalfEven` or `Trunc`) and a NaN or infinite float fails; text parses any number of digits (`"ff" ↦ inf via Hex`); `inf ↦ list<b8> via Be`/`Le` is minimal two's complement. A store from `inf` into a bounded slot traps (``300 does not fit in `u8` ...``). `inf` is host-only: `SEM008 tensor_element_unbounded` (tensor/vector/matrix/sparsa element), `SEM008 nucleum_host_type` (kernel), `SEM059 air_unbounded_integer` (AIR lane). `∞` is the float infinity, never an `inf` value, and a float ∞ prints as `inf`. Targets: the runner, Rust, TypeScript, Go, Python and sexp carry it; Swift, Haskell, LLVM and Wasm fail closed by name (`inf_target_unsupported`, `llvm_target_inf_unsupported`, `mir_wasm_unsupported`) — check `faber targets` for per-target gaps.
- **`≈` on integers.** Between integers of any width including `inf`, `≈` is one exact rule, `10⁹·|a − b| ≤ max(|a|, |b|)`, not a float compare (`corpus/operatores/approx-integers.fab`).

## Bit Reinterpretation and Float Field Reads

- Bit patterns go through the raw-bits markers `b8 b16 b32 b64` (bits are not numbers): `x ↦ b32` reads the pattern of an `f32` (`↦ b64` for `f64`), `u32 ↦ b32 ↦ f32` and `u16 ↦ b16 ↦ f16` reinterpret a pattern as a float; reinterpretation, never value conversion. `via Bits` is retired (`SEM016 conversio_via_bits_retired`, names the `bN` route).
- **The `via` clause.** A convert hint follows the target: `x ↦ u16 via Le`, `"ff" ↦ i32 via Hex`, `65 ↦ char via Code`; register targets take it too (`v ↦ list<b8, size 16> via Le`). The text/byte hint set is `Hex`, `Bin`, `Oct`, `Be`, `Le`, `Code`, and a float or `d64` to integer takes the rounding hints `Floor`, `Ceil`, `HalfEven`, `Trunc`; it binds tighter than `⊥` and applies per hop (`list<b8>[0‥2] ↦ u16 via Le ↦ b16 ↦ f16 ↦ f32`). On bits lists a `via Le|Be` clause is written exactly when the bit width changes (`list<b8> ↦ list<u16> via Le`; `list<b8> ↦ list<u8>` takes none). `via` is a contextual keyword (an ordinary identifier elsewhere). The retired spellings — a hint as a type argument (`int<u32, Hex>`) or a register tail (`list<b8, size 16><Le>`) — are parse errors (`PARSE030 conversio_hint_type_argument` / `conversio_hint_tail_argument`). `@ kernel lane "x"` is removed (`nucleum_lane_removed`).
- **Bulk bytes ↔ tensor.** `list<b8> ↦ tensor<E, shape>` reinterprets a whole byte buffer in row-major order, and the inverse `tensor ↦ list<b8>` is shipped and infallible. For elements wider than one byte write `via Le` or `via Be` (no hint, or `Bits`/`Hex`, is `SEM016 list_b8_to_tensor_unspecified`); one-byte `u8`/`i8` elements take none. `E` is one of `u8 i8 i16 u16 i32 u32 i64 u64 f32 f64` (fixed-layout record elements also have supported forms; another element type is `list_b8_to_tensor_element_unsupported`). Shapes may carry supported run-time extents, e.g. `tensor<f32, [2, size n]>`. A pure bit copy (NaN payloads, `-0.0`, subnormals survive); the byte count must equal `product(shape)·w`: a statically known mismatch is `list_b8_to_tensor_length_mismatch`, a dynamic one is a recoverable error (`do { } catch`/`⊥`/`⇥`; a bare `↦` outside `main` needs a handler). Example: `let tf32[2, 2] m ← raw ↦ tf32[2, 2] via Le`. Decode a whole buffer at once this way, never element by element; `list<f32> via Le` is still `SEM016 unsupported_conversio`. Diagnostic names are `list_b8_to_tensor_*` and `tensor_to_list_b8_*` (the old byte-type names have no aliases); check target support before choosing an emitter.
- finite/NaN detection reads the bit fields through `x ↦ b32 ↦ u32` (exponent all-ones, fraction nonzero separates NaN from ±Inf) — never `x ≢ x`, which is lane-dependent (the MIR runner's float `≢` is NaN-total). Exemplar: `corpus/scalar/finite-nan.fab`.

## Copy-Into (`⇇`)

`target ⇇ value` copies every value of the tensor `value` INTO the existing
storage of `target`: the storage identity is preserved, so every alias of the
target sees the new values. `←` only ever rebinds a name; `⇇` is the one way to
write into a tensor that already exists. It is a statement (result `void`),
single only (`transfer ::= ternary ('⇇' ternary)?`), and binds above assignment
and below ternary, so `output ⇇ (q · kt) ⊙ s` copies the whole product.

```fab
fn fill(mut tf32[2, 2] target, tf32[2, 2] source) → void {
    target ⇇ source.transpose()
}

main {
    let list<f32> flat_source ← [1.0, 2.0, 3.0, 4.0]
    let list<f32> flat_target ← [10.0, 20.0, 30.0, 40.0]
    let tf32[2, 2] source ← flat_source ↦ tf32[2, 2]
    mut tf32[2, 2] target ← flat_target ↦ tf32[2, 2]
    # `←` makes an alias of the same storage
    let tf32[2, 2] alias ← target

    fill(mut target, source)
    # writes in place; `alias` sees it
    target ⇇ source + source
    print alias.flatten()
}
```

The copy-into rules, each a named diagnostic:

- The target and value have the same element type and shape (`copy_into_type_mismatch`;
  a static shape mismatch is `incompatible_tensor_index`; runtime-extent shapes
  give a recoverable runtime error value, never a kernel trap).
- The target is writable tensor storage: a `mut` parameter, a `mut` local, a field of a writable
  root; never a `let` or `const` (`copy_into_target_immutable`), a view
  (`copy_into_target_view`, future question), or a scalar/list/matrix/vector
  (`copy_into_target_not_tensor`; a value type is written by `←`).
- The value may be a view but must not alias the target
  (`copy_into_alias`, `←` aliases count).
- There is no value use (`copy_into_value_used`).
- There is no chain (`a ⇇ b ⇇ c` is `PARSE030
  copy_into_chain`).
- A whole-tensor `←` into a host `mut` tensor parameter is an ordinary rebind (`corpus/transfer/copy-into-rebind-host.fab`). Inside a kernel a `mut` tensor parameter is device storage and cannot be rebound whole (`SEM057 mut_tensor_param_rebind`, help names `⇇`); use `⇇` to write its existing storage (`corpus/transfer/copy-into-rebind-reject.fab`).
- The old callable-sink form `sink ⇇ payload` (a callable on the left) is retired:
  `SEM012 transfer_sink_retired`, scheduled for removal in a future major release.
  Output is `print` (la `nota`), not `⇇`; the `output-transfer-codemod` command is
  gone.

Execution paths: the MIR runner, Rust (`Tensor::write_values`), and Metal/WGSL kernels
(the value is computed straight into the output buffer) carry it; TS, Go,
Swift, LLVM, Wasm and sexp fail closed by name. Teaching exemplum:
`corpus/transfer/transfer.fab` (negatives `transfer/copy-into-*-reject.fab`).

## Classes, Interfaces, Unions

```fab
class Person {
    let string name
    mut int age = 0
    const string ORIGIN = "Rome"
}

interface Logger {
    fn log(string message) → void
}

union Event {
    Click { int x int y },
    Quit
}
```

Class and variant construction uses `Type { field = expr }`. Bare `{ field = value }` takes an expected class or record type (without one it defaults to `record<*>`); the quoted-key `{ "key": constant }` is the JSON-literal form.

**Copy with changes:** `Class { field = value, … } from source` (la `ex`) builds a new value: the braced fields
override, every other field comes from `source` by value (`from` takes values: an omitted list or object field is
duplicated, so the new value shares nothing with `source`; private fields copy across too). `from` must open on the closing `}`'s line — a line-leading `from` is
instead the extraction statement (`from p let x, y`). Exactly one source per literal. `spread` (la `sparge`) was
removed from construction literals (it stays for lists and calls); `nexum` was removed outright.

**Field modifiers and privacy:** every field declares exactly one of `let`
(la `sit`: per instance, set once in the construction literal), `mut` (la `varia`: per instance, reassignable), or `const T X = e` (la `fixum`: the type-level compile-time constant, one per type, read through the type name `Person.ORIGIN`; a store is `SEM020 assignment_to_const_member`, a missing `=` is `PARSE010 const_member_requires_initializer`); an unmarked field is `PARSE010 field_modifier_missing`, `static` (la `generis`) is `PARSE010 static_member_retired`, and `ref` (la `de`) on a field is `ref_field_refused`. Members are public
by default; `@ private` restricts a member to the type's own method bodies, `@ internal` to the
declaring package. A construction literal, and `Class { … } from p`, may still set/copy a private field
from any file. Reading/writing/calling an inaccessible member from outside its scope is `SEM063`.
`@ public` on a member is a redundant-annotation warning (`WARN028`). `field-modifier-codemod` (see
`faber field-modifier-codemod --help`) inserts `mut` (mutable) before every unmarked field, and
nothing else. There is no more
`sub`/`abstractus` inheritance surface — `class` types compose through `implements` contracts only.

Imports:

```fab
import from "norma:console" const print

main {
    print("hello")
    print "keyword form"
}
```

Other import shapes (they need their packages, or a sibling module, to exist):

```text
import from "norma:console" console
import from "norma:text"                       # binds `text`; no marker means no re-export
import { from = "norma:json/solve", as = solve_mod }
import from "./types" public User              # `public` re-exports a sibling module
import from "norma:console" const write_partial as output
import from "norma:fs" const write_file as out_file, write_bytes as out_bytes
```

External packages and sibling modules must resolve. `import from "faber:*"` is script-only and not a package import, and the kernel namespace supplies no host-library modules: use the matching `norma:*` library.

The record form's five field names are reader-pack words like every other keyword: en `from`, `visibility`
(value `public`), `name`, `as`, `all` (la `ex`, `visibilitas`, `nomen`, `ut`, `omnia`; the other packs follow their
own keyword rows). A field word from another locale is `PARSE050 wrong_locale_import_field`, so the Latin `ex = …`
in an en file fails and names `from`; an English word in a la record is `unknown_import_field`.

The `private` import marker (la `privata`) was removed (`PARSE050 import_privata_removed`): an import without a marker
does not re-export and `public` is the only marker. Norma module names are the English source names
(`norma:console`, `norma:text`, `norma:fs`, `norma:json`, `norma:process`, `norma:time`, …; the module list
is `lib/norma/src/*.fab`); a Latin name such as `norma:consolum` or `norma:chorda` is `PKG001
unknown_library_module`. The `la` pack only projects member names (for example `dic`, `scribe`, `funde` for the en
`write_partial`, `write_file`, `write_bytes`);
`lib/norma/locale/la/pack.toml` lists them.

Selective imports (`const` member list after the import path) bind exported
members as ordinary immutable locals: the imported file interface supplies the
complete type, and the bindings are never re-exports.

## Types

Primitives:

- `string`, `ascii`, `int`, `float`, `bool`
- `trapping<W>` (la `exactus<W>`) — the trapping policy spelled out: the same type as the bare marker `W`, and a store traps when the value does not fit (integer widths and `d64`; float widths too: a trapping float cell rejects non-finite values at stores and conversions)
- `wrapping<W>` (la `modulus<W>`) — modular word, signed or unsigned; the value reduces mod 2^W at the store, and `↦` into it reduces and cannot fail
- `saturating<W>` (la `saturatus<W>`) — saturating integer, signed or unsigned; arithmetic is exact and the value clamps to `W` **once, at the store** (never per operation); `↦` into it clamps and cannot fail
- **Ranged store policy** — `saturating|trapping|wrapping<W, lo‥hi>` (la `saturatus|exactus|modulus<W, lo‥hi>`), integer widths only (teaching exempla `corpus/operatores/ranged-saturating.fab`, `ranged-trapping.fab`, `ranged-wrapping.fab`, `ranged-sugar.fab`). A range is the second argument of the policy word; the width stays explicit and the range refines it. `‥` is half open (`0‥8` holds 0 to 7) and `…` is inclusive (`1…5` holds 1 to 5); both ends are always written, and `∞` / `-∞` write "no bound on this side" (`-∞‥8`, `4‥∞`). The store applies the range once and arithmetic stays exact: saturating clamps into it, trapping traps (message names ``trapping<i32, -2‥3>``), wrapping reduces modulo `hi - lo` offset by `lo` (12 in `wrapping<u8, 3‥9>` is 6). The whole-width range is the existing spelling (`saturating<u8, 0…255>` is `saturating<u8>`, `trapping<u8, 0…255>` is `u8`, `trapping<inf, -∞…∞>` is `inf`: one type). Narrowing a range of the same policy is a store; another policy family still needs `↦` (superseded by ruling 51, operator 2026-10-27, ruled and not yet built: a value of any policy stores into a cell and the cell's policy applies to the exact value; until it is built the compiler still refuses the store, so keep writing `↦`; see `docs/design/numeric-model.md` §8). Refused by name: `u8<0‥8>` (`ranged_policy_word_required`), `0…` (`PARSE040 range_bound_required`), `5‥5` (`ranged_policy_range_empty`), `0‥300` on `u8` (`ranged_policy_range_outside_width`), `-1‥5` on `u8` (`ranged_policy_unsigned_negative_bound`), a float, decimal or `inf` width with a finite range (`ranged_policy_width_unsupported`), and a name or `size` extent as an end (`ranged_policy_bound_not_constant`, not built). Built on the runner and Rust only; other targets decline (`ranged_policy_target_unsupported`) and kernels refuse (`nucleum_ranged_policy_not_built`)
- **Float policy cells** are distinct types: `saturating<f32>` keeps IEEE results (infinity, NaN) and does not clamp to the largest finite float; `trapping<f32>` checks for non-finite values when stored or converted into the cell, not after each operation. Both are host-only (`corpus/operatores/policy-float.fab`).
- **List length range** — `list<T, size lo‥hi>` / `list<T, size lo…hi>` (la `lista<T, magnitudo lo‥hi>`; teaching exempla `corpus/lista/ranged-length.fab`, `ranged-length-conversio.fab`, `ranged-length-sink-trap.fab`). The range follows the `size` marker (a bare range bounds a value, as in `trapping<u8, 0…255>`; a bare `list<T, 4>` or `list<T, 1…8>` is retired, `SEM008 list_count_needs_size`; a kernel or AIR lane needs a finite upper size, `size …∞` is `nucleum_unbounded_list_size` / `air_unbounded_list_size`). The range is the second argument of `list`, with the same glyphs as integer ranges (`‥` half open, `…` inclusive, `∞` no upper end, both ends written); it is trapping only, and `list<T>` is `0…∞` while `list<T, size N>` is `0…N` (one type). `append` past the maximum and `remove_first` / `remove_last` below the minimum report on `⇥` (`⇜` past the maximum dies); `list ↦ list<T, size lo‥hi>` checks the length and takes `⊥`; a range with a minimum of one or more is never empty, so `first()` / `last()` return `T`, not `T ∪ none`. Refused by name: a literal or `empty` outside the range (`SEM011 ranged_list_length_out_of_range`), `4‥4` (`ranged_list_range_empty`), a negative end (`ranged_list_bound_negative`), a name as an end (`ranged_list_bound_not_constant`), and a policy word around a list (`ranged_list_policy_word`). Built on the runner and Rust only; other targets decline (`ranged_list_target_unsupported`)
- `none`, `void`, `never`, `unknown`, `instant`
- `char` (la `littera`) — one Unicode scalar value (4 bytes, never allocates); element of `string`/`ascii` iteration and `string[i]`/`ascii[i]` indexing
- There is no `bytes` or `byte` type. A byte string is `list<b8>` (la `lista<b8>`, bounded `list<b8, size N>`); a raw byte is `b8`; a numeric byte is `u8`. `list<b8>` is not a list of numbers: it converts to `list<u8>` only with an explicit `↦`

Sized numerics are **bare markers**: `i8`…`i64`, `u8`…`u64`, `f16`, `bf16`, `f32`, `f64`, `d64` (the decimal), `inf` (unbounded integer). Bare `int` / `float` are `i64` / `f64`. The wrapped forms `int<W>` / `float<W>` (each locale's own word for int and float; la `numerus<W>` / `fractus<W>`), the holes `int<_>` / `float<_>` and wrong-family forms (`int<f32>`) are `PARSE040 numeric_wrapper_retired`. `d32` is removed (`SEM008 removed_decimal_width_d32`). The policy words keep their argument: `trapping<W>` / `wrapping<W>` / `saturating<W>`, e.g. `wrapping<u32>`, `saturating<i16>`. Check EBNF for the live marker sets.

Collections and buffers:

- `list<T>`, `map<K,V>`, `set<T>`, `queue<T>`, `stack<T>` (la `lista`, `tabula`, `copia`)
- bounded capacity forms take a `NATURAL` (or `_` to infer, which stays bare) written after `size` (la `magnitudo`): `list<T, size N>`, `queue<T, size N>`, `stack<T, size N>`,
  `string<size N>`, `ascii<size N>`; the bare `list<T, N>` is `SEM008 list_count_needs_size` and the bare `queue<T, N>`, `stack<T, N>`, `string<N>`, `ascii<N>` are `SEM008 capacity_needs_size`; these container capacities are compile-time values today (supported run-time tensor extents are a separate case)
- **Short literals:** a fresh list literal takes the destination's capacity or length range, so `let list<int, size 0…3> pair ← [1, 2]` is legal and prints `[1, 2]`; an overlong literal is still refused, and the rule covers list-valued fields. Sized and length-ranged list types keep their constraints across export and import (`corpus/lista/ranged-short-literal.fab`, `corpus/genus/sized-list-field-literal.fab`).
- **List search and join:** `xs.find(pred)` walks left to right, stops at the first match and returns the element or `none` (one optional, never nested; tensor `reduce via first` instead evaluates every predicate). `xs.extend(ys)` appends all of `ys` in order, in place; the receiver must be an unbounded `mut list<T>` (a bounded or length-ranged receiver is `SEM011 extend_requires_plain_list`), the source may be bounded, and `xs.extend(xs)` appends the original elements once (`corpus/lista/find-optional-element.fab`, `methodi-extende.fab`).
- **Invariant type arguments:** a named `list<u8>` does not become `list<int>`; container, tuple, reference and function type arguments are invariant. Write `↦ list<int>` or build a fresh destination (fresh literals, spreads and lifted results are built by element stores). A generic class method keeps its type parameters rigid, so `T ∪ none` cannot be stored into `T` unhandled. A function or module value may share a pack type word such as `list` or `tensor`; type lookup still finds the builtin.
- `promise<T>`, `iterator<T>` (la `promissum`, `cursor`)
- `tensor<T, Figura>`, `sparsa<T, Figura>` — shaped buffers
- `vector<T, size N>`, `matrix<T, [R, C]>` — register-class, not buffer aliases. The vector long form writes its lane count after `size` (la `magnitudo`), as a list does; a bare `vector<T, 4>` or `vector<T, N>` is `SEM008 vector_width_needs_size` (clean break, no alias). The `vf32[4]` shorthand, `vector<T, [4]>` and the `_` hole are unchanged: the brackets already mark an extent
- `atomic<T>` — storage-sensitive atomic cell; `T` is `i32` or `u32` only (`SEM008 atomic_element`), access through `load`/`store`/`exchange`/`compare_exchange`
- `record<Label: T, …>` (la `ratio`) — labeled record type; every element needs a unique label, fields are accessed by label only (no positional or bracket access). Record identity ignores field order and compares field types exactly (no width subtyping); a record is not a class. `record<*>` (la `ratio<*>`) is the whole-field-list hole, legal only as the sole `record` argument: `record<g: f32, *>` is `PARSE001 ratio_field_list_hole_not_sole` and `record<_>` is `ratio_type_arg_label_required`. A bare `{ field = value }` with no typed cell is `record<*>`, each field taking its default type, while `_ { … }` still needs a cell. A record is immutable (no field assignment), prints like a genus value without the name (`{ g = 1.0, w = 2 }`, written order), and `↦` converts a record to and from a genus with the same fields. `faber explain record` for construction forms; see `corpus/ratio/ratio.fab`.

`at [i, j]` coordinate binding, the sort expression `sort via asc|desc from xs` (la `ordinata via ascendens|descendens ex xs`; grammar `sort_expr` [234]; a body `let r { return r.k }` (la `sit r`) gives the key, ties keep source order; a map sorts values with `from`, keys with `ref`; no body sorts by natural order via `Orderable`; a bare sort or reduce as a `for` source is `PARSE030 itera_source_sort_reduce_unbound`, bind it first), the in-place method `xs.sort(key)` (la `ordina`), and hashable keys (tuples,
`class`/`union` values when every part is hashable) are live on `list`/`map`/`set`; a
non-hashable map key or set element is a named diagnostic (`tabula_key_not_hashable`,
`copia_element_not_hashable`).

Type forms:

- nullable values: `T ∪ none` (la `T ∪ nihil`; not `T?`). The null *type* and null *value* are different words: type `none` (la `nihil`), value `null` (la `nulla`); `T ∪ null` is `PARSE040 null_value_in_type_position`, and `is`/`not is` (la `est`/`non est`) is a type test only (`x is none`; `x is true` is `SEM011 est_value_rhs`, use `≡`)
- inline unions: `T ∪ U`
- tagged unions: `union`
- function types: `(A) → B`, optionally `(A) → B ⇥ E`
- ownership prefixes on types: `ref T`, `mut T`
- inferred binding type: `let _ name ← value`, `mut _ name ← value`, `ref _ name ← place`, `const _ NAME = value`; there is no type-omitted declaration (`let name ← value`, `mut name`, `ref name`, `const name` are `PARSE010 binding_type_slot_required`); binders (`for … let x`) take no type slot
- type holes: `_` infers one inhabitant; standalone `∪` infers a finite union
- inferred constructor: `_ { x = 1 }` builds the cell's class, `_ [1, "a"]` its tuple or list; a bare `{ x = 1 }` or `[1, "a"]` takes the cell's type (sugar for `_ shape`); with no typed cell a bare `{ x = 1 }` defaults to `record<*>`, while `_ { … }` is `SEM016 inferred_constructor_no_expected_type` (`corpus/genus/inferred-constructor*.fab`, `inferred-literals.fab`)
- labeled tuple forms: `tuple` (la `iuncta`) type arguments, construction, and member access
- width / shape sugar (type position only, identical to the long form): `lu32` = `list<u32>`, `tf32[2, 3]` =
  `tensor<f32, [2, 3]>`, `sf32` = `sparsa<f32, _>`, `vf32[4]` = `vector<f32, size 4>`, `mf32[4, 4]` =
  `matrix<f32, [4, 4]>` (matrix needs both extents); `inf` takes no prefix (`linf` is an identifier)
- `list<∪>` / `map<K, ∪>` infer a heterogeneous union element; `list<_>` the single inhabitant
- inline intersection `A ∩ B` (nominal; binds tighter than `∪`); `class G implements A ∩ B` is the same as `implements A, B`
- contract bounds: `fn larger<T implements C<T>>(T a, T b) → T` — one keyword, `implements` (la `implet`), on a `class`
  declaration and on a type parameter; bounds are written in full (`T implements Orderable<T>`, no bare-name
  shorthand). `Orderable<T>` (`norma:order`) is the one ordering contract — its `compare` drives every
  comparison glyph so they cannot disagree; built-ins conform automatically, user types opt in.

Banned:

- `cum` keyword
- postfix `T[]` array syntax
- `Type(...)` construction for tensors, vectors, matrices, or scalars — use `↦`, named constructors, or `Class { field = value }`

`empty` (la `vacua`) needs an explicit collection type. `empty` is the DEFAULT EMPTY INSTANCE of the declared type on the left: an empty collection for lists, maps and sets, one default slot for a rank-0 tensor, an all-zero sparse tensor, and for a tensor, vector or matrix the ZERO, so `mut tf32[2, 3] blank ← empty` means exactly `mut tf32[2, 3] blank ← tf32[2, 3] {}`. A tensor with an unresolved `_` extent is refused by name (`SEM011 vacua_requires_resolved_extents`). `faber format` keeps writing `empty` as authored. The typed zero `Type {}` stays the explicit spelling; the bare type as a value (`← tf32[2, 3]`) stays retired.

## Control Flow

- `if` / `elif` / `else` (la `si` / `sin` / `secus`) — conditionals; `elif` takes its condition directly (`elif if b` and `else if b` are parse errors)
- `then` (la `ergo`) — one-statement bodies of `if`/`while`/`for`/`case`/`default` arms, including `then return`; `∴` is **not** accepted there (`PARSE026`): `∴` is the compact-closure joint only (`(int a) ∴ a * 2`)
- `while` — loops
- `for from` / `for ref` / `for range` — iteration (values / keys / range, `range 0‥10 step 2` or `0 until 10`); the binder is `let`/`mut` (`ref` is refused, `PARSE010 ref_binder_refused`; the old `const` binder is `binder_fixum_retired`) plus a name or an array/object pattern, and an optional `at [i, j]` clause (only on `for from`, over an indexed iterable) binds coordinates
- `switch` / `case` / `default` — switch-like dispatch
- `match` / `case` — pattern-match dispatch, a statement not an expression; `match all` (la `discerne omnia`) is the exhaustive form and forbids `default` (`SEM044 catchall_arm`, `SEM040 missing_variants`); patterns are flat (no nesting, no guards, no range patterns)
  - closed set only: a union's members or an enum/`union`'s variants. A type pattern outside the scrutinee's closed set is `SEM010 discerne_pattern_not_in_closed_set`; width (`case u32`) and length-shape (`case list<int, size 4>`) patterns are retired — ask width with `is`, length with `if x.length() …`; open scrutinees use `switch`
- concrete patterns bind in `match`/`case` clauses, `for` binders, and `←` destructuring: `_`, literals, `ID as alias`, multi-bind `(let|mut) a as b, c as d`, arrays `[a, _, rest tail]`, objects `{ f, f as alias, rest … }` (`pattern`, `array_pattern`, `object_pattern`)
- `reduce via op from expr at? per? (thread? (let | mut) binder { … } | coalesce identity)?` (la `reducta via op ex …`) — shaped-source reduction expression (`reducta_expr`; `via` selects one of ten locale-pack operator words; `first` (la `prima`) is the first-match selection: its body is a boolean predicate written with `return`, the result is always `T ∪ none` (none for no match or an empty source), the predicate runs for every element, and it takes no `per` group and no `coalesce` identity; the source is a tensor or matrix)
- `guard { if cond … }` — guard
- `c ✓ a ✗ b` — the one value conditional ("`a` when `c` holds, else `b`"); replaces the retired
  `?:`/`sic … secus` ternary, which now reports a migration diagnostic
- `comptime { ... }` (la `praefixum`) — build-time-evaluated block; legal only as the direct initializer of an `=`
  binding (module/block constant, `class` field default), else `SEM064 praefixum_outside_constant`. Body stands
  alone: no enclosing params/locals/`self`, no type parameters (`praefixum_captures_local`,
  `praefixum_type_parameter`); no `⇥` channel inside — failable calls must be handled within the block
- `do { ... }` — explicit do block; `do { ... } while cond` is the post-test loop
- `catch` — structured local handler on `do` and other named forms, not arbitrary bare blocks
- `pass` (la `tacet`) — no-op
- `main { ... }` (la `incipit`) — entry block
- `main args args { ... }` — CLI entry with bound args; `async_main` (la `incipiet`) is the async entry; `exit expr` (la `exitus`) supplies the exit code

Membership is the glyph pair `∈` (member) / `∉` (not member), in the comparison tier, with no ASCII spelling. One operator covers
collections (`x ∈ list`, `x ∈ set`, map keys) and ranges (`x ∈ 0…10`, `x ∈ a‥b`); `∉` is a first-class negation. There is no
membership over text (`SEM010 membership_over_text`): use `haystack.contains(needle)` (la `continet`). The old words `within` /
`between` (la `intra` / `inter`) are retired as operators (`PARSE001 membership_keyword_retired`, which names `∈`) but remain
ordinary identifiers.
`break`/`continue` take no label. Functions and type declarations are top-level only
(`PARSE010 declaration_not_top_level` inside a block).

## Error Handling

- `require` (la `requirit`) — recoverable condition plus typed thrown value; it requires a `⇥ E` channel and is distinct from fatal `assert` (la `adfirma`)
- `reject` (la `reice`) — the boolean opposite of `require`: it throws the typed value when the condition holds (`reject cond throw err` lowers to `if (cond) { throw err }`); same `⇥ E` requirement
- `throw` (la `iace`) — throw (recoverable)
- `panic` (la `mori`) — panic (fatal)
- `assert` — runtime invariant; failure is fatal and uncatchable by `catch`
- `catch name { ... }` — local handler on the structured forms that name `catchClause`
- same-line `throw val if cond` / `panic val if cond` is line-sensitive sugar for an `if` block

Current error handling is `do { } catch err { }`. The old `tempta` / `demum` keyword pair is retracted. `tempta` may still appear as an ordinary Norma function name.

### Declared error-conversion functions

The live EBNF keeps unions pure (fields plus variants) and declares each
error conversion as a named standalone annotated function:

```ebnf
union_member ::= annotation* field_decl
```

```fab
union WireError {
    @ shared
    string message

    Invalid
}

union BoundaryError {
    @ shared
    string message

    WireFailure {
        WireError inner
    }
}

@ conversion
fn from_wire_error(WireError err) → BoundaryError {
    return variant WireFailure { message = err.message, inner = err }
}
```

The en surface is `@ conversion fn …` (la `@ conversio functio …`); the annotation identity remains
`conversio` and the active locale pack supplies the spelling. Annotation
stacking is legal. The historical in-union
arm form (`union X { @ conversion Y err { … } }`) is retracted under schema 7
and no longer parses.

The function's concrete source parameter and destination return type form one
ordered pair, enrolled exactly once in the shared conversion table alongside
the compiler-shipped builtin rows. The function stays callable by its declared
name.

A failable call may propagate through that pair, but a missing pair is a named
fail-closed `failable_call_conversion_absent` diagnostic; the compiler does not
compose A→B and B→C rows. The declaring module must own, define, or re-export
the source or destination type, and the body is a pure re-presentation. A local
handler sees the original source error, and non-call sinks such as `↤`, `throw`,
and `require` retain their ordinary type checks.

## Capture-free closures (retired)

The closure-signature modifier (`closure_modifier ::= 'libera' | 'nucleum'` in the EBNF, which uses the Latin spellings)
was removed outright (the
grammar rows left the EBNF). Closures carry no
modifier slot. Closures inside kernels are legal with no modifier;
kernel-ness is owned by the `@ kernel` (la `@ nucleum`) declaration annotations
plus the check-time kernel legality pass. Kernel examples live in the kernel
sections of `references/idioms.md`.

## Async and Streams

Prefer posture keywords over the old annotations:

- `async` (la `fiet`) — async finite
- `generator` (la `fiunt`) — sync stream; a call returns `iterator<T>` (collect with `gen() ↦ list<T>`; `let list<T> xs ← gen()` is a type error)
- `async_generator` (la `fient`) — async stream
- `await_const` / `await_var` (la `figendum` / `variandum`) — await-bind
- `return_await` / `await` (la `reddet` / `tacebit`) — await-return / await-discard
- `yield` (la `cede`) — yield (`generator` / `async_generator` only)

`@ future` (la `@ futura`) and `@ cursor` still parse as legacy annotations. Do not emit them in new code.

## Literals and Construction

- `"..."` / `«...»` — `string`
- `'...'` — `ascii`
- `|de ad be ef|` — `list<b8>` (compile-time hex bytes)
- `` `...` `` — backtick string token: `capture-pending` in the grammar (reserved; it does not parse as an expression today)
- `[ ... ]` — list
- `{ "key": constant }` — JSON document (`json`); values are JSON constants only
- `Type { field = expr }` — class / variant construction
- `∞` — positive infinity; the named NaN literal is `nan` in every shipped pack except the Latin (`la`) pack, where it is `nonnumerus`. Both are contextual floating-point literals: a surrounding `f32` or `f64` supplies the width, bare `float` remains unsized, and neither form has a width suffix. `-∞` is unary negation of `∞`, not a separate literal. An `int` context rejects either form (fail-closed); neither maps to an integer.
- `"pattern" ↦ regex` — compiled regex from dynamic text, failable at run time; `regex /pattern/` and `_ /pattern/` — inline regex literal (the locale regex type word or `_`, then a raw slash-delimited body; `\/` is the only escape and a slash inside a character class needs it too, so write `[\/]`; no suffix flags, use the inline `(?i)` form)

Bare `{ key = expr }` field literals are legal: they use the expected type or default to an inferred record. They are not the colon-keyed JSON-literal form.

**Format (`¶`) and default (`⊥`):** `value ¶ "spec"` renders a value to `string` — the value is the
value, `¶` says how it is shown (`"§"(price ¶ ".2")`). Same glyph in every locale (no word twin);
binds looser than arithmetic, tighter than comparison; does not chain. `⊥` supplies a value when a
conversion or a failable call fails: `let int n ← "abc" ↦ int ⊥ 0`, or `let int n ←
read_count() ⊥ 0` when `read_count` is failable. On a conversion it follows the target immediately (`↦ T ⊥
expr`); at a call site it follows the call. `⊥` replaced the retired `⇥ value` inline-recovery form —
`⇥ E` now only ever names an error **type**, never supplies a fallback value.

## Operators and expression notes

Grounded in `EBNF.md` and probes against the built binary.

- Ranges: `a‥b` (exclusive), `a…b` and `a until b` (inclusive), `a before b` (exclusive), optional `step k` (la `usque`, `ante`, `per`).
  `x ∈ list` is collection membership and `x ∈ lo‥hi` is range containment; `x ∉ …` negates either. Membership over a
  range checks and emits on Rust, Go, Swift, and TS, but `faber run` fails with `unsupported MIR lowering: binary operator
  without a MIR primitive` (no MIR lowering for range containment yet) — do not rely on running it.
  A char-range form such as `x ∈ '0'…'9'` is not supported (a known gap).
- `a ⤒ b` / `a ⤓ b` — max / min (additive tier, left-associative). `↑` / `↓` — postfix increment / decrement
  statements (checked store of the slot's policy).
- `x ∷ T` — static ascription (compile-time fact; a numeric narrowing through `∷` is `SEM016
  numeric_ascription_requires_conversion`); `x ↦ T` is the checked runtime conversion. `x ↦ lo‥hi` /
  `x ↦ lo…hi` clamps a number into an interval (`15 ↦ 0‥10` is 9, `15 ↦ 0…10` is 10) and takes no `via`/`⊥`.
- `?.` `?[` `?(` — optional chaining; `!.` `![` `!(` — non-null assertion (`p?.n`, `p!.n` on `P ∪ none`). An optional call on an absent receiver skips its argument evaluation; the receiver is captured before the arguments, `mut` arguments work, and an optional result is flattened.
- Tensor glyph products `· × ⊗ ⊙ ⊘`, postfix `ᵀ` and `∇` are covered in `references/idioms.md`.
- `trap { … }` (la `capta`) — expression block that reifies the error channel: the trailing expression is the
  success value and the result type is the success type united with every escaping error type
  (`let r ← trap { f() }`); success and error types that coincide are rejected.
- `reduce via max from t at? (coalesce identity)?` / `reduce via min from t …` (la `reducta via maxima ex t apud? (vel identity)?` / `reducta via minima ex t …`) — extrema over a shaped tensor or matrix, not a `list`.
- `reduce via first from t at? let x { return pred }` (la `reducta via prima ex t apud? sit x { redde pred }`) — first element of a shaped tensor or matrix whose predicate holds, as `T ∪ none` (none for no match or an empty source; `corpus/summa/reducta-first-en.fab`); the former dedicated first-match form is retired, and a `list` is not a source.
- Mixing plain (`§`, `§{name}`) and numbered (`§0`) holes is legal but warns `WARN035 mixed_plain_numbered_template_holes`; plain holes count from 0 independently (`"§0 and §"("a")` renders `a and a`), so number every hole.
- `"§"(a)` renders a template; `format("§!", x)` (la `scriptum`) is its explicit desugaring. `read` / `read line` (la `lege` / `lege lineam`) are builtin read expressions (a user binding of the same spelling wins).
- `#` comments start a line (`LEX007 inline_hash_after_code` after code); `//` and `/* */` are not comments.
- Commas are required between items and forbidden after the last (`PARSE001 item_list_trailing_comma`); declaration
  blocks (`class` members, `interface` methods, `union` payload fields, `column` rows) carry no commas.
- `print` of a `class` shows its construction literal (`P { x = 1, name = "hi" }`), a `union` variant
  prints qualified (`N.Sig { x = 2 }`), a `list` as `[1, 2]`. A statement-initial `print(x)` is a call
  of an identifier `print`, not output.
- `axis` is a marker word beside `mut`/`own`/`copy`: `t.sum(axis 1)` and `t.mean(axis 0)` reduce a rank-2 tensor over a literal axis; `t.mean()` is sum / count, so an empty source is NaN (`corpus/tensor/axis-builtins.fab`, `corpus/summa/mean-sugar.fab`).
- Collection reads: `xs[i]` out of range dies, `xs.get(i)` gives `none`, `xs.at(i)` reports an unproven bounds failure on the error channel (a statically proven access needs no handler; `do { } catch err { }`, la `fac { } cape err { }`, or `⊥ default`); integer guards, loop bounds and lengths prove accesses, and a mutation invalidates the proof; text indexes by Unicode character; an out-of-range range index dies (`corpus/lista/reads-three-ways.fab`).
- Value-side markers: `mut b ← copy a` duplicates, `mut b ← own a` takes over, an unmarked `b ← c` shares; read-only is deep, so a `let` (or `ref`, or unmarked-parameter) container or class into a `mut` cell is `SEM065 const_shared_into_var`; `ref`/`mut` may hold `T ∪ none` for a reference type, never a scalar (`corpus/mutabilitas/ref-optional-cursor.fab`) (`corpus/mutabilitas/copy-bind-deep.fab`, `const-share-into-var-reject.fab`). A method that changes the shape of the collection a loop walks is `SEM011 loop_collection_shape_change`; a bracket store that grows a map in its own loop traps `map changed size during iteration`.
- Retired: `sum from …`, `max from …`, and `min from …` (la `summa ex …`, `maxima ex …`, and `minima ex …`) report `PARSE010`; use `reduce via <op> from …` (la `reducta via <op> ex …`).
- `do device { … } catch e { … }` (la `fac machina`) is a checked device region: kernel launches, entry-known loops and guards, and no host effect, host call, allocation, direct lane loop, nested region or crossing `return`/`break` (named `SEM008` rows); `do all device` is `PARSE020`. V0 lowers it exactly as `do all` (`corpus/kernel/region.fab`, `region-reject.fab`, `docs/design/device-region.md`).
- `for … at […] thread lane let value { … }` (la `itera … filum`) is shipped lane-loop syntax, and host `trapping<f32>` / `saturating<f32>` cells are shipped. Superscript powers (`x²`) and general run-time container capacities remain unshipped (supported run-time tensor extents are a separate case, e.g. `tensor<f32, [2, size n]>`). See "Admitted, Not Shipped" in `grammar/EBNF.md`.
- **Return placement:** a newline ends a bare `return`; its value must begin on the same line. `return thread` ends only the current lane (no value, also from a nested ordinary loop); plain `return` cannot leave a lane; outside a lane, with a value, or inside a nested closure or reduction body it is a named `SEM010`. Placement is check-only in 1.17: execution refuses it as `return_thread_lowering_pending`.
- **Ordered prefix scan:** `scan via op [total inclusive|exclusive] from source [at […]] [per key] [order asc|desc] thread lane let value { … }` (square-bracketed clauses optional; default inclusive, row-major; operators `sum product max min all any count`, exclusive extrema refused by name; lowers to the runner, Metal and CUDA). The shape is the grammar's `scan_expr`; there is no `corpus/scan` exemplar.
- `do all { … } catch e { … }` defers kernel fault delivery to the block exit; `do device { … } catch e { … }` also checks device-region legality and runs as `do all` in 1.17. `faber check --device cpu|metal|cuda` reports placement and synchronisation advice (`INFO001`) without running.

## Capability Calls

`call 'route'` (la `ad`) is an expression that yields a `channel` (la `sermo`) conversation handle. The route is an `ascii` string. Optional opener `(expr)` is request data.

Legacy typed `call "route" (args) → T { }` and statement-level `send`/`recv` (la `meus`/`tuus`) blocks are parse errors. Use postfix `↦ T` or `s.send<T>()` / `s.recv<T>()` on the handle. View methods in en: `finish` (la `fini`), `get` (la `accipe`), `drain` (la `exhauri`), `cursor`; `da` is unchanged. Latin spellings are refused in en files.

`channel<O, R>` carries its types: `O` each item sent, `R` each item framed back; bare
`channel` means `channel<value, value>`. Types `frame` (la `scrinium`; opaque frame) and
`status`/`send`/`recv` cover the conversation surface. `@ call 'prefix:name'` (la `@ ad`) on a
top-level function marks it as the Faber-served answerer for that route — a static table, built at
link time; duplicate routes across packages are a compile error. Served on Rust, Go, TypeScript and the
MIR runner (`faber run corpus/ad/serve-plain.fab` prints its answer).

## CLI Annotations

Structured CLI annotations parsed into AST/HIR metadata:

- `@ cli "name"` — CLI root on a `main`
- `@ command "cmd"` — command entry (la `@ imperium`)
- `@ option ...` — option declaration (la `@ optio`)
- `@ operand ...` — operand declaration (la `@ operandus`)
- `@ description`, `@ version`, `@ alias`, `@ global`, `@ args` (la `descriptio`, `versio`, `ubique`, `argumenta`)

Canonical tour: `corpus/cli/cli.fab`.

Rust owns the full application CLI framework; other targets carry at most a fail-closed root-scalar subset —
see `faber targets` before promising another target runs a CLI program.

## Annotations (general)

Common annotations include:

- visibility: `@ public` exports, `@ private` restricts to the declaring type's methods (members) or
  makes a top-level declaration explicitly private, `@ internal` restricts to the declaring package —
  member/top-level privacy is enforced (`SEM063`/`SEM062`), not decorative; `@ protected` (la `protecta`) is reserved/rejected
- compiler/runtime metadata: `@ radix lane "air" | "mir" | "hir-direct"`, `@ radix typus ...`, `@ rename ...` (la `@ verte ...`)
- contracts/data: `@ annotation { target = fn }` (la `@ annotatio`), application `@ Contract { field = value }`, `@ json`, `@ json { name = "wire" }`
- testing: `test "name" skip "why" | only | tag "x" | timeout N | bench | repeat N | flaky N | todo "why" | expect_failure { … }` are `test`/`describe` modifiers, not annotations (la `proba "name" omitte "why" | solum | tag "x" | temporis N | metior | repete N | fragilis N | futurum "why" | erratur { … }`, `probandum`); `expect_failure` expects a failure captured from the body (such as an assertion failure or runtime fault), not an error-channel escape; clean completion fails, while timeout, step-budget exhaustion and blocked/capability outcomes are not expected body failures
- compute: `@ kernel` on device kernels, with an optional tier word: `@ kernel` (default tier: IEEE-correct float math, every unproven index check kept), `@ kernel relaxed` (adds approximate `/` and `sqrt`, fast transcendentals, flush-to-zero, float reassociation) and `@ kernel unsafe` (includes `relaxed` and drops the index checks the compiler cannot prove; an out-of-range access is undefined behaviour); la `@ nucleum`, `@ nucleum laxus`, `@ nucleum intutus` (provisional). One ordered choice, never both words (`PARSE018 duplicate_nucleum_tier`); an unknown word is `unknown_nucleum_tier`, a tier on a `fragment` is `nucleum_tier_on_fragment`, on anything but a kernel function `nucleum_tier_non_kernel`; the braced form carries `fragment` only. `faber run --safe`, `faber build --safe` and `safe = true` under `[device]` in `faber.toml` run every kernel at the default tier (they only lower a tier; no environment variable). Exemplars `corpus/kernel/tier.fab`, `tier-la.fab`; ruling `docs/factory/faber-language-decisions/kernel-math-tiers-ruling-2026-10-28.md`
- kernel contracts in 1.17: the tiers state permissions, not that every backend implements each one: Metal selects precise or fast math from the tier; CUDA does not yet select math by tier, so `--safe` is no precise-math guarantee there, and `unsafe` does not yet remove unproven index checks. Explicit source checks stay and integer and decimal results stay exact. **Kernel numeric widths:** `i64`, `u64`, `wrapping<u64>`, `saturating<u64>` and `d64` are legal kernel values (`nucleum_wide_integer` and `nucleum_d64` are retired); Metal carries wide integer lanes and decimals (stores round half-even to eight fractional digits; overflow and division by zero are device faults); unsupported operations or devices refuse by name; `f64` stays forbidden. A host-free kernel may declare `⇥ string`; another error type is `SEM008 nucleum_typed_error_channel`.
- availability: `@ unstable [TARGET] ["reason"]` (la `@ nondum`) marks a declaration planned but unavailable; with no `TARGET` it is unavailable on every target. A top-level `fn` or a `class` method marked this way may omit its block (`nondum_functio_decl`, `nondum_method_decl`); a signature-only `fn` or method without it is `PARSE012 function_body_absent`. The function or method may be declared and exported, but any use (call, call through an imported namespace or name, or taken as a value; for a method, a call through an instance, `self`, or an imported class) is `SEM017 nondum_function_for_target` (interface methods: `nondum_method_for_target`). Corpus: `corpus/meta/nondum-function.fab`, `nondum-method.fab`, `nondum.fab`

Do not add `@ externa` or `@ subsidia` to current Norma/Triga/Gradus/Tela source.

Use flat `norma:*` imports. `norma:hal/*` is retired.

## Constructs beyond the core scan

The highlight layer is selective rather than a second grammar. Read the
corresponding idiom entry for:

- shared union fields (`@ shared`, la `@ commune`)
- generic class and explicit call-site application, including the newly landed
  `size` (la `magnitudo`) kind
- kernel fragments (`@ kernel fragment`, la `@ nucleum fragment`) versus
  graphics `@ fragment`
- tagged-union `variant` construction and `case` payload binding
- recoverable `require`
- its boolean opposite `reject` (throws when the condition holds)
- labeled tuples (`tuple`)
- precision-tagged instants (`instant`)
- annotation locale sealing and its `PARSE018` diagnostic

Tensor glyph operators and `sum` thread reductions are already covered
in `references/idioms.md`; do not restate their grammar here. The admitted
transpose suffix is `ᵀ` (U+1D40) on a matrix or a tensor of known rank 3 or more (batched: leading axes kept, last two swapped), and the settled precedence
form is `a · bᵀ ∇ [x]` → `(a · bᵀ) ∇ [x]`. Rank-1 has no row/column distinction
and is refused (`glyph_transpose_rank1_no_row_column_distinction`).


## Validation Discipline

When asked whether syntax is valid:

1. Quote the construct shape from `EBNF.md`, not from memory.
2. Confirm with one real exemplar under `corpus/`.
3. If behavior depends on parser reality, start in `crates/radix` and follow the live parser crate.
4. If behavior depends on codegen or CLI runtime, inspect the live driver and the relevant backend.
5. Probe small: write a `.fab` under the scratchpad (a file with no `locale` frontmatter reads as en), then
   `faber check f.fab` (message and help text; `check --no-diagnostics` prints only the code and issue),
   `faber explain <CODE>` for a diagnostic, and `faber run f.fab`. `faber parse` / `faber lex` print the AST/tokens.

**1.17 command boundaries**

- `check`, `build`, `run`, `test`, `emit`, `format` and `convert` show diagnostic message and help by default; `--no-diagnostics` gives the compact form and the old `--diagnostics` is removed. JSON output is unchanged.
- `faber run --target go|ts|llvm-host` builds the package and runs it, forwarding arguments, standard streams and the program's status (Go needs its toolchain; TypeScript needs `tsc` and Node; LLVM host needs LLVM tools, and release binaries carry the runtime archive). `--target clif-jit` compiles and runs in memory with the Cranelift language limits; `build` and `emit` refuse it.
- `faber run --device cpu --threads N` uses CPU workers (`[device] threads` is the default; `--threads` needs the CPU device); `--device reference` is sequential; `--partitions N` requests CPU, Metal or CUDA partitions. `faber check --device cpu|metal|cuda` checks the device image and prints informational `INFO001` advice without running it.
- Package kernel tests take `--device cpu --threads N` or `--device reference`, never Metal or CUDA; the environment selector is `--only-in` (`--solum-in` is retired).
- `faber format --width N` opts into wrapping (`[format] width = N` is the package default; the flag wins; unwrapped otherwise).
- Tool failure statuses: `64` bad arguments, `65` source errors, `66` missing input, `69` unavailable compiled support, `70` internal failure, `73` unwritable output, `78` bad configuration; failed tests and formatting drift return `1` and a program's own status passes through, so do not assume every trap exits `1` or Rust's `101`.
- `fmir-bin` is removed; `fmir` and `fmir-text` are image outputs, not inputs to `faber run`. A relative `--library-home` resolves against the working directory.
