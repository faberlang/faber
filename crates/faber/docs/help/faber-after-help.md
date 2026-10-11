LLM Guidance:
  1. Use `faber` for packages and day-to-day workflows; use `radix` for single-file
     compiler phase inspection when you do not need `faber.toml` or Cargo integration.
     `faber run` interprets and runs source (single file, package, or archive)
     through the runner; it never compiles. Compiled artifacts are built with
     `faber build` and are not executed by faber.
  2. Discover commands:
     faber --help
     faber <command> --help
     faber doctor
     faber check-library <name>
  3. Establish project shape before compiling:
     faber init my-app
     faber targets
  4. Check and emit before run/test on a new package:
     faber check .
     faber build .
  5. Language reference (glyphs, keywords, grammar terms):
     faber explain --list
     faber explain functio
     faber explain --json functio
     faber explain --list --json
  6. Compatibility aliases (`lex`, `parse`, `hir`, `cli-ir`, `emit`) match `radix`
     for single-file inspection; package paths still prefer explicit `faber` commands.

Common flows:
  New package:
    faber init hello
    faber check hello
    faber build hello
    faber run hello
    faber test hello

  Check or build current directory:
    faber check .
    faber build . -t rust -o dist/
    faber build . --release

  Run with arguments forwarded after `--`:
    faber run . -- --flag value

  Device execution (Metal/CUDA, differentiable-GPU campaign S1-6):
    faber run --device metal <device-package>   # Apple Metal
    faber run --device cuda  <device-package>   # NVIDIA CUDA
    faber run --device auto  <device-package>   # exactly one admitted device
    faber run --device reference <input>        # plain sequential code on the MIR
                                                # runner; the oracle device
                                                # results are checked against
    faber run --device cpu   <input>            # processor cores as a device
                                                # (until the worker device lands
                                                # this is the same route as
                                                # reference)

    `--device` takes auto|reference|cpu|metal|cuda. Precedence: `--device` > manifest
    `[device] device` > auto.

    A device-capable package has an `@ nucleum` kernel and a `[device]` manifest
    section (device + inputs). The packaged image's `device` section carries
    the canonical device program and the MSL/PTX artifacts; the composite host
    (host driver) runs the kernel on a real Metal/CUDA session and reports the
    selected device + artifact hash + outputs. Explicit GPU requests never
    silently fall back; failures carry stable codes (E_BACKEND_UNAVAILABLE,
    E_DEVICE_*, E_NO_DEVICE_PROGRAM).
    See `faber targets` (metal-text/llvm-text rows).

  Native host executables (llvm-host-parity, Stage 9):
    faber build --target llvm-host <input>             # native executable

    `llvm-host` is a distinct target name (no alias). Build = shared
    package-to-LLVM builder (one .ll per unit) + llvm-as verify + pinned
    `opt -O2` (release) + one clang link against the faber-host-llvm runtime
    archive; never Rust codegen for the program, never a cc fallback. Artifacts
    land in target/faber-llvm/{debug|release}/ (modules/, link-manifest.toml,
    runtime/identity.toml, binary). Compiled artifacts are executed by the
    target ecosystem, not by faber. Requires a coherent LLVM toolchain
    (llvm-as + clang, plus opt for release) and a buildable hosts/llvm
    staticlib; unsupported host triples fail with a structured
    diagnostic. `-t llvm-text` stays emit-only (radix emit).

  Test selection (proba cases run on the runner, not a Cargo harness):
    faber test .
    faber test path/to/file.fab
    faber test . --name my_case
    faber test . --suite suite/path
    faber test . --tag slow

  Explain corpus:
    faber explain --list
    faber explain --category keywords
    faber explain --search return
    faber explain ≡
    faber explain --json functio

  Interpret source (runner, no Cargo) — `faber run` is interpreted execution,
  always; `faber script` retired and folded into it:
    faber run crates/exempla/corpus/incipit/salve-munde.fab
    faber run path/to/pkg           # package directory or faber.toml
    faber run app.zip               # source archive
    faber run script.fab -- alpha beta
    faber -c 'incipit { nota 1 }'
    faber repl

  Scripting host routes (`ad` routes):
    runtime:echo and processus:exi work (processus:exi exits the interpreter).
    Every other host route fails closed at plan time and is unavailable in
    this release; the rest return with the native host (core-norma N2).

  Single-file check (non-package):
    faber check crates/exempla/corpus/salve-munde.fab

  Format and convert complete Faber source units:
    faber format path/to/file.fab           (report drift, writes nothing; exit 1)
    faber format path/to/file.fab --write
    faber format path/to/file.fab --stdout
    faber convert path/to/file.fab --to la --stdout
    faber convert path/to/file.fab --from en --to la --stdout
    printf 'main { print "ok" }\n' | faber convert --stdin --to la

    `format` selects each file's source locale from frontmatter, then the
    package manifest, then product-default `en`; `<stdin>` uses `en` without
    frontmatter. `convert --from` forces one source pack for every selected
    file, while `convert --to` is one target pack for the whole invocation.
    Conversion requires a complete source unit, not a local-declaration
    fragment. Converted output replaces or inserts its target `locale` key and
    retains unrelated frontmatter metadata.

  Inspect resolved library home (FABER_LIBRARY_HOME or the in-tree lib/):
    faber doctor
    faber check-library norma

  Package-aware check/emit (directory or faber.toml):
    faber check path/to/pkg
    faber emit -t rust path/to/pkg
    faber build --target fmir-text path/to/pkg
    faber build --target fmir path/to/pkg
    faber build --target fmir-bin path/to/pkg
    path/to/pkg/target/faber-mir/exe/run alpha beta

  Inspect phases without a package (radix-compatible aliases):
    faber lex crates/exempla/corpus/salve-munde.fab
    faber parse crates/exempla/corpus/salve-munde.fab

  MIR probe emit on single `.fab` files (systems lane; package MIR image targets use `build`):
    faber emit -t llvm-text crates/exempla/corpus/incipit/salve-munde.fab
    faber emit -t wgsl-text crates/exempla/corpus/vector/kernel.fab
    faber emit --reflection -t wgsl-text crates/exempla/corpus/vector/kernel.fab

Output contract:
  - `build`: path of written artifact on stdout; diagnostics on stderr
  - `build --target fmir-text`: path to the inspectable FMIR text image on stdout
  - `build --target fmir`: path to the compact FMIR binary image on stdout
  - `build --target fmir-bin`: path to a self-contained FMIR runner on stdout
  - `check` / `emit` (package): compiler diagnostics on stderr; emit writes source
  - `run` / `test`: runner output on stdout; forwards the program's exit code
  - `run <path>`: interprets the input (single file, package, or archive) and
    forwards runtime args after `--`; nothing is compiled
  - `init`: manifest path on stdout
  - `explain`: human text on stdout; `--json` for one term, or with `--list` for the index as a JSON array
  - `lex`, `parse`, `hir`, `cli-ir`: JSON on stdout (same as `radix`)
  - `targets`: capability rows on stdout
  - `doctor`: resolved library home and per-library availability as key=value on stdout
  - `check-library`: selected home/root as key=value on stdout; diagnostics on stderr
  - errors: stderr with hints (e.g. `faber explain --list`)
