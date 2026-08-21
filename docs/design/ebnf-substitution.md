# EBNF locale substitution — `§{…}` keyed rendering

**Status**: design locked 2026-08-20 (operator session); pre-implementation
**Syntax authority**: [`radix/docs/factory/named-template-holes/goal.md`](../../radix/docs/factory/named-template-holes/goal.md) — the marker is the language's named-template-hole syntax; this document owns only its use in docs rendering
**Consumers**: the grammar render pipeline (`faber/docs/EBNF.md` source → locale grammar surfaces; today's hand-maintained `faberlang.dev/generator/grammar/EBNF.{locale}.md` files are its pre-implementation stand-ins)

---

## Model

A locale render **is a labeled template construction**. The grammar document
is one template; a locale glossary is a labeled record keyed by the canonical
Latin keyword; the render applies it once:

```text
ebnf_template(dum: 當, fixum: 定值, …)
```

The renderer implements keyed substitution itself (same algorithm family as
`radix-module/src/forma_render.rs` hole rendering). The **identity pack** maps
every key to itself — the canonical `EBNF.md` is the identity render, and once
the grammar-source migration lands, it becomes reproducible from
template + identity glossary like every other pack.

Why this replaces the status quo: hand-maintained locale grammar files drift.
Evidence (2026-08-20): `EBNF.zh-Hant.md` still lists the retired `adStmt`, predates
the `annotation* statementCore` layer, and spells different frontmatter
terminals than canonical. Generated renders cannot rot.

## The three rules

1. **Region scoping.** Substitution runs only in designated regions. Every
   other byte of the document is inert by construction — no escaping exists
   and none is needed.
2. **Key resolution.** A marker substitutes only when its inner identifier is
   a real glossary key. An unknown key is a render error, not a pass-through.
3. **Zero-residual gate.** Rendered output contains zero `§{` occurrences.
   Any survivor is a finding (unmatched key or stray marker) — never a
   silently shipped artifact.

## Region table

| Region | Substituted? | Why |
| --- | --- | --- |
| Fenced `ebnf` blocks (keyword terminals only) | yes | the locale surface itself |
| Inline code spans in shared prose | yes | keyword mentions in commentary |
| Fenced `fab` example blocks | **never** | live language source — may legitimately contain `§{…}` once named-template-holes ships |
| Plain prose outside code spans | never | inert layer; markers shown *about* the mechanism live here |
| Production identifiers (`dum_stmt`, `fab_file`) | never | cross-locale spine; names are the stable ID, not the localized surface |
| Glyphs (`← ∷ ∪ ⇥ §` …) | never | glyphs never localize |

Substitution inside substituted regions touches **only** keyword spellings —
quoted `'dum'` terminals and keyword mentions — never identifiers, never
glyphs.

## Collision resolution (the load-bearing rule)

Once the language owns `§{name}`, a Faber example can legitimately contain
`"...§{dum}..."` as language syntax. The docs marker and the language marker
share a spelling by design; **region scoping is what separates the two
roles** — the same syntax is docs-meta in a designated region and language in
a `fab` fence. Dictionary membership cannot be the discriminator (a
false-substitution leaves no residual for the gate to catch); region
membership always can. This replaces the earlier `⟦…⟧` de-coupling proposal
and the retired `§§{` escape idea — no escape, no second notation, one
syntax owned by the language.

## Gates

- **Normative triple, fail-on-mismatch:** source template ⇄ identity render
  ⇄ `grammar.jsonl` are normative; any stale member is a spec lie (the
  drift-gate ruling). No degrade path.
- **Locale renders:** generated, zero-residual-checked; a glossary key
  referencing an unknown production ID is an error, not a skip.
- **Site pack rendering** (informative surface) keeps its existing
  degrade-gracefully convention.

## Dependencies and sequencing

1. `named-template-holes` lands first — it owns the marker syntax and its
   language legality. Interim is safe: the marker is reader-invisible under
   the zero-residual gate, and `fab` examples cannot meaningfully use `§{`
   until the compiler assigns it semantics.
2. Substitution spec applies (this document) to locale grammar rendering.
3. Grammar-source dialect migration subsumes the template: `source.fg` +
   sidecars become the template source; this spec's rules carry over
   unchanged.

## Non-goals

- Translating prose — that is the sidecar layer (`sidecar.{locale}.toml`),
  hand-work under every scheme
- Substituting production names or glyphs (never-localized spine)
- The grammar-source dialect itself (separate effort; this spec survives it)
- Any change to the language — this document is a consumer, not an authority
