# crypta-sha2 pretty-v1 grammar-guard investigation

**Date**: 2026-08-11
**Unit**: `format-pretty-crypta-sha2-grammar-guard` (task 3a108594)
**Source want**: 0d06124e
**Path**: `norma/exempla/crypta-sha2/src/main.fab`
**Disposition**: **document intentional behavior-flag + keep excluded**

## Repro

```text
faber format --policy pretty-v1 --check norma/exempla/crypta-sha2/src/main.fab
# stderr: error: …: pretty-v1 cannot preserve this construct confidently; file left unchanged
# exit 0 under --check when the body is returned byte-identical (no drift)
```

Binary used for repro: `faber 1.6.0` (`/Users/ianzepp/.cargo/bin/faber`).

## Exact construct

**One hit** of `GuardKind::ErgoBlockNesting` at the compact `ergo` arm whose
body is a brace block:

```text
norma/exempla/crypta-sha2/src/main.fab:85
        si failed.nonvacua() ergo {
            varia textus msg ← "crypta-sha2: algorithm rejection missed these names: "
            itera ex failed fixum name {
                msg ← (msg + name) + " "
            }
            iace msg
        }
```

AST shape: `IfBody::Ergo(Stmt { kind: StmtKind::Block(...) })`.

The guard pre-pass (`radix/crates/radix/src/forma/pretty/guard.rs`) flags any
compact `ergo` body whose statement owns braces (`is_block_owning_stmt`
includes `StmtKind::Block`, `Si`, `Fac`, `Dum`, `Itera`, …). This file has
exactly that form once.

## Ruled out (S6 inventory guess)

The S6 inventory residual named "incipit blocks, byte literals
(`|61 62 63|`), underscore-prefixed function names" as suspects. Probes:

| Source shape | pretty-v1 --check |
| --- | --- |
| `incipit` + octeti literals + `_repete` + simple `si … ergo iace …` | clean (no guard) |
| `si failed.nonvacua() ergo { iace "x" }` | **FORMAT001 / ErgoBlockNesting** |
| `si failed.nonvacua() { iace "x" }` (bare block, no `ergo`) | clean |

No other file under `norma/exempla/` currently uses `ergo {`.

## Why intentional (not a false positive)

`pretty-v1` keeps compact one-statement `ergo` arms when they fit and, when
the arm exceeds the soft width, **expands by dropping `ergo` and wrapping in
a plain block** (`write_compact_ergo` in `forma/pretty/emit.rs`; covered by
`ergo_arm_expands_to_block_when_the_arm_exceeds_the_soft_width`).

For an arm that is **already** a block-owning statement:

- Compact trial would re-emit `ergo { … }` only if the emitter path is
  faithful for nested block layout.
- Width expansion would restructure into a bare block (and can double-nest if
  the body is itself a `Block`).

That is exactly the S3 contract: *a partial rewrite is worse than an
unformatted file*. The guard refuses the whole file rather than guess. Simple
`ergo` one-liners in the same file (lines 15, 19) are confident and would
format; only the nested-block arm blocks the file.

## Disposition

| Option | Decision |
| --- | --- |
| Fix grammar-guard in radix | **No** — true positive; guard matches design |
| Document intentional + keep excluded | **Yes** — this note; remain S7-excluded |
| Rebaseline-safe after fix | **No** — no false-positive fix; silent rebaseline while the guard fires is forbidden |

**Keep excluded** from mechanical pretty-v1 rebaseline (S7) and from any
default-flip expectation that every exemplum is `--check`-clean under
pretty-v1 without package adoption.

## Package adoption path (not this unit)

When `norma/exempla/crypta-sha2` deliberately adopts pretty-v1, rewrite the
one arm to a bare block (semantically equivalent control form):

```text
si failed.nonvacua() {
    …
}
```

That is a deliberate source edit under package adoption, not a silent S7
rebaseline of a guard-blocked behavior-flag file.

## Residuals (out of scope)

1. **Diagnostic args not shown on the CLI.** `compile_pretty_author` already
   attaches `issue=ergo_body_nested_block` and `location=line N` as
   `DiagnosticArg`s; `faber format` only prints `diag.message`, so operators
   see the generic FORMAT001 text without the line. Improve print surface
   later if desired.
2. **`--check` exit 0 on guard-only hits.** Guard returns
   `output = Some(unchanged body)` plus an error diagnostic. CLI still returns
   `Ok(formatted)` when output is present, so `--check` sees no drift and
   exits 0 while printing `error:`. Guard module docs claim `--check` fails;
   product only fails on drift or missing output. Separate product decision
   whether FORMAT001 should force nonzero exit even when the body is unchanged.
3. **Optional future engine work.** Teach `write_compact_ergo` a dedicated
   preserve path for `ergo` + already-block bodies (never drop `ergo`, never
   double-nest), then narrow the guard. Not required for disposition; only
   if product wants this author form formattable under pretty-v1.
