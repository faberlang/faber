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

## R3. `break` stops at a `do` block; `continue` passes through it (2026-09-25)

Supersedes the 2026-09-24 draft, which rejected `continue` inside a `do` block.

- **`break`** (la `rumpe`) targets the nearest breakable construct: a loop or
  a `do` block. `break` inside `do { } catch` leaves the `do` block.
- **`continue`** (la `perge`) targets the nearest loop in the same function.
  `do … while` is a loop. A bare `do` block is not a loop, so `continue`
  passes through it, and through `catch` arms, to the enclosing loop. It does
  not cross a function or closure boundary. With no loop in the same function,
  `continue` is a compile error.
- Leaving a `do … catch` by `break` or `continue` does not run the `catch`,
  because neither is an error.
- **Where `continue` lands** (2026-09-25): `continue` transfers control to the
  end of the nearest loop's body, skipping the rest of every block it leaves.
  For `do … while c` and `while c` the condition `c` is then tested; for a
  `for` loop the next iteration begins. `continue` never jumps back to the top
  of a body without the loop's own step or test.

The two words mean different things: `break` leaves a block, and `continue`
starts the next iteration. A `do` block has no iterations.

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

## R6. One const/var rule for writes and mutating calls (2026-10-02)

Once a `copy` parameter's duplicate exists, its owner may change it exactly as
far as the binding's const/var declaration allows. A field write (`c.n ← 1`)
and a call to a method that changes its receiver (`c.inc()`) are the same kind
of write and follow one rule: a `var` binding allows both, an immutable binding
(`const`, or a parameter) rejects both.

- A method changes its receiver when its body assigns through `self`, calls an
  in-place collection verb on a place rooted at `self`, or calls another
  mutating method on such a place. Methods carry no receiver marker; the
  property is inferred from the body (and closed over sibling calls). Only
  methods declared in the same module are classified.
- A parameter is an immutable binding, and `copy` is the only marker in its
  type position, so no spelling makes a `copy` parameter `var`. To modify the
  duplicate, bind it to a `var` local first (`var C mine ← c`). The `in`/`mut`
  marker is the way to modify the caller's own value, and it duplicates nothing.
- The `unnecessary_varia` lint counts a mutating method call as a modification.

Pinned by `corpus/mutabilitas/copy-param.fab` (the D-6 copy-parameter
exemplar) and the semantic tests in `method_receiver_mutation_test.rs`.

## R7. `copy` applies at the initializer only (2026-10-02)

`copy` marks how a binding is initialized, never the binding afterwards.
`var copy Point b ← a` duplicates `a`. A later `b ← c` of a class value is
always by reference, whether or not `b` was declared with `copy`: after
`var copy Point b ← a; b ← c; b.x ← 5`, `c.x` is 5 and `a.x` is untouched.

The point of the declaration is that the reader knows at each assignment site
whether it references or copies. A `var copy` declaration copies, and a plain
`←` never does. If `copy` followed the object, an assignment would never say
which one it is. To copy again, write a new `var copy` declaration.

Pinned by `corpus/mutabilitas/copy-bind-deep.fab` (documentation beside the
D-6 exemplar) and the MIR runner test `copy_duplicate_test.rs`.
