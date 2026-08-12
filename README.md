# Faber

Faber is a statically typed programming language with reader-localized source
vocabulary and shared, locale-independent semantics.

This public repository is the language project home and the source home for
Faber's generated-language support packages. The compiler and `faber` CLI
implementation are maintained in a private repository. Public binaries,
checksums, grammar, documentation, examples, and target APIs remain available.

## Install

Supported release archives are published at
[faberlang/releases](https://github.com/faberlang/releases/releases).

```sh
# Select the current version and platform archive from the releases page.
tar -xzf faber-vX.Y.Z-<target>.tar.gz
./bin/faber --version
```

Supported release platforms are listed with each release. Package acquisition
is owned by [Cista](https://github.com/faberlang/cista); the Faber CLI consumes
the resulting project lock.

## Language

- [Documentation and learning paths](https://faberlang.dev/en-US/)
- [Formal grammar](https://faberlang.dev/en-US/reference/grammar.html)
- [Examples](https://github.com/faberlang/examples)
- [Public releases](https://github.com/faberlang/releases/releases)

The website grammar is generated from the compiler's versioned grammar source.
Release notes and manifests record the product version and immutable build
inputs used for each binary release.

## Target APIs

| Target | Public source | Package identity |
| --- | --- | --- |
| Rust | [`runtime/rust/`](runtime/rust/) | package `faber-runtime`, crate `faber` |
| TypeScript | [`runtime/typescript/`](runtime/typescript/) | `@faber/runtime` |
| Go | [`runtime/go/`](runtime/go/) | `faber/rt` |
| Swift | [`runtime/swift/`](runtime/swift/) | `FaberRuntime` |

The Rust ABI and carrier sources are under
[`runtime/rust/src/contract/`](runtime/rust/src/contract/). Concrete
filesystem, process, network, browser, LLVM, and device behavior belongs in
Hosts, not these generated-language packages.

The [HTTP package](packages/http/) and
[Rust HTTP transport](crates/http-transport/) are also public here.

## Repository Family

| Repository | Role | Visibility |
| --- | --- | --- |
| `faberlang/faber` | Language home and generated-language APIs | Public |
| `faberlang/releases` | Signed release objects and checksums | Public |
| `faberlang/norma` | Standard library source | Public |
| `faberlang/cista` | Package store and lock writer | Public |
| `faberlang/hosts` | Concrete host implementations | Public |
| `faberlang/examples` | Examples and application packages | Public |
| `faberlang/tree-sitter-faber` | Editor grammar | Public |
| Radix | Compiler and `faber` CLI implementation | Private |

The machine-readable form is [`catalog.json`](catalog.json).

## Issues And Contributions

Use this repository for language reports, public target-package bugs, and
documentation routing. Compiler implementation patches cannot be accepted
because that source is private. Reproducible reports should include the Faber
version, target, source fixture, exact command, and observed output.

Public package changes are accepted when they preserve the generated-code
contract and include focused tests. See [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md).

Report security issues through
[GitHub private vulnerability reporting](https://github.com/faberlang/faber/security/advisories/new),
not a public issue.

## History

Older compiler and CLI source remains available in this repository's Git
history. It is historical, not the current implementation.

## License

MIT. See [`LICENSE`](LICENSE).
