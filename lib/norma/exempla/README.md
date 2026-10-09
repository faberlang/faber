# Norma stdlib exempla

Instructional and contract demos for `norma:*` modules.

These programs used to live under the private Radix language corpus
(`stdlib-nativum/`, stdlib `chorda/` tours). They belong with the standard
library source, not the keyword language dictionary.

## Layout

```text
exempla/
  error-conversio/  # declared error arm and wrapper-deletion contract
  stdlib-nativum/    # module tours (chorda, mathesis, toml, solum.glob, …)
  chorda/            # deeper chorda method demos
  caelum/            # net type construction (terminus / connexus / auscultator)
  http/              # HTTP directory-form import fixtures (headers / request / response / facade)
```

## Coverage ledger

| Package | Contract | Proof |
| --- | --- | --- |
| `error-conversio` | Declared `@ conversion` arm replaces an administrative wrapper; a missing direct arm remains a named fail-closed decline. | `src/error_conversio.fab` + `src/error-conversio.proba` |

Language keyword exempla: sibling `examples/corpus/`.
