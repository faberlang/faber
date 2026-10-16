# Source Map

Where to look next in this repository. Layout moves; verify a path with `ls`
or `rg` before relying on it.

## Top level

| Path | Contents |
| ---- | -------- |
| `crates/` | the Rust workspace (compiler, runner, CLI, support crates) |
| `lib/norma/` | the Norma standard library: `src/`, `exempla/`, `locale/`, `proof/`, `faber.toml`, `cista.toml` |
| `locale/` | language reader packs, one directory per locale, each with `pack.toml` |
| `grammar/` | generated EBNF files and `sources/` (the editable grammar sources) |
| `corpus/` | single-file language exempla with `.expected` goldens |
| `corpus-product/` | package fixtures with `faber.toml` |
| `corpus-suites/public/` | the verifier suite: `corpus.toml` and `cases/` |
| `runtime/c/`, `runtime/clif/` | the C runtime header and the CLIF host runtime crate |
| `skills/` | the `faber` and `faberlang` skills |

## Authoritative sources

| Surface | Path |
| ------- | ---- |
| Grammar source | `grammar/sources/{source.fg,sidecar.en.toml,prose.en.md}` |
| Grammar (generated, do not hand-edit) | `grammar/EBNF.md`, `grammar/EBNF.<locale>.md`, `grammar/sources/grammar.jsonl` |
| Language exempla | `corpus/` (`explains = [...]` frontmatter owns `faber explain` terms) |
| Package fixtures | `corpus-product/` |
| Diagnostic catalog | `crates/radix-semantic/src/catalog*.rs` |
| Diagnostic text | `locale/<id>/pack.toml`, `[diagnostics.CODE.issues.<issue>]` |
| CLI | `crates/faber/src/` (`cli/mod.rs`, `commands/`, `package/`) |
| Verifier command | `crates/faber/src/commands/corpus.rs` and `commands/corpus/` |
| Norma | `lib/norma/src/` |

## Compiler first stops

| Concern | First stop |
| ------- | ---------- |
| driver and pipeline | `crates/radix-module/src/lib.rs`, `crates/radix-module/src/driver/` |
| lexer, keywords | `crates/radix-lexer/src/` |
| parser | `crates/radix-parser/src/` |
| syntax tree | `crates/radix-syntax/src/` |
| diagnostics transport | `crates/radix-diagnostics/src/` |
| semantic passes, typecheck | `crates/radix-semantic/src/passes/` |
| AST to HIR lowering | `crates/radix-semantic/src/lower/` |
| reader locale, CLI IR, file interfaces | `crates/radix-semantic/src/{locale,cli,file_interface}.rs` |
| type model | `crates/radix-types/src/` |
| intrinsics | `crates/radix-intrinsics/src/` |
| HIR | `crates/radix-hir/src/` |
| MIR lowering | `crates/radix-mir-lower/src/` |
| MIR model and validation | `crates/radix-mir/src/` |
| MIR runner (`faber run`) | `crates/radix-mir-runner/src/` |
| FMIR / FHIR images | `crates/radix-mir-fmir/src/`, `crates/radix-hir-fhir/src/` |
| Faber source emitter | `crates/radix-hir-faber/src/`, entry `radix-module/src/driver/faber_format/compile.rs` |
| C emitter | `crates/radix-hir-c/src/` |
| Cranelift backend | `crates/radix-mir-clif/src/`, runtime `runtime/clif/src/` |
| package analysis | `crates/radix-program/src/` |
| proba (`faber test`) | `crates/proba/src/` |

Target names and per-target support are live facts: run `faber targets`.

## Package and CLI

| Concern | Path |
| ------- | ---- |
| subcommand definitions and flags | `crates/faber/src/cli/mod.rs` |
| command implementations | `crates/faber/src/commands/` |
| package build, check, run | `crates/faber/src/package/` |
| explain and the embedded reference | `crates/faber/src/explain.rs`, `reference.rs`, `crates/faber/build.rs` |
| embedded reader packs and locale selection | `crates/faber/build.rs`, `crates/faber/src/package/locale.rs` |
| library home resolution | `crates/faber/src/library.rs` |

## Lookup strategy

Syntax question: `grammar/EBNF.md`, then the matching `corpus/` exemplar, then
the live parser.

"How is this written in Faber": the relevant exemplar, adjacent exempla for
style, then `EBNF.md` for legality. Or run `faber explain <term>`.

Compiler-owned collection methods: `crates/radix-intrinsics/src/`, then
`corpus/intrinseca/`.

A diagnostic: search the code and issue name across `crates/radix-semantic/src`
and `locale/en/pack.toml`.

## Grep hints

```sh
rg -n "functio|genus|discretio|discerne|elige" corpus
rg -n '^explains = ' corpus
rg -n "parse_discerne|parse_elige|parse_func|parse_class" crates
rg -n "numeric_wrapper_retired" crates locale
```
