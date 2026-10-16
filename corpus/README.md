# Faber language corpus

Keyword-oriented language reference programs for Faber.

**Invariant:** every entry here is **single-file** and checkable with
`faber check`. No `faber.toml`, no multi-file packages. Package fixtures
belong in `corpus-product/`.

This tree is the development source for `faber explain` (via the reference
pack) and the main input for the corpus suites under `corpus-suites/`.

## Layout

```text
corpus/
  <keyword>/           # e.g. si/, vel/, functio/
    <keyword>.fab      # primary smoke
    …
  operatores/          # glyph / operator groups
  lista/ tabula/ …     # language types and compiler-owned method surfaces
  legacy-redirects.toml
  excluded-terms.toml
```

## Not here

| Content | Location |
| ------- | -------- |
| Package build fixtures (`faber.toml`) | `corpus-product/` |
| Application packages | the separate `examples` repository |
| Norma stdlib tours | `lib/norma/exempla/` |

## Check / test

```bash
faber check corpus/incipit/incipit.fab
faber test corpus/incipit/incipit.fab
```

## Explain terms

A file owns the `faber explain` terms it lists in frontmatter:

```toml
explains = ["←", "="]
```

Each term has exactly one owner. The `faber` build embeds every file that
declares `explains`, so there is no index to regenerate.
