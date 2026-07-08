# Phase 003 - Instans Runtime Conversion In Script Stepper

## Interpreted Unit

Implement script-stepper support for the `instans` runtime conversion family
exercised by `conversio/instans.fab` and `conversio/fallibilis.fab`.

## Normalized Spec

Functional requirements:

- The MIR stepper can represent typed `instans` values.
- `valor`/text-like runtime values can convert to `instans`, `instans<ms>`,
  `instans<us>`, and `instans<ns>` using the same precision contract as the
  Rust backend.
- `instans` values can convert to `textus` as RFC3339 UTC at their declared
  precision.
- `instans` to `instans<N>` conversion re-tags/truncates through the runtime
  precision API.
- Equality and ordering on `instans` values compare at the coarser precision,
  matching Rust backend behavior.
- Inline `⇥` recovery is honored when an `instans` conversion fails.

Constraints and non-goals:

- Do not change grammar or semantic typing.
- Do not implement broad `valor` aggregate extraction or unrelated runtime
  conversions.
- Do not absorb `conversio/fallibilis.fab` or `conversio/instans.fab` into
  expected failures.
- S-expression, Wasm, and LLVM inspection is required. If their existing
  architecture cannot represent stepper-only `instans` runtime values without
  broad design, record the reason as deferred.

## Repo-Aware Baseline

Evidence:

- Grammar: `EBNF.md` defines runtime conversion as
  `conversio := '↦' typeAnnotation typeParams? inlineRecovery?`.
- Exempla:
  - `crates/exempla/corpus/conversio/instans.fab`
  - `crates/exempla/corpus/conversio/fallibilis.fab`
- Rust reference:
  - `crates/radix/src/driver/mod_test.rs`
  - `crates/radix/src/codegen/rust/tests/failable_test.rs`
- Runtime API:
  - `crates/faber/src/instans.rs`
  - `crates/faber/src/valor.rs`
- Stepper surfaces:
  - `crates/radix/src/mir/stepper/value.rs`
  - `crates/radix/src/mir/stepper/conversio.rs`
  - `crates/radix/src/mir/stepper/mod.rs`
  - `crates/radix/src/mir/stepper/valor.rs`

## Stage Graph

1. Add a typed `instans` value representation to the MIR stepper.
2. Implement `instans` conversion targets and text emission.
3. Implement equality/ordering support for `instans`.
4. Add focused tests for conversion, recovery, text output, and comparison.
5. Inspect MIR-adjacent S-expression, Wasm, and LLVM status.
6. Run focused validation and the script e2e gate.

## Implementation Work

Primary write surfaces:

- `crates/radix/src/mir/stepper/value.rs`
- `crates/radix/src/mir/stepper/conversio.rs`
- `crates/radix/src/mir/stepper/mod.rs`
- `crates/radix/src/mir/stepper/valor.rs`
- Focused stepper tests under `crates/radix/src/mir/stepper*_test.rs`
- `docs/factory/faber-script-e2e-hardening/ledger.md`

Out of scope:

- S-expression/Wasm/LLVM broad runtime value model design.
- CLI/package/norma import behavior.
- Runtime conversions for collection or tensor targets.

## Checkpoints And Gates

Checkpoint target:

- `conversio/instans.fab` and `conversio/fallibilis.fab` move past
  `FaberScript unsupported: runtime conversio to Primitive(Instans)`.

Gate expectations:

- Focused stepper tests pass.
- `radix mir` still lowers both representative exempla.
- The script e2e harness is run and the changed failure status is recorded.

Release checkpoint:

- Deferred. This is internal script/MIR hardening.

## Validation

Planned commands:

```bash
timeout 120 cargo test -p radix instans
cargo run -p radix --bin radix -- mir crates/exempla/corpus/conversio/instans.fab
cargo run -p radix --bin radix -- mir crates/exempla/corpus/conversio/fallibilis.fab
timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture
cargo fmt --all -- --check
git diff --check
```

## Companion Skill Plan

- `factory`: phase execution and checkpoint discipline.
- `faber`: grammar/exempla/compiler authority.

## Open Questions

No blocking open questions.

