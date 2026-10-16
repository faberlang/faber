# Faber Grammar

Use this file when the task is about syntax, grammar, or whether a construct is legal Faber.

Primary source:

- `grammar/EBNF.md`

Secondary source:

- `corpus/`

This file is a compact map. Quote the live EBNF section before claiming a construct exists.

## Locale spellings (read this first)

Faber's reader surfaces are **sealed per locale**: one file, one language.
Canonical grammar names below are Latin (`itera`, `summa`, `dum`, `si`) — those
are the production names and the `la`-locale spellings. **`en` is the default
reader**: a file with no locale frontmatter (and no package pin) reads as en,
so a Latin file must say `locale = "la"` in its `+++` frontmatter (or the
package must pin it). An en file may state `locale = "en"` (most
exempla) and uses the English rows of `locale/en/pack.toml`; the other
packs sit beside it in `locale/`. Mixing spellings inside one file is
always a defect. Every Latin sample in this file needs `locale = "la"` to run
as written.

The en forms writers most often need (all probed against the built binary):

| Canonical (la) | en spelling | Use |
| --- | --- | --- |
| `itera ex x` / `itera de m` / `itera ab a‥b per k` | `for from x` / `for ref m` / `for range a‥b step k` | iterate values / map keys / integer range |
| `itera … apud [i, j]` | `for from … at [i, j]` | coordinate-binding walk of a tensor/matrix/lista/tabula (binds indices; tensor rank must match) |
| `reducta via summa ex x apud? filum f sit s { … }` | `reduce via sum from x at [i]? thread f let s { … }` | device sum reduction (see below) |
| `dum` | `while` | pre-test loop (`fac … dum` = post-test) |
| `si` / `sin` / `secus` | `if` / `elif` / `else` | conditionals |
| `ergo` | `then` | one-statement body joint |
| `sit` / `varia` / `de` / `fixum` | `let` / `mut` / `ref` / `const` | run-time bindings (write-once / writable / re-pointable read-only) and the compile-time constant (`fixum`/`const` takes `=` only) |
| `functio` / `incipit` / `nota` | `fn` / `main` / `print` | declaration, entry, output |
| `genus` / `implendum` / `implet` | `class` / `interface` / `implements` | classes |
| `discretio` / `ordo` / `finge` | `union` / `enum` / `variant` | tagged unions, enums |
| `discerne` / `elige` / `casu` / `ceterum` | `match` / `switch` / `case` / `default` | dispatch |
| `fac … cape` / `custodi` | `do … catch` / `guard` | handler block, guards |
| `vel` / `nulla` / `nihil` (type) | `coalesce` / `null` / `none` | nullable default, null value, null type |

**Canonical exemplars to copy, never transliterate:** en-locale iteration lives
in `corpus/itera/` (see `apud-rank1.fab`, `apud-rank3.fab` and the en
files there), the device reduction in `corpus/summa/summa-en.fab`
(`reduce via sum from … at [i] thread f let s { … }` inside a kernel entry), and tensor
glyph arithmetic in `corpus/tensor/glyph-*.fab`. When writing en Faber, open the nearest
exemplar first and match its spellings exactly.

## Core Shape

Programs are statement sequences after optional `+++` TOML frontmatter peeled by the driver.

Major statement families include:

- declarations: `functio`, `genus`, `implendum`, `typus`, `ordo`, `discretio`
- imports: `importa`; file name: `regio` (below)
- flow and control: `si`, `dum`, `itera`, `elige`, `discerne`, `custodi`, `fac`/`cape`
- imperative forms: `redde`, `rumpe`, `perge`, `iace`, `mori`, `adfirma`, `scribe`, `mone`, `vide`, `nota`
- entry and test forms: `incipit`, `incipiet`, `proba`, `probandum`
- inferred locals: `sit`

## File Frontmatter

Optional `+++` TOML on line 1 is peeled by the driver before lexing; it is not token grammar. Tooling reads known metadata such as `group`, `sectio`, `[probanda]`, and package keys consumed by `faber`.

Line-start `§` file directives were removed. Inside quoted strings, `§` remains the string-template hole.

## Module declaration and top-level constants

`regio NAME` (en `module NAME`) optionally names a file. Legal only as the file's very first
declaration, before any import or statement, at most once. Contextual: `regio`/`module` is claimed
only in that leading position, so it stays an ordinary identifier everywhere else.

A module-level constant is a top-level `fixum T X = e` (en `const T X = e`): `=`, not `←`, because the
value is compile-time (folded, `praefixum`, or an embed) and must fit `T`; `fixum _ X = e` infers the
type; `@ publica` exports it. A top-level `sit`/`varia` (en `let`/`mut`) with `←` stays `SEM062 top_level_binding`,
and a run-time `fixum T x ← e` (en `const`) is `PARSE010 runtime_const_retired`. The
retired module-level `static`/`generis` declaration is a parse error: `PARSE010 static_decl_retired`
(`keyword` names the spelling used) — write `fixum`/`const` instead; `static`/`generis` is also retired
as a `genus` member (see the field modifiers below). `insere "path"` (en `embed`) is a build-time file embed, read once at
build time, package-relative path, no `..` escape; `textus` requires valid UTF-8, `octeti` reads raw
bytes. `insere`/`embed` is contextual: claimed in expression position only when directly followed by a
string literal. It is admitted only as the whole initializer of any `=` constant (module level or
block level) or `genus` field default under any modifier; elsewhere `SEM061 insere_outside_constant`.
`praefixum { … }` (en `comptime`) is legal as the direct initializer of every `=` binding; its body
stands alone: no captures of enclosing parameters, locals or `self` (`praefixum_captures_local`), no
type parameters (`praefixum_type_parameter`). There is no `faber const-codemod`: the migration command
is gone, not a thing to run.

Unmarked top-level declarations are module-private by default; `@ publica` exports, `@ interna`
is package-internal-only, `@ privata` is an explicit private marker. A leading-underscore name is a
warning (`WARN030`), not an error.

## Canonical Declaration Forms

Variables (la spellings; an `en` file uses its own pack's words, e.g. `int`, `string`, `list<string>`, never these Latin type names):

```fab
incipit {
    # sit: write-once (en let); varia: writable (en mut)
    sit textus nomen ← "Marcus"
    varia numerus index ← 0
    # inferred forms write the `_` type slot; `sit inferred ← 1` is PARSE010 binding_type_slot_required
    sit _ inferred ← 1
    varia _ total ← 0
    sit lista<textus> xs ← vacua
    # deferred: assign exactly once before any read
    sit numerus late
    # de: re-pointable read-only name over a place (en ref)
    de lista<textus> cur ← xs
}
```

`sit name` with no initializer is the inferred deferred form. `sit`/`varia T x ↢ future` and
`sit x ↢ future` are the await-directed initializers (same as `figendum T x ← future`; `↢` is not an
expression operator and cannot target an existing place).

Functions:

```fab
functio saluta() {
    scribe "Salve"
}

functio duplica(numerus n) → numerus {
    redde n * 2
}
```

Parameter and field markers:

- ownership prefixes `de` / `in` / `own` / `copy` appear in type position (`owned_type ::= ('de' | 'in' | 'own' | 'copy')? base_type`), not as parameter prefixes — the parameter prefix slot is retired
- call-site markers repeat the parameter's marker on the argument: a `mut` / `own` / `copy` parameter (la `in` / `own` / `copy`) is called with `bump(mut a)`, `eat(own a)`, `dup(copy a)`; `ref` (la `de`) is optional (`argument ::= template_argument | call_marker? 'sparge'? expression`). It applies to every source-signature function and to a method's arguments (`obj.run(mut src)`, no receiver marker), and function-value calls use the modes in the function type; built-in/intrinsic methods are exempt. `SEM057` `call_marker_missing` / `call_marker_mismatch` / `call_marker_on_read_only`; a temporary to `mut` is `SEM020 mut_argument_not_writable`; `own`/`copy` take a fresh value (`eat(own make())`)
- parameter writes follow the marker, never a binding word (parameters take no `let`/`mut`/`const`): unmarked and `ref` are read-only (`SEM020`), `mut` writes the caller's value, `own` and `copy` are always writable by the callee (method call, field store, reassignment, element store; an `own` tensor may be rebound whole, a `mut` tensor may not). Only a whole local binding may be passed as `own`: field/element, unmarked parameter, module constant, or `itera` binder is `SEM057 own_argument_not_local_binding` / `own_argument_unmarked_parameter` / `own_argument_module_constant` / `own_argument_loop_binder` (`copy b.xs` is fine and keeps `b` alive); after `eat(own a)` a later read of `a` is `SEM050 use_after_own` (also `moved_in_loop`, `moved_name_captured_by_closure`; `own`/`copy` belong on the value side of a binding, `let b ← own a`, and a binding-position marker is `PARSE010 binding_marker_moved_to_value`)
- `ceteri` — rest parameter
- `sponte` — optional/voluntary declaration slot (post-name)
- `ut` — alias
- `vel` — default expression on a declaration slot
- generic parameters live in `<>` after the name: `functio f<T, magnitudo N>(…)`, type parameters first,
  `magnitudo` (en `size`) size parameters after them; there is no `prae typus` parameter form

Recoverable errors:

- `→ T` — success return type
- `⇥ E` — alternate error-channel type
- `fac { ... } cape err { ... }` — explicit do-block with a local handler

`vel` (en `coalesce`) is also local nullable elimination in expression position (`T ∪ nihil vel T → T`). It is not logical `aut`.

## Conversion-Directed Assignment (`↤`)

`place ↤ value` evaluates the right side, converts it to the statically known
type of the left place through the existing `↦` route, then assigns. Same
precedence and right-associativity as `←`. The
operator is preserved verbatim through syntax and emission — never rewritten
to `←` or `↦`. A `⊥ default` (see **Format (`¶`) and default (`⊥`)** below) is legal only on
`↤` and attaches to the nearest `↤` in a right-associated chain.

```fab
varia numerus count ← 0
count ↤ "42"               # textus → numerus conversion, then store
sit numerus typed ↤ "7"  # typed initializer: convert to the written type
count ↤ "x" ⊥ 0            # ⊥ default stored only on conversion failure (retired: `⇥ 0`)
a ↤ b ↤ "42"               # right-associated: inner converts first
```

Rejected: `varia _ x ↤ …` (no concrete destination type), `sit x ↤ …` (`PARSE002 conversio_assign_requires_concrete_type`), and
`figendum`/`variandum` with `↤` (awaited bindings keep `←`).

## Numeric Model

Teaching exemplar: `corpus/operatores/numerus-widening.fab`.

- **Exact values, checked stores.** Integer arithmetic computes the exact result; the type of an expression is its range type (`u8 + u8` is `u16`, `u8 - u8` is `i16`; at 64 bits the sign is kept, so `u64 - u64` is `i64`). A value is checked only where it lands in a typed slot (declaration, assignment, field, argument, `redde`, element). A trapping slot traps with "N does not fit in `T` (…)"; `wrapping<W>` reduces; `saturating<W>` clamps once at the store. An expression with no slot (`print a + b`) never traps for size. An intermediate of bounded operands outside [−2⁶³, 2⁶⁴ − 1] traps (`2.potentia(64)` traps, `2.potentia(63)` is exact); an operand typed `inf` has no such bound (see **The unbounded integer `inf`** below). An implicit trap never enters `⇥`; explicit `↦ T` is the recoverable form.
- **Untyped constants** are exact values that join the other operand's range (`x + 10` is the same operation at every `x`); in an annotated slot a constant must fit at compile time; `_` slots take the range type (or `int` for constants alone).
- **Division.** `/` floors, `%` takes the divisor's sign (`-7 / 2` is `-4`, `-7 % 2` is `1`); `÷` is true division (`f32` for an 8/16-bit widest operand, else `f64`; `7 ÷ 2` is `3.5`).
- **Families.** Implicit integer widening is lossless only, and `u64` has no implicit widening: a typed `u64` with a typed signed operand is a compile error (fix with `↦`); constants are exempt. Crossing integer / decimal (`d64`) / float needs `↦`; comparisons across families are exact and need none (`i64 ≺ f64` is legal).
- **Bits.** On numbers shifts are unmasked (`x ⇐ n` is `x·2ⁿ`); `¬x` is `-x - 1`; for fixed-width complement or hashing use `wrapping<W>`. The raw-bits markers `b8 b16 b32 b64` are not numbers: each operation acts at the type's own width (`0xF0 ⇐ 4` on a `b8` is `0x00`, `¬0xF0` is `0x0f`, a shift by N or more gives zeros, a negative count is an error); a bN has equality, unsigned-pattern ordering and `∧ ∨ ⊻ ¬ ⇐ ⇒` only, with explicit `↦` to numbers.
- **Float to integer.** A float or `d64` source needs a rounding choice: `x ↦ int via Floor|Ceil|HalfEven|Trunc`; the bare `x ↦ int` is `SEM016 float_to_integer_requires_rounding`, and `⊥ default` / `⇥` follow the `via` as for any failable conversion (`corpus/conversio/float-to-int-via.fab`). The glyph `⌊x⌉` (nearest) is the short form of `via HalfEven`.
- **Local constant.** `fixum T x = e` (en `const`) is a compile-time local (evaluated by the compiler, must fit `T`); a runtime store is `sit T x ← e` (en `let`), and `fixum T x ← e` is `PARSE010 runtime_const_retired`.
- **AIR lane.** In an `@ radix lane "air"` function every integer store and operation must be proven to fit by the range rule; otherwise it is a compile error (declare a wider slot or use `↦ T ⊥ default`).
- **The unbounded integer `inf`** (teaching exemplar `corpus/operatores/inf.fab`, with `inf-conversio.fab`, `inf-json.fab`). The bare marker `inf` is an integer with no bound — opt in by declaring a slot `inf` or converting an operand `↦ inf`; nothing becomes `inf` by default, and a lone literal of any length defaults to `int` (a literal past `u64` needs an `inf` slot or operand, else `SEM010 numerus_literal_out_of_range`). `inf`, `trapping<inf>`, `wrapping<inf>`, `saturating<inf>` (la `exactus`/`modulus`/`saturatus`) all name the same type (the wrapped `int<inf>` is retired, `PARSE040`); the marker is ASCII in every locale, with no prefixed sugar (`linf` is an identifier). `inf` is the implicit lossless target of every integer width including `u64` (`sit inf x ← u` needs no `↦`) and widens into nothing; `u64 - u64` is still `i64` — opt in with `(a ↦ inf) - b`. On `inf`: `+ - * ⇐ potentia` never trap, `/` `%` floor, `∧ ∨ ⊻ ¬` use infinite two's complement, a zero divisor or negative shift count/exponent still traps, comparisons against every integer width, `d64` and float are exact, and an `inf` is a valid `tabula` key. Narrowing back is a checked conversion: `x ↦ u64 ⊥ 9` (a value that does not fit takes the default; without `⊥`/handler it is `failable_conversio_requires_handler`), `↦ wrapping<W>` reduces and `↦ saturating<W>` clamps (never fail); `↦ float` rounds and a magnitude past the float range is `inf`/`-inf`; float → `inf` truncates and fails only on NaN/∞; `d64 ↦ inf` rounds half-even; text parses any number of digits (`"ff" ↦ inf via Hex`); `inf ↦ octeti via Be`/`Le` is minimal two's complement. A store from `inf` into a bounded slot traps (``300 does not fit in `u8` ...``). `inf` is host-only: `SEM008 tensor_element_unbounded` (tensor/vector/matrix/sparsa element), `SEM008 nucleum_host_type` (kernel), `SEM059 air_unbounded_integer` (AIR lane). `∞` is the float infinity, never an `inf` value, and a float ∞ prints as `inf`. Targets: the runner, Rust, TypeScript, Go, Python and sexp carry it; Swift, Haskell, LLVM and Wasm fail closed by name (`inf_target_unsupported`, `llvm_target_inf_unsupported`, `mir_wasm_unsupported`) — check `faber targets` for per-target gaps.
- **`≈` on integers.** Between integers of any width including `inf`, `≈` is one exact rule, `10⁹·|a − b| ≤ max(|a|, |b|)`, not a float compare (`corpus/operatores/approx-integers.fab`).

## Bit Reinterpretation and Float Field Reads

- Bit patterns go through the raw-bits markers `b8 b16 b32 b64` (bits are not numbers): `x ↦ b32` reads the pattern of an `f32` (`↦ b64` for `f64`), `u32 ↦ b32 ↦ f32` and `u16 ↦ b16 ↦ f16` reinterpret a pattern as a float; reinterpretation, never value conversion. `via Bits` is retired (`SEM016 conversio_via_bits_retired`, names the `bN` route).
- **The `via` clause.** A convert hint follows the target: `x ↦ u16 via Le`, `"ff" ↦ i32 via Hex`, `65 ↦ littera via Code`; register targets take it too (`v ↦ octeti<16> via Le`). The text/byte hint set is `Hex`, `Bin`, `Oct`, `Be`, `Le`, `Code`, and a float or `d64` to integer takes the rounding hints `Floor`, `Ceil`, `HalfEven`, `Trunc`; it binds tighter than `⊥` and applies per hop (`octeti[0‥2] ↦ u16 via Le ↦ b16 ↦ f16 ↦ f32`). On bits lists a `via Le|Be` clause is written exactly when the bit width changes (`lista<b8> ↦ lista<u16> via Le`; `lista<b8> ↦ lista<u8>` takes none). `via` is a contextual keyword (an ordinary identifier elsewhere). The retired spellings — a hint as a type argument (`numerus<u32, Hex>`) or a register tail (`octeti<16><Le>`) — are parse errors (`PARSE030 conversio_hint_type_argument` / `conversio_hint_tail_argument`). `@ nucleum lane "x"` is removed (`nucleum_lane_removed`).
- **Bulk bytes to tensor.** `octeti ↦ tensor<E, [d1, …]> via Le` / `via Be` is a faithful reinterpretation of a byte string as a typed tensor: source `octeti` or `octeti<N>`; literal extents only; `E` is one of `u8 i8 i16 u16 i32 u32 i64 u64 f32 f64`; the hint is REQUIRED for every width (no hint, or `Bits`/`Hex`, is `SEM016 octeti_to_tensor_unspecified`; another element type is `octeti_to_tensor_element_unsupported`); row-major, element `i` at bytes `[i·w, (i+1)·w)`; pure bit copy (NaN payloads, `-0.0`, subnormals survive); the byte length must be exactly `product(shape)·w` (a compile error `octeti_to_tensor_length_mismatch` when statically known, otherwise a recoverable runtime error: handle it with `do { } catch`/`⊥`/`⇥`, and a bare `↦` outside `main` needs a handler). Example: `let tf32[2, 2] m ← raw ↦ tf32[2, 2] via Le`. Decode a whole buffer or weight file at once this way, never element by element through a list. NOT available: the reverse `tensor ↦ octeti via Le|Be` (held back), `lista<f32> via Le` (still `SEM016 unsupported_conversio`), runtime (non-literal) extents.
- finite/NaN detection reads the bit fields through `x ↦ b32 ↦ u32` (exponent all-ones, fraction nonzero separates NaN from ±Inf) — never `x ≢ x`, which is lane-dependent (the MIR runner's float `≢` is NaN-total). Exemplar: `corpus/scalar/finite-nan.fab`.

## Copy-Into (`⇇`)

`target ⇇ value` copies every value of the tensor `value` INTO the existing
storage of `target`: the storage identity is preserved, so every alias of the
target sees the new values. `←` only ever rebinds a name; `⇇` is the one way to
write into a tensor that already exists. It is a statement (result `vacuum`),
single only (`transfer ::= ternary ('⇇' ternary)?`), and binds above assignment
and below ternary, so `output ⇇ (q · kt) ⊙ s` copies the whole product.

```fab
fn fill(mut tf32[2, 2] target, tf32[2, 2] source) → void {
    target ⇇ source.transpose()      # a view on the right is fine
}
main {
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    mut tf32[2, 2] t ← flat ↦ tensor<f32, [2, 2]>
    let tf32[2, 2] a ← t           # `←` makes an alias of the same storage
    t ⇇ t + t                        # writes in place; `a` sees it
}
```

The copy-into rules, each a named diagnostic:

- **R1** same element type and shape on both sides (`copy_into_type_mismatch`;
  a static shape mismatch is `incompatible_tensor_index`; runtime-extent shapes
  give a recoverable runtime error value, never a kernel trap). **R2** the target
  is writable tensor storage: a `mut` parameter, a `mut` local, a field of a writable
  root; never a `let` or `const` (`copy_into_target_immutable`), a view
  (`copy_into_target_view`, future question), or a scalar/list/matrix/vector
  (`copy_into_target_not_tensor`; a value type is written by `←`).
  **R3** the value may be a view but must not alias the target
  (`copy_into_alias`, `←` aliases count). **R4** no value use
  (`copy_into_value_used`). **R5** no chain (`a ⇇ b ⇇ c` is `PARSE030
  copy_into_chain`).
- **R6 (PROVISIONAL)** a plain `←` of a whole tensor into a `mut` tensor
  parameter is rejected everywhere, host and kernel alike
  (`SEM057 mut_tensor_param_rebind`, help names `⇇`), so a `mut` tensor has one
  write meaning. R6 is regarded as arbitrary and is to be reconsidered once
  in-place writes are settled; do not treat it as permanent law. **R7** the old
  callable-sink form `sink ⇇ payload` (a callable on the left) is retired:
  `SEM012 transfer_sink_retired`, scheduled for removal in a future major release.
  Output is `print`/`nota`, not `⇇`; the `output-transfer-codemod` command is
  gone.

Execution paths: the MIR runner, Rust (`Tensor::write_values`), and Metal/WGSL kernels
(the value is computed straight into the output buffer) carry it; TS, Go,
Swift, LLVM, Wasm and sexp fail closed by name. Teaching exemplum:
`corpus/transfer/transfer.fab` (negatives `transfer/copy-into-*-reject.fab`).

## Classes, Interfaces, Unions

```fab
genus Persona {
    sit textus nomen
    varia numerus aetas = 0
    fixum textus GENS = "Roma"
}

implendum Scriba {
    functio scribe(textus nuntius) → vacuum
}

discretio Event {
    Click { numerus x, numerus y }
    Quit
}
```

Genus and variant construction uses `Type { field = expr }`. Bare `{ ... }` is a JSON document, not an anonymous Faber object.

**Copy with changes:** `Genus { field = value, … } ex source` builds a new value: the braced fields
override, every other field comes from `source` by value (`from` takes values: an omitted list or object field is
duplicated, so the new value shares nothing with `source`; private fields copy across too). The en spelling is `Genus { … } from source`. `ex` must open on the closing `}`'s line — a line-leading `ex` is
instead the extraction statement (`ex p sit x, y`). Exactly one source per literal. `sparge` was
removed from construction literals (it stays for lists and calls); `nexum` was removed outright.

**Field modifiers and privacy:** every field declares exactly one of `sit`
(en `let`: per instance, set once in the construction literal), `varia` (en `mut`: per instance, reassignable), or `fixum T X = e` (en `const`: the type-level compile-time constant, one per type, read through the type name `P.X`; a store is `SEM020 assignment_to_const_member`, a missing `=` is `PARSE010 const_member_requires_initializer`); an unmarked field is `PARSE010 field_modifier_missing`, `generis` (en `static`) is `PARSE010 static_member_retired`, and `de` (en `ref`) on a field is `ref_field_refused`. Members are public
by default; `@ privata` restricts a member to the type's own method bodies, `@ interna` to the
declaring package. A construction literal, and `Genus { … } ex p`, may still set/copy a private field
from any file. Reading/writing/calling an inaccessible member from outside its scope is `SEM063`.
`@ publica` on a member is a redundant-annotation warning (`WARN028`). `field-modifier-codemod` (see
`faber field-modifier-codemod --help`) inserts `varia` (en `mut`, mutable) before every unmarked field, and
nothing else.There is no more
`sub`/`abstractus` inheritance surface — `genus` types compose through `implet` contracts only.

Imports:

```fab
importa ex "hono" Hono
importa ex "norma:console" console
importa ex "norma:text"                # binds `text`; no marker means no re-export
importa { ex = "norma:json/solve", ut = solve_mod }
importa ex "faber:*" faber             # kernel manifest glob (script mode only), not wildcard import
importa ex "lodash" * ut _
importa ex "./types" publica User      # `publica` re-exports
# Selective imports: one fixum local per exported member (value or type).
importa ex "norma:console" fixum dic ut output
importa ex "norma:fs" fixum scribe ut out_file, funde ut out_bytes
```

The `privata` import marker was removed (`PARSE050 import_privata_removed`): an import without a marker
does not re-export and `publica` is the only marker. Norma module names are the English source names
(`norma:console`, `norma:text`, `norma:fs`, `norma:json`, `norma:process`, `norma:time`, …; the module list
is `lib/norma/src/*.fab`); a Latin name such as `norma:consolum` or `norma:chorda` is `PKG001
unknown_library_module`. The `la` pack only projects member names (`dic`, `scribe`, `funde`);
`lib/norma/locale/la/pack.toml` lists them.

Selective imports (`fixum` member list after the import path) bind exported
members as ordinary immutable locals: the imported file interface supplies the
complete type, and the bindings are never re-exports.

## Types

Primitives:

- `textus`, `ascii`, `numerus`, `fractus`, `bivalens`
- `exactus<W>` — en `trapping<W>`; the trapping policy spelled out: the same type as the bare marker `W`, and a store traps when the value does not fit (integer widths and `d64`; `exactus<f32>` is `trapping_float_not_implemented`)
- `modulus<W>` — en `wrapping<W>`; modular word, signed or unsigned; the value reduces mod 2^W at the store, and `↦` into it reduces and cannot fail
- `saturatus<W>` — en `saturating<W>`; saturating integer, signed or unsigned; arithmetic is exact and the value clamps to `W` **once, at the store** (never per operation); `↦` into it clamps and cannot fail
- `nihil`, `vacuum`, `numquam`, `ignotum`, `octeti`, `instans`
- `littera` — en `char`; one Unicode scalar value (4 bytes, never allocates); element of `textus`/`ascii` iteration and `textus[i]`/`ascii[i]` indexing
- `octetus` — en `byte`; an exact alias of `u8` (fixed-width, rejects applied parameters); `octeti ≡ lista<octetus>` is a type-identity fact, not mutual convertibility

Sized numerics are **bare markers**: `i8`…`i64`, `u8`…`u64`, `f16`, `bf16`, `f32`, `f64`, `d64` (the decimal), `inf` (unbounded integer). Bare `numerus` / `fractus` are `i64` / `f64`. The wrapped forms `numerus<W>` / `fractus<W>` (en `int<W>` / `float<W>`, every locale's word), the holes `numerus<_>` / `fractus<_>` and wrong-family forms (`numerus<f32>`) are `PARSE040 numeric_wrapper_retired`. `d32` is removed (`SEM008 removed_decimal_width_d32`). The policy words keep their argument: `exactus<W>` / `modulus<W>` / `saturatus<W>` (en `trapping` / `wrapping` / `saturating`), e.g. `modulus<u32>`, `saturatus<i16>`. Check EBNF for the live marker sets.

Collections and buffers:

- `lista<T>`, `tabula<K,V>`, `copia<T>`, `queue<T>`, `stack<T>`
- bounded capacity forms take a `NATURAL` (or `_` to infer): `lista<T, N>`, `queue<T, N>`, `stack<T, N>`,
  `textus<N>`, `ascii<N>`, `octeti<N>`; capacities are compile-time values today (run-time capacities and
  extents are admitted, not shipped)
- `promissum<T>`, `cursor<T>`
- `tensor<T, Figura>`, `sparsa<T, Figura>` — shaped buffers
- `vector<T, N>`, `matrix<T, [R, C]>` — register-class, not buffer aliases
- `atomic<T>` — storage-sensitive atomic cell; `T` is `i32` or `u32` only (`SEM008 atomic_element`), access through `load`/`store`/`exchange`/`compare_exchange`
- `ratio<Label: T, …>` — labeled record type; every element needs a unique label, fields are accessed by label only (no positional or bracket access, no structural equivalence with a genus); `faber explain ratio` for construction forms

`apud [i, j]` coordinate binding, the sort expression `sort via asc|desc from xs` (la `ordinata via ascendens|descendens ex xs`; grammar `sort_expr` [234]; a body `let r { return r.k }` (la `sit r`) gives the key, ties keep source order; a map sorts values with `from`, keys with `ref`; no body sorts by natural order via `Orderable`; a bare sort or reduce as a `for` source is `PARSE030 itera_source_sort_reduce_unbound`, bind it first), the in-place method `xs.sort(key)` (la `ordina`), and hashable keys (tuples,
`genus`/`discretio` values when every part is hashable) are live on `lista`/`tabula`/`copia`; a
non-hashable map key or set element is a named diagnostic (`tabula_key_not_hashable`,
`copia_element_not_hashable`).

Type forms:

- nullable values: `T ∪ nihil` (en `T ∪ none`; not `T?`). The null *type* and null *value* are different words: type `nihil` / `none`, value `nulla` / `null`; `T ∪ nulla` / `T ∪ null` is `PARSE040 null_value_in_type_position`, and `est`/`non est` is a type test only (`x est nihil`; `x est verum` is `SEM011 est_value_rhs`, use `≡`)
- inline unions: `T ∪ U`
- tagged unions: `discretio`
- function types: `(A) → B`, optionally `(A) → B ⇥ E`
- ownership prefixes on types: `de T`, `in T`
- inferred binding type: `sit _ name ← value`, `varia _ name ← value`, `de _ name ← place`, `fixum _ NAME = value`; there is no type-omitted declaration (`sit name ← value`, `varia name`, `de name`, `fixum name` are `PARSE010 binding_type_slot_required`); binders (`itera … sit x`) take no type slot
- type holes: `_` infers one inhabitant; standalone `∪` infers a finite union
- inferred constructor: `_ { x = 1 }` builds the cell's class, `_ [1, "a"]` its tuple or list; a bare `{ x = 1 }` or `[1, "a"]` takes the cell's type (sugar for `_ shape`); with no typed cell it is `SEM016 inferred_constructor_no_expected_type` (`corpus/genus/inferred-constructor*.fab`, `inferred-literals.fab`)
- labeled tuple forms: `iuncta` / `tuple` type arguments, construction, and member access
- width / shape sugar (type position only, identical to the long form): `lu32` = `lista<u32>`, `tf32[2, 3]` =
  `tensor<f32, [2, 3]>`, `sf32` = `sparsa<f32, _>`, `vf32[4]` = `vector<f32, 4>`, `mf32[4, 4]` =
  `matrix<f32, [4, 4]>` (matrix needs both extents); `inf` takes no prefix (`linf` is an identifier)
- `lista<∪>` / `tabula<K, ∪>` infer a heterogeneous union element; `lista<_>` the single inhabitant
- inline intersection `A ∩ B` (nominal; binds tighter than `∪`); `genus G implet A ∩ B` is the same as `implet A, B`
- contract bounds: `functio maior<T implet C<T>>(T a, T b) → T` — one keyword, `implet`, on a `genus`
  declaration and on a type parameter; bounds are written in full (`T implet Orderable<T>`, no bare-name
  shorthand). `Orderable<T>` (`norma:order`) is the one ordering contract — its `compare` drives every
  comparison glyph so they cannot disagree; built-ins conform automatically, user types opt in.

Banned:

- `cum` keyword
- postfix `T[]` array syntax
- `Type(...)` construction for tensors, vectors, matrices, or scalars — use `↦`, named constructors, or `Genus { field = value }`

`vacua` needs an explicit collection type. `empty`/`vacua` is the DEFAULT EMPTY INSTANCE of the declared type on the left: an empty collection for lists, maps and sets, one default slot for a rank-0 tensor, an all-zero sparse tensor, and for a tensor, vector or matrix the ZERO, so `mut tf32[2, 3] blank ← empty` means exactly `mut tf32[2, 3] blank ← tf32[2, 3] {}`. A tensor with an unresolved `_` extent is refused by name (`SEM011 vacua_requires_resolved_extents`). `faber format` keeps writing `empty`/`vacua` as authored. The typed zero `Type {}` stays the explicit spelling; the bare type as a value (`← tf32[2, 3]`) stays retired.

## Control Flow

- `si` / `sin` / `secus` — conditionals; `sin` takes its condition directly (`sin si b` and `secus si b` are parse errors)
- `ergo` (en `then`) — one-statement bodies of `si`/`dum`/`itera`/`casu`/`ceterum` arms, including `ergo redde`; `∴` is **not** accepted there (`PARSE026`): `∴` is the compact-closure joint only (`(numerus a) ∴ a * 2`)
- `dum` — loops
- `itera ex` / `itera de` / `itera ab` — iteration (values / keys / range, `ab 0‥10 per 2` or `0 usque 10`); the binder is `sit`/`varia` (en `let`/`mut`; `de`/`ref` is refused, `PARSE010 ref_binder_refused`; the old `fixum` binder is `binder_fixum_retired`) plus a name or an array/object pattern, and an optional `apud [i, j]` clause (only on `itera ex`, over an indexed iterable) binds coordinates
- `elige` / `casu` / `ceterum` — switch-like dispatch
- `discerne` / `casu` — pattern-match dispatch, a statement not an expression; `discerne omnia` (en `match all`) is the exhaustive form and forbids `ceterum` (`SEM044 catchall_arm`, `SEM040 missing_variants`); patterns are flat (no nesting, no guards, no range patterns)
  - closed set only: a union's members or an enum/`discretio`'s variants. A type pattern outside the scrutinee's closed set is `SEM010 discerne_pattern_not_in_closed_set`; width (`casu u32`) and length-shape (`casu lista<numerus, 4>`) patterns are retired — ask width with `est` (en `is`), length with `si x.longitudo() …` (en `.length()`); open scrutinees use `elige`
- concrete patterns bind in `discerne`/`casu` clauses, `itera` binders, and `←` destructuring: `_`, literals, `ID ut alias`, multi-bind `(sit|varia) a ut b, c ut d`, arrays `[a, _, ceteri rest]`, objects `{ f, f ut alias, ceteri … }` (`pattern`, `array_pattern`, `object_pattern`)
- `primus_quem(expr apud? [i, j], ubi binder { predicate })` — first-match selection expression (`first_match_expr`)
- `reducta via op ex expr apud? pro? (filum? (sit | varia) binder { … } | vel identity)?` (en `reduce via op from …`) — shaped-source reduction expression (`reducta_expr`; `via` selects one of nine locale-pack operator words)
- `custodi { si cond … }` — guard
- `c ✓ a ✗ b` — the one value conditional ("`a` when `c` holds, else `b`"); replaces the retired
  `?:`/`sic … secus` ternary, which now reports a migration diagnostic
- `praefixum { ... }` (en `comptime`) — build-time-evaluated block; legal only as the direct initializer of an `=`
  binding (module/block constant, `genus` field default), else `SEM064 praefixum_outside_constant`. Body stands
  alone: no enclosing params/locals/`self`, no type parameters (`praefixum_captures_local`,
  `praefixum_type_parameter`); no `⇥` channel inside — failable calls must be handled within the block
- `fac { ... }` — explicit do block; `fac { ... } dum cond` is the post-test loop
- `cape` — structured local handler on `fac` and other named forms, not arbitrary bare blocks
- `tacet` — no-op
- `incipit { ... }` — entry block
- `incipit argumenta args { ... }` — CLI entry with bound args; `incipiet` is the async entry; `exitus expr` supplies the exit code

Membership is the glyph pair `∈` (member) / `∉` (not member), in the comparison tier, with no ASCII spelling. One operator covers
collections (`x ∈ list`, `x ∈ set`, map keys) and ranges (`x ∈ 0…10`, `x ∈ a‥b`); `∉` is a first-class negation. There is no
membership over text (`SEM010 membership_over_text`): use `haystack.contains(needle)` (la `continet`). The old words `intra` /
`inter` (en `within` / `between`) are retired as operators (`PARSE001 membership_keyword_retired`, which names `∈`) but remain
ordinary identifiers.
`rumpe`/`perge` take no label. Functions and type declarations are top-level only
(`PARSE010 declaration_not_top_level` inside a block).

## Error Handling

- `requirit` / `require` — recoverable condition plus typed thrown value; it requires a `⇥ E` channel and is distinct from fatal `adfirma` / `assert`
- `reice` / `reject` — the boolean opposite of `requirit`: it throws the typed value when the condition holds (`reice cond iace err` lowers to `si (cond) { iace err }`); same `⇥ E` requirement
- `iace` — throw (recoverable)
- `mori` — panic (fatal)
- `adfirma` — runtime invariant; failure is fatal and uncatchable by `cape`
- `cape name { ... }` — local handler on the structured forms that name `catchClause`
- same-line `iace val si cond` / `mori val si cond` is line-sensitive sugar for a `si` block

Current error handling is `fac { } cape err { }`. The old `tempta` / `demum` keyword pair is retracted. `tempta` may still appear as an ordinary Norma function name.

### Declared error-conversion functions

The live EBNF keeps unions pure (fields plus variants) and declares each
error conversion as a named standalone annotated function:

```ebnf
union_member ::= annotation* field_decl
```

```fab
@ conversion
fn from_wire_error(WireError err) → BoundaryError {
    return variant WireFailure {message = err.message, inner = err}
}
```

The Latin surface is `@ conversio functio …`; the annotation identity remains
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
handler sees the original source error, and non-call sinks such as `↤`, `iace`,
and `requirit` retain their ordinary type checks.

## Capture-free closures (retired)

The closure-signature modifier (`closure_modifier ::= 'libera' | 'nucleum'`)
was removed outright (the
grammar rows left the EBNF). Closures carry no
modifier slot. Closures inside kernels are legal with no modifier;
kernel-ness is owned by the `@ kernel` / `@ nucleum` declaration annotations
plus the check-time kernel legality pass. Kernel examples live in the kernel
sections of `references/idioms.md`.

## Async and Streams

Prefer posture keywords over the old annotations:

- `fiet` — async finite
- `fiunt` — sync stream; a call returns `cursor<T>` (collect with `gen() ↦ lista<T>`; `sit lista<T> xs ← gen()` is a type error)
- `fient` — async stream
- `figendum` / `variandum` — await-bind
- `reddet` / `tacebit` — await-return / await-discard
- `cede` — yield (`fiunt` / `fient` only)

`@ futura` and `@ cursor` still parse as legacy annotations. Do not emit them in new code.

## Literals and Construction

- `"..."` / `«...»` — `textus`
- `'...'` — `ascii`
- `|de ad be ef|` — `octeti` (compile-time hex bytes)
- `` `...` `` — backtick string token: `capture-pending` in the grammar (reserved; it does not parse as an expression today)
- `[ ... ]` — list
- `{ "key": constant }` — JSON document (`json`); values are JSON constants only
- `Type { field = expr }` — genus / variant construction
- `∞` — positive infinity; the named NaN literal is `nonnumerus` in the Latin (`la`) pack and `nan` in every other shipped pack. Both are contextual floating-point literals: a surrounding `f32` or `f64` supplies the width, bare `fractus` remains unsized, and neither form has a width suffix. `-∞` is unary negation of `∞`, not a separate literal. A `numerus` context rejects either form (fail-closed); neither maps to an integer.
- `"pattern" ↦ regex` — compiled regex; slash-delimited regex literals are not grammar

Anonymous `{ key = expr }` objects are retired.

**Format (`¶`) and default (`⊥`):** `value ¶ "spec"` renders a value to `textus` — the value is the
value, `¶` says how it is shown (`"§"(pretium ¶ ".2")`). Same glyph in every locale (no word twin);
binds looser than arithmetic, tighter than comparison; does not chain. `⊥` supplies a value when a
conversion or a failable call fails: `sit numerus n ← "abc" ↦ numerus ⊥ 0`, or `sit numerus n ←
risum() ⊥ 0` when `risum` is failable. On a conversion it follows the target immediately (`↦ T ⊥
expr`); at a call site it follows the call. `⊥` replaced the retired `⇥ value` inline-recovery form —
`⇥ E` now only ever names an error **type**, never supplies a fallback value.

## Operators and expression notes

Grounded in `EBNF.md` and probes against the built binary.

- Ranges: `a‥b` (exclusive), `a…b` and `a usque b` (inclusive), `a ante b` (exclusive), optional `per k` step.
  `x ∈ lista` is collection membership and `x ∈ lo‥hi` is range containment; `x ∉ …` negates either. Membership over a
  range checks and emits on Rust, Go, Swift, and TS, but `faber run` fails with `unsupported MIR lowering: binary operator
  without a MIR primitive` (no MIR lowering for range containment yet) — do not rely on running it.
  A char-range form such as `x ∈ '0'…'9'` is not supported (a known gap).
- `a ⤒ b` / `a ⤓ b` — max / min (additive tier, left-associative). `↑` / `↓` — postfix increment / decrement
  statements (checked store of the slot's policy).
- `x ∷ T` — static ascription (compile-time fact; a numeric narrowing through `∷` is `SEM016
  numeric_ascription_requires_conversion`); `x ↦ T` is the checked runtime conversion. `x ↦ lo‥hi` /
  `x ↦ lo…hi` clamps a number into an interval (`15 ↦ 0‥10` is 9, `15 ↦ 0…10` is 10) and takes no `via`/`⊥`.
- `?.` `?[` `?(` — optional chaining; `!.` `![` `!(` — non-null assertion (`p?.n`, `p!.n` on `P ∪ nihil`).
- Tensor glyph products `· × ⊗ ⊙ ⊘`, postfix `ᵀ` and `∇` are covered in `references/idioms.md`.
- `capta { … }` (en `trap`) — expression block that reifies the error channel: the trailing expression is the
  success value and the result type is the success type united with every escaping error type
  (`sit r ← capta { f() }`); success and error types that coincide are rejected.
- `reducta via maxima ex t apud? (vel identity)?` / `reducta via minima ex t …` (en `reduce via max from` / `reduce via min from`) — extrema over a shaped tensor or matrix, not a `lista`.
- `"§"(a)` renders a template; `scriptum("§!", x)` is its explicit desugaring. `lege` / `lege lineam` are builtin read expressions (a user binding of the same spelling wins).
- `#` comments start a line (`LEX007 inline_hash_after_code` after code); `//` and `/* */` are not comments.
- Commas are required between items and forbidden after the last (`PARSE001 item_list_trailing_comma`); declaration
  blocks (`genus` members, `implendum` methods, `discretio` payload fields, `columna` rows) carry no commas.
- `print` of a `genus` shows its construction literal (`P { x = 1, nomen = "hi" }`), a `discretio` variant
  prints qualified (`N.Sig { x = 2 }`), a `lista` as `[1, 2]`. A statement-initial `nota(x)` is a call
  of an identifier `nota`, not output.
- `axis` is a marker word beside `mut`/`own`/`copy`: `t.sum(axis 1)` and `t.mean(axis 0)` reduce a rank-2 tensor over a literal axis; `t.mean()` is sum / count, so an empty source is NaN (`corpus/tensor/axis-builtins.fab`, `corpus/summa/mean-sugar.fab`).
- Collection reads: `xs[i]` out of range dies, `xs.get(i)` gives `none`, `xs.at(i)` reports on the error channel (`do { } catch err { }` / `fac { } cape err { }` / `⊥ default`); text indexes by Unicode character; an out-of-range range index dies (`corpus/lista/reads-three-ways.fab`).
- Value-side markers: `varia b ← copy a` duplicates, `varia b ← own a` takes over, an unmarked `b ← c` shares; read-only is deep, so a `sit` (or `de`, or unmarked-parameter) container or genus into a `varia` cell is `SEM065 const_shared_into_var`; `de`/`varia` may hold `T ∪ nihil` for a reference type, never a scalar (`corpus/mutabilitas/ref-optional-cursor.fab`) (`corpus/mutabilitas/copy-bind-deep.fab`, `const-share-into-var-reject.fab`). A method that changes the shape of the collection a loop walks is `SEM011 loop_collection_shape_change`; a bracket store that grows a map in its own loop traps `map changed size during iteration`.
- Retired: `summa ex …`, `maxima ex …`, and `minima ex …` (en `sum from …`, `max from …`, and `min from …`) report `PARSE010`; use `reducta via <op> ex …` (en `reduce via <op> from …`).
- Admitted but **not** accepted by the parser today: `fac omnia`, `itera … filum`,
  superscript powers (`x²`), slash regex literals, run-time capacities/extents, `trapping`/`saturating` float
  cells. See "Admitted, Not Shipped" in `grammar/EBNF.md`.

## Capability Calls

`ad 'route'` is an expression that yields a `sermo` conversation handle. The route is an `ascii` string. Optional opener `(expr)` is request data.

Legacy typed `ad "route" (args) → T { }` and statement-level `meus`/`tuus` blocks are parse errors. Use postfix `↦ T` or `s.meus<T>()` / `s.tuus<T>()` on the handle.

`sermo<O, R>` (en `channel`) carries its types: `O` each item sent, `R` each item framed back; bare
`sermo` means `sermo<valor, valor>`. Types en-named `scrinium`/`frame` (opaque frame) and
`status`/`send`/`recv` cover the conversation surface. `@ ad 'prefix:name'` (en `@ call`) on a
top-level function marks it as the Faber-served answerer for that route — a static table, built at
link time; duplicate routes across packages are a compile error. Served on Rust, Go, TypeScript and the
MIR runner (`faber run corpus/ad/serve-plain.fab` prints its answer).

## CLI Annotations

Structured CLI annotations parsed into AST/HIR metadata:

- `@ cli "name"` — CLI root on an `incipit`
- `@ imperium "cmd"` — command entry
- `@ optio ...` — option declaration
- `@ operandus ...` — operand declaration
- `@ descriptio`, `@ versio`, `@ alias`, `@ ubique`, `@ argumenta`

Canonical tour: `corpus/cli/cli.fab`.

Rust owns the full application CLI framework; other targets carry at most a fail-closed root-scalar subset —
see `faber targets` before promising another target runs a CLI program.

## Annotations (general)

Common annotations include:

- visibility: `@ publica` exports, `@ privata` restricts to the declaring type's methods (members) or
  makes a top-level declaration explicitly private, `@ interna` restricts to the declaring package —
  member/top-level privacy is enforced (`SEM063`/`SEM062`), not decorative; `@ protecta` is reserved/rejected
- compiler/runtime metadata: `@ radix lane "air" | "mir" | "hir-direct"`, `@ radix typus ...`, `@ verte ...`
- contracts/data: `@ annotatio { target = functio }`, application `@ Contract { field = value }`, `@ json`, `@ json { nomen = "wire" }`
- testing: `proba "name" omitte "why" | solum | tag "x" | temporis N | metior | repete N | fragilis N | futurum "why" | erratur { … }` are `proba`/`probandum` modifiers, not annotations; `erratur` expects the body to escape through `⇥`
- compute: `@ nucleum` on device kernels
- availability: `@ nondum [TARGET] ["reason"]` (en `@ unstable`) marks a declaration planned but unavailable; with no `TARGET` it is unavailable on every target. A top-level `functio` or a `genus` method marked this way may omit its block (`nondum_functio_decl`, `nondum_method_decl`); a signature-only `functio` or method without it is `PARSE012 function_body_absent`. The function or method may be declared and exported, but any use (call, call through an imported namespace or name, or taken as a value; for a method, a call through an instance, `self`, or an imported genus) is `SEM017 nondum_function_for_target` (interface methods: `nondum_method_for_target`). Corpus: `corpus/meta/nondum-function.fab`, `nondum-method.fab`, `nondum.fab`

Do not add `@ externa` or `@ subsidia` to current Norma/Triga/Gradus/Tela source.

Use flat `norma:*` imports. `norma:hal/*` is retired.

## Constructs beyond the core scan

The highlight layer is selective rather than a second grammar. Read the
corresponding idiom entry for:

- shared union fields (`@ commune` / locale surface `@ shared`)
- generic class and explicit call-site application, including the newly landed
  `magnitudo` / `size` kind
- nucleus fragments (`@ nucleum fragment` / English `@ kernel fragment`) versus
  graphics `@ fragment`
- tagged-union `variant` construction and `case` payload binding
- recoverable `requirit` / `require`
- its boolean opposite `reice` / `reject` (throws when the condition holds)
- labeled tuples (`iuncta` / `tuple`)
- precision-tagged instants (`instans` / `instant`)
- annotation locale sealing and its `PARSE018` diagnostic

Tensor glyph operators and `summa`/`sum` thread reductions are already covered
in `references/idioms.md`; do not restate their grammar here. The admitted
transpose suffix is `ᵀ` (U+1D40), rank-2 only in v1, and the settled precedence
form is `a · bᵀ ∇ [x]` → `(a · bᵀ) ∇ [x]`. Rank-1 has no row/column distinction
and is permanently declined; rank-3+ waits for a batched-transpose consumer.


## Validation Discipline

When asked whether syntax is valid:

1. Quote the construct shape from `EBNF.md`, not from memory.
2. Confirm with one real exemplar under `corpus/`.
3. If behavior depends on parser reality, start in `crates/radix` and follow the live parser crate.
4. If behavior depends on codegen or CLI runtime, inspect the live driver and the relevant backend.
5. Probe small: write a `.fab` under the scratchpad (a file with no `locale` frontmatter reads as en), then
   `faber check --diagnostics f.fab` (message and help text; plain `check` prints only the code and issue),
   `faber explain <CODE>` for a diagnostic, and `faber run f.fab`. `faber parse` / `faber lex` print the AST/tokens.
