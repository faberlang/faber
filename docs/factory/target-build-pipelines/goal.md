# Target Build Pipeline Completion

**Status**: proposed — follow-on to the Radix target/emit contract; blocked until that goal completes
**Created**: 2026-08-03
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/target-build-pipelines/`
**Prerequisite**: [`radix/docs/factory/target-emit-contract/goal.md`](../../../radix/docs/factory/target-emit-contract/goal.md)
**Related**:
- [`src/package/cmd.rs`](../../src/package/cmd.rs) — package build routing
- [`src/package/compile.rs`](../../src/package/compile.rs) — package target assembly boundary
- [`src/package/cargo.rs`](../../src/package/cargo.rs) — generated Rust crate and Cargo invocation
- [`src/package/fhir.rs`](../../src/package/fhir.rs) — FHIR package assembly/load/run
- [`src/package/mir.rs`](../../src/package/mir.rs) — FMIR image and runner assembly
- [`../radix/docs/design/target-capability-matrix.md`](../../../radix/docs/design/target-capability-matrix.md) — target policy reference

## Summary

After Radix establishes canonical backend target names and a strict immediate
emit boundary, make `faber build` and `faber run` complete as much of the
appropriate downstream pipeline as each target can honestly support.

Faber owns package discovery, dependency/module assembly, intermediate-artifact
materialization, external toolchain invocation, final artifact layout, and
execution/host selection. It must reuse Radix's emit contract rather than
inventing target-specific source generation or treating a file emission as a
completed build.

The intended lifecycle is:

```text
emit:  Faber source/package → Radix immediate target artifact
build: Faber source/package → final target artifact
exec:  build → execute the final target artifact
run:   source/package/image → direct program execution
```

`emit` and `build` are different operations even when they use the same target
name. Radix owns `emit` and stops at the immediate target artifact. Faber owns
`build`, including package assembly, external toolchains, linking, and final
artifact layout. `build --exec` is the explicit build pipeline that executes
the artifact after a successful build.

`run` is execution-only. Its input determines the execution route: source or a
source package is lowered and stepped directly; a serialized FMIR image is
loaded and stepped directly; a FHIR package is loaded and lowered to FMIR when
that loader is supported. `run` does not select a compiler target, release
profile, or build mode.

This goal is a remapping exercise where the downstream process already exists,
and a toolchain-integration exercise where the local compiler/linker exists but
has never been wired into Faber. It must not advertise a stage that only emits
text or passes a superficial validator.

The scope also removes `faber run --interpret` and its paired `--compile`
override. Direct execution belongs to `faber run`, including existing `.fmir`
and `.fmir.txt` payloads. `faber script` is not a second execution semantic; it
may remain temporarily as a compatibility alias while callers migrate, but it
is not the canonical command and carries no build flags. The legacy hidden
`scena` target and its source-backed artifact route are removed.

## Product rules

### `build`

`faber build --target <target>` must either:

1. produce the target's final package, module, image, library, or executable;
2. produce a deliberately named intermediate product when that is the target's
   complete contract, such as a portable FHIR or FMIR image; or
3. fail with an explicit “build stage unavailable” diagnostic.

`build` must not report success merely because Radix emitted source text.

`faber build <input> --target <target> --exec` is the explicit build-and-execute
pipeline. It first completes the same build and then executes the resulting
artifact. For `rust`, this means running the linked native executable. For
`fmir`, this means loading the newly built FMIR payload through the stepper;
it does not wrap Faber user code in a Rust executable unless an explicit native
runner mode is selected by the FMIR build contract.

### `run`

`faber run <input> [-- <args>...]` must execute the input through the correct
direct runtime or loader without first compiling a target artifact. Supported
input classes are:

- Faber source, a package, or a supported source archive: analyze, lower, and
  execute through the MIR stepper;
- `.fmir` or `.fmir.txt`: load the existing source-independent FMIR image and
  execute it through the stepper;
- a supported FHIR package: load it, lower it to FMIR, and execute it.

`run` may retain source/runtime diagnostics such as reader-locale selection,
but it must not expose build-selection flags such as `--target` or `--release`.
It must not invoke Cargo or a target compiler merely because the input is a
package. A native runner around FMIR is a packaging mode of the FMIR build,
not evidence that the Faber program was natively compiled.

`faber run` is the public execution command even when the input is commonly
called a script. `faber script`, if retained during migration, delegates to
the same input dispatcher and is eventually removable without changing the
execution model.

### Toolchain discovery

External toolchains may be used when they are locally available, but discovery
must be explicit, coherent, and diagnosable. The implementation must record or
report the selected toolchain, reject incompatible versions, preserve a stable
artifact layout, and fail closed when the required runtime or host ABI is not
available.

## Target completion matrix

The following is the starting contract after Radix normalization. “Current”
describes what exists today; “goal endpoint” describes the strongest honest
Faber behavior this goal should attempt to wire.

| Target | Immediate emit | Current Faber stage | Goal build artifact | `build --exec` / direct `run` |
| ------ | --------------- | ------------------ | ------------------ | --------------------------- |
| `rust` | Rust source | Cargo package build and native run already work | Linked native executable | Execute the native artifact; direct source `run` remains stepper execution |
| `fhir` | Serialized HIR | Package envelope build and source-free HIR → FMIR run already work | FHIR package | Load, lower to FMIR, and execute; direct FHIR `run` uses the same loader |
| `fmir` | FMIR image | Text image, binary image, and native FMIR runner paths already exist under split names | One FMIR target with an explicit image format | Load the newly built or existing image through the stepper; native runner is an explicit packaging mode |
| `faber` | Faber source | Re-emission only | None | Keep emit-only; source inputs can still use direct `run` when the stepper supports them |
| `go` | Go source | Emit only | Go package or executable when package/runtime/entry contracts are complete | Execute only after a real Go build and run contract exists |
| `swift` | Swift source | Emit only | Swift package or executable when the generated layout is valid | Execute only after a real Swift build and run contract exists |
| `ts` | TypeScript source | Emit only | JavaScript/TypeScript package or executable when module/runtime contracts are explicit | Execute only after a real compiler and runtime contract exists |
| `wasm` | Wasm module | Emit only; external host required | Reproducible Wasm module/package | Execute only through an explicit Wasm host contract |
| `llvm` | LLVM IR, with text/bitcode format explicit | External verify/link harness exists, but no product target | Linked native executable for the supported subset | Execute the linked artifact; direct source `run` does not silently select LLVM |
| `metal` | Metal source, with library format explicit | Emit only | AIR/metallib when the kernel ABI is complete | Dispatch only through an explicit host/provider contract |
| `wgsl` | WGSL source | Emit only; external validation exists | Validated/package artifact when a WebGPU host contract exists | Execute only through a real WebGPU host/launch contract |
| `sexp` | Racket/S-expression source | Emit only; external validation route | None unless an intentional Racket package contract exists | Keep validation distinct from Faber product execution |

The matrix is a plan and must be refreshed from live source before each
implementation phase. “Local tool installed” is not sufficient evidence for a
product build: the generated artifact, runtime imports, package graph,
entrypoint, and observable run behavior must all be proven.

## Implementation stages

### Stage 0 — Import the Radix contract

- Consume the canonical target names and artifact kinds from the completed
  Radix goal.
- Remove Faber-facing `-text`, `-bin`, and `-host` target identities.
- Delete the legacy `scena` target and its source-backed package-artifact
  routing. Migrate direct source and payload execution to `faber run`; keep
  source-independent package construction under the single `fmir` target with
  explicit format/runner options.
- Extend `faber run` input dispatch to recognize `.fmir` and `.fmir.txt`
  images, load them through the existing FMIR loaders, and execute them on the
  stepper without reading or rebuilding source.
- Remove `RunArgs.interpret` and `RunArgs.compile`, their parser/help entries,
  the `should_interpret` override branch, and direct `run --interpret`/
  `run --compile` references. Migrate interpreted tests and docs to `faber run`
  before deleting the compatibility surface; retain `faber script` only as a
  temporary alias if needed for migration.
- Move compiler/build-selection options such as `--target`, `--release`,
  output directory, and format selection off `run` and onto `build`.
- Add `build --exec` as the sole explicit build-and-execute pipeline. Its
  implementation must execute the final artifact produced by the selected
  target, including loading an FMIR payload rather than rebuilding it.
- Define explicit format/runner options for FMIR and any other target with
  multiple downstream artifact forms.
- Rewrite `faber targets` so `emit`, `build`, `package`, and `run` are separate
  capabilities rather than one overloaded build flag.

### Stage 1 — Preserve existing product routes

- Keep Rust → generated crate → Cargo → native executable behavior green.
- Keep FHIR package envelope/load/run behavior green.
- Remove `scena` build/run routing, stale help text, tests, artifact constants,
  and manifest handling; do not preserve a hidden compatibility path.
- Remap FMIR text/binary image and native-runner paths under one FMIR target
  without changing the semantics of the stepper or falsely calling the runner
  native user-code compilation.
- Make `faber run` the direct source/package/image execution route and make
  `faber build --exec` the build-then-run route.
- Make the hidden `__fmir-run` command an implementation seam only; it must not
  remain the required public way to execute a serialized FMIR payload.
- Remove duplicated target-name translation and stale suffix references.

### Stage 2 — Build-plan and artifact boundary

- Introduce or consolidate a target build plan that records immediate emit
  artifact, package inputs, external commands, final artifact, runtime/host,
  and supported `build --exec` mode.
- Materialize intermediates under stable target directories for diagnostics and
  reproducibility, without making intermediate source the final build result.
- Make unsupported build stages fail closed with structured diagnostics.
- Keep package graph, local import, Norma, CLI argument, and exit-code handling
  explicit for every target that claims package build/run.

### Stage 3 — Wire available local toolchains

Prioritize by existing evidence and smallest complete vertical slice:

1. LLVM host build for the proven supported subset: emitted `.ll`, external
   verification, runtime archive, native link, executable path, and output/exit
   parity against Rust.
2. Metal compilation for a proven kernel ABI slice: MSL → AIR/metallib and
   artifact inspection; host dispatch only where the provider contract is live.
3. Wasm validation/module packaging and an explicit host route where imports
   and entrypoint behavior are complete.
4. Go, Swift, and TypeScript package compilation only after their package graph,
   generated module layout, runtime dependencies, and executable/run semantics
   are sufficient for a real build.
5. WGSL and S-expression validation remain honest validation stages unless a
   product host/runtime contract exists.

The exact order may change after toolchain probes, but no target receives a
`build=yes` claim from tool presence alone.

### Stage 4 — Run and host completion

- Route `faber build --exec` through the final artifact for native executables,
  portable loaders, external Wasm/Racket hosts, and GPU host/provider paths.
- Route `faber run` directly from source or an existing portable image. It has
  no compiler, target, release, or build-mode flags after the clean break.
- Keep any temporary `faber script` alias thin and semantically identical to
  direct `run`; do not maintain a second runtime path.
- Keep compile-only, validate-only, package-only, and run-capable states
  distinct in the capability table.
- Add bounded executable/output/exit parity fixtures for each promoted target.
- Ensure command-line args, local imports, package dependencies, and diagnostics
  survive the target-specific pipeline.

### Stage 5 — Matrix and release truth

- Update `faber targets`, `faber build --help`, `faber run --help`, README, and
  target capability documentation.
- Add target rows to the product support matrix with artifact kind, toolchain,
  runtime, and evidence tier.
- Separate emission, validation, compilation, packaging, linking, and run
  evidence in the exempla/parity harnesses.
- Record unresolved backend/runtime gaps as named residuals rather than
  downgrading the target contract.

## Acceptance criteria

- [ ] Faber uses the canonical Radix target names with no lifecycle suffixes.
- [ ] `scena` is removed from parsing, target discovery, build/run routing,
  artifacts, tests, and current documentation.
- [ ] `faber run --interpret` and `faber run --compile` are removed rather than
  retained as aliases; direct-execution tests/docs use `faber run`.
- [ ] `faber run image.fmir` and `faber run image.fmir.txt` load and run
  existing payloads without source access, Cargo, or payload regeneration.
- [ ] Malformed, unsupported-version, wrong-target, and runtime-requirement
  failures for direct FMIR script inputs fail closed with structured diagnostics.
- [ ] The hidden `__fmir-run` command is no longer required as the public
  payload-execution contract and can delegate to the same direct-run loader
  seam.
- [ ] `faber build <input>` stops after producing its final artifact and does
  not execute it by default.
- [ ] `faber build <input> --exec` builds once and executes that exact artifact;
  it does not dispatch through a second implicit build.
- [ ] `run` no longer exposes duplicated build flags such as `--target`,
  `--release`, `--interpret`, or `--compile`.
- [ ] `faber targets` reports separate `emit`, `build`, `package`, and `run`
  behavior with artifact and toolchain notes.
- [ ] Rust, FHIR, and FMIR existing product paths remain green after remapping.
- [ ] `faber build --target llvm` either produces a linked native executable
  for its proven subset or fails with a precise unsupported/build-stage
  diagnostic; it never stops at an unlinked `.ll` while claiming success.
- [ ] LLVM toolchain discovery selects coherent `llvm-as`/`clang` tools and
  links the correct Faber runtime archive.
- [ ] Metal build behavior distinguishes MSL source, compiled library, and
  host dispatch; compilation alone does not claim kernel execution.
- [ ] Wasm, WGSL, S-expression, Go, Swift, and TypeScript rows distinguish
  emitted/validated artifacts from actual package builds and runs.
- [ ] FMIR text/binary image and native-runner choices are explicit build modes,
  not separate backend identities.
- [ ] Each target either has a real build/run proof or a stable fail-closed
  diagnostic and documented residual.
- [ ] No target row claims native execution solely because an external command
  exists on the local machine.

## Non-goals

- Reworking Radix HIR/MIR lowering or target names before its prerequisite goal
  is complete.
- Making every backend feature-complete or semantically parity-complete in one
  factory goal.
- Adding in-process LLVM bindings or a JIT when the external LLVM path is the
  selected contract.
- Treating FMIR's native runner as native compilation of Faber code.
- Claiming Metal or WebGPU dispatch without a live host/provider contract.
- Silently falling back to Rust when a selected non-Rust target cannot build.

## Validation

Validation must be target-specific and cheap-first:

```bash
cargo test --manifest-path faber/Cargo.toml -p faber --lib target
cargo test --manifest-path faber/Cargo.toml -p faber --lib package
cargo run --manifest-path faber/Cargo.toml -- targets

# Product smoke commands are added per promoted target.
cargo run --manifest-path faber/Cargo.toml -- build --target rust <package>
cargo run --manifest-path faber/Cargo.toml -- build --target rust <package> --exec
cargo run --manifest-path faber/Cargo.toml -- run <package>
```

LLVM, Metal, Wasm, and other external-toolchain checks must capture the
selected tool path/version, emitted intermediate, final artifact kind, and
run/validation result. A clean source emission is not a build or run proof.

## Stop conditions

- Stop if a target requires a new runtime ABI, host provider, or package-linking
  design larger than the current build-plan boundary; file that as a separate
  goal rather than hiding it behind a tool invocation.
- Stop if the only evidence is that a compiler binary is installed locally.
- Stop if implementation restores target suffixes to encode lifecycle stages.
- Stop if `build --exec` falls back silently to Rust, FMIR, or another runtime
  without an explicit product contract, or if direct `run` silently compiles a
  target artifact.
- Stop if the build matrix becomes a parity campaign for every language feature;
  split backend semantic burn-down into target-specific goals.
