# Package acquisition contract (dev kit, story A)

Durable acquisition contract for the Faber dev kit. Story A — Cista is a
separately installed tool; Faber never installs packages. Confirmed
2026-08-12 (faber-radix-recomposition Q1b). Recorded from the Stage 2
contract freeze (S2-U1) and the cista-boundary §6.2 default text. This is
the content source for the public mirrors (Faber onboarding docs, public
landing).

## Contract text (default story A)

> Install Faber from the public archive. The archive contains the `faber`
> product CLI and reference/locale packs. Package acquisition is a separate
> tool: install the `cista` binary from the Cista release channel (Cista has
> no published release yet). Use `cista install` to populate the shared store and
> rewrite `faber.lock`. Then use `faber check|build|run|test` against that
> lock. Faber never installs packages.

## What this contract means

- **Faber never installs packages.** The product CLI has no install
  subcommand; package acquisition is Cista's job.
- **Faber never discovers or mutates the store.** `check` / `build` / `run` /
  `test` operate from an already-materialized valid `faber.lock`.
- **Cista is a separate tool**, obtained from the Cista release channel (it
  has no published release yet). It populates the shared store and rewrites
  `faber.lock`.

## Packaging fact (live)

The dev kit ships `bin/faber` and share packs only. Cista appears as a
release-manifest **source pin**, not a shipped binary. The story-A contract
text makes that packaging explicit so a clean-room user knows how to obtain
Cista.

## Alternative story B (Stage 5 distribution option)

Bundling a `bin/cista` binary next to `bin/faber` in the kit remains an
alternate Stage 5 option. It is a **distribution** option, not a Stage 2
composition change: story B may be promoted at Stage 5 without reopening the
Cargo-edge decision.
