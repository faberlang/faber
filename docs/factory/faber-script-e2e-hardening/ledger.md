# Faber Script E2E Hardening Ledger

## Current Checkpoint

- Goal source: `docs/factory/faber-script-e2e-hardening/goal.md`
- Factory status: **paused** (2026-07-04) — no new phases until explicitly resumed
- Last active phase: Phase 034 - nested JSON valor MIR lowering
- Delivery spec: `phase-034-nested-json-valor.md`
- Phase status at pause: housekeeping cleanup after recent feature fixtures
- Release checkpoint: deferred; harness hardening only
- Resume from: Phase 034 closeout or next failure-category selection per goal loop

## Phase 034 Evidence

Invariant:

- Compile-time JSON `valor` literals lower nested object and array values
  recursively into MIR aggregate construction. Nested values do not fall
  through to unsupported nested-aggregate diagnostics or path fallout.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_nested_json_valor_literal
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_destructura_literal_fixture
  -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/destructura/literal.fab` passed and printed the four
  debug-map valor lines from the fixture.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/destructura/literal.fab` showed recursive `construct
  map` operations for the nested `extra.medium` object, with no unsupported
  MIR or path diagnostics.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/destructura/literal.fab` emitted recursive
  `faber::Valor::Tabula` construction.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/destructura/literal.fab` failed on the existing
  S-expression `valor` default gap: `default value for type Primitive(Valor)`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/destructura/literal.fab` failed on the existing Wasm
  `valor` type gap: `type Primitive(Valor)`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/destructura/literal.fab` emitted nested aggregate map
  helper calls including a pointer-valued nested map.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `destructura/literal.fab` moved out of expected failures and now passes as a
  smoke-checked script fixture.
- Pass/run counts moved from `198/258` and `198/258` to `199/258` and
  `199/258`.
- Output-checked count stayed at `45/258` because valor map debug output is not
  canonicalized for stable key ordering.
- The live `unsupported-mir` bucket is now empty and no longer appears in the
  harness summary.

## Housekeeping Cleanup Evidence

Invariant:

- Recent feature and policy fixtures must either pass script mode or have a
  structured non-script reason. Ordinary examples must follow the active scalar
  arithmetic contract instead of living as unclassified script failures.

Validation:

- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed after cleanup.

Live script e2e effect:

- `ad/runtime-echo.fab` is classified as `capability-stream`; it is a frame /
  `ad` vertical-slice fixture, not a script-owned runtime surface.
- `air/air-lane.fab` is classified as `mir-backed-target-only`; it exists for
  MIR-backed emit targets such as `wasm-text`.
- `genus/creo.fab` and `praefixum/praefixum.fab` now use explicit `↦ fractus`
  conversions where the scalar numeric-width policy rejects integer/fractus
  mixed arithmetic.
- Script ratchet floors moved to `run>=203` and `output_checked>=47`.

MIR-boundary backend status:

- Rust remains the reference and emits recursive `faber::Valor` trees.
- LLVM can emit representative nested aggregate calls with the existing opaque
  pointer aggregate model.
- S-expression and Wasm remain blocked by pre-existing broad `valor` type
  support gaps, not by nested JSON MIR lowering.

## Phase 033 Evidence

Invariant:

- Compiler-owned `lista.reducta(reducer, init)` lowers to explicit MIR loop
  control flow using a synthetic two-parameter reducer function. It returns the
  final accumulator value and does not fall through to unresolved method-call or
  path diagnostics.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lista_higher_order_methods_are_classified_for_mir -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lowers_lista_reducta_with_synthetic_reducer -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_lista_methodi_functionales_fixture -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/lista/methodi-functionales.fab` passed and printed
  `[2, 4]`, `[2, 4, 6, 8, 10]`, and `15`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/lista/methodi-functionales.fab` showed `filtrata`,
  `mappata`, and `reducta` as ordinary MIR loop flow using collection
  `length`, collection `append`, and synthetic closure calls `f1`, `f2`, and
  `f3`, with no unresolved method/path diagnostics.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/lista/methodi-functionales.fab` emitted native
  iterator `filter`, `map`, and `fold` code.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/lista/methodi-functionales.fab` failed on the existing
  S-expression array default gap: `default value for type Array(TypeId(1))`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/lista/methodi-functionales.fab` emitted existing
  `length` and `append` runtime import probes, with no new `reducta` import
  surface.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/lista/methodi-functionales.fab` emitted ordinary
  closure calls, aggregate index calls, and existing `length`/`append` runtime
  declarations.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `lista/methodi-functionales.fab` moved out of expected failures and now
  passes as an output-checked script fixture.
- Pass/run/output counts moved from `197/258`, `197/258`, and `44/258` to
  `198/258`, `198/258`, and `45/258`.
- `unsupported-mir` bucket count moved from `2` to `1`.

MIR-boundary backend status:

- Rust remains the reference and emits `fold` for `reducta`.
- Wasm and LLVM do not need a new collection operation name because MIR lowers
  `reducta` into ordinary loop, index, assignment, and synthetic function-call
  flow using existing runtime import probes.
- S-expression remains blocked by the pre-existing array default gap before the
  lowered loop can be inspected there.

## Phase 032 Evidence

Invariant:

- Compiler-owned `lista` copy/view methods that Rust already executes lower to
  explicit MIR collection operations. They return copied list values and do not
  fall through to generic unresolved method-call diagnostics.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_lista_copy_view_methods
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_lista_methodi_copiae_fixture -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/lista/methodi-copiae.fab` passed and printed all
  seven pinned list copy/view method lines.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/lista/methodi-copiae.fab` emitted clone-backed
  `addita`, slice-backed `sectio`, iterator `take`/`skip`, saturating
  `ultima`, `reverse`, and `sort`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/lista/methodi-copiae.fab` showed runtime collection
  operations for `append_immutable`, `slice`, `take`, `take_last`,
  `drop_first`, `reverse`, and `sort`, with no unresolved method/path
  diagnostics.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/lista/methodi-copiae.fab` failed on the existing
  S-expression array default gap: `default value for type Array(TypeId(1))`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/lista/methodi-copiae.fab` emitted runtime import
  probes for `append_immutable`, `slice`, `take`, `take_last`, `drop_first`,
  `reverse`, and `sort`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/lista/methodi-copiae.fab` emitted runtime declarations
  and calls for `append_immutable`, `slice`, `take`, `take_last`,
  `drop_first`, `reverse`, and `sort`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `lista/methodi-copiae.fab` moved out of expected failures and now passes as
  an output-checked script fixture.
- Pass/run/output counts moved from `196/258`, `196/258`, and `43/258` to
  `197/258`, `197/258`, and `44/258`.
- `unsupported-mir` bucket count moved from `3` to `2`.

MIR-boundary backend status:

- Rust remains the reference and emits copied list operations using native Rust
  slice/iterator collection behavior.
- Wasm and LLVM expose the promoted copy/view operations through their existing
  runtime import probe paths.
- S-expression remains blocked by the pre-existing array default gap before the
  new collection operations can be inspected there.

## Phase 031 Evidence

Invariant:

- Compiler-owned `lista` mutation methods that Rust already executes lower to
  explicit MIR collection operations or equivalent MIR assignments. They do not
  fall through to generic unresolved method-call diagnostics.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_lista_mutatio_methods
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_lista_methodi_mutatio_fixture -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_morphologia_fixture
  -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/lista/methodi-mutatio.fab` passed and printed
  `[1, 2, 4]` and `3`.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/morphologia/morphologia.fab` passed and printed all
  nine pinned morphology method lines.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/lista/methodi-mutatio.fab` emitted `push`, guarded
  front removal, `reverse`, and `sort`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/lista/methodi-mutatio.fab` failed on the existing
  S-expression array default gap: `default value for type Array(TypeId(1))`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/lista/methodi-mutatio.fab` emitted runtime import
  probes for `append`, `remove_first`, `reverse_in_place`, and `sort`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/lista/methodi-mutatio.fab` emitted runtime declarations
  and calls for `append`, `remove_first`, `reverse_in_place`, and `sort`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `lista/methodi-mutatio.fab` moved out of expected failures and now passes as
  an output-checked script fixture.
- `morphologia/morphologia.fab` also moved out of expected failures because its
  remaining blocker was the same `ordina` mutation path; its documented stdout
  is now pinned by an output sidecar.
- Pass/run/output counts moved from `194/258`, `194/258`, and `41/258` to
  `196/258`, `196/258`, and `43/258`.
- `unsupported-mir` bucket count moved from `5` to `3`.

MIR-boundary backend status:

- Rust remains the reference and emits native list mutation calls.
- Wasm and LLVM expose the promoted `remove_first` collection operation through
  their existing runtime import probe paths; `ordina` reuses the existing
  `sort` collection operation plus assignment.
- S-expression remains blocked by the pre-existing array default gap before the
  new collection operation can be inspected there.

## Phase 030 Evidence

Invariant:

- Unit-variant `elige`/`discerne` lowering preserves normal arm fallthrough.
  When an arm body completes normally, it jumps to the join block and later
  source statements continue lowering. All-terminating variant `discerne`
  expressions do not synthesize a no-value fallthrough return. `→ nihil`
  remains an effect-only return shape and accepts no-value return terminators,
  matching semantic return-path checking and Rust backend behavior.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  allows_no_value_return_for_nihil_side_effect_function -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lowers_ordo_elige_statement_continues_after_match -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lowers_all_returning_variant_discerne_without_fallthrough_return
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_ordo_fixture
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_discerne_fixture
  -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/ordo/ordo.fab` passed and printed `rubrum` and
  `actum`.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/discerne/discerne.fab` passed and printed
  `discerne exempla parata`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/ordo/ordo.fab` emitted native Rust enum matches for
  `Color` and `Condicio`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/ordo/ordo.fab` failed on the existing S-expression enum
  default gap: `default value for type Enum(DefId(4096))`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/ordo/ordo.fab` emitted the existing variant aggregate
  and diagnostic import probes.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/ordo/ordo.fab` emitted branch joins from successful
  variant arms to the post-`elige` blocks.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `ordo/ordo.fab` moved out of expected failures and now passes as an
  output-checked script fixture.
- `discerne/discerne.fab` remains passing and now has its documented stdout
  pinned by an output sidecar.
- Pass/run/output counts moved from `193/258`, `193/258`, and `39/258` to
  `194/258`, `194/258`, and `41/258`.
- `unsupported-mir` bucket count moved from `6` to `5`.

MIR-boundary backend status:

- Rust emits native enum construction and pattern matching for representative
  `ordo` selections.
- Wasm and LLVM retain existing opaque variant aggregate behavior and branch
  lowering.
- S-expression remains blocked by the pre-existing enum default gap.

## Phase 029 Evidence

Invariant:

- `tacet` is an explicit no-op statement. MIR lowering emits no statement for
  it and leaves the current block open so surrounding control flow proceeds.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_tacet_to_noop -- --nocapture`
  passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_tacet_fixture -- --nocapture`
  passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/tacet/tacet.fab` passed and printed `cond verum` and
  `finis`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/tacet/tacet.fab` emitted the existing explicit no-op
  comment in Rust output.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/tacet/tacet.fab` emitted ordinary control flow with no
  no-op runtime surface.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/tacet/tacet.fab` emitted the existing diagnostic import
  probe and no no-op runtime surface.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/tacet/tacet.fab` emitted ordinary branch flow with no
  no-op runtime surface.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `tacet/tacet.fab` moved out of expected failures and now passes as an
  output-checked script fixture.
- Pass/run/output counts moved from `192/258`, `192/258`, and `38/258` to
  `193/258`, `193/258`, and `39/258`.
- `unsupported-mir` bucket count moved from `7` to `6`.

MIR-boundary backend status:

- No MIR-boundary backend needed new no-op support because `tacet` emits no MIR
  statement.
- Rust retains its source-level no-op comment.
- S-expression, Wasm, and LLVM emit ordinary surrounding control flow.

## Phase 028 Evidence

Invariant:

- Named `discretio` variant payloads construct and project by field symbol.
  `finge Variant { field = value }` and `discerne` payload destructuring agree
  on the same variant-field metadata; symbol ids are not positional indices.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_finge_fixture
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_omnia_fixture
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_discerne_insanum_fixture
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_unio_fixture
  -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/finge/finge.fab` passed and printed
  `finge expressiones paratae`.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/omnia/omnia.fab` passed and printed `activa`.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/integratio/discerne-insanum.fab` passed and printed
  the three variant destructuring lines.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/unio/unio.fab` passed and printed inline union values
  plus tagged variant descriptions.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/integratio/discerne-insanum.fab` emitted native Rust
  enum variants and `match` destructuring.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/integratio/discerne-insanum.fab` failed on the
  existing S-expression enum default gap: `default value for type
  Enum(DefId(4096))`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/integratio/discerne-insanum.fab` emitted the existing
  binary Wasm variant aggregate and variant-field probe helpers.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/integratio/discerne-insanum.fab` emitted the existing
  LLVM variant aggregate and variant-field probe helpers.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `finge/finge.fab`, `omnia/omnia.fab`,
  `integratio/discerne-insanum.fab`, and `unio/unio.fab` moved out of expected
  failures and now pass as output-checked script fixtures.
- Pass/run/output counts moved from `188/258`, `188/258`, and `34/258` to
  `192/258`, `192/258`, and `38/258`.
- `unsupported-mir` bucket count moved from `11` to `7`.

MIR-boundary backend status:

- Rust emits native enum construction and pattern matching for the representative
  named-payload fixture.
- Wasm and LLVM retain existing opaque variant aggregate and variant-field probe
  behavior.
- S-expression remains blocked by the pre-existing enum default gap.

## Phase 027 Evidence

Invariant:

- Optional access is null-safe: omitted `sponte` fields and optional-chain index
  misses or out-of-bounds accesses return `nihil`. Ordinary `receiver[index]`
  access remains strict.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_optionalis_fixture
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_optional_chain_operator_fixture -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_keeps_ordinary_index_strict
  -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/optionalis/optionalis.fab` passed and printed omitted
  optional fields, missing optional indices, and coalesced fallback output.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/operatores/optional-chain.fab` passed and printed
  optional field and optional index output.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/optionalis/optionalis.fab` emitted `None` for omitted
  `sponte` fields and `.get(...).cloned()` for optional index access.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/optionalis/optionalis.fab` failed on the existing
  S-expression struct default gap: `default value for type Struct(DefId(4097))`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/optionalis/optionalis.fab` emitted the existing binary
  Wasm optional aggregate field/index probe.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/optionalis/optionalis.fab` emitted the existing LLVM
  optional aggregate field/index probe.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `optionalis/optionalis.fab` and `operatores/optional-chain.fab` moved out of
  expected failures and now pass as output-checked script fixtures.
- Pass/run/output counts moved from `186/258`, `186/258`, and `32/258` to
  `188/258`, `188/258`, and `34/258`.
- `unsupported-mir` bucket count moved from `13` to `11`.

MIR-boundary backend status:

- Rust, Wasm, and LLVM retain existing optional aggregate probe behavior.
- S-expression remains blocked by the pre-existing struct default gap.

## Phase 026 Evidence

Invariant:

- Script-mode execution runs only the lowered `incipit` entry block. Test-runner
  declarations may lower to MIR functions, but they are not script entries.

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_rejects_omitte_without_incipit
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix function_name_prefers_source_and_entry
  -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/omitte/omitte.fab` failed cleanly with
  `error: no entry function in MIR program` instead of panicking through the
  skipped test body.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/omitte/omitte.fab` emitted the existing ignored Rust
  test shape.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/omitte/omitte.fab` failed on the existing
  S-expression assert runtime-call gap: `runtime_call Assert`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/omitte/omitte.fab` emitted the existing binary Wasm
  probe with `database_connection` and assert import names.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/omitte/omitte.fab` emitted the existing LLVM assert
  runtime-call probe.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `omitte/omitte.fab` no longer aborts inside a skipped test body; it is now a
  normal no-entry script failure.
- Fourteen declaration-only testing fixtures that previously passed only because
  script mode executed their test functions are now classified under
  `no-entry-reference`: `fragilis/fragilis.fab`, `futurum/futurum.fab`,
  `meta/requirit.fab`, `postpara/postpara.fab`,
  `postparabit/postparabit.fab`, `praepara/praepara.fab`,
  `praeparabit/praeparabit.fab`, `proba/proba.fab`,
  `probandum/probandum.fab`, `repete/repete.fab`, `solum/solum.fab`,
  `solum-in/solum-in.fab`, `tag/tag.fab`, and `temporis/temporis.fab`.
- Pass/run/output counts moved from `200/258`, `200/258`, and `32/258` to
  `186/258`, `186/258`, and `32/258` because false-positive script execution of
  declaration-only test fixtures was removed.
- `no-entry-reference` bucket count moved from `11` to `25`.
- `unsupported-mir` bucket count stayed at `13`.

MIR-boundary backend status:

- Rust, Wasm, and LLVM retain existing test-declaration probe behavior.
- S-expression remains blocked by the pre-existing assert runtime-call gap.
- The shared MIR probe fallback entry naming remains unchanged; only the stepper
  now requires an explicit lowered `incipit`.

## Phase 025 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_regex_conversio_fixture
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_lege_regex_fixture
  -- --nocapture` passed.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/literalia/regex.fab` passed and printed regex pattern
  carrier text.
- `printf 'salve\n' | timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/lege/lege.fab` passed and printed `(?g)\d+` followed
  by the supplied line.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/literalia/regex.fab` failed on the existing
  S-expression regex default gap: `default value for type Primitive(Regex)`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/literalia/regex.fab` emitted the existing binary Wasm
  probe with regex literal import names.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/literalia/regex.fab` emitted the existing LLVM probe
  with regex literal declarations.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `literalia/regex.fab` and `lege/lege.fab` now pass script execution.
- Pass/run/output counts moved from `198/258`, `198/258`, and `32/258` to
  `200/258`, `200/258`, and `32/258`.
- `unsupported-mir` bucket count moved from `15` to `13`.

MIR-boundary backend status:

- Rust already emits `faber::Regex::new(...)` and displays the carrier pattern.
- Wasm and LLVM already expose regex literal/import probe paths.
- S-expression remains blocked by the pre-existing regex default gap.

## Phase 024 Evidence

Validation:

- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/conversio/conversio.fab` passed and printed `0.0` for
  the recovered `fractus` value.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust
  ../radix/crates/exempla/corpus/conversio/conversio.fab` showed Rust emission lowering
  `f2` as `f64` with `unwrap_or(0.0)`.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_conversio_exemplum
  -- --nocapture` passed.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `conversio/conversio.fab` moved out of expected failures and now passes as a
  normal output-checked script fixture.
- `conversio.expected` now records `0.0` for the `fractus` recovery output,
  matching the fixture target type and emitted Rust behavior.
- Pass/run/output counts moved from `197/258`, `198/258`, and `32/258` to
  `198/258`, `198/258`, and `32/258`.
- `unsupported-mir` bucket count moved from `16` to `15`.

MIR-boundary backend status:

- Not applicable; this phase updated a stale output fixture and expected-failure
  classification only.

## Phase 023 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  conversio_capitalized_user_type_target_parses_as_type -- --nocapture`
  passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_valor_genus_conversio_fixture -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix valor_bridge_scalars -- --nocapture`
  passed after the bridge API started accepting optional interner context.
- `timeout 120 cargo run -- run
  ../radix/crates/exempla/corpus/conversio/valor-genus.fab` passed and printed the
  current struct debug representation for `roundtrip`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/conversio/valor-genus.fab` failed on the existing
  S-expression valor default gap: `default value for type Primitive(Valor)`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/conversio/valor-genus.fab` failed on the existing Wasm
  valor type gap: `MIR-to-WASM unsupported: type Primitive(Valor)`.
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/conversio/valor-genus.fab` emitted the existing
  pointer-carrier runtime conversion probe for valor/genus conversion.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` passed.
- `timeout 120 cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `conversio/valor-genus.fab` now passes script execution.
- Stale expected-failure entries that now pass were removed:
  `adfirma/adfirma.fab`, `adfirma/in-functione.fab`,
  `conversio/bivalens.fab`, `conversio/radix.fab`, and `mori/mori.fab`.
- Pass/run/output counts moved from `196/258`, `197/258`, and `32/258` to
  `197/258`, `198/258`, and `32/258`.
- `unsupported-mir` bucket count moved from `21` to `16`.

MIR-boundary backend status:

- S-expression and Wasm remain blocked by pre-existing dynamic `valor` carrier
  representation gaps.
- LLVM emits the existing pointer-carrier runtime conversion probe.

## Phase 022 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lowers_tensor_intrinsic_methods_to_collection_ops -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_tensor_shape_fixture
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_tensor_textus_fixture
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_tensor_arithmetic_elementwise_fixture -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_tensor_arithmetic_reduction_fixture -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_valor_tensor_conversio_fixture -- --nocapture` passed after array
  display switched from debug output to list output.
- `cargo run -- run ../radix/crates/exempla/corpus/tensor/shape.fab`
  passed and printed `2`, `[2, 3]`, `2`, `9.0`, `3`.
- `cargo run -- run ../radix/crates/exempla/corpus/tensor/textus.fab`
  passed and printed `alpha`.
- `cargo run -- run
  ../radix/crates/exempla/corpus/tensor/arithmetic-elementwise.fab` passed and printed
  `[1.0, 4.0, 9.0, 16.0]`.
- `cargo run -- run
  ../radix/crates/exempla/corpus/tensor/arithmetic-reduction.fab` passed and printed
  `21.0`, `3.5`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/tensor/shape.fab` failed on the existing S-expression
  array default gap: `default value for type Array(...)`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/tensor/shape.fab` failed on the existing Wasm tensor type
  gap: `MIR-to-WASM unsupported: type Tensor(...)`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/tensor/shape.fab` emitted an experimental LLVM probe
  declaring the new tensor runtime calls.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on the remaining unclassified live failure.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `tensor/shape.fab`, `tensor/textus.fab`, `tensor/arithmetic-elementwise.fab`,
  and `tensor/arithmetic-reduction.fab` now pass script execution.
- Pass/run/output counts moved from `193/258`, `193/258`, and `30/258` to
  `196/258`, `197/258`, and `32/258`.
- The only remaining unexpected failure is `conversio/valor-genus.fab`.
- `conversio/conversio.fab` remains an expected failure; float display now shows
  `0.0` for the recovered fractus value, while its current expected file says
  `0`.

MIR-boundary backend status:

- LLVM probe names all promoted tensor collection calls through existing runtime
  import machinery.
- S-expression and Wasm remain blocked by pre-existing aggregate/tensor carrier
  gaps before the new tensor runtime calls can execute.

## Phase 021 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p exempla script_expected_failure -- --nocapture`
  passed.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- Pass/run/output counts stayed at `193/258`, `193/258`, and `30/258`.
- `instans/instans.fab` moved under the existing `norma-import` bucket because
  it imports `norma:hal/tempus` and `norma:toml`.
- `norma-import` bucket count moved from `11` to `12`.
- The remaining unexpected failures are now:
  `conversio/valor-genus.fab`, `tensor/arithmetic-elementwise.fab`,
  `tensor/arithmetic-reduction.fab`, `tensor/shape.fab`, and
  `tensor/textus.fab`.

MIR-boundary backend status:

- Not applicable; this phase updated script expected-failure taxonomy only.

## Phase 020 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix validates_valor_array_carrier_aggregate
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_valor_tensor_conversio_fixture -- --nocapture` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/valor-tensor.fab` passed and printed the
  current stepper array carrier debug form:
  `Array(RefCell { value: [Int(1), Int(2), Int(3), Int(4)] })`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/conversio/valor-tensor.fab` failed on the existing
  S-expression valor default gap: `default value for type Primitive(Valor)`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/conversio/valor-tensor.fab` failed on the existing Wasm
  valor type gap: `MIR-to-WASM unsupported: type Primitive(Valor)`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/conversio/valor-tensor.fab` emitted an experimental
  LLVM probe with pointer-carrier runtime conversion calls.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `conversio/valor-tensor.fab` now passes script execution.
- Pass/run/output counts moved from `192/258`, `192/258`, and `30/258` to
  `193/258`, `193/258`, and `30/258`.
- Expected failure buckets are unchanged.
- The remaining unexpected failures are now:
  `conversio/valor-genus.fab`, `instans/instans.fab`,
  `tensor/arithmetic-elementwise.fab`, `tensor/arithmetic-reduction.fab`,
  `tensor/shape.fab`, and `tensor/textus.fab`.

MIR-boundary backend status:

- S-expression and Wasm remain blocked by pre-existing dynamic `valor` carrier
  representation gaps.
- LLVM emits the existing pointer-carrier runtime conversion probe.

## Phase 019 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lowers_tensor_longitudo_to_collection_length -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_tensor_decl_fixture
  -- --nocapture` passed.
- `cargo run -- run ../radix/crates/exempla/corpus/tensor/decl.fab`
  passed and printed `0`.
- `cargo run -- run ../radix/crates/exempla/corpus/conversio/tensor.fab`
  passed and printed `4`, `4`, and five `0` lines.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/tensor/decl.fab` failed on the existing tensor default
  value gap: `default value for type Tensor(...)`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/tensor/decl.fab` failed on the existing Wasm tensor type
  gap: `MIR-to-WASM unsupported: type Tensor(...)`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/tensor/decl.fab` emitted an experimental LLVM probe with
  `__faber_aggregate_tensor_0` and `__faber_runtime_length_1_ptr_to_i64`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `tensor/decl.fab` and `conversio/tensor.fab` now pass script execution.
- Pass/run/output counts moved from `190/258`, `190/258`, and `30/258` to
  `192/258`, `192/258`, and `30/258`.
- Expected failure buckets are unchanged.
- The remaining unexpected failures are now:
  `conversio/valor-genus.fab`, `conversio/valor-tensor.fab`,
  `instans/instans.fab`, `tensor/arithmetic-elementwise.fab`,
  `tensor/arithmetic-reduction.fab`, `tensor/shape.fab`, and
  `tensor/textus.fab`.

MIR-boundary backend status:

- S-expression and Wasm remain blocked by pre-existing tensor representation
  gaps before the promoted `length` call can matter.
- LLVM already has an established pointer-carrier probe path for tensor vacua
  and collection length.

## Phase 018 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p exempla script_expected_failure -- --nocapture`
  passed.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- Pass/run/output counts stayed at `190/258`, `190/258`, and `30/258`.
- Added the `norma-import` expected-failure bucket with `11` fixtures.
- Classified `11` `norma:*` import fixtures, `2` `intervallum` no-entry
  reference files, and `1` explicitly negative sized-family fixture.
- Expected failure buckets are now `frontend-negative: 3`, `package-only: 1`,
  `norma-import: 11`, `cli-program: 8`, `capability-stream: 10`,
  `no-entry-reference: 11`, and `unsupported-mir: 21`.
- The remaining unexpected script failures are the tensor/instans/conversio
  hardening slice:
  `conversio/tensor.fab`, `conversio/valor-genus.fab`,
  `conversio/valor-tensor.fab`, `instans/instans.fab`,
  `tensor/arithmetic-elementwise.fab`, `tensor/arithmetic-reduction.fab`,
  `tensor/decl.fab`, `tensor/shape.fab`, and `tensor/textus.fab`.

MIR-boundary backend status:

- Not applicable; this phase updated script expected-failure taxonomy only.

## Phase 017 Evidence

Validation:

- `cargo run -- run ../radix/crates/exempla/corpus/binarius/binarius.fab`
  passed and printed direct booleans as `verum` and `falsum`.
- `cargo run -- run ../radix/crates/exempla/corpus/vel/vel.fab` passed and
  printed direct `falsum`.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p exempla script_expected_failure -- --nocapture`
  passed.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `binarius/binarius.fab` and `vel/vel.fab` now pass script output comparison.
- Overall pass/run/output counts moved from `188/258`, `190/258`, and `30/258`
  to `190/258`, `190/258`, and `30/258`.
- The `unsupported-mir` expected-failure bucket count dropped from `23` to `21`.

MIR-boundary backend status:

- Not applicable; this phase updated script expected output files and expected
  failure classification only.
- Explicit `bivalens ↦ textus` output remains separate and unchanged.

## Phase 016 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lowers_logical_operators_to_short_circuit_branches -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_short_circuits_logical_operands
  -- --nocapture` passed.
- `cargo run -- run ../radix/crates/exempla/corpus/binarius/binarius.fab`
  passed and no longer prints `hoc non videatur` for `falsum et carum()`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/binarius/binarius.fab` failed before the logical section
  on the existing S-expression option default gap:
  `default value for type Option(TypeId(0))`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/binarius/binarius.fab` emitted an experimental Wasm
  probe for the branch-shaped MIR.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/binarius/binarius.fab` emitted an experimental LLVM
  probe with branch-shaped logical control flow.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- Pass/run/output counts stayed at `188/258`, `190/258`, and `30/258`.
- `binarius/binarius.fab` now reaches the expected number of output lines, but
  remains a stdout mismatch because script displays booleans as `verum`/`falsum`
  while the Rust-backed expected file uses `true`/`false`.
- `binarius.expected` was corrected from stale assignment output `14` to `20`,
  matching both Rust output and the current source (`0 + 10`, `⊕`, `⊖`, then
  `* 2`).

MIR-boundary backend status:

- S-expression is blocked by a pre-existing option default gap in this fixture.
- Wasm and LLVM already emit branch-shaped MIR for this surface.
- Boolean display policy remains a separate pending output-contract phase.

## Phase 015 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p exempla script_expected_failure -- --nocapture`
  passed.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- Pass/run/output counts stayed at `188/258`, `190/258`, and `30/258`.
- `SCRIPT_EXPECTED_FAILURES` no longer classifies paths that now pass:
  `assignatio/assignatio.fab`, `ego/ego.fab`, `in/in.fab`,
  `redde/redde.fab`, `sub/sub.fab`, and `tabula/methodi-accessus.fab`.
- The `unsupported-mir` expected-failure bucket count dropped from `29` to `23`.

MIR-boundary backend status:

- Not applicable; this was a classification ratchet phase with no MIR or backend
  semantic changes.

## Phase 014 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_concatenates_textus_addition
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_assignatio_fixture_with_textus_addition -- --nocapture` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/assignatio/assignatio.fab` passed and printed `20`,
  `25`, `15`, `30`, `10`, `salve munde`.
- `cargo run -- run ../radix/crates/exempla/corpus/ego/ego.fab` passed and
  printed `capsa: prima`.
- `cargo run -- run ../radix/crates/exempla/corpus/redde/redde.fab` passed
  and printed `30`, `Salve, Munde`, `42`, `0`, `20`.
- `cargo run -- run ../radix/crates/exempla/corpus/sub/sub.fab` passed and
  printed `Lupa latrat`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/assignatio/assignatio.fab` emitted a Racket probe with
  `string-append`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/assignatio/assignatio.fab` emitted an experimental Wasm
  probe importing `__faber_text_concat`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/assignatio/assignatio.fab` emitted an experimental LLVM
  probe importing `__faber_text_concat_2_ptr_ptr_to_ptr`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `assignatio/assignatio.fab`, `ego/ego.fab`, `redde/redde.fab`, and
  `sub/sub.fab` now pass script execution.
- Overall pass/run/output counts moved from `184/258`, `186/258`, and `28/258`
  to `188/258`, `190/258`, and `30/258`.

MIR-boundary backend status:

- S-expression, Wasm, and LLVM already support text `Add` at the MIR boundary.
- This phase changed only the in-process stepper evaluator and corrected stale
  exemplar expectation comments in `assignatio` and `sub`.

## Phase 013 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lowers_numeric_operator_methods_to_binary_mir -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_numeric_operator_methods_fixture -- --nocapture` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/intrinseca/numeric-operator-methods.fab` passed and
  printed `9`, `15`, `8`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/intrinseca/numeric-operator-methods.fab` passed and
  shows `+`, `*`, and `&` binary MIR for `addita`, `multiplicata`, and
  `coniuncta`, plus assignment back into `acc` for `adde`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/intrinseca/numeric-operator-methods.fab` emitted a
  Racket probe with `+`, `*`, and `bitwise-and`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/intrinseca/numeric-operator-methods.fab` emitted an
  experimental Wasm probe.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/intrinseca/numeric-operator-methods.fab` emitted an
  experimental LLVM probe with `fadd`, `fmul`, and `and`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.

Live script e2e effect:

- `intrinseca/numeric-operator-methods.fab` now passes script execution.
- Overall pass/run/output counts moved from `183/258`, `185/258`, and `28/258`
  to `184/258`, `186/258`, and `28/258`.

MIR-boundary backend status:

- S-expression, Wasm, and LLVM already support the existing binary MIR produced
  by this phase for the fixture's arithmetic and bitwise operations.
- No MIR-boundary backend patch was needed.
- This phase intentionally did not change Rust codegen for chained receiver
  method precedence; MIR follows the parsed receiver chain and the exemplar
  expectation comment now records the observed script output `9`, `15`, `8`.
- Tensor method failures remain a likely next unsupported-method category.

## Phase 012 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  promoted_tabula_methods_share_mir_ops_with_registry -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_tabula_runtime_methods_to_intrinsics
  -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_tabula_methodi_accessus_fixture -- --nocapture` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/tabula/methodi-accessus.fab` passed and printed
  `95`, `nihil`, `0`, `verum`, `verum`, `1`, `falsum`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/tabula/methodi-accessus.fab` passed and shows
  `runtime collection put` for both `puncta.pone(...)` calls.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/tabula/methodi-accessus.fab` failed on the existing
  S-expression map default gap: `default value for type Map(TypeId(0),
  TypeId(1))`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/tabula/methodi-accessus.fab` failed on the existing
  Wasm option-carrier gap: `option coalesce mixed wasm carriers`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/tabula/methodi-accessus.fab` emitted an experimental
  LLVM probe with `__faber_runtime_put_3_ptr_ptr_i64`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.

Live script e2e effect:

- `tabula/methodi-accessus.fab` now passes script execution.
- Overall pass/run/output counts moved from `182/258`, `184/258`, and `28/258`
  to `183/258`, `185/258`, and `28/258`.

MIR-boundary backend status:

- S-expression remains blocked before `pone` by existing map default support.
- Wasm remains blocked after MIR lowering by existing option coalesce carrier
  support.
- LLVM emits an experimental probe using the established collection runtime
  import naming pattern.
- This phase intentionally promoted only `tabula.pone` to MIR and did not add
  deferred map utility names or cursor/view APIs.

## Phase 011 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix approximata -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lowers_numeric_runtime_methods_to_intrinsics -- --nocapture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_fractus_approximata_fixture -- --nocapture` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/intrinseca/fractus-approximata.fab` passed and
  printed `verum`, `falsum`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/intrinseca/fractus-approximata.fab` passed and shows
  `runtime collection approximate(_0, _1, const float 0.2) -> ty#3`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/intrinseca/fractus-approximata.fab` failed on the
  existing S-expression collection runtime-call gap:
  `runtime_call Collection(Approximate)`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/intrinseca/fractus-approximata.fab` emitted an
  experimental Wasm probe with `approximate_3_f64_f64_f64_to_i32`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/intrinseca/fractus-approximata.fab` emitted an
  experimental LLVM probe with `__faber_runtime_approximate_3_f64_f64_f64_to_i1`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.

Live script e2e effect:

- `intrinseca/fractus-approximata.fab` now passes script execution.
- Overall pass/run/output counts moved from `181/258`, `183/258`, and `28/258`
  to `182/258`, `184/258`, and `28/258`.

MIR-boundary backend status:

- S-expression remains blocked by existing collection runtime call support.
  This probe still has no aggregate/runtime collection model for such calls.
- Wasm emits an experimental probe using the established collection runtime
  import naming pattern.
- LLVM emits an experimental probe using the established collection runtime
  import naming pattern.
- This phase intentionally promoted only fractus `approximata` to MIR and did
  not change `≈`, numeric equality, tensor tolerance, or `numerus.approximata`.

## Phase 010 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  lista_summa_is_promoted_to_mir_collection_sum` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_in_parameter_summa_fixture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_sums_empty_numerus_lista`
  passed.
- `cargo run -- run ../radix/crates/exempla/corpus/in/in.fab` passed and
  printed `6`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir ../radix/crates/exempla/corpus/in/in.fab`
  passed and shows `runtime collection sum(_0) -> ty#1`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/in/in.fab` failed on the existing S-expression
  collection runtime-call gap: `runtime_call Collection(Sum)`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm
  ../radix/crates/exempla/corpus/in/in.fab` emitted an experimental Wasm probe with
  `sum_1_aggregate_to_i64`.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm
  ../radix/crates/exempla/corpus/in/in.fab` emitted an experimental LLVM probe with
  `__faber_runtime_sum_1_ptr_to_i64`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture` failed as expected on remaining unclassified live failures.

Live script e2e effect:

- `in/in.fab` now passes script execution.
- Overall pass/run/output counts moved from `180/258`, `182/258`, and `28/258`
  to `181/258`, `183/258`, and `28/258`.

MIR-boundary backend status:

- S-expression remains blocked by existing collection runtime call support.
  This probe has no aggregate/collection runtime model yet, so adding only
  `sum` would be a broader backend design.
- Wasm emits an experimental probe using the established collection runtime
  import naming pattern.
- LLVM emits an experimental probe using the established collection runtime
  import naming pattern.
- This phase intentionally promoted only numeric `lista.summa()` to MIR and did
  not implement generic reducers or tensor reductions.

## Phase 009 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_octeti_unify_fixture`
  passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_appends_octeti_byte_in_place`
  passed.
- `cargo run -- run ../radix/crates/exempla/corpus/octeti/unify.fab`
  passed and printed `4`, `222`, `2`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on remaining unclassified live failures.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/octeti/unify.fab` failed on existing collection
  runtime-call support (`Collection(Length)`).
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm-text
  ../radix/crates/exempla/corpus/octeti/unify.fab` failed on existing
  `SizedNumeric(Numerus, U8)` support.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm-text
  ../radix/crates/exempla/corpus/octeti/unify.fab` emitted an experimental probe.

Live script e2e effect:

- `octeti/unify.fab` now passes script execution.
- `octet/octet.fab` also now passes script execution because the same octeti
  append support removed its `append receiver type mismatch`.
- Overall pass/run/output counts moved from `178/258`, `180/258`, and `28/258`
  to `180/258`, `182/258`, and `28/258`.

MIR-boundary backend status:

- S-expression remains blocked by existing collection runtime call support.
- Wasm text remains blocked by existing `numerus<u8>` type support.
- LLVM text emits an experimental probe with runtime length/index/append stubs.
- This phase intentionally changed only script-stepper octeti/lista behavior.

## Phase 008 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_instans_conversio_fixture` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/instans.fab` passed and printed
  seconds/millis/micros/nanos/narrowed RFC3339 values at declared precision.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on remaining unclassified live failures.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp
  ../radix/crates/exempla/corpus/conversio/instans.fab` failed on the existing
  `default value for type Primitive(Valor)` backend gap.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm-text
  ../radix/crates/exempla/corpus/conversio/instans.fab` failed on the existing
  `type Primitive(Valor)` backend gap.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm-text
  ../radix/crates/exempla/corpus/conversio/instans.fab` emitted an experimental probe.

Live script e2e effect:

- `conversio/instans.fab` now passes script execution.
- Overall pass/run/output counts moved from `177/258`, `179/258`, and `28/258`
  to `178/258`, `180/258`, and `28/258`.

MIR-boundary backend status:

- S-expression remains blocked by existing `Primitive(Valor)` default support.
- Wasm text remains blocked by existing `Primitive(Valor)` type support.
- LLVM text emits an experimental probe with runtime conversion stubs.
- This phase intentionally corrected stale exemplar expectations to match the
  shipped runtime precision contract rather than changing MIR or runtime
  precision semantics.

## Phase 007 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_octeti_conversio_fixture` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_radix_conversio_fixture` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/octeti.fab` passed and printed `2`, `hi`,
  `?`, `2`, `hi`, `x`.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/radix.fab` passed and printed `255`, `10`,
  `493`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed with the repository's existing rustfmt
  `struct_lit_width` warning.
- `git diff --check` passed.

Live script e2e effect:

- `conversio/octeti.fab` now passes script execution.
- `conversio/radix.fab` now passes script execution.
- Overall pass/run/output counts moved from `175/258`, `177/258`, and `28/258`
  to `177/258`, `179/258`, and `28/258`.

MIR-boundary backend status:

- S-expression remains blocked for `octeti` by existing
  `default value for type Primitive(Octeti)` support.
- S-expression remains blocked for `radix` by existing
  `default value for type SizedNumeric(Numerus, I32)` support.
- Wasm text remains blocked for `octeti` by existing `type Primitive(Ascii)`
  support.
- Wasm text remains blocked for `radix` by existing
  `type SizedNumeric(Numerus, I32)` support.
- LLVM text emits experimental probes for both representative fixtures.
- This phase intentionally changed script-stepper behavior and MIR constant
  typing; broad backend support for octeti/ascii and sized numeric defaults
  remains separate work.

## Phase 006 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_runs_collection_conversio_fixture` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/collectiones.fab` passed.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on remaining unclassified live failures.

Live script e2e effect:

- `conversio/collectiones.fab` now passes script execution.
- Overall pass/run/output counts moved from `174/258`, `176/258`, and `28/258`
  to `175/258`, `177/258`, and `28/258`.

MIR-boundary backend status:

- S-expression remains blocked by existing
  `default value for type Array(TypeId(1))` support.
- Wasm text remains blocked by existing
  `type Tensor(TypeId(1), IndexId(19))` support.
- LLVM text emits an experimental probe with runtime conversion stubs.
- The script stepper intentionally represents tensor and cursor conversions as
  eager arrays for this phase; full tensor storage and lazy cursor behavior
  remain separate runtime work.

## Phase 005 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_handles_direct_conversio_inside_fac_cape` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_propagates_failable_runtime_conversio_through_try_call` passed.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/conversio/fallibilis.fab` passed and now shows
  `tutumDirect` using `try_call f4(_0)`.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/fallibilis.fab` passed.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on remaining unclassified live failures.

Live script e2e effect:

- `conversio/fallibilis.fab` now passes script execution.
- Overall pass/run/output counts moved from `173/258`, `175/258`, and `28/258`
  to `174/258`, `176/258`, and `28/258`.

MIR-boundary backend status:

- S-expression remains blocked by existing
  `default value for type Primitive(Instans)` support.
- Wasm text remains blocked by existing `type Primitive(Valor)` support.
- LLVM text remains blocked by existing `try_call` support.
- The new script path reuses the existing MIR `TryCall` shape through a
  synthetic failable conversion helper, rather than adding a new terminator.

## Phase 004 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_propagates_failable_runtime_conversio_through_try_call` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_keeps_inline_conversio_recovery_local` passed.
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix
  stepper_keeps_non_failable_conversio_failure_hard` passed.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/conversio/fallibilis.fab` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/fallibilis.fab` still fails with
  `textus to instans conversion failed`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on remaining unclassified live failures.

Live script e2e effect:

- The stepper now propagates bare runtime conversion failure out of functions
  with an alternate-exit type, and existing MIR `try_call` handlers observe that
  alternate return.
- Inline conversion recovery remains local, and non-failable conversion failure
  remains a hard stepper error.
- `conversio/fallibilis.fab` still fails overall because
  `tutumDirect` lowers `fac { redde v ↦ instans } cape err { ... }` as an
  ordinary runtime conversion statement plus a disconnected handler block. MIR
  does not yet encode an error edge from a direct handled conversion to the
  local `cape` handler.
- Overall pass/run/output counts stayed at `173/258`, `175/258`, and `28/258`.

## Phase 003 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_instans_runtime_conversio`
  passed.
- `timeout 180 cargo test --manifest-path ../radix/Cargo.toml -p radix instans` passed.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/conversio/instans.fab` passed.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/conversio/fallibilis.fab` passed.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/instans.fab` still aborts at a runtime
  assertion after conversions lower and execute. The remaining mismatch is the
  exemplar/Rust precision expectation around converting a seconds-precision
  `instans` to `instans<ms>`.
- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/fallibilis.fab` now fails with
  `textus to instans conversion failed`; that is the next failable-conversion
  propagation gap rather than an unsupported `instans` conversion target.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on remaining unclassified live failures.

Live script e2e effect:

- `conversio/fallibilis.fab` moved from
  `FaberScript unsupported: runtime conversio to Primitive(Instans)` to
  `textus to instans conversion failed`.
- `conversio/instans.fab` moved from missing conversion support to
  `stepper aborted`; the file now reaches assertions after executing runtime
  conversions.
- Overall pass/run/output counts stayed at `173/258`, `175/258`, and `28/258`.

MIR-boundary backend status:

- S-expression: `conversio/instans.fab` is blocked by existing
  `default value for type Primitive(Valor)` support; `fallibilis` is blocked by
  existing `default value for type Primitive(Instans)` support.
- Wasm text: `conversio/instans.fab` is blocked by existing
  `type Primitive(Valor)` support before the stepper conversion path matters.
- LLVM text: `conversio/instans.fab` emits an experimental probe with runtime
  conversion stubs; it does not execute or validate the stepper's `instans`
  semantics.
- This phase intentionally changed only the in-process script stepper value and
  conversion semantics.

## Phase 002 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_top_level_const_paths_inside_functions`
  passed.
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- mir
  ../radix/crates/exempla/corpus/conversio/fallibilis.fab` passed and no longer reports
  `path that does not resolve to a local value`.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on remaining unclassified live failures.
- `cargo fmt --all -- --check` passed with the repository's existing rustfmt
  `struct_lit_width` warning.
- `git diff --check` passed.

Live script e2e effect:

- `conversio/fallibilis.fab` moved from
  `unsupported MIR lowering: path that does not resolve to a local value` to
  `FaberScript unsupported: runtime conversio to Primitive(Instans)`.
- Overall pass/run/output counts stayed at `173/258`, `175/258`, and `28/258`.

MIR-boundary backend status:

- S-expression: deferred for `fallibilis`; the representative check is blocked
  by existing `default value for type Primitive(Instans)` support, not by the
  top-level const path MIR shape.
- Wasm text: deferred for `fallibilis`; the representative check is blocked by
  existing `type Primitive(Valor)` support.
- LLVM text: deferred for `fallibilis`; the representative check is blocked by
  existing `try_call` support.
- The implemented MIR shape reuses ordinary local declarations, assignments,
  and place operands, which the MIR-boundary emitters already inspect/handle in
  their existing local paths.

## Phase 001 Evidence

Validation:

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p exempla script_expected_failure` passed.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
  failed as expected on unclassified live failures.
- `cargo fmt --all -- --check` passed with the repository's existing rustfmt
  `struct_lit_width` warning.
- `git diff --check` passed.

Live script e2e baseline after phase implementation:

- `173/258` exempla files pass end-to-end.
- `175/258` stepper ran.
- `28/258` output checked.
- Classified expected-failure buckets:
  - `frontend-negative`: 2
  - `package-only`: 1
  - `cli-program`: 8
  - `capability-stream`: 10
  - `no-entry-reference`: 9
  - `unsupported-mir`: 29
- Unclassified failures remain visible and fail the gate.

## Pending Units

Recommended next MIR-owned implementation category:

- None currently visible in the live `unsupported-mir` bucket.

Evidence from the live gate:

- The live script harness has no `unsupported-mir` bucket after Phase 034.

Implementation evidence:

- `destructura/literal.fab`, `lista/methodi-functionales.fab`, and
  `lista/methodi-copiae.fab` have all moved out of `unsupported-mir`.

Additional follow-up:

- Rust package execution for `conversio/instans.fab` still fails before
  assertions because Rust codegen boxes `valor ← "..."` as `Valor::Textus`;
  script-mode fixture convergence is complete, but that Rust codegen issue
  remains a separate backend/reference cleanup.

Alternative pending categories:

- If the factory continues beyond `unsupported-mir`, the next work should be a
  fresh classification/review pass over the remaining non-MIR buckets rather
  than another unsupported-MIR implementation phase.

## Deferred Findings

- S-expression, Wasm, and LLVM MIR-boundary inspection was not applicable to
  Phase 001 because no MIR surface was implemented.
- Phase 002 backend checks on `fallibilis` are blocked by pre-existing
  non-const gaps listed above.
- Phase 003 found that the Rust package build path for the same representative
  exempla is not a reliable oracle: `conversio/instans.fab` builds but the
  generated binary panics on `valor to instans conversion failed`, while
  `conversio/fallibilis.fab` fails Rust compilation on const-eval and move
  errors.
