# Radix language corpus

Keyword-oriented language reference programs for Faber.

**Invariant:** every entry here is **single-file** and **Radix-checkable**.
No `faber.toml`, no multi-file packages. Package fixtures belong in
`faber/corpus/` (see `docs/factory/corpus-split-radix-faber/goal.md`).

This tree is the development source for `faber explain` (via the reference
pack) and the primary input for multi-target compile matrices.

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
| Package build fixtures (`faber.toml`) | `faber/corpus/` |
| Application packages | sibling `examples/` (`coreutils/`, …) |
| GPU workload rungs | `examples/gpu-workload/` |
| Norma stdlib tours | sibling `norma/exempla/` |

## Check / test

```bash
cargo run -p radix-cli --bin radix -- check corpus/incipit/incipit.fab
cargo run -p radix-cli --bin radix -- test corpus/incipit/incipit.fab
```

## Explain terms

A file owns the `faber explain` terms it lists in frontmatter:

```toml
explains = ["←", "="]
```

Each term has exactly one owner. The `faber` build embeds every file that
declares `explains`, so there is no index to regenerate.
