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

The intended product pipeline is:

```text
Faber source/package
  → analyze and resolve package graph
  → Radix immediate target emit
  → target-specific package/compiler/link stage
  → final artifact
  → target-specific run/load/dispatch stage
```

This goal is a remapping exercise where the downstream process already exists,
and a toolchain-integration exercise where the local compiler/linker exists but
has never been wired into Faber. It must not advertise a stage that only emits
text or passes a superficial validator.

## Product rules

### `build`

`faber build --target <target>` must either:

1. produce the target's final package, module, image, library, or executable;
2. produce a deliberately named intermediate product when that is the target's
   complete contract, such as a portable FHIR or FMIR image; or
3. fail with an explicit “build stage unavailable” diagnostic.

`build` must not report success merely because Radix emitted source text.

### `run`

`faber run --target <target>` must build or locate the target artifact and then
execute it through the correct runtime, loader, external host, or device
dispatch path. A native runner around FMIR is a packaging mode of the FMIR
target, not evidence that the Faber program was natively compiled.

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

| Target | Immediate emit | Current Faber stage | Goal endpoint |
| ------ | --------------- | ------------------ | ------------- |
| `rust` | Rust source | Cargo package build and native run already work | Preserve and remap; keep native executable as the endpoint |
| `fhir` | Serialized HIR | Package envelope build and source-free HIR → FMIR run already work | Preserve; make package/load/run semantics explicit |
| `fmir` | FMIR image | Text image, binary image, and native FMIR runner paths already exist under split names | One FMIR target with explicit image format and optional native runner mode |
| `faber` | Faber source | Re-emission only | Keep emit-only; no artificial build stage |
| `go` | Go source | Emit only | Assemble package and invoke `go build` only when package/runtime/entry contracts are complete |
| `swift` | Swift source | Emit only | Assemble package and invoke `swiftc`/Swift package tooling only when the generated layout is valid |
| `ts` | TypeScript source | Emit only | Wire the appropriate TypeScript/JavaScript compiler and runtime only when package and module contracts are explicit |
| `wasm` | Wasm module | Emit only; external host required | Build a reproducible module/package artifact; add run only through an explicit Wasm host contract |
| `llvm` | LLVM IR text | External verify/link harness exists, but no product target | Wire `llvm-as`/`opt` as appropriate, link with `clang` and the Faber LLVM runtime, and produce a native executable for the supported subset |
| `metal` | Metal source | Emit only | Wire Apple Metal compilation (`xcrun metal`/`metallib`) where the kernel ABI is complete; add dispatch only through an explicit host/provider contract |
| `wgsl` | WGSL source | Emit only; external validation exists | Wire validation and any WebGPU package/host route that has a real artifact and launch contract |
| `sexp` | Racket source | Emit only; external validation route | Keep validation distinct from Faber product build; add run only if an intentional Racket runtime contract exists |

The matrix is a plan and must be refreshed from live source before each
implementation phase. “Local tool installed” is not sufficient evidence for a
product build: the generated artifact, runtime imports, package graph,
entrypoint, and observable run behavior must all be proven.

## Implementation stages

### Stage 0 — Import the Radix contract

- Consume the canonical target names and artifact kinds from the completed
  Radix goal.
- Remove Faber-facing `-text`, `-bin`, and `-host` target identities.
- Define explicit format/runner options for FMIR and any other target with
  multiple downstream artifact forms.
- Rewrite `faber targets` so `emit`, `build`, `package`, and `run` are separate
  capabilities rather than one overloaded build flag.

### Stage 1 — Preserve existing product routes

- Keep Rust → generated crate → Cargo → native executable behavior green.
- Keep FHIR package envelope/load/run behavior green.
- Remap FMIR text/binary image and native-runner paths under one FMIR target
  without changing the semantics of the stepper or falsely calling the runner
  native user-code compilation.
- Remove duplicated target-name translation and stale suffix references.

### Stage 2 — Build-plan and artifact boundary

- Introduce or consolidate a target build plan that records immediate emit
  artifact, package inputs, external commands, final artifact, runtime/host,
  and supported run mode.
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

- Route `faber run` through the final artifact for native executables,
  portable loaders, external Wasm/Racket hosts, and GPU host/provider paths.
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
cargo run --manifest-path faber/Cargo.toml -- run --target rust <package>
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
- Stop if a target's run path falls back silently to Rust, FMIR, or interpretation
  without an explicit product contract.
- Stop if the build matrix becomes a parity campaign for every language feature;
  split backend semantic burn-down into target-specific goals.
