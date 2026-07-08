# Stage 1 — `faber script` (Delivery Spec)

**Campaign stage**: Stage 1 — `faber script`
**Lowers from**: [`CAMPAIGN.md`](CAMPAIGN.md)
**Batching posture**: batch-by-default
**Status**: complete (2026-07-06)
**Date**: 2026-07-06
**Baseline**: [`stage0-baseline.md`](stage0-baseline.md)

## Interpreted Unit

Add `faber script [path] [-- args…]` as the canonical user-facing interpreted
source-execution command. It routes through the **same** interpreted-execution
code currently behind `faber run --interpret` and must never generate Rust or
invoke Cargo. `faber run` keeps compiled package execution as its default; its
`--interpret`/`--compile` flags stay functional and are removed later in
Stage 6 (clean break, per baseline Compatibility Decision).

## Normalized Spec

**Command shape** — `ScriptArgs`:

- `path: PathBuf` (default `"."`) — file / package dir / `faber.toml` / entry / archive.
- `args: Vec<String>` — `trailing_var_arg`, forwarded after `--`.
- No `--interpret` / `--compile` / `--release`. `script` always interprets.

**Routing** — identical to current `cmd_run_interpret`:

| Input | Route |
| --- | --- |
| `.zip` | `package::run_package_mir` over archive input |
| manifest-backed / manifestless-package / manifestless `.fab` declaring `importa` | `package::run_package_mir` |
| other single `.fab` | `script::interpret_source_or_exit` (single-source stepper) |

**Invariants** (must hold after Stage 1):

- `faber script` never writes `target/faber/Cargo.toml` and never invokes Cargo.
- `faber run` compiled default is unchanged; `--interpret`/`--compile` still work.
- No new language syntax, no generated-Rust fallback, no `faber:*`/`norma:*`
  interchange (Stage 1b owns the package host bridge).
- User-facing help uses `script` wording; `interpret`/`MIR`/`stepper` stay
  internal/diagnostic.

## Repo-Aware Baseline

See [`stage0-baseline.md`](stage0-baseline.md) for the full dispatch map,
package-MIR unsupported surfaces, kernel policy, and the 30-function
`--interpret` test inventory. Relevant seams:

- `src/commands/run.rs` — `cmd_run`, `should_interpret`,
  `is_single_fab_file`, `cmd_run_interpret`, `is_package_interpret_input`,
  `manifestless_file_declares_import`, `eprint_archive_diagnostics`,
  `cmd_run_compiled`.
- `src/commands/script.rs` — `cmd_eval` (`-c`), `cmd_repl`.
- `src/script.rs` — single-source stepper entry
  (`interpret_source`, `interpret_source_or_exit`).
- `src/cli/mod.rs` — `Command` enum, `RunArgs`.
- `src/commands/mod.rs` — `dispatch`.
- `tests/run_integration_test.rs` — 30 `run --interpret` tests.
- `src/commands/run_test.rs` — dispatch-predicate unit tests.

## Stage Graph

1. **Extract interpret path into the script command module.** Move
   `cmd_run_interpret` → `commands/script.rs::interpret_path` (`pub(super)`),
   along with `is_package_interpret_input`, `manifestless_file_declares_import`,
   and `eprint_archive_diagnostics`. `run.rs::cmd_run` calls
   `script::interpret_path` on the interpret branch; `should_interpret` and
   `is_single_fab_file` stay in `run.rs` (run-dispatch-local). Move the
   `is_package_interpret_input` unit tests into a new
   `commands/script_test.rs`. *Why move*: `script` is the canonical owner of
   interpreted execution; keeping the implementation in `run.rs` would leave the
   names backwards. This is a focused, justified move — not opportunistic
   reshaping.

2. **Add the `script` subcommand.** `ScriptArgs` in `cli/mod.rs`; `Command::Script`;
   `cmd_script(ScriptArgs)` in `commands/script.rs` → `interpret_path`. Wire into
   `commands/mod.rs::dispatch`.

3. **Migrate the test surface.** Convert the 30 `run --interpret` subprocess
   tests to `faber script` (command slice `"run", "--interpret",` → `"script",`;
   rename `run_interpret_*` → `script_*`). This proves `script` routes the same
   surface with parity. `run --interpret` remains functional (shared function)
   and is removed in Stage 6; no dedicated `run --interpret` test is kept during
   the transition (baseline transition discipline).

4. **Help text.** Add a `Script` doc comment ("Run Faber source through the
   interpreter"). Add a one-line `faber script` mention to
   `docs/help/faber-after-help.md` common flows. Do not touch README lane
   framing (Stage 6 owns closeout docs).

## Implementation Work

Single delivery-sized unit, one factory phase, single writer (no parallelism —
all edits touch this repo). Order: stages 1→4 as listed.

## Checkpoints And Gates

**Stage 1 gate (from campaign):**

- [ ] `faber script file.fab` → single-file stepper.
- [ ] `faber script <package-dir|faber.toml|entry.fab>` → package-MIR.
- [ ] `faber script archive.zip` → archive interpretation.
- [ ] `faber script -- …` → argv forwarding.
- [ ] `faber run` keeps compiled default.
- [ ] Tests prove `faber script` emits no `target/faber` and invokes no Cargo.

**Batching / Split Decision**: batch-by-default. One coherent slice: command +
shared plumbing + test migration + help. Split would only be warranted if the
shared-plumbing refactor and the CLI surface contended on the same files with
conflicting validation — they do not (all in this repo, one writer).

**Release checkpoint**: `defer-release`. `faber script` is additive and
`run --interpret` still works; the user-visible lane split is not closed until
Stage 6 removes the old flags. Release belongs to Stage 6 (or an explicit
mid-campaign release decision by the user).

## Validation

```bash
# Targeted interpret surface (migrated to `faber script`) — 30/30 pass
timeout 1200 cargo test --test run_integration_test
# Dispatch predicate unit tests
timeout 1200 cargo test --bin faber 'commands::script'
timeout 1200 cargo test --bin faber 'commands::run'
# Lint + build
timeout 1200 cargo clippy --all-targets -- -D warnings
timeout 1200 cargo build --release
```

All green at closeout (lib 189, bin 185, integration 30, format 4, hygiene 1,
clippy clean). One pre-existing failure unrelated to this stage:
`package_check_diagnostics_uses_expanded_renderer` in `emit_integration_test.rs`
(diagnostic-rendering rot from the structured-diagnostics workstream — normal
`faber check` now emits expanded `error[SEM010:…]` records; this stage touched
no diagnostic rendering). Left out of scope; flagged for the diagnostics owner.

## Companion Skill Plan

- `factory` — executes this spec (implement → verify → review → commit).
- `housekeeping` — not invoked; production/test file boundaries preserved
  (tests stay in `tests/` and `_test.rs`).
- `poker-face` — optional acceptance check against the Stage 1 gate before
  committing.

## Open Questions

- None blocking. The `--time` convenience and `scena` binary are Stages 2–4,
  explicitly out of scope here.
