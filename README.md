# Faber

Faber is a programming language designed to be written by language models and
read by people. Its rules are uniform and mechanical, so a model can predict
what a program means without guessing at the habits of other languages. Its
keywords come in locale packs, so the same program can be read and written in
English, Latin, Arabic, Hindi, Thai, Vietnamese, or Simplified and Traditional
Chinese.

```fab
fn greet(string name) → string {
    return "Hello, §!"(name)
}

main {
    const list<int> scores ← [3, 9, 4]
    const int total ← scores.sum()
    print greet("world")
    print "total: §"(total)
}
```

The same kind of program in the Latin locale:

```fab
+++
locale = "la"
+++
functio saluta(textus nomen) → textus {
    redde "Salve, §!"(nomen)
}

incipit {
    nota saluta("munde")
}
```

## What is in this repository

This repository is the open-source core of Faber:

- **The compiler front end and middle** (`crates/radix-*`): lexer, parser,
  semantic analysis, the typed intermediate representations (HIR and MIR), and
  the checks that run on them.
- **The MIR runner** (`crates/radix-mir-runner`): it executes Faber programs
  directly and is the reference every other execution of a program is checked
  against.
- **The `faber` command** (`crates/faber`): `check`, `format`, `convert`
  (between locales), `run`, and `test`.
- **Norma**, the standard library (`lib/norma`): files, processes, time, text,
  JSON, TOML, HTTP and more.
- **The grammar** (`grammar/`) and **the locale packs** (`locale/`).
- **The corpus** (`corpus/`): several hundred small programs, each with its
  expected output, that document and test the language.

The official `faber` release also includes code generators that are not in
this repository: Rust, Go, TypeScript and other targets, GPU kernels on Metal
and CUDA, and automatic differentiation. A native backend for this repository,
so a build from source alone can produce executables, is the next milestone.

## Build and run

You need a recent stable Rust toolchain.

```sh
cargo build --release -p faber
target/release/faber run corpus/incipit/salve-munde.fab
target/release/faber check path/to/program.fab
target/release/faber format path/to/program.fab
```

## Status

Faber is under active development and changes quickly; the 2.0 line is being
prepared. Expect the language and the commands to keep moving until then.

## License and authorship

Faber is written by Ian Zepp. It is available under either of the MIT license
or the Apache License, Version 2.0, at your option (`LICENSE-MIT`,
`LICENSE-APACHE`). This repository does not accept pull requests.

Documentation: <https://faberlang.dev>
