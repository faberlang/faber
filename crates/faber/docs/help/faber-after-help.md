LLM Guidance:
  1. Use `faber` for packages and day-to-day workflows; use `radix` for single-file
     compiler phase inspection when you do not need `faber.toml` or Cargo integration.
     `faber run` compiles and runs a package; `faber script` interprets source
     (single file, package, or archive) without compiling to Rust.
  2. Discover commands:
     faber --help
     faber <command> --help
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
    faber run --backend metal <device-package>   # Apple Metal
    faber run --backend cuda  <device-package>   # NVIDIA CUDA
    faber run --backend auto  <device-package>   # exactly one admitted backend

    A device-capable package has an `@ nucleum` kernel and a `[device]` manifest
    section (backend + inputs). The packaged image's `device` section carries
    the canonical device program and the MSL/PTX artifacts; the composite host
    runs the kernel on a real Metal/CUDA session and reports the selected
    device + artifact hash + outputs. Explicit GPU requests never silently fall
    back; failures carry stable codes (E_BACKEND_UNAVAILABLE, E_DEVICE_*,
    E_NO_DEVICE_PROGRAM). See `faber targets` (metal-text/llvm-text rows).

  Native host executables (llvm-host-parity, Stage 9):
    faber build --target llvm-host <input>             # native executable
    faber run   --target llvm-host <input> -- <args>   # build then execute

    `llvm-host` is a distinct target name (no alias). Build = shared
    package-to-LLVM builder (one .ll per unit) + llvm-as verify + pinned
    `opt -O2` (release) + one clang link against the faber-host-llvm runtime
    archive; never Rust codegen for the program, never a cc fallback. Artifacts
    land in target/faber-llvm/{debug|release}/ (modules/, link-manifest.toml,
    runtime/identity.toml, binary). Requires a coherent LLVM toolchain
    (llvm-as + clang, plus opt for release) and a buildable faber-runtime/
    hosts/llvm staticlib; unsupported host triples fail with a structured
    diagnostic. `-t llvm-text` stays emit-only (radix emit).

  Test selection (proba cases run on the MIR stepper, not a Cargo harness):
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

  Interpret source (MIR stepper, no Cargo) — `faber script`:
    faber script crates/exempla/corpus/incipit/salve-munde.fab
    faber script path/to/pkg          # package directory or faber.toml
    faber script app.zip              # source archive
    faber script script.fab -- alpha beta
    faber -c 'incipit { nota 1 }'
    faber repl

  Scripting host routes (import processus provider):
    processus.argumenta()  — argv after `--` (or repl/-c trailing args)
    processus.lege(name)   — environment variable (fails if unset; use future legeret for effective-value read)
    processus.scribe(name, value) — set environment variable
    processus.sedes()        — current working directory
    processus.identitas()    — process id
    processus.exi(code)      — exit interpreter

  Single-file check (non-package):
    faber check crates/exempla/corpus/salve-munde.fab

  Package-aware check/emit (directory or faber.toml):
    faber check path/to/pkg
    faber emit -t rust path/to/pkg
    faber build --target fmir-text path/to/pkg
    faber build --target fmir path/to/pkg
    faber build --target fmir-bin path/to/pkg
    path/to/pkg/target/faber-mir/exe/run alpha beta
    faber build --target scena path/to/pkg
    faber run --target scena path/to/pkg -- alpha beta

  Inspect phases without a package (radix-compatible aliases):
    faber lex crates/exempla/corpus/salve-munde.fab
    faber parse crates/exempla/corpus/salve-munde.fab

  MIR probe emit on single `.fab` files (systems lane; package MIR image targets use `build`/`run`):
    faber emit -t llvm-text crates/exempla/corpus/incipit/salve-munde.fab
    faber emit -t wgsl-text crates/exempla/corpus/vector/kernel.fab
    faber emit --reflection -t wgsl-text crates/exempla/corpus/vector/kernel.fab

Output contract:
  - `build`: path of written artifact on stdout; diagnostics on stderr
  - `build --target fmir-text`: path to the inspectable FMIR text image on stdout
  - `build --target fmir`: path to the compact FMIR binary image on stdout
  - `build --target fmir-bin`: path to a self-contained FMIR runner on stdout
  - `build --target scena`: path to the MIR package artifact manifest on stdout
  - `check` / `emit` (package): compiler diagnostics on stderr; emit writes source
  - `run` / `test`: forwards child process exit code; build diagnostics on stderr
  - `run --target fmir-text|fmir|fmir-bin`: builds the selected MIR package artifact and forwards runtime args after `--`
  - `run --target scena`: builds/loads the MIR package artifact and forwards runtime args after `--`
  - `init`: manifest path on stdout
  - `explain`: human text on stdout; `--json` for one term only
  - `lex`, `parse`, `hir`, `cli-ir`: JSON on stdout (same as `radix`)
  - `targets`: capability rows on stdout
  - errors: stderr with hints (e.g. `faber explain --list`)
