# Faber Architecture

How the pieces fit: the compilation model, the repository ecosystem, and the
package workflow. These diagrams render natively on GitHub; the styled version
of the compilation model also lives on
[the documentation site](https://faberlang.dev/en-US/toolchain/radix.html).

GPU computation follows the cross-repository ownership and execution contract
in [GPU Execution Architecture](gpu-execution-architecture.md): Gradus owns ML
semantics and Faber-authored kernels, Radix compiles target artifacts and
execution facts, and Hosts binds and executes them on physical devices.

## Compilation model

Faber's intermediate representation is the semantic authority. No target or
human-language surface is privileged: each target is a **projection** of HIR
meaning, and support is measured target by target (see
[`EBNF_MATRIX.md`](EBNF_MATRIX.md)).

```mermaid
flowchart LR
    Source["Faber source + frontmatter"] --> Frontend["Lex → parse → AST"]
    Frontend --> Semantic["Collect → resolve → HIR lower → typecheck → analysis"]
    Semantic --> Snapshot["AnalyzedUnit\nHIR + TypeTable + DefIds + side tables"]

    Snapshot --> HIR["HIR-direct route"]
    HIR --> Rust["Rust\nwidest package product surface"]
    HIR --> OtherHIR["Faber / TypeScript / Go / Swift"]

    Snapshot --> MIRLower["HIR → MIR lowering"]
    MIRLower --> Validated["ValidatedMir\nproof-carrying MIR"]
    Validated --> Stepper["MIR stepper"]
    Validated --> FMIR["FMIR package image"]
    Validated --> Wasm["Wasm / WAT"]
    Validated --> LLVM["LLVM text staging"]
    Validated --> Metal["Metal compute text"]
    Validated --> WGSL["WGSL compute or graphics text"]
    Validated --> Sexp["Racket S-expression text"]

    Snapshot -. "@ radix lane air + backward" .-> AIR["Typed HIR → AIR\nreverse AD / fusion"]
    AIR --> AIRMIR["AIR → MIR replacement + companions"]
    AIRMIR --> Validated
```

Lane split: the **application lane** (HIR) emits source languages; the
**systems lane** (MIR) emits IR and device artifacts. Real device execution
runs through `faber run --backend metal` / `--backend cuda` on the packaged
image — it is not a text-emit product surface.

## Repository ecosystem

```mermaid
flowchart TB
    subgraph faberrepo["faber — public repo"]
        Home["home: grammar, matrices, docs"]
        Runtime["runtime/\ngenerated-code carriers (faber package)"]
    end
    Radix["radix\nprivate compiler + faber CLI"]
    Hosts["hosts\nABI consumers and platform/browser hosts"]
    Norma["norma\nstdlib source"]
    Cista["cista\npackage store and lock writer"]
    Triga["triga\ngraphics / geometry"]
    Examples["examples\ncorpus and applications"]

    Home --> Radix
    Home --> Norma
    Home --> Hosts
    Home --> Cista
    Radix --> Runtime
    Radix --> Triga
    Radix --> Hosts
    Examples --> Home
    Examples --> Radix
```

`faber/docs/` holds the public grammar ([`EBNF.md`](EBNF.md)) and the rendered
target matrices ([`EBNF_MATRIX.md`](EBNF_MATRIX.md),
[`CONVERSIO_MATRIX.md`](CONVERSIO_MATRIX.md)). Radix measures: it emits
per-target measurement JSON at the end of its test ladder
(`radix/corpus/measurement/`, committed at release), and this repo renders the
matrices from that data (`scripta/render-matrices.py`); the documentation site
renders the localized views.

## Package workflow

```mermaid
flowchart LR
    User["faber CLI"] --> Compile["radix\ncompiler / lowering"]
    Compile --> Image["package image\nFMIR + target artifacts"]
    Image --> Run["run / test / emit"]
    User --> Store["cista\npackage store, lock"]
    User --> Stdlib["norma\nstdlib"]
    Run --> Hosts["hosts\nconcrete effects"]
    Store --> Lock["faber.lock\nproject lock"]
```

Package acquisition is owned by [Cista](https://github.com/faberlang/cista);
the `faber` CLI consumes the resulting project lock. Concrete filesystem,
process, network, browser, LLVM, and device behavior belongs in Hosts, not the
generated-language packages.
