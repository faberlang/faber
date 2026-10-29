# Compiler Work

Use this reference for parser, syntax tree, diagnostics, recovery, semantic,
lowering or MIR work in this repository, not for writing `.fab` application
code.

The compiler driver is `crates/radix-module`; semantic analysis is
`crates/radix-semantic`. The lexer, parser, syntax tree, types, HIR, MIR and
backends are their own crates (see the crate map in `SKILL.md`). Follow the
live tree instead of a memorized list.

Co-authorities: `grammar/EBNF.md` (generated from `grammar/sources/`),
`corpus/`, and the charters at the top of each source file.

## When to use

- clean up recent parser, syntax or diagnostics work
- tighten code after a language feature landed in the compiler
- review compiler changes for grammar fidelity and mechanical correctness
- fix syntax-phase bugs without losing the visible structure of the grammar
- add or review file charters, roadsigns or rationale comments

Typical first stops: `crates/radix-parser/src/`, `radix-syntax/src/`,
`radix-diagnostics/src/`, `radix-lexer/src/`, `radix-semantic/src/passes/`; go
to `radix-mir-lower` or `radix-module` only when the root cause or the cleanup
boundary demands it.

Not for: broad hygiene unrelated to compiler mechanics, architecture critique
disconnected from current code, or generic style cleanup outside phase
boundaries.

## Core stance

The language is mechanical on purpose; the compiler should honor that.

- Grammar is law. Check `EBNF.md` and `corpus/` before inventing a rule.
- Preserve explicit syntactic distinctions; do not collapse node shapes early
  because downstream code could recover the meaning.
- Keep parse decisions local when the grammar allows it. Prefer predictive
  clarity over clever inference.
- Recovery is part of correctness. Malformed input produces diagnostics and
  forward motion, not a panic, a hang, or silently dropped structure.
- Diagnostics are code-first: emit `code`, `issue` and named args, and leave
  prose to the reader packs. The default CLI prints the message and help; `--no-diagnostics` selects the
  compact identity, and `faber explain` provides catalog detail.
- A new diagnostic issue is a catalog entry (`radix-semantic/src/catalog*.rs`)
  plus message and help rows in `locale/<id>/pack.toml`
  (`[diagnostics.CODE.issues.<issue>]`), `en` and `la` in the same change. The
  `*_pack_completeness_test.rs` tests in `radix-module/src/` enforce it. Tests
  assert code, issue and args, never prose.
- Make phase boundaries, recovery points and invariants visible in the code.
- Refactor only when it sharpens structure; do not hide grammar truth behind
  abstraction.

## Mechanical biases

- keyword-led constructs are a feature, not noise
- early syntactic signaling stays visible in parser code
- type-first syntax stays explicit in AST shape and parser helpers
- node shape mirrors what the source made unambiguous
- spans, node IDs and attached annotations are real compiler data
- fallback guessing in a later phase is usually evidence an earlier phase got
  lazy

If a cleanup makes code shorter but blurs one of these, reject it.

## Workflow

### 1. Orient to the construct

Read the grammar and the nearest `corpus/` exempla before touching code. Inspect
the exact recent change and its neighbors. Name the phase boundary (lexing,
parsing, syntax tree, diagnostics, semantic, HIR, MIR, lowering) and the
invariants that should hold there before editing.

### 2. Check mechanical fidelity

Which token announces the construct? Where does disambiguation happen? What
must survive into the syntax tree? Which spans, annotations or node IDs stay
attached? If parsing fails halfway, where does synchronization resume? Is a
later phase compensating for an avoidable syntax-phase blur? Start from the
construct, not the implementation.

### 3. Fix the real boundary

- grammar misunderstandings belong in parser logic or tree shape
- missing source facts belong in syntax structures, not downstream guesswork
- weak diagnostics belong in the diagnostics path, not in comments
- recovery bugs belong in synchronization and control flow, not scattered guards

If the root cause is structural but the safe fix is small, make the smallest
honest correction and say plainly what design debt remains.

### 4. Shape the code

Give helpers one syntactic job each. Keep the happy path visible. Use guard
clauses to expose failure edges. Align sibling branches that correspond to
sibling grammar forms. Extract only helpers that map to named grammar concepts.
Keep token movement obvious; hidden advancement is a footgun.

### 5. Present the source

Follow `references/compiler-source-presentation.md` after correctness and
structure are settled. A routine local change reviews the touched charter and
adjacent rationale only.

### 6. Validate

Smallest proof first, then widen:

- targeted `_test.rs` files in the owning crate
- `cargo test -p <owning-crate>`
- `faber parse|check|emit` on `corpus/` files that exercise the construct
- `faber corpus corpus-suites/public --target runner` and `--target roundtrip`
  when behavior crosses phases or the formatter

Validate both directions: valid exempla still flow through, and malformed input
reports the right diagnostics and continues safely.

## What to inspect

- token consumption and lookahead clarity
- synchronization boundaries
- syntax node shape and whether fields pay rent
- span coverage, annotation attachment, node identity
- source distinctions lost during parsing
- parser loops that may fail to advance
- diagnostics precision
- ad hoc downstream recovery for upstream syntax mistakes

## Red flags

- token advancement that is hard to audit
- helpers named after implementation accidents instead of grammar concepts
- syntax variants merged because the parser wanted less work
- recovery that can stall, recurse badly or skip too far
- diagnostics that describe the implementation's confusion
- later phases inferring facts the parser already knows
- formatting-only cleanup that obscures token flow or invariants
- generic abstractions that erase the rhythm of the grammar

## Review output

Findings first, in this order:

1. Mechanical problem: the one compiler boundary that is wrong or messy.
2. Findings: flat bullets by severity.
3. Cleanup plan: the exact parser, tree, diagnostics or recovery changes.
4. Validation: what ran and what still needs to.
5. Residual risk: only if something important remains.

For implementation, report the boundary corrected, the shape pruned and how it
was validated.

## Success criteria

The code matches the grammar more directly than before; the tree preserves the
distinctions the source made explicit; malformed input behaves better, not
merely differently; diagnostics are more precise; token flow is easier to
follow; the file charter stays truthful and stable.
