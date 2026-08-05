# Faber

Public Faber project and package tool (`faber` binary).

Builds, checks, runs, tests, formats, and interprets Faber packages. The
compiler engine lives in the private **Radix** workspace; this repository is
the user-facing product surface.

## Local development layout

```text
faberlang/
  faber/           this repo (public CLI + corpus e2e harness)
  faber-runtime/   public Rust runtime types (`use faber::…`)
  radix/           private compiler (formatter is `radix::forma`)
  hosts/           public host monorepo embedded in core support
  norma/           public standard library source
  triga/           optional graphics and geometry library
  examples/        public application examples (corpus split: radix/corpus + faber/corpus)
  cista/           public package-store CLI/lib
```

Path dependencies (not published):

- `../radix/crates/radix` — compiler library (includes `radix::forma` formatter and MIR stepper)
- `../cista` — package-store library CLI/lib
- `crates/hygiene-ratchet` — hygiene budgets for non-test sources (dev-dependency)
- `crates/exempla` — corpus e2e harness (workspace member; depends on radix + this CLI)

Generated packages from `faber build` depend on sibling **`faber-runtime`**
(package name `faber-runtime`, crate name `faber`), not on this CLI crate.

## Build

```bash
cargo build --release
./target/release/faber --help
```

End users of released binaries do not need sibling checkouts. Building this
crate from source requires the sibling Radix, Cista, `faber-runtime`, and
`hosts` checkouts used by the local `faberlang/` layout.

## Commands

- `faber check` / `build` / `run` / `test`
- `faber check --deny-warnings`, `faber check --deny <CODE>` — promote warnings
  or specific diagnostic-catalog codes to hard failures (also on `build`, `run`, `test`)
- `faber script` — MIR interpret (no Cargo)
- `faber format`, `faber explain`, `faber targets`

See `faber --help` and after-help text for the full surface.

## Reader locale

Faber source and diagnostics can render in a reader locale. `faber check`,
`build`, `run`, and `test` accept `--locale <locale>` for the code locale and
`--diagnostic-locale <locale>` for the message language (independent of the code
locale); `faber format --locale la` reproduces the former `--canonical` re-emit
surface. The manifest equivalent is the `faber.toml` `[locale]` table (legacy
`[reader]` alias still accepted during the rename sweep). Installed packs live
in the private Radix `stdlib/locale/` (eight locales: `la`, `ar`, `hi`, `vi`,
`th-TH`, `zh-Hans`, `zh-Hant`, `en`); a package can override with a local
`locale/<locale>.toml`.

## Device execution (Metal / CUDA)

`faber run` selects a device backend for device-capable packages (the
`gpu-training-lowering` campaign, stages 1–6):

```bash
faber run --backend metal <package>   # Apple Metal (e.g. Apple M5 Max)
faber run --backend cuda  <package>   # NVIDIA CUDA (e.g. RTX 5070)
faber run --backend auto  <package>   # resolve: exactly one admitted backend
```

- Backend selection precedence: CLI `--backend` > manifest `[device] backend`
  > `auto`.
- A package carries a device program when its source has an `@ nucleum`
  compute kernel and its manifest declares a `[device]` section
  (`backend`, and `inputs` for the kernel's input buffers).
- The packaged FMIR image's `device` section carries the canonical device
  program, the Metal MSL + CUDA PTX artifacts (each with a provenance hash),
  the selection request, and the device runtime requirements; the composite
  host runs it through a real Metal/CUDA session (load → allocate → copy-in →
  launch → sync → readback → release) and reports the selected device, the
  artifact/module hash, and the observed outputs.
- An explicit GPU request never silently falls back: unavailable backends,
  bad descriptors, entry/dtype/shape mismatches, and payload-less packages
  fail closed with a stable structured code (`E_BACKEND_UNAVAILABLE`,
  `E_DEVICE_DESCRIPTOR`, `E_DEVICE_ABI_MISMATCH`, `E_DEVICE_ENTRY_MISMATCH`,
  `E_DEVICE_DTYPE_MISMATCH`, `E_DEVICE_SHAPE_MISMATCH`, `E_NO_DEVICE_PROGRAM`).
- The `faber targets` rows for `metal-text` / `llvm-text` report `run=yes`
  for this device-execution surface (`package=no` until the Stage 7 archive
  gate); `-t metal-text` / `-t llvm-text` remain emit-only.

Proof fixture: `examples/training/device-summa` (one tree-reduction kernel
through the whole pipeline on both backends, numeric-policy v1.0.0 parity
against its pinned CPU oracle). The surface also materializes training loops
end-to-end: a library-backed `train_step` / companion VJP with per-step
observation cadence (loss), gradient-slot → buffer mapping, and end-of-run value
readback.

## Factory goals

Open product-lane factory tracks for this CLI live under
[`docs/factory/`](docs/factory/) (moved out of private Radix on 2026-07-08).
