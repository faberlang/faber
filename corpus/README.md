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

## Run

```bash
cargo run --manifest-path Cargo.toml -- run --target fmir-bin corpus/tensor-package/fmir-matmul
```

Resolution: `exempla::paths::package_corpus_dir()` / `FABER_PACKAGE_CORPUS`.
