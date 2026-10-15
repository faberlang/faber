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
| local names | the shared binding policy (`prefixed_binding_name`): `l_<name>`. Nothing C reserves starts with `l_`, so `char`, `goto`, `stdout` or `INT64_MAX` need no keyword table |

## Rejected by name

`f32` and other float widths, policy floats, explicit `↦` conversions, absent
options, sized integers, `inf`, text comparison, nested lists,
other list methods, lists of genus, `ref` / `mut` on scalars, generic genera,
contracts, static and optional fields, shorthand object-literal fields,
bitwise operators, shifts, `for` loops, generic, async, device, error-channel
and command functions, parameter access markers and optional parameters,
function values, enums, maps and every other collection, tensors,
closures, annotations, imports, and every other expression and statement
kind. Each is an `unsupported(<construct>)` site; the construct name tables are
in `expr.rs` and `ty.rs`.

## Fixtures

`tests/fixtures/run/*.fab` are compared with `faber run` by `scripta/diffcheck`
(stdout and exit status); `scalars.c`, `c_edges.c`, `loops.c` and
`functions.c` and `narrowing.c` are pinned. `functions.fab` runs recursion, call order with
printing callees, booleans, module constants and a function without a result.
`narrowing.fab` checks the fast path at the `int64_t` extremes and its exact
fallback on intermediates that overflow `int64_t` but not the cap. `loops.fab` runs Collatz, floor division over negative variables, both loop
jumps, the logical operators, and an unstored 2^63. The `trap_*.fab` fixtures
trap at a store, a call argument, a division by zero, and an intermediate
beyond the cap, and
must exit 1 with the runner's partial stdout. `tests/fixtures/reject/*.fab`
must fail with the named rejection.
