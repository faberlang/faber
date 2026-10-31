# Provenance and review

Reviewed 2026-10-30 for publication. `faber.toml` and `src/` are the original
project-authored Faber package, moved from `acceptance/h264-encoder/`; there is
no duplicate shipping copy under `acceptance/`. The implementation was written
for this repository and does not include copied third-party encoder code.
Source comments and the private engineering notes cite the ITU-T H.264
recommendation for syntax/equations, but do not reproduce its text. The package
contains no dependency on the private acceptance benchmark tools at runtime.

`fixtures/translation-16x16-2.yuv` is a deterministic synthetic test clip
created by the repository's `acceptance/h264-encoder/common.py` fixture
producer (`translation`, 16×16, two frames, seed 20261030). It is not captured
or derived from third-party footage. SHA-256:

- input fixture, 768 bytes: `75bfb868444b57a213f0c3045e631c92257b5b0836680f4047ad304f62a041f0`
- expected Annex-B stream, 124 bytes: `6d6f0369081b483c441df72d97efb5af984b072bb5503f0fba77d5e9472b7b83`
- expected reconstruction, 768 bytes: `aa3d797163a4308819f756ca4b09747e9609d63684423410260ebc07474f8cdb`

The expected stream and reconstruction were emitted by the Faber encoder from
this source at QP 28 with four workers. FFmpeg independently decoded the stream;
the stream metadata, I/P picture sequence, 30-fps SPS timing, decoded pixel
count, and exact decoded-to-reconstruction comparison passed. The hash-bound
receipt is retained in `acceptance/h264-encoder/results/public-fixture-2026-10-30.json`.
Benchmark harnesses, large comparison inputs and raw receipts stay under
`acceptance/h264-encoder/` and are not published in this directory.
