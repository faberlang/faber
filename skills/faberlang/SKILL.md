---
name: "faberlang"
description: "Build, test and change the code in the Faber repository: the compiler crates (lexer, parser, semantic, HIR, MIR, runner), the faber CLI and package workflow, Norma, locale packs, grammar sources, and the corpus and corpus suite. Use when changing this repository itself. For writing, checking or reviewing .fab source, use the faber skill instead."
---

# Faberlang (maintainer)

Use this skill when the task is building or changing the software in this
repository. Use the `faber` skill (`skills/faber/`) when the task is writing
Faber source: programs, packages, exempla, translations. When a session does
both, keep the modes explicit: repository changes follow this skill;
language-surface questions follow `faber`.

## Status

Faber 1.x is experimental and unstable. The language changes between minor
releases, there is no compatibility promise, and a breaking change does not
need an alias or a migration path to land. This repository takes no outside
pull requests.

The official release binary also contains code generators that are not in this
repository (Rust, Go, TypeScript, Swift, Python, Haskell, LLVM, WebAssembly and
GPU emitters). Do not look for them here and do not describe how they work.

## First decision

| Task | Skill | Home |
| ---- | ----- | ---- |
| Write, check or translate `.fab` source | `faber` | `corpus/`, `lib/norma/` |
| Lexer, parser, syntax tree, diagnostics | here | `crates/radix-lexer`, `radix-parser`, `radix-syntax`, `radix-diagnostics` |
| Semantic analysis, typecheck, HIR lowering | here | `crates/radix-semantic`, `radix-types`, `radix-hir` |
| MIR, the runner, the portable images | here | `crates/radix-mir*` |
| `faber` CLI and package workflow | here | `crates/faber/` |
| Grammar text | here | `grammar/sources/` |
| Norma standard library | here | `lib/norma/{src,exempla,locale,proof}/` |
| Locale packs | here | `locale/<id>/pack.toml`, `lib/norma/locale/` |
| Corpus and corpus suite | here | `corpus/`, `corpus-product/`, `corpus-suites/public/` |

## Build and run

Build and test in this checkout. Do not trust an installed `faber`: installed
binaries go stale and lag the grammar, which shows up as false failures.

```bash
cargo build -p faber                      # product CLI -> target/debug/faber
cargo run -p faber -- check <path>        # run it without installing
target/debug/faber doctor                 # resolved library home and library availability
target/debug/faber targets                # live per-target capabilities of this build
cargo test -p <crate>                     # the owning crate first
cargo test --workspace                    # broad; at closeout
```

Bare `cargo build` builds only the default members (the compiler crates and the
`radix` library facade). The product crate `faber` is reached with `-p faber`.
The `radix` crate is a library only: it has no binary.

`faber` carries single-file phase commands (`lex`, `parse`, `hir`, `mir`,
`cli-ir`, `emit`), so most probes need only that binary. `faber -c '<source>'`
and `faber repl` interpret one-liners. Read diagnostics with
`faber check --diagnostics` or `faber explain <CODE>[.<issue>]`; the default
output is the compact code-first identity.

The binary embeds the locale packs, the `faber explain` reference (corpus
exempla whose frontmatter lists `explains`), the runtime support sources, and
Norma (`lib/norma/src`, `lib/norma/locale`, manifests). After editing any of
those, rebuild `faber` before judging behavior.

## Crate map

All crates live under `crates/` unless noted; the workspace is `Cargo.toml` at
the root, and clippy pedantic is denied workspace-wide.

| Crate | Owns |
| ----- | ---- |
| `radix-lexer` | tokens, keywords, spans-adjacent lexing, interner |
| `radix-syntax` | the parsed syntax tree |
| `radix-parser` | recursive-descent parser and recovery |
| `radix-diagnostics` | diagnostic transport types and source spans |
| `radix-types` | semantic type model, `TypeTable`, `DefId` |
| `radix-intrinsics` | compiler-owned receiver intrinsic catalog |
| `radix-semantic` | collect, resolve, AST to HIR lowering, typecheck, post-typecheck analyses, the diagnostic catalog, reader-locale loading |
| `radix-hir` | HIR nodes and visitors |
| `radix-mir`, `radix-mir-lower` | MIR model and validation; HIR to MIR lowering |
| `radix-mir-runner` | the in-process MIR interpreter behind `faber run` and `faber test` |
| `radix-mir-fmir` | the portable MIR package image (FMIR) |
| `radix-hir-fhir` | the portable analyzed-HIR artifact (FHIR) |
| `radix-hir-faber` | the HIR-backed Faber source emitter used by `format` and `convert` |
| `radix-hir-c` | the C emitter |
| `radix-mir-clif`, `runtime/clif` (package `faber-host-clif`) | the Cranelift backend and the runtime a CLIF-compiled program links |
| `radix-target` | compilation-target identity, spelling, feature gating |
| `radix-codegen-shared` | shared text-emit utilities |
| `radix-runtime-contract`, `radix-abi` | the runtime contract and host ABI table |
| `radix-module` | the compiler driver: phase ordering, target dispatch, services; re-exports the leaves |
| `radix-program` | multi-file package analysis and artifact assembly |
| `radix` | thin library facade over `radix-module` and `radix-program` |
| `faber` | the CLI and package workflow (`src/commands/`, `src/package/`) |
| `faber-leaf` | native effect leaves (fs, stdio, process, clock, entropy, tcp, http, websocket) |
| `proba` | proba inventory and MIR-runner case execution (`faber test`) |
| `handroll` | std-only, zero-dependency primitives (entropy, sha256, the `Magnus` bignum) |
| `radix-hygiene` | the production-code hygiene budget scanner used by each crate's `tests/hygiene.rs` |

Crate layout moves. Search the live tree before trusting this table;
`references/source-map.md` lists first stops per concern.

## Pipeline

Lex, parse, then in `radix-semantic`: collect, resolve, lower to HIR, typecheck,
and the post-typecheck analyses (air-unroll, air-purity, borrow,
definite-assignment, exhaustiveness, return-path, lint; the registry in
`radix-semantic/src/passes` is the authority). Collection, resolution and
lowering errors stop the pipeline; typecheck and later passes collect into one
result. Backends take HIR or MIR from there, and `radix-module/src/driver/`
sequences it. Radix lowers and serializes; formatting, linting and package
workflow belong to `faber`.

## Grammar, locales, Norma

- Grammar sources are `grammar/sources/source.fg`, `sidecar.en.toml`,
  `prose.en.md`. `grammar/EBNF.md`, `grammar/EBNF.<locale>.md` and
  `grammar/sources/grammar.jsonl` are generated: never edit them by hand.
- Locale packs are `locale/<id>/pack.toml` for `ar`, `en`, `hi`, `la`, `th-TH`,
  `vi`, `zh-Hans`, `zh-Hant`. `en` and `la` change together in one change; the
  other packs follow as proposals. Library member rows live beside the library
  (`lib/norma/locale/{en,la}`).
- Norma is `lib/norma/src/`, with exempla in `lib/norma/exempla/` and proofs in
  `lib/norma/proof/`.

## Reader-locale contract

Every source file is assigned its reader `LocalePack` before lexing, per file:
an explicit override (`check --locale`, `convert --from`) wins, then the file's
frontmatter `locale`, then the package manifest, then the product default `en`.
`faber convert --to <LOCALE>` selects only the target pack and never becomes
the source pack of an untagged input. Package selection lives in
`radix-program/src/source_files.rs`; the product default and embedded pack
loader are in `crates/faber/src/package/locale.rs`; the Faber re-emit seam is
`reemit_faber` in `radix-module/src/driver/faber_format/compile.rs`.

## Diagnostics

Diagnostics are identified by code, issue and named arguments, never by prose.
A new issue is a catalog entry in `radix-semantic/src/catalog*.rs` plus message
and help rows in the packs (`locale/<id>/pack.toml`,
`[diagnostics.CODE.issues.<issue>]`), `en` and `la` together. The
`*_pack_completeness_test.rs` tests in `radix-module/src/` enforce completeness.
Tests assert code, issue and args; the hygiene test
`non_renderer_tests_do_not_assert_diagnostic_text` (`crates/radix-hygiene/tests/`)
fails a test that matches diagnostic text.

## Corpus and the public suite

- `corpus/<term>/<name>.fab` are single-file exempla (no `faber.toml`); each has
  a `<name>.expected` stdout golden. Frontmatter is TOML between `+++` lines
  (`term`, `explains`, `kind`, `grammar`, `category`, `summary`, `syntax`,
  `related`, `locale`). A file owns the `faber explain` terms it lists, and each
  term has exactly one owner.
- `corpus-product/<name>/` are packages with a `faber.toml`.
- `corpus-suites/public/` is the verifier suite: `corpus.toml` holds the target
  recipes and default expectations, and `cases/` holds one `case.toml` per case.

```bash
target/debug/faber corpus corpus-suites/public --target runner
target/debug/faber corpus corpus-suites/public --target roundtrip
target/debug/faber corpus corpus-suites/public --target runner --filter <substring>
target/debug/faber corpus corpus/<term>/<name>.fab      # one file against its .expected
```

Targets in `corpus.toml` are `runner`, `roundtrip` and `mir`. A pass is
`failed: 0`. The run log is JSON lines and large: redirect it to a file and
parse it, never print it.

Add an exemplar:

1. Write `corpus/<term>/<name>.fab` and its `.expected`.
2. Add `corpus-suites/public/cases/language/<term>/<name>.fab/case.toml`
   (`id = "corpus/<term>/<name>.fab"`, `group = "language"`,
   `input_root = "corpus"`, `input = "<term>/<name>.fab"`,
   `[expect] stdout = "stdout.expected"`, `check_only = false`) and a
   `stdout.expected` identical to the `.expected`.
3. A negative exemplar uses `[expect] check_only = true` and a
   `[targets.<name>]` table per target with `outcome = "rejects"`, `phase` and
   `diagnostic_contains = "<CODE>.<issue>"`.
4. Run the three targets above with `--filter <term>`.

Product cases (`group = "product"`, `input_root = "product"`, `input = "<name>"`)
are check-only in this suite. The case outcomes are `pass`, `known-fail`,
`not-applicable` and `rejects`; a failing case is a regression or a stale
expectation, never something to silence.

## Compiler rules

- Grammar is law: check `grammar/EBNF.md` and the nearest `corpus/` exempla
  before inventing a rule. Preserve explicit syntactic distinctions in the AST.
- Malformed input yields diagnostics and forward motion, never a panic or hang.
- Production code obeys the hygiene budgets: each crate's `tests/hygiene.rs`
  gives monotonic ceilings for `unwrap`, `expect`, `panic!`, `todo!` and
  similar, and requires tests in companion `_test.rs` files, not inline modules.
  Lowering a budget is fine; raising one needs a stated reason.
- Fix the phase that is wrong. A later phase compensating for an earlier one is
  a defect.
- Width limits apply only at the store: arithmetic runs unbounded, and a
  `wrapping`, `saturating` or `trapping` policy is applied once where the value
  lands in a sized cell. Never clamp or trap per operation.
- The MIR runner is an interpreter, not a device executor: `faber run` and
  `faber test` refuse functions with compute-stage `@ kernel` metadata with
  `runner_kernel_refused`.
- A target that cannot express a feature rejects it by name; the language is not
  narrowed to fit a target.
- No Bun or Node in product work; Cargo only.
- Tests prove the target. Never weaken a test to get green.
- When this skill or any prose disagrees with the code, the code wins; fix the
  prose.

## Retired surfaces (do not reintroduce)

These are named parse or semantic diagnostics in the tree: wrapped numerics such
as `numerus<W>` (`PARSE040 numeric_wrapper_retired`); `static` and `generis`
(`PARSE010 static_decl_retired`, `static_member_retired`); the old binding
spellings (`PARSE010 runtime_const_retired`, `var_retired`,
`binding_type_slot_required`, `binder_fixum_retired`; `ref_binder_refused`,
`ref_field_refused`); unmarked genus fields (`field_modifier_missing`); the
callable transfer sink (`SEM012 transfer_sink_retired`); `discerne` over open
scrutinees (`SEM010 discerne_pattern_not_in_closed_set`). Run-time names are
`let`, `mut`, `ref`; `const` is compile-time only and takes `=`.

## Language-change checklist

A language change is more than compiler code. Touch, as applicable:

1. `grammar/sources/` (the generated grammar files follow, never by hand).
2. `corpus/` exempla with `.expected`, negatives for new diagnostics, and the
   matching `corpus-suites/public/cases/` entries.
3. Locale packs: `en` and `la` together.
4. The formatter round trip in `radix-hir-faber`, proven by the `roundtrip`
   target above.
5. The `faber` and `faberlang` skills.

## References

- `references/source-map.md` — first stops per concern, lookup and grep hints
- `references/compiler-work.md` — parser, AST, diagnostics and lowering work: stance, workflow, review format
- `references/compiler-source-presentation.md` — charters, roadsigns and rationale comments

Read only the reference the task needs, then inspect the live paths.
