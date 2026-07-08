# Campaign: Faber Script Runtime

**Status**: proposed (2026-07-06)
**Mode**: draft/maintain -- routing artifact; do not implement directly from
this file.
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Primary surfaces**: `src/` (this repo; script host absorbed former scena),
sibling radix `crates/radix/src/mir/stepper/`, `src/package/mir.rs`

---

## Summary

Create a first-class source-execution lane for Faber scripts and packages. The
user-facing entry point is `faber script`, backed by the MIR stepper and package
MIR runner. Interpreted execution lives in this repo (former `scena` embed API absorbed into
`src/script/`). Sibling radix owns the MIR stepper engine.

This campaign is about interpreted execution only. The generated Rust/Cargo
package path stays owned by `faber run`, `faber build`, and `faber test`.

## Problem

Today, `faber run` has the right product default: package directories compile to
Rust and run as native binaries, while single `.fab` files interpret through the
MIR stepper. Package interpretation exists but is hidden behind
`faber run --interpret`, which makes source execution feel like a debug backend
override instead of a first-class workflow.

`scena` is already the in-process script stage crate, but it is library-only.
That leaves performance and support questions to ad hoc external timing rather
than runtime-owned diagnostics.

## Desired End State

- `faber script [path]` runs source through the MIR stepper for files,
  manifest-backed packages, manifestless package directories, and supported
  archives.
- `faber run [path]` remains the compiled product path for packages.
- The old `faber run --interpret` path is replaced or retired by an explicit
  compatibility decision, not left as the canonical UX.
- `scena` ships as a binary for script-runtime power users.
- `scena time` reports phase timing for load, parse/analyze, package MIR link,
  MIR lowering, validation, and execution where the implementation can measure
  those phases honestly.
- `scena bench` provides repeated-run timing with warmup and machine-readable
  output.
- `scena support` explains whether a source/package is supported by the
  interpreted package-MIR surface and why unsupported shapes fail.
- Package sources that run through `faber script` and later ship through
  `faber run` do not need import rewrites between lanes. Application source uses
  canonical `norma:*` imports only; interpreted package mode supplies an
  explicit allowlisted bridge to stepper kernels where support exists.
- Docs and help text describe the lane split without presenting MIR stepping as
  the default application build path.

## Development Posture

- **Clean UX split.** `run` means compiled package execution; `script` means
  interpreted source execution.
- **Implementation words stay internal.** Prefer `script` in user-facing help;
  reserve `interpret`, `MIR`, and `stepper` for diagnostics, developer docs, or
  `scena`.
- **No generated-Rust fallback in script mode.** Unsupported interpreted package
  shapes must fail with actionable diagnostics.
- **Scena is a power tool, not the primary user CLI.** Ordinary users should be
  able to use `faber script` without knowing about the `scena` binary.
- **Timing must be honest.** Do not report phase timings that are only inferred
  from wall-clock wrappers when in-process boundaries can be instrumented.
- **Do not duplicate Radix.** `radix` owns compiler phase inspection; `scena`
  owns runtime behavior, timing, benchmarking, support, and future tracing.
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
4. Validate with focused Cargo tests and `faber`/`scena` subprocess tests for
   the touched command surface.
5. Update this campaign only when routing, invariants, gates, or stage status
   change.

## Scope Routing

**In campaign**

- `faber script` command shape and help text.
- Shared interpreted-run plumbing that avoids duplicating `faber run
  --interpret`.
- `scena` binary target and its command grammar.
- Script-runtime timing, benchmark, and support diagnostics.
- Deprecation or clean removal decision for `faber run --interpret`.
- Docs/help updates for the script/runtime lane split.

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

- `AGENTS.md`: `faber run`/`faber build` are application-lane Rust package
  paths; MIR/scena is a systems/script lane and must not be confused with
  package build.
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
| User script UX | `faber script` added as canonical interpreted-source command; 30 `--interpret` subprocess tests migrated to `script`; `run --interpret`/`--compile` retained until Stage 6. | Lower Stage 2 (scena binary) via delivery. |
| Scena binary | No binary target; `scena` is library-only. | Lower Stage 2 after Stage 1 command semantics are stable. |
| Runtime timing | External wall-clock benchmarks only; no in-process phase report. | Lower Stage 3 after shared command plumbing exists. |
| Runtime benchmark | No built-in repeated-run script benchmark. | Lower Stage 4 after timing output shape exists. |
| Package host imports | Stage 1b complete: interpreted package execution bridges supported `norma:<manifest-module>` imports (`solum`, `processus`, `aleator`, `json`) to the stepper kernel via a post-validation link-time rewrite; unsupported verbs/modules fail closed. One `norma:*` import spelling works on both lanes. | Next: Stage 2 (scena binary) via delivery. |
| Support diagnostics | Package-MIR unsupported cases are surfaced as normal diagnostics, but no dedicated support command explains support by shape. | Lower Stage 5 after `scena` command plumbing exists. |
| Docs/help | Current command shape emphasizes `run --interpret`; lane split needs product wording. | Update alongside Stages 1 and 6. |

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

### Stage 2 - Scena Binary Shell

**Status**: planned
**Lowers to**: delivery -> factory
**Batching posture**: split-on-boundary

Add a `scena` binary target with basic command dispatch and shared interpreted
run plumbing.

**Gate**

- `cargo build --release -p scena --bin scena` succeeds.
- `scena run [path]` executes the same supported script/package/archive surface
  as `faber script`.
- Shared execution helpers avoid copy-pasting ``faber`` command logic where a
  library seam is more maintainable.
- `radix` phase-inspection commands are not duplicated.

### Stage 3 - `scena time`

**Status**: planned
**Lowers to**: delivery -> factory
**Batching posture**: split-on-boundary

Expose script-runtime phase timings.

**Gate**

- `scena time [path]` reports human-readable total and phase timings.
- `scena time --json [path]` reports a stable machine-readable shape.
- Timed phases are measured at real code boundaries, not guessed from external
  process wrappers.
- Timing implementation does not materially change normal `faber script` or
  `scena run` semantics.

### Stage 4 - `scena bench`

**Status**: planned
**Lowers to**: delivery -> factory
**Batching posture**: split-on-boundary

Provide repeated-run benchmarking for script workloads.

**Gate**

- `scena bench [path] --n <count>` runs repeated interpreted executions with a
  configurable warmup.
- Output includes total, mean, min/max, and enough environment metadata to make
  local comparisons meaningful.
- `--json` output is stable for tooling.
- Benchmark command does not hide unsupported package-MIR failures.

### Stage 5 - `scena support`

**Status**: planned
**Lowers to**: delivery -> factory
**Batching posture**: discovery-first

Explain whether an input is supported by the interpreted runtime and why not.

**Gate**

- `scena support [path]` reports whether the input can run through the current
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

- Decision recorded for `faber run --interpret`: remove, hide, or keep as a
  temporary alias with deprecation wording.
- `faber --help`, `faber run --help`, `faber script --help`, and `scena --help`
  tell one consistent story.
- `README.md`, target capability docs, or relevant help docs distinguish:
  `faber run` = compiled package execution; `faber script`/`scena` = interpreted
  source execution.
- Focused CLI and package-MIR tests pass.

## Dependency Rules

| Situation | Route |
| --- | --- |
| Command UX for ordinary source execution | Stage 1 (`faber script`) |
| Package source imports host I/O and must work in both `faber script` and compiled package gates | Stage 1b package host import bridge; require `norma:*` as the package source spelling |
| Coreutils file-backed stepper slices need `solum`/`processus` host effects | Stage 1b here, coordinated with sibling examples `docs/factory/coreutils/CAMPAIGN.md` Stage 1b |
| Runtime diagnostics, timing, benchmark, or support introspection | Stages 2-5 (`scena`) |
| Compiler phase dumps or target emit inspection | `radix`, not this campaign |
| Generated Rust, Cargo cache behavior, package build/test | Existing application-lane work, not this campaign |
| New package-MIR language support discovered while adding commands | Stop and route through a package-MIR delivery spec unless required for the selected stage gate |
| Broad attempt to share all `norma` runtime code with the stepper | Stop and create an architecture delivery spec; do not fold it into Stage 1b |

## First Useful Milestones

- `faber script` exists and replaces `faber run --interpret` in help text.
- Supported package host imports use one package-source spelling (`norma:*`) in
  both the interpreted development lane and compiled ship lane.
- `scena run` proves the new binary can execute the same interpreted surface.
- `scena time --json` produces trustworthy phase timings for the next
  performance discussion.

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
timeout 1200 cargo test -p scena
timeout 1200 cargo test --manifest-path ../radix/Cargo.toml -p radix mir::stepper
timeout 1200 cargo build --release
timeout 1200 cargo build --release -p scena --bin scena
```

Docs-only campaign maintenance may validate with:

```bash
git diff --check
```

## Open Questions

- Should `faber run --interpret` be removed immediately, hidden as an alias, or
  kept temporarily with deprecation wording?
- Which `norma:*` modules belong in the first package-host bridge slice:
  `solum` only, `solum` plus `processus`, or the whole currently implemented
  stepper kernel allowlist?
- Should `faber script` accept a `--time` convenience flag, or should all timing
  live under `scena time`?
- Should `scena run` support stdin and `-c` one-liners, or should those remain
  `faber` conveniences?
- What minimum package-MIR support explanation is enough for the first
  `scena support` stage?

## Stop Conditions

- Stop before preserving `--interpret` as a compatibility layer if the delivery
  spec cannot prove that compatibility is required.
- Stop before making `scena` duplicate `radix` compiler inspection commands.
- Stop before adding generated Rust/Cargo behavior to `faber script` or `scena`.
- Stop before making `faber:*` and `norma:*` globally interchangeable instead of
  adding a narrow, explicit package-interpret bridge.
- Stop before implementing a debugger/trace UI unless a delivery spec explicitly
  selects that scope.
