# H.264 encoder example

This package is an experimental, **CPU-only** H.264/AVC baseline encoder. It
uses Faber's native Rust target to parallelize independent macroblock jobs; it
does not contain device kernels and does not claim GPU acceleration. The
released Faber executable is required because the Rust package emitter/runtime
is not part of the open-source export. The installed Faber 1.17.0 release
reported `rust available=yes`; the source-only build does not carry this target.
No `--device metal` or `--device cuda` route is involved.

## Build and run

From the repository root, use the released Faber binary:

```sh
faber build --target rust --release corpus-simulations/h264-encoder

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
corpus-simulations/h264-encoder/target/release/h264-encoder \
  corpus-simulations/h264-encoder/fixtures/translation-16x16-2.yuv \
  "$work/translation.264" "$work/reconstruction.yuv" \
  16 16 2 28 4
cmp "$work/translation.264" \
  corpus-simulations/h264-encoder/expected/translation-16x16-2.264
cmp "$work/reconstruction.yuv" \
  corpus-simulations/h264-encoder/expected/translation-16x16-2-reconstruction.yuv
```

The checked-in fixture is a deterministic synthetic 16×16 YUV420 clip with two
frames (one I picture and one P picture). The `.264` and reconstruction files
are the expected outputs for QP 28 and four workers. The encoder writes an
encode-only timer to stdout; elapsed time and encode time are distinct.

## Quick fixture smoke

The small `smoke/` package reads the checked-in YUV fixture and passes its first
four luma bytes through a deterministic tensor checksum. This validates cheap
reference and CPU-device routes without requiring the released Rust emitter;
it is not a substitute for running the encoder shown above. Run from the
repository root:

```sh
target/debug/faber run --device reference corpus-simulations/h264-encoder/smoke
target/debug/faber run --device cpu --threads 2 corpus-simulations/h264-encoder/smoke
```

Both routes print `h264 fixture luma checksum 1140.0`.

The released Faber binary is needed for `--target rust`; the open-source build
omits that backend. This workload is native CPU work, not a device workload. Its
`workers` argument controls concurrent native conversation jobs; the resulting
bitstream and reconstructed pixels are deterministic across supported worker
counts. The format is a deliberately limited experiment, not an x264
replacement: it implements constrained-baseline syntax, one macroblock per
slice, integer-pixel P16x16 search, and no CABAC, B pictures, rate control or
fractional motion.

## CPU measurements

The table uses the same native encoder source and a generated 256×128,
12-frame translation clip at QP 28. These are local measurements, not a
production-comparison claim. Process FPS includes process startup and file I/O;
encode FPS uses only the encoder's reported encode interval.

| Workers | Process FPS | Encode FPS | Route |
| ---: | ---: | ---: | --- |
| 1 | 18.53 | 19.08 | native CPU |
| 4 | 22.44 | 23.24 | native CPU |
| 18 | 24.90 | 25.87 | native CPU |

The retained source-side receipt is
`acceptance/h264-encoder/results/2026-10-30-cpu-scaling.json`; methodology and
scope are in `acceptance/h264-encoder/RESULTS.md`. The small published fixture
was checked with FFmpeg's H.264 decoder: stream dimensions, frame count, I/P
slices and the fixed progressive 30-fps SPS contract passed, and decoded pixels
matched the expected reconstruction exactly. Its hash-bound checker receipt is
`acceptance/h264-encoder/results/public-fixture-2026-10-30.json` in the full
repository. Acceptance harnesses, large benchmark data and receipts remain in
`acceptance/` and are not exported with this package.

See [`PROVENANCE.md`](PROVENANCE.md) for the source and fixture review.
