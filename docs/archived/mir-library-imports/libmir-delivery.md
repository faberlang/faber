# Delivery: LIB-MIR — MIR library-import execution

**Goal**: `docs/archived/mir-library-imports/goal.md` (LIB-MIR gate)
**Status**: in factory — unit lowered 2026-08-01
**Repo**: faber (`src/package/mir.rs`, `src/package/library.rs`)

## Interpreted Unit

Make `faber run -t fmir` execute packages that import library modules
(`gradus:*`, and non-bridged `norma:*`), reusing the local-unit linking
mechanism already present in the MIR package path. Reproduced during warmup:

```
error: package MIR does not yet support library imports such as `gradus:gradient`; use compiled package execution for this surface
```

## Normalized Spec

1. `library_import_diagnostics` (`faber/src/package/mir.rs:606/:1438`) stops
   being a blanket rejection for the Interpreted consumer. It becomes a
   resolution/linkability gate: resolvable library imports pass, genuinely
   unsupported cases (unresolvable provider, unlinkable target) keep the
   `package_mir_library_imports_unsupported` diagnostic identity.
2. `local_namespace_call_targets` (`mir.rs:3129`) handles
   `ImportResolution::Library(_)`: load the library module's analyzed program
   (reuse `library_cached_analysis` / the library resolver), allocate
   synthetic `DefId`s (`PACKAGE_MIR_SYNTHETIC_DEF_BASE`) for exported library
   functions, and record `targets`/`namespaces`/`sources` exactly like local
   sibling units. Export set comes from the library file interface (after
   SEM004, includes companions).
3. `lower_package_units` (`mir.rs:3841`) lowers the library units alongside
   package units (library units are function-only; no entry). Symbol/text and
   semantic-type remapping reuse the existing per-unit path
   (`remap_program_text_symbols`, `import_lowered_semantic_types`,
   `rewrite_program_sources`).
4. Companion calls: the linker must also map companion names to the library's
   companion `DefId`s (`analysis.radix_lanes.iter_backward()`), since
   companions have no HIR item — extend `exported_top_level_functions`-style
   lookup with a radix-lanes pass.
5. `is_bridged_norma_module` / `is_bridged_norma_import_path` carve-out stays:
   kernel-manifest `norma:*` keeps using the stepper kernel bridge; bridged
   imports are not re-linked.

## Repo-Aware Baseline

- Pipeline: `with_prepared_package_mir_with_cli_mode_and_consumer`
  (`mir.rs:592`); gate at :606; namespace linking at :617-619; lowering at
  :620; norma kernel bridge at :634. Only `PackageMirConsumer::Interpreted`
  is gated (`ExternalTarget` builds already tolerate library imports).
- Linking model: synthetic def-ids (`mir.rs:67`), `PackageMirLinks`
  (`:282`), `local_namespace_call_targets` (`:3129`), `rewrite_unit_namespace_calls`
  (`:3240`), `lower_package_units` (`:3841`).
- Library analysis: `faber/src/package/library.rs` — `library_cached_analysis`
  (:380), `library_cached_file_interface` (:357), `library_imported_function_params`
  (:488, compiled-package reuse surface).
- Resolution: `resolve_import` → `ImportResolution::{Local, Library}`.
- Repro: `gradus/exempla/gradient-seam/` (mirror-free after SEM004) →
  `faber run -t fmir` must exit 0 and FD-match.

## Stage Graph

1. **Link library units** — extend `local_namespace_call_targets` to resolve
   `ImportResolution::Library` into synthetic targets + source rewrites, using
   the library resolver/cache; relax `library_import_diagnostics` to a
   linkability gate. Exit: `gradient.simple_loss` call links and runs.
2. **Companion linking** — extend the linker's export lookup with radix-lanes
   companions (depends on SEM004 export wiring for the consumer typecheck;
   MIR-side needs companion `DefId`s from the library's analysis). Exit:
   `gradient.loss_backward` runs in the consumer.
3. **Fail-closed + norma regression** — unresolvable library imports keep the
   same diagnostic identity; `norma:solum`-style bridged packages still run via
   the kernel bridge. Exit: existing fmir exempla and the two rejection tests
   still green.
4. **Goal update** — status line + acceptance record.

## Implementation Work

- One workstream, sequential stages (single repo, shared write surface in
  `mir.rs`). No safe parallelism within the unit.
- Depends on SEM004 (consumer typecheck of the companion) for the full seam;
  can land in parallel for the non-companion half.

## Checkpoints And Gates

- Gate: faber `cargo nextest run -p faber --lib`, `cargo test --test hygiene`;
  radix ladder `--check`; cross-repo consumer proof
  `faber run -t fmir gradus/exempla/gradient-seam/` FD-matches.
- Release: no bump (CLI behavior change is gated behind the fmir run path).
- Batching / Split Decision: split on the companion-linking dependency (Stage
  2 waits on SEM004); stages 1+3 first, stage 2 after SEM004 wiring lands.

## Validation

```bash
cd faber && cargo nextest run -p faber --lib && cargo test --test hygiene
cd ../radix && ./scripta/test --check
../faber/target/debug/faber run -t fmir ../gradus/exempla/gradient-seam/
```

## Companion Skill Plan

`faber` skill (MIR package path, fmir targets); `check-work` for final
verification.

## Open Questions

- How `lower_unit` consumes library units (signature shape) — resolved during
  Stage 1 by reading `lower_unit` and adapting `lower_package_units`.
- Whether library modules with their own imports (gradus:tensor from
  gradus:gradient) need recursive linking — gradus modules import sibling
  modules; likely required for the full seam; fold into Stage 1 if the repro
  demands it.

## Completion (2026-08-01)

All stages done. Delivered: library imports resolve, link, and execute in the
Interpreted fmir path — `local_namespace_call_targets` builds synthetic targets
for library exports (incl. `@ radix backward` companions), `lower_package_units`
lowers linked libraries through the cache, and all library functions get
synthetic sources (separate def-id space). Fail-closed identity preserved for
unlinkable imports; bridged-norma kernel path untouched; ExternalTarget builds
keep the previous silent-skip. Consumer proof
`faber run -t fmir gradus/exempla/gradient-seam/` FD-matches (~1e-11).
Nested library function calls are now linked through the same identity-scoped
synthetic-source graph: dependencies are resolved to a deduplicated module
closure, each caller namespace is rewritten before lowering, and unresolved
dependencies keep the existing fail-closed diagnostic. Cross-library nominal
type unification across separate analyses remains out of scope.

### Audit follow-up (2026-08-01)

Independent post-land audit (general-purpose subagent) found and verified two
real defects, both fixed in `983d6c7` with regression tests:
- sub-companion symbol remap panic (remap now sources from the post-lowering
  interner);
- sub-companion synthetic-source collision across library analyses
  (`extend_unmapped_library_sources` skips already-rewritten values).
Deferred findings recorded: non-function library members (structs/consts)
are not linked (fail closed); every library function is lowered whether used
or not; companion export is fail-open if the resolver symbol is missing.
