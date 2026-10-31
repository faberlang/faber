# Adding a target: follow the C backend

Targets are statically wired Rust crates and explicit dispatch arms, not runtime
plugins. There is no universal backend registration function. HIR source
emitters, MIR consumers, and artifact serializers have different boundaries.
This chapter traces the existing `radix-hir-c` integration as an engineering
checklist; it does not authorize changes to Faber syntax, semantics, types,
Norma names, or locale surfaces.

Read the [overview](overview.md), [HIR contract](hir.md), and
[MIR contract](mir.md) before choosing a boundary. A new target must implement
the existing language faithfully or reject unsupported constructs by name;
it must not quietly narrow the language or compensate for missing upstream
analysis. Language-surface changes require separate approval.

## The C route in one table

| Layer | Existing C integration |
| --- | --- |
| Identity | `Target::HirC`, spelling `c`, required feature `hir-c` in [radix-target](../../crates/radix-target/src/lib.rs). |
| Leaf crate | [radix-hir-c/Cargo.toml](../../crates/radix-hir-c/Cargo.toml). |
| Leaf API | `CCodegen`, `COutput`, and `impl Codegen` in [lib.rs](../../crates/radix-hir-c/src/lib.rs). |
| Shared trait/error | `Codegen`, associated `Output`, and `CodegenError` in [radix-codegen-shared](../../crates/radix-codegen-shared/src/lib.rs). |
| Module re-export | [codegen/c/mod.rs](../../crates/radix-module/src/codegen/c/mod.rs). |
| Analyzed dispatch | `generate_from_analyzed_with_options` and `generate_c_from_analyzed` in [codegen/mod.rs](../../crates/radix-module/src/codegen/mod.rs). |
| Driver output | Feature-gated `Output::C` and module-owned `COutput` in [radix-module/src/lib.rs](../../crates/radix-module/src/lib.rs). |
| Text extraction | `output_code` in [services/emit.rs](../../crates/radix-module/src/services/emit.rs). |
| Capability/extension | C row and `c` extension in [services/targets.rs](../../crates/radix-module/src/services/targets.rs). |
| Product selection | `FaberCliTarget::HirC` in [cli/emit.rs](../../crates/faber/src/cli/emit.rs) and [target_gate.rs](../../crates/faber/src/package/target_gate.rs). |
| Output policy | [commands/emit.rs](../../crates/faber/src/commands/emit.rs) and [postprocess.rs](../../crates/faber/src/postprocess.rs). |
| Runtime | [runtime/c/faber.h](../../runtime/c/faber.h), selected by [helpers.rs](../../crates/radix-hir-c/src/helpers.rs). |

## 1. Choose the representation, not just the extension

C is a **HIR source emitter**. It consumes analyzed source structure and semantic
types, produces a C translation unit, and does not invoke a C compiler. A MIR
backend instead starts from `ValidatedMir` and is routed by the driver; see the
[Cranelift entry points](../../crates/radix-mir-clif/src/emit.rs). FHIR/FMIR are
artifact formats with codec and reconstruction contracts, not ordinary HIR
`Codegen` implementations.

Do not make the HIR trait pretend it can execute a program, build a native
binary, or serialize every representation. Select the existing boundary that
owns the required facts.

## 2. Keep the emitter a leaf

[C's manifest](../../crates/radix-hir-c/Cargo.toml) depends on neutral data and
utility crates, not the module driver or semantic pass coordinator in normal
dependencies. Its dev-dependency on `radix-module` is for compiling focused
fixtures through the real pipeline, not for production re-analysis.

The existing HIR trait contract is:

```rust
// Excerpt of radix_codegen_shared::Codegen, not a new interface.
pub trait Codegen {
    type Output;

    fn generate(
        &self,
        hir: &HirModule,
        types: &TypeTable,
        interner: &Interner,
    ) -> Result<Self::Output, CodegenError>;
}
```

The excerpt uses `HirModule` from `radix_hir`, `TypeTable` from `radix_types`,
`Interner` from `radix_lexer`, and `CodegenError` from `radix_codegen_shared`.
C's output type contains the generated `code: String`. `CCodegen::new` builds a
`NameCatalog` from the module and interner; dispatch installs the analyzed
module's display tokens before calling `generate`.

The module layer's wider `CodegenAnalysis` view also exposes effects, imports,
provenance, shape obligations, and carried provider analyses. Consume the facts
a target needs through an explicit contract. Do not reconstruct type decisions,
resolve imports from the filesystem, or re-enter semantic analysis from a leaf.
A backend needing additional facts requires a deliberate engineering change to
the boundary, not a hidden global table.

### How C divides emission

The leaf's [lib.rs](../../crates/radix-hir-c/src/lib.rs) owns the emitter context
and program assembly. More focused decisions live in:

- [expr.rs](../../crates/radix-hir-c/src/expr.rs) and
  [stmt.rs](../../crates/radix-hir-c/src/stmt.rs): expressions, statements, and
  sequencing;
- [items.rs](../../crates/radix-hir-c/src/items.rs): declarations and functions;
- [ty.rs](../../crates/radix-hir-c/src/ty.rs): supported semantic-to-C type forms;
- [names.rs](../../crates/radix-hir-c/src/names.rs): target-safe names;
- [numeric.rs](../../crates/radix-hir-c/src/numeric.rs) and
  [conversio.rs](../../crates/radix-hir-c/src/conversio.rs): numeric operations
  and checked conversion emission;
- [helpers.rs](../../crates/radix-hir-c/src/helpers.rs): runtime-facility and
  allocation requirements;
- [error.rs](../../crates/radix-hir-c/src/error.rs): structured unsupported errors.

`CCodegen::generate` rejects recovery HIR and emits once to discover whether
allocation occurs. If it does, emission runs again with an explicit arena
parameter threaded through the generated functions. This is a current C
implementation strategy, not a requirement on every HIR backend.

## 3. Wire identity, features, dispatch, and output together

Use C's complete path as the review checklist:

1. **Neutral identity:** review `Target`, `Target::ALL`, canonical spelling and
   parsing, availability spellings, `required_feature`, `is_available`, and
   target-policy methods in [radix-target](../../crates/radix-target/src/lib.rs).
   Identity must not depend on backend implementation crates. A new enum arm
   must get an explicit policy answer, not fall through a permissive wildcard.
2. **Cargo availability:** the module's `hir-c` feature enables the optional leaf
   dependency and forwards the identity feature. Forwarding continues through
   [radix-program](../../crates/radix-program/Cargo.toml),
   [radix](../../crates/radix/Cargo.toml), and
   [faber](../../crates/faber/Cargo.toml). Check
   [radix-module's manifest](../../crates/radix-module/Cargo.toml) and
   [radix-target's manifest](../../crates/radix-target/Cargo.toml) too.
   `hir-c` is outside `full-targets` but explicitly in `faber`'s default feature
   list. A generic “default-off everywhere” description would be wrong.
3. **Analyzed dispatch:** expose the leaf under its feature and route
   `Target::HirC` to `generate_c_from_analyzed`. That adapter constructs the
   emitter, supplies display tokens, invokes the trait, and wraps the leaf's
   output in the module's `Output::C`. The leaf and module have distinct
   `COutput` carriers; the adapter copies the `code` field, not the carrier type.
   Preserve an unavailable-target error arm in builds without the feature.
4. **Driver family:** inspect `generate_output` in
   [driver/mod.rs](../../crates/radix-module/src/driver/mod.rs). C belongs to the
   HIR group that calls shared analyzed dispatch, not the MIR-lowering group.
   The driver also handles CLI entry and target qualification before ordinary
   dispatch; adding an emitter arm alone does not admit every entry form.
5. **Artifact consumption:** update output payload handling, text extraction,
   extension metadata, and any reflection/binary handling actually needed by
   the output family. C is text; native objects are not text strings.
6. **Product surface:** review CLI target conversion, the product compiled-in
   gate, and reported capabilities. Match reported support to real routes.
   Target selection and an emitter trait implementation do not automatically
   create a package builder or run command.

The C capability row currently reports file check/build support and no
run/package support. [Package planning](../../crates/radix-program/src/artifact_plan.rs)
also rejects C through its unsupported-target branch. Do not advertise package
support until graph linking and artifact assembly genuinely exist.

## 4. Fail closed with structured facts

[Shared dispatch](../../crates/radix-module/src/codegen/mod.rs) rejects poisoned
HIR, applies shared gates, and checks availability. These checks do not replace
leaf-specific subset checks. C's `unsupported` helper returns `CodegenError`
with named arguments:

```text
issue     = c_target_unsupported
construct = the unsupported construct's name
target    = c
```

The [driver](../../crates/radix-module/src/driver/mod.rs) converts emission
failures into diagnostics; [C test support](../../crates/radix-hir-c/src/test_support.rs)
checks `CODEGEN001` plus the issue/target/construct facts and requires no output
on rejection. Tests should not match localized message fragments. Shared gates
or earlier analysis can reject before the C leaf, so not every failed C compile
necessarily carries `c_target_unsupported`.

Target-specific rejection is not permission to approximate accepted constructs.
C must account for unspecified operand evaluation order, integer overflow,
aliasing, trapping, and allocation. Its exact arithmetic and store helpers are
implemented in [the runtime header](../../runtime/c/faber.h); do not replace
them with unchecked signed C arithmetic simply because the target compiles it.
This manual does not establish that every accepted construct is free of defects.

## 5. Separate runtime delivery and external tools from emission

[helpers.rs](../../crates/radix-hir-c/src/helpers.rs) tracks runtime use and
selects `#include "faber.h"` when needed. The header is `static inline` support;
the emitter writes program-specific declarations, functions, and entry code.
[build.rs](../../crates/faber/build.rs) includes `runtime/c` among the embedded
runtime roots; [embedded_runtime.rs](../../crates/faber/src/package/embedded_runtime.rs)
materializes those sources at the product boundary. The leaf does not launch
GCC or Clang.

A downstream C build must provide the header include directory. The runtime
uses compiler extensions such as `__int128`; it is not a promise of support for
every ISO C implementation. Float programs may require linking the host math
library. Emission, host compilation, execution, and sanitizer checks are separate
proofs. External-tool orchestration belongs at the product/harness layer, not
in a reusable compiler leaf.

## 6. Prove the narrow boundary you changed

C has public, focused examples of complementary proof styles:

| Proof | Existing location and what it establishes |
| --- | --- |
| Emitted shape | [emit_test.rs](../../crates/radix-hir-c/src/emit_test.rs), including `scalars_program_is_pinned`, compares generated C with committed text. |
| Named rejection | [reject_test.rs](../../crates/radix-hir-c/src/reject_test.rs) and [test_support.rs](../../crates/radix-hir-c/src/test_support.rs) assert failure identity and absence of output. |
| Semantic hazards in shape | [control_test.rs](../../crates/radix-hir-c/src/control_test.rs) pins break/switch behavior; other companion tests focus on their own constructs. |
| Host runtime behavior | [runtime/c/tests/faber_test.c](../../runtime/c/tests/faber_test.c) exercises the header independently of HIR dispatch. |
| Production-source constraints | [tests/hygiene.rs](../../crates/radix-hir-c/tests/hygiene.rs) checks the leaf's production budgets. |

For an emitted-shape-only edit, a single named Rust test is a useful first
proof, for example `cargo test -p radix-hir-c scalars_program_is_pinned`.
A changed runtime operation additionally needs the relevant runtime case and a
single end-to-end emitted-program comparison, including output, status, and
trap behavior where applicable. A feature-wiring edit needs enabled and disabled
builds and capability assertions. Do not substitute a broad corpus sweep for a
specific missing boundary proof, or count an output snapshot as execution proof.

These are validation recommendations, not claims that builds or tests ran for
this documentation change. Platform/toolchain support beyond inspected code and
focused executable evidence remains unverified.
