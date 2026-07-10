# Campaign: Aer Purgatus — Code-Smell Remediation

**Status**: active — Goal 1 complete; Goal 2 selected for factory
**Date**: 2026-07-09
**Mode**: routing artifact — does not implement code directly
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Working repos**: `faber`, `radix`, `faber-runtime`, `norma`, `examples`

## Summary

Aer Purgatus coordinates four bounded corrections for proof-grade code that has
started carrying product-grade responsibility:

1. replace textual Faber binding inspection with compiler-grounded contract
   verification;
2. introduce one formal, object-rooted JSON document type that preserves the
   compiler's JSON-safety proof;
3. migrate manual JSON construction and parsing to the formal type, with FVI as
   the first substantial application proof;
4. replace synchronous, platform-coupled async `sermo` scaffolding with an
   honest host and executor boundary.

Each goal is a separate campaign stage and lowers to its own delivery spec and
factory run. The campaign does not authorize opportunistic cleanup outside
those four seams.

## Problem

Recent implementation work is well tested at its happy paths, but four seams
encode language or runtime policy through local approximations:

| Goal | Smell | Product risk |
| --- | --- | --- |
| 1 — Binding contracts | `verify-library` scans source lines and checks non-empty symbol strings. | False acceptance and rejection; nested methods and layout become accidental API facts. |
| 2 — Formal JSON document | Inline JSON, `@ json genus`, `valor`, and Norma codecs prove overlapping facts but erase them into unconstrained `valor` through separate lowering paths. | JSON safety is lost at conversion boundaries; root-object policy and renamed fields are inconsistent. |
| 3 — JSON migration | Five AI workbench commands copy JSON emission; index/query copy partial JSON scanners; other first-party Faber source still assembles JSON text manually. | Parser drift, incomplete RFC handling, repeated fixes, avoidable rescans. |
| 4 — Async `sermo` | Async materializers sit over synchronous host dispatch, blocking fallbacks, and a default private macOS host dependency. | Executor blocking, latent hangs, platform coupling, sync/async drift. |

The shared smell is a local representation pretending to be the authority. The
remedy is not another wrapper: each goal moves enforcement to the layer that
owns the contract.

## Desired End State

- Faber declaration identity, scope, body presence, and signature come from the
  supported Radix frontend, never line scanning.
- Binding verification proves both sides of the contract: the Faber declaration
  and the selected backend symbol/signature.
- All package-relative manifest paths are contained by the package root.
- Faber has one formal JSON document type whose root is an object and whose
  nested values are recursively JSON-safe. It is distinct from unrestricted
  `valor`, even if its runtime representation reuses `Valor::Tabula`.
- Inline JSON objects, `@ json genus` conversion, and `norma:json` parsing and
  emission converge on that formal type and one lowering contract.
- First-party Faber source no longer hand-builds or partially scans JSON where
  the formal type applies. FVI schema checks stay separate from JSON syntax.
- Async `ad` waits without blocking an executor thread, unknown routes terminate
  with structured errors, and sync/async collectors share one policy core.
- Generated packages do not acquire a private `radix/hosts/macos-arm64` path
  dependency merely because source contains `ad`.
- The old approximation paths are deleted. No compatibility fallback keeps
  them alive.

## Development Posture

- **Clean break.** Internal callers move to the canonical seam in the same goal;
  no textual-parser fallback, duplicate JSON helper, or legacy host hook remains.
- **Truthful boundaries.** Tests prove the architectural invariant, not only the
  current fixture shape.
- **Behavior changes are explicit.** JSON follows one object-rooted document
  contract; invalid or escaping package paths become rejected; async unsupported
  routes become errors rather than indefinite waits.
- **Cross-repo changes commit in their owning repositories.** The factory parent
  validates every repo diff and reports each commit.
- **No attribution archaeology.** Git does not identify which model authored a
  hunk, and campaign decisions do not depend on guessing.

## Implementation Workflow

For each goal:

1. Read the saved delivery spec for the selected goal and verify its live-code
   assumptions before code changes.
2. Run one `factory` phase for the coherent goal by default.
3. Validate in the owning repo first, then run the named cross-repo gates.
4. Review against the goal invariant and delete the displaced implementation.
5. Commit intentional changes in each repository and record hashes here.
6. Return to this campaign and select the next incomplete goal.

Goal 4 must also reconcile the existing Radix
[`async-ad-lowering`](../../../../radix/docs/factory/async-ad-lowering/goal.md)
goal and ledger; this campaign must not create a competing async design.

## Scope Routing

| Surface | Owner | Campaign goal |
| --- | --- | --- |
| `src/package/binding.rs`, manifest path validation, `verify-library` tests | `faber` | Goal 1 |
| Frontend API needed to expose declaration contracts | `radix` | Goal 1, only if the supported API is insufficient |
| JSON primitive/type tables, inline literal typing, conversio, `@ json genus`, codegen, EBNF, corpus harness | `radix` | Goal 2 |
| Formal JSON runtime representation and checked `valor` bridge | `faber-runtime` | Goal 2 |
| `norma:json` parse/emit signatures and implementation | `norma` | Goal 2 |
| First-party Faber JSON construction/parsing, AI workbench commands, FVI codec, harness fixtures | `examples`, `norma`, and other owning sibling repos | Goal 3 |
| Migration audits and compiler/application exempla | owning repo; `radix` owns corpus harness integration | Goal 3 |
| `src/frame.rs`, runtime dispatch/collector protocol, and runtime tests | `faber-runtime` | Goal 4 |
| Public bounded-worker native host adapter | `faber-runtime/hosts/native` | Goal 4 |
| `ad` codegen, route-requirement analysis, async lowering goal and ledger | `radix` | Goal 4 |
| Generated package dependency construction and package E2E tests | `faber` | Goal 4 |

Out of campaign:

- general package manifest Phase 3 build-graph work unrelated to verification;
- top-level JSON array documents or a public arbitrary-root JSON-node type;
- broad custom JSON encoders, serde-style hooks, or null-omission policy;
- HTTP server multiplexing, daemon transport, or broad host-provider expansion;
- unrelated `frame.rs` cleanup or AI workbench product features;
- compatibility shims for the displaced approximation paths.

## Batching And Split Policy

| Goal | Posture | Permitted split boundary |
| --- | --- | --- |
| 1 — Binding contracts | discovery-first, then batch | Split only if Radix needs a new stable frontend query API before Faber can implement verification. |
| 2 — Formal JSON document | split-on-boundary | Runtime/type foundation may land before compiler and Norma integration; no split may leave inline JSON or `@ json genus` permanently typed as unconstrained `valor`. |
| 3 — JSON migration | discovery-first, then batch | Inventory/classification may precede migration; after the pattern is proven, batch all homogeneous first-party Faber call sites. |
| 4 — Async `sermo` | split-on-boundary | Runtime wait/collector core may land before compiler/package integration; do not split sync/async policy or leave the private host dependency as an accepted final state. |

## Ground Truth Researched

| Source | Evidence / authority |
| --- | --- |
| `radix/EBNF.md` and `examples/corpus/` | Faber declaration and `ad` language contract |
| `radix/crates/radix/src/{driver.rs,file_interface.rs}` and `faber/src/package/file_interface.rs` | Supported analyzed-unit/interface seam already exposes portable callable signatures |
| `faber/src/package/binding.rs` | Current line scanner, binding keys, shim-path checks |
| `faber/src/package_test.rs` | Current verification fixtures and missing negative coverage |
| `faber/docs/factory/unified-package-manifest/goal.md` | Original target-binding intent; Phase 4 completion claim is historical evidence, not proof of correctness |
| `norma/src/json/{solve,pange}.fab` | Current native RFC-oriented JSON authority |
| `radix/docs/factory/inline-json-valor/{contract.md,goal.md}` | Object-rooted source-literal rule and the historical decision to type literals as `valor` |
| `radix/docs/factory/json-genus-contract/goal.md` | Completed JSON-safe genus validation and one-way `nomen` boxing behavior |
| `faber-runtime/src/valor.rs` | Dynamic carrier is broader than JSON (`Octeti`, tagged `Instans`, unrestricted root shape) |
| `examples/ai-workbench/packages/faber-ai/src/commands/{chat,embed,generate,index,model,query}.fab` and `examples/vivilite/src/main.fab` | Live first-party manual emitters/scanners; campaign intake omitted two active paths |
| `faber-runtime/src/frame.rs` | Sync/async receivers, materializers, blocking fallback dispatch |
| `radix/hosts/macos-arm64/src/kernel/host.rs` | Current synchronous host attachment |
| `radix/crates/radix/src/codegen/frame_shim.rs` | Generated concrete host hook |
| `faber/src/package/cargo.rs` | Generated default macOS host path dependency |
| `faber/src/package/manifest.rs` | Rust target metadata has bindings/dependencies but no explicit host selection |
| `radix/docs/factory/async-ad-lowering/{goal.md,ledger.md}` | Existing async design authority and open phase state |

All six active repositories were clean when this campaign was drafted.

## Current State

| Goal | State | Next action |
| --- | --- | --- |
| 1 — Compiler-grounded binding contracts | **complete** | Factory gate passed; evidence and commits recorded below. |
| 2 — Formal object-rooted JSON document | **selected for factory** | Execute [`goal-2-json-document-delivery.md`](goal-2-json-document-delivery.md). |
| 3 — First-party JSON migration and FVI adoption | delivery ready; implementation blocked on Goal 2 | Execute [`goal-3-json-migration-delivery.md`](goal-3-json-migration-delivery.md) after Goal 2 J6. |
| 4 — Honest async `sermo` boundary | delivery ready; authority reconciliation first | Execute [`goal-4-async-sermo-delivery.md`](goal-4-async-sermo-delivery.md) after reconciling the live Radix ledger. |

The four saved delivery documents are the factory production inputs. Their
status is planning evidence only; no implementation stage is marked complete.

## Campaign Path

### Goal 1 — Compiler-Grounded Binding Contract Verification

| Field | Value |
| --- | --- |
| **Status** | complete — factory gate passed 2026-07-09 |
| **Source** | `faber/src/package/binding.rs`; unified-package-manifest Phase 4 |
| **Invariant** | Binding verification derives declarations and signatures from the supported compiler frontend, and every referenced file remains inside the package root. |
| **Why first** | It has an immediate false-verification risk and a bounded package-tool surface. |
| **Lowers to** | [`goal-1-binding-contracts-delivery.md`](goal-1-binding-contracts-delivery.md) → `factory` |
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

Factory evidence:

- Faber `063dd1f` (`polish(binding): prove compiler-grounded library contracts`)
  replaces the line scanner with analyzed package contracts, contained path
  resolution, bounded Cargo probes, structured diagnostics, and the full
  positive/negative fixture matrix.
- Radix `70b60cbeb` (`polish(parser): scope bodyless functions to library
  analysis`) restores the optional-body grammar only for explicit library
  analysis; ordinary compilation remains fail-closed.
- Radix `0392894d7` (`polish(codegen): render exact Rust binding probes`) exposes
  the narrow renderer backed by normal Rust signature policy.
- Gates passed: Faber binding 20/20, manifest 41/41, package 209/209, hygiene,
  clippy, format, and diff checks; Radix file-interface 10/10, binding-related
  42/42, probe 2/2, default bodyless rejection, hygiene, repo-native lint,
  format, and diff checks.
- Correctness, read-only review/bonsai, cleanliness, housekeeping, and per-file
  polish completed with no deferred findings. Release decision: `defer-release`
  until the next normal Faber release.

### Goal 2 — Formal Object-Rooted JSON Document Type

| Field | Value |
| --- | --- |
| **Status** | selected — ready for factory |
| **Source** | inline-json-valor contract; JSON-genus contract; EBNF; runtime `Valor`; `norma:json` |
| **Invariant** | A Faber JSON document is object-rooted and recursively JSON-safe. It may widen to `valor`, but arbitrary `valor` enters it only through checked conversion. |
| **Why now** | Inline literals and `@ json genus` already prove JSON safety, but separate paths erase that proof into a broader carrier. |
| **Lowers to** | [`goal-2-json-document-delivery.md`](goal-2-json-document-delivery.md) → cross-repo `factory` |
| **Batch posture** | split-on-boundary |

Required implementation outcome:

- Use canonical public spelling `json` and runtime spelling `faber::Json`. The
  type is a JSON **document**, not an arbitrary-root JSON node.
- Add a runtime representation that enforces a `Tabula` root and recursively
  permits only null, booleans, finite numbers, text, arrays, and objects. Prefer
  a validated wrapper over the existing carrier unless delivery evidence proves
  a separate recursive enum is cleaner.
- Keep `valor` as the unrestricted dynamic/frame carrier. Provide explicit
  infallible JSON-to-`valor` widening and failable `valor`-to-JSON narrowing.
- Type bare inline `{ "key": ... }` literals as the formal JSON document instead
  of unconstrained `valor`. Preserve the source rule that top-level arrays are
  Faber `lista`, while arrays remain legal inside JSON objects.
- Make `@ json genus` convert bidirectionally with the formal JSON type. Apply
  `@ json { nomen = "wire_name" }` on both boxing and extraction paths, including
  nested genera and collections.
- Route inline literals and JSON-genus conversion through one semantic/lowering
  contract instead of parallel JSON-object builders.
- Change `norma:json.solve`/`tempta` to produce the formal JSON type and reject
  scalar or array roots. Change `json.pange` to accept the formal type.
- Keep syntax parsing, document-root validation, schema/genus conversion, and
  wire rendering as separate layers with structured failures.
- Add a canonical corpus exemplum proving inline construction, genus round-trip,
  renamed fields, nested arrays/objects, text parse/emit, and checked `valor`
  narrowing.
- Update EBNF, reader/type vocabulary, target emission, MIR/stepper behavior,
  and user documentation as required by the chosen type.
- Update the direct canonical callers needed to keep compiler, runtime, Norma,
  and corpus gates green. Goal 3 owns the broader first-party cleanup inventory.
- Record a release/version decision at Goal 2 closeout because changing inline
  literal typing and `norma:json` signatures is a public clean break.

Gate:

- Type tests distinguish formal JSON from `valor` and reject implicit narrowing.
- Inline object literals infer the formal type; top-level array/scalar JSON
  documents are rejected by both source and Norma parsing boundaries.
- `@ json genus → JSON → @ json genus` round-trips every supported field family
  and uses `nomen` symmetrically.
- JSON-to-`valor` widening preserves the tree; invalid-root, `Octeti`, tagged
  `Instans`, and non-finite `valor` narrowing fail structurally.
- Rust application output and MIR stepper behavior agree for the canonical
  exemplum; other emit targets fail closed until explicitly supported.
- `timeout 300 cargo test -p radix json -- --format terse`
- `timeout 180 cargo test --manifest-path ../faber-runtime/Cargo.toml json`
- `timeout 180 ./scripta/check-reader-pack-completeness`
- `timeout 180 ../norma/scripta/check-source`
- `cargo fmt --all -- --check` and `git diff --check` in every touched repo.

### Goal 3 — First-Party JSON Migration And FVI Adoption

| Field | Value |
| --- | --- |
| **Status** | delivery ready — implementation blocked on Goal 2 |
| **Source** | post-Goal-2 migration inventory; AI workbench command files and harnesses |
| **Invariant** | First-party Faber code uses the formal JSON document and typed genera instead of maintaining local JSON grammars or assembling JSON text. |
| **Why after Goal 2** | Migration is the product proof that the new type pays rent; doing it earlier would target an obsolete `valor` API. |
| **Lowers to** | [`goal-3-json-migration-delivery.md`](goal-3-json-migration-delivery.md) → `factory` after Goal 2 closeout |
| **Batch posture** | discovery-first, then batch |

Required implementation outcome:

- Inventory active first-party Faber source for manual JSON string assembly,
  copied escaping, partial scanners, repeated `valor` field walking, and schema
  models that should be `@ json genus`. Record each site as migrate, retain with
  rationale, or out of scope.
- Establish one migration pattern: typed `@ json genus` models for stable
  schemas, the formal JSON document for ad hoc object-rooted payloads, and
  `norma:json` only at wire boundaries.
- Batch-migrate homogeneous sites after that pattern passes. Delete displaced
  `json_quote`, `json_escape`, delimiter scanners, field substring searches, and
  object concatenation rather than wrapping them.
- Add one shared FVI codec/schema module in `faber-ai`. Parse each document once
  into the formal JSON type, convert to typed FVI genera, and keep FVI
  schema/version validation separate from JSON syntax.
- Migrate chat, embed, generate, index, query, the additionally discovered
  `model` command, and `vivilite` to typed JSON documents; preserve
  deterministic compact output and stable command schemas.
- Extend the audit beyond AI workbench to other active Norma/examples Faber
  sources. Do not rewrite unrelated Rust tooling JSON merely because it emits
  JSON; Rust code should use its own typed serializer authority.
- Add a lightweight regression audit that detects reintroduced application-local
  JSON grammars without banning legitimate string or protocol work broadly.

Gate:

- Migration ledger has no unexplained first-party Faber manual-JSON site in the
  selected scope.
- Existing chat/embed/generate/index/query harnesses remain green.
- New fixtures cover legal whitespace, reordered fields, every JSON escape,
  Unicode, control characters, nested delimiters, malformed syntax, schema
  errors, and vector dimension/type errors.
- Python `json` readback validates every JSON stdout and written artifact.
- `cargo run --manifest-path ../faber/Cargo.toml -- check ai-workbench/packages/faber-ai`
- Run each affected `ai-workbench/harness/check-*.py` with an explicit timeout.
- Run the migration regression audit and `../norma/scripta/check-source` when
  Norma changes.
- `git diff --check` in every touched repo.

### Goal 4 — Honest Async `Sermo` And Host Boundary

| Field | Value |
| --- | --- |
| **Status** | delivery ready — existing async authority must be reconciled |
| **Source** | Radix async-ad-lowering goal/ledger plus live runtime and package code |
| **Invariant** | Async `ad` never performs blocking host work while polling, always has a completion producer, and depends only on a portable runtime host contract. |
| **Why last** | Highest cross-repo and behavioral risk; benefits from the campaign's first three established delivery patterns. |
| **Lowers to** | reconcile existing goal → [`goal-4-async-sermo-delivery.md`](goal-4-async-sermo-delivery.md) → `factory` |
| **Batch posture** | split-on-boundary |

Required implementation outcome:

- Reconcile the existing async goal and ledger with the extracted
  `faber-runtime` repo and the host bridge that landed on 2026-07-09.
- Replace the cross-thread `Rc<RefCell<_>>` conversation core with an explicit
  thread-safe state machine and define a `Send + Sync` portable host-dispatch
  contract owned by the runtime boundary.
  Generated code calls that contract rather than constructing
  `faber_host_macos_arm64::HostKernel`.
- Select the public native application host explicitly with
  `[target.rust] host = "native"`; an absent host permits runtime-only routes.
  Build dependencies from analyzed route/executor requirements without a
  default private Radix path or emitted-code string sniffing.
- Ensure async receive has a real producer or returns a structured no-route or
  detached error. No unsupported route may remain pending forever.
- Move timer, filesystem, and process work out of `Future::poll` into a public
  native host adapter with an explicit bounded blocking-worker boundary.
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
Goal 2 ────────┬────── formal JSON document foundation
               └──► Goal 3 broad JSON migration and FVI adoption
Goal 4 ─────────────── must reconcile async-ad-lowering authority first
Campaign closeout ──── requires all four goals and cross-repo validation
```

- A delivery may not mix files from two goals merely because both are cleanup.
- If Goal 1 requires a Radix frontend API, expose semantic facts rather than raw
  parser internals or a Faber-specific text helper.
- Goal 2 owns the JSON type and codec contract. Goal 3 consumes it and must not
  add a second representation or application-local grammar.
- Goal 3 cannot begin implementation until Goal 2 closeout proves the public
  type, `@ json genus` round-trip, and Norma wire boundaries.
- Goal 4 must preserve one route per capability and the existing `sermo ↦ T`
  materialization policy.
- Any unexpected dirty path in a touched repo stops that goal before edits.
- Lock-producing Git commands run serially within each repository.

## First Useful Milestones

1. `verify-library` rejects a deliberately wrong Rust signature and accepts a
   multiline, annotated Faber interface without textual heuristics.
2. Inline JSON, `@ json genus`, and `norma:json` all produce or consume the same
   object-rooted formal JSON document type.
3. All selected first-party Faber sources, including the five AI workbench
   commands, have zero unexplained local JSON grammar/emitter helpers.
4. An `@ futura` package route yields while another task runs and builds without
   a private Radix host path.

## Acceptance Criteria

Campaign routing is ready when:

- [x] Four goals map to the observed smells, with formal JSON construction and
      its dependent cleanup migration kept as separate delivery units.
- [x] Each goal has an invariant, owning repos, implementation outcome, gate,
      batching posture, and downstream lowering route.
- [x] Goal 1 is selected as the next campaign stage.
- [x] Overlap with unified-package-manifest and async-ad-lowering is explicit.
- [x] Clean-break and stop conditions are recorded.
- [x] A detailed saved delivery document exists for each of the four goals.
- [x] Goal 2's public spelling, representation, number/root policy, conversion
      matrix, and Norma clean break are decided.
- [x] Goal 4's thread-safe producer, explicit host-selection, collector, async
      posture, cancellation, and same-route policies are decided.

Campaign implementation is complete when:

- [ ] All four goals have executed delivery specs and factory evidence.
- [ ] The line-based binding scanner, unconstrained inline-JSON `valor` typing,
      asymmetric JSON-genus extraction, copied JSON helpers, synchronous
      generated host hook, and default private macOS host dependency are absent.
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

Closeout validation is the union of the four goal gates plus:

- `timeout 240 cargo test --manifest-path faber/Cargo.toml`
- `timeout 240 cargo test --manifest-path faber-runtime/Cargo.toml`
- `timeout 240 cargo test --manifest-path radix/Cargo.toml -p radix`
- affected AI workbench harnesses with explicit timeouts;
- standard format/lint gates in every touched repository.

## Resolved Delivery Decisions

- Goal 1 reuses the analyzed `FileInterface`/`InterfaceCallable` seam and uses
  Radix-owned typed Rust adapters; Faber owns containment and the probe crate.
- Goal 2's public spelling is `json`, represented by private-field
  `faber::Json(Valor)`. It is object-rooted, recursively validated, numerically
  strict, and distinct from `valor`.
- `norma:json` delegates to formal conversions and deletes the incomplete
  parser/serializer and old broad signatures.
- Goal 3 centralizes FVI wire genera in the owning AI workbench package, parses
  once, separates schema/domain validation, and audits all active first-party
  Faber sources rather than only the original five.
- Goal 4 requires cross-thread runtime state and a `Send + Sync` nonblocking
  producer contract. Generated packages explicitly select a public native host;
  async/sync Norma wrappers use one capability route and one collector policy.

Factory-time questions that remain are implementation discoveries recorded in
the individual delivery specs. None changes these product or architecture
decisions.

## Stop Conditions

Pause and return to the user rather than improvising when:

- work outside Goal 2 requires a new Faber grammar surface, or any goal requires
  changing the public `sermo ↦ T` materialization contract;
- Rust ABI mapping for a Faber type is undefined and cannot be derived from live
  codegen;
- removing the private host dependency requires a publication, deployment,
  credential, or external registry action;
- a proposal weakens the object-root rule, silently aliases arbitrary `valor` to
  the formal JSON type, or preserves the old Norma signatures as a second
  canonical route;
- a stage encounters foreign uncommitted changes in its allowed paths;
- a validation failure reveals policy debt outside the selected goal.
