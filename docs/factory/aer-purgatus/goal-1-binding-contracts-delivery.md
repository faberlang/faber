# Delivery: Compiler-Grounded Binding Contracts

**Status**: ready for factory — selected Aer Purgatus unit
**Date**: 2026-07-09
**Campaign**: [`CAMPAIGN.md`](CAMPAIGN.md)
**Primary repo**: `/Users/ianzepp/work/faberlang/faber`
**Conditional repo**: `/Users/ianzepp/work/faberlang/radix`
**Factory checkpoint**: `verify-library` proves the Faber and Rust sides of one
typed contract without source-line heuristics.

## Interpreted Unit

Replace `faber/src/package/binding.rs`'s line scanner and non-empty-symbol check
with compiler-grounded package interface analysis, contained path resolution,
and a Rust compile probe. This is a verification correction, not the unfinished
generated-library build graph from unified-package-manifest Phase 3.

Explicit requirements:

- derive callable identity, eligibility, body presence, generics, ownership
  modes, return type, alternate exit, async/generator flags, and source span
  from the supported Radix frontend;
- use one typed declaration contract for manifest comparison, diagnostics, and
  target proof generation;
- reject duplicate contract keys and nested methods presented as top-level
  bindings;
- contain every manifest-selected source, binding, and shim path within the
  package root, including existing symlink targets;
- prove that each Rust symbol exists with a compatible signature by compiling
  generated typed references or adapters;
- delete `collect_file_declarations` and all textual fallbacks.

Inferred requirement, grounded in live code: reuse the existing
`AnalyzedUnit` + `FileInterface`/`InterfaceCallable` seam already used by
package compilation. A second signature model would recreate the defect at a
different layer.

## Normalized Spec

### Invariant

Binding verification accepts a row if and only if a package-contained,
top-level Faber callable with the same canonical key exists and the selected
backend can compile a symbol with the callable's exact target ABI.

### Contract model

Add a target-neutral callable declaration record at the Radix analysis
boundary. The exact Rust names may follow local conventions, but the data must
include:

| Field | Source of truth |
| --- | --- |
| declaration name and `DefId` | analyzed top-level HIR item |
| source span and file label | `HirItem.span` + analyzed file |
| portable signature | existing `InterfaceCallable` snapshot |
| body status | `HirFunction.body.is_some()` |
| attachment scope | top-level `HirItemKind::Function` only |
| module identity | package graph's source-relative file module |
| export eligibility | existing package visibility/export policy |

`provider:module.function` remains the manifest key for this delivery. A
compiler-local `DefId` must not cross files or enter a durable manifest.

### Rust proof contract

Radix owns rendering Faber callable contracts into Rust ABI probes because it
already owns type-to-Rust mapping. Faber owns the temporary crate, dependency
manifest, shim inclusion, `cargo check`, and diagnostic projection.

The probe should use compile-time typed references rather than invoke symbols:

- ordinary functions: typed function-item or adapter assignment;
- `de` / `in` / `ex`: the same reference/mutable-reference/move ABI emitted by
  normal Rust codegen;
- failable functions: `Result<Success, Error>` target shape;
- async functions: an adapter whose future output is constrained to the
  expected success/error shape;
- generics: a generated generic adapter carrying the same Radix-owned bounds,
  not a single concrete witness that could falsely prove a nongeneric symbol;
- body-backed declarations with an explicit row: prove the row too; bodyless
  declarations require a row.

The probe crate includes the declared shim by path/module and uses only
`[target.rust.dependencies]`. A parseable Rust path or a successful shim build
alone is not proof of the declared function contract.

### Path contract

Create one package-member resolver and use it for `paths.source`, target
binding manifests, and shim paths.

1. Reject empty, absolute, root/prefix, and lexically escaping `..` paths before
   joining.
2. Normalize `.` without allowing a preceding component to be popped past the
   package root.
3. Canonicalize the package root and an existing candidate; require the latter
   to remain under the former.
4. For a missing candidate, canonicalize its nearest existing ancestor so a
   symlinked parent cannot escape containment, then report the missing file or
   directory separately.
5. Return a resolved path plus the manifest/source anchor used for diagnostics.

Do not reuse `package/paths.rs::normalize_path` as the security check: it is a
graph-key normalizer and intentionally pops parent components.

### Non-goals

- Do not implement the Phase 3 generated-library dependency graph.
- Do not change the binding manifest key format or add compatibility aliases.
- Do not expose parser AST nodes from Radix.
- Do not prove dynamic loading, C ABI, or non-Rust targets in this unit.
- Do not accept unsupported ABI mappings by erasing them to `valor` or
  `ignotum`; stop with a structured unsupported-contract diagnostic.

## Repo-Aware Baseline

### Facts

- `binding.rs` scans `source.lines()`, recognizes only lines beginning with
  `functio `, and treats a `{` on the same line as body presence.
- `validate_bindings` checks only non-empty symbol text and key membership.
- `validate_shim` joins an unchecked manifest path to the package root.
- Radix exposes `driver::analyze_source` and a coherent `AnalyzedUnit` with HIR,
  interner, resolver, and type table.
- `radix::file_interface::InterfaceCallable` already snapshots generics,
  parameter modes/optionality, return/error types, and async/generator flags.
- `faber/src/package/file_interface.rs` already converts analyzed top-level HIR
  functions into that snapshot for package imports.
- Package compilation already analyzes imported files in dependency order and
  supplies typed file interfaces to importers.
- Existing binding tests cover one success, one missing row, and one unknown
  row; they do not prove layout, nested-scope, paths, or Rust ABI failures.

### Architectural decision

Prefer a small Radix declaration-contract query that reuses
`InterfaceCallable`, then refactor package interface extraction and binding
verification to share it. If the live package analyzer can expose its analyzed
file set without a new Radix API, keep the new query in Faber. Radix changes are
justified only for target-neutral declaration facts or Rust ABI rendering.

## Stage Graph

```text
B1 contract query ───────┐
                         ├─> B3 manifest reconciliation ─> B5 diagnostics/tests
B2 contained paths ─────┘                 │
                                          └─> B4 Rust probe ─┘
```

| Stage | Entry condition | Work and output | Exit gate |
| --- | --- | --- | --- |
| B1 — Analyze interfaces | clean Faber/Radix trees | Reuse package dependency order; produce canonical top-level callable contracts with signatures, body status, spans, and module identity. | Multiline, next-line-body, annotation, generic, and nested-method fixtures classify correctly. |
| B2 — Resolve package members | manifest loaded | Add shared containment resolver; route source, binding, and shim paths through it. | Absolute, lexical escape, existing symlink escape, missing-under-symlink, and valid nested paths are distinguished. |
| B3 — Reconcile manifest | B1 + B2 | Compare typed declaration and manifest maps; collect duplicate, unknown, and required-missing errors without first-error masking. | Deterministic result independent of filesystem iteration order. |
| B4 — Prove Rust | B3 | Render Radix-owned ABI adapters, build an isolated probe crate with declared dependencies and shim, and map compiler failures to the relevant row/declaration. | Missing symbol and every selected signature mismatch fail `cargo check`; valid SQLite fixture passes. |
| B5 — Close out | B4 | Delete scanner/fallback, update docs and focused fixtures, run gates. | No source-line declaration inspection remains. |

Retry/resume: B1 and B2 are independent. B4 must not begin until B3 produces a
stable contract set. Probe artifacts live under a temporary/test-owned
directory and are never committed.

## Implementation Work

### Workstream A — Radix/Faber contract extraction

Allowed primary surfaces:

- `faber/src/package/file_interface.rs`
- package analysis/cache modules needed to expose analyzed files
- conditional `radix/crates/radix/src/file_interface.rs` or a focused sibling
  module

Acceptance:

- one callable snapshot implementation serves imports and bindings;
- visibility and top-level eligibility follow package compilation behavior;
- diagnostics retain the source file and span;
- unsupported snapshot types fail explicitly.

### Workstream B — Faber manifest and path verification

Allowed primary surfaces:

- `faber/src/package/binding.rs`
- `faber/src/package/paths.rs` or a new focused containment module
- `faber/src/package/manifest.rs` only where path validation belongs globally

Acceptance:

- no direct unchecked `package_root.join(manifest_value)` remains on the three
  binding-verification paths;
- errors are structured by `issue` and carry path/key args;
- filesystem enumeration and diagnostic order are stable.

### Workstream C — Rust proof crate

Allowed primary surfaces:

- a focused Faber binding-probe module;
- a narrow public Radix Rust binding-probe renderer if normal codegen cannot be
  reused safely;
- package tests and SQLite fixture.

Acceptance:

- a deliberately wrong return, parameter mode, error channel, async output,
  and generic shape each fail for the expected row;
- no string comparison of emitted Rust substitutes for `cargo check`;
- child-process timeouts and captured diagnostics are deterministic.

Parallelism: A and B have disjoint write surfaces and may proceed together.
C begins after A fixes the contract representation. Lock-producing Cargo/Git
commands remain serial per repository.

## Checkpoints And Gates

### Batching / split decision

Discovery-first, then one batch. Split into a preliminary Radix phase only if
the existing analysis/file-interface API cannot represent body status and
portable signatures without exposing private compiler structures. That split
must land a target-neutral query, not a Faber-specific parser helper.

### Correctness gates

- duplicate callable keys are diagnosed before map insertion can overwrite;
- an annotated or multiline signature is never reconstructed from source text;
- a nested method cannot satisfy a top-level function binding;
- a body beginning on the next line is body-backed;
- an escaping path cannot be accepted because its final file is missing;
- Rust proof covers symbol existence and type compatibility.

### Release decision

`defer-release`. This corrects verification behavior but does not by itself
complete the unified package library build graph. Record the stricter rejection
behavior in the next Faber release notes if it reaches a public CLI release.

## Validation

Run from `faber/` unless stated otherwise, every Cargo command with the shown
timeout:

```bash
timeout 120 cargo test --lib binding
timeout 120 cargo test --lib manifest
timeout 180 cargo test --lib package
timeout 180 cargo test --test hygiene
cargo fmt --all -- --check
git diff --check
```

If Radix changes:

```bash
cd ../radix
timeout 180 cargo test -p radix file_interface -- --format terse
timeout 180 cargo test -p radix binding -- --format terse
timeout 180 cargo test -p radix --test hygiene
cargo fmt --all -- --check
git diff --check
```

The probe integration test must run its spawned `cargo check` with an explicit
timeout and use a dedicated target directory so concurrent tests do not share a
Cargo lock.

## Companion Skill Plan

- `correctness`: review duplicate handling, containment, and child-process
  failure projection.
- `red-green`: add the false-positive/false-negative fixture matrix before
  deleting the scanner.
- `cleanliness`: keep package analysis, path policy, and Rust proof in separate
  modules; do not grow `binding.rs` into a second compiler.
- `polish`: inspect every modified primary Rust file before factory closeout.

## Open Questions

No blocking product question remains. Factory must answer two implementation
questions from live code before B1/B4:

1. Can package compilation expose its analyzed file/interface set directly, or
   should a shared analysis-only package function be extracted?
2. Can normal Rust type rendering safely emit binding adapters, or is a small
   public Radix binding-probe renderer required?

Stop if a Faber type in a selected binding has no defined Rust ABI. Do not
erase the type or weaken the contract to make the probe compile.
