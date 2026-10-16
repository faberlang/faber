---
name: canonical-faber
description: "Review existing Faber source for non-idiomatic patterns. Use when the user asks for a canonical-faber review, a style or idiom audit of .fab code, whether code is idiomatic Faber, or when accreting good/bad pattern pairs into the library."
---

# Canonical Faber

Review existing Faber source and flag patterns that are **not** the preferred
idiomatic form. This skill is the review pass; the parent `$faber` skill owns
building, writing, and canon routing. The pattern library accretes over time:
every review may grow it.

This skill is **prescriptive**: it says which patterns to follow and which to
avoid. It does not inventory what libraries currently contain, and usage
frequency alone is never the reason a pattern is preferred.

## Anti-prior law: say the shape, not the accumulator

LLM training data is saturated with C/Python softmax, RoPE, matmul, and
stride-walk code written without language-level tensor or collection
constructs. That prior produces accumulator variables, counter-`while` loops,
`while`+`for` pairs, nested counter walks, and managed-binding folds because
those languages have no streamlined forms. It is the wrong prior for Faber.
Faber puts the fold into the expression itself:
`reduce via sum from t at [i] let s { return term }` (the distributed
`thread f` clause is admitted only inside a kernel entry;
`corpus/summa/summa-en.fab`). It
walks Cartesian coordinates in one definition:
`for range 0‥m, 0‥n let i, j`. It walks aligned lists in lockstep:
`for from a, b let x, y`. It folds through closure intrinsics, whose
parameters are typed: `nums.reduce((int acc, int x) ∴ acc + x, 0)`. Prefer
the construct that states the operation's shape instead of reproducing its
imperative scaffolding.

| Hand-rolled prior | Faber construct |
| --- | --- |
| counter-`while` / managed counter walks | n-ary `for range` for Cartesian walks; aligned collections use lockstep `for from a, b let x, y` (`corpus/itera/lockstep-n2.fab`) |
| accumulator variable + loop | `.reduce((int acc, int x) ∴ expr, init)` — no managed binding; closure parameters are typed |
| transform loop building a new list | `.map(int x ∴ expr)` |
| predicate loop with appends | `.filter(int x ∴ pred)` |
| tensor reduction loops | `reduce via sum from t at [i] let s { return s }`, `reduce via max from scores at [i, j]` (one `at` name per axis) |
| best-index scan loops | `.argmax() coalesce 0` / `.argmin() coalesce 0` twins (`int ∪ none`; empty yields null) |

**Convert or record.** Hunt these families in existing source and convert
when a covering construct exists. If no construct covers the site, leave the
implementation in place with an in-code reason. Do not invent semantics or
force a conversion merely to remove a loop.

The mechanism is compiler lowering, not aesthetic compression. Language-level
constructs carry their shape into MIR and per-target kernel plans; for
example, tensor `·` is a `CollectionKernelPlan::TiledMatMul` in `radix-mir`
(`crates/radix-mir/src/kernel_plan/`). A hand-rolled loop freezes one
imperative shape and cannot reach those recipes, so the compiler can optimize
the language construct for the target better than the hand can. A library may
record the sites it deliberately keeps as `# legacy-L2:` / `# legacy-L3:`
comments stating the reason; those are recorded, not findings.

## Scope

- Input: existing `.fab` source — packages, exempla, library source, applications.
- Output: findings naming the non-idiomatic pattern, the preferred form, and
  the authority behind it — plus a library accretion note.
- Not this skill: grammar/type errors (compile findings), writing new code from
  scratch (`$faber`), compiler-source presentation (`$faberlang` reference).

## What counts as canonical

Evidence ladder, strongest first:

1. **Documented rules** — `$faber` Hard Rules and `../references/idioms.md`.
2. **Corpus and library exemplars** — `corpus/` (find a term by its
   frontmatter `explains`) plus named library files cited per entry
   (`lib/norma/src/…`).
3. **Library entries** — `references/patterns.md`, each marked `settled` or
   `tentative`.

A suspicion backed by none of these is an `observation`, never an asserted
finding. The live tree always wins over this library.

## Review procedure

1. **Locale first.** Resolve the code locale (CLI `--locale`, `+++`
   frontmatter, `faber.toml`, pack data) exactly as `$faber` requires. Never
   flag a spelling as non-idiomatic before locale resolution — surface
   spellings are pack facts, not style.
2. **Separate law from style.** Run `faber check <path>` on the target (from
   the repo root: `cargo run -p faber -- check <path>`; add `--diagnostics` for the
   message text). Compile errors are reported separately as compile findings;
   this review proceeds over the legal code. That includes the retired forms
   the compiler now rejects: `static`/`generis` module constants and class
   members (`PARSE010 static_decl_retired` / `static_member_retired`), `var`
   and run-time `const T x ← e` (`PARSE010 var_retired` /
   `runtime_const_retired`; write `mut` / `let`), a `const` binder
   (`binder_fixum_retired`), `numerus<W>`/`int<W>`/`fractus<W>`
   wrappers (`PARSE040 numeric_wrapper_retired`), `?:` ternary (`PARSE030`),
   `⇥ value` recovery (`PARSE030 recovery_uses_exit_arrow`), `∴` as a
   statement joint (`PARSE026`), `T ∪ null` (`PARSE040`), `T?`, anonymous
   `{ key = expr }`, `@ futura`, and `norma:hal/*` (`PKG001`). Hard-rule
   violations that still compile (banned or invented forms) stay in scope.
3. **Mechanical pass.** Walk every entry in `references/patterns.md` against
   the target. Settled entries are asserted; tentative entries are reported
   with their status.
4. **Ground novel suspects.** For a construct not covered by the library, look
   the term up by frontmatter `explains` in `corpus/`, read the nearby exemplar, and
   compare. Flag `non-preferred` only with a corpus citation; otherwise record
   an `observation`.
5. **Report.** For each finding: severity, `path:line`, offending snippet,
   preferred form rendered in the file's locale surface, and authority (rule
   citation or exemplar path). Close with the accretion note.

## Severities

- `canon-violation` — violates a documented rule the compiler still accepts:
  banned or invented forms (`cum`, `@ externa` / `@ subsidia` in library
  source, …). A retired form the compiler now rejects is a compile finding, not
  a canon finding.
- `non-preferred` — legal, but a documented preference or corpus-dominant form
  was not taken (block guard instead of the one-statement `then return`,
  Allman braces, `∪ none` for an omittable parameter instead of `optional`,
  a leading `_` on a private helper — `WARN030`, …).
- `observation` — suspected non-idiom with no backing yet; stated as a
  question and queued for accretion.

## Construct-choice pairs

`../references/idioms.md` (Semantic Tone) owns the distinctions — `discerne`
vs `elige`, `ordo` vs `discretio`, `de`/`in`/`ex`, `sit`/`varia`/`de`/`fixum` (en `let`/`mut`/`ref`/`const`),
`fiet`/`fiunt`/`fient`. When a review finds the wrong member of a pair in
use, accrete the concrete code pair into the library rather than restating
the tone list here. Two pairs the compiler now enforces or hints at:

- `match` (`discerne`) discriminates closed sets only — unions, enums, variant
  types. Literal and range dispatch is `switch` (`elige`); the pack help for
  `SEM010 discerne_pattern_not_in_closed_set` says so.
- Module constants are `const T X = e` with `=` (compile-time value; `←` there
  is `SEM062 top_level_binding`). A failable `↦` is covered by `⊥ default`, a
  local handler, or an enclosing `⇥ string` (`main` is exempt and traps):
  `SEM011 failable_conversio_requires_handler` otherwise.

## Accretion

After each review, update `references/patterns.md`:

- **Add** an entry when a pattern recurs, a maintainer confirms a taste, or a
  documented rule backs it. One-off suspicions stay out.
- **Prescriptions, not inventories** — an entry carries the preferred form,
  the anti form, and its authority. Usage counts and survey tables do not
  belong in this skill.
- **Entry shape**: `id`, one-line claim, severity, status, authority, `added`
  date, bad/good pair, optional scope.
- **Promote** `tentative → settled` on maintainer confirmation or clear corpus
  dominance; note what settled it.
- **Retire or fix** entries the live tree contradicts — verify cited exemplar
  paths still exist and still show the good form. Never keep a pattern the
  corpus rejects.
- **Do not accrete**: grammar facts (owned by `grammar/EBNF.md`), locale surface
  differences (pack facts, not style), or rule prose already owned by parent
  references (add only the new code pair).

Keep library edits in a separate commit from any code under review.

## Fast checks

- Locale resolved before any finding?
- Compile findings separated from style findings?
- Every finding cites a rule or a live corpus path?
- Preferred forms rendered in the file's locale surface?
- Library updated for genuinely new recurring patterns — and left alone for
  one-offs?

## Union errors: use commune access directly

When every variant carries the same error field, a handwritten `message()`
match wrapper is legal but non-canonical. The shared-field region already owns
that projection.

**Bad — repeated match wrapper:**

```fab
fn message(ShapeError e) → string {
    match e {
        case Incompatible let message { return message }
        case ElementMismatch let message, expected, actual { return message }
    }
}

return message(error)
```

**Good — direct commune member:**

```fab
return error.message
```

Authority: `corpus/commune/commune.fab`, which checks the shared field
on `ShapeError` and reads `e.message` directly. Do not invent a wrapper merely
to restate a field declared in the `@ shared` / `@ commune` region.

## Error conversion: wrapper family versus declared conversion function

At an administrative alias or re-export boundary, a repeated catch-and-
reconstruct wrapper is a canon violation of the seam law (`../references/operators.md`). Declare the direct
pair as one named standalone conversion function and let the failable call
propagate. The historical in-union arm form is retracted (schema 7): a union
is pure fields plus variants.

**Bad — wrapper family (anti-pattern):**

```fab
fn inspect_before() → string ⇥ BoundaryError {
    do {
        return read_wire()
    }
    catch err {
        throw variant WireFailure {message = err.message, inner = err}
    }
}
```

**Good — declared conversion function (idiom):**

```fab
@ conversion
fn from_wire_error(WireError err) → BoundaryError {
    return variant WireFailure {message = err.message, inner = err}
}

union BoundaryError {
    @ shared
    string message

    WireFailure { WireError inner }
}

fn inspect_after() → string ⇥ BoundaryError {
    return read_wire()
}
```

The `err` parameter names the whole source value. Do not write `self` for this
binding in English: the keyword registry maps canonical `ego` to English `self`,
and `self.message` outside a method is rejected. The Latin `la` pack maps
`ego = "ego"`; `self` is not reserved there, but `err` is the safe teaching
name across locales. Trust boundaries still require an explicit `match err` in
the function body. One row per concrete source→destination pair; duplicates,
builtin shadowing, both-ends-foreign pairs, and impure bodies are all
violations. Missing direct pairs fail closed with
`failable_call_conversion_absent`; no transitive chain is inferred.

Authority: `lib/norma/exempla/error-conversio/src/error_conversio.fab` is the
small runnable pair.

## Container walks preserve the static bound

A range bound re-derived from a container's shape and then fed into stride
indexing loses the container's static fact. This is a canon finding about lost
information, not a style preference.

**Bad — coalesce-fed range bound plus stride indexing over flat data:**

```fab
let list<int> f ← x.shape
let int rows ← f.get(0) coalesce 1
let int cols ← f.get(1) coalesce 1
for range 0‥rows let i {
    …
    let f32 v ← x.data.get(i * cols + j) coalesce 0.0
}
```

**Good — direct container walk:**

```fab
for from grid at [r, c] let cell {
    if r ≤ c {
        print r, c, cell
    }
}
```

Why: bounds belong to the container; conditions belong in the body. The bad
form severs the loop from the thing it iterates, making stride math
unverifiable, preventing AIR unroll because the unroll machinery is fail-closed
on non-literal extents, and leaving OOB access writable through `coalesce`.
This review finding is specifically for range bounds that re-derive a
container extent and manually index that same container; genuinely count-like
ranges remain correct.

Authority: `corpus/itera/apud-rank2-matrix.fab` uses the direct rank-2 walk,
`corpus/itera/apud-index-causal.fab` puts the condition in the body, and
`corpus/itera/apud-decline.fab` records the fail-closed
shaped-iterable and non-concrete-lowering boundaries.

## Idiom wave 2026-08-26: say the walk, not the counter

Distilled from a library cleanup wave (2026-08-26). The
theme: hand-rolled control flow and ad-hoc predicates replaced by the
language's direct forms — `for range` over counter-`while`, `for from xs` over
range-plus-get re-indexing, `∈` / `contains` over or-nests and scan
helpers, module-level `set` consts over per-call rebuilds, `# ---` stage
banners over loose prose comments, and `@ kernel` on glyph-composed tensor
functions. Every pair below was distilled from a real conversion,
not an aspiration. The
anti-pattern direction is always "write the right form instead", and the
keep-class entries below are the cleanup's own recorded boundary — flagging
a keep-class site for conversion is as wrong as leaving a convert-class site
on the counter form.

The tree has moved since. Cited helpers lost their leading `_` (`WARN030`,
unmarked top-level declarations are already private). Snippets are trimmed to the
pattern, with `∷` ascriptions that are now redundant and `0 - 1` negation
(`-1`) modernized.

### 1. Known-span count loops are `for range`, never counter-`while`

**Bad — index variable + `while` + manual increment (`matmul`):**

```fab
for range 0‥m let i {
    mut int j ← 0
    while j ≺ n {
        mut f32 s ← 0.0
        mut int p ← 0
        while p ≺ k {
            let f32 va ← a.data.get(i * k + p) coalesce 0.0
            let f32 vb ← b.data.get(p * n + j) coalesce 0.0
            s ← s + va * vb
            p ← p + 1
        }
        out.append(s)
        j ← j + 1
    }
}
```

**Good — the span is declared, not maintained:**

```fab
for range 0‥m let i {
    for range 0‥n let j {
        mut f32 s ← 0.0
        for range 0‥k let p {
            let f32 va ← a.data.get(i * k + p) coalesce 0.0
            let f32 vb ← b.data.get(p * n + j) coalesce 0.0
            s ← s + va * vb
        }
        out.append(s)
    }
}
```

Sub-forms:

- Inclusive bound becomes an exclusive end `+ 1`: `mut int k ← 0; while k ≤ k_max { …; k ← k + 1 }` → `for range 0‥(k_max + 1) let k`.
- Non-zero start rides the range: `mut int k ← 7; while k ≺ parts.length() { … }` → `for range 7‥parts.length() let k`.
- Unused binder is `_`: `mut int j3 ← end; while j3 ≺ cols { out.append(0.0); j3 ← j3 + 1 }` → `for range end‥cols let _ { out.append(0.0) }`.

This wave is the mass live-tree conversion behind `references/patterns.md`
`range-loop-over-open-while`.

### 2. A two-collection bound hoists the minimum, then takes one range

**Bad — compound `while` predicate over two lengths:**

```fab
mut int i ← 0
while i ≺ target_ids.length() and i ≺ candidate_ids.length() {
    if (target_ids.get(i) coalesce -1) ≢ (candidate_ids.get(i) coalesce -1) {
        return GreedyAcceptance {accepted_prefix = i, divergence = i}
    }
    i ← i + 1
}
```

**Good — compute the bound once, walk one range:**

```fab
mut int bound ← target_ids.length()
if candidate_ids.length() ≺ bound {
    bound ← candidate_ids.length()
}
for range 0‥bound let i {
    if (target_ids.get(i) coalesce -1) ≢ (candidate_ids.get(i) coalesce -1) {
        return GreedyAcceptance {accepted_prefix = i, divergence = i}
    }
}
```

Write the hoisted
`bound` binding plus `for range 0‥bound`, not a two-clause `while`.

### 3. Keep-class: `while` stays for open predicates, shrinking searches, and non-+1 strides

The wave did **not** convert these, and the commit names the rule ("Open
predicates, shrinking searches, and stride-2 walks stay while"):

```fab
# ✓ kept while — shrinking search bound
while length ≥ MIN_SUFFIX_LENGTH {
    …
    length ← length - 1
}

# ✓ kept while — column walk with a stride-2 arm
mut int col ← 0
while col ≺ d {
    …
    col ← col + 2
}
```

Flagging these for `for range` conversion is a false finding. A `while` whose
bound advances by computed strides or shrinks inside the body has no range
spelling; a `while` that exists to re-derive a `+1` counter does.

### 4. One-collection walks are `for from xs`, not range-plus-get

**Bad — the range exists only to `get` the same collection:**

```fab
for range 0‥checkpoint.layers().length() let i {
    let kv.KVCache layer ← checkpoint.layers().get(i) coalesce kv.default()
    let int layer_remaining ← layer.capacity() - checkpoint.prefix_length()
    …
}
```

**Good — walk the collection directly; a method call is a legal source:**

```fab
for from checkpoint.layers() let layer {
    let int layer_remaining ← layer.capacity() - checkpoint.prefix_length()
    …
}
```

An index test for "not the first element" converts to a
flag under the element walk: `if i ≻ 0 { out ← out + "," }` inside `for range 0‥n` becomes
`mut bool first ← true` with `if not first { … }; first ← false` inside
`for from f let dim`. This is the flat-carrier sibling of the rank-2
container-walk rule above.

### 5. Keep-class: `for range` stays when the index is the value, on zips, and on bytes

The leftover class is correct, not missed:

- **Index is identity** — the index feeds a lookup that is not `xs.get(i)` on the same list (e.g. `tokenizer.is_eog(i)` vocab ids, best-index returns).
- **Write-back** — the index writes into a collection, or feeds arithmetic on a different carrier.
- **Bytes walks** — `for from o` over `bytes` now checks and binds `u8`, but the MIR runner rejects it (`unsupported MIR lowering: itera collection before iterator MIR lowering`, probed 2026-10-01); a byte-list helper keeps `for range` + `get(i) coalesce 0 ∷ u8`.

Aligned collections are no longer keep-class: the lockstep walk
`for from a, b let x, y` (landed 2026-08-26, `corpus/itera/lockstep-n2.fab`,
`lockstep-n3.fab`; equal lengths are a fail-closed law) is the spelling for
parallel-list comparisons once recorded as `legacy-L2`.
Flag a hand-indexed zip over two lists whose lengths are equal by contract.

Zero remaining *convert-class* sites was the closeout, not zero `.length()`
hits. Report a range-plus-`get` walk only when the index re-indexes that one
same collection.

### 6. Charset membership is `contains` or `∈ [..]`, not an or-nest

**Bad — character-equality or-chain:**

```fab
fn _digit(string c) → bool {
    return ((((((((c ≡ "0" or c ≡ "1") or c ≡ "2") or c ≡ "3") or c ≡ "4") or c ≡ "5") or c ≡ "6") or c ≡ "7") or c ≡ "8") or c ≡ "9"
}
```

**Good — substring test against the charset string (respelled):**

```fab
fn _digit(string c) → bool {
    return "0123456789".contains(c)
}
```

Membership never applies to text (`SEM010 membership_over_text`), so the
charset-string form is `.contains` (la `continet`); the original used the
now-retired `between` keyword. The set may be a literal list instead, with the
membership glyph: `c ∈ [" ", "\t", "\n", "\r"]`. Whole-string
predicates compose `for from` with the cast: `for from s let c { if not
"0123456789".contains(c ↦ string) then return false }`.

### 7. List membership is `x ∈ xs`, not a scan helper

**Bad — hand-rolled contains loop:**

```fab
fn _in_name(string v, list<string> names) → bool {
    for from names let n {
        if v ≡ n then return true
    }
    return false
}
```

**Good — the membership predicate inline:**

```fab
fn _in_name(string v, list<string> names) → bool {
    return v ∈ names
}
```

Delete `_contains` scan helpers and inline membership at `require` sites:
`require "general.architecture" ∈ keys throw variant BadArchitecture
{…}`. Likewise `return idx ∈ seen` and `return name ∈
support_flags()`. Write the `∈` form (the original used the retired
`between` keyword; `∉` is the first-class negation); do not carry a
per-module membership helper.

### 8. Reserved-character scans are `s.contains`, not per-character walks

**Bad — walk the string to find one separator:**

```fab
fn _has_no_separator(string s) → bool {
    for from s let c {
        if (c ↦ string) ≡ "/" or (c ↦ string) ≡ ";" then return true
    }
    return false
}
```

**Good — the string's own membership test (respelled):**

```fab
fn _contains_separator(string s) → bool {
    return s.contains("/") or s.contains(";")
}
```

The same conversion gives `return s.contains("/")` and `return not
s.contains(IDENTITY_SEPARATOR)`. A loop whose only job is "does this
substring appear" is `contains`.

### 9. Constant key sets are data, not per-call rebuilds or if-ladders

**Bad — the set is rebuilt inside the predicate:**

```fab
fn key_value(string k) → bool {
    let set<string> admitted ← [
        "general.architecture",
        …37 pinned keys…
    ] ↦ set<string>
    return k ∈ admitted
}
```

**Good — the key universe is a named declaration and membership stays `∈`:**

```fab
fn admitted_keys() → set<string> {
    return [
        "general.architecture",
        …37 pinned keys…
    ] ↦ set<string>
}

fn key_value(string k) → bool {
    return k ∈ admitted_keys()
}
```

An earlier form hoisted the set to a module-level `const`; that spelling no longer
compiles. A module constant must be compile-time evaluable, and a
`↦ set<string>` conversion is not (`SEM060 constant_initializer_not_constant`,
probed 2026-10-01; the `static` form was retired by `PARSE010`). The live
forms are a module-level `const list<string> NAMES = [ … ]` (a plain list
literal is a constant; `const T X = e`, with `=`) read through `k ∈
NAMES`, or a zero-argument function returning the set when a `set<T>` value is
required (with a comment recording the reason). Flagging that function form for conversion
is a false finding.

The if-ladder is the same defect in another spelling: 22 lines of `if key ≡
"qwen35moe.block_count" then return true …` collapsed into one key universe
plus `return key ∈ required_qwen_keys()`. A frozen membership
universe is data, never control flow inside the predicate.

### 10. Multi-stage pipelines are gated by `# ---` section banners

**Bad — bare prose comments marking stages:**

```fab
    # Pre-LN
    let ln1 ← x.layer_norm(1, 0.00001, ln1_s, ln1_o)

    # Q/K/V projections (per-channel [8] biases, S6-C2 added_bias)
    let q ← ln1 · wq
```

**Good — the banner form, validation stages marked fail-closed:**

```fab
    # --- pre-LN -----------------------------------------------------------
    let ln1 ← x.layer_norm(1, 0.00001, ln1_s, ln1_o)

    # --- Q/K/V projections (per-channel [8] biases) -----------------------
    let q ← ln1 · wq
```

The rule was applied across a library's model pipelines in several waves, and
newly written routes carried it from birth. Conventions: the banner
is `# --- <short stage label> ---…---` padded to a fixed width; a stage whose
content is `require` validation carries the suffix `(fail closed)`
(for example `# --- scalar invariants (fail closed) ---…`); legacy `# --` banners normalize to `# ---`
and stray inter-stage prose folds into the banner label. A long function with
ungated stages is the finding; the fix is the banner, not a rewrite.

### 11. Glyph-composed tensor functions carry `@ kernel`

**Bad — a tensor function whose body is pure glyph composition left unannotated:**

```fab
@ public
fn linear<size M, size K, size N>(tensor<f32, [M, K]> x, tensor<f32, [K, N]> w, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return (x · w) + b
}
```

**Good — `@ kernel` above the export marker once the body is glyphs:**

```fab
@ kernel
@ public
fn linear<size M, size K, size N>(tensor<f32, [M, K]> x, tensor<f32, [K, N]> w, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return (x · w) + b
}
```

A kernel-purity census annotated zero-body-change twins the same
way, and the typed leaves took the tag at birth. Ordering in every
case: `@ kernel` first, `@ public` second. A staged-carrier body that walks
flat lists is *not* a kernel — the census annotated only glyph bodies
(`let kt ← kbᵀ; let scores ← qb · kt`), and `gather_carrier` /
`transpose_carrier` are deliberately untagged
(commented "Not @ kernel: no gather glyph or method twin yet").

### 12. Staged carriers pin into dimensional tensors through `do`/`catch`, and shared carriers replace local reimplementations

**Bad — a local, module-private reimplementation of an operation the library already carries (a local `transpose`):**

```fab
fn transpose(tensor.NumericBlock t) → tensor.NumericBlock ⇥ DenseError {
    let list<int> f ← t.shape
    …
    for range 0‥cols let j {
        for range 0‥rows let i {
            out.append(t.data.get(i * cols + j) coalesce 0.0)
        }
    }
    return tensor.construct(out, [cols, rows])
}
```

**Good — call the shared carrier, then pin:**

```fab
fn transpose(tensor.NumericBlock t) → tensor.NumericBlock ⇥ DenseError {
    return nn.transpose_carrier(t)
}
```

The pin idiom from the same arc — a staged carrier's flat `data` becomes a
dimensional tensor inside `do`/`catch conv`, with the typed variant thrown on
failure (`pin_rank1` / `pin_rank2`):

```fab
fn pin_rank1<size N>(tensor.NumericBlock t, string name) → tensor<f32, [N]> ⇥ DenseError {
    do {
        return t.data ↦ tensor<f32, [N]>
    }
    catch conv {
        throw variant BadShape {message = "canonical tensor § pin failed"(name)}
    }
}
```

An earlier boundary — a size-generic pin on a `⇥ DenseError` function is
SEM016, so pin only concrete shapes — no longer holds: size-generic pins
check today (`pin_rank1<size N>` above; probed 2026-10-01). Duplicate
flat-walk implementations of an operation a sibling module already exposes as
a carrier are the finding; the fix is the shared carrier plus (when a
dimensional type is needed) the pin.

### 13. `en`-locale files spell identifiers in English

The wave respelled leftover Latin in an `en`-locale package: the
parameter `causalis` → `causal`, the error variant `TerminusExcedit` →
`IndexOutOfBounds`, and the field `ligatum` → `tied` (dense config). Do not
flag deliberate paper-Greek identifiers (`γ`, `ε`, `θ` — see
`greek-paper-identifiers`); the finding is Latin-*word* residue in an `en`
file, and the fix is the plain English identifier. Locale surfaces are sealed
per file — `en` and `la` spellings never mix in one file.

Authority for this section: the library conversions summarized above
(while→range, collection walks, membership/contains, module-level sets,
section banners, the kernel census, typed assembly, English respells). The
`between` spelling it used is retired (see 6 and 7), as is the module-level
set spelling (see 9).

### 14. Mixed `⇥` functions split: wrapper vs kernel region

Section 11 covers a body that is already pure glyphs. This section covers the
mixed function: admit/policy and tensor math in one `⇥` signature. That
function cannot be `@ kernel`. Cut it. Full pair and decision tree:
`references/patterns.md` `kernel-region-split` (settled,
2026-08-28).

**Bad — policy and math share the `⇥` function, so the math never becomes a
device region.**

**Good — wrapper keeps `require` / bags / `source` / optional-bias presence;
math reuses existing `@ kernel` leaves, or a named `@ kernel` leaf (`@ public` when host code calls it; a private kernel
called from host is `SEM008 nucleum_private_host_call`) when a second caller
will share it. The kernel-closure modifier is ruled for retirement (K7b,
2026-09-30); do not add new uses.**
An illegal nested call becomes an argument, not a poison. Session drivers stay
ordinary. A conversion that does not change the exported launch plan must not
claim a launch-count delta.

This supersedes the earlier "assemblers stay unannotated / no
in-body call" guidance for fusion parents: parents that call leaves are the fusion
surface; composition inlines those calls. Flag a `⇥` function whose body is
already only kernel/glyph math as convert-class. Flag marking that `⇥`
function `@ kernel` as a false fix.
