# Goal: MIR Library-Import Execution (LIB-MIR)

**Status**: implemented — `faber run -t fmir` links and executes library imports (`gradus:*` and non-bridged `norma:*`); consumer proof FD-matches; bridged-norma kernel path and fail-closed identity preserved (2026-08-01)

**Created**: 2026-08-01
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/mir-library-imports/`
**Lowers from**: Gradus [`SCOPE.md`](../../../gradus/docs/factory/gradus-ml-foundation/SCOPE.md) gate register + [`train-seam-decision.md`](../../../gradus/docs/factory/gradus-ml-foundation/train-seam-decision.md) (Option C, operator decision 2026-08-01)
**Companion goal**: sibling [`radix/docs/factory/gradus-consumer-seam/goal.md`](../../../radix/docs/factory/gradus-consumer-seam/goal.md) (SEM004 + SEM010 gates)
**Process**: `$factory` per [AGENTS.md](../../AGENTS.md)

## Summary

Gradus is a new Faber autograd/ML library (`gradus:*` imports) whose Horizon 0
checkpoint proved the gradient wrapper seam compiles (`faber check` passes for
a consumer importing `gradus:gradient`) but discovered the **run path cannot
execute a package that imports a library module**: `faber run -t fmir` fails
closed with

```
error: package MIR does not yet support library imports such as `gradus:gradient`; use compiled package execution for this surface
```

This goal makes the MIR package run path accept library imports so a Gradus
consumer (and any future `gradus:*` / `norma:*` library-importing package) can
execute through `faber run -t fmir` — the LIB-MIR gate.

## Evidence (Gradus U1 fixture, 2026-08-01)

| Fact | Evidence |
| --- | --- |
| `faber check` resolves library imports | `faber check gradus/exempla/gradient-seam/` → `ok:` (import DAG valid) |
| `faber run -t fmir` rejects library imports | `src/package/mir.rs:1456`, `:3161` — fail-closed diagnostic `package_mir_library_imports_unsupported`; repro: `faber run -t fmir gradus/exempla/gradient-seam/` exits 1 |
| Compiled package execution exists | Error text itself: "use compiled package execution for this surface"; sibling radix AGENTS.md application-lane (HIR → Rust → Cargo binary) supports library imports today |
| Execution proof (self-contained) | `gradus/exempla/gradient-seam-nolib/` runs and FD-matches (~1e-11) — the compiler's gradient output is correct; only the library-import run path is missing |

## Scope

In scope (this goal):

- `faber run -t fmir` (and the fmir build/run package path) accepts packages
  that import library modules (`gradus:*`, `norma:*`, and any provider the
  resolver already supports at check time).
- Reuse the existing compiled-package execution surface where it already
  handles library imports; do not invent a parallel resolution mechanism.
- Fail-closed behavior preserved for genuinely unsupported cases (e.g. an
  unresolvable provider or a target that cannot link library bindings), with
  `code` + `issue` diagnostic identity.

Out of scope (this goal):

- Companion export across `importa` (SEM004) and tensor-returning calls in
  loops (SEM010) — sibling
  [`radix/docs/factory/gradus-consumer-seam/goal.md`](../../../radix/docs/factory/gradus-consumer-seam/goal.md).
- Gradus library surface (`loss/mse`, `optimize/sgd`, `train`) — Gradus
  Horizon 1–2 delivery, gated on this fix.
- Runtime autograd tape (`faber-runtime`) — test-only oracle, never extended.
- GPU gradient path, distribution (cista), checkpointing — separate gates.

## Acceptance criteria

1. `faber run -t fmir gradus/exempla/gradient-seam/` exits 0 and produces the
   `nota` FD-match output (the Gradus U1 consumer executes through the
   library-importing path — no `package_mir_library_imports_unsupported`
   error).
2. A `norma:*`-importing package (or existing exemplum) still runs via
   `faber run -t fmir`; no regression on the current bridged-norma path
   (`is_bridged_norma_module`).
3. Unsupported cases still fail closed with the same diagnostic identity, not a
   new/ad-hoc message.
4. Gradus train-seam re-evaluation trigger fires: a consumer importing
   `gradus:gradient` can `faber run` without error.

## Validation

Faber ladder (radix ladder is the workspace gate):

```bash
cargo nextest run -p faber --lib
cargo test --test hygiene            # no new .expect/.unwrap/panic in src
cd ../radix && ./scripta/test --check   # or --stage 1-4 for the touched surface
```

Plus the cross-repo consumer proof:

```bash
cd /Users/ianzepp/work/faberlang
faber run -t fmir gradus/exempla/gradient-seam/
```

## Sequencing

LIB-MIR is the first gate in the Gradus train-seam sequence (LIB-MIR → SEM004 →
SEM010): it gates whether a consumer can run at all. It is independent of the
two Radix gates (different repos, different surfaces) and can land in parallel
with them; the full reusable `gradus:train` contract waits on all three.

## Non-goals (stop conditions)

- Do not build the Gradus library or the reusable train loop here.
- Do not extend the runtime tape or add a compatibility facade.
- Do not weaken the fail-closed behavior to force green; unsupported cases
  keep their diagnostic identity.
- No PyTorch parity claim; no GPU/device work; no distribution (cista) work.
