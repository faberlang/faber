# Campaign: Aer Purgatus — Code-Smell Remediation

**Status**: proposed — ready for delivery; Goal 1 selected
**Date**: 2026-07-09
**Mode**: routing artifact — does not implement code directly
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Working repos**: `faber`, `radix`, `faber-runtime`, `norma`, `examples`

## Summary

Aer Purgatus coordinates three bounded corrections for proof-grade code that has
started carrying product-grade responsibility:

1. replace textual Faber binding inspection with compiler-grounded contract
   verification;
2. replace duplicated application-local JSON scanners and emitters with one
   canonical FVI codec over Norma JSON;
3. replace synchronous, platform-coupled async `sermo` scaffolding with an
   honest host and executor boundary.

Each goal is a separate campaign stage and lowers to its own delivery spec and
factory run. The campaign does not authorize opportunistic cleanup outside
those three seams.

## Problem

Recent implementation work is well tested at its happy paths, but three seams
encode language or runtime policy through local approximations:

| Goal | Smell | Product risk |
| --- | --- | --- |
| 1 — Binding contracts | `verify-library` scans source lines and checks non-empty symbol strings. | False acceptance and rejection; nested methods and layout become accidental API facts. |
| 2 — FVI codec | Five AI workbench commands copy JSON emission; index/query copy partial JSON scanners. | Parser drift, incomplete RFC handling, repeated fixes, avoidable rescans. |
| 3 — Async `sermo` | Async materializers sit over synchronous host dispatch, blocking fallbacks, and a default private macOS host dependency. | Executor blocking, latent hangs, platform coupling, sync/async drift. |

The shared smell is a local representation pretending to be the authority. The
remedy is not another wrapper: each goal moves enforcement to the layer that
owns the contract.

## Desired End State

- Faber declaration identity, scope, body presence, and signature come from the
  supported Radix frontend, never line scanning.
- Binding verification proves both sides of the contract: the Faber declaration
  and the selected backend symbol/signature.
- All package-relative manifest paths are contained by the package root.
- AI workbench JSON is parsed and emitted by one shared codec using
  `norma:json`; FVI schema checks are separate from JSON syntax handling.
- Async `ad` waits without blocking an executor thread, unknown routes terminate
  with structured errors, and sync/async collectors share one policy core.
- Generated packages do not acquire a private `radix/hosts/macos-arm64` path
  dependency merely because source contains `ad`.
- The three old approximation paths are deleted. No compatibility fallback keeps
  them alive.

## Development Posture

- **Clean break.** Internal callers move to the canonical seam in the same goal;
  no textual-parser fallback, duplicate JSON helper, or legacy host hook remains.
- **Truthful boundaries.** Tests prove the architectural invariant, not only the
  current fixture shape.
- **Behavior changes are explicit.** Legal JSON becomes more broadly accepted;
  invalid or escaping package paths become rejected; async unsupported routes
  become errors rather than indefinite waits.
- **Cross-repo changes commit in their owning repositories.** The factory parent
  validates every repo diff and reports each commit.
- **No attribution archaeology.** Git does not identify which model authored a
  hunk, and campaign decisions do not depend on guessing.

## Implementation Workflow

For each goal:

1. Run `delivery` over the entire goal and save its delivery spec before code
   changes.
2. Run one `factory` phase for the coherent goal by default.
3. Validate in the owning repo first, then run the named cross-repo gates.
4. Review against the goal invariant and delete the displaced implementation.
5. Commit intentional changes in each repository and record hashes here.
6. Return to this campaign and select the next incomplete goal.

Goal 3 must also reconcile the existing Radix
[`async-ad-lowering`](../../../../radix/docs/factory/async-ad-lowering/goal.md)
goal and ledger; this campaign must not create a competing async design.

## Scope Routing

| Surface | Owner | Campaign goal |
| --- | --- | --- |
| `src/package/binding.rs`, manifest path validation, `verify-library` tests | `faber` | Goal 1 |
| Frontend API needed to expose declaration contracts | `radix` | Goal 1, only if the supported API is insufficient |
| `ai-workbench/packages/faber-ai/src/commands/*.fab`, shared package modules, harness fixtures | `examples` | Goal 2 |
| `norma:json` / `norma:valor` defects exposed by the migration | `norma` | Goal 2, only for real general-purpose gaps |
| `src/frame.rs` and runtime tests | `faber-runtime` | Goal 3 |
| `ad` codegen, host trait/adapter, async lowering goal and ledger | `radix` | Goal 3 |
| Generated package dependency construction and package E2E tests | `faber` | Goal 3 |

Out of campaign:

- general package manifest Phase 3 build-graph work unrelated to verification;
- a new JSON library or JSON syntax in the compiler;
- HTTP server multiplexing, daemon transport, or broad host-provider expansion;
- unrelated `frame.rs` cleanup or AI workbench product features;
- compatibility shims for the displaced approximation paths.

## Batching And Split Policy

| Goal | Posture | Permitted split boundary |
| --- | --- | --- |
| 1 — Binding contracts | discovery-first, then batch | Split only if Radix needs a new stable frontend query API before Faber can implement verification. |
| 2 — FVI codec | batch-by-default | Split only if a demonstrated Norma JSON defect must land independently before the application migration. |
| 3 — Async `sermo` | split-on-boundary | Runtime wait/collector core may land before compiler/package integration; do not split sync/async policy or leave the private host dependency as an accepted final state. |

## Ground Truth Researched

| Source | Evidence / authority |
| --- | --- |
| `radix/EBNF.md` and `examples/corpus/` | Faber declaration and `ad` language contract |
| `faber/src/package/binding.rs` | Current line scanner, binding keys, shim-path checks |
| `faber/src/package_test.rs` | Current verification fixtures and missing negative coverage |
| `faber/docs/factory/unified-package-manifest/goal.md` | Original target-binding intent; Phase 4 completion claim is historical evidence, not proof of correctness |
| `norma/src/json/{solve,pange}.fab` | Current native RFC-oriented JSON authority |
| `examples/ai-workbench/packages/faber-ai/src/commands/{chat,embed,generate,index,query}.fab` | Duplicated emitters and partial scanners |
| `faber-runtime/src/frame.rs` | Sync/async receivers, materializers, blocking fallback dispatch |
| `radix/hosts/macos-arm64/src/kernel/host.rs` | Current synchronous host attachment |
| `radix/crates/radix/src/codegen/frame_shim.rs` | Generated concrete host hook |
| `faber/src/package/cargo.rs` | Generated default macOS host path dependency |
| `radix/docs/factory/async-ad-lowering/{goal.md,ledger.md}` | Existing async design authority and open phase state |

All six active repositories were clean when this campaign was drafted.

## Current State

| Goal | State | Next action |
| --- | --- | --- |
| 1 — Compiler-grounded binding contracts | selected | Lower the full goal through `delivery`; establish the frontend contract representation and backend proof mechanism. |
| 2 — Canonical FVI JSON codec | planned | Lower after Goal 1, or independently if it touches only `examples`/`norma`. |
| 3 — Honest async `sermo` boundary | planned; overlaps active Radix goal | Reconcile live async ledger during delivery, then execute remaining work under one design authority. |

## Campaign Path

### Goal 1 — Compiler-Grounded Binding Contract Verification

| Field | Value |
| --- | --- |
| **Status** | selected — ready for delivery |
| **Source** | `faber/src/package/binding.rs`; unified-package-manifest Phase 4 |
| **Invariant** | Binding verification derives declarations and signatures from the supported compiler frontend, and every referenced file remains inside the package root. |
| **Why first** | It has an immediate false-verification risk and a bounded package-tool surface. |
| **Lowers to** | `delivery` → saved spec under `faber/docs/factory/aer-purgatus/`, then `factory` |
| **Batch posture** | discovery-first, then batch |

Required implementation outcome:

- Delete `collect_file_declarations` and every source-line heuristic.
- Parse/analyze every library interface through Radix and walk only eligible
  top-level declarations. Preserve module identity, source span, generic/type
  signature, failable exit, and bodyless/body-backed status.
- Define one typed binding-contract representation used by manifest validation,
  diagnostics, and backend proof generation.
- Reject duplicate contract keys and bindings for ineligible nested methods.
- Validate `paths.source`, target binding manifests, and shim paths with a shared
  package-contained path resolver. Reject absolute and parent-escaping paths,
  including symlink escapes where the target exists.
- Prove backend symbols and signatures. For Rust, generate or reuse a minimal
  adapter/probe crate whose typed references fail `cargo check` when a symbol is
  absent or incompatible. A non-empty string is not verification.
- Emit structured diagnostics anchored to the Faber declaration or manifest
  entry that broke the contract.

Gate:

- Focused tests cover next-line bodies, multiline signatures, annotations,
  generics, nested methods, duplicate keys, unknown bindings, missing bindings,
  missing Rust symbols, signature mismatch, `..`, absolute paths, and symlinks.
- Existing SQLite Stage 1 fixture verifies without weakening its API contract.
- `timeout 120 cargo test --lib binding`
- `timeout 120 cargo test --lib manifest`
- Delivery-selected Rust probe integration test with an explicit timeout.
- `cargo fmt --all -- --check` and `git diff --check` in every touched Rust repo.

### Goal 2 — One Canonical FVI JSON Codec

| Field | Value |
| --- | --- |
| **Status** | planned |
| **Source** | AI workbench command files; `norma:json`; `norma:valor` |
| **Invariant** | AI workbench code owns FVI schema policy, not a second JSON grammar or five serializers. |
| **Why now** | Every new command or escaping fix currently multiplies maintenance work. |
| **Lowers to** | `delivery` in `examples/docs/factory/`, then `factory` |
| **Batch posture** | batch-by-default |

Required implementation outcome:

- Add one shared `faber-ai` codec module for FVI documents and command-result
  JSON. Commands import it; they do not define local `json_escape`, scanners, or
  object-string assembly.
- Parse with `norma:json.solve` and emit with `norma:json.pange`. Use
  `norma:valor` or typed conversion for field extraction.
- Keep JSON syntax validation separate from FVI schema/version validation.
- Parse each input document once. Validate required fields, exact numeric
  expectations, vector dimensions, normalization, and record structure from the
  parsed value tree.
- Preserve deterministic compact output and stable command schemas.
- Delete all displaced JSON helper copies from chat, embed, generate, index, and
  query.

Gate:

- Existing chat/embed/generate/index/query harnesses remain green.
- New fixtures cover legal whitespace, reordered fields, all JSON escapes,
  Unicode escapes, control-character emission, nested delimiters, malformed
  syntax, duplicate/unknown schema fields according to the selected FVI policy,
  and dimension/type errors.
- Python `json` readback validates every JSON stdout and written artifact.
- `cargo run --manifest-path ../faber/Cargo.toml -- check ai-workbench/packages/faber-ai`
- Run each affected `ai-workbench/harness/check-*.py` with an explicit timeout.
- `git diff --check` in `examples` and `norma` if touched.

### Goal 3 — Honest Async `Sermo` And Host Boundary

| Field | Value |
| --- | --- |
| **Status** | planned — existing async goal active |
| **Source** | Radix async-ad-lowering goal/ledger plus live runtime and package code |
| **Invariant** | Async `ad` never performs blocking host work while polling, always has a completion producer, and depends only on a portable runtime host contract. |
| **Why last** | Highest cross-repo and behavioral risk; benefits from the campaign's first two established delivery patterns. |
| **Lowers to** | reconcile existing goal → `delivery` for the remaining coherent slice → `factory` |
| **Batch posture** | split-on-boundary |

Required implementation outcome:

- Reconcile the existing async goal and ledger with the extracted
  `faber-runtime` repo and the host bridge that landed on 2026-07-09.
- Define a portable host-dispatch contract owned by the runtime boundary.
  Generated code calls that contract rather than constructing
  `faber_host_macos_arm64::HostKernel`.
- Select/install platform hosts through package/build configuration without a
  default private Radix path dependency and without emitted-code string sniffing.
- Ensure async receive has a real producer or returns a structured no-route or
  detached error. No unsupported route may remain pending forever.
- Move timer, filesystem, and process work out of `Future::poll`; use genuinely
  non-blocking facilities or an explicit bounded blocking-worker boundary.
- Replace copy-paired sync/async collectors with one collector state machine or
  policy core driven by sync and async receivers. Preserve the materialization
  table and failable-conversio behavior.
- Wire `HirFunction.is_async` through `ad` materialization so `@ futura` selects
  yielding behavior; keep source route strings identical for sync/async pairs.
- Specify cancellation/drop behavior and test it. Do not silently leak a waiter.

Gate:

- Runtime tests prove pending → wake → frame, no-route completion, cancellation,
  terminal errors, multiple-frame policy, and sync/async collector parity.
- An executor responsiveness test proves timer/process/file work does not block
  an unrelated ready task.
- Generated-package tests prove `@ futura` emits/executes async materialization
  and ordinary `ad` remains sync.
- A package using `ad` builds without a sibling private Radix checkout and does
  not contain the macOS host dependency unless that platform host was selected.
- `timeout 120 cargo test --manifest-path ../faber-runtime/Cargo.toml`
- `timeout 180 cargo test -p radix ad`
- `timeout 180 cargo test --manifest-path ../faber/Cargo.toml package_host`
- Delivery-selected package E2E route tests with explicit timeouts.
- `cargo fmt --all -- --check` and `git diff --check` in every touched Rust repo.

## Dependency Rules

```text
Goal 1 ─────────────── independent, selected first
Goal 2 ─────────────── independent; may run after Goal 1
Goal 3 ─────────────── must reconcile async-ad-lowering authority first
Campaign closeout ──── requires all three goals and cross-repo validation
```

- A delivery may not mix files from two goals merely because both are cleanup.
- If Goal 1 requires a Radix frontend API, expose semantic facts rather than raw
  parser internals or a Faber-specific text helper.
- If Goal 2 exposes a real Norma JSON defect, fix the general JSON contract and
  prove it in Norma before resuming the FVI migration.
- Goal 3 must preserve one route per capability and the existing `sermo ↦ T`
  materialization policy.
- Any unexpected dirty path in a touched repo stops that goal before edits.
- Lock-producing Git commands run serially within each repository.

## First Useful Milestones

1. `verify-library` rejects a deliberately wrong Rust signature and accepts a
   multiline, annotated Faber interface without textual heuristics.
2. All five AI workbench commands have zero local JSON grammar/emitter helpers.
3. An `@ futura` package route yields while another task runs and builds without
   a private Radix host path.

## Acceptance Criteria

Campaign routing is ready when:

- [x] Exactly three goals map to the three observed smells.
- [x] Each goal has an invariant, owning repos, implementation outcome, gate,
      batching posture, and downstream lowering route.
- [x] Goal 1 is selected as the next campaign stage.
- [x] Overlap with unified-package-manifest and async-ad-lowering is explicit.
- [x] Clean-break and stop conditions are recorded.

Campaign implementation is complete when:

- [ ] All three goals have executed delivery specs and factory evidence.
- [ ] The line-based binding scanner, copied JSON helpers, synchronous generated
      host hook, and default private macOS host dependency are absent.
- [ ] All stage gates and cross-repo closeout gates pass.
- [ ] Existing overlapping goals/ledgers contain no stale completion claims.
- [ ] Every touched repository is clean and all commits are recorded here.
- [ ] A release/version decision is recorded. Default is explicit deferral until
      the next normal Faber release unless a goal changes a published contract.

## Validation

Artifact checks:

- Links resolve to live sibling artifacts.
- Exactly one stage is selected.
- Each stage lowers through `delivery` before `factory`.
- `faber/docs/factory/README.md` indexes this campaign.

Closeout validation is the union of the three goal gates plus:

- `timeout 240 cargo test --manifest-path faber/Cargo.toml`
- `timeout 240 cargo test --manifest-path faber-runtime/Cargo.toml`
- `timeout 240 cargo test --manifest-path radix/Cargo.toml -p radix`
- affected AI workbench harnesses with explicit timeouts;
- standard format/lint gates in every touched repository.

## Open Questions

These are delivery-level decisions, not blockers to campaign routing:

- Which existing Radix API is the narrowest stable source for top-level
  declaration contracts?
- Should Rust binding proof use generated typed function pointers, generated
  adapters, or the normal backend library crate build? It must prove existence
  and compatibility, not merely parse paths.
- Does the runtime host contract need cross-thread `Send + Sync` in the first
  delivery, or can a documented single-thread executor contract satisfy every
  current package host? Delivery must decide from actual producer topology.

## Stop Conditions

Pause and return to the user rather than improvising when:

- a proposed fix requires changing Faber grammar or the public `sermo ↦ T`
  materialization contract;
- Rust ABI mapping for a Faber type is undefined and cannot be derived from live
  codegen;
- removing the private host dependency requires a publication, deployment,
  credential, or external registry action;
- a Norma JSON limitation would require a new language feature rather than a
  library correction;
- a stage encounters foreign uncommitted changes in its allowed paths;
- a validation failure reveals policy debt outside the selected goal.
