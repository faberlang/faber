# HIR: source structure with semantic identities

HIR is the high-level intermediate representation. It retains source-level
structure for analysis, diagnostics, and readable source emission, while
replacing resolved paths with semantic identities. It is neither the parsed
syntax tree nor a control-flow graph, and the existence of a HIR value alone
is not proof that analysis succeeded.

See the [overview](overview.md) for the whole pipeline,
[MIR](mir.md) for the execution-shaped successor, and
[adding a target](adding-a-target.md) for a HIR consumer.

## Ownership and lifecycle

| Responsibility | Public source |
| --- | --- |
| Nodes and neutral fact carriers | [radix-hir](../../crates/radix-hir/src/lib.rs), especially [nodes.rs](../../crates/radix-hir/src/nodes.rs). |
| Read-only/mutable traversal | [visit.rs](../../crates/radix-hir/src/visit.rs). |
| AST-to-HIR production | [radix-semantic lowering](../../crates/radix-semantic/src/lower/mod.rs), `LowerRequest`, `lower`, and `LoweredProgram`. |
| Types and definition handles | [radix-types](../../crates/radix-types/src/lib.rs). |
| Analysis sequencing | [radix-semantic](../../crates/radix-semantic/src/lib.rs) and [pass registry](../../crates/radix-semantic/src/passes/mod.rs). |
| Successful module carrier | `AnalyzedModule` in [the driver](../../crates/radix-module/src/driver/mod.rs). |
| HIR source-emission view | `CodegenAnalysis` in [codegen/input.rs](../../crates/radix-module/src/codegen/input.rs). |
| MIR-lowering view | `AnalyzedModuleApplicationView` in [analyzed_application.rs](../../crates/radix-semantic/src/analyzed_application.rs). |

Collection and resolution establish definitions, scopes, and type references
before HIR lowering. Lowering normalizes syntax, creates HIR identities, and
returns both the semantic tree and presentation data. Typechecking then fills
or validates type facts; subsequent passes establish flow, effect, shape,
legality, and other facts. These operations may rewrite eligible HIR forms as
well as populate tables. The semantic coordinator, not the node crate, owns
this ordering.

Collection/resolution/lowering hard errors stop downstream analysis. Typecheck
and later passes accumulate diagnostics. `SemanticResult` can contain partial
state, and `SemanticResult::success` requires HIR, access facts, and no error
severity diagnostic. `driver::analyze_source` additionally performs the driver's
qualification/admission work before returning an `AnalyzedModule`. Do not take
a recovered tree directly to a backend because it is traversable.

**Inspection is a different lifecycle:** `hir_inspection_for_source_with_config`
in [inspect.rs](../../crates/radix-module/src/services/inspect.rs) prepares
collection/resolution and lowers, without running the full typecheck pipeline.
That is the source of `faber hir` output. It is useful for normalization bugs,
not a certificate that an expression has its final analyzed type.

## Core tree shape

The following are existing types in [nodes.rs](../../crates/radix-hir/src/nodes.rs):

- `HirModule` contains `items`, an optional executable `entry` block, entry
  annotations, optional entry argument binding, and async-entry posture. The
  entry is for **one source module**, not a package root. `entry == None` is a
  legitimate module shape, not an analysis failure.
- `HirItem` / `HirItemKind` describe declarations and imports. Functions,
  parameters, fields, methods, variants, and locals carry definition identities
  appropriate to their roles.
- `HirBlock` holds statements and an optional tail expression. A tail expression
  is not an extra statement that a consumer may evaluate twice.
- `HirStatement` has a `HirId`, kind, and source span. `HirStatementKind` includes
  locals, expressions for effect, returns, break/continue, and other normalized
  statement forms.
- `HirExpression` has a `HirId`, `HirExpressionKind`, optional `TypeId`, and span.
  Resolved paths use `HirExpressionKind::Path(DefId)`. Calls, method calls,
  blocks, conditionals, patterns, optional operations, and conversions retain
  their distinct semantic structure rather than collapsing to strings.

An internal enum name need not match the source keyword in a reader's locale.
Consumers dispatch on the representation and consult the interner/locale
contracts where spelling is actually needed; they do not reparse emitted names.

## Identity and companion-state invariants

| Handle or fact | Meaning and consumer obligation |
| --- | --- |
| `DefId` | Named semantic definition, defined in `radix-types` and re-exported by `radix-hir`. Resolution supplies many IDs; lowering also creates synthetic bindings. Carry IDs instead of resolving a name again. |
| `HirId` | Annotatable HIR node identity used by analyses and presentation. It is not a `DefId` and is not a persistent source-location key. |
| `TypeId` | Reference into a particular `TypeTable`. It is semantic identity, not a byte layout, target type spelling, or portable cross-module number. |
| `Symbol` | Reference into the corresponding `Interner`. Never resolve a symbol with another module's interner simply because its numeric value is in range. |
| `Span` | Source byte range for diagnostics. Normalization should preserve the source construct that ought to receive an error, not an invented target-code position. |
| Optional expression type | Before typecheck, `None` is expected. A consumer requiring a final type must diagnose a missing fact, not select an arbitrary backend type. Other optional fields have their own contracts. |

Keep arenas and facts coherent. `AnalyzedModule` carries an `AnalysisStamp` and
the HIR, resolver, arenas, presentation, library registry, function facts,
resolved uses, qualified identities, provenance, shape obligations, and other
metadata from that unit. Neither `CodegenAnalysis` nor the application view is
permission to combine independently produced tables.

This does **not** assert that every optional field becomes `Some` on success.
For example, a local's explicit annotation and a function's authored return
annotation have different meanings from an expression's analyzed type. Follow
the field contract and resolver/type tables rather than imposing a blanket
“all options filled” rule.

### Side tables are semantic input, not backend hints

Examples of carried facts and their owners:

- `FunctionFactTable` in
  [function_facts.rs](../../crates/radix-semantic/src/function_facts.rs) records
  function effect/capture/call facts.
- `ResolvedUseIndex` in
  [resolved_use.rs](../../crates/radix-semantic/src/resolved_use.rs) records
  definition/import uses without asking emitters to repeat resolution.
- `ShapeObligationTable` in
  [shape_obligations.rs](../../crates/radix-semantic/src/shape_obligations.rs)
  retains runtime shape relations at checked boundaries.
- `LibraryRegistry` in [HIR nodes](../../crates/radix-hir/src/nodes.rs) carries
  library import provenance and imported declaration metadata.
- `TensorEdgePolicyTable` in
  [tensor_edge_policy.rs](../../crates/radix-hir/src/tensor_edge_policy.rs)
  carries resolved policy/provider facts.

The list is illustrative, not an alternative schema. The analyzed carrier's
fields and the consumer traits are authoritative for required facts.

## Semantic tree versus presentation

`HirPresentation` is a serializable sidecar in
[nodes.rs](../../crates/radix-hir/src/nodes.rs), not a field on `HirModule`.
It carries trivia ownership/attachments, source anchors, structural block and
module ends, entry explicitness, and witnesses for selected authored forms that
normalize to the same semantic shape.

This separation lets semantic passes remain trivia-blind while formatting and
conversion preserve distinctions that cannot be recovered from the tree alone.
For example, `elige_expr_ids` records expressions lowered from the switch form;
structurally similar authored match forms must not be re-spelled as switch just
because a backend recognizes the same tree shape. The Faber path in
[codegen dispatch](../../crates/radix-module/src/codegen/mod.rs) passes HIR and
presentation together to
[radix-hir-faber](../../crates/radix-hir-faber/src/lib.rs), with witness
normalization in
[presentation_witness.rs](../../crates/radix-module/src/hir/presentation_witness.rs).

A HIR rewrite must consider which IDs, spans, semantic tables, and presentation
witnesses it preserves or invalidates. Updating only the executable shape can
silently break formatting or analyzed-artifact round trips.

## Traversal and rewriting

`HirVisitor` and `HirVisitorMut` in
[visit.rs](../../crates/radix-hir/src/visit.rs) centralize the recursive shape.
Override the narrowest hook and call the matching walker when the traversal
should continue into children. Omitting that call deliberately cuts off the
subtree. `visit_def` observes introduced definitions, including bindings that
are not top-level items.

Walkers visit HIR children, not the contents of type arenas or interners. They
provide traversal, not semantic validation. Read-only and mutable walkers should
retain matching recursive coverage. A new variant or field must be reviewed in
both, as well as in analyses and consumers that need the new fact.

## Consumer boundaries and rejection

[Shared HIR dispatch](../../crates/radix-module/src/codegen/mod.rs) accepts
`CodegenAnalysis`, rejects recovery expressions via `reject_hir_errors`, applies
shared feature/type/leaf gates, checks target availability, and invokes the
selected leaf. A supported source construct can still be outside one backend's
implemented subset. A named rejection is preferable to partial or incorrect
output. The recovery variant `HirExpressionKind::Error` must not become target
placeholder code.

[MIR lowering](../../crates/radix-mir-lower/src/lower.rs) requires the analyzed
application view, not raw HIR alone. It copies semantic identity into MIR and
returns explicit errors for missing types or unsupported forms. It does not
repair a bad semantic analysis by inventing one.

[FHIR](../../crates/radix-hir-fhir/src/lib.rs) stores analyzed-HIR artifacts.
[Driver assembly/loading](../../crates/radix-module/src/hir/artifact.rs) bridges
wire data to coherent analyzed carriers; the format crate owns schema and
integrity checks. Serializable Rust nodes are not, by themselves, a versioning
policy. Presentation and other schema changes must follow the format's checks.

## Focused investigation

- Wrong normalization: compare parsed syntax with raw HIR inspection, then
  inspect the matching semantic-lowering module.
- Missing or wrong analyzed type: inspect semantic typechecking and the unit's
  resolver/type arena; raw `faber hir` alone cannot answer this question.
- Wrong source re-emission with correct execution: inspect presentation
  witnesses and the Faber emitter before changing semantic nodes.
- Missing definition only after linking: inspect package identity and arena
  remapping, not textual name lookup inside the emitter.

Useful existing proof locations are
[HIR node tests](../../crates/radix-hir/src/nodes_test.rs),
[visitor tests](../../crates/radix-hir/src/visit_test.rs),
[presentation tests](../../crates/radix-hir/src/presentation_test.rs), and
[analyzed compilation tests](../../crates/radix-module/src/driver/compile_analyzed_test.rs).
These cover different boundaries; none alone proves every backend or package
route. This chapter does not establish exhaustive HIR coverage for any target.
