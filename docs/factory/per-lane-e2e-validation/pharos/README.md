# Pharos host config — Faber per-lane e2e grid (EL-5)

Layout and operations for the nightly per-lane grid on pharos (delivery
`../delivery.md` §EL-5, decision 7). Tier 1 report-only: the grid reports
standing green/red with receipts; it does not gate merges yet.

## Layout on pharos

| Path | Purpose |
| --- | --- |
| `/home/ianzepp/work/lane-grid-mirror/<repo>.git` | private bare mirror hosting the grid's `origin/main` for faber, radix, cista, faber-runtime, hosts, norma, examples |
| `/home/ianzepp/work/lane-grid/{faber,radix,cista,faber-runtime,hosts,norma,examples}` | runner checkout (cloned from the mirror; faber/radix keep full history for bisect; norma+examples mirror the dev container layout so the workspace library-home probe and `faberlang_home()` resolve) |
| `/home/ianzepp/work/lane-grid/receipts/grid-<ts>/` | per-run receipts: one `<lane>.json` + `<lane>.log` per lane + `manifest.json` |
| `…/faber/docs/factory/per-lane-e2e-validation/grid-status.md` | standing status file (written by the runner) |
| `~/.config/systemd/user/faber-lane-grid.{service,timer}` | nightly timer (03:10) + oneshot runner unit |

## Refresh (burgus side)

`faber/scripta/lane-grid-provision` pushes this machine's local `main` for
the seven runner repos into the pharos mirror, then detaches each checkout at
`origin/main`. Run it after main advances so the nightly grid validates the
new main. `--check` reports HEADs without pushing.

`norma/` and `examples/` are provisioned because the faber workspace probes
need them: the package resolver's default library home is the workspace
containing `norma/src/solum.fab`, and `faberlang_home()` requires `examples/`
plus `radix/` (or `norma/`) — without them, library-importing corpus files
fail with `PKG001: missing_library_home` and the grid reports false reds.

## Host toolchains (lane requirements)

The runner probes per-lane tools and marks a lane `skipped` (never green) when
its toolchain is missing. Full-grid hosts need:

| Tool | Lane(s) | pharos install |
| --- | --- | --- |
| cargo / rustc / rustfmt | rust (+ every lane build) | rustup (`~/.cargo/bin`) |
| go | go | apt `golang-go` |
| node + tsc | ts | apt `nodejs npm` + `npm i -g typescript` |
| deno | ts (alt toolchain) | `~/.local/bin` (deno installer) |
| wasmtime + wasm-tools | wasm | `~/.local/bin` (release binaries) |
| swiftc | swift | Swift.org Ubuntu tarball at `~/swift` |
| racket | sexp | apt `racket` |
| llvm-as + opt | llvm | apt `llvm` |

> **ts lint tier is skipped on the grid host (by design).** The ts harness's
> lint tier uses `eslint` or `biome` when one is on PATH; the burgus reference
> has neither, so the lane's lint tier is skipped there. The apt `eslint`
> 6.4.0 was removed from pharos — it errors on every file (no config file),
> turning the whole ts lane red. Keep eslint/biome off the grid PATH so the
> lane matches the burgus reference signal. Same for the formatter: prettier
> is absent, so `deno fmt` is the formatter (present in `~/.local/bin`).

> **swift lane is macOS-`Darwin`-sensitive.** The swift backend emits
> `import Darwin`, which does not exist on Linux. Two corpus files
> (`mone/mone.fab`, `nota/gradus.fab`) are red on the Linux grid host for that
> reason only; the rest of the swift lane passes. `ternarius/ternarius.fab`
> fails with a real codegen typing issue (`String?` unwrap).

## Install

```sh
scp -r faber/docs/factory/per-lane-e2e-validation/pharos/ pharos:/tmp/lane-grid-pharos/
ssh pharos '/tmp/lane-grid-pharos/install-grid-unit.sh'
```

## Manual run

```sh
ssh pharos 'systemctl --user start faber-lane-grid.service'
ssh pharos 'tail -f ~/grid-run.log'        # or: journalctl --user -u faber-lane-grid.service
```

## Notes

- The grid never runs cargo on the dev machine; `lane-grid-run` refuses hosts
  named `burgus*` unless `--allow-burgus` is passed.
- Cargo targets are independent per checkout (`target/` under the lane-grid
  faber checkout); no shared cache, no `CARGO_TARGET_DIR`.
- After the lane-grid scripts land on faber main, `lane-grid-provision`
  refreshes them into the checkout automatically. Until then, deploy edited
  scripts with scp.
