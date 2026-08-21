# Typed error unions — the message-mirror and remap-chain problem

**Status**: design discussion — open (operator-chaired dedicated session)
**Origin**: canonical-faber idiom audit of gradus (Vivi task `be9013ce` → audit
mail `0606c6d6` → receipt memo `b0bb1fb8`, observation O7); the
`typed-error-union` operator TBD in the canonical-faber pattern library
(authority surface `gradus/src/shape.fab`)
**Related rulings same day**: F14 restructure (`9041f01b`), O6 facade split
(`614987f8`), O7 extraction (`640db2ff`) — all Vivi memos, project
`/Users/ianzepp/work/faberlang`

This doc frames the problem and the open design forks. It records no design
decisions; the operator session owns those.

---

## Problem

The settled error idiom — failable operations return `T ⇥ XError` over a
`union` whose variants each carry a `string message`, guarded by
`require … throw variant` — carries two pieces of boilerplate that scale badly:

1. **The message mirror.** Every error union needs a hand-written
   `fn message(XError e) → string` whose every arm extracts the same single
   field:

   ```fab
   case UnknownName const message {
       return message
   }
   case UnknownVersion const message {
       return message
   }
   …
   ```

   The variant identity and the message field state the same fact twice per
   union: once in the variant list, once per mirror arm. Operator 2026-08-21:
   "match arms everywhere that have exactly one message field. It's honestly
   quite ugly."

2. **The remap chain.** Cross-module variant matching is a language constraint
   (`SEM001`/`SEM041`, recorded PML1 and in
   `gradus/docs/api-shape-policy.md`): a caller cannot match another module's
   variants, so `message()` text is the only cross-module error-identity
   surface. Wrappers therefore recover error identity by string comparison —
   `_map_error` chains at `gradus/src/transformer.fab:259`,
   `gradus/src/gradus.fab:160` (relocates to `src/mlp.fab` under the O6
   facade split), and `_map_cached` in `gradus/src/model/dense.fab`. Text
   matching to recover type identity is the fragile half of the problem.

## Evidence census (verified live 2026-08-21)

| Surface | Count |
| --- | --- |
| gradus source files with a hand-written `fn message()` mirror | 33 of 34 modules |
| gradus variant payloads that are a single `string message` field | ~260 |
| string→variant remap chains | 3 (`transformer.fab`, `gradus.fab`→`mlp.fab`, `model/dense.fab`) |
| norma / triga / tela / examples mirrors | 1 message field, 0 mirrors (norma); 0 elsewhere |

The pain is gradus-concentrated today, but the idiom is the pattern-library
authority surface — the design decision is language-level, not
gradus-local. Every new gradus module adds another mirror.

## What is settled (the core stays)

From the pattern-library entry (operator ruling 2026-08-21): the
union / `⇥` / `require … throw variant` core **is** the pattern and is not
in question. The redesign targets the per-variant `string message` +
hand-written `message()` mirror and the text-matching remap chains — the
mirror boilerplate is explicitly "not required" by the pattern's own entry.

## Design forks to resolve

Not mutually exclusive; (1)+(3) is plausibly the real shape.

1. **Compiler-side derive.** The accessor is generated (or variant identity
   renders natively) and ~33 hand-written mirrors are deleted from gradus.
   Needs a Radix surface decision; nothing changes at the Faber source level
   for library authors beyond deleting the mirrors.
2. **Convention-only.** Library-side restructure (single-message union
   shapes, a shared message-carrying convention). Smallest blast radius, but
   expressibility is bounded by the language: if `message()` remains the only
   cross-module error surface, a mirror of some form remains required.
3. **Typed wrapping / chaining.** A typed error-composition surface across
   module boundaries — kills the string-matching remap chains (the fragile
   half). May require relaxing or working within the `SEM001`/`SEM041`
   cross-module variant law, or an error-carrier abstraction that carries
   identity without variant matching.

## Constraints and adjacent surfaces

- Gradus is pre-1.0: whatever ships lands as a clean break
  (`gradus/docs/compatibility-policy.md` v1.2.2; no shims).
- The cross-module variant law (`SEM001`/`SEM041`) is a recorded language
  constraint, not a gradus choice — any fork that crosses it is a Radix
  change.
- **Fold-don't-churn stands until this design lands**: no mirror or
  remap-chain edits ride the gradus cleanup train (U1–U13) or the
  F14/O6-spawned units (U14 block records, U15 facade split).
- Adjacent but distinct — do not fuse: `radix/docs/factory/diagnostic-identity-and-errors/`
  governs the compiler's own Rust error enums and diagnostics. Same theme
  family, different artifact and repo.

## Open questions (seed list for the session)

- Does the message mirror become compiler-generated, or does the variant
  shape change so no mirror is needed?
- Should cross-module error identity travel as text at all? If not, what
  replaces `message()` as the boundary surface given `SEM001`/`SEM041`?
- Is the single-`string message` payload the right shape for every variant,
  or do some variants want structured payloads once the mirror problem is
  solved?
- What happens to the ~260 existing throw sites and 33 mirrors — one wave,
  or derive-first (mirrors delete mechanically) then remap chains?
- Does the answer generalize to norma/triga/tela before they grow mirrors,
  and is the pattern-library entry's wording the post-design authority?

## Provenance

```bash
vivi mail show 0606c6d6 --project /Users/ianzepp/work/faberlang   # the audit (O7 block, cleanup plan)
vivi memo show b0bb1fb8 --project /Users/ianzepp/work/faberlang   # receipt: F14/O6/O7 parked
vivi memo show 640db2ff --project /Users/ianzepp/work/faberlang   # this ruling: extract to design doc
rg -c 'fn message\(' gradus/src --glob '*.fab'                   # the 33 mirrors
rg -n 'fn _map_error|fn _map_cached' gradus/src --glob '*.fab'   # the remap chains
```
