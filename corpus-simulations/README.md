# Corpus simulations

Small, runnable Faber programs that demonstrate numerical and systems workloads.
Each directory contains its Faber package, a focused fixture with expected
output, and a provenance/license note. Larger or private reference data,
benchmark harnesses and receipts remain under `acceptance/`.

- [Fusion containment](fusion-containment/README.md) — one Latin Faber source
  compared on the reference route, multicore CPU and Metal. The same source
  also has a partition audit; CUDA is not claimed.
- [H.264 encoder](h264-encoder/README.md) — a constrained-baseline encoder
  using native multicore CPU work. It does not claim GPU acceleration.

The open-source export contains these simulation packages. Device execution
and the native Rust emitter are available only in the released Faber binary;
see each example's README for its exact route requirements.
