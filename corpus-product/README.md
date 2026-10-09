# Faber package corpus

Package-shaped product fixtures for Faber build/run/test proofs.

**Invariant:** every top-level entry used as a package fixture has a
`faber.toml`. Pure single-file keyword programs live in **`radix/corpus/`**
(see `radix/docs/factory/corpus-split-radix-faber/goal.md`).

| Path | Role |
| --- | --- |
| `tensor-fragment/tiny-linear/` | FMIR package fragment (host linear) |
| `tensor-fragment/tiny-linear-device/` | FMIR + device linear + reference JSON |
| `tensor-fragment/tiny-linear-device-relu/` | Rung-2 fragment: device linear + ReLU + reference JSON (negative pre-activation weights) |
| `tensor-package/fmir-matmul/` | FMIR package matmul proof |
| `proba-canary/` | Stage-3 proba canary: pure functions with colocated `main.proba` tests; `faber test` must report 0 failed / 0 blocked (`scripta/proba-canary.list`) |
| `regio/` | D7.7 `module` default import name: `import from "./planum"` binds the sibling's declared `geometry` (runner + Rust, `exempla_e2e/regio_package_test.rs`) |
| `reader-locale/` | Radix-authored `th-TH` and `zh-Hans` reader-locale packages (`th-TH/twins/main.la.fab` is the Latin twin); sample for the `faber convert` and package reader-locale tests; `<locale>/faults/*.fab` are the radix-authored diagnostic fault inputs for `package_text_contract_test` |

## Run

```bash
cargo run --manifest-path Cargo.toml -- run --target fmir-bin corpus/tensor-package/fmir-matmul
```

Resolution: `exempla::paths::package_corpus_dir()` / `FABER_PACKAGE_CORPUS`.
