# `faber format` on `'la'`-Frontmatter Source Libraries

**Status**: planned — pre-implementation; discovered 2026-08-10 via housekeeping run on norma (mechanical stage)
**Created**: 2026-08-10
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/faber-format-locale-la/`

## Summary

Make `faber format` work on a source-library root that carries
`locale = "la"` frontmatter (norma `src/` and `stdlib-nativum/`) without an
opaque error, so the stdlib can be formatted and a `--check` gate can be
enforced — today the command errors out and the source tree drifts from
canonical form.

## Problem

- `faber format` (default and explicit-path invocations) errors on
  `locale = "la"` frontmatter files in `norma/src/` and `stdlib-nativum/` when
  no stdlib path is configured — an env/root-shape condition that blocks the
  formatter entirely on those trees.
- Consequence: housekeeping's mechanical block found two exempla files already
  out of canonical form (`exempla/ad-multiplica-backward/src/main.fab`,
  `exempla/crypta-sha2/src/main.fab`); the format surface is not enforced, and
  the `'la'` error hides that.
- A library root without a package manifest is not clearly handled: the formatter
  needs either a graceful default or an actionable error.

## Goals

- `faber format` on a source-library root with `'la'` frontmatter either
  succeeds or fails with an actionable message (configured stdlib path /
  explicit fallback) — not an opaque locale error.
- `faber format` (and `--check`) become usable as a CI gate on library roots.
- Canonical form achievable for norma `src/` + `stdlib-nativum/` and kept enforced.

## Non-goals

- The comment-preserving lossless formatter (separate goal: `faber-format-lossless`).
- The `'la'` reader-locale packs themselves (reader packs are a different surface).
- The proba/test-process gaps (separate goal: `proba-companion-tests`).

## Ground Truth Researched

- Housekeeping run `wf_019feb5c7dbc7760b1e6b360f7ca1bb9`, mechanical receipts
  (journal seq 1): "faber format canonicalized 2 exempla files…; `src/` and
  `stdlib-nativum/` files error on locale 'la' frontmatter with no stdlib path
  configured — both default and explicit-path format invocations hit it, skipped
  with note." Committed as `3bbe0e5` in norma.
- `faber/src/cli/mod.rs` (`FormatArgs`): `--locale` flag, `--check` flag,
  default = current package directory.

## Reference Packet

Before editing, inspect:

- `faber/src/cli/mod.rs` (`FormatArgs`): the format command surface.
- The format implementation behind `faber format` (locale/frontmatter handling).
- `norma/src/model.fab` (a `locale = "la"` file): the failing input shape.
- Neighbor goals: `faber-format-lossless`, `faber-format-pretty` (do not overlap).

## Constraints And Invariants

- Formatting must stay behavior-preserving (the mechanical block's rule).
- `--check` semantics: exit 1 if any file would change; must work on library roots.
- No new required env var for basic formatting unless paired with a clear
  fallback path (documented, not guessed).

## Supporting Skills

- `housekeeping` Step 1 (mechanical block): the consumer that runs `faber format`.
- `faber`: grounding on the format surface.

## Implementation Shape

- Phase 1: reproduce and classify — what exactly errors on `'la'` frontmatter
  (locale pack lookup? stdlib path resolution?).
- Phase 2: fix or graceful fallback (e.g. author-mode default for unknown locales,
  or explicit `--locale` override) with an actionable message.
- Phase 3: format + enforce norma `src/` and `stdlib-nativum/` to canonical form.

## Release Posture

Decision: defer release — normal commits; rides the next faber release if the
protocol picks it up.

## Exit Strategy

Decision: not included.

## Acceptance Criteria

- `faber format` on norma root completes (files canonicalized or explicitly skipped
  with reason, exit understood).
- `faber format --check` exits correctly (0 clean / 1 would-change) on the library root.
- No opaque `'la'`-locale error blocks the formatter.
- Housekeeping mechanical run on norma formats `src/` without the env/root-shape note.

## Validation

- Manual: `cd norma && faber format src && faber format --check src`.
- Review check: formatter diff is layout-only; frontmatter preserved.

## Open Questions

- Is the `'la'` failure a missing-locale-pack error (env) or a missing-stdlib-path
  error (config)? The fix differs. Reproduce with `FABER_LIBRARY_HOME` set vs unset.

## Stop Conditions

- Stop if making `'la'` formatable would change frontmatter or source semantics.
- Stop if the fix turns into the lossless-formatter project (that's the neighbor goal).
