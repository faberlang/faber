# Phase 007 - Conversion Recovery And Radix Parsing

## Interpreted Unit

Fix the remaining scalar conversion failures represented by
`conversio/octeti.fab` and `conversio/radix.fab`.

## Normalized Spec

Functional requirements:

- MIR lowering preserves target-typed recovery operands for octeti/text/ascii
  conversions so MIR validation accepts semantically valid `⇥` recovery.
- `textus ↦ numerus<W, Hex|Bin|Oct>` parses with the declared radix in the
  script stepper.
- Radix parse failure still uses the ordinary conversion failure path, including
  inline `⇥` recovery and failable propagation from previous phases.
- Existing decimal `textus ↦ numerus` behavior remains unchanged.

Constraints and non-goals:

- Do not change grammar or semantic typing.
- Do not weaken MIR validation's recovery type check.
- Do not implement unrelated numeric-width overflow semantics in the stepper.
- Do not broaden collection, tensor, or method-call support in this phase.

## Repo-Aware Baseline

Evidence:

- `conversio/octeti.fab` currently fails MIR validation with
  `conversion recovery type mismatch`.
- `conversio/radix.fab` lowers to MIR with hint symbols but script execution
  fails with `textus to numerus conversion failed`.
- Rust emission for `radix.fab` uses `from_str_radix` with bases 16, 2, and 8.
- Grammar defines runtime conversion as
  `conversio := '↦' typeAnnotation typeParams? inlineRecovery?`.

## Stage Graph

1. Inspect recovery operand lowering for textus/ascii/octet conversion.
2. Preserve or recover the semantic target type at the MIR recovery operand.
3. Teach the stepper numerus conversion to resolve radix hints.
4. Add focused tests for octeti/ascii recovery and radix parsing.
5. Run representative exempla, MIR-adjacent backend checks, and the script e2e
   gate.

## Implementation Work

Primary write surfaces:

- `crates/radix/src/mir/nodes.rs`
- `crates/radix/src/mir/lower.rs`
- MIR constant consumers in dump/type/validation/probe backends
- `crates/radix/src/mir/stepper/conversio.rs`
- `crates/radix/src/mir/stepper/runtime.rs`
- `crates/radix/src/mir/stepper_test.rs`
- `docs/factory/faber-script-e2e-hardening/ledger.md`

Out of scope:

- S-expression/Wasm/LLVM broad conversion backend work beyond recording current
  status unless a narrow existing pattern applies.
- `conversio/instans.fab` precision expectations.

## Checkpoints And Gates

Checkpoint target:

- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/octeti.fab` passes or moves to a new recorded
  blocker.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/radix.fab` passes or moves to a new recorded
  blocker.

Gate expectations:

- Focused stepper/MIR tests pass.
- The ignored script e2e gate is run and changed failure status is recorded.

Release checkpoint:

- Deferred. This is internal script/MIR hardening.

## Validation

Planned commands:

```bash
timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_radix_conversio_fixture
timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_octeti_conversio_fixture
cargo run -- run ../radix/crates/exempla/corpus/conversio/octeti.fab
cargo run -- run ../radix/crates/exempla/corpus/conversio/radix.fab
timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture
cargo fmt --all -- --check
git diff --check
```

## Companion Skill Plan

- `factory`: phase execution and checkpoint discipline.
- `faber`: grammar/exempla/compiler authority.

## Open Questions

No blocking open questions.
