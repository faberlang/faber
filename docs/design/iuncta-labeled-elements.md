# iuncta labeled elements

**Status**: design locked 2026-08-20 (operator session)
**Ruling authority**: [`radix/docs/factory/iuncta-labeled-elements/goal.md`](../../../radix/docs/factory/iuncta-labeled-elements/goal.md)
**Grammar**: [`docs/EBNF.md`](../EBNF.md) Types — `labeledTypeArgument`

Tuple element labels are a per-element, identity-erased alias layer. The six
ruling clauses and the four adopted open-question defaults below are recorded
verbatim from the goal.

---

## Ruling

The locked ruling (six clauses; surface sketch, not implementation):

1. **Surface** — optional label prefix on tuple type arguments:
   `iuncta<loss: f32, T>`. Per-element; mixed labeled/unlabeled legal. No
   `_: T` "unlabeled" spelling — absence is the only unlabeled form. Labels
   unique within a tuple. Keyword spellings are legal labels under the
   contextual law (`iuncta<fixum: A>`; `i.fixum` is ordinary member access).
2. **Type identity** — labels erased: identical types for assignment, `≡`,
   `↦`, and every emitter. Labels live as a checker-side position-keyed table.
   No structural record semantics; `genus` untouched; the Stage 6 anonymous-object
   retirement stays closed.
3. **Access** — `i[0]`: literal-integer bracket index on tuples, always legal,
   every element, labeled or not. Non-literal index expressions remain rejected
   (positions must be compile-time facts). `i.gx`: member access by label, legal
   iff a label is declared at that position. No `.0` — the member suffix stays
   `.` IDENTIFIER; positions are brackets only. `.gx` lowers to the identical
   constant-index node as `i[0]` and positional destructuring (zero runtime
   cost, zero emitter surface).
4. **Destructuring** — by-position `fixum [gx, gw] ← i` (existing) and
   by-label `fixum {gx, gw} ← i` riding the existing `objectPattern`
   production. arrayPattern is the positional face, objectPattern the named
   face, of one construct.
5. **Holes** — `iuncta<f32, _>` legal: monomorphic element hole solved
   element-wise from the single position witness (construction argument or
   callee return element). `iuncta<f32, ∪>` rejected statically with a
   targeted diagnostic (monomorphic witness slot; same ruling family as
   `explicit_union_type_arg_unsupported`); a wanted union is declared with
   binary cup (`iuncta<f32, textus ∪ nihil>`). Labels compose freely with
   holes (`iuncta<loss: _, T>`).
6. **Unchanged** — ordering, positionality, and the arity cap of 16.

---

## Adopted open-question defaults

Open questions 1–4 carry recorded defaults and are adopted.

1. **Diagnostic issue slugs** for `∪`-in-`iuncta` rejection and unknown-label
   member access. Default: follow the `*_unsupported` / `unknown_*` families.
2. **By-label destructuring exhaustiveness** — must a `{gx, gw}` pattern bind
   every label, or is partial binding legal (with `ceteri` rest, as
   `objectPattern` already allows on genera)? Default: mirror `objectPattern`
   on genera — partial legal, `ceteri` binds the rest as the sub-tuple value.
3. **Mixed positional/label patterns** in one destructuring (`fixum [gx, {gw}]
   ← i`). Default: reject until asked.
4. **Arity cap as declared law** — making the 16 cap grammar-declared metadata
   instead of a parser magic constant is real but separate. Default: separate
   goal when the grammar-source dialect lands.
