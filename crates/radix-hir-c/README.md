# radix-hir-c

Novelty C target: one C translation unit from checked HIR. CLI target `c`,
Cargo feature `hir-c`, default-off and outside `full-targets`. Goal:
`docs/factory/hir-c/goal.md`.

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
| integer arithmetic `+ - * / %` and negation over variables | exact helper calls, narrowed by `faber_store_i64(value, "assignment to `n`")` at the store; the store text is the runner's. Where typecheck put no store node (its range rule proved the result fits), `faber_fit_i64` narrows instead |
| the 64-bit fast path | `/` and `%` of an `int64_t` by a constant other than 0 and -1 stay `int64_t` (`faber_fdiv_i64`, `faber_fmod_i64`). A pure `+ - *` tree over `int64_t` values ending in a store runs on `int64_t` with `__builtin_*_overflow` and one flag; on overflow the same expression is re-evaluated through the exact helpers, so the value, trap and trap text are the exact model's. An expression with a call never takes it, because the fallback re-evaluates |
| integer arithmetic over constants | folded at emit time with the same rules; `INT64_C(n)`, `-INT64_C(n)`, `INT64_MIN`, or an exact `UINT64_C` value above `i64` |
| integer comparisons `≺ ≻ ≤ ≥ ≡ ≢` | C comparisons by exact value (an `int64_t` widens to `faber_i128` against an exact operand) |
| `and`, `or`, `not`, `true`, `false` | `&&`, `\|\|`, `!`, `(1)`, `(0)` |
| `if` / `else`, `while`, `break`, `continue`, nested blocks, `return` from `main` | the same C statements |
| `print` of integers, exact intermediates, text and booleans | one `printf` per `print`; the format holds only `%" PRId64 "`, `%s`, spaces and `\n`; an unstored exact value prints through `faber_i128_text`; a boolean prints the module's display tokens (`true` / `false` in `en`, `verum` / `falsum` in `la`) |
| text literals | C string literal of the UTF-8 bytes: non-ASCII and control bytes as octal escapes, `?` escaped so no trigraph forms |
| text `+` and templates `"…§…"(args)` | `faber_concat` and one `faber_join` per template, in the arena. `§` shows the next argument, `§N` argument N; the split runs on the raw template before decoding, and each argument that is not a constant or a plain name is evaluated once, in order, before the join. Template escapes decode like ordinary strings (the runner prints them undecoded) |
| `list<int>`, `list<string>`, `list<bool>` | a pointer to an arena-allocated `faber_list_<elem>`, so binds alias (R1); one helper per operation, emitted only when used: literals, `empty`, `length`, `is_empty`, `append`, checked `xs[i]` reads and writes with the runner's trap texts, `for from` iteration, display with the runner's escapes. `ref` / `mut` on a list parameter passes the same pointer |
| genera (`class`) | an arena-allocated struct `g_<Name>` handled by pointer, so binds alias (R1); forward typedefs first; members `m_<name>` (a field may be a C keyword). Construction, from a struct literal or a typed object literal, allocates and then assigns the given fields in literal order and the omitted fields' defaults. Methods are `g_<Genus>_<method>` with the receiver `faber_self` after the arena. Display matches the runner (`Point { x = 1, tag = "p" }`), recursing into nested genera and lists. `ref` / `mut` on a genus parameter passes the same pointer |
| `float` (binary64) | C `double`; arithmetic is plain C, bit-exact once fused multiply-add is off (the runtime's pragmas cover clang and gcc in every mode). Display matches the runner: integral values print their exact expansion and `.0`, others their shortest round-trip digits positionally, and `inf`, `-inf`, `NaN`. `%` floors with the divisor's sign; `÷` on integers traps on zero; `⌊x⌋` `⌈x⌉` `⌊x⌉` round (ties to even) and saturate to `int` as the runner does; an integer compared with a float compares true values. `list<float>` is a list family. Programs link `-lm` |
| range loops `for range a‥b [step s] const i`, ranges with `…`, and the range product `for range a‥b, c‥d const i, j` | an exact `faber_i128` counter and bounds evaluated once in source order (the runner's: an inclusive walk to `i64::MAX` steps past it, and only a step beyond the cap traps `numerus overflow`); the bounds pick the direction (`start ≤ end` ascends), a written step must be positive or the program aborts (`abort: range step must be positive`); the binder is `int64_t` read from the counter each pass; C's `break` / `continue` act on the innermost axis |
| list position `for from xs at [i] const x` | the walk's index as `const int64_t l_i` |
| `debug` and `warn` | `fprintf(stderr, …)`, the same format as `print`; `write` is a stdout line like `print` |
| `type` aliases | resolved in the type table, nothing emitted |
| `test "…" { … }` blocks | not emitted: `faber run` runs the entry block only, so the program is the same without them (`faber test` is another route) |
| `main args xs` | `int main(int argc, char **argv)`, and `xs` is a `list<string>` of `argv[1..]` (`faber_argv_text`) |
| `c ✓ a ✗ b` and `if` in value position | C's `?:`; each arm is a block holding only its value, and only the arm taken runs |
| `elif` chains | `else if`, flat |
| `do { … } while c` | C `do … while`, so `perge` (C `continue`) runs through the test as in Faber; a plain `fac { … }` block is a brace block, and `rumpe` inside it is a `goto` to a label after it |
| `n ↑` / `n ↓` on an `int` local | `faber_store_i64(faber_add(n, 1), "assignment to `n`")`, the assignment's own check and text |
| `panic "m"` and `assert c [panic "m"]` | `faber_abort(m)`: `abort: m` on stderr, exit 1 (`assertion failed` without a message) |
| text and boolean comparison | text `≺ ≻ ≤ ≥ ≡ ≢` by `strcmp`, which orders UTF-8 bytes as unsigned: the code-point order the runner uses; booleans `≡` / `≢` only |
| `switch` / `discerne` on `int`, text, boolean and float values | scrutinees evaluated once in order into temporaries, then an `if` / `else if` chain over literal, negated-literal, alternative (`aut`), binding and wildcard patterns; the first arm that matches runs, a default is the `else`. C's `switch` is not used (it takes no text, and its `break` would stop leaving the loop around it) |
| a module with no entry block | the functions, and a `main` that traps `no entry function in MIR program`, which is what `faber run` does (stderr text, exit 1) |
| `i8` `i16` `i32` `u8` `u16` `u32` `u64` | the plain C integer of that width. A read widens to `int64_t` (`u64`, which does not fit one, to an exact value), arithmetic is the exact helpers, and the only way into the cell is `faber_store_<w>` / `faber_fit_<w>`, which write the runner's `<n> does not fit in `<w>` (<position>)` for the store's position (declaration, assignment, `argument`, `return`, field). A constant in range needs no check. Lists of sized integers are rejected |
| `value ∷ T` | the value, range-checked against `T` |
| `print a, b` | one space-joined line when the runner joins: a literal text first with up to two more, or every argument has a shared rendering. A sized integer or a genus among them makes the runner (and so this target) print each argument on its own line |
| `T ∪ none` over `int`, `bool`, `float`, text, lists and genera | text, lists and genera are pointers, so none is `NULL` and wrapping is the identity; `int`, `bool` and `float` are `faber_opt_i64` / `_bool` / `_f64` structs (`has`, `value`). `null`, `x is none`, `x not is none`, `x ≡ null`, `a coalesce b` (the fallback runs only for none), `x!` (aborts `unwrap of nihil`), `x!.field`, and a guard-narrowed read of the payload; printing shows the payload or the locale's none word |
| text methods: `length`, `is_empty`, `contains`, `starts_with`, `ends_with`, `uppercase`, `lowercase`, `trim`, `split`, `replace`, `slice`, and `s[i]` | `faber_text_*` in the runtime. Text is counted UTF-8: length, slice and index count code points, `slice` clamps and an empty range is `""`, `trim` strips Unicode White_Space, `split` and `replace` follow Rust's `str` (an empty separator splits around every code point). A negative position traps `expected non-negative numerus index`, an index past the end `text index out of bounds`. Case mapping is ASCII only: a byte outside ASCII aborts `unsupported on target c: case mapping of non-ASCII text` (the Unicode tables are not in the runtime) |
| `x ↦ T [via hint] [⊥ recovery]` between `int`, sized integers, `float`, text and `bool` | one C expression where it cannot fail (`int ↦ float`, `↦ string`, `bool ↦ int`, `int ↦ bool`); otherwise `faber_try_int` / `_text_int` / `_text_float` / `_float_int`, which report why. With `⊥` the recovery value takes the failure's place; without, the program ends with the runner's text (`textus to numerus conversion failed`, `… out of range`, `fractus to textus…`). Text to integer takes `via Hex|Bin|Oct` and reads an optional sign and digits only; text to float reads what Rust's `f64::from_str` reads; float to integer needs `via Floor|Ceil|HalfEven|Trunc` and fails on NaN and out of range; `float ↦ string` writes no `.0` on an integral value. Conversion of a list to a bounded list is rejected: the runner does not do it at run time either |
| the `creo` method | the post-construction hook: called once on the finished object, after every field is set, whichever way it was built |
| bounded lists `list<T, N>`, and a literal's own length-bounded type | the same list class, with `fixed` set at run time: a literal is `faber_list_<e>_of_fixed` (capacity its length), `empty` is `_new_fixed(N)`, and a push at capacity ends the program with `bounded collection push at capacity`. `[…] ∷ list<T>` is written growable. A list is not converted to a bounded list at run time (the runner does not either) |
| list methods `first`, `last`, `get`, `contains`, `remove_first`, `remove_last`, `invert`, `reverse`, `sorted`, `added`, `slice`, `take`, `take_last`, `drop`, `sum`, `argmax`, `argmin`, and `sort via asc\|desc from xs` | `faber_list_<e>_*` in the runtime. An element read or removal is an optional (`int ∪ none` is `faber_opt_i64`, text is a nullable pointer); `get` of a negative or past-the-end position is none; `slice` / `take` / `drop` clamp and a negative position traps `expected non-negative numerus index`; floats order by IEEE totalOrder (`-0.0` before `0.0`), text by code point, bool `false` before `true`. `sum` and `argmax` / `argmin` are over `list<int>` (the sum is exact and checked against the cap). A method that changes the list is an effect, so print arguments keep their written order |
| `ordo` and `discretio` (`enum`, `union`), `variant V { f = x } ∷ T`, `Type.member`, `discerne`/`match` over variants, `ordo ↦ int`, `ordo ↦ string`, `int ↦ ordo [⊥ default]` | a genus-shaped value: a pointer to `{ int64_t tag; <every variant's fields> }`; a variant with no payload is one static object, so payload-less values compare by pointer. A payload variant is one arena allocation; a variant pattern tests the tag and binds payload fields by position (names and wildcards only). `≡` on a payload union is a generated structural function over int, bool, text and nested union payloads. The tag is the position; an `ordo` discriminant is read from the `↦` route. A number no member has aborts `no ordo member has this value`. Rejected: generic unions, unions with shared (commune) fields, text discriminants |
| `a?.b` | none when `a` is none, else the field as an optional (a field that is already optional stays one level). A base that is not optional is the plain read, wrapped |
| local names | the shared binding policy (`prefixed_binding_name`): `l_<name>`. Nothing C reserves starts with `l_`, so `char`, `goto`, `stdout` or `INT64_MAX` need no keyword table |

## Rejected by name

`f32` and other float widths, policy floats, absent
options, sized integers, `inf`, text comparison, nested lists,
other list methods, lists of genus, `ref` / `mut` on scalars, generic genera,
contracts, static and optional fields, shorthand object-literal fields,
bitwise operators, shifts, other `for` loops (lockstep, map and text sources), generic, async, device, error-channel
and command functions, parameter access markers and optional parameters,
function values, async entries, maps and every other collection, tensors,
closures, annotations, imports, and every other expression and statement
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
must exit 1 with the runner's partial stdout. `list_methods.fab` runs the list methods over every element class. `bounded.fab` runs the fixed and growable forms and the capacity trap. `conversio.fab` runs the scalar conversions and their edges; `trap_conversio_*.fab` end each failure kind. `genus_creo.fab` runs the hook. `text_methods.fab` runs every text method and edge against the runner. `optional.fab` runs the optional forms and a trapping unwrap. `sized.fab`, `trap_sized.fab` and `trap_sized_argument.fab` run the width checks. `switch.fab` runs value dispatch including inside a loop that breaks. `control.fab` runs conditionals, `elif`, do-while with `continue` and `break`, increment, and text and boolean comparison; `trap_assert.fab` and `trap_increment.fab` abort and trap. `entry_args.fab` runs `main args` with none given (checked by hand with arguments). `tests/fixtures/reject/*.fab`
must fail with the named rejection.
