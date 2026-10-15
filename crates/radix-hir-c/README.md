# radix-hir-c

Novelty C target: one C translation unit from checked HIR. CLI target `c`,
Cargo feature `hir-c`, default-off and outside `full-targets`. No goal doc
yet.

This is a **spike**: an integer-and-text subset with real codegen (exact
arithmetic over variables, loops, conditionals). Everything else fails closed
with `unsupported on target c: <construct>`, never an approximation. The leaf
never invokes a C compiler (the `no_toolchain_command_new` ratchet forbids
it); `scripta/diffcheck` builds the emitted program and runs the built binary.

The output needs GCC or Clang: it uses `__int128` and
`__builtin_mul_overflow`. No runtime library is emitted; headers and helper
functions are written into the program only when it uses them. The emitted
program allocates nothing and frees nothing.

## The rule this target exists to keep

C is the permissive compiler, so the emitter is the strict one: **no emitted
program may reach undefined behavior.** `scripta/diffcheck` builds every run
fixture with `-std=c11 -pedantic -Wall -Wextra -Werror` and
UndefinedBehaviorSanitizer with recovery off, and fails on any sanitizer
report.

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
| functions with `int`, `string` and `bool` parameters and results, or no result; recursion | `static` C functions named `f_<name>` with prototypes first, so any call order works; parameters `p_<name>`; a result returns through its `return` store |
| calls | C leaves argument and operand order unspecified, so when two or more operands of one evaluation include a call, they are bound to temporaries in source order inside a GNU statement expression; an exact argument is checked like a store (`argument`) |
| module constants | `static const` `k_<name>` with the folded value |
| integer arithmetic `+ - * / %` and negation over variables | exact helper calls, narrowed by `faber_store_i64(value, "assignment to `n`")` at the store; the store text is the runner's |
| integer arithmetic over constants | folded at emit time with the same rules; `INT64_C(n)`, `-INT64_C(n)`, `INT64_MIN`, or an exact `UINT64_C` value above `i64` |
| integer comparisons `≺ ≻ ≤ ≥ ≡ ≢` | C comparisons by exact value (an `int64_t` widens to `faber_i128` against an exact operand) |
| `and`, `or`, `not`, `true`, `false` | `&&`, `\|\|`, `!`, `(1)`, `(0)` |
| `if` / `else`, `while`, `break`, `continue`, nested blocks, `return` from `main` | the same C statements |
| `print` of integers, exact intermediates, text and booleans | one `printf` per `print`; the format holds only `%" PRId64 "`, `%s`, spaces and `\n`; an unstored exact value prints through `faber_i128_text`; a boolean prints the module's display tokens (`true` / `false` in `en`, `verum` / `falsum` in `la`) |
| text literals | C string literal of the UTF-8 bytes: non-ASCII and control bytes as octal escapes, `?` escaped so no trigraph forms |
| local names | the shared binding policy (`prefixed_binding_name`): `l_<name>`. Nothing C reserves starts with `l_`, so `char`, `goto`, `stdout` or `INT64_MAX` need no keyword table |

## Rejected by name

Floats, absent options, sized integers, `inf`, text arithmetic and comparison,
bitwise operators, shifts, `for` loops, generic, async, device, error-channel
and command functions, parameter access markers and optional parameters,
function values, genera, enums, lists and every other collection, tensors,
closures, annotations, imports, and every other expression and statement
kind. Each is an `unsupported(<construct>)` site; the construct name tables are
in `expr.rs` and `ty.rs`.

## Fixtures

`tests/fixtures/run/*.fab` are compared with `faber run` by `scripta/diffcheck`
(stdout and exit status); `scalars.c`, `c_edges.c`, `loops.c` and
`functions.c` are pinned. `functions.fab` runs recursion, call order with
printing callees, booleans, module constants and a function without a result.
`loops.fab` runs Collatz, floor division over negative variables, both loop
jumps, the logical operators, and an unstored 2^63. The `trap_*.fab` fixtures
trap at a store, a call argument, a division by zero, and an intermediate
beyond the cap, and
must exit 1 with the runner's partial stdout. `tests/fixtures/reject/*.fab`
must fail with the named rejection.
