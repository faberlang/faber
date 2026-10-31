# Documentation standard

This standard is for compiler, emitter, runtime, and tooling documentation. It
specifies what maintainers should explain; it does not add Faber syntax or
semantics and does not claim that every existing comment already complies.
Start with the [overview](overview.md); representation contracts are in
[HIR](hir.md) and [MIR](mir.md), and backend wiring is in
[adding a target](adding-a-target.md).

## Evidence and scope

The implementation is the authority. Describe what the code does now, not what
a historical plan intended. Link to the owning public source file and name the
relevant item. A private plan, issue number, workstation path, or unpublished
crate is not a usable reference for a public reader.

Separate three kinds of statement:

- **Contract:** what a producer guarantees and a consumer may rely on. Identify
  the check or construction rule that enforces it.
- **Implementation:** how the current code satisfies that contract. Explain why
  a non-obvious choice exists, without promising that choice forever.
- **Limitation or uncertainty:** what is unsupported or not established. Say
  “not verified” when evidence is missing; say where to investigate. Do not
  turn a plausible inference into a guarantee.

A target enum variant, a capability row, a compiled-in backend, and an executable
product route are different things. Do not infer one from another. Similarly,
“HIR” alone does not imply that typechecking has completed. These distinctions
belong in API documentation, not just in this manual.

## File headers

Rust modules should begin with `//!` documentation. Use the equivalent native
comment form for other source files; scripts can use a module docstring. A
header should answer:

1. **Ownership:** what responsibility lives here, and what deliberately lives
   elsewhere? Name the representation or phase, not a vague “helpers” bucket.
2. **Boundary:** what inputs arrive, what outputs leave, and which arenas or
   side tables must accompany them?
3. **Invariants:** what facts must hold on entry and what is established on exit?
   Identify ID scopes, ordering constraints, and optional-field meanings.
4. **Failures and effects:** does invalid input return diagnostics, stop a phase,
   recover, or fail closed? Does this file read files, launch tools, mutate
   shared state, or merely construct data?
5. **Navigation:** where does the next phase live? Put detailed algorithms near
   their implementation rather than duplicating them throughout the tree.

Keep a small file's header small. A data model needs its invariants; a phase
coordinator also needs phase order and error policy. Do not fill headings with
boilerplate where no such concern exists.

### Header examples from the tree

- [HIR nodes](../../crates/radix-hir/src/nodes.rs) separates definition identity
  (`DefId`) from annotatable-node identity (`HirId`), explains source spans,
  and explicitly says that HIR is not necessarily fully typed.
- [MIR lowering](../../crates/radix-mir-lower/src/lower.rs) identifies analyzed
  HIR as input, validated MIR as output, and missing type information as an
  error rather than a backend fallback.
- [Codegen input](../../crates/radix-module/src/codegen/input.rs) explains why
  `CodegenAnalysis` exposes coherent semantic facts without depending on the
  driver's carrier type. This records a dependency direction, not just a list
  of declarations.

For example, the following is an **adapted header**, not a new API or a quotation:

```rust
//! HIR-to-MIR lowering for one analyzed module.
//!
//! Reads AnalyzedModuleApplicationView and returns LoweredMirUnit with a
//! ValidatedMir token. The analyzed arenas are not mutated: synthesized
//! types and symbols belong to lowering-owned copies.
//!
//! Missing semantic types, unsupported HIR, and invalid produced MIR return
//! MirError values. This phase does not execute the program.
```

An old header is evidence of intent, not proof of current behavior. For example,
[C's crate header](../../crates/radix-hir-c/src/lib.rs) describes inline runtime
helpers, but [its helper implementation](../../crates/radix-hir-c/src/helpers.rs)
selects the header-only [C runtime](../../runtime/c/faber.h). Prefer the latter
when explaining the current output dependency.

## Public items

Document public types, traits, functions, constants, variants, and non-obvious
fields with `///`. A re-export should make its owner findable rather than invent
another contract. Begin with a useful one-sentence summary, then supply the
parts applicable to that item:

| Concern | What the documentation must establish |
| --- | --- |
| Inputs and identity | Meaning, units, lifetime, ID namespace, and required companion data; not merely the Rust parameter types. |
| Result | What success proves, what is owned or borrowed, and whether order is significant. |
| Preconditions | What the caller must establish. Distinguish enforced checks from assumptions. |
| Errors | An `# Errors` section for fallible operations: causes and structured failure surface, including unsupported features. |
| Panics | A `# Panics` section if deliberate panic conditions exist. Malformed source is not a valid reason to introduce a panic in a compiler phase. |
| Effects | Mutation, I/O, tool execution, caching, or process termination, when present. |
| Safety | A precise `# Safety` contract for an unsafe API; local safety arguments at unsafe operations and implementations. |
| Example | A minimal real use when it clarifies the contract; state whether an example is executable, illustrative, or an excerpt. |

Do not imply that `None` always means “not yet computed.” It can also mean “not
applicable,” “no entry,” or “failed analysis,” depending on the field. Explain it
at the field or link to the precise owner. Do not label arena-local IDs as
persistent source identities.

### Item examples from the tree

- `SemanticResult::success` in
  [semantic analysis](../../crates/radix-semantic/src/lib.rs) explains that
  warnings are non-fatal. The implementation also requires HIR and access facts;
  a reader should not equate a nonempty result with successful analysis.
- `MirType` and `MirProgram::allocate_function_id` in
  [MIR nodes](../../crates/radix-mir/src/nodes.rs) explain semantic versus layout
  identity and the allocator's checked overflow behavior. The allocator also
  records its current scan cost and the condition for reconsidering it.
- `ValidatedMir::patch_program` in
  [the validator](../../crates/radix-mir/src/validate/validator.rs) consumes the
  proof token and only returns a replacement after validation. That ownership
  transition is part of the API, not an implementation detail to omit.

Here is a small **usage example** of the existing `CodeWriter` API in
[radix-codegen-shared](../../crates/radix-codegen-shared/src/lib.rs):

```rust
use radix_codegen_shared::CodeWriter;

let mut writer = CodeWriter::new();
writer.writeln("outer");
writer.indented(|writer| writer.writeln("inner"));
assert_eq!(writer.finish(), "outer\n    inner\n");
```

The example illustrates line-start indentation and consuming finalization. It
is not an example of Faber source or a new formatting rule for the language.

## Implementation comments

Use comments to preserve reasoning that code alone does not express:

- why evaluation order, a representation, or an extra pass is necessary;
- which upstream fact permits an optimization;
- why an apparently simpler alternative is wrong;
- a bounded workaround, its observable limitation, and its removal condition;
- the exact safety argument for an unsafe operation.

Avoid narrating assignments, paraphrasing a function name, or citing an
unavailable design discussion in place of the reasoning. A TODO is not a
contract: describe the current behavior first, especially if it rejects input.

Two concrete examples:

- `CCodegen::generate` in [the C leaf](../../crates/radix-hir-c/src/lib.rs)
  explains the second emission pass: allocation is discovered during emission,
  and only allocating programs need the hidden arena parameter.
- `a_switch_with_a_break_keeps_the_if_chain` in
  [C control tests](../../crates/radix-hir-c/src/control_test.rs) explains that a
  C `switch` would capture a break intended for the surrounding loop. The
  comment states the semantic hazard and the test pins the chosen shape.

For unsafe code, “safe because checked above” is insufficient. The `Send`
implementation for `DetachedMir` in
[the validator](../../crates/radix-mir/src/validate/validator.rs) gives a useful
model: it explains the uniquely owned type table, the absence of accessors and
cloning, and how the remaining fields are checked for thread transfer.

## Keeping documentation reviewable

- Update the owning API documentation and these pages when a boundary changes.
  Avoid copying full enum inventories that will immediately drift.
- Use relative links to public source and companion pages. Name symbols as
  search anchors; line numbers and commit-specific links age badly.
- State what a test proves. A pinned output test proves emitted shape, not that
  a host compiler accepts it or that executing it matches the interpreter.
  Diagnostic tests should assert code, issue, and named arguments, not localized
  prose; see [C test support](../../crates/radix-hir-c/src/test_support.rs).
- Review examples against source and focused fixtures. If examples were not run,
  report that fact rather than presenting them as tested output.
- Choose narrow proofs for the changed boundary. A prose-only correction needs
  link, name, and contract review; a runtime or compiler change additionally
  needs the relevant executable proof on a machine allowed to build it.

A documentation review is complete when a reader can identify the producer,
the consumer, the facts exchanged, the failure behavior, and a public place to
check each important claim.
