---
name: "faber"
description: "Write, check, build, run, review, or translate Faber (.fab) source: locale packs and the en/la reader surfaces, grammar and idioms, corpus exempla, and compiler validation recipes (check, stdin checking, parse-only, build, run, format, convert). Use for any question about whether Faber syntax is legal or how to express something in Faber. For compiler internals, corpus management, or other changes to the Faber repo itself, use the faberlang skill instead."
---

# Faber (language usage)

Use this skill whenever the task involves Faber *source*: writing programs,
checking whether syntax parses or validates, building and running packages,
reviewing `.fab` code, or translating between reader locales. The companion
`faberlang` skill covers changing the repo itself (compiler, CLI, corpus,
runtimes, Norma, grammar); when a session does both, keep the modes separate.

Faber 1.x is experimental and unstable: the language changes between minor
releases, so verify any construct with `faber check` against the repo you have.

This is a usage skill, not a specification. Never answer from memory when a
construct, spelling, or flag can be checked against the live tree. When this
skill and the repo disagree, the live tree wins — inspect before answering.

## The two rules that prevent most bad Faber

**1. One locale per file — locale surfaces are sealed against each other.**

Faber has one grammar with multiple reader-localized code surfaces (`en`,
`la`, `zh-Hans`, …). A locale pack maps stable Latin-keyed identities to that
locale's spellings (`fn` is the en spelling of the identity `functio`, `string`
of `textus`), and **only the selected
pack's keyword, value, method and type spellings are accepted** (a Latin type
name like `textus` in an en file is `PARSE040 wrong_locale_type`; write
`string`). English and Latin are therefore not interchangeable
spellings of one language — `fn` and `functio` are as mutually foreign as
English and Chinese keywords would be. Mixing them in one file is the single
most common LLM failure: seeing `fn` in one example and `functio` in another
does not mean they combine. Pick one locale per file (default `en`; a Latin
file must declare `locale = "la"`), declare it in frontmatter, and use only that
surface throughout — keywords, types, intrinsics, and translated library
members alike.

**2. Verify with the compiler; do not trial-and-error past a diagnostic.**

Faber looks like other languages often enough to trigger wrong priors, and it
is mechanically different often enough that those priors fail. The escape is
always the same loop: check the construct in `grammar/EBNF.md` (generated
from `grammar/sources/source.fg`), mirror a `corpus/` exemplar, and
confirm with `faber check`. Diagnostics teach
the actual error; guessing the next spelling teaches nothing.

## Common wrong guesses (mainstream-language priors)

These are wrong, and each fails at parse or check. Examples are `en` (the
product default); `la` spellings differ (`print` is `nota`, `fn` is `functio`).

| If you reached for… | Faber is… |
| ------------------ | --------- |
| `x = 5`, `x += 1`, `x++` | the only runtime store is `←` (`x ← x + 1`); postfix `x ↑` / `x ↓` are statements. `=` exists only for compile-time-known values: field defaults, `type` aliases, enum members, annotation metadata, module constants. A bare `x = 6` statement reports only `PARSE030 expected_expression` |
| `out[:] = x`, `memcpy`, `out ← x` into a `mut` tensor param | `←` only rebinds a name; writing into existing tensor storage is the copy-into statement `out ⇇ x` (rules in `references/grammar.md`, Copy-Into; a whole-tensor `←` into a `mut` tensor param is `SEM057 mut_tensor_param_rebind`, a provisional rule under reconsideration). `sink ⇇ "text"` (the old callable sink) is `SEM012 transfer_sink_retired`: output is `print x` |
| `bytes`, `byte` (la `octeti`, `octetus`) | not Faber types, just unknown names: write `list<b8>` for a byte string (la `lista<b8>`), `b8` for a raw byte, `u8` for a numeric byte |
| `print(x)` / `println(x)` | line-level statement: `print x` (en) / `nota x` (la), never a call (`SEM001 scribe_callee`) |
| `==`, `!=`, `!x`, `&&`, `\|\|`, `->`, `<-` | the glyph and word forms: equality is `≡` (negation `≢`), logic is `and` / `or` / `not` (la `et` / `aut` / `non`), a return type follows `→`, a store is `←`. `==`, `!=`, `!x` and `<-` report `PARSE030 expected_expression`; `&&` and `\|\|` report `LEX004 unexpected_character`; `fn f(int a) -> int { … }` leaves the function without a body (`PARSE012`) |
| `x qua T`, `x innatum T`, `novum T { … }`, `x ⊕ 2`, `x ⊛ 3` | old cast, construction and operator spellings that no longer exist. The three words are ordinary identifiers now, so the error appears as unknown names (`SEM001`), not as a syntax error; `⊕` and `⊛` report `LEX004 unexpected_character`. Convert with `x ↦ T`, construct with `T { field = expr }` |
| `name: T` declarations and params | type-first: `string name`, `fn greet(string name) → string` |
| `const x ← 5`, `var x ← 5`, `const T x ← e` | the run-time binding words are `let` (fixed name, write-once: `let int x ← 5`, inferred `let _ x ← 5`; may be declared without an initializer and assigned once), `mut` (writable: `mut int n ← 0`, inferred `mut _ n ← 0`) and `ref` (re-pointable read-only name over a place; reference types only). `const` is compile-time only and goes with `=`: `const int N = 5`. A run-time `const T x ← e` is `PARSE010 runtime_const_retired`, `var` is `PARSE010 var_retired`, and every declaration writes its type slot (`_` infers): `let x ← 5`, `mut x`, `ref x` and `const x` are `PARSE010 binding_type_slot_required` (write `let _ x ← 5`); binders (`for … let j`) take no type slot. la: `sit` / `varia` / `de` / `fixum` |
| `T?` optional types | `T ∪ none` in en (la `T ∪ nihil`); the null value is `null` (la `nulla`). Never mixed in one file. `T ∪ null` in a type is `PARSE040 null_value_in_type_position` |
| `{ key: value }` anonymous objects | JSON document `{ "key": constant }` (typed `json`) or `Type { field = expr }`; `P { x: 1 }` and `{ key = expr }` are parse errors |
| `fn main()`, a top-level entry function | the entry is the block `main { … }` (la `incipit`); `fn main()` checks but run reports no entry |
| mixing `fn` with `functio` (any locale blend) | one locale pack per file, sealed (rule 1): keywords, values and methods are sealed (the Latin `functio`, `verum`, `xs.longitudo()` all fail in an en file; en spells them `fn`, `true`, `xs.length()`); type names too (see the next row) |
| Latin type names (`numerus`, `textus`, `lista<…>`, `nihil`) in an `en` file, or any non-Latin file | Latin is a locale pack like any other, with no fallback: a Latin type word in a non-Latin file is `PARSE040 wrong_locale_type`, and `faber check` names the file locale's spelling (en `int`, `string`, `list<…>`, `none`). Write that locale's own spelling. A user-declared name that looks like a Latin type word is not rejected |
| expecting `verum` / `falsum` / `nihil` / `iuncta` from a program's output, or the Latin words in an `en` golden | printed words follow the CODE locale: an `en` program prints `true` / `false` / `none` / `tuple`, a `la` program `verum` / `falsum` / `nihil` / `iuncta`, any other pack its own words (a pack that lacks a word falls back to the Latin word for that token only). Every target agrees (runner, rust, ts, go, swift, python, sexp, wasm, llvm); Haskell keeps its native `True`/`False`/`Nothing`. Write `.expected` files in the locale of the source |
| try/catch | `do { … } catch err { … }` (la `fac { … } cape err { … }`); `tempta`/`demum` are retracted |
| `c ? a : b` / `c yields a else b` ternary (la `sic … secus`) | `c ✓ a ✗ b`; both old forms report a migration diagnostic (`PARSE030`) |
| `for (i = 0; …)`, `for x in xs` | `for from xs let x { … }`; `for range 0‥5 let i { … }` (en; la `itera ex xs sit x`); the binder `let`/`mut` is mandatory (`ref` is refused there: `PARSE010 ref_binder_refused`; the old `const` binder is `PARSE010 binder_fixum_retired`); no parentheses around conditions: `if c { } elif c { } else { }` |
| `if c ∴ return x` one-liner | `if c then return x` (la `si c ergo redde x`); `∴` is only the compact-closure joint (`PARSE026` elsewhere) |
| `class P { int x }` | every `class` field (la `genus`) carries a modifier: `class P { let int x; mut int y; const int Z = 1 }` (la `sit`/`varia`/`fixum`); `let` is set once in the construction literal, `mut` is reassignable, and `const T X = e` is the type-level compile-time constant (read as `P.Z`; a store is `SEM020 assignment_to_const_member`, a missing `=` is `PARSE010 const_member_requires_initializer`). A bare field is `PARSE010 field_modifier_missing`; `ref` on a field is `PARSE010 ref_field_refused`; the old `var` field word is `var_retired`; `static`/`generis` members are `PARSE010 static_member_retired`. Construct with `P { x = 1 }`; a missing required field is `SEM004` |
| `import … "norma:solum"`, `norma:consolum`, `norma:hal/*` | module names are the English source names (`norma:fs`, `norma:text`, `norma:json`, `norma:console`, `norma:process`, `norma:time`, …; the list is `lib/norma/src/*.fab`) in every locale; Latin and `hal` names are `PKG001 unknown_library_module`. The `privata` import marker is gone (`PARSE050`) |
| `const N ← 5` at module level, `static` | module constant is `const int N = 5` (la `fixum`), `=` with a compile-time value; `←` there is `SEM062 top_level_binding`; `static`/`generis` is retired everywhere (module level `PARSE010 static_decl_retired`, class level `static_member_retired`; a type-level constant is a class member `const T X = e`) |
| a bodiless `fn f(string t) → string` (declare now, implement later) | `PARSE012 function_body_absent`, unless the top-level function or genus method carries `@ unstable "reason"` (la `@ nondum`): it may then omit its block, with no target it is unavailable everywhere, declaring and exporting it is fine, and any use (call, call through an imported namespace or name, or taken as a value; for a method, a call through an instance, `self`, or an imported genus) is `SEM017 nondum_function_for_target`, while the genus's other methods keep working. Never a body that panics as a stub |
| `"${x}"` interpolation, `"a" + 1` | no interpolation: format with `"n=§"(3)`; `+` joins strings only; length is a method, `xs.length()` (`xs.length` is `SEM004 length_is_method`) |
| `"she said \"hi\""` (escaping quotes inside a `"…"` string) | the guillemet block form `«she said "hi"»` is the same `string` (`textus`) type as `"…"`, compares equal to it, and needs no escape for `"` or `'`; it takes template holes (`«hello, §»("me")`) and may span lines, does not nest (`«a «b» c»` is `LEX004 unexpected_character`), and writes a literal `»` inside as `\»`. Escaping a quote inside a `"…"` literal is `WARN034 prefer_guillemet_for_quote`; the teaching exemplar is `corpus/literalia/guillemet-quotes.fab` |
| param defaults `f(int x = 3)` | `int x optional` (nullable inside the body) or a default `int x coalesce 3`; `=` and `←` there are `PARSE001` |
| calling a `⇥` function without handling | outside `main`: `⊥ default`, a local `do/catch`, or an enclosing `⇥ E` function (`SEM011 failable_call_requires_handler`; conversions: `failable_conversio_requires_handler`) |
| `a.b` on `T ∪ none` | `a?.b` or `a!.b` (`SEM004 optional_receiver_requires_chain`); a nullable passed to a non-null slot checks and traps at run |
| mutating a `let` list/set | `SEM020` (`xs.append(3)`, `xs.sort()` on a `let` receiver); declare `mut` |
| `mut b ← a` copies, or `let` shares read-only | unmarked `b ← c` shares (a container or class value aliases); the markers sit on the value side: `mut b ← copy a` duplicates deeply, `mut b ← own a` takes it over. Read-only is deep: a `let` (or `ref`, or unmarked parameter) container or genus into a `mut` cell is `SEM065 const_shared_into_var` (spell `copy` or `own`). `ref T x ← place` is a re-pointable read-only name; `ref`/`mut` may hold `T ∪ none` when the non-none member is a reference type (never over a scalar: `SEM008 scalar_access_marker`; `corpus/mutabilitas/ref-optional-cursor.fab`). `Genus { f = v } from a` takes values (omitted collection fields are duplicated). `corpus/mutabilitas/{copy-bind-deep,const-share-into-var-reject,alias-on-bind}.fab` |
| `xs[i]` returns null / throws, or `xs.get(i)` panics | three reads: `xs[i]` out of range dies (the run ends); `xs.get(i)` gives `T ∪ none`; `xs.at(i)` reports on the error channel (`do { } catch err { }`, la `fac { } cape err { }`, or `⊥ default`). Text indexes by Unicode character, not byte; a range index `xs[a‥b]` out of range dies (`corpus/lista/reads-three-ways.fab`) |
| `"hello".slice(2, 99)` trims to `llo`, or `t.length()` is a tensor's rank, or `v.length()` a vector's norm | `slice` reports an end past the length, a negative bound or a start past the end on the error channel, on text as on lists (`s.slice(a, b) ⊥ ""`, or `do { } catch err { }`), and an unhandled call outside `main` is `SEM011 failable_call_requires_handler`. `length()` is the element count on every collection, a `tf32[2, 3]` has 6; `t.rank()` is the number of extents and `t.norm()` / `v.norm()` the Euclidean norm (`corpus/textus/slice-window.fab`, `corpus/tensor/rank-norm.fab`) |
| `xs.sorted(key)`, `sort(xs)`, `for from sort via asc from xs …` | the sort is an expression: `sort via asc from xs` / `sort via desc from xs` (la `ordinata via ascendens ex xs`), keyed by a body `sort via asc from xs let r { return r.k }`; stable, builds a new list (a map: `from` sorts values, `ref` sorts keys). The keyed `.sorted(key)` method is `SEM011 sorted_key_retired` (keyless `.sorted()` and in-place `xs.sort(key)` remain). A bare sort or reduce as a `for` source is `PARSE030 itera_source_sort_reduce_unbound`: bind it to a name first (`corpus/sort/`, `corpus/itera/sort-source-reject-en.fab`) |
| appending inside `for from xs` | a method that changes the shape of the collection being walked is `SEM011 loop_collection_shape_change`; a bracket store that grows a map in its own loop is unchecked and traps `map changed size during iteration` (`corpus/itera/loop-shape-reject.fab`, `corpus/tabula/loop-store-adds-key-trap.fab`) |
| `match` on numbers or open types | `match` (la `discerne`) matches closed sets only (unions, enums, variants): `SEM010 discerne_pattern_not_in_closed_set`; use `switch` (la `elige`) for open scrutinees |
| camelCase everything | snake_case functions, fields and locals; PascalCase types, unions and variants (Norma and the sibling libraries: thousands of snake_case names, no camelCase); not compiler-enforced |
| Latin-flavored pseudo-code | only spellings that exist in the selected pack and `EBNF.md` |
| `/` truncates toward zero (C, Rust, Go) | integer `/` **floors**: `-7 / 2` is `-4`; `7 ÷ 2` is `3.5` |
| `%` takes the dividend's sign | `%` takes the **divisor's** sign: `-7 % 2` is `1`, `7 % -2` is `-1` |
| integer `/` gives a float | `/` on integers is an integer; true division is `÷` (`f32` when the widest operand is 8/16-bit, else `f64`) |
| `int → float` is implicit | crossing integer / decimal / float needs `↦` (`let f64 f ← n` is SEM010; untyped constants are exempt). Comparisons across families are exact and need no `↦` |
| `x ↦ int` rounds or truncates a float | a float or `d64` goes to an integer only with a rounding choice: `x ↦ int via Floor` / `Ceil` / `HalfEven` (nearest, ties to even) / `Trunc`; a bare `x ↦ int` is `SEM016 float_to_integer_requires_rounding`. `⊥ default` and `⇥` still apply after the `via` (`corpus/conversio/float-to-int-via.fab`) |
| `u8 + u8` wraps or traps at the `+` | arithmetic is exact (`u8 + u8` is `u16`, value 400); the slot checks at the store: a trapping slot traps (`400 does not fit in u8`), `wrapping<W>` reduces, `saturating<W>` clamps |
| `u8<0‥8>` or `saturating<u8, 0…>` is a range type | a range needs its policy word and both ends: `saturating<u8, 0‥8>` (`‥` half open, `…` inclusive, `∞`/`-∞` for no bound on that side); `u8<0‥8>` is `ranged_policy_word_required` and `0…` is `range_bound_required`. The range applies at the store only (saturating clamps into it, trapping traps, wrapping reduces modulo `hi - lo` offset by `lo`); integer widths only, runner and Rust only, a `size` end is not built |
| `saturating` clamps every step | it clamps **once, at the store**: `x + 200 - 100` with `x = 250` is `255` (exact 350 clamped), not `155` |
| shifts mask the count | on numbers unmasked: `x ⇐ n` is `x·2ⁿ`, `x ⇒ n` is `floor(x / 2ⁿ)`; `¬x` is `-x - 1` (`¬250` is `-251`); fixed-width complement is `wrapping<W>`. Raw bits `b8`/`b16`/`b32`/`b64` are not numbers and act at their own width every operation: `(b ⇐ 4)` on a `b8` drops the high bits, `¬` flips exactly N bits, a shift by N or more gives zeros, a negative count is an error (`0xF0 ⇐ 4` is `0x00`, `¬0xF0` is `0x0f`) |
| `u64 + i8` widens somewhere | compile error (`u64_signed_arithmetic_requires_conversion`); convert one side with `↦`. A constant is exempt (`x - 1` on `u64` is fine) |
| `↦ saturating<u8>` can fail / take `⊥` | `↦` into `wrapping<W>` reduces and into `saturating<W>` clamps; neither fails or takes `⊥` |
| `int<i32>` / `int<u32>` / `float<f32>` sized wrappers (la `numerus<i32>`, `fractus<f32>`) | retired (`PARSE040 numeric_wrapper_retired`): sized numerics are bare markers (`i32`, `u32`, `f32`); bare `int`/`float` (la `numerus`/`fractus`) are `i64`/`f64`. Policy words keep their argument: `wrapping<u32>` (la `modulus<u32>`) |
| `d32` decimals | `d32` is removed; `d64` is the decimal |
| `inf` is a keyword or the float infinity | `inf` is the opt-in **unbounded integer type** (a three-letter width marker, no keyword); `∞` is the float infinity and prints as `inf`; `let inf x ← ∞` is a compile error |
| `u64 - u64` is exact or becomes `inf` | it stays `i64` (and traps past ±2⁶³); opt in with `(a ↦ inf) - b` or declare `a` as `inf` |
| a big literal is a big integer | a literal past `u64` is legal only in an `inf` slot or beside an `inf` operand (`let _ x ← 18446744073709551616` is `numerus_literal_out_of_range`); `inf` is never the default |

When a diagnostic surprises you, the cause is usually one of these rows —
check the table before re-guessing.

## Verifying source (quick reference)

The stable core:

```bash
faber check <file-or-package>        # full check; prints ok or diagnostics; exit 65 on error
cat file.fab | faber check -         # stdin, import-free sources only; needs a frontmatter `locale` (see below)
faber run <file-or-package>          # interpreted execution (MIR runner, always; `faber run -` is not supported)
faber -c '+++
locale = "en"
+++
main { print 1 }'                    # one-liner through the runner (reads Latin unless the source starts with a frontmatter locale)
faber test <file-or-package>         # proba cases on the runner
faber parse <file>                   # parse-only AST JSON (`faber lex|hir|mir|emit` likewise)
faber init <dir>                     # scaffold: faber.toml ([package], [paths], [build] kind = "bin") + src/main.fab
faber doctor                         # resolved library home and per-library availability (`norma:*` is embedded in the binary)
faber explain --list                 # glyph / keyword / grammar-term reference
```

Exit statuses of the tool (not of the program it runs; a program's own exit
code passes through): `0` success (warnings alone are success), `65` the source
has error diagnostics (`check`, `build`, `run`, `test`, `emit`, `format`; a
warning promoted by `--deny` / `--deny-warnings` counts), `64` bad
command-line arguments (including an unknown `--deny` code, and a
`faber corpus --target` the suite has no recipe for), `66` an input file, path
or stdin that cannot be read, `69` a feature, runner or target this binary was
not built with, `70` an internal serialization or render failure, `73` an
output file or directory that cannot be written, `78` a bad `faber.toml` key,
a locale pack that fails to load, or a malformed `corpus.toml` / `case.toml`,
`1` only a failure with no known class. Outcomes keep their own status:
failing `faber test` cases and `faber format --check` drift exit `1`;
`faber update --check` exits `0` current, `1` update available, `2` lookup
error. Branch on the status, not on stderr text.

Build from the repo, not from an installed binary, when behavior matters:
installed `faber` binaries go stale and lag the grammar, which shows up as false
reds. From the repo root: `cargo build -p faber` then `target/debug/faber ...`, or
`cargo run -p faber -- check <path>`.

### Reading a diagnostic

Default `faber check` prints expanded phase-aware diagnostics, including the
rendered message and help. Use `--no-diagnostics` for the former compact code
and source-line form:

```bash
faber check <path>                                  # message + help + phase/span
faber check --no-diagnostics <path>                 # compact code + source line
faber explain SEM010                                  # catalog entry by code
faber explain SEM010.initializer_annotation_mismatch  # catalog entry by code.issue
faber check --diagnostics-locale la <path>           # message language
```

`--diagnostics-locale` applies to the default rendered diagnostics too.
Embedded diagnostic locales: `ar`, `en`, `hi`, `la`, `th-TH`, `vi`,
`zh-Hans`, `zh-Hant`, default `en`. The flag takes a locale *name*, not a
pack path. Build, run, test, emit, format, and convert also show message/help
by default; each accepts `--no-diagnostics` for compact output.

- `faber verify` prints the expanded record without message or help (it passes
  no pack); do not read its silence as a missing template.
- Never infer a fix from the bare code. Read the default message and help, or
  use `faber explain`, before editing source.

## Locale First

Before diagnosing syntax, generating source, or translating an exemplar:

1. Inspect line 1 for `+++` TOML frontmatter and read `locale = "…"`.
2. Inspect the nearest `faber.toml`: canonical `[locale]`, then legacy `[reader]`.
3. Check the command for explicit `--locale`, which is authoritative.
4. Open the selected `pack.toml` and map suspect spellings through
   `[keywords]`, `[types]`, `[intrinsics]`, `[[library_members]]`.
5. Only after resolving the locale, consult `EBNF.md` and live parser code to
   decide whether the grammar itself changed.

The product default for an untagged file or package is `en` (a Latin file must
say `locale = "la"` in frontmatter or the package manifest). Two routes
diverge: `faber check -` (stdin) and `faber -c` read **Latin** when there is no
frontmatter, so give stdin sources a `+++ locale = "en" +++` header (`format --stdin` and `convert --stdin` read `en`). Converting between locale
surfaces is `faber convert <path> --to <LOCALE>` — not `format` (formatting
reflows; converting translates the surface). Code locale and diagnostics
locale are separate; error-message language is not evidence of source locale.

Read `references/locales.md` for pack structure, route-specific precedence,
and the syntax-or-locale diagnostic procedure.

### Convert complete source units

`faber convert` translates a complete source unit; it does not accept a
statement or local-declaration fragment and does not invent a hidden `main`.
Use a wrapper when testing a local type form:

```fab
main {
    let list<u32> b ← empty
    let lu32 c ← empty
}
```

Use these live routes:

```bash
printf 'main { print "ok" }\n' | faber convert --stdin --to la
faber convert source.fab --to la --stdout
faber convert source.fab --from en --to la --stdout  # repair stale metadata
faber convert src/ --to zh-Hans --check
```

For `convert`, source selection is `--from` → that file's frontmatter → the
owning package manifest → product-default `en`; `--to` is one target pack for
the entire invocation. Successful output replaces or inserts its target
frontmatter `locale` key while preserving unrelated metadata. For `format`,
source selection is file frontmatter → package manifest → `en`, and the output
stays on that source surface. Conversion and formatting both report structured
diagnostics with the real file line when frontmatter is present.

## Required Workflow

1. Resolve the active code locale (CLI `--locale` → file frontmatter →
   manifest `[locale]` → product default `en`). State it before judging or
   emitting syntax.
2. Read the relevant grammar section in `grammar/EBNF.md` — quote it, don't
   recall it.
3. Read at least one nearby exemplar under `corpus/` (find it by its
   frontmatter `explains`), including its frontmatter locale, and mirror its
   keyword order and spelling.
4. For `norma:*`, inspect `lib/norma/src/`; for any other library, inspect that
   library's own `src/` in the checkout you have.
5. Verify with the compiler (`faber check`) whenever legality matters.
6. Only then answer, review, or generate.

## Canon

In order:

1. `grammar/EBNF.md` — formal grammar (generated; source `grammar/sources/source.fg` + `prose.en.md`)
2. `corpus/` — reference programs
3. `crates/faber/` — CLI behavior (`--help` is live truth)
4. `lib/norma/src/` — Norma library APIs
5. `faber targets` — target/package/run capability

Archived or retired sources are never evidence that a construct exists.
Example repositories demonstrate but do not legislate. A tree-sitter grammar is
highlighting only, never grammar or diagnostics truth.

## More documentation

The public website https://faberlang.dev holds the language guide, reference, cheat sheet, targets, toolchain notes and release notes. https://faberlang.dev/llms.txt is an index written for language models. If the site and this repository disagree, the repository is the source of truth.

## References

- `references/grammar.md` — compact construct map
- `references/idioms.md` — writing natural Faber: style, naming, semantic tone, and the selective beyond-core construct highlights (including library/device seams)
- `references/operators.md` — operator table, precedence, tensor glyphs, the error-conversion seam law
- `references/locales.md` — locale packs, precedence, pack schema, diagnosis
- `references/examples.md` — corpus navigation and high-value exempla
- `canonical-faber/SKILL.md` — style audit of existing `.fab` source

Read only the reference needed for the task, then inspect live repo paths.

## Hard Rules

Names use stable Latin registry keys when describing a language identity (for
example `functio`, which en spells `fn`); render spellings through the active
locale pack.

- Never invent keywords, modifiers, or annotations because they sound Latin enough. `cum` is not a keyword in any pack (it checks as an identifier); it is a repo convention never to introduce it as a Latin particle.
- Never blend locale surfaces in one file; never classify a spelling as old/new syntax before resolving locale.
- Type-first syntax `T name`; never `name: T`.
- Glyph law: arrows mean runtime effects — `←` runtime assign, `↤` convert-assign, `↦` conversion, `→` return type / function type only, `⇥` error channel. `=` and `:` mean compile-time facts: `=` sets a compile-time-known value (field defaults, `type` aliases, enum members, annotations, module constants); `:` labels (e.g. `ratio` type arguments). `=` is legal but never a runtime assignment.
- `⇥ E` names an error **type** only; it never carries a fallback value. A fallback on a conversion or a failable call is `⊥ expr` (`↦ T ⊥ 0`, `risum() ⊥ 0`); `⇥ value` is `PARSE030 recovery_uses_exit_arrow`.
- Ternary is `c ✓ a ✗ b`; `?:` and `c yields a else b` (la `sic…secus`) are removed. `¶` is the display-format operator (`value ¶ "spec"`), same glyph in every locale, no word twin.
- Output is a line-level statement (`print` en / `nota` la), not a function call.
- A `string` literal whose payload contains `"` or `'` is written with `«…»`: same `textus` type, no escapes, template holes and multi-line allowed, a literal `»` inside is `\»`, no nesting. A `"…"` literal with an escaped quote is `WARN034 prefer_guillemet_for_quote` (exemplar `corpus/literalia/guillemet-quotes.fab`).
- No `Type?`; nullability is `T ∪ none` (en) / `T ∪ nihil` (la); the null value is `null` / `nulla`; optional declaration slots are `optional` (la `sponte`) after the name.
- `unknown` (la `ignotum`) is the unknown/escape type, not nullability.
- Empty collections need an explicit type: `let list<int> xs ← empty` (la `vacua`); `let _ xs ← empty` is `SEM014`, and an untyped `[]` checks but fails at run.
- One-statement bodies use `then` (la `ergo`): `if c then return x` (la `si c ergo redde x`). `∴` is the compact-closure joint only (`PARSE026` elsewhere).
- Stroustrup braces; flat `norma:*` imports with English module names (`norma:fs`; `norma:hal/*` and Latin module names are `PKG001`).
- There is no `@ externa` / `@ subsidia` (`SEM018`); native routes are `call 'mod:op' (args) ↦ T` (la `ad`), and library source (Norma and the sibling libraries) uses none of them.
- Every `class` field needs `let`/`mut`/`const` (la `sit`/`varia`/`fixum`): `let T x` and `mut T x` are per-instance, `const T X = e` is the type-level compile-time constant (`static`/`generis` is retired); a module constant is `const T X = e`.
- Sized numerics are bare width markers (`i32`, `u8`, `f32`, `d64`, `inf`); wrapped `int<W>` forms (la `numerus<W>`) are `PARSE040`. Policy words keep their argument (`trapping<W>`, `wrapping<W>`, `saturating<W>`).
- Separate grammar facts from implementation facts; if a construct exists only
  in archived sources or stale docs, say so and verify live support first.
  Known checker/runner gaps are real: a program can pass `faber check` and still
  fail on the MIR runner (`x ∈ lo‥hi` range membership, string `≈`, an untyped `[]`).

Operator table, precedence, tensor glyphs, and the error-conversion seam law
(`@ conversion`) live in `references/operators.md`.

## Response Style

When explaining Faber, state the active code locale first, then show that
locale's accepted surface; cite the authoritative paths used; say whether a
claim came from grammar, exempla, stdlib, or implementation.

When writing Faber, determine the locale before choosing any spelling; mirror
a same-locale corpus exemplar; for an untagged product package use `en`. Keep
examples small and canonical unless asked otherwise.

## Fast Checks

- Did I state the active code locale before judging or emitting syntax?
- Is every spelling I emitted from one pack, only that pack?
- Did I quote `EBNF.md` and open a real exemplar rather than recall both?
- Did I verify legality with `faber check` when it matters?
- If a diagnostic was unclear, did I read its message and help (shown by default; or
  `faber explain`) instead of guessing from the code?
- If a library/target/host claim: did I inspect that repo's live source?
- Did I avoid filling gaps with confident invention?

If any answer is no, inspect first.
