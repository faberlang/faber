# Faber Example Corpus

Use this file to navigate exempla. Do not treat it as a hand-maintained file list.

## Canonical Index

Language corpus (keyword / type / operator dictionary):

- `corpus`

Term → path mapping lives in exempla frontmatter: the file that owns an
explain term lists it under `explains = [...]`. There is no index file and
nothing to regenerate; the `faber` build embeds those files for `faber explain`.

Count files and terms from the tree when you need them; do not cache those numbers in this skill.

`faber explain` lists and looks terms up by the active reader locale's spelling
(`--locale <locale>`, else the current package's `faber.toml` `[locale]`, else
`en`): `fn`, `match`, `print` in en; `functio`, `discerne`, `nota` in la. The
spelling comes from the locale pack, so a renamed row renames the explain term
with it. Corpus frontmatter still `explains` the Latin identity; that is the
key the pack maps. Two entries that share one spelling in a pack are listed as
`spelling` and `spelling (category)`. A miss on an identity names the spelling
to ask for.

Package fixtures and format goldens (`faber.toml` packages such as `regio/`,
`contract-import/`, `proba-canary/`; `format/*.fab` with `*.format.expected`):

- `corpus-product`

Package fixtures live in `corpus-product/`; there is no `faber/corpus/` home
for them.

Norma exempla live in `lib/norma/exempla/`; libraries outside this repo keep
their own exempla beside their source. Run `faber check` on one before
trusting it; library modules get renamed.

Locale surfaces: `locale/<id>/exemplars/salve-munde.<id>.fab` (the pack's
`[llm]` exemplar, one per shipped pack) and the per-locale packages under
`corpus-product/reader-locale/<id>/` (`src/` programs plus `faults/` reject
cases).

## Layout Rule

How `corpus/` is laid out:

- Every language entry is a single file that the compiler can check on its own.
- No `faber.toml` and no multi-file packages in `corpus/`.
- Top-level directories cover keywords, operator groups, and language type surfaces.
- Operators stay grouped by logical family (`operatores/`, `assignatio/`, `conversio/`, …).
- Compiler-owned method/type surfaces live in directories such as `lista/`, `tabula/`, `tensor/`, and `intrinseca/`.

Each keyword directory usually has:

- `<keyword>.fab` — primary smoke (required)
- optional focused variants

## Metadata Conventions

Every corpus `.fab` begins with `+++` TOML frontmatter (the repo's own
checks enforce it). Keys seen: `term`,
`kind`, `category`, `grammar` (EBNF rule names), `canonical`, `summary`,
`syntax`, `aliases`, `related`, `locale`, plus package keys. The `locale` key
selects the reader pack for that file: most exempla are `en`, a minority are
`la`.

Below that, most files carry a `#` comment block: `What this teaches`,
`Common mistakes`, `See also`. Some also carry `GRAMMAR:`, `EXPECTED OUTPUT:`
and `BACKEND:` lines. These are for humans and `faber explain`; they do not
replace `EBNF.md`.

Not every exemplar shows current style: some exist to cover a migration or a
rejected form. Check one with `faber check`, and prefer a same-locale exemplar
from the keyword's own directory.

## How To Find An Example

1. Search `corpus/` frontmatter for the file whose `explains` lists the keyword.
2. Open that `.fab` file; files sharing its `term` are further examples.
3. If the term is user-facing, try `faber explain <term>` in the spelling of
   the code locale you are reading (`faber explain fn` in en).
4. For legacy spellings, check `corpus/legacy-redirects.toml`.
5. For package-shaped proofs, search `corpus-product/`.

Example lookup (the key is the Latin identity; `discerne` is `match` in en):

```sh
rg -n '^explains = .*"discerne"' corpus
```

## High-Value Entry Points

Paths are under `corpus/` unless noted.

| Need | Start here |
| ---- | ---------- |
| first program | `incipit/salve-munde.fab` |
| CLI framework tour | `cli/cli.fab` |
| function declarations | `functio/functio.fab` |
| optional params | `optionalis/optionalis.fab` |
| classes | `genus/genus.fab` |
| interfaces | `implendum/implendum.fab` |
| tagged unions | `discretio/discretio.fab` |
| pattern match | `discerne/discerne.fab` |
| switch-like dispatch | `elige/elige.fab` |
| imports | `importa/importa.fab` |
| inferred locals | `sit/` |
| null value / null type, nullable unions | `nihil/nihil.fab`, `est/est.fab`; grammar in `EBNF.md` |
| `optional` slots (la `sponte`) | `sponte/sponte.fab` |
| output | `nota/`, `mone/`, `vide/` (the `scribe` term maps to `nota/nota.fab`) |
| failure / exit | `iace/`, `mori/`, `fac/`, `cape/`, `exitus/` |
| async / streams | `fiet/`, `fiunt/`, `fient/` |
| tests | `proba/`, `probandum/` |
| package frontmatter | `directiva/`; package fixtures under `corpus-product/` |
| collection intrinsics | `intrinseca/` |
| tensors / device types | `tensor/`, `vector/`, `sparsa/`, `gpu-core-types/` |
| kernels | `kernel/` |
| kernel tiers and device regions | `kernel/tier.fab`, `kernel/tier-la.fab`, `kernel/region.fab`, `kernel/region-reject.fab` |
| lane-local return placement (execution still pending) | `itera/filum-return-thread.fab` (intentional rejection); grammar `redde_stmt` |
| list length ranges and destination-sized short literals | `lista/ranged-length.fab`, `lista/ranged-short-literal.fab`, `genus/ranged-list-field-literal.fab` |
| first matching tensor or matrix element | `summa/reducta-first-en.fab`, `summa/reducta-first-decline.fab` |
| list search and whole-list append | `lista/find-optional-element.fab`, `lista/methodi-extende.fab`, `lista/methodi-extende-reject.fab` |
| ranged integer store policies | `operatores/ranged-saturating.fab`, `operatores/ranged-trapping.fab`, `operatores/ranged-wrapping.fab` |
| tensor rank, norm and English edge policies | `tensor/rank-norm.fab`, `tensor/edge-shift-policies.fab` |
| locale-sealed record imports | `importa/braced-field-latin-reject.fab`, `importa/braced-field-english-reject-la.fab` |
| mixed template-hole warning | `scriptum/template-mixed-holes.fab` |
| callable console output | `importa/selective-import-console-print.fab` |
| sized numerics (bare `i32`, `u8`, `f32`) | `typi/sized-numerus.fab`; retired wrappers and `inf` rejects in `typi/*-error.fab` |
| unbounded integer `inf` | `lista/literal-reuse.fab`, `typi/inf-literal-range-error.fab` |
| conversions (`↦`, `↤`, `via` hint) | `conversio/conversio.fab`, `conversio/via-hint.fab`, `assignatio/conversio-assign.fab` |
| float to integer rounding (`via Floor\|Ceil\|HalfEven\|Trunc`) | `conversio/float-to-int-via.fab` (reject: `float-to-int-reject.fab`) |
| sort expression (`sort via asc\|desc from xs`) | `sort/`, `lista/ordina-clavis.fab`; rejects `lista/sorted-key-retired.fab`, `itera/sort-source-reject-en.fab` |
| three collection reads (`xs[i]`, `get`, `at`) | `lista/reads-three-ways.fab` |
| copy / own / deep const | `mutabilitas/copy-bind-deep.fab`, `alias-on-bind.fab`, reject `const-share-into-var-reject.fab` |
| inferred constructor `_ { }` / `_ [ ]` and bare literals | `genus/inferred-constructor.fab`, `inferred-literals.fab` |
| tensor `axis` reductions, `mean` | `tensor/axis-builtins.fab`, `summa/mean-sugar.fab` |
| loop shape change | `itera/loop-shape-reject.fab`, `tabula/loop-store-adds-key-trap.fab` |
| compile-time (`const` constant, `comptime`, `embed`; la `fixum`, `praefixum`, `insere`) | `fixum/fixum.fab`, `praefixum/praefixum.fab`, `insere/insere.fab` |
| shared union fields | `commune/commune.fab` |
| bounded types (`ascii`, bounded `list`) | `bounded/` |
| decimals | `decimal/` (`d64` only; `d32` is removed) |
| operator tours | `operatores/` |
| multi-feature flows | `integratio/` |

## Grouped Families Worth Knowing

| Directory | Purpose |
| --------- | ------- |
| `operatores/` | comparatio, bitwise, optional-chain, function-types, metadata, etc. |
| `intrinseca/` | compiler-owned `set`, `string`, `int`, `float` method surfaces (`lista/` and `tabula/` have their own directories) |
| `integratio/` | multi-construct happy paths |
| `conversio/` | conversions and casts |
| `assignatio/` | assignment operators |
| `mutabilitas/` | access markers: `mut` / `own` / `copy` / `ref`, call-site markers (`call-marker.fab`), writable `own`/`copy` parameters (`own-param-writable.fab`, `copy-param-writable.fab`), own-argument and use-after-own rules (`own-arg-*.fab`) and their SEM rejections |
| `destructura/` | destructuring patterns |
| `literalia/` | literal forms including JSON |
| `meta/` | compiler metadata directives (`throws`, `require`, `reject`, `unstable`, `nondum-function` and `nondum-method` for bodiless unavailable functions and class methods, `versio`, `targets`; the `meta/` file names are the Latin `iacit`, `requirit`, `reice`, `nondum`) |

Package-shaped proofs live under `corpus-product/`, not in the language tree.

## Output Contracts

Some exempla ship a `.expected` sidecar next to the `.fab` file.

A single corpus file checks and runs through the product binary (`faber check <file>`, `faber run <file>`; probe: `typi/sized-numerus.fab` output equals its `.expected`). Package fixtures run through `faber`.

To validate the corpus, run `faber corpus corpus-suites/public --target runner` from the repo root.

Narrow corpus work with `--domain`, `--category`, `--case` or `--filter`; the selectors add together (a case runs if any selector names it). Domain and category selection need a suite that declares domains, and an unknown exact case ID is an error. Ordered scans are described by `scan_expr` in `grammar/EBNF.md`; there is no `corpus/scan/` path.

## Corpus sidecars

- Legacy redirects: `corpus/legacy-redirects.toml`
- Excluded explain terms: `corpus/excluded-terms.toml`
- Variant-cell ledger (which variant cells a corpus exemplum teaches): `corpus/variant-cells.toml`

## Rule

Prefer opening the exact example file that declares the term in `explains` over relying on prose summaries here. When this file and the live corpus disagree, the corpus wins.
