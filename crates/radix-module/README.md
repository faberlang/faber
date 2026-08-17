# radix

Rust implementation of the Faber compiler pipeline in the `radix` crate.

## CLI surface

The `radix` binary is the compiler-developer tool. Package and project workflows
belong to the public sibling **`faber`** CLI (`../faber`, Cargo package `faber`).

- inspection commands: `lex`, `parse`, `hir`, `mir`, `cli-ir`, `emit`, `check`, `targets`
- no `build` subcommand — use `faber build` for package builds and `faber emit` for stdout-oriented file emission
- formatter lives here as `radix::forma` (invoked via `faber format`)

## Design overview

- Frontend stages: lexing -> parsing -> semantic passes.
- Semantic passes: collect -> resolve -> lower -> typecheck -> borrow -> exhaustive -> lint.
- HIR is the stable, resolved AST used for codegen across targets.
- MIR is the validated middle IR used by `mir`, `radix mir`, and systems-lane emitters such as `wasm-text` and `llvm-text`.
- Types are interned in a `TypeTable` and referenced by `TypeId`.
- Name resolution uses a scoped symbol table keyed by `DefId`.

## Known TODO items

- Resolve pass: add namespaced lookup for enum variants and class members.
- Type lowering: wire type alias resolution and generic arity checks.
- Type checking: implement bidirectional inference for expressions and statements.
- Borrow analysis: implement ownership tracking for Rust target diagnostics.
- Exhaustiveness: implement match exhaustiveness and unreachable pattern checks.
