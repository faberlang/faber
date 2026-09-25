# Semantics Rulings

Operator rulings on Faber runtime semantics. Faber is its own language: these
rules define the behavior, and every backend (MIR runner, Rust, LLVM host,
device targets, emit-only targets) conforms to them. A backend that differs is
defective, whatever its host language would do by default.

Design principle (2026-09-24): Faber is written mostly by LLMs, so its rules
must be mechanical and predictable — one rule per construct, visible in the
source, and the same on every backend.

Each ruling should be pinned by a corpus exemplum that runs on every
run-capable backend. The normative grammar sidecar
(`docs/grammar/sidecar.en.toml`) should absorb these rules as it is next
revised; until then this file is the record.

## R1. Unmarked binds alias, for every type (2026-09-24)

`const T b ← a` and `var T b ← a` bind `b` to the same value as `a`, whether
`T` is a list, a map, a class, or any other aggregate. The access marker
decides the behavior, never the type (mutability-engine D-1). `copy` is the
explicit duplicate: `var copy T b ← a` and `fn f(copy T x)` deep-copy the value
when it is acquired, and the source stays live (D-6).

## R2. `coalesce` short-circuits (2026-09-24)

`a coalesce b` (la `vel`) evaluates `b` only when `a` is `nihil`, the same as
the short-circuit rule for `and` and `or`: an operand runs only if its value is
needed. Precedence is unchanged: `coalesce` binds tighter than the
multiplicative operators, and its right operand is a unary expression
(`vel_expr`, `vel_rhs`).

## R3. `continue` cannot target a `do` block (2026-09-24)

`break` inside a `do { } catch` block leaves the `do` block. `break` and
`continue` must target the same construct, and continuing a `do` block has no
meaning. So a `continue` whose nearest enclosing breakable construct is a `do`
block is a compile error. A `continue` inside a loop that is itself nested in
the `do` block stays legal.

## R4. Closures share the enclosing scope unless marked `free` (2026-09-24)

An unmarked closure captures by reference: a captured name is the enclosing
binding itself. Writes through a captured `var` are visible outside the
closure, and outer writes made after capture are visible inside. Creating a
closure copies nothing. `free` (la `libera`) declares a capture-free closure,
and `kernel` is `free` plus the device-safe subset. Loop bindings
(`for … const i`) are fresh on every iteration, so each closure created in a
loop captures its own `i`.

## R5. Floats follow IEEE 754 (2026-09-24)

Float arithmetic, comparison, and standard math functions produce IEEE 754
results. Invalid operations return NaN, overflow returns ±∞, and no function
substitutes a sentinel such as `0.0`. This supersedes the v1 "silent 0.0"
domain policy.

- **Comparison.** `≺ ≼ ≻ ≽ ≡` involving NaN are false, `≠` is true. Comparing
  a NaN never traps.
- **`minimum` / `maximum`.** These are IEEE 754-2019 `minimum` and `maximum`:
  NaN propagates, and `-0.0` orders below `+0.0`.
- **Sorting.** Float sort uses IEEE 754 `totalOrder`:
  `-NaN < -∞ < … < -0.0 < +0.0 < … < +∞ < +NaN`.
- **`power`.** `x.power(y)` follows IEEE 754 `pow`: a negative base with an
  integral exponent returns the signed result, and a non-integral exponent
  returns NaN.
