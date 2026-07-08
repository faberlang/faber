# Stage 0 — Delivery Baseline (Script Runtime Campaign)

**Campaign stage**: Stage 0 — Delivery Baseline
**Lowers from**: [`CAMPAIGN.md`](CAMPAIGN.md)
**Batching posture**: discovery-first (docs-only; no code change)
**Status**: complete
**Date**: 2026-07-06

> Record the current `faber run` dispatch behavior and interpreted-execution test
> inventory before any CLI UX change in later stages. This stage produces this
> baseline document only; it does not modify code.

## Interpreted Unit

Pin the as-is interpreted-execution surface of the `faber` CLI so that Stage 1
(`faber script`) can route the **same** inputs through the **same** code and
prove behavioral parity, and so the Stage 6 compatibility decision for
`faber run --interpret` rests on recorded evidence rather than memory.

## Normalized Spec

Stage 0 must establish three facts before Stage 1 begins:

1. **Dispatch behavior** — exactly which `faber run` inputs interpret today, and
   which fall through to compiled execution.
2. **Test inventory** — the full set of subprocess tests that exercise
   `--interpret`, so Stage 1 can migrate the command surface without losing
   coverage.
3. **Compatibility decision** — an explicit, recorded decision for the future of
   `faber run --interpret` / `--compile`, so Stage 1 and Stage 6 are not blocked
   by an unresolved design question.

No code, help text, or test changes belong to this stage.

## Repo-Aware Baseline

### Dispatch entry points (`src/commands/run.rs`)

`cmd_run(args)` calls `should_interpret(&args, &input_path)`:

- `args.compile == true` → compiled (explicit override).
- `args.interpret == true` → interpret (explicit override).
- otherwise → interpret iff `is_single_fab_file(path)` (file with `.fab`
  extension).

When interpreting, `cmd_run_interpret(path, program_args)` routes in order:

| Input shape | Route | Function |
| --- | --- | --- |
| `.zip` archive (via `archive::is_zip_archive`) | package MIR | `package::run_package_mir` over `archive.package_input()` |
| manifest-backed dir / `faber.toml` / manifestless package dir / manifestless `.fab` that declares `importa` (via `is_package_interpret_input`) | package MIR | `package::run_package_mir` |
| everything else (single `.fab` with no imports) | single-source stepper | `script::interpret_source_or_exit` |

When compiling, `cmd_run_compiled(args)` runs the full package lane:
`package::compile_package` (Rust) → `emit_generated_crate` →
`invoke_cargo_build` → spawn the binary, forwarding `args.args` and the process
exit code.

### Single-source stepper entry (`src/script.rs`)

- `interpret_source(name, source, host)` — analyze/lower/interpret one source
  string; returns `Result<ExitCode, RunSourceError>`.
- `interpret_source_or_exit(name, source, host)` — wraps the above and exits the
  process on error.

These sit on top of `scena`/`radix` stepper primitives (`StdioHost`,
`run_source`).

### Package MIR runner (`src/package/mir.rs::run_package_mir`)

Pipeline: `analyze_package` → error check → `library_import_diagnostics` →
`plan_cli_package` → `local_namespace_call_targets` → `select_entry_unit` →
`rewrite_unit_namespace_calls` → `lower_package_units` → `validate_program` →
`run_entry`.

**Currently unsupported package-MIR surfaces** (each returns an actionable
diagnostic; none fall back to Cargo):

- Any library import (`norma:*` **and** any other library path) —
  `library_import_diagnostics` rejects the first binding per unit with
  *"package MIR does not yet support library imports such as `X`"*. This is the
  Stage 1b blocker for host-I/O packages.
- Private namespace exports — *"namespace does not export `<name>`"*.
- Import cycles — *"import cycle detected"*.
- Cross-unit text/symbol remapping — *"cross-unit symbol remapping is not
  implemented"* (e.g. a module function returning `textus`).
- Zero or multiple entry units — *"package MIR run requires exactly one entry
  unit"* / *"… found multiple entry units"*.
- CLI shapes beyond a documented support matrix: subcommand dispatch options,
  non-scalar/optional/global variants beyond what `plan_cli_*` recognizes,
  dynamic exit expressions, unsupported operand defaults.

### Kernel import policy (`crates/radix/src/kernel/`)

- `faber:<module>` is **script-mode only**. Package builds reject `faber:*`
  imports (`kernel_script_mode_only_message`).
- The kernel manifest (`manifest.rs`) declares four modules — `solum`,
  `processus`, `aleator`, `json` — each with a `verbs` list that is a **subset**
  of the matching sibling `../norma/src/<module>.fab` public surface (parity contract).
- `faber:*` import paths are flat (`faber:solum`); nested paths
  (`faber:hal/solum`) are invalid.
- Consequently: application/package source must spell host imports as
  `norma:*`; direct script/kernel source spells them `faber:*`. They are **not**
  interchangeable. The Stage 1b bridge must keep this invariant.

### Script host (former script host (former scena))

The standalone script host (former scena) is **gone** — absorbed into this repo (`src/script/`
and package MIR host). Public embed API remains conceptually `run_source` /
session-based interpret through `radix::mir::run_source` with a trap that
converts `processus.exi`/`exi` into an `ExitCode` rather than exiting the
embedder.

### Test inventory — `tests/run_integration_test.rs`

All interpreted-execution coverage lives in this one subprocess test file and
exercises `faber run --interpret <path>` against the built `faber` binary
(`CARGO_BIN_EXE_faber`). 30 test functions, grouped:

**Failure / unsupported surfaces (must keep failing, no Rust emit):**
- `run_interpret_private_namespace_failure_does_not_emit_rust`
- `run_interpret_import_cycle_failure_does_not_emit_rust`
- `run_interpret_norma_import_failure_does_not_emit_rust` ← Stage 1b flips this
  for bridged `norma:*` modules
- `run_interpret_text_return_package_reports_symbol_remap_gap_without_rust_emit`

**CLI root execution (success, no Cargo/Rust emit):**
- `run_interpret_cli_root_package_executes_without_cargo_or_rust_emit`
- `run_interpret_cli_root_text_operand_…`
- `run_interpret_cli_root_rest_text_operand_…`
- `run_interpret_cli_root_fixed_exit_uses_process_status_without_rust_emit`
- `run_interpret_cli_root_flag_option_…`
- `run_interpret_cli_root_defaulted_option_…`
- `run_interpret_cli_root_optional_option_…`
- `run_interpret_cli_root_global_flag_option_…`
- `run_interpret_cli_root_defaulted_operand_…`

**CLI mounted subcommands (success, no Cargo/Rust emit):**
- `run_interpret_cli_mounted_command_…`
- `run_interpret_cli_mounted_global_option_…`
- `run_interpret_cli_mounted_global_optional_option_…`
- `run_interpret_cli_mounted_global_text_operand_…`
- `run_interpret_cli_mounted_text_operand_…`
- `run_interpret_cli_mounted_numerus_operand_…`
- `run_interpret_cli_mounted_defaulted_option_…`
- `run_interpret_cli_mounted_alias_text_operand_…`

**Archive interpretation (success + safety):**
- `run_interpret_archive_root_manifest_…`
- `run_interpret_archive_wrapped_package_…`
- `run_interpret_archive_manifestless_root_…`
- `run_interpret_archive_rejects_parent_traversal_entry`
- `run_interpret_archive_reports_missing_package_root`
- `run_interpret_archive_package_diagnostic_reports_archive_member_path`

**Package / single-file execution (success, no Cargo/Rust emit):**
- `run_interpret_manifestless_entry_package_…`
- `run_interpret_package_inputs_execute_without_cargo_or_rust_emit`
- `run_interpret_manifestless_single_file_uses_single_source_stepper`

Additional unit-level coverage in `src/commands/run_test.rs`
proves the dispatch predicates: `should_interpret` defaults,
`is_single_fab_file`, `is_package_interpret_input` (manifest, manifestless
import, manifestless single-file negative).

Every test asserts `assert_no_generated_rust` (no `target/faber/Cargo.toml`)
and absence of `Compiling`/`cargo` in stderr where relevant — these are the
parity invariants Stage 1 must preserve when migrating to `faber script`.

## Compatibility Decision (`faber run --interpret` / `--compile`)

**Decision**: clean break. Stage 1 adds `faber script` as the canonical
interpreted-source command; `faber run --interpret` and `--compile` are
**retired by removal in Stage 6**, not preserved as aliases.

**Rationale** (per repo `AGENTS.md` "Default Change Stance" — backward
compatibility is guilty until proven required; and the campaign's
"Development Posture" + Stop Condition against preserving `--interpret` as a
compatibility layer without proof):

- The repo default is clean-break unless compatibility is explicitly requested.
  It has not been requested.
- Keeping `--interpret` as an alias would leave two canonical ways to invoke
  interpreted source, contradicting the campaign's "Clean UX split" posture
  (`run` = compiled; `script` = interpreted).
- The 28+ tests already prove the underlying shared function; aliasing the flag
  adds maintenance surface with no proven contract.

**Transition discipline** (governs Stages 1–5 so mid-campaign state stays safe):

- Stages 1–5 keep `run --interpret`/`--compile` **working unchanged** (same
  shared function `faber script` calls). No behavioral break mid-campaign.
- Stage 1 migrates the 30 `--interpret` subprocess tests to `faber script`
  (renamed or parametrized) to prove parity; during that window `run
  --interpret` may become indirectly-covered rather than directly tested —
  acceptable because the shared function is the tested seam.
- Stage 6 removes the `interpret`/`compile` fields from `RunArgs`, the
  `should_interpret` branch, the corresponding help text, and any remaining
  direct `--interpret` references.

**Confirmation gate**: Stage 6's removal is the single point at which the
user-visible flag disappears. If the user requests aliasing before Stage 6,
record the override there; otherwise removal proceeds as the default.

## Checkpoints And Gates

**Stage 0 gate (this document):**

- [x] Current `faber run` dispatch behavior recorded (single-file default
      interpretation, package default compilation, archive interpretation,
      package-MIR unsupported surfaces) — see *Repo-Aware Baseline*.
- [x] Existing subprocess tests that exercise `--interpret` identified — see
      *Test inventory* (30 functions in `run_integration_test.rs`).
- [x] Compatibility decision for `faber run --interpret` is explicit — see
      *Compatibility Decision* (clean break; removed in Stage 6).

**Batching / Split Decision**: not applicable (discovery, docs-only).

**Release checkpoint**: `defer-release`. No user-visible change in Stage 0;
release belongs to whichever later stage crosses a user-visible CLI change
(Stage 1 adds `faber script`; Stage 6 removes `--interpret`/`--compile`).

## Validation

Docs-only stage:

```bash
git diff --check
```

No code, test, or help-text change is expected from this stage.

## Companion Skill Plan

- `delivery` — produced this baseline.
- Stage 1 lowers next via `delivery` → `factory` (see `stage1-faber-script.md`).

## Open Questions Inherited By Later Stages

- Stage 1b: which `norma:*` modules belong in the first package-host bridge
  slice (`solum` only, `solum` + `processus`, or the full four-module manifest)?
  The coreutils stdin/stdout/argv surface leans on `processus`.
- Stage 1b: implementation seam for the bridge — does `library_import_diagnostics`
  gain an allowlist that rewrites supported `norma:*` imports to `MirProvider`
  kernel providers, or does package MIR learn a new `norma:*` provider kind?
  To be decided in the Stage 1b delivery spec against the containment rules.
- Stage 3: should `faber script` gain a `--time` convenience, or is all timing
  owned by `scena time`?
