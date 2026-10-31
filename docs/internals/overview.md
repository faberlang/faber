# Compiler internals: source to output

This is a source-guided manual of the compiler and runtimes present in the
public tree. It is not a language specification or a promise of full support
on every target. The implementation and the Cargo features of the binary being
used determine what is available.

## Reading map

| Page | Purpose |
| --- | --- |
| [Documentation standard](documentation-standard.md) | What file headers, public items, and implementation comments must explain. |
| [HIR](hir.md) | The source-structured representation, analysis state, and companion facts. |
| [MIR](mir.md) | Execution-shaped control flow, storage, validation, and consumers. |
| [Adding a target](adding-a-target.md) | Static backend integration, traced through the C emitter. |

Paths below are relative links into the public repository. Some shared enums
and feature-gated branches mention targets whose implementations do not ship
here. Their presence is not evidence that those backends are available; this
manual does not describe their internals. For grammar and source examples, use
[the grammar](../../grammar/EBNF.md), [the corpus](../../corpus/), and
[the authoring skill](../../skills/faber/SKILL.md).

## The pipeline and its owners

```text
UTF-8 source + reader locale + explicit import context
  -> tokens + interner                         radix-lexer
  -> parsed syntax                             radix-parser / radix-syntax
  -> collect -> resolve -> HIR lowering         radix-semantic
  -> typecheck + post-typecheck analyses        radix-semantic
  -> coherent analyzed module                  radix-module driver
       |
       +-> HIR source emission                 radix-hir-c / radix-hir-faber
       +-> analyzed-HIR artifact                radix-hir-fhir + driver assembly
       +-> HIR-to-MIR lowering + validation     radix-mir-lower / radix-mir
             +-> interpretation                radix-mir-runner
             +-> portable MIR image            radix-mir-fmir + package assembly
             +-> native object or JIT          radix-mir-clif
```

The diagram describes data flow, not a promise that every command visits every
box. `radix-module` sequences module compilation; `radix-program` composes
module analyses into package graphs and artifacts; `faber` owns user-facing
policy, output writing, external tool invocation, and execution orchestration.
There is no second package-specific parser or typechecker.

### 1. Establish source context before lexing

The product resolves reader packs in
[faber's locale policy](../../crates/faber/src/package/locale.rs). Package
per-file selection lives in
[source_files.rs](../../crates/radix-program/src/source_files.rs).
The intended precedence is an explicit reader override, file frontmatter,
package locale, then the product default (`en`). Diagnostic locale selection is
separate: changing message rendering must not change the reader language.
The module layer receives that policy via `Config` and `Session` in
[session.rs](../../crates/radix-module/src/driver/session.rs).

[Source preparation](../../crates/radix-module/src/driver/source.rs) and
[frontmatter handling](../../crates/radix-module/src/driver/file_frontmatter.rs)
keep the body and its source offsets available. Import contracts are explicit
inputs to `driver::compile`; they are not guessed by the code generator.

### 2. Lex and parse

[radix-lexer](../../crates/radix-lexer/src/lib.rs) produces tokens and an
`Interner`. Symbols belong to that interner. Spans use byte offsets, not
character positions; identifier normalization does not rewrite literal payloads.
Keywords are lexed as identifiers with reader-keyword identity information that
the parser can claim.

[radix-parser](../../crates/radix-parser/src/lib.rs) builds the untyped syntax
model in [radix-syntax](../../crates/radix-syntax/src/lib.rs), allocating
phase-local node IDs and recovering at syntax boundaries. Lexical errors stop
the driver front end; recovered parse diagnostics can be retained while
semantic analysis finds additional errors. The final error-bearing analysis
must not become emitted output. See the actual continuation checks in
[the module driver](../../crates/radix-module/src/driver/mod.rs).

### 3. Analyze and qualify

[radix-semantic](../../crates/radix-semantic/src/lib.rs) owns collection,
resolution, AST-to-HIR lowering, typechecking, and post-typecheck analyses.
[The pass registry](../../crates/radix-semantic/src/passes/mod.rs) is the
navigation map; `analyze_with_cli` in the semantic crate is the execution
coordinator. Collection, resolution, and HIR-lowering hard errors stop this
front half. Later analyses accumulate diagnostics; warnings alone are not
failure. Pass configuration and target qualification matter, so not every
analysis runs with identical policy on every route.

The driver packages a successful analysis as `AnalyzedModule`: HIR, type and
symbol arenas, resolver metadata, presentation, effect and provenance facts,
import identities, and diagnostics from the same run. `analyze_source` returns
this without emitting code. `compile_analyzed` applies warning policy,
qualifies the unit for the selected target, handles CLI entry programs, and
dispatches to output generation. `compile` joins those two operations. See
[HIR](hir.md) for the boundary contract.

### 4. Select an output family

- **Source output:**
  [HIR dispatch](../../crates/radix-module/src/codegen/mod.rs) reads
  `CodegenAnalysis`, runs shared rejection gates, and invokes a feature-enabled
  leaf. C emits one translation unit; the Faber re-emitter also uses the
  presentation sidecar and output locale.
- **HIR artifact:**
  [driver artifact assembly](../../crates/radix-module/src/hir/artifact.rs)
  bridges analyzed state to
  [the FHIR format](../../crates/radix-hir-fhir/src/lib.rs). This is a codec and
  integrity boundary, not a source-emitter trait implementation.
- **Execution-shaped output:**
  [MIR lowering](../../crates/radix-mir-lower/src/lower.rs) consumes
  `AnalyzedModuleApplicationView` and returns `LoweredMirUnit`, whose
  `ValidatedMir` token carries the program and validation context. The
  [MIR chapter](mir.md) follows this into the interpreter, FMIR, and Cranelift.

`Target` in [radix-target](../../crates/radix-target/src/lib.rs) owns identity,
canonical spellings, and feature availability. The
[service capability table](../../crates/radix-module/src/services/targets.rs)
separately describes check/build/run/package support. An enum variant can exist
while its backend feature is absent. Use `faber targets` for the current binary's
reported surface; do not infer it from the workspace member list.

### 5. Write, build, or execute at the product boundary

[Emit commands](../../crates/faber/src/commands/emit.rs) resolve product locale
policy, request compiler output, report diagnostics, and hand the artifact to
[postprocessing](../../crates/faber/src/postprocess.rs). Compiler leaf emitters
do not run target toolchains.

Execution is not synonymous with source emission. In
[the run command](../../crates/faber/src/commands/run.rs), an unqualified run
can try Cranelift JIT when that feature and route are eligible, falling back on
a compile-time decline to the interpreter. It does not retry after execution
has begun. `--target runner` explicitly selects interpretation; `--target
clif-jit` explicitly selects the JIT route. Runtime output then goes through
the selected engine and host, not through `services::output_code`.

## A concrete program to follow

The public C fixture
[scalars.fab](../../crates/radix-hir-c/tests/fixtures/run/scalars.fab) has
English frontmatter, arithmetic, locals, and printing. Its first expression is
`print 1 + 2`. Follow it without needing a private backend:

1. The front end reads its `en` pack, interns names, and parses its entry block.
2. Semantic lowering builds `HirModule::entry` and HIR expressions. Analysis
   establishes types and any folds or store facts before backend dispatch.
3. On the C path, `Target::HirC` reaches `CCodegen` through
   [generate_c_from_analyzed](../../crates/radix-module/src/codegen/mod.rs).
   The fixture's output is pinned by `scalars_program_is_pinned` in
   [emit_test.rs](../../crates/radix-hir-c/src/emit_test.rs).
   [scalars.c](../../crates/radix-hir-c/tests/fixtures/run/scalars.c) shows the
   first sum as `INT64_C(3)` passed to `printf`, plus the required runtime header.
4. On the interpreter path, HIR becomes validated MIR. Computation and printing
   are explicit operations; [the runner](../../crates/radix-mir-runner/src/lib.rs)
   evaluates them and its [Host](../../crates/radix-mir-runner/src/host.rs)
   boundary delivers output. The C fixture is not an assertion that MIR keeps
   the same constant-folded shape as C.

Optional local probes, **not a record of commands run for this manual** (they
require a built `target/debug/faber`):

```bash
target/debug/faber lex crates/radix-hir-c/tests/fixtures/run/scalars.fab
target/debug/faber parse crates/radix-hir-c/tests/fixtures/run/scalars.fab
target/debug/faber hir crates/radix-hir-c/tests/fixtures/run/scalars.fab
target/debug/faber mir crates/radix-hir-c/tests/fixtures/run/scalars.fab
target/debug/faber emit -t c crates/radix-hir-c/tests/fixtures/run/scalars.fab
target/debug/faber run --target runner crates/radix-hir-c/tests/fixtures/run/scalars.fab
```

**Inspection caveat:** `faber hir` uses
[the HIR inspection service](../../crates/radix-module/src/services/inspect.rs):
collect/resolve prerequisites followed by HIR lowering, not the full typecheck
pipeline. Missing expression types in that dump are not evidence that emitted
code received untyped input. MIR inspection takes the analyzed route.

## Packages add a graph, not another language front end

[Discovery](../../crates/radix-program/src/discovery.rs),
[manifest loading](../../crates/radix-program/src/manifest.rs), and
[package analysis](../../crates/radix-program/src/analyze.rs) establish the
source units and import graph in `AnalyzedProgram`. `AnalysisSeams` supplies
product-owned inputs such as library resolution.
[Artifact planning](../../crates/radix-program/src/artifact_plan.rs) decides
whether a selected output family has a supported package route.

For MIR packages, [the package driver](../../crates/radix-program/src/mir/driver.rs)
prepares analysis, links reachable definitions, lowers units, merges identities
and arenas, and validates the result before lending it to a consumer.
[Linking](../../crates/radix-program/src/mir/link.rs) and
[remapping](../../crates/radix-program/src/mir/remap.rs) are first stops for a
failure that occurs only across module boundaries. FMIR assembly/loading lives
in [image.rs](../../crates/radix-program/src/mir/image.rs); its wire contract is
owned by the separate format crate. A single-file C emitter does not imply
package C support.

## Public crate map

This table covers the current public compiler/product/support crates, not every
member of a larger development workspace. Each link is an implementation entry
point, not a claim that all historical crate-header text is current.

| Crate or runtime package | Responsibility |
| --- | --- |
| [radix-lexer](../../crates/radix-lexer/src/lib.rs) | Tokens, lexing, symbols/interner, reader keyword identities. |
| [radix-syntax](../../crates/radix-syntax/src/lib.rs) | Parsed syntax nodes and traversal surface. |
| [radix-parser](../../crates/radix-parser/src/lib.rs) | Recursive-descent parsing and syntax recovery. |
| [radix-diagnostics](../../crates/radix-diagnostics/src/lib.rs) | Shared diagnostic transport, arguments, and source spans. |
| [radix-types](../../crates/radix-types/src/lib.rs) | Semantic type/index arenas and definition identity. |
| [radix-intrinsics](../../crates/radix-intrinsics/src/lib.rs) | Compiler-owned receiver intrinsic catalog. |
| [radix-semantic](../../crates/radix-semantic/src/lib.rs) | Resolution, HIR lowering, typechecking, analyses, locale and diagnostic catalogs. |
| [radix-hir](../../crates/radix-hir/src/lib.rs) | HIR nodes, visitors, and neutral HIR fact carriers; not the lowering passes. |
| [radix-mir](../../crates/radix-mir/src/lib.rs) | MIR nodes, validation, layout/control-flow helpers, dumps, and analysis. |
| [radix-mir-lower](../../crates/radix-mir-lower/src/lib.rs) | Analyzed HIR to validated MIR and lowering metadata. |
| [radix-target](../../crates/radix-target/src/lib.rs) | Target identity and feature-gate vocabulary. |
| [radix-codegen-shared](../../crates/radix-codegen-shared/src/lib.rs) | HIR emitter trait, text/naming utilities, shared rejection gates. |
| [radix-hir-c](../../crates/radix-hir-c/src/lib.rs) | Fail-closed HIR-to-C source emitter. |
| [radix-hir-faber](../../crates/radix-hir-faber/src/lib.rs) | Faber source re-emission for formatting and conversion. |
| [radix-hir-fhir](../../crates/radix-hir-fhir/src/lib.rs) | Analyzed-HIR wire schemas, codecs, integrity checks. |
| [radix-mir-fmir](../../crates/radix-mir-fmir/src/lib.rs) | Portable MIR image schemas, text/binary codecs, integrity checks. |
| [radix-mir-runner](../../crates/radix-mir-runner/src/lib.rs) | MIR interpreter and execution APIs. |
| [radix-mir-clif](../../crates/radix-mir-clif/src/lib.rs) | Cranelift native-object and JIT emission. |
| [faber-host-clif](../../runtime/clif/src/lib.rs) (`runtime/clif`) | Host runtime and symbols used by Cranelift output. |
| [radix-runtime-contract](../../crates/radix-runtime-contract/src/lib.rs) | Shared runtime display, traps, shape, and scalar contracts. |
| [radix-abi](../../crates/radix-abi/src/lib.rs) | Compiler-owned host ABI symbol and wire-code tables. |
| [radix-module](../../crates/radix-module/src/lib.rs) | Module driver, services, output dispatch, and leaf re-exports. |
| [radix-program](../../crates/radix-program/src/lib.rs) | Package graph analysis, linking, and artifact assembly. |
| [radix](../../crates/radix/src/lib.rs) | Library-only facade over module and program APIs; no binary. |
| [faber](../../crates/faber/src/lib.rs) | Product library and CLI package: commands, package policy, tooling, execution. |
| [proba](../../crates/proba/src/lib.rs) | Test inventory, selection, and MIR-runner case execution. |
| [faber-leaf](../../crates/faber-leaf/src/lib.rs) | Native effects such as filesystem, console, process, clock, and network routes. |
| [handroll](../../crates/handroll/src/lib.rs) | Std-only primitives, hashing, entropy, and arbitrary-precision integers. |
| [radix-hygiene](../../crates/radix-hygiene/src/lib.rs) | Production-source budget scans and structural hygiene checks. |

Outside Cargo crates, [Norma](../../lib/norma/) supplies Faber library source,
[locale packs](../../locale/) supply reader/message data, and
[the C runtime](../../runtime/c/faber.h) supplies header-only support for C output.

## Where to look when something goes wrong

Locate the earliest incorrect representation or missing fact; do not repair an
upstream type or identity failure by guessing in a backend.

| Symptom | First investigation |
| --- | --- |
| Wrong locale, keyword, or literal interpretation | Product locale policy, per-file source selection, lexer, then parser. Compare `lex` and `parse` inspections. |
| Incorrect diagnostic location | Source peeling/body offsets and driver line adjustment; then diagnostic conversion/rendering in [radix-module](../../crates/radix-module/src/diagnostics/). |
| Unresolved definition, wrong type, or invalid inferred shape | Semantic collect/resolve/lower/typecheck and the matching HIR/type arena. |
| Correct semantics but lost comments or changed authored form | `HirPresentation`, [presentation witnesses](../../crates/radix-module/src/hir/presentation_witness.rs), and the Faber re-emitter. |
| Checks but cannot lower | MIR lowerer error issue/span, then validator category; distinguish unsupported lowering from malformed produced MIR. `faber check --lower` is the product probe. |
| Correct MIR but wrong interpreted result | Runner value/statement execution and Host implementation; inspect runtime contract helpers for traps/display. |
| Correct single unit but wrong package | Import contracts, package link/remap, merged type/symbol arenas, and metadata carriage. |
| Named target unavailable | Feature forwarding, `Target::is_available`, and [the product gate](../../crates/faber/src/package/target_gate.rs), not emitter expression logic. |
| Wrong generated C | C expression/statement/type/naming modules and the runtime header; compare the focused pinned/rejection fixture before host execution. |
| Correct emitted artifact but build/link failure | Product postprocessing/build orchestration, runtime dependencies, and the external toolchain; not parser recovery. |

**Limits of this manual:** it is a structural guide, not a measured target
coverage matrix. No cross-target equivalence, performance, platform availability,
or complete device execution support is established here. Treat those as
unverified unless the relevant focused tests and host-specific proofs have run.
