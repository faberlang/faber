# Faber

> **About this repository's history.** It was produced by running
> `git filter-repo` over the private Radix compiler repository as part of the
> public release. Only the open-source paths were kept, so commit messages may
> refer to commits, issues and files that are not here. The earlier public
> repository is archived as
> [faber-1.x](https://github.com/faberlang/faber-1.x).

Faber is a programming language designed to be written by language models and
read by people. It is built on a small number of uniform, mechanical rules, so
that a model can predict what a program means without importing the habits of
whichever language it saw most in training. It compiles to many targets, from
a reference interpreter and readable C to Rust, Go, TypeScript and GPU kernels.

> **Experimental through version 1.** The language changes constantly, and
> breaking changes ship in minor releases. See [Status](#status).

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
    let vf32[3] a ← [1.0, 2.0, 3.0] ↦ vf32[3]
    let vf32[2] p ← [1.0, 1.0] ↦ vf32[2]
    let vf32[4] s ← [1.0, 2.0, 3.0, 4.0] ↦ vf32[4]

    let matrix<f32, [3, 2]> left ← a ⊗ p
    let matrix<f32, [2, 4]> right ← p ⊗ s
    let matrix<f32, [3, 4]> product ← left · right
    print product

    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    let tf32[2, 3] m ← flat ↦ tf32[2, 3]
    let tf32[3, 2] t ← mᵀ.materialize()
    let tf32[2, 2] gram ← m · t
    let tf32[2, 3] squares ← m ⊙ m
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

    let list<int> price ← [4, 9, 2]
    let list<int> count ← [3, 1, 5]
    mut int bill ← 0
    for from price, count let p, n {
        bill ← bill + p * n
    }
    print bill

    let tf32[5] x ← [3.0, 1.0, 4.0, 1.0, 5.0] ↦ tf32[5]
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
    let wrapping<u8> a ← 200
    let wrapping<u8> b ← 100
    let wrapping<u8> mean ← (a + b) / 2
    print mean

    let saturating<u8> hi ← 250
    let saturating<u8> chain ← hi + 200 - 100
    print chain

    let int big ← 300
    let wrapping<u8> wrapped ← big ↦ wrapping<u8>
    let saturating<u8> clamped ← big ↦ saturating<u8>
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

    let list<int> xs ← [10, 20, 30]
    let int ∪ none tenth ← xs.get(9)
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

**Explicit types, explicit conversions.** Declarations are type-first, and
four binding words say how a name is bound: `let` (fixed), `mut` (writable),
`ref` (re-pointable, read-only) and `const` (a compile-time fact, set with
`=`). Each writes its type slot (`let int count ← 0`, `mut list<string> names ← []`);
there is no short form, and `_` in the slot asks for inference
(`PARSE010 binding_type_slot_required` otherwise). Nullability is a union,
`string ∪ none`, not a sigil. Nothing narrows silently: crossing number families is a `↦`
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

<!-- target-maturity:begin -->
How mature each target is, measured on the corpus (897 programs the suite runs, each with expected output; 302 more are meant to be refused). "Suite" counts are cases that pass in `corpus-suites/full` (`faber corpus`) out of the cases the suite applies to; the rest are pinned expected failures. A library file with no entry function has nothing to run, so it counts on a compiled target when it emits and builds, and on the runner when it checks and lowers to valid MIR. C and Haskell have no corpus lane: each single-file program the runner executes and passes was emitted, built with the real toolchain, and its output and exit status compared with the runner's.

| Target | `-t` | Output | Evidence | Source |
| --- | --- | --- | --- | --- |
| **Reference** | | | | |
| MIR runner | (`faber run`) | executes the program; the oracle | suite 892 / 897 (the rest are pinned expected failures); 53 of the passes are library files with no entry function, which count when they check and lower to valid MIR | public |
| Faber | `faber` | canonical Faber in the source's locale (`format`, `convert`) | round-trip 861 / 869 | public |
| FMIR / FHIR | `fmir`, `fhir` | serialized MIR and analyzed HIR artifacts | no corpus lane (exercised by the package build and run tests) | public |
| **Application languages** | | | | |
| Rust | `rust` | Rust source, or a full Cargo project | suite 854 / 889 | official binary |
| Go | `go` | Go source | suite 730 / 896 | official binary |
| TypeScript | `ts` | TypeScript source | suite 804 / 897 | official binary |
| Swift | `swift` | Swift source | suite 468 / 897 | official binary |
| Python | `python` | self-contained Python | suite 640 / 871 | official binary |
| Haskell | `haskell` | Haskell source | 85 of 704 single-file programs emit; 34 of those match the runner | official binary |
| **Systems and native** | | | | |
| C | `c` | C11 with a header-only runtime | preview: 342 of 704 single-file programs emit; 338 of those match the runner | public |
| LLVM | `llvm-text`, `llvm-host` | LLVM IR, or a native executable via clang | suite 692 / 897 | official binary |
| WebAssembly | `wasm`, `wasm-text` | WebAssembly text | suite 605 / 897 assemble (no host runtime yet) | official binary |
| Cranelift | `clif` | native objects with no external toolchain | preview: suite 646 / 869 | public |
| **GPU kernels** (kernel functions only) | | | | |
| Metal | `metal-text` | Metal Shading Language | kernel programs only | official binary |
| WGSL | `wgsl-text` | WebGPU compute shaders | kernel programs only | official binary |
| CUDA | (`--device cuda`) | PTX built at run time from NVVM IR (needs `clang` with the NVPTX target) | kernel programs only: `faber run --device cuda` runs whole-tensor kernels on NVIDIA GPUs; lane-form kernels are not lowered yet and fail closed | official binary |
| **Validation** | | | | |
| Racket | `sexp` | Racket s-expressions (a validation target) | suite 673 / 878 | official binary |
| **Experimental** (not in the released `faber`) | | | | |
| Fortran, COBOL, Pascal, Odin, BASIC | `fortran`, `cobol`, `pascal`, `odin`, `basic` | historical languages, a deliberate subset | default-off features; no corpus lane | private source |
<!-- target-maturity:end -->

Notes:

- "Match" counts compare the emitted program's output and exit status with the
  runner's, for the single-file programs the runner passes; "emit" alone means
  source was produced, not that it built.
- The C and Cranelift targets are previews. Both are in the default `faber`
  build, neither runs under `faber run`, and a construct either cannot express
  yet is rejected by name.
- The experimental historical targets (Fortran, COBOL, Pascal, Odin, BASIC)
  cover a deliberate subset (scalars, text, simple collections) and reject the
  rest by name. They exist partly as a test of the rule that targets implement
  Faber and never the reverse. They are default-off features and are not in the
  released `faber`.
- Standard-library I/O (files, processes, clocks, network) is wired in full on
  the Rust target and on the MIR runner (`faber run`). Go has standard I/O,
  clocks and randomness, WebAssembly standard I/O, and Cranelift clocks; the
  other targets reject programs that use I/O, which accounts for much of the gap
  between their counts and Rust's. `faber targets` lists each target's I/O.
- "Public" means the generator's source is in this repository. "Official
  binary" means it ships in the released `faber` but its source does not.
  `faber targets` on the binary you are running is the authority on what it can
  emit; a target missing from the build is rejected with
  `PKG001 target_not_in_this_build`.
- The table is generated from the corpus suite at release time, not edited by
  hand.

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
- **The Cranelift code generator** (`crates/radix-mir-clif`, `runtime/clif`):
  lowers checked MIR to a native object for the host with no external
  toolchain beyond a linker, so a build from this repository alone produces
  executables (`faber build --target clif`). It is a preview: a fail-closed
  subset of the language, with named gaps.
- **The artifact formats** (`crates/radix-hir-fhir`, `crates/radix-mir-fmir`):
  serialized analyzed HIR and MIR images that `faber` can load and run.
- **The `faber` command** (`crates/faber`): `check` (with `--lower` to also
  compile the program without running it), `run`, `test`, `build`, `format`,
  `convert` between locale packs, `corpus` and `targets`.
- **Norma**, the standard library (`lib/norma`): files, processes, time, text,
  JSON, TOML, HTTP and more.
- **The grammar** (`grammar/`) and **the locale packs** (`locale/`).
- **The corpus** (`corpus/`): several hundred small programs, each with its
  expected output, that document and test the language, and the suites that
  run them (`corpus-suites/`).

The official `faber` release also includes the code generators that are not in
this repository (see Targets).

## Install

Prebuilt `faber` binaries for macOS (Apple silicon) and Linux (x86-64) are
published on the [releases page](https://github.com/faberlang/releases/releases);
this one is [Faber 1.17.0](https://github.com/faberlang/releases/releases/tag/faber-v1.17.0).
The installer downloads a release, verifies its SHA-256, installs it as
`<prefix>/bin/faber-<version>`, and points a `faber` symlink at it:

```sh
curl -fsSL https://raw.githubusercontent.com/faberlang/faber/main/install.sh | sh
```

The default prefix is `~/.faber`; add `~/.faber/bin` to `PATH`. Pass `--prefix`
or `--version` to choose another location or pin a release:

```sh
curl -fsSL https://raw.githubusercontent.com/faberlang/faber/main/install.sh \
  | sh -s -- --version 1.17.0 --prefix "$HOME/.local"
```

Check for and install updates with:

```sh
faber update --check
faber update
```

Every released version stays in `<prefix>/bin/faber-<version>`, and `faber
update` moves the `faber` symlink. `faber update --version 1.15.2` switches
back, and `faber versions` lists what is installed. If you installed with an
earlier faber installer, run `install.sh` once to adopt the versioned layout;
`faber update` manages installs that have the `faber` symlink.

### Manual install

The tarball holds a single file, `bin/faber`. Download the archive and its
`.sha256` file into one directory, check it, and put `faber` on your `PATH`:

```sh
shasum -a 256 -c faber-v1.17.0-aarch64-apple-darwin.tar.gz.sha256   # Linux: sha256sum -c
tar -xzf faber-v1.17.0-aarch64-apple-darwin.tar.gz
sudo mv bin/faber /usr/local/bin/faber
faber --version
```

On Linux the platform name is `x86_64-unknown-linux-gnu`. Documentation, the
language reference and the target matrix are at <https://faberlang.dev>.

## Build and run

To build the open-source core from this repository instead, you need a recent
stable Rust toolchain.

```sh
cargo build --release -p faber
target/release/faber run program.fab
target/release/faber check program.fab
target/release/faber format program.fab
target/release/faber corpus corpus-suites/public --target runner
```

## Status

**Faber is experimental through version 1.** The language, the standard library
and the commands change from one minor release to the next, and the changes are
often breaking: syntax is retired and replaced outright, with no compatibility
layer. No 1.x release carries a stability promise. Pin an exact version, and read
the release notes before you upgrade.

This repository's first release is Faber 1.14. A 2.0 release is planned for when
the language is largely stable.

## License and authorship

Faber is written by Ian Zepp. It is available under either of the MIT license
or the Apache License, Version 2.0, at your option (`LICENSE-MIT`,
`LICENSE-APACHE`). This repository does not accept pull requests.

Documentation: <https://faberlang.dev>
