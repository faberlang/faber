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

## R6. One write rule for fields and mutating calls; the marker decides what a parameter may write (2026-10-02, amended 2026-10-08)

A field write (`c.n ← 1`) and a call to a method that changes its receiver
(`c.inc()`) are the same kind of write and follow one rule. For a local binding
the rule is its declaration: a `var` binding allows both, a `const` binding
rejects both. For a parameter the rule is its marker. Parameters take no `var`
or `const`.

*Amended 2026-10-08:* the 2026-10-02 wording ("an immutable binding (`const`, or
a parameter) rejects both", "bind the `copy` parameter to a `var` local first")
is replaced. The callee's right to write follows the marker:

| Parameter | May the callee write it? | Whose value changes |
| --- | --- | --- |
| unmarked, `ref` (la `de`) | no (`SEM020`) | none |
| `mut` (la `in`) | yes | the caller's own value |
| `own` | yes | the callee's value; the caller gave it away |
| `copy` | yes | the callee's duplicate; the caller's value is untouched |

- Writing includes a field store, a method that changes its receiver, an
  element store, a whole reassignment (`xs ← [1, 2]`) and an in-place
  collection verb. An `own` tensor parameter may be rebound whole; a `mut`
  tensor parameter may not (`mut_tensor_param_rebind`, write through `⇇`).
- A method changes its receiver when its body assigns through `self`, calls an
  in-place collection verb on a place rooted at `self`, or calls another
  mutating method on such a place. Methods carry no receiver marker; the
  property is inferred from the body (and closed over sibling calls). Only
  methods declared in the same module are classified.
- Binding a `copy` parameter to a `var` local first (`var C mine ← c`) is still
  legal, and is no longer needed.
- The `unnecessary_varia` lint counts a mutating method call as a modification.
  `WARN009 unused_own_parameter` counts a write to an `own` parameter as a use.

Pinned by `corpus/mutabilitas/own-param-writable.fab` and
`copy-param-writable.fab` (every write form on `own` and `copy`), by
`corpus/mutabilitas/copy-param.fab` (the D-6 copy-parameter exemplar; its
header comment still describes the 2026-10-02 rule), and by the semantic tests
in `method_receiver_mutation_test.rs` and `own_param_writable_test.rs`.

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

## R8. An argument carries the marker its parameter declares (2026-10-08)

A parameter declared `mut` / `own` / `copy` (la `in` / `own` / `copy`) receives
an argument that carries the same marker at the call: `bump(mut a)`,
`eat(own a)`, `dup(copy a)`. The read marker `ref` (la `de`) is optional and may
be written explicitly. The reader of a call sees which arguments may be changed,
consumed or duplicated without opening the callee.

- **Where it applies.** Every function with a source signature, top-level or in
  a class/genus. A method's arguments follow the rule (`obj.run(mut src)`); the
  receiver carries no marker. A call through a function value or closure is
  checked against the modes in the function type. Built-in, intrinsic and
  provider methods are exempt: the intrinsic registry records no access mode.
- **Diagnostics (`SEM057`).** `call_marker_missing` (parameter marked, argument
  bare), `call_marker_mismatch` (a different marker), `call_marker_on_read_only`
  (a marker other than `ref` on a read-only parameter).
- **Temporaries.** A temporary passed to a `mut` parameter is rejected
  (`SEM020 mut_argument_not_writable`): there is no place to change. `own` and
  `copy` accept a fresh value with the matching marker (`eat(own make())`).
- **`copy` at the call** is a plain echo of the parameter marker. It is not the
  parked assignment-site `copy` (R7 stays as ruled).
- **Landed 2026-10-08 (`own` and `copy`).** `own` and `copy` parameters are
  writable by the callee (R6). Only a whole local binding may be passed as
  `own`; `SEM057` names the refusals: `own_argument_not_local_binding` (a field
  or element: write `copy` there instead, and `copy b.xs` does not kill `b`),
  `own_argument_unmarked_parameter`, `own_argument_module_constant`,
  `own_argument_loop_binder`. After `eat(own a)` the name `a` is dead and a
  later use is `SEM050 use_after_own`, blamed on the call argument; an unmarked
  source gets a `copy` suggestion. `moved_in_loop` (a name declared outside a
  loop and given away inside it) and `moved_name_captured_by_closure` are the
  loop and closure forms. An exclusive (`own`/`copy`) source keeps
  `use_after_move` at the later read. A fresh value (`eat(own make())`) is
  always accepted. The binding form `const own b ← a` is unchanged.

Pinned by `corpus/mutabilitas/call-marker.fab` and the rejection fixtures
`call-marker-missing.fab`, `call-marker-mismatch.fab`,
`call-marker-on-read-only.fab`; for the landed items by `own-arg-local.fab`,
`own-fresh.fab` and the rejections `own-arg-use-after.fab`,
`own-arg-moved-in-loop.fab`, `own-arg-captured-closure.fab`,
`own-arg-element.fab`, `own-arg-field.fab`, `own-arg-unmarked-param.fab`,
`own-arg-module-constant.fab`, `own-arg-loop-binder.fab`.
