# Faber Operators and the Error-Conversion Seam

Scan aid, not a replacement for the live grammar. Every row was probed against
the built `faber` binary (`faber check` / `faber run`, en locale).
Rows give the en spelling, with the stable Latin identity in parentheses where
it helps; word forms follow the active pack, glyphs are the same in every locale. When this
file and the repo disagree, the repo wins: `grammar/EBNF.md` (generated from
`grammar/sources/source.fg`) and `corpus/operatores/`. A tree-sitter grammar
is a highlighting consumer and never grammar truth.

**Glyph law:** arrows mark runtime effects or typed boundaries. `←` stores,
`↤` converts then stores, `↦` converts, `→` appears only in a return clause or
function type (`"5" → int` is a parse error), `⇥` names the error channel type.
`=` and `:` state compile-time facts or labels, never a runtime store. There is
no `x += 1`, no `x++`, no `x = 5` statement; the only runtime store is `←`, with
postfix `↑` / `↓` as statements (not expressions).

## Operator table

| Surface | Meaning and boundary |
| --- | --- |
| `+` `-` `*` `/` `%` | add, subtract, multiply, floor-divide, floor-remainder; unary `-` negates. Integer arithmetic is exact; a value is checked only at a store into a typed slot (or traps when an intermediate leaves [−2⁶³, 2⁶⁴ − 1]). `/` floors (`-7 / 2` is `-4`); `%` takes the divisor's sign (`-7 % 2` is `1`). `+` concatenates strings only (`"x" + 1` is `SEM011`). |
| `÷` | true division on every numeric family: `7 ÷ 2` is `3.5`; on integers the result is `f32` when the widest operand is 8/16-bit, else `f64`; never an integer. Same precedence as `/`. |
| `·` `⊗` `⊙` `⊘` `×` | tensor dot/matvec/matmul (rank dispatch), outer/Kronecker, Hadamard product, Hadamard divide, cross product. Shapes must match exactly, with no broadcasting (`glyph_hadamard_shapes_mismatch`, `glyph_inner_dim_mismatch`, `glyph_cross_width_three_required`). `tensor * tensor` is rejected (`numeric_operands_required`); use `⊙`. Glyphs are canonical; method twins (`.dot`, `.matmul`) also exist. A scalar operator lifts elementwise over a tensor. |
| `ᵀ` / `ᵗ` | postfix rank-2 transpose; `ᵗ` is the rejected lookalike (it reads as an identifier), use capital superscript `ᵀ`. |
| `⤒` / `⤓` | infix maximum/minimum, additive tier, left-associative; chains on tensors express elementwise clamp. Whole-tensor reductions are `reduce via max from` / `reduce via min from` (`reducta via maxima` / `reducta via minima`), which take a shaped tensor or matrix; a `list` operand is `reducta_requires_shaped_iterable`, and an empty-possible result needs a `coalesce` (la `vel`) identity or nullable handling. |
| `⌊x⌋` / `⌈x⌉` / `⌊x⌉` | rounding glyphs: floor, ceiling, nearest; each reads as a method twin (`floor`, `ceiling`, `round`) and the result is an integer. Nearest sends ties to the even neighbour: `⌊2.5⌉` is 2, `⌊3.5⌉` is 4, `⌊-2.5⌉` is -2, on every target. `⌈x⌋` is the named `PARSE030 ceiling_floor_delimiters` error pointing at `⌊x⌉`; a tensor operand is `SEM011 glyph_rounding_tensor_unsupported` until the elementwise lift is built. |
| `√` `∛` `∜` | radical prefixes, spaced prefix sugar binding like unary `-` / `¬` (`√ x + 1` is `(√ x) + 1`): `√` is `.sqrt()` exactly where the method is legal (floats and float tensors; an int operand is the `.sqrt()` error), `∛` is the real cube root of `.cbrt()` (`∛ -8.0` is `-2.0`), `∜` is `.sqrt().sqrt()` (NaN for negatives, as `sqrt`). Locale-invariant math glyphs, no word twin. |
| `.argmax()` / `.argmin()` | list-only (a tensor gives `SEM004 unknown_method`). First-index tie law; empty source yields the null value; NaN never wins while a number participates, and an all-NaN source keeps index 0. Keep the result as `int ∪ none` or coalesce it: a plain `int` store checks but fails at run on empty input; there is no `-1` sentinel. |
| `≡` / `≢` | strict equality and its one negation; identical types are required (`≡` between different widths, or between `T ∪ none` and `T`, is `SEM010 equality_exact_types_required`). `≠` is retired: it is `PARSE030 neq_glyph_retired` naming `≢`. |
| `≅` / `≇` | promoted-exact equality for numbers: widths join losslessly and compare exactly (`i8` -1 `≇` `u64` max; `int ≅ f64` joins exactly). Use it for numbers only; mixed non-numeric operands are `SEM011 numeric_value_equality_operands_required`, and same-typed lists or strings currently pass as plain equality, which is not a contract. |
| `≈` / `≉` | fuzzy equality: relative 1e-9, absolute 0 (`0.0 ≈ 1e-13` is false), in the operands' own joined precision. Between integers of any width, `inf` included, the exact rule `10⁹·|a − b| ≤ max(|a|, |b|)`. Numeric operands only: a string or list operand checks but fails at run. Tensor `≈` is deferred (`tensor_approx_comparison_deferred`). |
| `is` / `not is` (la `est` / `non est`) | type test; the right side is always a type. `x is 3` is a parse error, `b is true` is `SEM011 est_value_rhs`. The null test is `x is none` (la `x est nihil`); `x is null` is `PARSE040 null_value_in_type_position`; the null-value comparison is `x ≡ null`. `x is int` works on `int ∪ none`. Negation in en is `not is`, never `is not`. |
| `≺` / `≻` / `≤` / `≥` | less-than, greater-than, inclusive ordering; ASCII `<` / `>` are legacy comparison aliases, while `<` / `>` also remain generic delimiters. |
| `∈` / `∉` | membership and its first-class negation; comparison tier; no ASCII spelling. `x ∈ [v1, v2, …]`, `x ∈ set`, and `k ∈ map` (map keys) test collection membership; `x ∈ a‥b` / `x ∈ 0…10` test range containment. Never over text: `"a" ∈ s` is `SEM010 membership_over_text`, use `s.contains("a")` (la `continet`). The old words `within` / `between` (la `intra` / `inter`) are retired as operators (`PARSE001 membership_keyword_retired`, naming `∈`) and remain ordinary identifiers; set intersection (`.intersect(...)`; la `.inter(...)`) is unrelated. Over a range the form checks and emits on Rust, Go, Swift, and TS but fails on the MIR runner (`binary operator without a MIR primitive`). |
| `∨` / `⊻` / `∧` | bitwise OR, XOR, AND on integers and on matching raw-bit widths (`b8`, `b16`, `b32`, `b64`); booleans use `or` / `and` / `not` (la `aut` / `et` / `non`). |
| `⇐` / `⇒` | left/right shift, unmasked: `x ⇐ n` is `x·2ⁿ`, `x ⇒ n` is `floor(x / 2ⁿ)`; a negative count is `SEM010 shift_count_negative`. |
| `¬` / `not` | on integers `¬x` is width-free complement, `-x - 1`; on `b8`/`b16`/`b32`/`b64` it complements within that raw-bit width (`¬true` is `SEM011`); `not` (la `non`) is boolean not (booleans only); `not is` (la `non est`) is the negated type test. |
| `‥` / `…` / `before` / `until` / `step` | exclusive/inclusive ranges and their word forms (la `ante` / `usque` / `per`); `step` supplies a stride. A descending `5‥0` iterates 5,4,3,2,1. |
| `coalesce` (la `vel`) | nullable coalesce/default; it is not logical `or` (`1 or 2` is `boolean_operands_required`). Binds tighter than `*`, so `n coalesce 2 * 3` is `6`. A `coalesce` right side may carry a range tail. |
| `or` / `and` | logical OR / AND (la `aut` / `et`). |
| `✓` `✗` | the one value conditional: `c ✓ a ✗ b`; nesting is rejected. Replaces the retired `?:` and `yields … else` (la `sic … secus`), which report `PARSE030 conditional_question_colon_removed` / `conditional_sic_removed`. `?` still starts optional access. |
| `¶` | display-format operator: `value ¶ "spec"` renders to a string (en `string`, la `textus`); same glyph in every locale; binds looser than arithmetic, tighter than comparison; does not chain (`format_chained`); the spec must be a literal. |
| `⊥` | default/fallback on a failing conversion or failable call: `↦ T ⊥ expr`, `call() ⊥ expr`. After a plain `←` it is `default_requires_failable`. `⇥ E` only ever names an error type; `⇥ 0` is `PARSE030 recovery_uses_exit_arrow`. |
| `←` / `↤` / `↦` | store, convert-then-store, explicit conversion. For integer widths `↦` into `wrapping<W>` reduces and into `saturating<W>` clamps (the slot type must match); neither fails. A float `saturating<W>` instead keeps IEEE results, infinity and NaN included. A `↦ via Hint` clause carries a hint (`x ↦ u16 via Le`); the type-argument hint form is gone. A float or `d64` into an integer needs `via Floor|Ceil|HalfEven|Trunc` (bare is `SEM016 float_to_integer_requires_rounding`). In a `↤` chain `⊥` attaches to the nearest `↤`. |
| `⇇` | tensor copy-into, `target ⇇ value`: writes the values of `value` into the existing storage of a writable tensor `target` (a statement, single, no chain; see Copy-Into in `references/grammar.md` for the rules; the old callable-sink form is retired, `SEM012 transfer_sink_retired`). `←` rebinds, `⇇` writes in place. Loosest operator. |
| `↑` / `↓` | postfix increment/decrement statements on an assignable place (not expressions); on a `let` it is `SEM020`. |
| `∇` | postfix gradient selection; parses, but `check` rejects it with `SEM010 gradient_expression_unsupported` until the autograd surface lands. |
| `∷` | static type ascription, a compile-time fact. A narrowing or int/float-crossing ascription is `SEM016 numeric_ascription_requires_conversion` (use `↦`); a redundant one warns `WARN005`. |
| `.` / `[]` / `?.` / `?[` / `?(` / `!.` / `![` / `!(` | member, index, optional-chain, and non-null-assertion suffixes. A member access on `T ∪ none` needs `?.` or `!.` (`SEM004 optional_receiver_requires_chain`); a nullable passed to a non-null slot checks and traps at run, so test with `is none` or `coalesce` first. |
| `then` (la `ergo`) / `∴` | `then` is the one-statement body joint of `if`/`while`/`for`/`case`/`default` arms (la `si`/`dum`/`itera`/`casu`/`ceterum`; `if c then return x`); `∴` is only the compact-closure joint (`let (i32) → i32 dupla ← i32 x ∴ x * 2`) and is `PARSE026` in a statement body. |
| `require … throw` / `reject … throw` (la `requirit … iace` / `reice … iace`) | guard statements: `require` throws into `⇥ E` when the condition is false; `reject` is the opposite and throws when the condition holds. The `throw` particle is required, and both need a declared `⇥ E` channel (`iace_requires_alternate_exit`); neither is fatal `assert` (la `adfirma`). |
| `=` / `:` / `<` / `>` / `@` | compile-time defaults, labels, generic delimiters, annotations; never a runtime store or width-blind equality. |
| `∪` / `∩` | `T ∪ U` unions and `A ∩ B` nominal intersections in type position. **Pending:** value-level set union/intersection have no semantics yet. |

Precedence, loosest to tightest: `⇇`; `✓ ✗`; `or`; `and`; equality (`≡ ≅ ≈ is`);
comparison (`≺ ≻ ≤ ≥ ∈ ∉`); `¶`; `∨`; `⊻`; `∧`; `⇐ ⇒`; ranges; `+ - ⤒ ⤓`;
`* / ÷ % · ⊙ ⊗ ⊘ ×`; `coalesce`; unary (`- ¬ not √ ∛ ∜`). In la, `or`, `and`, `is`, `coalesce` and `not` are `aut`, `et`,
`est`, `vel` and `non`. So `1 ∨ 2 ≡ 3` is true, `1 ⇐ 2 + 1`
is 8, and `10 - 3 ⤒ 5` is 7. Confirm against the `EBNF.md` expression productions
before relying on a corner.

List values reach exactness through a typed cast: `X ≡ ([…] ↦ list<int>)`.

## Tensor glyph operators

Tensor/vector arithmetic is glyph operators; `⊘` requires identical shapes and
waits for its first device consumer, matching the `⊗` fail-closed posture.
Legal device glyphs are documented per admitted consumer. Exemplars:
`corpus/tensor/glyph-*.fab` and
`references/idioms.md`.

`t.length()` is the element count, `t.rank()` the number of extents and `t.norm()` the Euclidean norm (vectors have `norm()` too); `rank` and `norm` are spelled that way in every locale. English edge-policy calls are `.edge<clamp>()`, `.edge<wrap>()` and `.edge<reflect>()`; Latin keeps `.limes<…>()`, and `limes` in an English file is wrong (`corpus/tensor/rank-norm.fab`, `corpus/tensor/edge-shift-policies.fab`).

## Error-conversion seam law

`@ conversion` (en) / `@ conversio` (la) marks one named standalone function,
the conversion-fn form, declaring one direct error-union pair: a source
error-union parameter and a destination error-union return (the compiler does
not check that both ends are unions; treat it as the convention). Each function
enrolls once in the shared conversion table, the same table the
compiler-shipped builtin rows use; it is a whole-source mapping seam, not a
general implicit conversion. The in-union `@ conversion` arm form is retracted
(a union declaration is pure fields plus variants). Working example:
`lib/norma/exempla/error-conversio/`.

```fab
union WireError {
    @ shared
    string message

    Invalid
}

union BoundaryError {
    @ shared
    string message

    WireFailure {
        WireError inner
    }
}

@ conversion
fn from_wire_error(WireError err) → BoundaryError {
    return variant WireFailure { message = err.message, inner = err }
}
```

- At an **administrative boundary** (alias or re-export), declare the
  conversion function once and let a failable call propagate directly. Delete
  the repeated catch-and-reconstruct wrapper family.
- At a **trust boundary**, keep explicit matching in the conversion-function
  body. Preserve, narrow, or reject source variants deliberately.
- Name the source parameter `err`. The compiler rejects `self` (la `ego`) outside a
  method (`ego_outside_method`), so a receiver spelling cannot serve as the
  whole-source binding in either locale.
- One row per concrete source→destination pair per scope. A duplicate pair is
  `conversio_registry_duplicate_pair`, a generic function cannot key a row
  (`conversio_generic_row`), and user rows may not shadow builtin rows. The
  function body is a pure re-presentation (no I/O or side effects) and stays
  callable by its declared name.
- The lookup is direct and fail-closed. A missing pair reports
  `failable_call_conversion_absent`; the table never composes a transitive
  A→B→C chain. A local `do { … } catch err { … }` (la `fac … cape`) sees the
  original source error.
- A failable `↦` or `↤` outside `main` must be handled: `⊥ default`, a local
  handler, or an enclosing `⇥ E` function (`SEM011 failable_conversio_requires_handler`;
  `main` is exempt and panics). A call into a `⇥ E2` caller converts only through
  a declared `@ conversion` row.
