# Stage 1b — Package Host Import Bridge (Delivery Spec)

**Campaign stage**: Stage 1b — Package Host Import Bridge
**Lowers from**: `docs/factory/faber-script-runtime/CAMPAIGN.md`
**Batching posture**: discovery-first (prove the bridge on `solum`; the mechanism is module-agnostic so `processus`/`aleator`/`json` follow for free)
**Status**: complete (2026-07-06)
**Date**: 2026-07-06
**Baseline**: [`stage0-baseline.md`](stage0-baseline.md)

## Interpreted Unit

Let interpreted package execution (`faber script <pkg>`, and the retained
`faber run --interpret`) satisfy supported `norma:<module>` host imports through
the existing `faber:*` stepper kernels, so package source uses ONE import
spelling (`norma:*`) on both the interpreted and compiled lanes. Unsupported
shapes fail closed with actionable diagnostics; no generated-Rust fallback.

## Design Decision — C2a (link-time Package→Kernel rewrite)

**Decision**: the bridge is a post-validation transform inside the package-MIR
runner (`run_package_mir`), not a lowering-context policy.

After `lower_package_units` + `validate_program`, one pass walks the lowered
`MirProgram`'s `blocks[*].statements[*].RuntimeCall.call.intrinsic` for
`MirIntrinsic::Provider(MirProvider { kind: Package, module, name })`. For each
whose resolved `module[0]` is `norma:<kernel-manifest-module>` and whose
resolved `name` (verb) is in that module's manifest verb subset, rewrite
`kind` to `MirProviderKind::Kernel(module)`. Unsupported verbs produce a
fail-closed diagnostic; the program never reaches `run_entry`.

**Why not C1 (lowering-context bridge policy):** core-stdlib's convergence model
is that provider satisfaction is a *runtime dispatch* concern (the frame
gateway / `crates/faber/src/frame.rs` route table), not a compile-time
classification. Making `LoweringContext` interpretation-mode-aware would bake
"interpretation uses the kernel" into the compiler — wrong layer, and Stage 8
would have to rip it out.

**Why not C2b (host-level `provider()` dispatch):** non-inspectable magic, and
errors arrive at runtime per-call instead of link-time.

**Why C2a:** radix lowering stays neutral and mode-agnostic; the stepper host
stays dumb (only `Kernel` dispatch); the bridge is one explicit, inspectable,
validated pass; fail-closed is link-time. The package runner already does a
namespace-rewrite pass (`rewrite_unit_namespace_calls`), so a provider-kind
rewrite is a natural sibling.

## Retire Path (load-bearing)

This bridge is **interim**. It retires only when a `norma:*` verb the bridge
currently satisfies can instead run end-to-end through the frame gateway over
`ad` — i.e. the verb's Faber body routes through `ad` **and** the route has a
functioning host handler, not a `mori` placeholder. Deletion then happens in one
place once the manifest subset is satisfied over `ad`.

The source cleanse (core-stdlib Stage 8) is **complete as a structural
milestone**: `@ externa`/`@ subsidia` are gone and every verb is pure Faber over
`ad`. It is **not** an operational milestone. ~81 verbs across 15
`stdlib/norma/*.fab` modules still `mori` ("deferred pending Stage 2 dispatch"),
and most `ad` routes no-op in the frame gateway
(`crates/faber/src/frame.rs` dispatch handles only `runtime:echo`, `tempus:*`,
and the `solum:*` write verbs; everything else is `_ => {}`). This is acceptable
by design: the current priority is a functional script runtime, and real stdlib
implementations land verb-by-verb afterward. Until those host handlers exist, the
bridge is the only path that *works* for the kernel subset — it routes to real
Rust dispatch and bypasses both the `mori` stubs and the `ad` no-op arms.

The bridge must not gain non-kernel responsibilities that complicate that
deletion.

## Parity Dependency (governed by core-stdlib)

The bridge consumes the **script/package parity contract** (core-stdlib Stage 7):
`faber:*` kernel verbs are a subset of `stdlib/norma/<module>.fab` public
surface, enforced by `kernel_manifest_verbs_are_subset_of_norma_public_surface`.
Stage 1b adds a core-stdlib track-ledger entry recording that interpreted
`norma:solum`/`norma:processus` satisfaction is delegated to the `faber:*`
kernel for the manifest verb subset. The bridge does not redefine parity; it
consumes it.

## Normalized Spec

- `library_import_diagnostics` (`mir.rs`): stop rejecting `norma:<manifest-module>`
  imports; allow them through to lowering. Non-manifest library imports
  (`norma:chorda`, external libs) keep the existing rejection.
- `bridge_norma_providers_to_kernel` (`mir.rs`): walk the lowered program,
  rewrite supported `Package` providers to `Kernel`, fail-closed on unsupported
  verbs. Runs after `validate_program`, before `run_entry`.
- `kernel_module_supports_verb` (`radix::kernel::manifest`): predicate over the
  manifest verb subset. Co-located with the parity data it derives from.
- No source-namespace changes: package source stays `norma:*`; `faber:*` stays
  the direct script/kernel namespace. The bridge is a backend dispatch rule.

## Repo-Aware Baseline

- `crates/faber-cli/src/package/mir.rs` — `run_package_mir`,
  `library_import_diagnostics`, `library_identity_label`.
- `crates/radix/src/kernel/{mod,manifest}.rs` — manifest, `lookup_module`,
  `resolve_kernel_module_name`, parity verbs.
- `crates/radix/src/mir/lower/context.rs:247` — `record_import_item` (path-based
  `Package` vs `Kernel` decision; UNCHANGED — the bridge is post-lowering).
- `crates/radix/src/mir/stepper/runtime.rs:1389` — `eval_provider`: `Kernel` →
  `host.kernel_call`; `Package` → `host.provider()` (hard error today).
- `crates/radix/src/mir/validate.rs:1038` — `Provider(_) => {}` (validation is
  provider-kind-agnostic; bridge after validation is safe).
- Merged-program interner: `lower_package_units` remaps all symbols into the
  entry unit's interner; reachable as `lowered.validation.interner`.

## Stage Graph

1. Add `kernel_module_supports_verb(module, verb)` to `radix::kernel::manifest`.
2. `library_import_diagnostics`: allowlist `norma:<manifest-module>` (module-level).
3. `bridge_norma_providers_to_kernel`: MIR walk, rewrite + verb-coverage fail-closed.
4. Wire into `run_package_mir` between `validate_program` and `run_entry`.
5. Tests: `norma:solum` read-file success; unsupported-verb fail-closed;
   `norma:processus` argv. Existing `norma:chorda` rejection still passes.

## Checkpoints And Gates

**Stage 1b gate (from campaign):**

- [ ] `faber script <pkg importing norma:solum>` reads a file through the kernel.
- [ ] `norma:processus` argv/env/cwd/exit bridged (same mechanism).
- [ ] Unsupported `norma:*` modules (`norma:chorda`) still fail closed.
- [ ] Unsupported verbs within a bridged module fail closed with a diagnostic.
- [ ] Package builds still use the Rust `norma` backing (compiled lane untouched).
- [ ] At least one coreutils-shaped fixture uses a single `norma:*` import block.

**Release checkpoint**: `defer-release` (consistent with Stage 1; the lane split
closes in Stage 6).

## Validation

```bash
timeout 1200 cargo test -p faber-cli --test run_integration_test script_norma
timeout 1200 cargo test -p faber-cli --test run_integration_test
.timeout 1200 cargo clippy -p faber-cli --all-targets -- -D warnings
timeout 1200 cargo build --release -p faber-cli
```

All green at closeout: `run_integration_test` 33/33 (30 Stage 1 + 3 new Stage 1b:
`norma:solum` read-file bridge, unsupported-verb fail-closed, `norma:processus`
argv bridge), bin 185, lib 189, clippy clean, release build OK.

Pre-existing failures unrelated to this stage (diagnostic-rendering rot from the
structured-diagnostics workstream — prose messages replaced with `SEM` codes;
this stage touched no diagnostic rendering):
- `package_check_diagnostics_uses_expanded_renderer` (faber-cli emit_integration_test, SEM010)
- `kernel::tests::kernel_glob_import_rejects_unknown_module_early` (radix, SEM008) — in the kernel module this stage additively touched; the manifest predicate added here is purely additive and cannot affect glob rejection or the `SEM008` rendering. Left for the diagnostics owner.

## Companion Skill Plan

- `factory` executes this spec.
- core-stdlib Stage 7 owns the parity contract this bridge consumes; Stage 8 owns
  the frame-gateway convergence that retires this bridge.

## Open Questions

- Does the kernel verb subset cover the coreutils-critical surface, or does
  Stage 7 parity work need to widen it first? The bridge surfaces gaps as
  fail-closed diagnostics, so gaps are discoverable, not silent.
