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
| rust | 288 | 290 | 99% |
| go | 267 | 290 | 92% |
| ts | 290 | 290 | 100% |
| faber | 290 | 290 | 100% |

**Systems lane (MIR → device/IR artifacts)**

| target | capable | analyzable | % |
|---|---|---|---|
| llvm-text | 279 | 285 | 98% |
| wasm-text | 256 | 285 | 90% |
| wasm | 256 | 285 | 90% |
| metal-text | 6 | 285 | 2% |
| wgsl-text | 6 | 285 | 2% |
| sexp-struct | 223 | 285 | 78% |
| sexp | 223 | 285 | 78% |
| scena | 242 | 285 | 85% |

## Keywords — application lane

### keyword

| term | rust | go | ts | faber |
|---|---|---|---|---|
| <a id="abstractus"></a>`abstractus` | ✓ | ✓ | ✓ | ✓ |
| <a id="ab"></a>`ab` | ✓ | ✓ | ✓ | ✓ |
| <a id="ad"></a>`ad` | ✓ | ✕ | ✓ | ✓ |
| <a id="adfirma"></a>`adfirma` | ✓ | ✓ | ✓ | ✓ |
| <a id="ante"></a>`ante` | ✓ | ✓ | ✓ | ✓ |
| <a id="atomic"></a>`atomic` | ✕ | ✓ | ✓ | ✓ |
| <a id="argumenta"></a>`argumenta` | ✓ | ✓ | ✓ | ✓ |
| <a id="bivalens"></a>`bivalens` | ✓ | ✓ | ✓ | ✓ |
| <a id="cape"></a>`cape` | ✓ | ✓ | ✓ | ✓ |
| <a id="casu"></a>`casu` | ✓ | ✓ | ✓ | ✓ |
| <a id="cede"></a>`cede` | ✓ | ✓ | ✓ | ✓ |
| <a id="ceteri"></a>`ceteri` | ✓ | ✓ | ✓ | ✓ |
| <a id="ceterum"></a>`ceterum` | ✓ | ✓ | ✓ | ✓ |
| <a id="clausura"></a>`clausura` | ✓ | ✓ | ✓ | ✓ |
| <a id="cli"></a>`cli` | ✓ | ✓ | ✓ | ✓ |
| <a id="copia"></a>`copia` | ✓ | ✓ | ✓ | ✓ |
| <a id="cura"></a>`cura` | ✓ | ✓ | ✓ | ✓ |
| <a id="curata"></a>`curata` | ✓ | ✓ | ✓ | ✓ |
| <a id="cursor"></a>`cursor` | ✓ | ✓ | ✓ | ✓ |
| <a id="custodi"></a>`custodi` | ✓ | ✓ | ✓ | ✓ |
| <a id="de"></a>`de` | ✓ | ✓ | ✓ | ✓ |
| <a id="descriptio"></a>`descriptio` | ✓ | ✓ | ✓ | ✓ |
| <a id="discerne"></a>`discerne` | ✓ | ✓ | ✓ | ✓ |
| <a id="discretio"></a>`discretio` | ✓ | ✓ | ✓ | ✓ |
| <a id="dum"></a>`dum` | ✓ | ✓ | ✓ | ✓ |
| <a id="ego"></a>`ego` | ✓ | ✓ | ✓ | ✓ |
| <a id="elige"></a>`elige` | ✓ | ✓ | ✓ | ✓ |
| <a id="errata"></a>`errata` | ✓ | ✓ | ✓ | ✓ |
| <a id="est"></a>`est` | ✓ | ✓ | ✓ | ✓ |
| <a id="ex"></a>`ex` | ✓ | ✓ | ✓ | ✓ |
| <a id="exitus"></a>`exitus` | ✓ | ✓ | ✓ | ✓ |
| <a id="fac"></a>`fac` | ✓ | ✓ | ✓ | ✓ |
| <a id="falsum"></a>`falsum` | ✓ | ✓ | ✓ | ✓ |
| <a id="fient"></a>`fient` | ✓ | ✓ | ✓ | ✓ |
| <a id="fiet"></a>`fiet` | ✓ | ✓ | ✓ | ✓ |
| <a id="figendum"></a>`figendum` | ✓ | ✓ | ✓ | ✓ |
| <a id="finge"></a>`finge` | ✓ | ✓ | ✓ | ✓ |
| <a id="fiunt"></a>`fiunt` | ✓ | ✓ | ✓ | ✓ |
| <a id="fixum"></a>`fixum` | ✓ | ✓ | ✓ | ✓ |
| <a id="fragilis"></a>`fragilis` | ✓ | ✓ | ✓ | ✓ |
| <a id="fractus"></a>`fractus` | ✓ | ✓ | ✓ | ✓ |
| <a id="functio"></a>`functio` | ✓ | ✓ | ✓ | ✓ |
| <a id="futura"></a>`futura` | ✓ | ✓ | ✓ | ✓ |
| <a id="futurum"></a>`futurum` | ✓ | ✓ | ✓ | ✓ |
| <a id="generis"></a>`generis` | ✓ | ✓ | ✓ | ✓ |
| <a id="genus"></a>`genus` | ✓ | ✓ | ✓ | ✓ |
| <a id="iace"></a>`iace` | ✓ | ✓ | ✓ | ✓ |
| <a id="iacit"></a>`iacit` | ✓ | ✓ | ✓ | ✓ |
| <a id="ignotum"></a>`ignotum` | ✓ | ✓ | ✓ | ✓ |
| <a id="immutata"></a>`immutata` | ✓ | ✓ | ✓ | ✓ |
| <a id="implet"></a>`implet` | ✓ | ✓ | ✓ | ✓ |
| <a id="importa"></a>`importa` | ✓ | ✓ | ✓ | ✓ |
| <a id="in"></a>`in` | ✓ | ✓ | ✓ | ✓ |
| <a id="incipiet"></a>`incipiet` | ✓ | ✓ | ✓ | ✓ |
| <a id="incipit"></a>`incipit` | ✓ | ✓ | ✓ | ✓ |
| <a id="inter"></a>`inter` | ✓ | ✓ | ✓ | ✓ |
| <a id="intra"></a>`intra` | ✓ | ✓ | ✓ | ✓ |
| <a id="instans"></a>`instans` | ✓ | ✓ | ✓ | ✓ |
| <a id="itera"></a>`itera` | ✓ | ✓ | ✓ | ✓ |
| <a id="lege"></a>`lege` | ✓ | ✓ | ✓ | ✓ |
| <a id="lineam"></a>`lineam` | ✓ | ✓ | ✓ | ✓ |
| <a id="lista"></a>`lista` | ✓ | ✓ | ✓ | ✓ |
| <a id="matrix"></a>`matrix` | ✓ | ✕ | ✓ | ✓ |
| <a id="mone"></a>`mone` | ✓ | ✓ | ✓ | ✓ |
| <a id="mori"></a>`mori` | ✓ | ✓ | ✓ | ✓ |
| <a id="nexum"></a>`nexum` | ✓ | ✓ | ✓ | ✓ |
| <a id="nihil"></a>`nihil` | ✓ | ✓ | ✓ | ✓ |
| <a id="numquam"></a>`numquam` | ✓ | ✓ | ✓ | ✓ |
| <a id="numerus"></a>`numerus` | ✓ | ✓ | ✓ | ✓ |
| <a id="non"></a>`non` | ✓ | ✓ | ✓ | ✓ |
| <a id="omitte"></a>`omitte` | ✓ | ✓ | ✓ | ✓ |
| <a id="omnia"></a>`omnia` | ✓ | ✓ | ✓ | ✓ |
| <a id="operandus"></a>`operandus` | ✓ | ✓ | ✓ | ✓ |
| <a id="optio"></a>`optio` | ✓ | ✓ | ✓ | ✓ |
| <a id="optiones"></a>`optiones` | ✓ | ✓ | ✓ | ✓ |
| <a id="ordo"></a>`ordo` | ✓ | ✓ | ✓ | ✓ |
| <a id="octeti"></a>`octeti` | ✓ | ✓ | ✓ | ✓ |
| <a id="implendum"></a>`implendum` | ✓ | ✓ | ✓ | ✓ |
| <a id="per"></a>`per` | ✓ | ✓ | ✓ | ✓ |
| <a id="perge"></a>`perge` | ✓ | ✓ | ✓ | ✓ |
| <a id="postpara"></a>`postpara` | ✓ | ✓ | ✓ | ✓ |
| <a id="postparabit"></a>`postparabit` | ✓ | ✓ | ✓ | ✓ |
| <a id="prae"></a>`prae` | ✓ | ✓ | ✓ | ✓ |
| <a id="praefixum"></a>`praefixum` | — | — | — | — |
| <a id="praepara"></a>`praepara` | ✓ | ✓ | ✓ | ✓ |
| <a id="praeparabit"></a>`praeparabit` | ✓ | ✓ | ✓ | ✓ |
| <a id="promissum"></a>`promissum` | ✓ | ✓ | ✓ | ✓ |
| <a id="privata"></a>`privata` | ✓ | ✓ | ✓ | ✓ |
| <a id="proba"></a>`proba` | ✓ | ✓ | ✓ | ✓ |
| <a id="probandum"></a>`probandum` | ✓ | ✓ | ✓ | ✓ |
| <a id="protecta"></a>`protecta` | — | — | — | — |
| <a id="publica"></a>`publica` | ✓ | ✓ | ✓ | ✓ |
| <a id="redde"></a>`redde` | ✓ | ✓ | ✓ | ✓ |
| <a id="reddet"></a>`reddet` | ✓ | ✓ | ✓ | ✓ |
| <a id="repete"></a>`repete` | ✓ | ✓ | ✓ | ✓ |
| <a id="requirit"></a>`requirit` | ✓ | ✓ | ✓ | ✓ |
| <a id="rumpe"></a>`rumpe` | ✓ | ✓ | ✓ | ✓ |
| <a id="scribe"></a>`scribe` | ✓ | ✓ | ✓ | ✓ |
| <a id="scriptum"></a>`scriptum` | ✓ | ✓ | ✓ | ✓ |
| <a id="secus"></a>`secus` | ✓ | ✓ | ✓ | ✓ |
| <a id="si"></a>`si` | ✓ | ✓ | ✓ | ✓ |
| <a id="sic"></a>`sic` | ✓ | ✓ | ✓ | ✓ |
| <a id="sin"></a>`sin` | ✓ | ✓ | ✓ | ✓ |
| <a id="sit"></a>`sit` | ✓ | ✓ | ✓ | ✓ |
| <a id="solum-in"></a>`solum_in` | ✓ | ✓ | ✓ | ✓ |
| <a id="solum"></a>`solum` | ✓ | ✓ | ✓ | ✓ |
| <a id="sparge"></a>`sparge` | ✓ | ✓ | ✓ | ✓ |
| <a id="sponte"></a>`sponte` | ✓ | ✓ | ✓ | ✓ |
| <a id="sub"></a>`sub` | ✓ | ✓ | ✓ | ✓ |
| <a id="tacet"></a>`tacet` | ✓ | ✓ | ✓ | ✓ |
| <a id="tacebit"></a>`tacebit` | ✓ | ✓ | ✓ | ✓ |
| <a id="tabula"></a>`tabula` | ✓ | ✓ | ✓ | ✓ |
| <a id="tag"></a>`tag` | ✓ | ✓ | ✓ | ✓ |
| <a id="temporis"></a>`temporis` | ✓ | ✓ | ✓ | ✓ |
| <a id="tensor"></a>`tensor` | ✓ | ✓ | ✓ | ✓ |
| <a id="textus"></a>`textus` | ✓ | ✓ | ✓ | ✓ |
| <a id="typus"></a>`typus` | ✓ | ✓ | ✓ | ✓ |
| <a id="ubique"></a>`ubique` | ✓ | ✓ | ✓ | ✓ |
| <a id="usque"></a>`usque` | ✓ | ✓ | ✓ | ✓ |
| <a id="ut"></a>`ut` | ✓ | ✓ | ✓ | ✓ |
| <a id="varia"></a>`varia` | ✓ | ✓ | ✓ | ✓ |
| <a id="variandum"></a>`variandum` | ✓ | ✓ | ✓ | ✓ |
| <a id="vector"></a>`vector` | ✓ | ◐ | ✓ | ✓ |
| <a id="vacuum"></a>`vacuum` | ✓ | ✓ | ✓ | ✓ |
| <a id="verum"></a>`verum` | ✓ | ✓ | ✓ | ✓ |
| <a id="vide"></a>`vide` | ✓ | ✓ | ✓ | ✓ |

## Operators — application lane

### operator-group

| term | rust | go | ts | faber |
|---|---|---|---|---|
| <a id=""></a>`⊜` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`∧` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`·` | ✓ | ◐ | ✓ | ✓ |
| <a id=""></a>`×` | ✓ | ○ | ✓ | ✓ |
| <a id=""></a>`⊗` | ✓ | ○ | ✓ | ✓ |
| <a id=""></a>`⊙` | ✓ | ◐ | ✓ | ✓ |
| <a id=""></a>`→` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`⇥` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`←` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`↤` | ✓ | ✓ | ✓ | ✓ |
| <a id="aut"></a>`aut` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`![` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`!.` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`≠` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`!(` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊻` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`↦` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`⇒` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`‥` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`…` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`≡` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`=` | ✓ | ✓ | ✓ | ✓ |
| <a id="et"></a>`et` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`≥` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`≤` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`↓` | ✓ | ✓ | ✓ | ✓ |
| <a id="modulus-u16"></a>`modulus<u16>` | ✓ | ✕ | ✓ | ✓ |
| <a id="modulus-u32"></a>`modulus<u32>` | ✓ | ✕ | ✓ | ✓ |
| <a id="modulus-u64"></a>`modulus<u64>` | ✓ | ✕ | ✓ | ✓ |
| <a id="modulus-u8"></a>`modulus<u8>` | ✓ | ✕ | ✓ | ✓ |
| <a id="non-est"></a>`non est` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊚` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`∨` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`∪` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`↑` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`?[` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`?.` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`?(` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`§` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`⇐` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊘` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊛` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`¬` | ✓ | ✓ | ✓ | ✓ |
| <a id="vel"></a>`vel` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`∷` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`∴` | ✓ | ✓ | ✓ | ✓ |
| <a id="ergo"></a>`ergo` | ✓ | ✓ | ✓ | ✓ |

## Keywords — systems lane

### keyword

| term | llvm-text | wasm-text | wasm | metal-text | wgsl-text | sexp-struct | sexp | scena |
|---|---|---|---|---|---|---|---|---|
| <a id="abstractus"></a>`abstractus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ab"></a>`ab` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ad"></a>`ad` | ✓ | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="adfirma"></a>`adfirma` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ante"></a>`ante` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="atomic"></a>`atomic` | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ |
| <a id="argumenta"></a>`argumenta` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="bivalens"></a>`bivalens` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="cape"></a>`cape` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="casu"></a>`casu` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="cede"></a>`cede` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="ceteri"></a>`ceteri` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ceterum"></a>`ceterum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="clausura"></a>`clausura` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="cli"></a>`cli` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="copia"></a>`copia` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="cura"></a>`cura` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="curata"></a>`curata` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="cursor"></a>`cursor` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="custodi"></a>`custodi` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="de"></a>`de` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="descriptio"></a>`descriptio` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="discerne"></a>`discerne` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="discretio"></a>`discretio` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="dum"></a>`dum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ego"></a>`ego` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="elige"></a>`elige` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="errata"></a>`errata` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="est"></a>`est` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="ex"></a>`ex` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="exitus"></a>`exitus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="fac"></a>`fac` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="falsum"></a>`falsum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="fient"></a>`fient` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="fiet"></a>`fiet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="figendum"></a>`figendum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="finge"></a>`finge` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="fiunt"></a>`fiunt` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="fixum"></a>`fixum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="fragilis"></a>`fragilis` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="fractus"></a>`fractus` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| <a id="functio"></a>`functio` | ✓ | ◐ | ◐ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="futura"></a>`futura` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="futurum"></a>`futurum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="generis"></a>`generis` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="genus"></a>`genus` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| <a id="iace"></a>`iace` | ✓ | ◐ | ◐ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="iacit"></a>`iacit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ignotum"></a>`ignotum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="immutata"></a>`immutata` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="implet"></a>`implet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="importa"></a>`importa` | ✓ | ◐ | ◐ | ✕ | ✕ | ◐ | ◐ | ◐ |
| <a id="in"></a>`in` | — | — | — | — | — | — | — | — |
| <a id="incipiet"></a>`incipiet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="incipit"></a>`incipit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="inter"></a>`inter` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="intra"></a>`intra` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="instans"></a>`instans` | ✓ | ✕ | ✕ | ✕ | ✕ | ✕ | ✕ | ◐ |
| <a id="itera"></a>`itera` | ✓ | ◐ | ◐ | ✕ | ✕ | ◐ | ◐ | ✓ |
| <a id="lege"></a>`lege` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="lineam"></a>`lineam` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="lista"></a>`lista` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| <a id="matrix"></a>`matrix` | ✕ | ✕ | ✕ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="mone"></a>`mone` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="mori"></a>`mori` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="nexum"></a>`nexum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="nihil"></a>`nihil` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="numquam"></a>`numquam` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✕ |
| <a id="numerus"></a>`numerus` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ◐ |
| <a id="non"></a>`non` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="omitte"></a>`omitte` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="omnia"></a>`omnia` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="operandus"></a>`operandus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="optio"></a>`optio` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="optiones"></a>`optiones` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="ordo"></a>`ordo` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="octeti"></a>`octeti` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="implendum"></a>`implendum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="per"></a>`per` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="perge"></a>`perge` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="postpara"></a>`postpara` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="postparabit"></a>`postparabit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="prae"></a>`prae` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="praefixum"></a>`praefixum` | — | — | — | — | — | — | — | — |
| <a id="praepara"></a>`praepara` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="praeparabit"></a>`praeparabit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="promissum"></a>`promissum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="privata"></a>`privata` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="proba"></a>`proba` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="probandum"></a>`probandum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="protecta"></a>`protecta` | — | — | — | — | — | — | — | — |
| <a id="publica"></a>`publica` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="redde"></a>`redde` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ◐ |
| <a id="reddet"></a>`reddet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="repete"></a>`repete` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="requirit"></a>`requirit` | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="rumpe"></a>`rumpe` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="scribe"></a>`scribe` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="scriptum"></a>`scriptum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="secus"></a>`secus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="si"></a>`si` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="sic"></a>`sic` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="sin"></a>`sin` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="sit"></a>`sit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="solum-in"></a>`solum_in` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="solum"></a>`solum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="sparge"></a>`sparge` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="sponte"></a>`sponte` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="sub"></a>`sub` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="tacet"></a>`tacet` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="tacebit"></a>`tacebit` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="tabula"></a>`tabula` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="tag"></a>`tag` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="temporis"></a>`temporis` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✕ |
| <a id="tensor"></a>`tensor` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ◐ |
| <a id="textus"></a>`textus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="typus"></a>`typus` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ubique"></a>`ubique` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="usque"></a>`usque` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ut"></a>`ut` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="varia"></a>`varia` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="variandum"></a>`variandum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="vector"></a>`vector` | ✓ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ | ✕ |
| <a id="vacuum"></a>`vacuum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="verum"></a>`verum` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="vide"></a>`vide` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |

## Operators — systems lane

### operator-group

| term | llvm-text | wasm-text | wasm | metal-text | wgsl-text | sexp-struct | sexp | scena |
|---|---|---|---|---|---|---|---|---|
| <a id=""></a>`⊜` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`∧` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`·` | ◐ | ◐ | ◐ | ○ | ○ | ◐ | ◐ | ◐ |
| <a id=""></a>`×` | ✓ | ○ | ○ | ○ | ○ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊗` | ○ | ○ | ○ | ○ | ○ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊙` | ◐ | ◐ | ◐ | ○ | ○ | ○ | ○ | ◐ |
| <a id=""></a>`→` | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`⇥` | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`←` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`↤` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="aut"></a>`aut` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`![` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`!.` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`≠` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`!(` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊻` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`↦` | ✓ | ◐ | ◐ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id=""></a>`⇒` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`‥` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`…` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`≡` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`=` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="et"></a>`et` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`≥` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`≤` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`↓` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="modulus-u16"></a>`modulus<u16>` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="modulus-u32"></a>`modulus<u32>` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| <a id="modulus-u64"></a>`modulus<u64>` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| <a id="modulus-u8"></a>`modulus<u8>` | ✓ | ✓ | ✓ | ✕ | ✕ | ✕ | ✕ | ✓ |
| <a id="non-est"></a>`non est` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊚` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`∨` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`∪` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`↑` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`?[` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`?.` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`?(` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`§` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`⇐` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊘` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`⊛` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`¬` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="vel"></a>`vel` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id=""></a>`∷` | ✓ | ✓ | ✓ | ✕ | ✕ | ◐ | ◐ | ✓ |
| <a id=""></a>`∴` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |
| <a id="ergo"></a>`ergo` | ✓ | ✓ | ✓ | ✕ | ✕ | ✓ | ✓ | ✓ |

## Types, intrinsics & meta

### existing-home

| term | rust | go | ts | faber |
|---|---|---|---|---|
| <a id="alias"></a>`alias` | ✓ | ✓ | ✓ | ✓ |
| <a id="arena"></a>`arena` | ✓ | ✓ | ✓ | ✓ |
| <a id=""></a>`@` | ✓ | ✓ | ✓ | ✓ |
| <a id="f16"></a>`f16` | ✕ | ✓ | ✓ | ✓ |
| <a id="imperia"></a>`imperia` | ✓ | ✓ | ✓ | ✓ |
| <a id="imperium"></a>`imperium` | ✓ | ✓ | ✓ | ✓ |
| <a id="manifest"></a>`manifest` | ✓ | ✓ | ✓ | ✓ |
| <a id="metior"></a>`metior` | ✓ | ✓ | ✓ | ✓ |
| <a id="nondum"></a>`nondum` | ✓ | ✓ | ✓ | ✓ |
| <a id="objectum"></a>`objectum` | ✓ | ✓ | ✓ | ✓ |
| <a id="prima"></a>`prima` | ✓ | ✓ | ✓ | ✓ |
| <a id="string"></a>`string` | ✓ | ✓ | ✓ | ✓ |
| <a id="block-string"></a>`block-string` | ✓ | ✓ | ✓ | ✓ |
| <a id="summa"></a>`summa` | ✓ | ✓ | ✓ | ✓ |
| <a id="targets"></a>`targets` | ✓ | ✓ | ✓ | ✓ |
| <a id="ultima"></a>`ultima` | ✓ | ✓ | ✓ | ✓ |
| <a id="versio"></a>`versio` | ✓ | ✓ | ✓ | ✓ |

## Regeneration

The measurement JSON is regenerated by the private radix ladder (`./scripta/test --full` / `--release`, stage-4 measurement gates) via `scripta/emit-compat-json.py` and committed at radix release. Render:

```bash
python3 scripta/render-matrices.py          # render in place
python3 scripta/render-matrices.py --check  # fail if committed docs are stale
```

Rerun whenever the codegen, MIR lowering, or exempla corpus changes.
