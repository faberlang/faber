# radix-hir-c

Novelty C target: one C translation unit from checked HIR. CLI target `c`,
Cargo feature `hir-c`, enabled by default in `faber` (outside
`full-targets`).

This is a **spike** growing by units: exact integer arithmetic, control flow,
functions, text, lists and genera so far. Everything else fails closed
with `unsupported on target c: <construct>`, never an approximation. The leaf
never invokes a C compiler (the `no_toolchain_command_new` ratchet forbids
it); `scripta/diffcheck` builds the emitted program and runs the built binary, and
`scripta/tally` measures corpus breadth: the first rejection per exemplar, and
with `--run` whether each exemplar that emits builds and matches its expected
output.

The runtime is **`runtime/c/faber.h`**, header-only (`static inline`), the C
sibling of `runtime/rust`: exact arithmetic and traps, the 64-bit fast path,
the arena, text, and the list families. The `faber` binary embeds it like the
other in-tree runtimes. A generated program that uses it starts with
`#include "faber.h"` and builds with `-I <runtime>/c`; one that needs nothing
from it includes nothing. The emitter writes only what is specific to the
program: genus structs and their show functions, functions, and `main`.
`runtime/c/tests/faber_test.c` checks the runtime on its own.

The output needs GCC or Clang: it uses `__int128`, the `__builtin_*_overflow`
family and statement expressions.

Memory is an arena: chunks from `malloc`, bump allocation aligned to
`max_align_t`, nothing freed while the program runs. `main` owns the root
arena and every function takes it as the hidden first parameter
`faber_arena *faber_a`; there is no global current arena. A program that never
allocates keeps signatures without it (emission runs a second time, with the
arena threaded through, only when the first pass allocated).

## The rule this target exists to keep

C is the permissive compiler, so the emitter is the strict one: **no emitted
program may reach undefined behavior.** `scripta/diffcheck` builds every run
fixture with `-std=c11 -pedantic -Wall -Wextra -Werror` and
UndefinedBehaviorSanitizer and AddressSanitizer with recovery off (leak
detection off: the arena frees nothing by design), and fails on any sanitizer
report. It checks the runtime first: `faber.h` compiles on its own and
`tests/faber_test.c` passes.

| Faber rule | C hazard | This target |
| --- | --- | --- |
| integer arithmetic is exact until the store | signed overflow is UB | never C `int64_t` arithmetic: `faber_add` / `faber_sub` / `faber_mul` / `faber_div` / `faber_rem` compute in `faber_i128`, check the runner's cap `[-2^63, 2^64-1]` after every operation (`__builtin_mul_overflow` for products, which can exceed 128 bits), and `faber_store_i64` narrows once, at the store |
| `/` and `%` floor; division by zero traps | C truncates; division by zero is UB | floor in `faber_div`, remainder `a - b * floor(a / b)`, zero divisor traps |
| a trap is `error: <reason>`, exit 1 | — | `faber_trap` / `faber_store_i64` write the runner's text to stderr and `exit(1)`; stdout already written stays written |
| `bN` shift by N or more gives zero | shift by ≥ width is UB | shift operators rejected |
| `↦` reinterprets bits | pointer punning breaks strict aliasing | conversions rejected (the exact form is `memcpy`) |
| text is a counted UTF-8 value | `%s` stops at NUL | a text literal containing NUL is rejected |

## Supported

| Faber feature | C |
| --- | --- |
| `int`, `string` and `bool` locals, `var` and `const` | `int64_t`, `const char *`, `bool` (`<stdbool.h>`), `const` when immutable |
| functions with `int`, `string` and `bool` parameters and results, or no result; recursion | `static` C functions named `f_<name>` with prototypes first, so any call order works; parameters `p_<name>`; a result returns through its `return` store. A function nothing else calls (a library module defines many) is `static __attribute__((unused))`, so `-Wunused-function` stays quiet under `-Werror` |
| calls | C leaves argument and operand order unspecified, so when two or more operands of one evaluation include a call, they are bound to temporaries in source order inside a GNU statement expression; an exact argument is checked like a store (`argument`) |
| module constants | `static const` `k_<name>` with the folded value; a list, a genus, or any value that is not a literal is a plain `static` set first in `main`, in source order, because the arena it allocates in belongs to `main` |
| type-level class constants `const T X = e` | a file-scope `static const` named `k_<Genus>_<NAME>` with its folded value, read through the type name (`Color.RUBER`); never an instance member, so a construction does not set it. It carries `__attribute__((unused))` so a constant no code reads stays legal under `-Werror` |
| integer arithmetic `+ - * / %` and negation over variables | exact helper calls, narrowed by `faber_store_i64(value, "assignment to `n`")` at the store; the store text is the runner's. Where typecheck put no store node (its range rule proved the result fits), `faber_fit_i64` narrows instead |
| the 64-bit fast path | `/` and `%` of an `int64_t` by a constant other than 0 and -1 stay `int64_t` (`faber_fdiv_i64`, `faber_fmod_i64`). A pure `+ - *` tree over `int64_t` values ending in a store runs on `int64_t` with `__builtin_*_overflow` and one flag; on overflow the same expression is re-evaluated through the exact helpers, so the value, trap and trap text are the exact model's. An expression with a call never takes it, because the fallback re-evaluates |
| integer arithmetic over constants | folded at emit time with the same rules; `INT64_C(n)`, `-INT64_C(n)`, `INT64_MIN`, or an exact `UINT64_C` value above `i64` |
| bitwise `∧ ∨ ⊻ ¬` and the shifts `⇐ ⇒` | exact helpers (`faber_bit_and`, `faber_bit_or`, `faber_bit_xor`, `faber_bit_not`, `faber_shl`, `faber_shr`) inside the cap, because two's complement on the exact window can leave it (`i64::MIN ^ u64::MAX`); a runtime negative count traps `negative shift count`, a left shift past `2^64` `numerus overflow`, a right shift of 127 or more fills with the sign. A constant pair folds at emit time; the frontend folds the constant forms first |
| integer methods `abs`, `sign`, `minimum`, `maximum`, `power` | `faber_abs`, `faber_sign`, `faber_min_i128`, `faber_max_i128`, `faber_pow`. `abs` of `i64::MIN` reaches `2^63` (still in the cap); `power` is repeated squaring that traps `numerus overflow`, and a negative exponent aborts `numerus potentia failed: negative exponent`, as the runner's `faber::exact::pow` does. `float.power` is libm `pow` (`faber_pow_f64`) |
| integer comparisons `≺ ≻ ≤ ≥ ≡ ≢` | C comparisons by exact value (an `int64_t` widens to `faber_i128` against an exact operand) |
| `and`, `or`, `not`, `true`, `false` | `&&`, `\|\|`, `!`, `(1)`, `(0)` |
| `if` / `else`, `while`, `break`, `continue`, nested blocks, `return` from `main` | the same C statements |
| `print` of integers, exact intermediates, text and booleans | one `printf` per `print`; the format holds only `%" PRId64 "`, `%s`, spaces and `\n`; an unstored exact value prints through `faber_i128_text`; a boolean prints the module's display tokens (`true` / `false` in `en`, `verum` / `falsum` in `la`) |
| text literals | C string literal of the UTF-8 bytes: non-ASCII and control bytes as octal escapes, `?` escaped so no trigraph forms |
| `ascii`, `ascii<size N>`, `string<size N>` | the same counted UTF-8 value `string` is (`ascii` is an ASCII-only text with no capacity). The typechecker proves a literal fits a bounded capacity; a run-time `↦ string<size N>` checks it in code points (`faber_bounded_text`, trap `textus to bounded textus conversion failed`), and the `ascii<size N> ↦ ascii<size M>` D7 resize checks the new capacity (`ascii to bounded ascii conversion failed`). `int ↦ ascii<size N> via Hex|Bin|Oct` is the zero-padded fixed-width radix pack (`faber_try_ascii_pack`; a constant source infers `ascii<_>`'s capacity, and a negative value or a width that does not fit takes `⊥` or ends the program). An argument of one of these prints on its own line, as the runner does |
| text `+` and templates `"…§…"(args)` | `faber_concat` and one `faber_join` per template, in the arena. `§` shows the next argument, `§N` argument N; the split runs on the raw template before decoding, and each argument that is not a constant or a plain name is evaluated once, in order, before the join. Template escapes decode like ordinary strings (the runner prints them undecoded) |
| `list<T>` (including `class` genera, bounded and unbounded) | a pointer to an arena-allocated `faber_list_<elem>`; the genus family stores `g_<Name> *` elements, so reads and binds preserve object identity (R1). Literals, `empty`, `length`, `is_empty`, `append`, checked index reads/writes and `for from` iteration work, including lists stored in genus fields. Unsupported operations such as genus-list membership, optional indexing, sorting, display and copy remain named rejections; unsupported mappings fail closed |
| `x ∈ xs` / `x ∉ xs` over a list | the list family's `contains` (`faber_list_<e>_contains`), one `bool`; the element class picks the family, so text, integer and boolean lists all work |
| `set<T>` (la `copia`) | the same list storage (`faber_list_<elem> *`) under the set contract: `add` is unique (`faber_set_<e>_add`), `delete` removes by value (`faber_set_<e>_delete`), `length` / `is_empty` read the count, and display is the list shape. The algebra `union`, `intersection`, `difference`, `difference_symmetric`, `is_subset`, `is_superset` returns new sets. `list ↦ set` deduplicates (`faber_set_<e>_of`) and `set ↦ list` materializes (`faber_list_<e>_copy`). Element order is unspecified (D19.2) |
| `map<K, V>` (la `tabula`) | an arena-allocated `faber_map_<k>_<v>` handled by pointer (R1), backed by parallel key/value lists and a linear scan; keys and values are `int` or `text`. `empty`, the literal `{ "k": v }` (a JSON object), `m[k]` (a checked read that traps `tabula missing key`) and `m[k] ← v`, `put` / `get` (an optional) / `has` / `delete` / `length` / `is_empty`, key membership `k ∈ m`, and `↦ list<K>` / `↦ list<V>` (the target element class picks the side). `for from m let v` walks the values and `for from m at [k] let v` binds the key beside them; a key added during the walk traps `map changed size during iteration`. `sort via asc\|desc from m` orders the values and `ref m` the keys (materialized first). Key order is unspecified (D19.2) |
| `schema` declaration | a `schema` is an application-owned heading with no runtime effect: `faber run` declares it and runs the entry, so the emitter skips it and a use of its type is rejected where it is used. |
| genera (`class`) | an arena-allocated struct `g_<Name>` handled by pointer, so binds alias (R1); forward typedefs first; members `m_<name>` (a field may be a C keyword). Construction, from a struct literal or a typed object literal, allocates and then assigns the given fields in literal order and the omitted fields' defaults. Methods are `g_<Genus>_<method>` with the receiver `faber_self` after the arena. Display matches the runner (`Point { x = 1, tag = "p" }`), recursing into nested genera and lists. `ref` / `mut` on a genus parameter passes the same pointer, and a `ref` cursor over `T ∪ none` for a genus is that pointer too (so `cr ← cur.next` re-binds it) |
| `float` (binary64) | C `double`; arithmetic is plain C, bit-exact once fused multiply-add is off (the runtime's pragmas cover clang and gcc in every mode). Display matches the runner: integral values print their exact expansion and `.0`, others their shortest round-trip digits positionally, and `inf`, `-inf`, `NaN`. `%` floors with the divisor's sign; `÷` on integers traps on zero; `⌊x⌋` `⌈x⌉` `⌊x⌉` round (ties to even) and `truncate` rounds toward zero, all saturating to `int` as the runner does; `abs`, `sign`, `minimum`, `maximum` are the float-valued methods (`faber_abs_f64`, `faber_signum_f64`, and the IEEE `minimum`/`maximum` whose NaN propagates and `-0.0` orders below `+0.0`); an integer compared with a float compares true values. `list<float>` is a list family. Programs link `-lm` |
| `f32` (binary32) | C `float`. A literal is the front end's folded binary32 value (`0.1f`); arithmetic is the plain C operation, which rounds once per operation at the binary32 width (the runner's declared-width rule). Display is the shortest binary32 round-trip with `.0` on an integral value (`16777216.0`, `0.30000001`, `0.0000001`), never the exact binary expansion. The floored `%` is `faber_fmod_f32`; `abs`, `sign`, `minimum`, `maximum`, `power` are the f32 helpers; the integer-yielding roundings and the integer/float comparison promote the operand exactly. Conversions go through the declared width (`int ↦ f32` is the cast; `text ↦ f32` parses binary64 then narrows; `f32 ↦ string` keeps the integral `.0` where `float ↦ string` drops it). `list<f32>` is a list family with its own total-order sort. |
| float math methods | `sqrt`, `exp`, `ln`, `log10`, `sin`, `cos`, `tan` are the libm function at the declared width (`sqrtf` for an `f32` receiver, `sqrt` for a `float` one), which the runner's own f32/f64 methods call on this host; `x.approx(y, tolerance)` is `|x − y| <= tolerance` with every operand promoted to binary64. `≈` / `≉` joins an integer against a float at the float's width. |
| range loops `for range a‥b [step s] let i`, ranges with `…`, and the range product `for range a‥b, c‥d let i, j` | an exact `faber_i128` counter and bounds evaluated once in source order (the runner's: an inclusive walk to `i64::MAX` steps past it, and only a step beyond the cap traps `numerus overflow`); the bounds pick the direction (`start ≤ end` ascends), a written step must be positive or the program aborts (`abort: range step must be positive`); the binder is `int64_t` read from the counter each pass; C's `break` / `continue` act on the innermost axis |
| list position `for from xs at [i] let x` | the walk's index as `const int64_t l_i` |
| lockstep `for from a, b let x, y` (and the three-source form) | every source evaluated once into a local; if any length differs the program aborts `itera lockstep sources must have equal length`, as the runner does; then one index walk binds the aligned element of each source, with the optional `apud [i]` coordinate as `const int64_t l_i` |
| `debug` and `warn` | `fprintf(stderr, …)`, the same format as `print`; `write` is a stdout line like `print` |
| `type` aliases | resolved in the type table, nothing emitted |
| `test "…" { … }` blocks | not emitted: `faber run` runs the entry block only, so the program is the same without them (`faber test` is another route) |
| `main args xs` | `int main(int argc, char **argv)`, and `xs` is a `list<string>` of `argv[1..]` (`faber_argv_text`) |
| `c ✓ a ✗ b` and `if` in value position | C's `?:`; each arm is a block holding only its value, and only the arm taken runs |
| `elif` chains | `else if`, flat |
| `do { … } while c` | C `do … while`, so `continue` runs through the test as in Faber; a plain `do { … }` block is a brace block, and `break` inside it is a `goto` to a label after it |
| `n ↑` / `n ↓` on an `int` local | `faber_store_i64(faber_add(n, 1), "assignment to `n`")`, the assignment's own check and text |
| `panic "m"` and `assert c [panic "m"]` | `faber_abort(m)`: `abort: m` on stderr, exit 1 (`assertion failed` without a message) |
| text and boolean comparison | text `≺ ≻ ≤ ≥ ≡ ≢` by `strcmp`, which orders UTF-8 bytes as unsigned: the code-point order the runner uses; booleans `≡` / `≢` only |
| `switch` / `match` on `int`, text, boolean and float values | scrutinees evaluated once in order into temporaries, then the first matching arm runs. Text, boolean, float and non-literal patterns use an `if` / `else if` chain; a final binding or wildcard may be its `else`. A dispatch on one integer subject uses C's `switch` when it has at least one integer-literal label, all labels are distinct, any catch-all is last, and no arm contains a Faber `break` (which must leave the surrounding loop); other supported dispatches stay on the chain. |
| a module with no entry block | the functions, and a `main` that traps `no entry function in MIR program`, which is what `faber run` does (stderr text, exit 1) |
| `i8` `i16` `i32` `u8` `u16` `u32` `u64` | the plain C integer of that width. A read widens to `int64_t` (`u64`, which does not fit one, to an exact value), arithmetic is the exact helpers, and the only way into the cell is `faber_store_<w>` / `faber_fit_<w>`, which write the runner's `<n> does not fit in `<w>` (<position>)` for the store's position (declaration, assignment, `argument`, `return`, field). A constant in range needs no check. Lists of sized integers are rejected |
| `value ∷ T` | the value, range-checked against `T` |
| `print a, b` | one space-joined line when the runner joins: a literal text first with up to two more, or every argument has a shared rendering. A sized integer or a genus among them makes the runner (and so this target) print each argument on its own line |
| a named function value, and a non-capturing anonymous closure | a C function pointer (`ret (*)(faber_arena *, params…)`), so a name in value position, a parameter, a local, a call through the value, and a closure literal all work the same way. A closure with no enclosing read is synthesized as one top-level `static` function (its parameters and body) and its value is a pointer to it; a call through a closure value is the same indirect call a named function value gets, and a `mut` list/genus parameter passes the caller's pointer. A closure that reads an enclosing binding needs a closure struct this subset does not emit and is rejected by name (`capturing closure`) |
| `copy` (R7) and `own` markers | `copy` deep-duplicates: a list gets a fresh arena array (`faber_list_<e>_copy`), a genus one duplicated field by field through its generated `g_<Name>_copy` (a list field a fresh array, a nested genus a recursive clone, a scalar / immutable text / scalar optional by assignment); a declaration's marker lands on the declared type, a call argument's on the argument. `own` is an ownership transfer and emits nothing. `ref` / `mut` alias the caller's pointer |
| `T ∪ none` over `int`, `bool`, `float`, text, lists and genera | text, lists and genera are pointers, so none is `NULL` and wrapping is the identity; `int`, `bool` and `float` are `faber_opt_i64` / `_bool` / `_f64` structs (`has`, `value`). `null`, `x is none`, `x not is none`, `x ≡ null`, `a coalesce b` (the fallback runs only for none), `x!` (aborts `unwrap of nihil`), `x!.field`, and a guard-narrowed read of the payload; printing shows the payload or the locale's none word |
| text methods: `length`, `is_empty`, `contains`, `starts_with`, `ends_with`, `uppercase`, `lowercase`, `trim`, `split`, `replace`, `slice`, and `s[i]` | `faber_text_*` in the runtime. Text is counted UTF-8: length, slice and index count code points, `slice` clamps and an empty range is `""`, `trim` strips Unicode White_Space, `split` and `replace` follow Rust's `str` (an empty separator splits around every code point). A negative position traps `expected non-negative numerus index`, an index past the end `text index out of bounds`. Case mapping is ASCII only: a byte outside ASCII aborts `unsupported on target c: case mapping of non-ASCII text` (the Unicode tables are not in the runtime) |
| `x ↦ T [via hint] [⊥ recovery]` between `int`, sized integers, `float`, text and `bool` | one C expression where it cannot fail (`int ↦ float`, `↦ string`, `bool ↦ int`, `int ↦ bool`); otherwise `faber_try_int` / `_text_int` / `_text_float` / `_float_int`, which report why. With `⊥` the recovery value takes the failure's place; without, the program ends with the runner's text (`textus to numerus conversion failed`, `… out of range`, `fractus to textus…`). Text to integer takes `via Hex|Bin|Oct` and reads an optional sign and digits only; text to float reads what Rust's `f64::from_str` reads; float to integer needs `via Floor|Ceil|HalfEven|Trunc` and fails on NaN and out of range; `float ↦ string` writes no `.0` on an integral value. `list<T> ↦ list<T, size N>` copies into a fixed list of capacity N, and fails `lista to bounded lista conversion failed` (or takes the `⊥` fallback) when the source is longer |
| the `creo` method | the post-construction hook: called once on the finished object, after every field is set, whichever way it was built |
| bounded lists `list<T, size N>`, and a literal's own length-bounded type | the same list class, with `fixed` set at run time: a literal is `faber_list_<e>_of_fixed` (capacity its length), `empty` is `_new_fixed(N)`, and a push at capacity ends the program with `bounded collection push at capacity`. `[…] ∷ list<T>` is written growable. `xs ↦ list<T, size N>` is `faber_list_<e>_bounded`, a fixed copy of capacity N (or the `⊥` fallback when the source is longer) |
| list methods `first`, `last`, `get`, `contains`, `remove_first`, `remove_last`, `invert`, `reverse`, `sorted`, `added`, `slice`, `take`, `take_last`, `drop`, `sum`, `argmax`, `argmin`, and `sort via asc\|desc from xs` | `faber_list_<e>_*` in the runtime. An element read or removal is an optional (`int ∪ none` is `faber_opt_i64`, text is a nullable pointer); `get` of a negative or past-the-end position is none; `slice` / `take` / `drop` clamp and a negative position traps `expected non-negative numerus index`; floats order by IEEE totalOrder (`-0.0` before `0.0`), text by code point, bool `false` before `true`. `sum` and `argmax` / `argmin` are over `list<int>` (the sum is exact and checked against the cap). A method that changes the list is an effect, so print arguments keep their written order |
| higher-order list methods `filter`, `map`, `reduce`, `cumulate`, `all`, `any` | one inlined `for` over the list's items calling the closure's C function pointer once per element (with the hidden arena when the program allocates); `filter` builds a new list of the elements the predicate admits, `map` builds a new list of the mapper's results (its element class is the call's result type), `reduce` carries the accumulator through the loop and yields it, `cumulate` pushes the running accumulator each element, and `all` / `any` short-circuit. The argument is a non-capturing closure or a named function |
| `enum` (la `ordo`) and `union` (la `discretio`), `variant V { f = x } ∷ T`, `Type.member`, `match` (la `discerne`) over variants, `enum ↦ int`, `enum ↦ string`, `int ↦ enum [⊥ default]` | a genus-shaped value: a pointer to `{ int64_t tag; <every variant's fields> }`; a variant with no payload is one static object, so payload-less values compare by pointer. A payload variant is one arena allocation; a variant pattern tests the tag and binds payload fields by position (names and wildcards only). `≡` on a payload union is a generated structural function over int, bool, text and nested union payloads. The tag is the position; an `enum` discriminant is read from the `↦` route. A number no member has aborts `no ordo member has this value`. Rejected: generic unions, unions with shared (commune) fields, text discriminants |
| `a?.b`, `a?[i]` | none when `a` is none, else the field or the checked element read as an optional (a field that is already optional stays one level). A base that is not optional is the plain read, wrapped |
| `x![i]` | the assertion (`x!`) then an ordinary checked element read (`xs[i]` or `s[i]`) |
| `optional` parameters (`T p optional`; la `sponte`) | the C storage is the optional of the declared type; a call that omits the argument passes none, and a body read keeps the none (the runner holds the omitted `nil` for `coalesce`) |
| `optional coalesce <expr>` parameters | the declared type alone; the call site evaluates the default and passes it, as the runner does in `lower_direct_call_args`. A call that omits a required slot is still rejected by name |
| exact `d64` decimals | a `faber_dec` pair (an `faber_i128` carrier at its own scale); a stored cell holds one at scale 8. `+ - * / %` are exact on the carriers, comparisons use the runner's three-way `faber_dec_cmp`, a store rounds half-even with the runner's trap text, and display keeps the shortest form (D2.6). The conversion rows cover decimal↔integer (`via` rounding hint), decimal↔float, and text↔decimal |
| elementwise list operators | `xs + ys` / `xs - ys` (equal-length pairing), `+ - * /` against a scalar on either side, the per-element comparisons `≺ ≻ ≤ ≥ ≅ ≇` (a `list<bool>`), and whole-list `≡` / `≢`. A length mismatch aborts `lista elementwise length mismatch`; integer elements run on the exact helpers and store at the element |
| `wrapping<W>` (and `modulus<W>`) | the plain C width; reads widen, arithmetic and shifts are uncapped (`faber_*_wide`), and only the store reduces modulo 2^W (`faber_store_wrapping_<w>`). Division and remainder floor on the representatives, `¬` and `↑`/`↓` land through the same store. `list<wrapping<W>>` is a width family (`faber_list_u8_*` …, display as the width prints, ordering by the width's own comparison); `list<saturating<W>>` is rejected |
| `saturating<W>` | the plain C width; arithmetic is exact until the store, which clamps to the width's ends (`faber_store_saturating_<w>`); `↦ saturating<W>` clamps and cannot fail |
| local names | the shared binding policy (`prefixed_binding_name`): `l_<name>`. Nothing C reserves starts with `l_`, so `char`, `goto`, `stdout` or `INT64_MAX` need no keyword table |

## Rejected by name

other float widths (`f16`, `bf16`), policy floats, absent
options, sized integers, `inf`, text comparison, nested lists,
other list methods, lists of genus, lists of `saturating` words,
`ref` / `mut` on scalars, generic genera,
contracts, shorthand object-literal fields,
bitwise operators, shifts, other `for` loops (lockstep, map and text sources), generic, async, device, error-channel
and command functions, parameter access markers, capturing closures, async entries, the other collections, tensors,
annotations, imports, and every other expression and statement
kind. Each is an `unsupported(<construct>)` site; the construct name tables are
in `expr.rs` and `ty.rs`.

## Fixtures

`tests/fixtures/run/*.fab` are compared with `faber run` by `scripta/diffcheck`
(stdout and exit status); `scalars.c`, `c_edges.c`, `loops.c` and
`functions.c` and `narrowing.c` are pinned. `functions.fab` runs recursion, call order with
printing callees, booleans, module constants and a function without a result.
`ranges.fab` runs every range form and edge, including an inclusive walk to `i64::MAX`; `trap_range_step.fab` and `trap_range_cap.fab` abort and trap as the runner does. `narrowing.fab` checks the fast path at the `int64_t` extremes and its exact
fallback on intermediates that overflow `int64_t` but not the cap. `loops.fab` runs Collatz, floor division over negative variables, both loop
jumps, the logical operators, and an unstored 2^63. The `trap_*.fab` fixtures
trap at a store, a call argument, a division by zero, and an intermediate
beyond the cap, and
must exit 1 with the runner's partial stdout. `list_methods.fab` runs the list methods over every element class. `bounded.fab` runs the fixed and growable forms and the capacity trap. `conversio.fab` runs the scalar conversions and their edges; `trap_conversio_*.fab` end each failure kind. `genus_creo.fab` runs the hook. `text_methods.fab` runs every text method and edge against the runner. `optional.fab` runs the optional forms and a trapping unwrap. `optional_chain.fab` and `sponte_params.fab` run optional-member and optional-index chaining, `optional` parameters (bare and `coalesce`) and the `x![i]` assertion index. `sized.fab`, `trap_sized.fab` and `trap_sized_argument.fab` run the width checks. `switch.fab` runs value dispatch including inside a loop that breaks. `modular_list.fab` runs `list<wrapping<W>>` over four widths and their order and display; `bounded_list_convert.fab` runs the `↦ list<T, size N>` copy, its `⊥` fallback and `↤`. `control.fab` runs conditionals, `elif`, do-while with `continue` and `break`, increment, and text and boolean comparison; `trap_assert.fab` and `trap_increment.fab` abort and trap. `entry_args.fab` runs `main args` with none given (checked by hand with arguments). `closure.fab` runs a compact closure, a block-body closure, a closure passed as an argument, a boolean closure in an `if` condition, and a closure with a `mut list<int>` parameter; `reject/capturing_closure.fab` fails by name. `list_higher_order.fab` runs `filter`, `map`, `reduce`, `cumulate`, `all` and `any` over a list. `tests/fixtures/reject/*.fab`
must fail with the named rejection.
