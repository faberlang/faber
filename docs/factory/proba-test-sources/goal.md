# Goal: `.proba` Test Source Files

**Status**: partial — inline `proba`/`probandum` is shipped; separate `.proba` discovery remains proposed (verified 2026-07-15)
**Created**: 2026-07-01
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/proba-test-sources/`
**Primary surface**: `faber test`, package discovery/loading, local import
resolution, stdlib test layout, source extension handling.

---

## Current implementation boundary

Inline `proba`/`probandum` support and CLI selection are shipped in the current
Faber package/test path. No live source-extension discovery for separate
`.proba` files was found; the remaining proposal is specifically the separate
file boundary described below. Keep this goal open for that boundary only.

## Historical summary

Introduce `.proba` as a first-class Faber test-source extension. A `.proba`
file is a complete Faber source file: it may contain helper functions, genera,
imports, setup code, `probandum`, `proba`, and `adfirma`. The main semantic
difference from `.fab` is placement in the source graph:

- `.fab` files are normal source and may be imported.
- `.fab` files may still contain inline `probandum` / `proba` declarations,
  included only in test mode.
- `.proba` files are test sources, discovered by `faber test`, and may not be
  imported by `.fab` or `.proba` files.

This gives the standard library its own test files without using sibling radix
`crates/exempla/corpus`, which should remain focused on language/core
intrinsics and reference surfaces. `.proba` is the preferred home for stdlib
behavior tests (sibling `norma` and packages), not the only legal location for
tests.

## Problem

New standard-library behavior needs Faber-level tests, but putting those tests
in exempla blurs ownership. Exempla are for core language and intrinsic
examples; stdlib modules need colocated, self-contained test sources that can
exercise public APIs, regression vectors, and cross-module behavior without
becoming importable library surface.

Inline tests inside `.fab` remain useful when a test is tightly coupled to
private implementation details in the same file. The problem is not inline tests
existing; the problem is forcing all stdlib validation into production source or
into exempla.

Current package discovery is `.fab`-oriented, and local import resolution will
resolve an explicitly named file extension when it exists. Without a test-source
boundary, `importa ex "./http.proba" privata http_test` could accidentally make
test code part of production/library code.

## Goals

- Treat `.proba` as full Faber syntax for parse, lower, typecheck, and Rust test
  harness generation.
- Make `faber test` discover `.proba` files in package and stdlib test scopes.
- Allow `.proba` files to contain arbitrary Faber helpers alongside
  `probandum` / `proba` declarations.
- Preserve inline tests in `.fab` files for same-file/private implementation
  cases.
- Ensure normal `faber build` / `faber run` ignore inline tests while
  `faber test` includes them.
- Disallow importing `.proba` files from any source file.
- Add a clear diagnostic for `.proba` imports:
  `.proba files are test sources and cannot be imported; move shared helpers to a .fab module`.
- Keep `.proba` out of normal `faber build`, `faber run`, and package library
  import graphs unless the command is explicitly test-oriented.
- Establish stdlib test placement such as `../norma/src/hash.proba` and
  `../norma/src/hash/sha256.proba`.

## Non-goals

- Moving existing exempla or deleting historical `stdlib-nativum` coverage in
  this goal.
- Changing `probandum`, `proba`, or `adfirma` syntax.
- Creating a separate test language.
- Forcing every test out of `.fab` files.
- Allowing `.proba` as an importable fixture format.
- Designing a full package-manager test convention beyond local package and
  stdlib discovery.

## Ground Truth Researched

- `EBNF.md` already defines `probandum`, `proba`, setup/teardown blocks, and
  test modifiers.
- `faber test` currently compiles package sources through
  `compile_package_with_test_selection` and then invokes Cargo's test harness.
- Package module naming strips `.fab` but does not yet treat `.proba` as a
  source extension.
- Local import resolution currently accepts explicit extensions when the target
  file exists, so `.proba` needs an explicit import rejection policy.
- Existing stdlib behavior tests often live under `crates/exempla/corpus`, which
  is not the desired home for new stdlib test coverage.

## Reference Packet

- `EBNF.md`
- `src/commands/test.rs`
- `src/package/compile.rs`
- `src/package/import_graph.rs`
- `src/package/modules.rs`
- `src/package_test.rs`
- `crates/radix/src/codegen/rust` test-harness emission
- `../norma/src/`
- `../radix/crates/exempla/corpus/probandum/probandum.fab`
- `../radix/crates/exempla/corpus/proba/proba.fab`

## Constraints And Invariants

- `.proba` is Faber source, not metadata and not a reduced grammar.
- `.proba` files are never production imports.
- Inline tests in `.fab` are allowed, but are test-mode declarations, not normal
  build/run behavior.
- Prefer `.proba` for public-contract tests, vector suites, regression matrices,
  and cross-module behavior.
- Prefer inline tests only when the test needs same-file private details or is a
  very small invariant attached to the implementation.
- Shared test helpers that need import reuse belong in `.fab` helper modules, not
  in `.proba`.
- Normal package builds must not include tests just because `.proba` files sit
  beside sources.
- `faber test` owns `.proba` discovery; `faber build` and `faber run` do not.
- Stdlib tests must not use exempla as their primary validation home.

## Implementation Shape

1. Source-extension model:
   - Add a source-kind distinction for `.fab` and `.proba`.
   - Teach module naming to strip `.proba` when generating test modules.
   - Keep `.fab` behavior unchanged.
2. Discovery:
   - Extend package test loading so `faber test` includes `.proba` files.
   - Preserve existing inline `proba` / `probandum` discovery from `.fab` files
     in test mode.
   - Keep package build/check/run loading `.fab` only unless a future command
     explicitly asks for test sources.
3. Import policy:
   - Reject any local import path that resolves to `.proba`.
   - Reject provider/library import paths ending in `.proba` if such a path is
     ever accepted by a resolver.
4. Stdlib test layout:
   - Add initial fixture tests under `../norma/src/*.proba` or
     `../norma/src/<module>/*.proba`.
   - Ensure `.proba` can import `norma:*` public facades and nested internal
     stdlib modules where appropriate.
5. Validation:
   - Add focused package tests for discovery, helper code in `.proba`, test
     selection, and import rejection.
   - Add a small stdlib proof test that does not touch exempla.

## Acceptance Criteria

- `faber test` discovers and runs a `.proba` file containing helper functions
  and `proba` declarations.
- `faber test` still discovers and runs inline tests declared in ordinary `.fab`
  files.
- `faber build` ignores adjacent `.proba` files.
- `faber build` and `faber run` do not emit or execute inline test declarations
  from `.fab` files.
- `importa ex "./http.proba" privata http_test` is rejected with a direct
  diagnostic.
- `.proba` files may import `.fab` helper modules and `norma:*` modules.
- `.proba` files may not import other `.proba` files.
- Stdlib tests can live beside stdlib source without adding new files to
  `crates/exempla/corpus`.

## Validation

```bash
cargo test -- proba
cargo test -- test_selection
cargo run -- test <fixture-package>
cargo run -- build <fixture-package>
```

Add a repository script such as `./scripta/test-stdlib` only after the first
stdlib `.proba` fixture proves the command shape.

## Open Questions

- Should stdlib `.proba` discovery be part of ordinary `faber test sibling norma/src`
  or a dedicated `./scripta/test-stdlib` wrapper first?
- Should `.proba` files generate module names with a `_proba` suffix to avoid
  collision with sibling `.fab` modules of the same stem?
- Should package manifests eventually declare test roots, or is recursive
  discovery beside sources enough for v1?

## Stop Conditions

- Stop if `.proba` import rejection requires weakening normal local import
  diagnostics.
- Stop if `.proba` discovery makes normal package builds include test-only code.
- Stop if implementation starts moving exempla or redesigning the test syntax
  instead of adding the test-source extension.
