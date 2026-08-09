# Delivery: Formal Object-Rooted JSON Document

**Status**: complete — factory gate passed 2026-07-09
**Date**: 2026-07-09
**Campaign**: [`CAMPAIGN.md`](CAMPAIGN.md)
**Primary repos**: `radix`, `faber-runtime`, `norma`, `examples`
**Direct integration repo**: `faber` only if package/runtime dependency metadata changes
**Factory checkpoint**: inline JSON, typed JSON genera, wire parsing/rendering,
and checked dynamic conversion share one `json` contract.

## Interpreted Unit

Introduce a formal Faber `json` type for an object-rooted JSON document. Preserve
`valor` as the broader dynamic/frame carrier. Redirect the already-shipped JSON
literal tree, JSON-genus safety proof, `valor` conversion machinery, runtime
carrier, and Norma facade so that proof is no longer erased at each boundary.

This is a public clean break:

- bare `{ "key": ... }` literals infer `json`, not `valor`;
- `norma:json.solve` returns `json` and rejects array/scalar roots;
- `norma:json.tempta` returns `json ∪ nihil`;
- `norma:json.pange` accepts `json`, not `valor`;
- arbitrary `valor` requires checked `↦ json` narrowing;
- only `@ json` genera convert directly to/from `json`.

The implementation must not add a second JSON tree. The existing compile-time
`JsonValue` syntax tree remains a literal representation; the runtime type is a
validated wrapper over `Valor`.

## Normalized Spec

### Public spelling decision

The canonical source spelling is **`json`**. The Rust runtime type is
`faber::Json`.

Reasons:

- `json` is already canonical in `@ json` and `norma:json`;
- Faber primitive type spellings are lowercase single words;
- `documentum` would claim a generic document concept rather than the JSON
  contract;
- type-position primitive resolution and expression/module resolution are
  separate compiler responsibilities, so this is unambiguous and must be
  proven by an exemplum containing both `json documentum` and `json.solve(...)`;
- no invented pseudo-Latin word is needed.

### Core invariant

A value of type `json` always owns one object-rooted, recursively JSON-safe
tree. Every safe constructor preserves that invariant. No unchecked public
constructor can create `json` from arbitrary `valor`.

### Runtime representation decision

Add `faber::Json` as a private-field wrapper around `Valor`:

```rust
pub struct Json(Valor);
```

The field stays private. Checked construction requires:

1. root variant is `Valor::Tabula`;
2. descendants contain only `Nihil`, `Bivalens`, `Numerus`, finite `Fractus`,
   `Textus`, `Lista`, and `Tabula`;
3. `Octeti` is rejected;
4. tagged `Instans` is rejected (an `instans` in a JSON genus boxes to RFC3339
   `Textus` instead);
5. every nested list/object is checked recursively.

The wrapper is preferred over a second recursive enum because the runtime,
frame model, genus extractor, and MIR stepper already exchange `Valor`. The
private field and one validator are sufficient to preserve the stronger
invariant while keeping `json → valor` lossless and allocation-light.

Expose focused operations rather than the inner field:

- checked `TryFrom<Valor>` with a structured path-aware error;
- infallible consuming/borrowed widening to `Valor`;
- object access needed by generated extraction without permitting mutation to
  invalid descendants;
- strict RFC parse and compact deterministic render helpers used by target
  lowering;
- an internal/asserting constructor for compiler-proven literal trees only if
  it still validates in debug/test builds. Do not make an unchecked constructor
  public to generated crates.

### Conversion matrix

`\u21a6` remains runtime conversio. `\u2237` must not assert dynamic JSON shape.

| Source | Target | Contract | Failure |
| --- | --- | --- | --- |
| inline `{ ... }` | `json` | compiler-proven object literal | none for a successfully compiled literal |
| `json` | `valor` | explicit lossless widening | infallible |
| `valor` | `json` | validate root and every descendant | failable |
| `textus` | `json` | strict RFC 8259 parse + object-root check | failable |
| `json` | `textus` | deterministic compact wire rendering | infallible |
| `@ json genus` | `json` | use validated field metadata and JSON-safe boxing | failable only for runtime value constraints such as non-finite floats |
| `json` | `@ json genus` | schema extraction with `nomen`, nested collections/genera, required/default policy | failable |
| non-JSON genus | `json` | unsupported | compile-time error |
| `valor` | non-JSON genus | existing broad dynamic extraction | unchanged by this goal |

No implicit `valor → json` assignment is permitted. Do not alias the two
primitives in `TypeTable::assignable`. JSON-to-valor may remain an explicit
conversion even though it is infallible; this keeps dynamic widening visible at
frame and application boundaries.

### Wire-number contract

- integer tokens without `.`/`e`/`E` must fit Faber `numerus` (`i64`) or fail;
- fractional/exponent tokens must parse to finite `fractus` (`f64`) or fail;
- rendering never emits `NaN`, positive infinity, or negative infinity;
- no lossy large-integer fallback to `f64` is allowed.

The Rust parser may use `serde_json`, but it must preserve numeric token shape
and reject duplicate keys. Default `serde_json::Value` map parsing is
insufficient because it overwrites duplicates and may obscure number intent;
use a custom visitor/seed or equivalent proof-grade path.

### Syntax and source-literal contract

The existing JSON island grammar remains object-rooted and constant-only:

- quoted keys, `:` separator, JSON wire tokens, nested objects/arrays;
- trailing commas remain a source-literal convenience;
- duplicate keys remain compile-time errors;
- root `[...]` remains a Faber `lista`, never a JSON document;
- `Type { field = value }` remains genus construction.

Rename compiler symbols that falsely claim the literal is `valor` when doing so
improves phase truth (`JsonValor` → `JsonDocument` or local equivalent), but do
not churn the parser tree merely for terminology. The semantic type must be
`Primitive::Json` from typecheck onward.

### JSON-genus contract

Keep the shipped semantic allowlist, with these corrections:

- boxing to `json` is available only for a genus recorded as `@ json`;
- extraction from `json` is available only to `@ json` genera;
- `@ json { nomen = "wire_name" }` selects the key in **both** directions;
- nested JSON genera apply their own field metadata recursively;
- `instans` boxes as canonical RFC3339 `Textus`, then extracts through the
  existing precision-aware path;
- `fractus` is checked for finiteness during genus-to-json conversion;
- collection conversion is atomic: one invalid child fails the whole document;
- required, `sponte`, nullable, and explicit-default fields keep the existing
  genus extraction policy unless tests show it conflicts with the JSON-genus
  contract. Missing required fields must never be silently zeroed.

The last point requires auditing `valor_conversio/extract.rs`: broad
`valor ↦ genus` currently gives several primitive fields natural-zero defaults.
For `json ↦ @ json genus`, requiredness must follow the source declaration,
not the broad compatibility extractor's historical convenience.

The JSON-specific missing/null policy is:

| Field declaration | Missing key | Present `null` |
| --- | --- | --- |
| plain `T` | fail at the field path | fail unless `T` is `nihil` |
| `T` with an explicit initializer | evaluate the initializer | treat as a supplied value; fail if `T` is non-nullable |
| `sponte T` | produce absent/`None` | produce absent/`None` when represented as optional |
| `T ∪ nihil` without `sponte` | fail; nullable is not optional | produce `nihil`/`None` |

Unknown object keys are ignored during genus extraction so a declared genus is
a typed projection and provider payloads may add fields. Duplicate wire keys
are still rejected by text parsing, and duplicate `nomen` values are rejected
at compile time. Exact-object/deny-unknown policy is a future explicit
annotation, not an accidental default.

### Norma public contract

Keep Norma as the public wire facade, but remove its incomplete hand-written
tree implementation from the authority path. The current source parser cannot
decode general Unicode escapes and the serializer can drop unnamed control
characters; preserving it would make Rust application behavior disagree with
the formal type.

Use the same pattern as `norma:tempus.solve`:

```fab
functio solve(textus wire) → json ⇥ textus {
    redde wire ↦ json
}

functio tempta(textus wire) → json ∪ nihil { ... }

functio pange(json documentum) → textus {
    redde documentum ↦ textus
}
```

`pange` becomes compact and infallible because `json` already excludes every
non-renderable value. Remove the currently ignored optional `indentum`
parameter instead of preserving a false pretty-printing contract. Pretty JSON
can return later as a distinct, actually implemented policy.

The old `valor` signatures are deleted in the same change; no overloaded or
compatibility facade remains. Goal 3 updates broader callers.

### Target posture

- Rust application: full support through `faber::Json`.
- MIR stepper: full semantic parity; use the runtime `Json` parser/validator or
  one shared conversion implementation instead of a divergent permissive
  `valor_to_json` path.
- Faber emitter/formatter: round-trip `json` type and literal syntax.
- TypeScript/Go: either implement the same wrapper/root checks or fail closed
  with target diagnostics. Do not lower `json` to unconstrained `any`/map and
  claim parity.
- wasm/LLVM/Metal/WGSL and other systems targets: fail closed until their type
  capability tables deliberately support `json`.

### Non-goals

- No top-level array/scalar JSON document type.
- No arbitrary-root `json_value` public type.
- No null omission, custom encoders, serde-style hooks, or implicit genus
  serialization.
- No compatibility alias from `json` to `valor`.
- No Goal 3 application-wide migration beyond direct canonical callers and
  the new exemplum.

## Repo-Aware Baseline

### Facts

- `EBNF.md` currently states that bare JSON objects have type `valor`.
- Syntax already stores a recursive constant `JsonValue` tree with object,
  array, scalar, and key-span data.
- Typecheck has one `check_json_valor_literal` path that defaults to
  `Primitive::Valor` and special-cases `tabula<K,V>` ascription.
- `Primitive::Valor` is assignable from scalars/lists/maps and maps to
  `faber::Valor`; no formal JSON primitive exists.
- `faber-runtime::Valor` also carries `Octeti` and tagged `Instans`, allows any
  root, and permits non-finite `Fractus` values.
- `@ json genus` already validates a recursive JSON-safe type subset and boxing
  already honors `nomen`.
- genus extraction still looks up the source field name, so `nomen` is
  asymmetric.
- Norma `solve` returns arbitrary-root `valor`; its Unicode escape support is
  explicitly incomplete. Norma `pange` accepts broad `valor`, is failable, and
  silently skips some control characters.
- the MIR JSON kernel accepts arbitrary roots and serializes tagged
  `Instans`/`Octeti`, which violates the proposed formal contract.
- Rust, TypeScript, and Go currently have independent JSON-literal emitters;
  MIR has its own aggregate lowering and stepper bridge.

### Architectural correction

The parser already owns syntax, the JSON-genus pass already owns schema safety,
`Valor` already owns the dynamic tree, and conversio already owns failable
runtime conversion. The delivery connects those seams:

```text
source literal ─┐
textus parse ───┼─> validated faber::Json ─> valor widening / wire text
@json genus ────┘             │
                              └─> @json genus extraction
```

No layer reparses source text or guesses schema from a runtime tree.

## Stage Graph

```text
J1 contract lock
      │
      v
J2 runtime Json + tests
      │
      ├──────────────> J3 compiler primitive/literal/type gates
      │                              │
      └─> J4 conversio + JSON genus ─┤
                                     v
                         J5 Norma facade + MIR parity
                                     │
                                     v
                         J6 corpus/docs/closeout
```

| Stage | Entry condition | Output | Exit gate |
| --- | --- | --- | --- |
| J1 — Lock contract | delivery accepted | `json` spelling, conversion matrix, root/value/number rules recorded in EBNF/design docs. | No unresolved representation or public-signature decision. |
| J2 — Runtime foundation | J1 | Private-field `faber::Json`, path-aware validation errors, strict parse, compact render, conversion tests. | Invalid roots/variants/non-finite/duplicates fail; valid nested trees round-trip. |
| J3 — Compiler type and literal | J1 + J2 | `Primitive::Json`, reader pack row, typecheck, HIR/MIR/type rendering, inline literal typing, fail-closed targets. | Bare object infers `json`; `json` and `valor` remain distinct. |
| J4 — Conversion convergence | J2 + J3 | Full matrix in semantic checking, Rust codegen, MIR stepper; one JSON-specific genus boxing/extraction policy with symmetric `nomen`. | Canonical genus round-trip and all negative conversions pass. |
| J5 — Norma and direct callers | J4 | `norma:json` delegates to formal conversions; old broad signatures/parser approximations removed; direct compiler/Norma fixtures updated. | Object roots parse/render; scalar/array roots reject identically in Rust and MIR. |
| J6 — Public proof | J5 | canonical corpus exemplum, EBNF/reader/docs/capability matrix, regenerated corpus index, release note decision. | Cross-repo gates green and no stale `JSON valor literal` public claim remains. |

J2 and the documentation part of J1 may be isolated in the runtime/Radix repos,
but J3–J5 are one convergence checkpoint. Do not stop with literals or genera
still typed as `valor`.

## Implementation Work

### Workstream A — Runtime representation and wire mechanics (`faber-runtime`)

Primary surfaces:

- `src/valor.rs` or a focused new `src/json.rs`;
- `src/lib.rs`, tests, and `Cargo.toml` if strict parsing needs a dependency.

Acceptance:

- validator reports a structural path such as `$.items[2].payload`, not only a
  boolean;
- duplicate keys and number representation are tested at text parse time;
- `Json -> Valor -> Json` preserves the tree;
- `Valor::Instans`, `Valor::Octeti`, any non-object root, and any nested
  non-finite float fail;
- compact rendering is deterministic.

### Workstream B — Semantic type and target capability (`radix`)

Primary surfaces:

- semantic primitive/type tables and reader type vocabulary;
- literal typecheck and conversio admission;
- HIR/MIR lowering, stepper values/conversio, Rust/Faber emitters;
- target capability classification and structured diagnostics.

Acceptance:

- `json` cannot accept generic parameters and is not an alias;
- type equality, assignability, imports, formatter output, and interface
  snapshots preserve it as a distinct primitive;
- existing `tabula<K,V>` ascription remains a map operation, not `json`;
- unsupported targets reject before partial code emission.

### Workstream C — Unified JSON-genus conversion (`radix`)

Primary surfaces:

- `semantic/passes/typecheck/json_genus.rs`;
- `codegen/rust/expr/valor_conversio/{boxing,extract}.rs` or a renamed shared
  structured conversion module;
- JSON-genus tests.

Acceptance:

- broad `T ↦ valor` stays broad and separate;
- JSON-specific boxing can only start from an `@ json` genus;
- extraction uses `struct_field_valor_key`/one equivalent in both directions;
- nested maps/lists/genera and nullable/default fields are atomic;
- required missing fields fail; non-finite fields fail.

### Workstream D — Norma wire facade (`norma`)

Primary surfaces:

- `src/json.fab`, `src/json/solve.fab`, `src/json/pange.fab`;
- JSON exempla/docs and source checks.

Acceptance:

- public source contains only the facade/lenient wrapper needed over formal
  conversions;
- old partial Unicode/control-character paths are deleted, not wrapped;
- no public `pange(valor)` or `solve(...) -> valor` remains.

### Workstream E — Public corpus (`examples` + Radix harness)

Add one canonical exemplum that proves in one source file:

- `importa ex "norma:json" privata json` coexists with `json` in type position;
- inline construction and nested object/array values;
- `@ json genus` with `nomen`, nullable/default, nested genus, and collections;
- genus → json → text → json → genus round-trip;
- explicit `json ↦ valor` and checked `valor ↦ json`;
- invalid root/dynamic narrowing is covered by negative compiler/runtime tests.

Parallelism: A can proceed after J1 while B adds the semantic primitive on a
separate repo. C waits for both. D waits for the conversion surface. E may draft
after J1 but cannot be validated until D.

## Checkpoints And Gates

### Batching / split decision

Split on the runtime/type boundary, then converge in one integration batch:

1. runtime `Json` foundation;
2. compiler + JSON-genus + Norma + corpus integration.

This is the only justified split. A parked wrapper with unchanged `valor`
typing is not a completed factory checkpoint.

### Cross-phase invariants

| Invariant | Enforcement |
| --- | --- |
| Every reachable `json` is object-rooted and recursively safe. | private wrapper + validator + no assignability alias |
| Errors never disappear across parse/validation/schema layers. | structured parse/conversion diagnostics and negative gates |
| `nomen` is symmetric. | shared field-key lookup used by boxing and extraction tests |
| Runtime and MIR agree. | same fixtures and value-tree comparison |
| Unsupported targets do not pretend. | capability checks before emission |
| Old public route is absent. | source/doc searches for old signatures and claims |

### Forbidden states

- `Primitive::Json` rendered as `faber::Valor` without a wrapper;
- a `json` wrapper containing `Octeti`, tagged `Instans`, or non-finite float;
- a successful `valor ↦ json` with an array/scalar root;
- JSON-genus boxing using renamed keys while extraction uses source names;
- Norma Rust behavior and MIR stepper behavior accepting different roots;
- both old and new `norma:json` signatures present.

### Release decision

`release-prep`. Inline literal typing, a new public primitive/runtime type, and
Norma signature changes are published contracts. Goal closeout must prepare a
minor-version release note at minimum; the campaign decides whether to release
immediately after dependent Goal 3 or defer the actual tag to the next normal
Faber release.

## Factory Evidence

- Faber runtime `c5a60b1` (`feat(json): add object-rooted runtime document`)
  introduced `faber::Json`, object-root validation, recursive value validation,
  duplicate-key rejecting parsing, compact rendering, and checked bridges to and
  from `Valor`.
- Radix `e75cac7a6` (`feat(json): make json a formal document type`) made
  `json` a formal primitive across frontend, HIR/MIR, Rust codegen, stepper,
  reader vocabulary, docs, and fail-closed unsupported targets.
- Norma `000d109` (`feat(json): expose object-rooted json documents`) moved
  `norma:json` to the formal document type and removed the old broad `valor`
  signatures.
- Examples `d8d0e1b` (`feat(json): add canonical json corpus`) added the
  canonical corpus proof and explicit broad-`valor` widening where intended.

Validated gates:

- `timeout 180 cargo test json -- --format terse`
- `timeout 180 cargo test -- --format terse`
- `timeout 180 cargo clippy --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `git diff --check`
- `timeout 300 cargo test -p radix json -- --format terse`
- `timeout 300 cargo test -p radix conversio -- --format terse`
- `timeout 300 cargo test -p radix mir -- --format terse`
- `timeout 300 cargo test -p radix codegen::ts -- --format terse`
- `timeout 300 cargo test -p radix codegen::go -- --format terse`
- `timeout 300 cargo test -p radix -- --format terse`
- `timeout 180 ./scripta/check-reader-pack-completeness`
- `timeout 180 ./scripta/check-ebnf-vocabulary`
- `timeout 180 ./scripta/check-exempla-pack`
- `timeout 180 ./scripta/check-exempla-frontmatter`
- `timeout 180 ./scripta/check-source`
- `timeout 180 cargo run --manifest-path ../faber/Cargo.toml -- run ../examples/corpus/json/json.fab`
- `timeout 180 cargo run -p radix --bin radix -- mir ../examples/corpus/json/json.fab`

`./scripta/test` still reports an unrelated pre-existing retired-surface
guardrail in `faber` tests containing `externa`; the JSON corpus issue surfaced
by that run was fixed and all focused Goal 2 gates pass.

## Validation

Runtime:

```bash
cd ../faber-runtime
timeout 180 cargo test json -- --format terse
timeout 180 cargo test
timeout 180 cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Compiler and application lane:

```bash
cd ../radix
timeout 300 cargo test -p radix json -- --format terse
timeout 300 cargo test -p radix conversio -- --format terse
timeout 300 cargo test -p radix mir -- --format terse
timeout 180 ./scripta/check-reader-pack-completeness
timeout 180 ./scripta/check-ebnf-vocabulary
timeout 300 ./scripta/test
cargo fmt --all -- --check
git diff --check
```

Norma and corpus:

```bash
cd ../norma
timeout 180 ./scripta/check-source
git diff --check

cd ../radix
./scripta/generate-exempla-index.py
timeout 300 cargo test -p exempla json -- --format terse
git diff --check
git -C ../examples diff --check
```

The canonical exemplum path is `examples/corpus/json/json.fab`. Run it through
both execution routes:

```bash
timeout 180 cargo run --manifest-path ../faber/Cargo.toml -- run ../examples/corpus/json/json.fab
timeout 180 cargo run -p radix --bin radix -- mir ../examples/corpus/json/json.fab
```

## Companion Skill Plan

- `correctness`: recursive validator, duplicate-key parser, numeric boundaries,
  required/default genus extraction, MIR/Rust parity.
- `red-green`: conversion matrix and forbidden-state tests before integration.
- `cleanliness`: keep syntax parsing, runtime validation, schema conversion,
  and wire codecs in separate modules; avoid another giant conversion match.
- `consequences`: re-audit public type/signature callers before J5.
- `polish`: every changed primary source file in all four repos before closeout.

## Open Questions

No blocking design question remains. Factory may choose implementation details
for the strict text parser and wrapper error type, provided the contracts above
hold.

Stop and return to campaign routing if:

- preserving `json` as a distinct type would require weakening type equality or
  target capability checks;
- a target can only support it by aliasing arbitrary dynamic values;
- direct callers reveal a required arbitrary-root public document contract;
- a compatibility overload is proposed for old Norma signatures.
