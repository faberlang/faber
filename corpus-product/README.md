# Faber package corpus

Package-shaped product fixtures for Faber build/run/test proofs.

**Invariant:** every top-level entry used as a package fixture has a
`faber.toml`, except `format/`, a flat directory of single-file inputs with
their expected output. Pure single-file keyword programs live in
**`corpus/`**.

| Path | Role |
| --- | --- |
| `proba-canary/` | Proba canary: pure functions with colocated `main.proba` tests; `faber test` must report 0 failed / 0 blocked |
| `regio/` | `module` default import name: `import from "./planum"` binds the sibling's declared `geometry` |
| `contract-import/`, `contract-import-alias/`, `contract-import-wildcard/`, `alias-contract-import/` | Contract-import packages (plain, aliased, wildcard) with their expected output |
| `go-failable-imports/` | Three plain local modules (`main`, `proc`, `flt`), no Norma: an imported `⇥` call is a failable call, a leaf fault inside a failable function reaches `catch` as an error value, and a declared `@ conversion` row converts at the propagation boundary |
| `rust-imported-error-first/` | Three plain local modules (`main`, `proc`, `flt`), no Norma: a `main` `do`/`catch` whose first failable call is imported takes the callee's `⇥` type, owned by a third module, as the handled block's error type rather than an entry-local type that shares its definition id |
| `kernel-fused-mean-extents/` | A fused matmul + bias + mean kernel (`@ radix { lane = "air" }`) at extents 4, 8, 32, 64 and two non-square shapes; the printed means are exact integer-data values, the golden for the runner and for Metal (`fused_matmul_mean_hw_test`). The mean splits into several workgroup partials from 257 elements on, the case that once returned wrong values on Metal |
| `kernel-copy-into-elementwise/` | Whole-tensor elementwise copy-into (`out ⇇ a ⊙ b`, `out ⇇ w - g * lr`, `out ⇇ a`, and a `size`-generic form) in a kernel; the golden for the runner and for Metal (`device_lowering_gaps_hw_test`) |
| `kernel-generic-backward/` | A size-generic `@ kernel` with `@ radix backward`, called with its companion from a concrete caller; the golden for the runner and for Metal |
| `kernel-mixed-output-backward/` | A two-layer MLP loss and its backward companion at gradient extents 1024 and 256 (a `[32,32]` first layer, a `[32,8]` output layer); the golden for the runner, and within the kernel tolerance for Metal |
| `format/` | `.fab` inputs with `.format.expected` files, the fixtures for `faber format` |
| `reader-locale/` | Reader-locale packages (`ar`, `hi`, `th-TH`, `vi`, `zh-Hans`, `zh-Hant`), samples for `faber convert` and the package reader-locale tests; `th-TH/twins/main.la.fab` is the Latin twin and `<locale>/faults/*.fab` are the diagnostic fault inputs |

## Run

```bash
faber run corpus-product/regio
faber test corpus-product/proba-canary
```
