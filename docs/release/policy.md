# Faber release policy

**Status:** normative product-release policy
**Applies to:** the coordinated Faber language/toolchain line, including the
Faber CLI, compiler language contract, ReaderPack contract, standard packages,
and published ABI/wire/package surfaces.

This policy separates a version's release lane from the evidence for a
particular release. A passing release gate does not by itself make a major
language-locked.

## Major parity

- **Odd major versions are development lines.** They are the place for
  language, compiler, ReaderPack, standard-package, and contract evolution.
  Faber 1 is the first coordinated public development line.
- **Even major versions are language-locked LTS lines.** Faber 2 is the first
  locked/LTS line; the planned sequence is Faber 3 development, Faber 4 LTS,
  and so on.
- The major number is a release-lane signal, not a substitute for the lock
  record. Each locked line must publish its own compatibility boundary and
  evidence before the line is described as LTS.

Faber `1.0.0` is historically the first coordinated public release. It is not
feature-locked: its `1.x` development line may still evolve under this policy.
The historical release note records what shipped at `1.0.0`; it does not turn
that release into an LTS baseline.

## Meaning of language-locked

A language-locked major freezes the contract that a conforming implementation
and package ecosystem rely on:

1. **Grammar and semantics:** no intentional change may make valid source stop
   parsing, change the meaning of valid source, or add a breaking type,
   ownership, evaluation, or diagnostic contract change within the locked line.
2. **ReaderPacks:** pack schema, locale identity, inheritance/fallback rules,
   diagnostic rendering requirements, and any localized source/re-emission
   contract are frozen. New translations may fill an explicitly compatible
   extension point; they do not silently redefine the contract.
3. **Standard packages:** public module names, signatures, documented behavior,
   and package activation rules remain compatible. A breaking removal or
   semantic replacement waits for the next development/major boundary.
4. **ABI, wire, and package formats:** generated-runtime ABI, serialized or
   network wire formats, manifests, and package-image contracts remain
   readable and interoperable according to their published compatibility
   rules. Format changes need an additive or versioned path that preserves the
   locked contract.
5. **Compatibility:** a locked line may receive documentation corrections,
   compatible additions, portability fixes, ordinary bug fixes, and security
   fixes. A security fix may reject input that was never valid under the
   documented contract. It may not be used as a pretext for unrelated language
   redesign.

A bug fix that restores the documented grammar, semantics, package behavior, or
ABI/wire contract is maintenance, even when it changes an implementation's
incorrect prior behavior. Any fix that knowingly changes a documented contract
must be called out as a compatibility exception and routed for explicit review.

## Branches, channels, and support

- An odd-major development branch is the integration point for planned
  evolution and may carry intentional breaking changes within its documented
  development boundary.
- An even-major LTS branch is the maintenance point for the frozen language
  contract. Backports must remain within the lock rules above.
- Release channels must identify the major line explicitly; `latest` must not
  blur a development line with an LTS line. Documentation, ReaderPack bundles,
  package manifests, and compatibility matrices identify the line they serve.
- A locked line needs a stated support window and an end-of-support notice in
  its release record. This policy does not invent a duration where none has
  been approved.

## Lock transition gate

Before an even-major LTS line is opened, the release record must include:

- a grammar and semantic contract snapshot plus a compatibility corpus;
- ReaderPack schema, installed-pack, fallback, diagnostics, and re-emission
  evidence;
- standard-package API/behavior and activation inventories;
- ABI, wire-format, manifest, and package-image compatibility vectors;
- a list of allowed maintenance changes and any explicitly versioned extension
  points;
- branch/channel/support ownership and a migration guide from the prior
  development line; and
- an operator-approved lock decision tied to the exact source and artifact
  heads.

Missing evidence leaves the line development, regardless of its version string.

## Corporate-record boundary

Faber documentation must not duplicate a corporate series record, invent a
series identifier, or claim external registered-series status. Any corporate
relationship is an internal Swarm record and is not established by a Faber
release, package, website, or version label. When Swarm reports the canonical
record and identifier, Faber may link to that record; until then, no corporate
series claim is made here.
