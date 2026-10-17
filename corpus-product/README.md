# Faber package corpus

Package-shaped product fixtures for Faber build/run/test proofs.

**Invariant:** every top-level entry used as a package fixture has a
`faber.toml`, except `format/`, a flat directory of single-file inputs with
their expected output. Pure single-file keyword programs live in
**`corpus/`**.

| Path | Role |
| --- | --- |
| `proba-canary/` | Proba canary: pure functions with colocated `main.proba` tests; `faber test` must report 0 failed / 0 blocked |
| `regio/` | `module` default import name: `import from "./planum"` binds the sibling's declared `geometry` |
| `contract-import/`, `contract-import-alias/`, `contract-import-wildcard/`, `alias-contract-import/` | Contract-import packages (plain, aliased, wildcard) with their expected output |
| `format/` | `.fab` inputs with `.format.expected` files, the fixtures for `faber format` |
| `reader-locale/` | Reader-locale packages (`ar`, `hi`, `th-TH`, `vi`, `zh-Hans`, `zh-Hant`), samples for `faber convert` and the package reader-locale tests; `th-TH/twins/main.la.fab` is the Latin twin and `<locale>/faults/*.fab` are the diagnostic fault inputs |

## Run

```bash
faber run corpus-product/regio
faber test corpus-product/proba-canary
```
