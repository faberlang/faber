# radix-module

The Faber module-compiler monolith: driver (phase ordering + target dispatch),
HIR→MIR lowering, codegen dispatch, services, and re-export modules for the
extracted leaves.

The semantic-analysis core lives in the `radix-semantic` leaf (collection,
resolution, AST→HIR lowering, typecheck, post-typecheck passes, locale/CLI/
file-interface contracts); the lexer, parser, syntax, types, HIR, diagnostics,
intrinsics, target identity, AIR, MIR core, and backend emitters each live in
their own leaves under `crates/`. This crate re-exports them under the
historical `crate::*` paths so `faber`, `radix-program`, and the `radix`
façade compile without call-site churn.

## CLI surface

This crate has no binary. The **`faber`** CLI (`../faber`, Cargo package
`faber`) drives it and owns package and project workflows.

- inspection commands: `faber lex`, `parse`, `hir`, `mir`, `cli-ir`, `emit`, `check`, `targets`
- `faber build` for package builds and `faber emit` for stdout-oriented file emission
- `faber format` and `faber convert` are thin compile entries over the HIR-backed Faber source emitter (`radix-hir-faber`); this crate's entry point for them is `src/driver/faber_format/compile.rs`

## Design overview

- Frontend stages: lexing -> parsing -> semantic passes (in `radix-semantic`).
- Semantic passes: collect -> resolve -> lower -> typecheck -> borrow -> exhaustive -> lint.
- HIR is the stable, resolved AST used for codegen across targets.
- MIR is the validated middle IR used by `faber mir` and systems-lane emitters such as `wasm-text` and `llvm-text`.
- Types are interned in a `TypeTable` and referenced by `TypeId`.
- Name resolution uses a scoped symbol table keyed by `DefId`.
