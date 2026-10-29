# Faber Locales and Locale Packs

Use this reference before deciding that two Faber files use different language
versions. Locale is a compile-time reader surface over shared grammar and
semantics. The locale changes accepted and emitted spellings; it does not create
a second parser grammar or runtime language.

## Shipped Packs

Eight reader packs ship under `locale/<id>/pack.toml` and are embedded in
the `faber` binary: `en` (product default), `la`, `zh-Hans`, `zh-Hant`, `hi`,
`ar`, `th-TH`, `vi`. Every one is a full source-reader pack (keywords, types,
`[llm]`, diagnostic templates); none is diagnostics-only. Each file is read in
exactly one locale: the packs are sealed, so a Latin keyword inside an `en` or
`zh-Hans` file is rejected, and no shipped pack sets `fallback` or `inherits`.

Differences that matter when writing source:

- `en` and the six translated packs map every intrinsic row (`[intrinsics]`).
  `la` has no `[intrinsics]` table: Latin uses the canonical registry names.
- Diagnostic prose is complete in `en` and `la` only. The translated packs
  carry fewer diagnostic tables and issue rows; a missing row renders the
  catalog message with a `[locale pack missing row: ...]` marker.
- Library member rows (`<lib>/locale/<id>/pack.toml`) exist for `norma` (en,
  la); libraries outside this repo carry their own rows or none. The translated
  packs have no member rows, so `text.reverse` stays `reverse` in a `zh-Hans`
  file; the Latin surface `retorta` is accepted only in `la`.
- Locale sealing covers type names: Latin is a pack like any other, with no
  canonical-Latin fallback. A Latin type word in a non-Latin file (la
  `numerus`, `textus`, `lista`, or `nihil` as a type, in place of `int`,
  `string`, `list`, `none`) is `PARSE040 wrong_locale_type` (probe: an `en`
  file with `let numerus x ← 1`), and the help names the file locale's
  spelling. Use the pack's `[types]` row. User-declared or imported
  names that look like Latin type words are not rejected.
- The null value and the null type are separate words in every pack: in
  `en` the literal is `null` and the type is `none`; in `la` they are
  `nulla` and `nihil`. The translated packs spell them from their own rows (probe:
  `zh-Hans` `缺失` value, `无` type).
- Width markers (`i8`..`u64`, `f16 f32 f64 bf16`) and the unbounded integer
  `inf` are bare and locale-invariant in every pack; they are not pack rows.
  The wrapped forms `int<i32>`, `int<32>` and the hole `int<_>` (la
  `numerus<i32>`, `numerus<_>`) are retired: `PARSE040 numeric_wrapper_retired`.
- The convert-hint word `via` (`x ↦ u32 via Hex`) is an ordinary keyword row in
  each pack; the type-argument hint form is retired.

## Mental Model

The compiler keeps stable Latin-keyed lexical identities (for example
`functio`, spelled `fn` in `en`). A locale pack maps those identities to source
spellings:

```toml
# locale/en/pack.toml
[keywords]
functio = "fn"
redde = "return"
incipit = "main"
nota = "print"

[types]
textus = "string"
numerus = "int"
```

The key is the stable identity. The value is the spelling accepted for that
locale. The Latin pack generally maps each identity to itself.

These two fragments therefore use different locale surfaces, not different
Faber versions:

```fab
+++
locale = "en"
+++

fn echo(string value) → string {
  return value
}

main {
  print echo("hello")
}
```

```fab
+++
locale = "la"
+++

functio echo(textus value) → textus {
  redde value
}

incipit {
  nota echo("salve")
}
```

User identifiers such as `echo` and `value`, string contents, structural
glyphs such as `←` and `→`, and import coordinates such as `norma:text`
are not translated merely because the code locale changes.

`grammar/EBNF.md` writes terminals with the stable Latin keys. Treat an EBNF row
such as `'functio'` as the grammar identity. Under the `en` pack, its accepted
surface spelling is `fn` (la `functio`).

## Find the Active Code Locale

Do not use one universal precedence claim without naming the route. Current
Faber and Radix paths select packs at different layers.

### Explicit CLI selection

`faber check --locale <id>` selects the code locale for checking. Source
**conversion** between locale surfaces is its own command — `faber convert`
(not `format`; formatting reflows syntax, converting translates the surface):

```bash
faber check --locale en <package-or-file>
faber check --locale la <package-or-file>
faber convert <file> --to la --stdout    # re-emit in the Latin surface
faber convert <dir> --to zh-Hans         # in-place, every .fab under the dir
faber convert <file> --to en --check     # report pending conversions only
faber convert <file> --from en --to la --stdout   # force the source pack
```

`--locale` needs a package path or a `.fab` file; it is rejected for stdin.
`faber check -` (stdin) has no package context: with no frontmatter it reads
**Latin**, not `en` (probe: `main { print "ok" }` fails on stdin, the same
program with the Latin entry word `incipit` (en `main`) passes), so give stdin documents a frontmatter `locale`.
`faber check <file>` without frontmatter or manifest reads `en`.

`--to` is required and fails closed listing the embedded pack locales for
unknown values (`faber check --locale zz` fails the same way). Conversion is the HIR round-trip: parse → analyze → re-emit under the
target pack, idempotent, no format policy applied.

Inspect live `faber <command> --help` before using a flag on a different
command; option shapes differ between commands.

### Package manifest

Write the canonical table in the nearest `faber.toml`:

```toml
[locale]
locale = "en"
```

Latin source must say:

```toml
[locale]
locale = "la"
```

`[reader]` is a legacy alias that is still accepted in the current manifest
deserializer. Read it when investigating existing packages, but write
`[locale]` in new work.

A package can pin a custom pack path relative to the package root:

```toml
[locale]
locale = "en"
pack = "locale/en.toml"
```

For ordinary `faber` package selection, the live order is:

1. CLI `--locale` (wins for every file, including files with frontmatter).
2. The file's own frontmatter `locale` (per file; not validated against the
   manifest, so one package can mix `en` and `la` files).
3. Manifest `[locale].locale`.
4. Product default `en`.

Probe: a package pinned `la` whose `main.fab` declares `locale = "en"` and
imports a `locale = "la"` sibling checks and runs; `check --locale en` on the
same package then rejects the sibling's `@ publica` (en `@ public`) with
`PARSE018:wrong_locale_annotation`.

After choosing the id, pack-path resolution checks the applicable manifest
`pack`, then package-local `locale/<id>.toml`, then the embedded language pack.
The source-tree authority is `locale/<id>/pack.toml`. Library member
rows come from `<lib>/locale/<id>/pack.toml` at import time.

### File frontmatter

Frontmatter is TOML between `+++` delimiters. It must begin on line 1:

```fab
+++
locale = "en"
+++

fn greet() {
  print "Hello"
}
```

The Radix driver resolves file frontmatter before lexing. With no explicitly
selected CLI pack, file frontmatter overrides the session pack. An explicit
CLI locale wins over frontmatter. Imported library interfaces are read under
their own file frontmatter first, then their own package manifest, rather than
blindly inheriting the consumer's code locale.

The product source-reemission routes make the distinction explicit:

- `faber format`: file frontmatter → package manifest → product default `en`;
- `faber convert`: `--from` → file frontmatter → package manifest → `en`;
- `faber convert --to <id>`: one command-wide target pack, independent of the
  source pack selected for each file.

Conversion requires a complete source unit. For a local declaration, use a
literal wrapper such as:

```fab
main {
    let list<u32> values ← empty
}
```

An untagged stdin document has no manifest context and therefore reads as
English unless `--from` or frontmatter selects another pack:

```bash
printf 'main { print "ok" }\n' | faber convert --stdin --to la
faber convert source.fab --from en --to la --stdout
```

Conversion output declares its target by replacing or inserting the
frontmatter `locale` key; unrelated frontmatter metadata is retained.

A frontmatter `locale` that is unknown or not a string fails the file with
`PKG001` (`frontmatter_locale_type` for a non-string); it does not fall back.

Package and format routes have their own selection plumbing. When exact
precedence matters, inspect these live owners:

- `crates/faber/src/package/locale.rs`
- `crates/radix-program/src/source_files.rs`
- `crates/faber/src/commands/format.rs`, `commands/convert.rs`
- `crates/faber/src/package/embedded_locale.rs` (the embedded pack ids)
- `crates/radix-module/src/driver/mod.rs::resolve_peeled_lex_pack`
- `crates/radix-module/src/program/compile.rs` (package-aware compile + locale-pack threading)

This route check matters because an old design document
may describe intended behavior that differs from the current caller.

## Code Locale Is Not Diagnostics Locale

The code locale selects keyword, type, intrinsic, annotation, and translated
library-member spellings for lexing and semantic resolution.

The diagnostics locale selects message templates. On `faber`, use
`--diagnostics-locale <id>` (`check`, `explain`, `emit`); when omitted, the
code pack's templates render. Probe: `faber check f.fab
--diagnostics-locale zh-Hans` renders `PARSE040` in Chinese for an `en` file.

Never infer source syntax from the language of an error message. A Latin source
surface can have English diagnostics, and a build can select diagnostic prose
independently from its code surface (the `la` pack's own diagnostic prose is
English).

## Pack Structure

The live loader is `crates/radix-semantic/src/locale.rs`. Installed source packs
are TOML files under `locale/<id>/pack.toml`. Library member rows live in
`<lib>/locale/<id>/pack.toml` (for example `lib/norma/locale/la/pack.toml`).

### `[pack]`

```toml
[pack]
id = "en"
schema_version = 1
```

The id must match the selected locale. The loader also models `inherits` and
`fallback`; only `la` sets one (`fallback = []`, an explicit empty list), so
every shipped pack is pack-authoritative: a spelling needs a declared row and
there is no implicit Latin acceptance. Inspect the live loader before relying
on inheritance or fallback in new work.

### `[keywords]` and `[types]`

These map stable Latin keys to localized source spellings. A value may be a
single string or a row with `canonical` and `aliases`. Do not reverse the map.

```toml
[keywords]
functio = "fn"

[types]
textus = "string"
```

The row form is part of the loader schema (one shipped use: an `en`
intrinsic row), but only add an alias when the locale contract intends to
accept both spellings. Do not invent fallback
aliases merely to make a fixture pass.

The stable NaN literal identity (`nonnumerus`) is the `[keywords]` row in every
pack: every shipped pack except the Latin one spells it `nan`, and the Latin
(`la`) pack spells it `nonnumerus`. The `∞` literal is a locale-invariant glyph and has no
pack row (it is the float infinity, not an `inf` value); do not treat either
named spelling as a cross-pack alias — only the selected pack's spelling
claims the literal slot.

### `[intrinsics]`

Intrinsic keys are stable registry method names and values are localized
member spellings:

```toml
[intrinsics]
longitudo = "length"
continet = "contains"
```

Semantic lookup applies these mappings within receiver families. The same
surface can have meaning only after the receiver type is known. The `en` and
all six translated packs own a row for every registered intrinsic (a
completeness test iterates the registry); `la` has no table and uses the
canonical names.

### `[[library_members]]`

Library API translation is identity-scoped, not a global lexer alias. Example
row from the Latin Norma pack (`lib/norma/locale/la/pack.toml`):

```toml
[[library_members]]
provider = "norma"
module_path = ["text"]
kind = "function"
canonical = "reverse"
surface = "retorta"
```

Rows can also include `owner` for methods or other owned members. Resolve the
provider, package, module, owner, member kind, and canonical member before
projecting the locale surface. Import coordinates and caller-chosen import
aliases remain unchanged.

Libraries ship locale fragments at `<lib>/locale/<id>/pack.toml`. The compiler
merges those rows into the consumer reader pack when the library is imported.

### Annotation spelling is sealed too

Annotation names are reader-surface spellings, not a locale-neutral escape hatch.
Use the registered annotation value from the selected pack. For example, an
English device source uses `@ kernel`; copying the Latin identity `@ nucleum`
into that file is rejected with `PARSE018:wrong_locale_annotation` by the live
binary. The same rule applies to annotation modifiers such as the shared-field
surface (`@ shared` in the English `commune` exemplar). Resolve the file locale
before deciding whether an annotation is legal, and do not infer the source
locale from the diagnostic language.

Probes against the live binary: an `en` `@ kernel` is accepted by the parser and
the Latin `@ nucleum` is `PARSE018:wrong_locale_annotation`; a `zh-Hans`
`@ nucleum` is the same `PARSE018` and `@ 内核` is accepted; in `la` the English
`@ kernel` is not a wrong-locale case but `SEM018:annotatio_unknown_contract`
(an unregistered annotation name). The shared-field annotation is `@ shared` in
`en`; every other pack spells it `@ commune`.

Closure-modifier spellings are sealed the same way. The signature slot after
the closure parameters admits only the selected pack's declared spelling —
`kernel` in an `en` file, `nucleum` under `la` — with no cross-pack fallback
(an `en` file with the Latin `nucleum` is a plain `PARSE001`), and a second
modifier in the slot is `PARSE019` duplicate modifier, not an alias. The same
sealing law covers the `@ kernel` (la `@ nucleum`) named-function annotation
above; the closure modifier is its expression-form twin.

### `[diagnostics.*]`

Diagnostic tables are keyed by stable codes and optional structured issues:

```toml
[diagnostics.LOCALE004]
message = "identifier '{identifier}' mixes scripts: {scripts}"
help = "check for look-alike characters from another alphabet; the name is accepted as written"
```

Templates render structured compiler facts. They do not change parsing or
semantics. The locale-specific advisories are warnings, never errors:
`LOCALE002` (a spelling close to a keyword of the active pack; the name stays
valid) and `LOCALE004` (an identifier mixing scripts; probe: a Cyrillic `а`
inside a Latin name in a `zh-Hans` file).

### `[llm]`

```toml
[llm]
system_prompt_snippet = "Write Faber using the selected locale surface."
exemplars = ["./exemplars/salve-munde.en.fab"]
```

These artifacts teach code generators the same surface the compiler accepts.
Exemplar paths are relative to the pack and are validated when file-backed
packs load. Verify that each exemplar declares an existing locale id and uses
that pack's actual spellings.

## Syntax-or-Locale Diagnostic

When source appears to have “shifted,” use this order:

1. Read the file's first frontmatter block.
2. Locate the package's `faber.toml` and read `[locale]` or `[reader]`.
3. Recover the exact command and check for `--locale`.
4. Resolve the selected pack path.
5. Look up the disputed word as a stable key and as a surface value in
   `[keywords]`, `[types]`, `[intrinsics]`, and `[[library_members]]`.
6. Compare a same-locale corpus exemplar.
7. Run the smallest explicit check under the suspected locale.
8. Only then inspect `EBNF.md`, parser changes, release notes, or Git history
   for a real language-version change.

Useful bounded probes from the repo root:

```bash
sed -n '1,20p' path/to/source.fab
rg -n '^\[locale\]|^\[reader\]|^locale\s*=|^pack\s*=' path/to/faber.toml
# keys are the Latin identities; the en values are fn, return, string
rg -n '^functio\s*=|^redde\s*=|^textus\s*=' locale/{en,la}/pack.toml
```

Do not “fix” a mismatch by changing keywords until the locale is known. The
correct repair may be a missing or wrong locale declaration, a missing pack, a
stale exemplar, an incorrect CLI override, or an actual syntax change. Name
which one the evidence proves.

## Live Authorities

- Grammar identity and frontmatter syntax: `grammar/EBNF.md`
- Installed pack data: `locale/` (language); `<lib>/locale/` (library members)
- Pack schema and validation: `crates/radix-semantic/src/locale.rs`
- Frontmatter parser: `crates/radix-module/src/driver/file_frontmatter.rs`
- Frontmatter pack selection: `crates/radix-module/src/driver/mod.rs`
- `faber.toml` locale fields: `crates/faber/src/package/manifest.rs`
- Faber package selection and default: `crates/faber/src/package/locale.rs`
- Cross-package locale resolution: `crates/radix-module/src/program/compile.rs`
- Library-member locale resolution: `crates/radix-program/src/library.rs` (cache + `LocaleLoader`), `load_library_locale_pack` in `locale.rs`
- Package source selection: `crates/radix-program/src/source_files.rs::resolve_locale_pack`
- Embedded pack set: `crates/faber/src/package/embedded_locale.rs`
- Checked source examples: `corpus/` and each file's frontmatter
