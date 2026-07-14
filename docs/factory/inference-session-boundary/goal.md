# Goal: Inference Session Boundary

**Status**: proposed — Faber-owned inference/session boundary, static model artifact oracle check, and session CLI contract defined; no runtime implementation or capability claim
**Created**: 2026-07-14
**Refreshed**: 2026-07-14
**Target workspace**: `/home/ianzepp/work/faberlang`
**Factory artifact dir**: `faber/docs/factory/inference-session-boundary/`
**Primary surfaces**: `faber` CLI package build/run UX, FMIR package artifacts,
generated Rust package/runtime wiring, future model artifact manifests, and
examples-owned inference oracles.

---

## Summary

Define what Faber owns in a future inference/session path after Radix target
reporting delegates FMIR package production to `faber build` and `faber run`.
This goal is a boundary and evidence packet, not an inference implementation.

Faber owns the user-facing package/session surface: command shape, package
artifact assembly, model artifact handoff validation, generated runtime
dependency wiring, fixture/oracle admission, and non-claim text. Radix remains
the compiler and target fact owner. Examples own runnable oracle fixtures.
Runtime/provider crates own actual model loading, tensor execution, device
lifecycle, tokenizer execution, and GPU/CPU kernels.

## Current Ground Truth

| Surface | Current Faber fact | Boundary implication |
| --- | --- | --- |
| `faber targets` | Faber exposes Faber command-surface capability rows. FMIR package targets report `build/run/package=yes` because Faber owns those package artifact commands, while Radix keeps its emit rows delegated/non-runnable. | Faber target rows must distinguish Faber package command truth from Radix emit truth. |
| `build --target fmir-text` / `fmir` / `fmir-bin` | Package FMIR image targets are produced by Faber package commands and print artifact paths on stdout. | Faber owns package artifact UX and runner bundling for FMIR package images. |
| `run --target fmir-text\|fmir\|fmir-bin` | Builds the package artifact and forwards runtime arguments after `--`. | Faber owns CLI/session argument forwarding and fail-closed runtime requirement checks for package images. |
| `run --target scena` | Builds/loads the MIR package artifact and forwards runtime args after `--`. | Faber owns package-scoped MIR runner UX; Radix owns MIR semantics. |
| Generated Rust packages | `faber build` emits Cargo packages and links `faber-runtime` plus native library target dependencies from package facts. | Faber owns package/runtime dependency assembly, not tensor kernel correctness. |
| Examples AI workbench | Examples lane records fake/oracle runner evidence and next tiny token/logits oracle fixture. | Faber should consume fixtures as acceptance oracles only after a package/session boundary is explicit. |

## Faber-Owned Boundary

Faber owns these future inference/session responsibilities:

- **CLI/session UX**: name and parse an inference-oriented package command or
  subcommand; forward session arguments deterministically; define stdout/stderr
  contracts for prompts, tokens, diagnostics, and artifact paths.
- **Package artifact assembly**: build or locate package artifacts, FMIR images,
  generated Rust crates, and future session manifests without asking users to
  call Radix internals directly.
- **Model artifact handoff shape**: validate paths, declared model metadata,
  checksums, declared format, tokenizer reference, and runtime requirements
  before dispatching to any runtime/provider layer.
- **Package/runtime boundary**: wire declared package dependencies and runtime
  requirements into generated artifacts; reject unknown runtime requirements
  before execution; keep model loading and tensor execution outside this CLI
  crate unless a later runtime package explicitly owns them.
- **Evidence and non-claim text**: keep help, factory docs, tests, and oracles
  explicit that the lane is boundary/session scaffolding until model loading,
  tokenizer behavior, tensor op coverage, placement/copy/readback, and kernels
  are implemented by their owners.

Faber does not own:

- Radix target capability truth, MIR lowering semantics, or target matrices.
- model file parsing, GGUF/safetensors loading, tokenizer implementation, tensor
  kernels, quantization/packing, device placement, copy/readback, or GPU launch.
- examples-local fake/oracle runner behavior except as accepted fixture input.

## Model Artifact Handoff Shape

The first Faber-owned handoff should be metadata-first and fail-closed. A future
manifest can be TOML, JSON, or a Faber package section, but it must carry at
least:

```text
model.id
model.format              # e.g. oracle, safetensors, gguf
model.path
model.sha256
tokenizer.path
tokenizer.sha256
session.entry             # package entry or exported command
runtime.requirements[]    # e.g. fmir-cli-args, external provider names
evidence.non_claims[]     # explicit disabled claims
```

For the first packet, `model.format = "oracle"` is enough. Real GGUF,
safetensors, tokenizer, and quantized kernel formats should remain rejected
until their owners provide verifiable loaders and runtime facts.

The first static oracle lives in
[`model-artifact-oracle.toml`](model-artifact-oracle.toml) and is checked by
[`check-model-artifact-oracle.py`](check-model-artifact-oracle.py). It validates
only handoff metadata, relative contained paths, checksum syntax, allowed and
rejected format policy, exact oracle runtime requirements, exact prompt argv
contract, and explicit non-claims. Its self-test rejects unknown runtime
requirements and argv-contract drift. The checker deliberately does not open
model or tokenizer artifacts, load model bytes, run tokenizers, or execute an
inference runtime.

## Session CLI Contract

The first command-shape contract lives in
[`session-cli-contract.toml`](session-cli-contract.toml) and is checked by
[`check-session-cli-contract.py`](check-session-cli-contract.py). It defines a
future `faber session <package-root> --model-manifest <manifest> --
<session-args>` shape, oracle-only manifest admission, package target
allow-lists, stdout/stderr contracts, fail-before-execution behavior, and
explicit non-claims. The checker validates only the contract metadata,
including the exact package-root, separator, stdout/stderr, required failure
prefix, and allowed package target set. Its negative fixture covers these
exact-contract fields plus unsupported target drift. It does not run `faber`,
load model or tokenizer artifacts, or execute an inference runtime.

## Package And Runtime Boundary

Faber should treat inference as package execution with extra declared artifacts,
not as a hidden CLI builtin:

1. package manifests declare source, dependencies, target, and any model/session
   artifact metadata;
2. Faber validates manifest shape and artifact existence/checksums;
3. Faber builds the requested package target or selects an already-built
   artifact;
4. Faber passes only validated paths/arguments to the runtime/provider layer;
5. runtime/provider code performs model loading, tokenizer, tensor execution,
   and device lifecycle;
6. Faber reports diagnostics and exit status without reinterpreting model data.

This preserves the current split: Faber is the product/package/session surface,
Radix is the compiler/target fact source, and runtime/provider crates own
execution behavior.

## Evidence And Non-Claims

Every inference/session packet in Faber should include these non-claims until
the named owner lands evidence:

- no public inference support;
- no llama.cpp equivalence;
- no GGUF, safetensors, transformer, tokenizer, or quantized-kernel runtime;
- no GPU/CUDA/WebGPU/Metal launch runtime;
- no model download, registry, or trusted model cache;
- no performance, memory, throughput, or correctness claim beyond the specific
  checked oracle.

## Smallest Follow-Up Packets

1. **Model artifact manifest oracle check** — complete for static metadata
   - Add a Faber-local docs/test fixture for an `oracle` model manifest.
   - Validate required fields, path containment, checksum shape, and non-claims.
   - Do not load model bytes or run inference.

2. **Session CLI contract spike** — complete for static metadata
   - Define a narrow command contract for a package/session invocation that
     forwards prompt/session arguments and reports artifact paths/diagnostics.
   - Acceptance can be help text plus parser tests or a docs checker.
   - Do not claim a model runtime.

3. **Examples oracle admission gate**
   - Consume the examples tiny token/logits oracle fixture as a documented
     external oracle input.
   - Check only that Faber can locate and pass through declared fixture metadata.
   - Keep examples responsible for fake/oracle behavior and expected outputs.

## Stop Conditions

- Stop if a Faber packet starts parsing GGUF/safetensors/model tensors inside
  this CLI crate.
- Stop if help text or docs imply public inference support before runtime,
  tokenizer, tensor, placement, and oracle gates land.
- Stop if Faber target text contradicts Radix target capability rows.
- Stop if a package/session path downloads model artifacts implicitly or trusts
  an unchecked cache.
