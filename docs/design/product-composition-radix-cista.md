# Product composition: Faber, Radix, and Cista

**Status:** DECISION — operator law recorded 2026-07-17
**Scope:** product architecture and install/package UX direction; no code migration
in this document.

## Decision

Faber is the user product composed from Radix compiler capability plus Cista
package-store capability. The `faber` CLI may depend on the `cista` crate in the
same way it may depend on `radix`: an in-process crate dependency is an allowed
implementation path when it gives the product one canonical command surface.

The older long-term rule that `faber` and `cista` must stay separated by
spawn-only integration is retired. Stable file formats, process boundaries, and
independent buildability are still useful engineering tools, but they are no
longer product law and must not block `faber install` from becoming the product
facade over the Cista store.

## Product invariant

A developer should install Faber and use `faber` for the normal project loop:
create, check, build, run, test, format, and install library packages. Radix and
Cista remain specialized technology tools (and usable libraries/CLIs on their
own), but **Faber is the product-level combination** of those tools into one
day-to-day interface.

### Composition pattern

Faber is the **one allowed product seam** where separate crates come together:

| Crate / tool | Kind | Role |
| --- | --- | --- |
| **Radix** | Technology (compiler library + CLI) | Language analysis, MIR, targets, emit |
| **Cista** | Technology (package-store library + CLI) | Store, install, resolve, registry client |
| **Faber** | **Product** (build tool + user CLI) | Cohesive day-to-day surface; may link both |

This is the same pattern as “the product binary depends on the compiler crate.”
It does **not** require merging repositories, deleting the `cista` or `radix`
CLIs, or putting package-store semantics inside Radix. Cista remains the library
of record for store behavior; Radix remains the library of record for language
behavior; Faber is where agents and humans meet a single product.

## Install North Star

`faber install` becomes the product facade over the Cista package store:

- installs Faber library packages from git, GitHub, or a registry;
- resolves the installed package set for `faber check`, `build`, `run`, and
  `test`;
- treats Norma as the platform default package available to every Faber project;
- leaves Triga as an optional third-party package rather than a platform
  default;
- keeps `FABER_LIBRARY_HOME` out of install; it may remain only as an explicit
  resolve override for local monorepo development, not as a package-store model
  or install destination.

This decision does not require one implementation shape. Direct crate calls,
small shared modules, process calls, or file contracts are choices to evaluate
per migration unit. The forbidden shape is preserving two competing product laws:
"Faber installs git libraries" versus "Cista installs packages" as permanent
parallel user paths.

## Milestones

- **M0 — Store loop:** landed the Cista shared store, lockfile, registry/cache,
  package validation loop, and product-facing install API as the source of
  package truth. Cista API export SHA: `693dc7a`.
- **M1.0 — Path install:** landed `faber install --path` as an in-process Cista
  store install with project lock rewrite; Cista API export `693dc7a`, Faber
  facade/test `09f3443`.
- **M1.1 — Git/URL install:** landed default `faber install <git-url>` as a
  temporary clone plus required `cista.toml` install into the Cista store with
  project lock rewrite. Faber SHA: `16bb59c`.
- **M1.2 — Registry pin + Triga dogfood:** landed `faber install name@version`
  as the product facade over Cista registry/cache install with `--registry` /
  `CISTA_REGISTRY` selection and no fallback from bare names to GitHub clones.
  Dogfood proof installed `../triga` into a temp Cista store, rewrote a consumer
  `faber.lock` with the Triga interface root, and checked `importa ex
  "triga:triga"` green from the lock path. Faber SHA: `7329a80`.
- **M1 — Product install:** landed for path, Git/URL, and registry pins: the
  Faber product facade now routes package installs through the Cista store and
  rewrites the project lock instead of maintaining a parallel installer. Faber
  SHAs: `09f3443`, `16bb59c`, `7329a80`.
- **M2 — Cold agent + legacy install removal:** landed. Store-only resolve proof
  is covered by `scripta/check-store-only-resolve.sh`: a temp consumer declares
  `norma` and `triga`, installs both via `faber install --path` into a temp
  Cista store, then runs `faber check --package` with `FABER_LIBRARY_HOME` and
  `FABER_ENABLE_WORKSPACE_LIBRARY_PROBE` unset. Since the monorepo sibling probe
  is opt-in, this proves lock/interface roots carry dependency resolution for
  the slice. The old `--legacy-library-home` / `FABER_LIBRARY_HOME` install path
  is removed; `FABER_LIBRARY_HOME` survives only as an explicit resolver
  override when set by local development workflows. Faber SHAs: `48ff510`,
  `bf4cf44`, `11ee45f`.

Browser-game and web-hosting flows are later work after MIR/host foundations;
they are not part of this composition decision.

## Supersedes

This document supersedes the long-term repo-separation invariant in
`../../../cista/docs/factory/cista-package-store/goal.md` and the previous
analysis in `cli-surface-vs-radix-cista.md` that treated no crate dependency /
spawn-only integration as architecture law.
