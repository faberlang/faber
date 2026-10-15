# radix-hir-c

Novelty C target: one C11 translation unit from checked HIR. CLI target `c`,
Cargo feature `hir-c`, default-off and outside `full-targets`. No goal doc
yet; the spike follows the shape of `hir-odin` unit OD1.

This is the **registration spike**: the skeleton and the full registration
across the tree, with a scalar-and-print emitter. Feature breadth arrives in
later units; everything else fails closed with
`unsupported on target c: <construct>`, never an approximation. The leaf never
invokes a C compiler (the `no_toolchain_command_new` ratchet forbids it);
`scripta/diffcheck` builds the emitted program and runs the built binary.

No runtime library is emitted, and the emitted program allocates nothing and
frees nothing.

## The rule this target exists to keep

C is the permissive compiler, so the emitter is the strict one: **no emitted
program may reach undefined behavior.** `scripta/diffcheck` builds every run
fixture with `-std=c11 -pedantic -Wall -Wextra -Werror` and
UndefinedBehaviorSanitizer with recovery off, and fails on any sanitizer
report. Where Faber semantics meet C undefined behavior, the construct is
rejected until its exact form lands:

| Faber rule | C hazard | Spike behavior |
| --- | --- | --- |
| integer arithmetic is exact until the store | signed overflow is UB | arithmetic on a variable is rejected; constants fold at emit time in exact 128-bit integers with the runner's `[-2^63, 2^64-1]` cap |
| `bN` shift by N or more gives zero; a negative count errors | shift by ≥ width or negative is UB | shift operators rejected |
| `↦` reinterprets bits | type punning through a pointer breaks strict aliasing | conversions rejected (the exact form is `memcpy`) |
| text is a counted UTF-8 value | `%s` stops at NUL | a text literal containing NUL is rejected |

## Supported

| Faber feature | C |
| --- | --- |
| `print` of integers and text, several values space separated | one `printf` per `print`; the format holds only `%" PRId64 "`, `%s`, spaces and `\n`, so a `%` in Faber text prints as itself |
| integer literals and constant folding of `+ - * / %` (floor `/` and `%`) | folded at emit time, emitted as `INT64_C(n)`, `-INT64_C(n)`, or `INT64_MIN` |
| `int` and `string` locals, and reads of them | `const int64_t` / `int64_t`, `const char *const` / `const char *`; an immutable integer local with a folded initializer folds when read |
| text literals | C string literal of the UTF-8 bytes: non-ASCII and control bytes as three-digit octal escapes (a `\x` escape would swallow a following hex digit), `?` escaped so no trigraph forms |
| Faber names that C or its headers reserve | keywords (C23) and used library names (`printf`, `stdout`, `EOF`, ...) take a numeric suffix; reserved patterns are rewritten: `_X`/`__x` → `v__X`, `*_t` → `*_t_v`, `INT*`/`UINT*` macro spellings and `PRI*`/`SCN*` → `v_*` |

Headers are included only when used: `<inttypes.h>` for any integer,
`<stdio.h>` for any print. `main` ends with `return 0;`.

## Rejected by name

Floats, booleans (`printing a boolean`), absent options
(`printing an absent option`), sized integers, text arithmetic, arithmetic or
negation over a variable, comparisons, logical and bitwise operators, shifts,
conditionals, loops, functions, module constants, genera, enums, lists and
every other collection, closures, the error channel, async, annotations,
imports, and every other expression and statement kind. Each is an
`unsupported(<construct>)` site; the construct name tables are in `expr.rs`
and `ty.rs`.

## Fixtures

`tests/fixtures/run/*.fab` are compared with `faber run` by `scripta/diffcheck`
(stdout and exit status); `scalars.c` and `c_edges.c` are the pinned emitted
programs. `tests/fixtures/reject/*.fab` must fail with the named rejection.
`c_edges.fab` covers the C-specific edges: `INT64_MIN`, `%` in text, `??!`,
non-ASCII followed by hex digits, and Faber names C reserves.
