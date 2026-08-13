# EBNF Target Support Matrix

**Rendered** by `faber/scripta/render-matrices.py` from `radix/corpus/measurement/compat/*.json` (emitted by the private radix ladder) — **do not hand-edit**.
**Measurement**: `emit_hir_target_matrix` + `emit_mir_target_matrix` (in-process radix harness, no external toolchains).
**Join**: `corpus/index.toml` terms → exempla.

This is the **official generated** grammar×target support matrix. It reports
**lowerability** — can target X lower grammar production Y — across every term in
the exempla corpus. Runtime semantics (erase/warn/defer policy verbs), per-target
contracts, and pipeline routing are covered on the
[target compatibility](https://faberlang.dev/en-US/toolchain/target-matrix.html)
and [Compiling and targets](https://faberlang.dev/en-US/toolchain/compiling.html)
pages of the documentation site.

## Legend

| Glyph | Meaning |
|---|---|
| ✓ | fully supported — all analyzable exempla for the term lower |
| ◐ | partial — some exempla lower, some have a measured gap |
| ○ | planned — not yet lowering; curated overlay (`scripta/ebnf-matrix-overrides.toml`) |
| ✕ | not supported — no exempla lower; default-truth, measured gap is real |
| — | not measured — no analyzable exempla for this term on this lane |

> A ✓ means the corpus exempla exercising this term lower to the target. It does
> **not** guarantee identical runtime semantics. Some targets *erase* or *warn* on
> certain constructs (e.g. Go erases borrow modes `de`/`in`/`ex`) — those still
> render ✓ here because they lower. See the policy doc for that nuance.

## Corpus-wide summary (all registered terms)

**Application lane (HIR → emitted source languages)**

| target | capable | analyzable | % |
|---|---|---|---|
| rust | 282 | 284 | 99% |
| go | 261 | 284 | 92% |
| ts | 284 | 284 | 100% |
| faber | 284 | 284 | 100% |

**Systems lane (MIR → device/IR artifacts)**

| target | capable | analyzable | % |
|---|---|---|---|
| llvm-text | 276 | 279 | 99% |
| wasm-text | 258 | 279 | 92% |
| wasm | 258 | 279 | 92% |
| metal-text | 6 | 279 | 2% |
| wgsl-text | 6 | 279 | 2% |
| sexp-struct | 221 | 279 | 79% |
| sexp | 221 | 279 | 79% |
| scena | 239 | 279 | 86% |

## Keywords — application lane

### keyword

| term | rust | go | ts | faber |
|---|---|---|---|---|
| `abstractus` | ✓ | ✓ | ✓ | ✓ |
| `ab` | ✓ | ✓ | ✓ | ✓ |
| `ad` | ✓ | ✕ | ✓ | ✓ |
| `adfirma` | ✓ | ✓ | ✓ | ✓ |
| `ante` | ✓ | ✓ | ✓ | ✓ |
| `atomic` | ✕ | ✓ | ✓ | ✓ |
| `argumenta` | ✓ | ✓ | ✓ | ✓ |
| `bivalens` | ✓ | ✓ | ✓ | ✓ |
| `cape` | ✓ | ✓ | ✓ | ✓ |
| `casu` | ✓ | ✓ | ✓ | ✓ |
| `cede` | ✓ | ✓ | ✓ | ✓ |
| `ceteri` | ✓ | ✓ | ✓ | ✓ |
| `ceterum` | ✓ | ✓ | ✓ | ✓ |
| `clausura` | ✓ | ✓ | ✓ | ✓ |
| `cli` | ✓ | ✓ | ✓ | ✓ |
| `copia` | ✓ | ✓ | ✓ | ✓ |
| `cura` | ✓ | ✓ | ✓ | ✓ |
| `curata` | ✓ | ✓ | ✓ | ✓ |
| `cursor` | ✓ | ✓ | ✓ | ✓ |
| `custodi` | ✓ | ✓ | ✓ | ✓ |
| `de` | ✓ | ✓ | ✓ | ✓ |
| `descriptio` | ✓ | ✓ | ✓ | ✓ |
| `discerne` | ✓ | ✓ | ✓ | ✓ |
| `discretio` | ✓ | ✓ | ✓ | ✓ |
| `dum` | ✓ | ✓ | ✓ | ✓ |
| `ego` | ✓ | ✓ | ✓ | ✓ |
| `elige` | ✓ | ✓ | ✓ | ✓ |
| `errata` | ✓ | ✓ | ✓ | ✓ |
| `est` | ✓ | ✓ | ✓ | ✓ |
| `ex` | ✓ | ✓ | ✓ | ✓ |
| `exitus` | ✓ | ✓ | ✓ | ✓ |
| `fac` | ✓ | ✓ | ✓ | ✓ |
| `falsum` | ✓ | ✓ | ✓ | ✓ |
| `fient` | ✓ | ✓ | ✓ | ✓ |
| `fiet` | ✓ | ✓ | ✓ | ✓ |
| `figendum` | ✓ | ✓ | ✓ | ✓ |
| `finge` | ✓ | ✓ | ✓ | ✓ |
| `fiunt` | ✓ | ✓ | ✓ | ✓ |
| `fixum` | ✓ | ✓ | ✓ | ✓ |
| `fragilis` | ✓ | ✓ | ✓ | ✓ |
| `fractus` | ✓ | ✓ | ✓ | ✓ |
| `functio` | ✓ | ✓ | ✓ | ✓ |
| `futura` | ✓ | ✓ | ✓ | ✓ |
| `futurum` | ✓ | ✓ | ✓ | ✓ |
| `generis` | ✓ | ✓ | ✓ | ✓ |
| `genus` | ✓ | ✓ | ✓ | ✓ |
| `iace` | ✓ | ✓ | ✓ | ✓ |
| `iacit` | ✓ | ✓ | ✓ | ✓ |
| `ignotum` | ✓ | ✓ | ✓ | ✓ |
| `immutata` | ✓ | ✓ | ✓ | ✓ |
| `implet` | ✓ | ✓ | ✓ | ✓ |
| `importa` | ✓ | ✓ | ✓ | ✓ |
| `in` | ✓ | ✓ | ✓ | ✓ |
| `incipiet` | ✓ | ✓ | ✓ | ✓ |
| `incipit` | ✓ | ✓ | ✓ | ✓ |
| `inter` | ✓ | ✓ | ✓ | ✓ |
| `intra` | ✓ | ✓ | ✓ | ✓ |
| `instans` | ✓ | ✓ | ✓ | ✓ |
| `itera` | ✓ | ✓ | ✓ | ✓ |
| `lege` | ✓ | ✓ | ✓ | ✓ |
| `lineam` | ✓ | ✓ | ✓ | ✓ |
| `lista` | ✓ | ✓ | ✓ | ✓ |
| `matrix` | ✓ | ✕ | ✓ | ✓ |
| `mone` | ✓ | ✓ | ✓ | ✓ |
| `mori` | ✓ | ✓ | ✓ | ✓ |
| `nexum` | ✓ | ✓ | ✓ | ✓ |
| `nihil` | ✓ | ✓ | ✓ | ✓ |
| `numquam` | ✓ | ✓ | ✓ | ✓ |
| `numerus` | ✓ | ✓ | ✓ | ✓ |
| `non` | ✓ | ✓ | ✓ | ✓ |
| `omitte` | ✓ | ✓ | ✓ | ✓ |
| `omnia` | ✓ | ✓ | ✓ | ✓ |
| `operandus` | ✓ | ✓ | ✓ | ✓ |
| `optio` | ✓ | ✓ | ✓ | ✓ |
| `optiones` | ✓ | ✓ | ✓ | ✓ |
| `ordo` | ✓ | ✓ | ✓ | ✓ |
| `octeti` | ✓ | ✓ | ✓ | ✓ |
| `implendum` | ✓ | ✓ | ✓ | ✓ |
| `per` | ✓ | ✓ | ✓ | ✓ |
| `perge` | ✓ | ✓ | ✓ | ✓ |
| `postpara` | ✓ | ✓ | ✓ | ✓ |
| `postparabit` | ✓ | ✓ | ✓ | ✓ |
| `prae` | ✓ | ✓ | ✓ | ✓ |
| `praefixum` | — | — | — | — |
| `praepara` | ✓ | ✓ | ✓ | ✓ |
| `praeparabit` | ✓ | ✓ | ✓ | ✓ |
| `promissum` | ✓ | ✓ | ✓ | ✓ |
| `privata` | ✓ | ✓ | ✓ | ✓ |
| `proba` | ✓ | ✓ | ✓ | ✓ |
| `probandum` | ✓ | ✓ | ✓ | ✓ |
| `protecta` | — | — | — | — |
| `publica` | ✓ | ✓ | ✓ | ✓ |
| `redde` | ✓ | ✓ | ✓ | ✓ |
| `reddet` | ✓ | ✓ | ✓ | ✓ |
| `repete` | ✓ | ✓ | ✓ | ✓ |
| `requirit` | — | — | — | — |
| `rumpe` | ✓ | ✓ | ✓ | ✓ |
| `scribe` | ✓ | ✓ | ✓ | ✓ |
| `scriptum` | ✓ | ✓ | ✓ | ✓ |
| `secus` | ✓ | ✓ | ✓ | ✓ |
| `si` | ✓ | ✓ | ✓ | ✓ |
| `sic` | ✓ | ✓ | ✓ | ✓ |
| `sin` | ✓ | ✓ | ✓ | ✓ |
| `sit` | ✓ | ✓ | ✓ | ✓ |
| `solum_in` | ✓ | ✓ | ✓ | ✓ |
| `solum` | ✓ | ✓ | ✓ | ✓ |
| `sparge` | ✓ | ✓ | ✓ | ✓ |
| `sponte` | ✓ | ✓ | ✓ | ✓ |
| `sub` | ✓ | ✓ | ✓ | ✓ |
| `tacet` | ✓ | ✓ | ✓ | ✓ |
| `tacebit` | ✓ | ✓ | ✓ | ✓ |
| `tabula` | ✓ | ✓ | ✓ | ✓ |
| `tag` | ✓ | ✓ | ✓ | ✓ |
| `temporis` | ✓ | ✓ | ✓ | ✓ |
| `tensor` | ✓ | ✓ | ✓ | ✓ |
| `textus` | ✓ | ✓ | ✓ | ✓ |
| `typus` | ✓ | ✓ | ✓ | ✓ |
| `ubique` | ✓ | ✓ | ✓ | ✓ |
| `usque` | ✓ | ✓ | ✓ | ✓ |
| `ut` | ✓ | ✓ | ✓ | ✓ |
| `varia` | ✓ | ✓ | ✓ | ✓ |
| `variandum` | ✓ | ✓ | ✓ | ✓ |
| `vector` | ✓ | ◐ | ✓ | ✓ |
| `vacuum` | ✓ | ✓ | ✓ | ✓ |
| `verum` | ✓ | ✓ | ✓ | ✓ |
| `vide` | ✓ | ✓ | ✓ | ✓ |

## Operators — application lane

### operator-group

| term | rust | go | ts | faber |
|---|---|---|---|---|
| `⊜` | ✓ | ✓ | ✓ | ✓ |
| `∧` | ✓ | ✓ | ✓ | ✓ |
| `·` | ✓ | ○ | ✓ | ✓ |
| `×` | ✓ | ○ | ✓ | ✓ |
| `⊗` | ✓ | ○ | ✓ | ✓ |
| `⊙` | ✓ | ○ | ✓ | ✓ |
| `→` | ✓ | ✓ | ✓ | ✓ |
| `⇥` | ✓ | ✓ | ✓ | ✓ |
| `←` | ✓ | ✓ | ✓ | ✓ |
| `↤` | ✓ | ✓ | ✓ | ✓ |
| `aut` | ✓ | ✓ | ✓ | ✓ |
| `![` | ✓ | ✓ | ✓ | ✓ |
| `!.` | ✓ | ✓ | ✓ | ✓ |
| `≠` | ✓ | ✓ | ✓ | ✓ |
| `!(` | ✓ | ✓ | ✓ | ✓ |
| `⊻` | ✓ | ✓ | ✓ | ✓ |
| `↦` | ✓ | ✓ | ✓ | ✓ |
| `⇒` | ✓ | ✓ | ✓ | ✓ |
| `‥` | ✓ | ✓ | ✓ | ✓ |
| `…` | ✓ | ✓ | ✓ | ✓ |
| `≡` | ✓ | ✓ | ✓ | ✓ |
| `=` | ✓ | ✓ | ✓ | ✓ |
| `et` | ✓ | ✓ | ✓ | ✓ |
| `≥` | ✓ | ✓ | ✓ | ✓ |
| `≤` | ✓ | ✓ | ✓ | ✓ |
| `⊖` | ✓ | ✓ | ✓ | ✓ |
| `modulus<u16>` | ✓ | ✕ | ✓ | ✓ |
| `modulus<u32>` | ✓ | ✕ | ✓ | ✓ |
| `modulus<u64>` | ✓ | ✕ | ✓ | ✓ |
| `modulus<u8>` | ✓ | ✕ | ✓ | ✓ |
| `non est` | ✓ | ✓ | ✓ | ✓ |
| `⊚` | ✓ | ✓ | ✓ | ✓ |
| `∨` | ✓ | ✓ | ✓ | ✓ |
| `∪` | ✓ | ✓ | ✓ | ✓ |
| `⊕` | ✓ | ✓ | ✓ | ✓ |
| `?[` | ✓ | ✓ | ✓ | ✓ |
| `?.` | ✓ | ✓ | ✓ | ✓ |
| `?(` | ✓ | ✓ | ✓ | ✓ |
| `§` | ✓ | ✓ | ✓ | ✓ |
| `⇐` | ✓ | ✓ | ✓ | ✓ |
| `⊘` | ✓ | ✓ | ✓ | ✓ |
| `⊛` | ✓ | ✓ | ✓ | ✓ |
| `¬` | ✓ | ✓ | ✓ | ✓ |
| `vel` | ✓ | ✓ | ✓ | ✓ |
| `∷` | ✓ | ✓ | ✓ | ✓ |
| `∴` | ✓ | ✓ | ✓ | ✓ |
| `ergo` | ✓ | ✓ | ✓ | ✓ |

## Keywords — systems lane

### keyword

| term | llvm-text | wasm-text | wasm | metal-text | wgsl-text | sexp-struct | sexp | scena |
|---|---|---|---|---|---|---|---|---|
| `abstractus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ab` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ad` | ✓ | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `adfirma` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ante` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `atomic` | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ |
| `argumenta` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `bivalens` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `cape` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `casu` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `cede` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `ceteri` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ceterum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `clausura` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `cli` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `copia` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `cura` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `curata` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `cursor` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `custodi` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `de` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `descriptio` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `discerne` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `discretio` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `dum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ego` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `elige` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `errata` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `est` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `ex` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `exitus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `fac` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `falsum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `fient` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `fiet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `figendum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `finge` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `fiunt` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `fixum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `fragilis` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `fractus` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| `functio` | ✓ | ◐ | ◐ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `futura` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `futurum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `generis` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `genus` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| `iace` | ✓ | ◐ | ◐ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `iacit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ignotum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `immutata` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `implet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `importa` | ✓ | ◐ | ◐ | ✕ | ✕ | ◐ | ◐ | ◐ |
| `in` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `incipiet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `incipit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `inter` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `intra` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `instans` | ✓ | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ | ◐ |
| `itera` | ✓ | ◐ | ◐ | ✕ | ✕ | ◐ | ◐ | ✓ |
| `lege` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `lineam` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `lista` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| `matrix` | ✕ | ✕ | ✕ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `mone` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `mori` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `nexum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `nihil` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `numquam` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✕ |
| `numerus` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ◐ |
| `non` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `omitte` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `omnia` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `operandus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `optio` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `optiones` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `ordo` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `octeti` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `implendum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `per` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `perge` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `postpara` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `postparabit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `prae` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `praefixum` | — | — | — | — | — | — | — | — |
| `praepara` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `praeparabit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `promissum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `privata` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `proba` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `probandum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `protecta` | — | — | — | — | — | — | — | — |
| `publica` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `redde` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ◐ |
| `reddet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `repete` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `requirit` | — | — | — | — | — | — | — | — |
| `rumpe` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `scribe` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `scriptum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `secus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `si` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `sic` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `sin` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `sit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `solum_in` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `solum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `sparge` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `sponte` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `sub` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `tacet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `tacebit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `tabula` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `tag` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `temporis` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| `tensor` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ◐ |
| `textus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `typus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ubique` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `usque` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ut` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `varia` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `variandum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `vector` | ✓ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ | ✕ |
| `vacuum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `verum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `vide` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |

## Operators — systems lane

### operator-group

| term | llvm-text | wasm-text | wasm | metal-text | wgsl-text | sexp-struct | sexp | scena |
|---|---|---|---|---|---|---|---|---|
| `⊜` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `∧` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `·` | — | — | — | — | — | — | — | — |
| `×` | — | — | — | — | — | — | — | — |
| `⊗` | — | — | — | — | — | — | — | — |
| `⊙` | — | — | — | — | — | — | — | — |
| `→` | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `⇥` | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `←` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `↤` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `aut` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `![` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `!.` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `≠` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `!(` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `⊻` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `↦` | ✓ | ◐ | ◐ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `⇒` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `‥` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `…` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `≡` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `=` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `et` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `≥` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `≤` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `⊖` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `modulus<u16>` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `modulus<u32>` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| `modulus<u64>` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| `modulus<u8>` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| `non est` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `⊚` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `∨` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `∪` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `⊕` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `?[` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `?.` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `?(` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `§` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `⇐` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `⊘` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `⊛` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `¬` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `vel` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `∷` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| `∴` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| `ergo` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |

## Types, intrinsics & meta

### existing-home

| term | rust | go | ts | faber |
|---|---|---|---|---|
| `alias` | ✓ | ✓ | ✓ | ✓ |
| `arena` | ✓ | ✓ | ✓ | ✓ |
| `@` | ✓ | ✓ | ✓ | ✓ |
| `f16` | ✕ | ✓ | ✓ | ✓ |
| `imperia` | ✓ | ✓ | ✓ | ✓ |
| `imperium` | ✓ | ✓ | ✓ | ✓ |
| `manifest` | ✓ | ✓ | ✓ | ✓ |
| `metior` | ✓ | ✓ | ✓ | ✓ |
| `nondum` | ✓ | ✓ | ✓ | ✓ |
| `objectum` | ✓ | ✓ | ✓ | ✓ |
| `prima` | ✓ | ✓ | ✓ | ✓ |
| `string` | ✓ | ✓ | ✓ | ✓ |
| `block-string` | ✓ | ✓ | ✓ | ✓ |
| `summa` | ✓ | ✓ | ✓ | ✓ |
| `targets` | ✓ | ✓ | ✓ | ✓ |
| `ultima` | ✓ | ✓ | ✓ | ✓ |
| `versio` | ✓ | ✓ | ✓ | ✓ |

## Regeneration

The measurement JSON is regenerated by the private radix ladder (`./scripta/test --full` / `--release`, stage-4 measurement gates) via `scripta/emit-compat-json.py` and committed at radix release. Render:

```bash
python3 scripta/render-matrices.py          # render in place
python3 scripta/render-matrices.py --check  # fail if committed docs are stale
```

Rerun whenever the codegen, MIR lowering, or exempla corpus changes.
