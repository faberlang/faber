# Goal: Stepper-Exclusive `faber test` (No Target Lowering)

**Status**: implemented (2026-07-30) — clean break; residuals in `residuals.md`
**Created**: 2026-07-30
**Target repos**:
- primary: `/Users/ianzepp/work/faberlang/faber` (`faber test` CLI, package load for tests, UX)
- secondary: `/Users/ianzepp/work/faberlang/radix` (HIR proba inventory, MIR lower of test cases, stepper execution, diagnostics)
**Factory artifact dir**: `faber/docs/factory/stepper-faber-test/`
**Primary surfaces**: `faber test`, MIR stepper, `proba` / `probandum` / `adfirma`, package + single-file test load

**Related (do not conflate)**:

| Artifact | Relationship |
| --- | --- |
| [`proba-test-sources/goal.md`](../proba-test-sources/goal.md) | `.proba` discovery and import boundary — **keep**; rehome its “Rust harness generation” claims under this goal |
| [`faber-script-runtime/`](../faber-script-runtime/) | In-process stepper for `faber script` / `-c` / REPL — **reuse**, do not fork a second interpreter |
| Exempla multi-backend e2e (`faber/crates/exempla`) | **Out of scope** — remains optional backend fidelity; never the definition of native Faber tests |
| `faber build` / `faber run` Rust (or other) emit | **Unchanged** — product build still targets languages; only **`faber test`** leaves the target path |

**Commit policy**: one coherent phase per commit where possible; never ship a hybrid “default stepper but Rust fallback” without operator re-authorization (this goal forbids that).

---

## Summary

Convert **`faber test` to run Faber-language tests exclusively on the MIR stepper**.

No lowering of test cases to Rust (or any other codegen target). No `cargo test` harness. No temp crate emission for the test path.

After this goal:

- `proba` / `probandum` / `adfirma` are **language-true** regardless of which backends the product ships.
- Native Faber tests remain valid when the only shipping targets are Go, TS, WASM, LLVM, etc.
- Corpus and package tests can form a **fast, target-neutral** gate before multi-backend e2e.

## North star

```text
faber test <path>
  → load package or single-file sources (include .proba / inline proba)
  → analyze + lower to HIR (test items kept)
  → lower test cases to MIR (shared pipeline, not a codegen backend)
  → for each selected proba case: interpret on mir-stepper
  → report pass / fail / skip (omitte, futurum, filters)
  → process exit 0 iff all selected runnable cases pass
```

**There is no arrow to `Target::Rust`, `cargo test`, or `invoke_cargo_test` on this path.**

Contrast (still legal elsewhere, not as `faber test`):

```text
faber build -t rust|go|ts|…   → product emit
exempla e2e --ignored         → multi-backend corpus fidelity (slow, optional)
```

## Principle

> **Proba proves Faber. Codegen proves a backend.**  
> Collapsing the first into Rust emit makes every non-Rust target a second-class citizen of the test system. That collapse is removed.

> **Clean break.** No dual runner, no `--runner rust` escape hatch, no silent fallback to Cargo when the stepper fails. Unsupported host/capability cases fail closed with actionable diagnostics (or skip via existing `omitte` / `requirit` / `solum-in` semantics where those modifiers already mean “not this environment”).

## Problem

### 1. Implicit “tests require Rust”

Today `faber test` (`faber/src/commands/test.rs`):

1. Compiles the package with test sources included.
2. Emits **Rust only** (`"test only supports Rust backend packages"`).
3. Writes a generated crate and runs **`cargo test`**.

So the product meaning of “native Faber test” became “Rust can run generated `#[test]` functions.” For every other codegen language, proba is either unused or a lie (green on Rust while the shipping target is something else).

### 2. Wrong cost class for language floors

Language / corpus proof should not pay rustc + Cargo for every case. The MIR stepper already executes `adfirma` (assert intrinsics) for `faber script` and many radix stepper tests. The missing piece is a **proba runner**, not a second semantics.

### 3. Coupled implementation debt

Test selection (`TestSelection`), ignore reasons (`omitte` / `futurum`), and filters live beside **Rust codegen** (`radix-codegen-rust`, `TestSelection` in `radix::codegen::rust`). That couples language test policy to one backend crate.

## Goals

1. **`faber test` uses the MIR stepper exclusively** for executing `proba` / `probandum` cases.
2. **Delete** (or fully retire from the test path) Rust emit + `invoke_cargo_test` + generated Cargo test crates for `faber test`.
3. **Preserve** CLI selection semantics where they are language-level: `--name`, `--suite`, `--tag`, `--include` / `--exclude` for `*.proba`, and a harness filter that selects cases by title/path (reimplemented without Cargo name substrings if needed).
4. **Preserve** modifier semantics on the stepper runner: `omitte`, `futurum`, `solum`, tags, and other existing modifiers — map to skip / run / fail consistently; document any intentional change.
5. **Single-file and package** inputs both work (corpus-style `.fab` with inline proba, packages with `.proba` siblings, `faber.toml` packages).
6. **Reuse** analyze / lower / package-MIR / script host bridges already used by `faber script` where they apply; do not invent a parallel frontend.
7. **Reporting**: human-readable case names, suite path, pass/fail/skip counts, non-zero exit on failure; optional verbose / nocapture-equivalent for assert messages.
8. **Docs + help**: README / `faber test --help` / factory notes state that tests are stepper-interpreted, not target-compiled.
9. **Update** `proba-test-sources` language that still says “Rust test harness generation” so the two goals agree.

## Non-goals

- Multi-backend e2e harnesses (`exempla_*_e2e`) — stay optional fidelity, not `faber test`.
- Changing EBNF for `proba` / `probandum` / `adfirma` (unless a hole blocks stepper execution — then minimal, justified).
- Making every corpus file into a package test in this goal (corpus dual-purpose + bulk migration is follow-on).
- Moving the language corpus into Radix (separate discussion).
- Progressive `scripta/test --stage` wiring (follow-on; this goal unlocks stage “proba”).
- Replacing `faber build` / `faber run` target emit.
- Feature-complete host I/O for every package on the stepper — fail closed; do not reintroduce Rust “so tests pass.”
- Keeping a compatibility mode that emits Rust tests “for CI that expects cargo.”

## Ground Truth Researched

- `faber/src/commands/test.rs`: package compile with test options → Rust code string → `emit_generated_crate_with_runtime_plan` → `invoke_cargo_test`.
- `faber/src/package/compile.rs`: `compile_package_with_test_options` / `include_proba`; selection types imported from `radix::codegen::rust::TestSelection`.
- `faber/src/package/cargo.rs`: `invoke_cargo_test` — Cargo harness contract.
- `faber/src/script/mod.rs` + `faber/src/commands/script.rs`: production stepper path (`run_source` / package MIR).
- `faber/src/package/mir.rs`: package MIR linking + norma kernel bridge for interpreted packages; library-import gaps fail closed with “use compiled package execution” — test runner must inherit honest limits, not paper over them with Rust.
- Radix: `proba` lowers to HIR items with `HirTestMetadata`; Rust codegen emits `fn proba_*` + `#[test]` / `#[ignore]` (`radix-codegen-rust`).
- Radix MIR: `adfirma` → assert runtime intrinsics (`mir/lower/runtime.rs`); stepper already runs many `adfirma` programs in `mir/stepper_test.rs`.
- Operator decision (2026-07-30): exclusive stepper; no target lowering for tests; target neutrality of language truth is the reason.

## Reference Packet

Before implementing, inspect:

| Path | Why |
| --- | --- |
| `faber/src/commands/test.rs` | Current exclusive Rust path to delete/replace |
| `faber/src/package/compile.rs` | Test source inclusion + selection wiring |
| `faber/src/package/cargo.rs` | `invoke_cargo_test` retirement from test path |
| `faber/src/script/mod.rs` | Stepper embed API to reuse |
| `faber/src/package/mir.rs` | Package MIR + host bridge constraints |
| `faber/src/cli/` (`TestArgs`) | CLI flags to preserve or re-spec |
| `radix` HIR lower for `Proba` / `Probandum` | Case inventory source of truth |
| `radix` MIR lower for `adfirma` + function items with `test` metadata | Execution units |
| `radix-mir-stepper` / `radix::mir::run_source` | Interpreter |
| `radix-codegen-rust` test emission | **Must no longer be on `faber test` path** |
| `docs/factory/proba-test-sources/goal.md` | Discovery rules to keep |
| Exempla / coreutils packages with inline `proba` | Migration validation samples |
| `triga/src/math.proba`, `norma/exempla/caelum/*.proba` | Sparse `*.proba` samples |

## Constraints And Invariants

1. **Clean break** — no dual runner; no env var that re-enables Cargo tests for `faber test`.
2. **Target neutrality** — success of `faber test` must not require a Rust toolchain beyond what is needed to build the `faber` / `radix` **host tools themselves** (developer builds the CLI with cargo; **running** tests must not invoke rustc on user packages).
3. **Same frontend as product analysis** — do not parse/lower tests with a toy path that diverges from `faber check` / script analysis.
4. **Fail closed on stepper gaps** — unsupported MIR / host / import → diagnostic with issue id; case fails or is skipped only under explicit language modifiers, never “try Rust.”
5. **Production graphs stay clean** — `faber build` / `faber run` still exclude `.proba` and do not run tests.
6. **Deterministic reporting** — stable order of cases (e.g. suite path + source order) for CI diffs.
7. **Private radix + public faber** — runner core may live in radix (testable without faber CLI); faber owns CLI and package discovery.
8. **Do not weaken** existing package tests by marking them all `omitte` to go green.

## Architecture Direction

### Ownership split

| Layer | Owner | Responsibility |
| --- | --- | --- |
| CLI `faber test` | faber | Args, exit code, human report, load path |
| Package / file discovery | faber (existing) | `.proba`, include/exclude, manifest |
| Proba case inventory | radix (HIR) | Enumerate cases + modifiers + suite path from lowered program |
| Case → runnable MIR | radix | Lower each case (with praepara/postpara) to a stepper-invokable unit |
| Execute + assert | radix mir-stepper | Run unit; surface adfirma / trap failures |
| Host / norma bridge | faber package MIR + radix kernel (existing) | Same as script path |

### Execution model (implementer fills detail in delivery)

Preferred shape (adjust only with evidence):

1. Load sources with `include_proba = true` (existing).
2. Analyze + HIR lower once per package/file set.
3. Collect all `HirFunction` (or equivalent items) with `test: Some(HirTestMetadata)`.
4. Apply selection filters (name, suite, tag, solum rules).
5. For each remaining case:
   - If `omitte` / `futurum` (or other skip modifiers) → **skip** with reason.
   - Else lower body to MIR (or select pre-lowered MIR entry) and **step**.
   - `adfirma` failure or stepper error → **fail** that case.
6. Print summary; exit non-zero if any fail.

**Anti-pattern:** emit one giant Rust module of `#[test]` functions.

**Anti-pattern:** run full package `incipit` once and hope tests ran.

### Selection types

Move or dual-define `TestSelection` **out of** `radix::codegen::rust` into a target-neutral module (e.g. `radix` driver/HIR test policy). Rust codegen may keep a thin adapter later for unrelated use, but `faber test` must not import selection from a codegen backend crate as its source of truth.

## Implementation Shape

### Phase 0 — Spec lock (docs only if needed)

- Inventory every CLI flag of `faber test` and map to stepper semantics.
- List blockers: package MIR import gaps, multi-file linkage, praepara/postpara, modifiers.
- Red tests: fixed small fixtures expected to pass/fail/skip on stepper **before** deleting Rust path (red-green).

### Phase 1 — Radix: proba inventory + stepper case runner API

- Public/testable API: given HIR (or session+source), list cases; run one case on stepper; collect result.
- Unit tests: pure arithmetic `proba` with `adfirma` pass/fail; `omitte` skip; nested `probandum` suite path.

### Phase 2 — Faber: wire `faber test` to runner (still may leave dead Rust code)

- `cmd_test` calls stepper runner instead of emit+cargo.
- Single-file + simple package green on fixtures under `faber` tests / examples fixtures.

### Phase 3 — Clean break deletion

- Remove test path use of `invoke_cargo_test`, Rust-only test emit, generated test crate layout for tests.
- Delete or gate tests that only asserted on generated Rust `fn proba_*` **as the definition of faber test** (replace with stepper result assertions).
- Grep guard: `faber test` code path must not reference `Target::Rust` for execution.

### Phase 4 — Package / norma / real consumers

- Prove `triga` / `norma/exempla/caelum` / selected coreutils or fixture packages on stepper, **or** document fail-closed gaps with issue ids and file residuals (not silent skip).
- Align package MIR bridge requirements with script path.

### Phase 5 — Docs, help, related goal sync

- Update help, README claims, `proba-test-sources` Rust-harness wording.
- Note follow-on: corpus dual-purpose proba, `scripta` stage for proba, exempla e2e remains separate.

## Release Posture

Decision: **release checkpoint when `faber test` behavior changes** (public CLI semantics).

- Coordinate faber version note: “`faber test` runs on MIR stepper; no longer invokes cargo/rustc on the package.”
- Operators who scripted `cargo test` after `faber test` emit must drop that expectation.
- No radix crates.io release required solely for this unless public rustc API surface of radix changes for embedding.

## Exit Strategy

Decision: **included as clean break** — no long-term opt-out.

- Short-term emergency only via git revert of the goal’s commits (operator).
- Not authorized: shipping a permanent `--legacy-rust-tests` flag.

If Phase 4 reveals systemic stepper gaps that block *all* real packages, **stop** and file a residual goal for stepper capability — do not restore Rust as the test definition without a new operator decision.

## Acceptance Criteria

- [ ] `faber test` on a fixture package with passing `proba` / `adfirma` exits 0 **without** invoking `cargo test` or writing a package test crate for rustc.
- [ ] A deliberately failing `adfirma` exits non-zero and names the case.
- [ ] `omitte` / `futurum` cases are skipped (or equivalent documented policy), not executed.
- [ ] `--name` / `--suite` / `--tag` (and include/exclude for `.proba`) still select cases.
- [ ] `faber build` / `faber run` do not execute proba and do not require the test runner.
- [ ] No code path from `cmd_test` to `Target::Rust` emit or `invoke_cargo_test`.
- [ ] Documented proof: `faber test` does not require the **package’s** target language to be Rust.
- [ ] Existing CI/docs that claimed “Rust harness” for `faber test` are updated or deleted.
- [ ] Red-green: at least one automated test fails if someone reintroduces Cargo as the test executor for `faber test` (lightweight guard test or path assertion).

## Validation

```bash
# From faberlang/ container layout
cargo test --manifest-path faber/Cargo.toml -p faber --lib <stepper_test_filter>
cargo test --manifest-path radix/Cargo.toml -p radix --lib <proba_runner_filter>

# Manual / script
cargo run --manifest-path faber/Cargo.toml -- test path/to/fixture
# expect: pass/fail/skip lines; no "Compiling" cargo package test harness for the fixture

# Negative: failing adfirma
cargo run --manifest-path faber/Cargo.toml -- test path/to/failing-fixture
# expect: non-zero exit

# Guard
rg -n 'invoke_cargo_test' faber/src/commands/test.rs   # must not appear
rg -n 'Target::Rust' faber/src/commands/test.rs         # must not drive execution
```

Review check: read `cmd_test` end-to-end; confirm architecture principle holds (language proof ≠ backend proof).

## Open Questions

Resolve in Phase 0 delivery; defaults below if unblockable:

| # | Question | Default if unresolved |
| --- | --- | --- |
| Q1 | Exact MIR entry shape per proba case (synthetic entry fn vs stepper “run item” API)? | Prefer minimal API in radix that script path can share |
| Q2 | Multi-file package: always package-MIR link (script path) vs single concatenated unit? | Match `faber script` package behavior |
| Q3 | Cargo-oriented flags (`--exact`, `--test-threads`, `--ignored`, `--include-ignored`) | Re-spec or drop; do not keep Cargo semantics that imply rustc |
| Q4 | Should radix expose `radix test` alias later? | Out of scope; faber remains product CLI |
| Q5 | Parallel case execution on stepper? | Serial first; parallel only if proven safe |

## Stop Conditions

- Stop if implementing this requires **weakening** `adfirma` or proba language semantics to fake green.
- Stop if the only way to pass existing package suites is to **reintroduce Rust emit** — escalate residual (stepper gaps), do not quietly hybridize.
- Stop if package MIR cannot load a class of tests that product depends on **and** no fail-closed diagnostic plan exists — file residual before deleting Rust path for those packages only after operator choice (prefer: fail closed + residual, still delete Rust as definition).
- Stop if scope expands into full exempla e2e rewrite or corpus repo move.

## Handoff Readiness

| Label | Meaning |
| --- | --- |
| **Ready for delivery** | This goal is grounded; next step is a delivery spec with phase file list and red tests |
| **Ready for factory** | After delivery spec + Phase 0 inventory closed |

**Recommended next step:** `$delivery` (or factory production) for Phase 0–1 only; do not implement from this goal alone without a delivery spec that lists concrete files and red fixtures.

## Done When (operator-facing)

`faber test` means: **interpret proba on the MIR stepper.**  
It does **not** mean: compile the package to Rust and ask Cargo.  
Native Faber tests are valid **without** Rust as a product target.
