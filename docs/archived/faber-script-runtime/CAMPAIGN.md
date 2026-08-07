# Campaign: Faber Script Runtime

**Status**: done — Stages 1 and 1b shipped; Stage 2 retired; Stages 3–5 deferred; Stage 6 residual is optional CLI clean-break (not a missing script runtime). Archived 2026-08-07.
**Archived**: 2026-08-07 — product intent (MIR-stepper script/repl/test) delivered; do not reopen this campaign for execution work
**Mode**: draft/maintain -- routing artifact; do not implement directly from
this file.
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Primary surfaces**: `src/` (this repo; script host absorbed former scena),
sibling radix `crates/radix/src/mir/stepper/`, `src/package/mir.rs`

---

## Summary

> **Superseded command model (2026-08-03):** This campaign was originally
> written around `script` as the canonical interpreted command and `run` as
> compiled package execution. The target-build pipeline goal now owns the
> product command contract: `build` produces artifacts, `build --exec` builds
> and executes an artifact, and `run` directly executes source or an existing
> FMIR image. The older Stage 1 wording below is historical implementation
> context and must be migrated before the next command-surface phase.

Create a first-class direct-execution lane for Faber source, packages, and
serialized execution images. The user-facing entry point is `faber run`, backed
by the MIR stepper and package MIR runner. Execution lives in this repo (the
former `scena` embed API was absorbed into `src/script/`); sibling Radix owns
the MIR stepper engine.

The original Stage 1 (`faber script`) and Stage 1b package-host bridging are
shipped and covered by current CLI/package tests. This campaign remains open
for migrating that implementation to the direct `faber run` contract and for
any separately selected runtime diagnostics; the command itself is not
proposed or missing.

This campaign is about direct execution only. Generated Rust/Cargo package
construction stays owned by `faber build`; `faber build --exec` is the explicit
build pipeline that executes the resulting artifact. `faber test` remains its
own MIR-stepper test surface.

## Problem

Today, `faber run` is overloaded: package directories compile to Rust and run
as native binaries, while single `.fab` files interpret through the MIR
stepper. Package interpretation is hidden behind `faber run --interpret`, and
build-selection flags are duplicated on `run`.

The desired contract removes that ambiguity. `faber run` directly executes
source/package inputs or loads an existing `.fmir`/`.fmir.txt` image. `faber
build --exec` owns the build-then-run pipeline. The legacy hidden `scena` target
is removed rather than promoted to a separate binary product.

## Desired End State

- `faber run [path]` runs source through the MIR stepper for files,
  manifest-backed packages, manifestless package directories, and supported
  archives.
- `faber run image.fmir` and `faber run image.fmir.txt` load existing
  source-independent images and execute them without source regeneration.
- `faber build [path]` produces the selected artifact and stops; `faber build
  [path] --exec` executes that exact artifact after building it.
- The old `faber run --interpret` and `--compile` flags are removed rather than
  retained as aliases.
- `script` may be a temporary compatibility alias for direct source execution,
  but it is not a separate runtime semantic and is eventually removable.
- Runtime timing, if retained, reports phase timing for load, parse/analyze, package MIR link,
  MIR lowering, validation, and execution where the implementation can measure
  those phases honestly.
- A future runtime diagnostics surface explains whether a source/package is
  supported by the direct package-MIR surface and why unsupported shapes fail.
- Package sources that run through direct `faber run` and later ship through
  `faber build` do not need import rewrites between lanes. Application source
  uses canonical `norma:*` imports only; direct package mode supplies an
  explicit allowlisted bridge to stepper kernels where support exists.
- Docs and help text describe the lane split without presenting MIR stepping as
  the default application build path.

## Development Posture

- **Clean UX split.** `build` produces artifacts, `build --exec` runs the build
  pipeline, and `run` directly executes a program input.
- **Implementation words stay internal.** Prefer `run` in user-facing help;
  reserve `interpret`, `MIR`, and `stepper` for diagnostics, developer docs, or
  internal implementation notes.
- **No generated-Rust fallback in direct run mode.** Unsupported interpreted package
  shapes must fail with actionable diagnostics.
- **No Scena product.** The hidden legacy target and its source-backed artifact
  route are removed; timing/support work belongs to Faber diagnostics if later
  selected.
- **Timing must be honest.** Do not report phase timings that are only inferred
  from wall-clock wrappers when in-process boundaries can be instrumented.
- **Do not duplicate Radix.** `radix` owns compiler phase inspection; Faber
  owns direct runtime behavior and any future timing, benchmarking, support,
  and tracing diagnostics.
- **One package source, two execution lanes.** Source intended to ship as a
  package should import the ship namespace (`norma:*`). `faber:*` remains the
  direct script/kernel namespace, not a second package dialect. The bridge is a
  backend dispatch rule for interpreted package execution, not source-level
  namespace interchangeability.

## Implementation Workflow

Campaign stages lower to `delivery` and then `factory`. Do not implement code
from this campaign artifact.

1. Select the first planned, unblocked campaign stage.
2. Create a stage delivery spec in this directory.
3. Execute the delivery spec through `factory`.
4. Validate with focused Cargo tests and Faber subprocess tests for the touched
   command surface.
5. Update this campaign only when routing, invariants, gates, or stage status
   change.

## Scope Routing

**In campaign**

- `faber run` direct-execution command shape and help text.
- Shared direct-run plumbing that avoids duplicating source and FMIR loaders.
- Script-runtime timing, benchmark, and support diagnostics.
- Clean removal of `faber run --interpret` and `--compile`.
- Migration or removal of the temporary `faber script` alias.
- Docs/help updates for the build/exec/run split.

**Split out**

- Generated Rust/Cargo performance and cache behavior.
- Package build/test behavior outside command help affected by the UX split.
- New language syntax or grammar.
- Kernel module implementation beyond what script diagnostics need.
- Broad stdlib/runtime unification between `norma` and the stepper. This
  campaign may route a narrow package-interpret bridge for existing stepper
  kernels, but it should not drag the `norma` crate wholesale into `scena`.
- Full debugger/interactive trace UI unless a later campaign explicitly selects
  it.

## Batching And Split Policy

- Stage 1 is `batch-by-default`: add `faber script` and route all existing
  interpreted inputs through it in one delivery slice.
- Stages 2-4 are `split-on-boundary`: split only if CLI plumbing, timing
  instrumentation, and benchmark harnesses contend on shared command code or
  require different validation gates.
- Stage 5 is `discovery-first`: support diagnostics should start with current
  package-MIR rejection surfaces, then batch additional explanations once the
  first reporting pattern is proven.

## Ground Truth Researched

- `AGENTS.md`: `faber build` owns application artifact construction; direct
  MIR execution must not be confused with a package build.
- `src/cli/mod.rs`: `RunArgs` currently exposes
  `--interpret` and `--compile`; `faber` already has `run`, `repl`, `host`, and
  compiler-compatibility aliases.
- `src/commands/run.rs`: single files interpret by default;
  package directories compile by default; `--interpret` routes packages,
  manifestless importing files, and archives through package MIR.
- `src/package/mir.rs`: interpreted package execution uses
  `analyze_package`, package MIR linking, validation, and `run_entry`; library
  imports are currently unsupported and must fail explicitly.
- `crates/radix/src/mir/stepper/kernel/{solum,processus}.rs`: the stepper
  already has file, cwd, env, stdin/stdout, and process primitives needed by
  coreutils-style development loops.
- `crates/radix/src/kernel/mod.rs`: `faber:*` kernel imports are currently
  script-mode only; package builds reject them.
- sibling examples `docs/factory/coreutils/CAMPAIGN.md`: Stage 1b identifies package-mode kernel
  import resolution as the blocker for file-backed stepper slices. The clean
  source-shape decision is one import string for package source: `norma:*`.
- `former scena crate (now `src/script/`) Cargo.toml`: the script host is currently library-embedded and describes
  itself as the in-process Faber script stage.
- `former scena crate (now `src/script/`) src/lib.rs`: public script embed API is `run_source`,
  `run_named`, `run_with_session`, and host/diagnostic exports.
- `crates/radix/src/tool/cli.rs` and `crates/radix/src/bin/radix.rs`: `radix`
  owns compiler developer commands: `lex`, `parse`, `hir`, `mir`, `cli-ir`,
  `check`, `verify`, `emit`, `targets`.

## Current State

| Track | State | Next action |
| --- | --- | --- |
| User direct-run UX | `faber script` exists, but the canonical endpoint is being moved to `faber run`; `run --interpret`/`--compile` remain until the clean break. | Migrate tests/docs and remove duplicated run build flags. |
| Scena target | Retired by the target-build pipeline goal; no binary target is planned. | Remove stale target, artifact, tests, and documentation references. |
| Runtime timing | External wall-clock benchmarks only; no in-process phase report. | Deferred; lower a Faber-owned diagnostic delivery if selected. |
| Runtime benchmark | No built-in repeated-run direct-run benchmark. | Deferred; lower a Faber-owned diagnostic delivery if selected. |
| Package host imports | Stage 1b complete: direct package execution bridges supported `norma:<manifest-module>` imports (`solum`, `processus`, `aleator`, `json`) to the stepper kernel via a post-validation link-time rewrite; unsupported verbs/modules fail closed. One `norma:*` import spelling works on both lanes. | Migrate the execution caller from the old compatibility route to `faber run`. |
| Support diagnostics | Package-MIR unsupported cases are surfaced as normal diagnostics, but no dedicated support command explains support by shape. | Deferred; lower a Faber-owned diagnostic delivery if selected. |
| Docs/help | Current command shape still describes `run --interpret` and build-then-run. | Update alongside the target-build pipeline and clean-break phase. |

## Campaign Path

### Stage 0 - Delivery Baseline

**Status**: complete (2026-07-06)
**Lowers to**: delivery
**Batching posture**: discovery-first
**Output**: [`stage0-baseline.md`](stage0-baseline.md)

Record the current command behavior and test inventory before changing CLI UX.

**Gate**

- Delivery spec lists current `faber run` dispatch behavior, single-file
  default interpretation, package default compilation, archive interpretation,
  and package-MIR unsupported surfaces.
- Existing subprocess tests that exercise `--interpret` are identified.
- Any compatibility decision needed for `faber run --interpret` is explicit.

### Stage 1 - `faber script`

**Status**: complete (2026-07-06)
**Lowers to**: delivery -> factory
**Batching posture**: batch-by-default
**Output**: [`stage1-faber-script.md`](stage1-faber-script.md)

Add `faber script [path]` as the canonical user-facing source execution command.
It must route through the same interpreted execution code currently behind
`faber run --interpret`, without generating Rust or invoking Cargo.

**Gate**

- `faber script file.fab` runs single-file stepper execution.
- `faber script <package-dir|faber.toml|entry.fab>` runs supported package-MIR
  execution.
- `faber script archive.zip` preserves existing archive interpretation behavior.
- `faber script -- ...` or equivalent argument forwarding preserves current
  `faber run --interpret` argv behavior.
- `faber run` keeps package compiled execution as its default.
- Tests prove `faber script` does not emit `target/faber` or invoke Cargo.

### Stage 1b - Package Host Import Bridge

**Status**: complete (2026-07-06)
**Lowers to**: delivery -> factory
**Batching posture**: discovery-first
**Output**: [`stage1b-package-host-bridge.md`](stage1b-package-host-bridge.md)

Allow package interpretation to run supported `norma:*` host imports through the
existing stepper kernels. This closes the source-shape gap for coreutils and
other package applications: one source file imports `norma:*`, then `faber
script` satisfies the supported calls through stepper kernels while compiled
package execution satisfies the same imports through the normal `norma` backing.

**Invariant**

Application/package source uses `norma:*`; `faber:*` remains direct
script/kernel syntax. Interpret mode may bridge selected `norma:*` modules to
stepper kernels, but it must not make all `faber:*` and `norma:*` imports
globally interchangeable.

Coreutils utility source must not carry lane-conditional import blocks or paired
`faber:*`/`norma:*` variants. If a utility needs host I/O in both development
and ship gates, the source imports `norma:*`; unsupported interpreted modules or
verbs fail as package-MIR capability gaps.

**Gate**

- `faber script <package>` or the current `faber run --interpret <package>`
  succeeds for a package that imports `norma:solum` and reads a file through an
  already-implemented stepper kernel verb.
- `norma:processus` support is either included for the coreutils stdin/stdout,
  argv, env, cwd, and exit surface, or explicitly deferred with diagnostics that
  `scena support` can explain.
- Unsupported `norma:*` modules still fail closed with actionable diagnostics;
  they do not fall back to generated Rust/Cargo.
- Package builds continue to use the normal `norma` Rust backing; the bridge is
  limited to interpreted package execution.
- Tests prove package source does not need lane-conditional import rewrites for
  the supported modules.
- At least one coreutils-shaped fixture uses a single `norma:*` import block and
  runs through interpreted package execution without a second stepper-only
  source file.

### Stage 2 - Scena Binary Shell (retired)

**Status**: retired (2026-08-03)
**Lowers to**: delivery -> factory
**Batching posture**: split-on-boundary

Do not add a `scena` binary. The legacy target is removed under the target-build
pipeline goal. Shared direct-execution plumbing belongs to `faber run`; any
temporary `faber script` alias must delegate to the same seam.

**Gate**

- No `scena` target, binary, or command grammar remains in the product surface.
- `faber run [path]` executes the supported source/package/archive surface.
- Shared execution helpers avoid copy-pasting source and FMIR loader logic.
- `radix` phase-inspection commands are not duplicated.

### Stage 3 - Runtime timing (deferred)

**Status**: deferred — lower separately if selected
**Lowers to**: delivery -> factory
**Batching posture**: split-on-boundary

Expose Faber-owned direct-runtime phase timings only if a later delivery selects
that product surface. This campaign does not create a `scena` command.

**Gate**

- A future Faber diagnostic command reports human-readable total and phase
  timings.
- A future machine-readable form has a stable documented shape.
- Timed phases are measured at real code boundaries, not guessed from external
  process wrappers.
- Timing implementation does not materially change normal `faber run` semantics.

### Stage 4 - Runtime benchmark (deferred)

**Status**: deferred — lower separately if selected
**Lowers to**: delivery -> factory
**Batching posture**: split-on-boundary

Provide repeated-run benchmarking for direct-run workloads only if a later
delivery selects it. This campaign does not create a `scena` command.

**Gate**

- A future Faber diagnostic command runs repeated direct executions with a
  configurable warmup.
- Output includes total, mean, min/max, and enough environment metadata to make
  local comparisons meaningful.
- `--json` output is stable for tooling.
- Benchmark command does not hide unsupported package-MIR failures.

### Stage 5 - Runtime support diagnostics (deferred)

**Status**: deferred — lower separately if selected
**Lowers to**: delivery -> factory
**Batching posture**: discovery-first

Explain whether an input is supported by direct execution and why not, if a
later delivery selects that surface. This campaign does not create a `scena`
command.

**Gate**

- A future Faber diagnostic command reports whether the input can run through the current
  stepper/package-MIR surface.
- Unsupported library imports, private namespace errors, unresolved local
  package shapes, and known package-MIR gaps produce actionable explanations.
- `--json` output can be consumed by agents and docs tooling.

### Stage 6 - Compatibility And Documentation Closeout

**Status**: planned
**Lowers to**: delivery -> factory
**Batching posture**: batch-by-default

Finalize the public command story and update docs/help.

**Gate**

- `faber run --interpret` and `--compile` are removed, not retained as aliases.
- `faber --help`, `faber build --help`, and `faber run --help` tell one
  consistent build/exec/run story.
- `README.md`, target capability docs, or relevant help docs distinguish:
  `faber build` = artifact construction, `faber build --exec` = build then
  execute, and `faber run` = direct source/image execution.
- Focused CLI and package-MIR tests pass.

## Dependency Rules

| Situation | Route |
| --- | --- |
| Command UX for ordinary source execution | Stage 1 migration to direct `faber run` |
| Package source imports host I/O and must work in both direct `faber run` and compiled package gates | Stage 1b package host import bridge; require `norma:*` as the package source spelling |
| Coreutils file-backed stepper slices need `solum`/`processus` host effects | Stage 1b here, coordinated with sibling examples `docs/factory/coreutils/CAMPAIGN.md` Stage 1b |
| Runtime diagnostics, timing, benchmark, or support introspection | Deferred Faber-owned delivery; no `scena` product |
| Compiler phase dumps or target emit inspection | `radix`, not this campaign |
| Generated Rust, Cargo cache behavior, package build/test | Existing application-lane work, not this campaign |
| New package-MIR language support discovered while adding commands | Stop and route through a package-MIR delivery spec unless required for the selected stage gate |
| Broad attempt to share all `norma` runtime code with the stepper | Stop and create an architecture delivery spec; do not fold it into Stage 1b |

## First Useful Milestones

- `faber run` replaces `faber run --interpret` as the direct execution route;
  any `script` alias is transitional only.
- Supported package host imports use one package-source spelling (`norma:*`) in
  both the interpreted development lane and compiled ship lane.
- A future Faber-owned diagnostic surface, if selected, produces trustworthy
  phase timings for the next performance discussion.

## Acceptance Criteria

- Campaign stages are ordered and ready to lower through delivery/factory.
- The next selected stage is Stage 0 or Stage 1, depending on whether the
  factory session needs a separate baseline delivery spec.
- The artifact prevents mixing script-runtime work with generated Rust/Cargo
  behavior.
- Every implementation-heavy stage declares a batching posture and gate.

## Validation

Each delivery spec should choose focused validation. Likely commands:

```bash
timeout 1200 cargo test run_interpret
timeout 1200 cargo test package_mir
timeout 1200 cargo test --manifest-path ../radix/Cargo.toml -p radix mir::stepper
timeout 1200 cargo build --release
```

Docs-only campaign maintenance may validate with:

```bash
git diff --check
```

## Open Questions

- Which `norma:*` modules belong in the first package-host bridge slice:
  `solum` only, `solum` plus `processus`, or the whole currently implemented
  stepper kernel allowlist?
- Should timing/support diagnostics be added to `faber run` or to a separate
  Faber-owned diagnostic command?
- What minimum package-MIR support explanation is useful before that diagnostic
  surface is selected?

## Stop Conditions

- Stop before preserving `--interpret` as a compatibility layer if the delivery
  spec cannot prove that compatibility is required.
- Stop before reintroducing `scena` as a product target or binary.
- Stop before adding generated Rust/Cargo behavior to direct `faber run`.
- Stop before making `faber:*` and `norma:*` globally interchangeable instead of
  adding a narrow, explicit package-interpret bridge.
- Stop before implementing a debugger/trace UI unless a delivery spec explicitly
  selects that scope.
