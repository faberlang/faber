# Faber

Faber is a programming language designed to be written by language models and
read by people. It is built on a small number of uniform, mechanical rules, so
that a model can predict what a program means without importing the habits of
whichever language it saw most in training. It compiles to many targets, from
a reference interpreter and readable C to Rust, Go, TypeScript and GPU kernels.

This README is written for people who design and implement languages. The short
version of what is different:

- **One glyph, one meaning.** `←` stores, `→` returns, `⇥` is the error channel,
  `↦` converts, `≡`/`≢` compare, `·` is the matrix product. `=` is never
  assignment and never a run-time comparison.
- **Math happens in the ether; width applies at the store.** Arithmetic is
  exact for the whole expression. A width limit (`wrapping`, `saturating`,
  `trapping`) is applied once, where the value lands in a sized cell.
- **Mathematics over hardware tradition.** `/` and `%` floor, ties round to
  even, raw bits are their own domain and never silently numbers.
- **Explicit everything.** Type-first declarations, nullability as a union
  (`int ∪ none`), conversions that name how they round or what they fall back
  to, failure as a typed second return channel.
- **Many targets, one language.** The targets implement Faber; Faber does not
  bend to its targets. A target that cannot express a feature rejects it by
  name.

## What it looks like

Every program below runs on the reference runner in this repository
(`faber run`); the output is shown after each.

### Tensors and shapes in the type

Outer product `⊗`, matrix product `·`, element-wise product `⊙` and transpose
`ᵀ` are glyphs, and shapes are part of the type: a mismatch is a compile error
(`SEM011 glyph_hadamard_shapes_mismatch`), not a run-time surprise.

```fab
main {
    const vf32[3] a ← [1.0, 2.0, 3.0] ↦ vf32[3]
    const vf32[2] p ← [1.0, 1.0] ↦ vf32[2]
    const vf32[4] s ← [1.0, 2.0, 3.0, 4.0] ↦ vf32[4]

    const matrix<f32, [3, 2]> left ← a ⊗ p
    const matrix<f32, [2, 4]> right ← p ⊗ s
    const matrix<f32, [3, 4]> product ← left · right
    print product

    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    const tf32[2, 3] m ← flat ↦ tf32[2, 3]
    const tf32[3, 2] t ← mᵀ.materialize()
    const tf32[2, 2] gram ← m · t
    const tf32[2, 3] squares ← m ⊙ m
    print gram.flatten()
    print squares.sum()
}
```

```text
[[2.0, 4.0, 6.0, 8.0], [4.0, 8.0, 12.0, 16.0], [6.0, 12.0, 18.0, 24.0]]
[14.0, 32.0, 32.0, 77.0]
91.0
```

### Loops and reductions

Ranges pick their direction from their bounds, a comma walks a Cartesian
product or lists in lockstep, and a reduction is an expression with an optional
body.

```fab
main {
    for range 10‥0 step 4 let i {
        print i
    }
    for range 1…2, 1…3 let row, col {
        print row * col
    }

    const list<int> price ← [4, 9, 2]
    const list<int> count ← [3, 1, 5]
    mut int bill ← 0
    for from price, count let p, n {
        bill ← bill + p * n
    }
    print bill

    const tf32[5] x ← [3.0, 1.0, 4.0, 1.0, 5.0] ↦ tf32[5]
    print reduce via max from x
    print reduce via argmax from x at [i] let s { return s }
    print reduce via count from x at [i] let s { return s ≻ 2.0 }
}
```

```text
10
6
2
1
2
3
2
4
6
31
5.0
4
3
```

### Errors are a typed channel

A function declares what it can fail with after `⇥`. A caller either supplies a
recovery value with `⊥` or handles the error, matching its variants and their
data.

```fab
union DivError {
    ByZero { int numerator },
    Negative { int divisor }
}

fn divide(int a, int b) → int ⇥ DivError {
    if b ≡ 0 then throw variant ByZero { numerator = a }
    if b ≺ 0 then throw variant Negative { divisor = b }
    return a / b
}

fn report(int a, int b) → string {
    do {
        return "§ / § = §"(a, b, divide(a, b))
    }
    catch err {
        match err {
            case ByZero let n { return "cannot divide § by zero"(n) }
            case Negative let d { return "negative divisor §"(d) }
        }
    }
}

main {
    print divide(84, 2) ⊥ -1
    print divide(84, 0) ⊥ -1
    print report(84, 2)
    print report(7, 0)
    print report(7, -3)
}
```

```text
42
-1
84 / 2 = 42
cannot divide 7 by zero
negative divisor -3
```

### The store rule

Arithmetic is exact in the expression; the cell's policy applies once, at the
store. `(200 + 100) / 2` in a `wrapping<u8>` is `150`, not `44`. A plain `u8`
cell traps, and only the store that does not fit traps.

```fab
main {
    const wrapping<u8> a ← 200
    const wrapping<u8> b ← 100
    const wrapping<u8> mean ← (a + b) / 2
    print mean

    const saturating<u8> hi ← 250
    const saturating<u8> chain ← hi + 200 - 100
    print chain

    let int big ← 300
    const wrapping<u8> wrapped ← big ↦ wrapping<u8>
    const saturating<u8> clamped ← big ↦ saturating<u8>
    print wrapped
    print clamped

    let u8 x ← 255
    let int exact ← x + x
    print exact
    let u8 y ← x + 1
    print y
}
```

```text
150
255
44
255
510
error: 256 does not fit in `u8` (declaration of `y`)      (stderr, exit 1)
```

### Bits are not numbers

`b8` … `b64` are exactly N bits: a shift acts at the type's width, and a float's
bit pattern is reached by an explicit conversion, never a cast. The same shift
on an `int` is arithmetic.

```fab
main {
    let b8 pattern ← 0xf3
    let b8 left ← pattern ⇐ 4
    let b8 cleared ← (pattern ⇐ 4) ⇒ 4
    print left
    print cleared
    print ¬pattern

    let int number ← 0xf3
    print number ⇐ 4

    let f32 one ← 1.0
    let b32 bits ← one ↦ b32
    print bits
}
```

```text
0x30
0x03
0x0c
3888
0x3f800000
```

### Conversions say how they round and what they fall back to

A float reaches an integer only through a named rounding mode
(`SEM016 float_to_integer_requires_rounding` otherwise). Absence is a type.

```fab
main {
    let float x ← 2.5
    let float y ← -2.5
    print "§ § §"(x ↦ int via HalfEven, x ↦ int via Floor, x ↦ int via Ceil)
    print "§ §"(y ↦ int via HalfEven, y ↦ int via Trunc)

    let int parsed ← "4x" ↦ int ⊥ -1
    print parsed

    const list<int> xs ← [10, 20, 30]
    const int ∪ none tenth ← xs.get(9)
    print tenth
    print tenth coalesce 0
}
```

```text
2 2 3
-2 -2
-1
none
0
```

## Design tenets

Faber is its own language. It is not a layer over Rust or a blend of the
languages it compiles to; the targets implement Faber, and a target that cannot
express a feature says so by name rather than bending the language.

**Written by models, read by people.** Most Faber code is expected to be
written by language models. That makes predictability the first requirement:
a model should be able to work out what a program means from a small set of
uniform rules, without importing the habits of whatever language it saw most
in training. Where a choice trades familiarity for mechanical consistency,
Faber takes consistency.

**One glyph, one meaning.** Operators are single glyphs, and each does exactly
one thing in every context:

| Glyph | Meaning |
| --- | --- |
| `←` | store a value at run time (assignment) |
| `→` | return / result type |
| `⇥` | the error channel: what a function can fail with |
| `↦` | explicit conversion |
| `≡` `≢` | equality and its negation (whole values, every type) |
| `∈` `∉` | membership |
| `⊙` `·` `⊗` `ᵀ` | element-wise product, matrix/dot product, outer product, transpose |
| `=` `:` | compile-time facts only: defaults, aliases, enum members, labels |

Arrows are run-time effects; `=` is never an assignment and never a run-time
comparison. Every negation glyph mirrors the glyph it negates (`≡`/`≢`,
`≅`/`≇`, `≈`/`≉`), and there are no synonyms: `≠` is a compile error that
points at `≢`.

**Explicit types, explicit conversions.** Declarations are type-first
(`int count`, `list<string> names`). Nullability is a union, `string ∪ none`,
not a sigil. Nothing narrows silently: crossing number families is a `↦`
conversion, and a conversion that loses information names its rounding
(`x ↦ int via Floor`). A conversion that can fail takes a recovery value with
`⊥` or goes to the error channel.

**Math happens in the ether; width applies at the store.** Arithmetic is
exact (or plain IEEE for floats) for the whole expression, and a width limit is
applied once, where the value is stored into a sized cell or converted to one.
`(200 + 100) / 2` stored into a `u8` is `150`, not an overflow; a
`wrapping<u8>`, `saturating<u8>` or `trapping<u8>` cell decides what happens
only if the stored value does not fit. Nothing traps, wraps or saturates in the
middle of an expression.

**Mathematics over hardware tradition.** When the mathematical answer and a
hardware-era convention disagree, Faber follows the mathematics: `/` and `%`
floor, rounding to nearest sends ties to the even neighbour everywhere, floats
follow IEEE 754 including NaN and infinities. Raw bits (`b8` … `b64`) are a
separate domain, not numbers: they have their width always, shifts act at that
width, and they convert to numbers only by an explicit `↦`.

**Sugar expands to one explicit form.** There are no hidden defaults. Every
short form is a fixed expansion the reader could write out
(`tf32[3, 2]` is `tensor<f32, [3, 2]>`; `t.sum()` is `reduce via sum from t`).

**Methods check; operators do what was written.** Every operation that can fail
has two spellings. A method is the checked form: `m.get(k)` returns `V ∪ none`.
An index or operator is the direct form: `m[k]` on a missing key stops the
program, because that is the spelling that was chosen.

**The language owns the bookkeeping.** Where a programmer would otherwise
manage indices, lanes, buffers or device residency by hand, Faber provides the
construct instead: iterate over elements rather than counters, reduce over
axes rather than writing the loop, let ownership markers decide copies.

**Errors are data.** A run-time error a program can read carries a kind and its
data, never prose, so programs and tests match on identity and the same program
behaves the same in every locale. Compiler diagnostics work the same way: a
stable code and issue name, with the text supplied by the locale pack.

**Keywords are a locale, not the language.** Keywords, type names, printed
words and diagnostics come from locale packs: English, Arabic, Hindi, Thai,
Vietnamese, and Simplified and Traditional Chinese among them. Every pack
compiles to the same program, `faber convert` translates a file between them,
and a file is written in one pack; packs never mix.

## Targets

One front end and two intermediate representations (a typed HIR and a MIR)
feed about two dozen code generators. HIR targets emit readable source in
another language; MIR targets lower further, to IR, kernels or native code.
Every target is held to the same reference: the MIR runner (`faber run`). A
generated program is correct when its output and exit status match the
runner's; a construct a target cannot express yet is rejected by name
(`unsupported on target <t>: <construct>`), never approximated.

How mature each target is, measured on the corpus (651 positive cases, each
with expected output; "suite" counts are pinned outcomes in
`corpus-suites/full`, "matches" counts were measured by building the emitted
program with the real toolchain and comparing it with the runner):

| Target | `-t` | Output | Evidence | Source |
| --- | --- | --- | --- | --- |
| **Reference** | | | | |
| MIR runner | (`faber run`) | executes the program; the oracle | suite 581 / 651 (most of the rest are fragments with no entry point) | public |
| Faber | `faber` | canonical Faber in the source's locale (`format`, `convert`) | round-trip 633 / 651 | public |
| FMIR / FHIR | `fmir`, `fhir` | serialized MIR and analyzed HIR artifacts | builds 616 and 590 of 638 files | public |
| **Application languages** | | | | |
| Rust | `rust` | Rust source, or a full Cargo project | suite 613 / 651 | official binary |
| Go | `go` | Go source | suite 538 / 651 | official binary |
| TypeScript | `ts` | TypeScript source | suite 506 / 651 | official binary |
| Swift | `swift` | Swift source | suite 336 / 651 | official binary |
| Python | `python` | self-contained Python | 240 of 257 comparable programs match | official binary |
| Haskell | `haskell` | Haskell source | 30 of 76 comparable programs match | official binary |
| **Systems and native** | | | | |
| C | `c` | C11 with a header-only runtime | preview: 71 programs emit, 65 of 66 comparable match | public |
| LLVM | `llvm`, `llvm-host` | LLVM IR, or a native executable via clang | suite 461 / 651 | official binary |
| WebAssembly | `wasm` | WebAssembly text | suite 460 / 651 assemble (no host runtime yet) | official binary |
| Cranelift | — | native objects with no external toolchain | in progress | public (planned) |
| **GPU kernels** (kernel functions only) | | | | |
| Metal | `metal` | Metal Shading Language | kernel programs only | official binary |
| WGSL | `wgsl` | WebGPU compute shaders | kernel programs only | official binary |
| CUDA | (`--device cuda`) | NVVM IR through the device runtime | kernel programs only | official binary |
| **Historical and novelty** | | | | |
| Fortran | `fortran` | Fortran 2008 | 40 of 40 comparable match | official binary |
| COBOL | `cobol` | GnuCOBOL free format | 34 of 37 comparable match | official binary |
| Pascal | `pascal` | Free Pascal | 36 of 37 comparable match | official binary |
| BASIC | `basic` | C64 BASIC V2 listing | 63 programs emit | official binary |
| Odin | `odin` | Odin | early: 6 of 6 match | official binary |
| Racket | `sexp` | Racket s-expressions (a validation target) | suite 456 / 651 | official binary |

Notes:

- "Comparable" excludes programs with no entry point; "emit" alone means
  source was produced, not that it built.
- The historical targets cover a deliberate subset (scalars, text, simple
  collections) and reject the rest by name. They exist partly as a test of the
  rule that targets implement Faber and never the reverse.
- Standard-library I/O (files, processes, clocks, network) is wired on the Rust
  target today; most other targets reject programs that use it, which accounts
  for much of the gap between their counts and Rust's.
- "Public" means the generator's source is in this repository. "Official
  binary" means it ships in the released `faber` but its source does not.

## What is in this repository

This repository is the open-source core of Faber:

- **The compiler front end and middle** (`crates/radix-*`): lexer, parser,
  semantic analysis, the typed intermediate representations (HIR and MIR), and
  the checks that run on them.
- **The MIR runner** (`crates/radix-mir-runner`): executes Faber programs
  directly. It is the reference every other target is checked against.
- **The C code generator** (`crates/radix-hir-c`, `runtime/c`): readable C
  (C11 plus a few GCC/Clang extensions) whose output matches the runner, checked
  under the undefined-behaviour sanitizer; a construct it cannot express yet is
  rejected by name. It is a preview and covers a growing subset of the
  language.
- **The `faber` command** (`crates/faber`): `check`, `run`, `test`, `format`,
  and `convert` between locale packs.
- **Norma**, the standard library (`lib/norma`): files, processes, time, text,
  JSON, TOML, HTTP and more.
- **The grammar** (`grammar/`) and **the locale packs** (`locale/`).
- **The corpus** (`corpus/`): several hundred small programs, each with its
  expected output, that document and test the language, and the suites that
  run them (`corpus-suites/`).

The official `faber` release also includes the code generators that are not in
this repository (see Targets). A native backend built on Cranelift, so that a
build from this source alone produces executables, is in progress.

## Build and run

You need a recent stable Rust toolchain.

```sh
cargo build --release -p faber
target/release/faber run program.fab
target/release/faber check program.fab
target/release/faber format program.fab
target/release/faber corpus corpus-suites/public --target runner
```

## Status

Faber is under active development and changes quickly; the 2.0 line is being
prepared. Expect the language and the commands to keep moving until then.

## License and authorship

Faber is written by Ian Zepp. It is available under either of the MIT license
or the Apache License, Version 2.0, at your option (`LICENSE-MIT`,
`LICENSE-APACHE`). This repository does not accept pull requests.

Documentation: <https://faberlang.dev>
