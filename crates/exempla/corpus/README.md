# Exempla corpus relocated

This private crate no longer vendors language sources.

| Content | Location |
| ------- | -------- |
| Keyword / language reference | sibling `radix/corpus/` |
| Package fixtures (tensor-*) | sibling `examples/corpus/` (moving to `faber/corpus/`) |
| GPU workload rungs | sibling `examples/gpu-workload/` |
| AIR lane demos | sibling `examples/air/` |
| Script-kernel demos | sibling `examples/script-kernel/` |
| Norma stdlib tours | sibling `norma/exempla/` |

Harnesses resolve these paths via `exempla::paths` (env overrides supported).
