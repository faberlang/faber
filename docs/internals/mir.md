# MIR: explicit execution and a validation boundary

MIR is the mid-level intermediate representation consumed by the interpreter
and native backend, and carried in portable MIR images. It makes storage,
evaluation order, calls, and control-flow edges explicit. It is not target
assembly, not a second typechecker, and not a globally single-assignment IR.

See [HIR](hir.md) for its semantic input and the [overview](overview.md) for
product routing.

## Owners and entry points

| Responsibility | Public source |
| --- | --- |
| Core nodes and ID types | [radix-mir/src/nodes.rs](../../crates/radix-mir/src/nodes.rs). |
| Traversal and successor enumeration | [visit.rs](../../crates/radix-mir/src/visit.rs). |
| HIR-to-MIR orchestration | [radix-mir-lower/src/lower.rs](../../crates/radix-mir-lower/src/lower.rs), `lower_analyzed_unit`. |
| Validator and proof token | [validate/validator.rs](../../crates/radix-mir/src/validate/validator.rs). |
| Semantic validation metadata | [validate/context.rs](../../crates/radix-mir/src/validate/context.rs), `MirValidationContext`. |
| Human-readable dump | [dump.rs](../../crates/radix-mir/src/dump.rs). |
| Package preparation/linking | [radix-program/src/mir/driver.rs](../../crates/radix-program/src/mir/driver.rs). |

`lower_analyzed_unit` accepts an `AnalyzedModuleApplicationView` and
`MirLoweringOptions`. It reads the analyzed unit immutably, clones its type
arena and interner for lowering-owned extensions, collects context maps, lowers
items and entry, and validates the result. The returned `LoweredMirUnit` owns
its `ValidatedMir` token and carries auxiliary data such as monomorphizations,
closure environments, generic templates, device metadata, and display tokens.
Those facts are not all fields of `MirProgram`.

Errors are `MirError` values with an issue, primary span, and optional definition
span. Unsupported HIR, missing semantic types, construction misuse, and invalid
produced MIR are distinct causes even when reported through the same result
channel. A successful semantic check does not imply that every consumer can
lower or execute that program.

## Program, function, and block

[The node definitions](../../crates/radix-mir/src/nodes.rs) establish this shape:

- `MirProgram` is a vector of `MirFunction` values. Function order gives stable
  rendering order; it is not the function-ID lookup table.
- `MirFunction` has a `MirFunctionId`, optional source `DefId` and name, parameters,
  locals, temporaries, blocks, return and optional alternate-exit types,
  async/generator posture, shader stage, and span. A synthetic function may have
  no source definition; that does not determine its device role or visibility.
- `MirParam` binds a typed parameter to a local slot. Its `MirParamMode` retains
  the parameter's access posture rather than making a backend rediscover it.
- `MirBlock` has a `MirBlockId`, a sequence of statements, and **exactly one**
  `MirTerminator`. Block adjacency is not a control-flow edge.

IDs have separate namespaces: functions belong to a program; blocks, locals,
temporaries, and value uses are interpreted in their function/block context.
Do not cast an ID into a vector index and assume it identifies that element.
`MirFunctionIndex` is the shared function lookup mechanism;
`MirProgram::allocate_function_id` allocates `max + 1` with checked overflow.
`MirFunction::block_by_id` checks its dense fast path and falls back to a search,
so sparse or reordered block IDs remain meaningful.

## Places, operands, values, and statements

These types are deliberately different:

| Type | Meaning |
| --- | --- |
| `MirPlace` | Storage location: a local/temp base plus projections. A projection's validity and resulting type depend on validation metadata. |
| `MirOperand` | Readable input: a place, temporary, earlier value ID, or constant. |
| `MirValue` | A typed computation with a `MirValueId`, kind, type, and source span. |
| `MirStatement` | An operation in the current block; it does not choose a CFG successor. |

`MirValueKind` includes operand, closure, unary/binary, option, and tensor-extent
computations. `MirStatementKind` separates assignment from ordinary calls,
runtime calls, and aggregate construction. Calls can write to an optional
destination; constructors write an aggregate into a place. A value's ID is not
the destination's storage ID.

**Value ordering invariant:** a `MirOperand::Value` is usable only after its
defining assignment has been processed in the **same block**. In
[statement validation](../../crates/radix-mir/src/validate/stmt.rs), the value
map is cleared for each block and populated after validating the assigned
value. [Operand validation](../../crates/radix-mir/src/validate/value.rs)
rejects an unknown value ID. This is not cross-block SSA dominance. To carry a
result across control-flow edges, the representation uses addressable locals
or temporaries, not an earlier block's value ID.

Locals and temporaries can be assigned repeatedly, so do not apply a global
SSA optimization merely because a `MirValueId` exists. Producers must preserve
evaluation sequencing, especially around effects, traps, and calls.

## Terminators own control flow

`MirTerminatorKind` includes:

- `Goto`, `Branch`, and `Switch`, with explicit block IDs;
- `Return` with an optional normal result;
- `ReturnError` for the alternate-exit result;
- `TryCall` with normal and error destinations/continuations;
- `Unreachable`;
- `ReturnThread`, a represented form that current structural validation rejects
  as pending lowering, rather than accepting it as ordinary return.

A failable call is a terminator because it selects a normal or error edge. An
ordinary statement call must not silently discard that split. The validator
checks successor existence, branch/return/call typing, and CFG structure; see
[stmt.rs](../../crates/radix-mir/src/validate/stmt.rs). Use
`terminator_successors` in [visit.rs](../../crates/radix-mir/src/visit.rs) rather
than maintaining a private, incomplete edge enumerator.

An illustrative control-flow sketch, **not dump syntax or a Rust constructor**:

```text
entry:   compute condition; Branch -> then / else
then:    store chosen value into a result local; Goto join
else:    store alternate value into the same local; Goto join
join:    read the local; Return
```

A value ID computed only in `then` is not a legal operand in `join` merely
because `then` precedes it in the block vector.

## Type and metadata invariants

`MirType` wraps a semantic `TypeId` and an optional `MirLayoutId`. Semantic
identity comes from the matching type arena; a layout handle is not an inferred
replacement type. Layout and physical representation helpers live in
[layout.rs](../../crates/radix-mir/src/layout.rs).

`MirValidationContext` carries the type table, layout table, optional interner,
function signatures, external identities, struct/variant field maps and order,
closure environments, display facts, and other metadata needed to interpret
those references. `MirProgram` alone is insufficient for all validation or
execution. Empty metadata is valid only for a program that actually validates
against it.

The validator is split by obligation:

| Checks | Implementation |
| --- | --- |
| Program/function declarations, statement destinations, calls/returns, CFG | [stmt.rs](../../crates/radix-mir/src/validate/stmt.rs). |
| Computations, operands, projections, call arguments, closures | [value.rs](../../crates/radix-mir/src/validate/value.rs). |
| Type validity and assignability helpers | [type.rs](../../crates/radix-mir/src/validate/type.rs). |
| Aggregate fields and element representation | [aggregate.rs](../../crates/radix-mir/src/validate/aggregate.rs). |
| Runtime intrinsic signatures and operation constraints | [intrinsic.rs](../../crates/radix-mir/src/validate/intrinsic.rs). |

This is a navigation summary, not a claim that validation proves all dynamic
properties. Validation is a well-formedness/type/structural boundary. Runtime
traps, target capability refusals, and host capability failures still exist.

## The `ValidatedMir` proof token

`validate_program` checks a borrowed program/context and returns success or
`MirValidationError` values. `ValidatedMir::new` consumes the program and context,
runs validation, and constructs the owned token. Public runner/native-emission
entry points take this token instead of arbitrary bare MIR.

The token has no unrestricted mutable program accessor:

- `into_parts` gives up the proof and returns the unvalidated parts.
- `patch_program` consumes the token, permits a mutation, and returns a fresh
  token only after revalidation. On a patch or validation error the consumed
  program/context/token are not returned as validated state.
- `LoweredMirUnit::patch_program` preserves this transition at the lowering
  carrier level.
- `detach` / `DetachedMir::attach` support transferring already-validated data
  to a worker. Attachment preserves the proof; it is not a public unchecked
  constructor for arbitrary MIR.

The implementation is in
[validator.rs](../../crates/radix-mir/src/validate/validator.rs). Explicit
role-aware constructors additionally enforce device composition obligations.
A transformed program must not retain a token for an older program snapshot.

[validated_mir_reject.rs](../../crates/radix-mir/tests/validated_mir_reject.rs)
constructs malformed programs to prove rejection, including missing block
edges and invalid return forms. These tests are a useful companion to the
positive lowering tests; acceptance alone cannot establish the rejection gate.

## Linking, serialization, and execution

### Package linking

[Package MIR preparation](../../crates/radix-program/src/mir/driver.rs) composes
analysis, import linking, unit lowering, merging, and validation. The
[linker](../../crates/radix-program/src/mir/link.rs),
[unit/merge lowering](../../crates/radix-program/src/mir/lower.rs), and
[remapper](../../crates/radix-program/src/mir/remap.rs) preserve identities across
independently allocated unit arenas. A numerical `TypeId`, `Symbol`, `DefId`, or
function ID from one unit is not automatically valid in the merged program.
Generic templates, closure metadata, display tokens, and device-origin facts
must accompany the functions that need them.

### Portable images

[radix-mir-fmir](../../crates/radix-mir-fmir/src/lib.rs) owns the FMIR schema and
TOML-text/postcard-binary codecs. Package image assembly and reconstruction live
in [image.rs](../../crates/radix-program/src/mir/image.rs). Version/tag and wire
integrity checks do not replace rebuilding the execution context and validating
loaded MIR. Do not treat a text dump as the FMIR wire format.

### Consumers

- [radix-mir-runner](../../crates/radix-mir-runner/src/lib.rs) interprets validated
  MIR. `run_entry` uses the default kernel-refusal policy. Other entry points
  carry explicit policies/limits; choosing a runner does not prove device
  support. [Host](../../crates/radix-mir-runner/src/host.rs) is its process-effect
  seam, including diagnostic output and provider dispatch. Concrete hosts can
  expose or refuse capabilities.
- [radix-mir-clif](../../crates/radix-mir-clif/src/emit.rs) accepts validated MIR
  for object or JIT emission and applies its own capability classification.
  [runtime/clif](../../runtime/clif/src/lib.rs) supplies host runtime symbols;
  [product build](../../crates/faber/src/package/clif_build.rs) and
  [JIT orchestration](../../crates/faber/src/package/clif_jit.rs) own linking or
  in-process launch. Structurally valid MIR can still be outside this backend's
  supported subset.

## Debugging the boundary

Use the narrowest observation that distinguishes the failure:

1. `faber check` failing: investigate front-end analysis first.
2. `faber check --lower` failing: inspect the structured lowering/validation
   issue and source span; no execution is needed to reproduce a bad CFG.
3. `faber mir` showing wrong sequencing or edges: investigate the producer in
   `radix-mir-lower` and package transformations, not the runner.
4. Correct MIR but wrong result: investigate runner operations, host effects, or
   the native lowering, with the same program/context as the failing run.
5. Failure only after serialization or linking: compare reconstructed arenas,
   metadata, and validation, not just the printed function bodies.

These commands are investigative suggestions, not execution proofs for this
manual. Full target coverage, device correctness, and runtime equivalence are
not established by the structural contracts described here.
