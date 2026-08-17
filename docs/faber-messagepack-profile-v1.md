# Faber Canonical MessagePack Profile 1

**Status**: draft 1 — protocol review required before implementation
**Profile version**: 1.0
**Created**: 2026-08-16
**Initial consumer**: FHIR unit and package artifacts

## Abstract

The Faber Canonical MessagePack Profile (FCMP) defines one language-neutral,
byte-deterministic use of MessagePack for durable Faber artifacts. MessagePack
defines a family of encodings for values; it does not define canonical bytes,
struct or enum representation, schema evolution, application framing, resource
limits, or duplicate-key behavior. FCMP supplies those missing rules.

An FCMP document has:

1. a fixed binary preamble;
2. a canonical MessagePack root carrying document kind and schema version; and
3. a document-specific value governed by a separately versioned schema.

Canonical bytes are part of the contract. Two conforming encoders given the
same schema value must emit identical bytes. A conforming artifact decoder
rejects alternate encodings even when a generic MessagePack decoder would
produce an equivalent value.

FHIR is the first registered FCMP document family. This profile is reusable
protocol law, not an instruction to build a generic framework before a second
consumer exists.

## Normative language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**,
**SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **NOT RECOMMENDED**, **MAY**, and
**OPTIONAL** are to be interpreted as described by BCP 14 when, and only when,
they appear in all capitals.

The base MessagePack syntax is the current MessagePack specification:
<https://github.com/msgpack/msgpack/blob/master/spec.md>.

When the base specification permits several representations, this document
selects exactly one. FCMP rules override implementation-library defaults.

## Goals

FCMP exists to provide:

- exact, deterministic artifact bytes across implementations and languages;
- explicit document identity before document payload interpretation;
- independent profile and document-schema evolution;
- a stable representation that does not inherit Rust field order, enum
  ordinals, collection hash order, or serializer-library defaults;
- fail-closed decoding of malformed, unsupported, ambiguous, or excessive
  inputs;
- reproducible content hashes and fixtures over complete framed documents; and
- a conformance surface suitable for Rust, TypeScript, Go, Swift, Python, and
  other MessagePack implementations.

## Non-goals

FCMP does not:

- define FHIR's HIR, package, or semantic schemas;
- make compiler-owned Rust structs into public wire types;
- guarantee that an external producer can construct semantically valid
  analyzed HIR;
- define compression, encryption, signatures, transport, package discovery, or
  registry storage;
- provide a human-authored or human-readable source format;
- canonicalize arbitrary MessagePack documents; or
- change FMIR's existing codecs or version-coupled execution role.

## Terminology

| Term | Meaning |
| --- | --- |
| **Profile** | This framing and canonical representation contract. |
| **Document kind** | A registered stable identity such as `fhir.unit`. |
| **Document schema** | The kind-specific field, tag, type, and invariant contract. |
| **Schema value** | The abstract typed value defined by a document schema before encoding. |
| **Canonical bytes** | The one FCMP encoding permitted for a schema value. |
| **Strict decoder** | An artifact-admission decoder that enforces framing, versions, limits, schema, and canonical bytes. |
| **Normalizer** | Optional tooling that reads a noncanonical value and emits canonical bytes; never an artifact-admission path. |

## 1. Frame

Every FCMP document begins with this fixed 20-byte preamble:

| Offset | Size | Field | Encoding |
| ---: | ---: | --- | --- |
| 0 | 8 | magic | bytes `46 41 42 45 52 4d 50 00` (`FABERMP\0`) |
| 8 | 2 | profile major | unsigned 16-bit, big-endian |
| 10 | 2 | profile minor | unsigned 16-bit, big-endian |
| 12 | 8 | payload length | unsigned 64-bit, big-endian |

The MessagePack payload begins at offset 20 and occupies exactly `payload
length` bytes.

Frame rules:

1. Profile 1.0 encodes profile major `1` and profile minor `0`.
2. The payload length excludes the 20-byte preamble.
3. The file or stream MUST end immediately after the declared payload.
4. A decoder MUST reject truncated payloads and trailing bytes.
5. Before the kind is known, a decoder MUST reject a declared length above the
   greatest limit of its enabled document kinds.
6. After reading `kind`, a decoder MUST validate the declared length against
   that kind's limit before allocating or reading the full payload.
7. Compression, encryption, and signatures are external containers or
   transports. No FCMP 1.0 frame flag enables them.
8. A payload without the FCMP magic is not FCMP. Product decoders MAY identify
   a known legacy format for a specific diagnostic, but MUST NOT silently
   reinterpret it as FCMP.

The fixed preamble exists so a decoder can distinguish FCMP from legacy or
unrelated bytes without attempting multiple schema decoders.

## 2. Root document

The payload root MUST be a MessagePack map with exactly these fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `kind` | string | Registered document kind. |
| `schema` | two-element array of unsigned integers | Document schema `[major, minor]`. |
| `value` | document-defined | The document schema value. |

The fields appear in canonical key order:

1. `kind`
2. `schema`
3. `value`

Example in diagnostic notation:

```text
{
  "kind": "fhir.unit",
  "schema": [1, 0],
  "value": { ... }
}
```

Root rules:

1. A document kind MUST be a registered lowercase ASCII dotted identifier.
2. Profile 1.0 document kinds use the grammar
   `[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+`.
3. Schema major and minor values MUST fit unsigned 16-bit integers.
4. A decoder MUST admit `kind` and `schema` before allocating or interpreting
   `value`.
5. Unknown root fields, duplicate root fields, unsupported kinds, and
   unsupported schema versions are errors.

FHIR initially reserves `fhir.unit` and `fhir.package`.

## 3. Canonical primitive encoding

### 3.1 Nil and booleans

- Null is MessagePack `nil`.
- Boolean values use the MessagePack `false` and `true` opcodes.
- A schema MUST state where null is a legal value. Null is never an implicit
  substitute for an absent optional field.

### 3.2 Integers

Integers MUST use the shortest MessagePack representation for their numeric
value:

- values from 0 through 127 use positive fixint;
- values from -32 through -1 use negative fixint;
- larger nonnegative values use the narrowest unsigned integer form; and
- smaller negative values use the narrowest signed integer form.

Integer width and signedness are schema constraints, not wire metadata. A
decoder MUST reject a value outside the field's declared range. An encoder MUST
NOT use an overlong integer representation to preserve an implementation
language's in-memory integer width.

### 3.3 Floating-point numbers

All floating-point values use MessagePack float64 (`0xcb`) followed by the
IEEE-754 binary64 bits in network byte order.

- MessagePack float32 (`0xca`) is noncanonical.
- Positive and negative infinity retain their IEEE-754 encodings.
- Negative zero retains the binary64 negative-zero bit pattern.
- Every NaN is normalized to quiet NaN bits `0x7ff8000000000000`.
- A document schema MAY forbid non-finite values.

The float64-only rule prevents language runtimes from changing the wire width
during decode and re-encode.

### 3.4 Strings

- Strings use the shortest MessagePack string header valid for their byte
  length.
- String payloads MUST be valid UTF-8.
- FCMP does not normalize document data strings globally. A document schema
  MUST state any Unicode normalization requirement for a particular field.
- Field names, enum tags, document kinds, and other schema identifiers are
  lowercase ASCII unless their document schema explicitly says otherwise.
- Invalid UTF-8 is an error, not replacement-character recovery.

### 3.5 Binary values

- Byte sequences use the shortest MessagePack binary header.
- Byte sequences MUST NOT be encoded as strings or arrays of integers.
- The legacy MessagePack raw/string ambiguity is not admitted.

### 3.6 Arrays

- Arrays use the shortest MessagePack array header valid for their length.
- Array order is significant.
- An encoder MUST know the final array length before emitting the header.

### 3.7 Maps

- Maps use the shortest MessagePack map header valid for their entry count.
- MessagePack map keys in FCMP MUST be strings.
- Keys MUST be unique.
- Keys MUST appear in ascending lexicographic order of their UTF-8 payload
  bytes.
- An encoder MUST know the final entry count before emitting the header.
- A decoder MUST reject duplicate or out-of-order keys.

### 3.8 Extension values

MessagePack extension values, including the timestamp extension, are forbidden
in FCMP 1.0. Document schemas represent such concepts with ordinary,
explicitly specified fields.

## 4. Canonical schema shapes

### 4.1 Records and structs

A record is a MessagePack map:

- field names are stable schema identifiers, not implementation-language
  member names discovered at runtime;
- fields are emitted in canonical map-key order;
- required fields are always present;
- optional fields are omitted when absent;
- an absent optional field uses the default named by the schema;
- `nil` is emitted only when null is a legal, distinct field value; and
- unknown fields are rejected after the document version has been admitted.

Adding a field in the middle of a source-language struct cannot affect FCMP
bytes unless the document schema itself adds that field.

### 4.2 Enums and tagged unions

Enum discriminants MUST NOT use source-language ordinals or a serializer's
default enum representation.

A variant without data is:

```text
{ "tag": "variant_name" }
```

A variant with data is:

```text
{ "tag": "variant_name", "value": ... }
```

Rules:

- `tag` is a stable lowercase ASCII schema identifier;
- `value` is absent for a unit variant;
- `value` is present for a payload variant, even when its payload is an empty
  record or array;
- fields appear in canonical order (`tag`, then `value`);
- unknown tags are rejected unless the document schema explicitly declares
  the union open; and
- renaming, reusing, or changing the meaning of a tag is a breaking schema
  change.

### 4.3 Tuples

A tuple is a fixed-length array. The document schema defines each position.
Changing tuple length, position, or member meaning is a breaking schema change.
Records SHOULD be preferred when independent field evolution is expected.

### 4.4 Logical maps

A schema map with string keys MAY use a MessagePack map and follows the
canonical map rules.

A schema map with any other key type is encoded as an array of two-element
`[key, value]` arrays. Entries are sorted by the lexicographic order of each
key's complete canonical MessagePack encoding under this profile. Duplicate
canonical keys are rejected.

### 4.5 Logical sets

A set is encoded as an array sorted by the lexicographic order of each
element's complete canonical MessagePack encoding under this profile.
Duplicate canonical elements are rejected.

Compiler hash-table iteration order is never a wire order.

## 5. Canonical admission

A strict decoder MUST validate all of the following:

1. frame magic, profile version, payload length, and exact end-of-frame;
2. document kind and document schema version;
3. document-kind resource limits;
4. MessagePack syntax and primitive canonicality;
5. map key type, uniqueness, and order;
6. required, optional, and unknown fields;
7. enum tags and payload shape;
8. integer ranges and document field types;
9. document structural and referential invariants; and
10. byte canonicality.

A decoder MAY prove byte canonicality while parsing. An initial implementation
MAY decode, validate, canonically re-encode, and compare the complete payload,
provided resource limits are enforced before material allocation. A mismatch is
a `noncanonical` error, not a warning.

Artifact build, load, install, cache, hash, and execution routes MUST use strict
decoding. A permissive normalizer, if one exists, is an explicit developer tool
and never an implicit fallback.

## 6. Resource limits

Every registered document kind MUST define finite limits for:

- complete frame bytes;
- nesting depth;
- array elements;
- map entries;
- string bytes;
- binary bytes;
- total decoded nodes or equivalent allocation budget; and
- kind-specific multiplicities such as package modules or embedded units.

A conforming decoder:

- checks frame and container lengths before allocation;
- rejects arithmetic overflow while computing lengths or budgets;
- does not reserve memory directly from an untrusted length without applying
  the admitted limit;
- fails before semantic reconstruction when a limit is exceeded;
- does not recurse without a depth bound; and
- reports the violated limit as structured data.

FCMP does not assign one universal payload limit because a compiler unit, a
package, and future document families have different legitimate sizes. Missing
kind-specific limits make a registration incomplete.

## 7. Versioning and evolution

### 7.1 Profile version

The frame carries profile `[major, minor]`.

- Increment profile major when framing or canonical representation changes.
- Increment profile minor only for an additive rule that a newer decoder can
  apply while retaining complete support for every older minor in the same
  major.
- A decoder accepts its supported profile major and any profile minor less
  than or equal to its implemented minor.
- A decoder rejects a greater profile minor before decoding the root.

Changing serializer-library output is not an evolution mechanism. If canonical
bytes change, the responsible profile or document version changes.

### 7.2 Document schema version

The root carries document schema `[major, minor]`.

Document major changes include:

- changing a field's type, meaning, default, or required status;
- removing or renaming a field or enum tag;
- adding a required field;
- changing a tuple position;
- changing a canonical sort key;
- admitting an old value with a different meaning; or
- tightening an invariant so previously valid documents become invalid.

Document minor changes MAY include:

- adding an optional field with an explicit default;
- adding metadata whose absence retains the old meaning;
- relaxing an invariant without changing old meanings; or
- adding a variant only to a union that was already declared open.

A document decoder accepts its supported major and any minor less than or
equal to its implemented minor. It rejects a greater minor before decoding
`value`.

### 7.3 Compatibility decoder policy

The Faber product SHALL retain readers for the current stable FCMP document
major and the two previous stable document majors. Each retained major has its
own format-owned DTOs and validation; compatibility MUST NOT be simulated by
deserializing old bytes into current compiler structs.

The support window begins with the first stable FCMP schema. Legacy postcard
FHIR is not FCMP major zero and is not included in this window.

Dropping the oldest supported major is a release-boundary change recorded in
release notes and the document-kind registry.

## 8. Schema ownership

A durable FCMP schema is language-neutral and format-owned.

- Compiler-owned AST, HIR, MIR, resolver, arena, and package structs MUST NOT
  be serialized directly as the public wire contract.
- Each document family owns explicit wire DTOs or equivalent schema values.
- Conversion between compiler state and wire values occurs at a reviewed
  boundary.
- Field identifiers and enum tags are explicit constants or generated from a
  normative schema, never inferred from Rust declaration order or enum
  ordinals.
- Unordered compiler collections are normalized before encoding.
- Compatibility DTOs remain isolated from current compiler types.

Generic Serde serialization MAY be used behind a canonical encoder only when
the emitted representation is fully controlled and conformance-proven. Generic
`rmp-serde` defaults, `to_vec`, `to_vec_named`, derived enum layouts, and
implementation map iteration order are not FCMP contracts.

## 9. Content identity and integrity

The canonical content identity of a document is the SHA-256 digest of the
complete FCMP frame: preamble plus payload.

- The digest is lowercase hexadecimal when rendered as text.
- Stores, locks, release manifests, and transport metadata carry the digest
  outside the document.
- A document MUST NOT embed a digest of its own complete bytes.
- A source-content hash inside FHIR is provenance for the source input. It is
  not proof of FHIR artifact integrity.
- Signature and trust policy are outside FCMP; a signature system signs the
  canonical frame bytes or their canonical SHA-256 digest.

Semantic equality does not replace byte identity for artifact hashes.

## 10. Document-kind registry

The public Faber repository owns the FCMP document-kind registry. Every entry
records:

- stable kind string;
- owning schema path;
- current stable schema version;
- supported schema-major window;
- resource limits;
- file extensions, if any;
- reference implementation and independent conformance implementation; and
- fixture-manifest path.

Kind strings are never reused. A retired kind remains reserved.

Initial reservations:

| Kind | Purpose | Initial schema |
| --- | --- | --- |
| `fhir.unit` | One analyzed-HIR compilation unit | draft `[1, 0]` |
| `fhir.package` | FHIR package envelope and embedded units | draft `[1, 0]` |

This document reserves the names but does not freeze the FHIR schemas.

## 11. Conformance

### 11.1 Generic profile vectors

The profile suite MUST include canonical-byte vectors for:

- integer boundaries around fixint and every integer width;
- negative zero, infinities, ordinary floats, and noncanonical/canonical NaNs;
- string, binary, array, and map header-width boundaries;
- sorted and unsorted maps;
- duplicate map keys;
- records with absent optional fields and explicit nullable values;
- unit and payload enum variants;
- logical maps with non-string keys;
- sets with competing element encodings;
- malformed frame lengths, truncation, and trailing bytes;
- invalid UTF-8;
- forbidden extension values;
- overlong primitive encodings;
- unsupported profile, kind, and schema versions; and
- every resource-limit failure class.

### 11.2 Document vectors

Each document schema MUST provide:

- the smallest valid document;
- a representative document exercising every field and closed enum variant;
- canonical bytes and SHA-256 for every positive fixture;
- decoded semantic expectations independent of one implementation language;
- negative fixtures for every structural and referential error class;
- fixtures for every supported schema major; and
- an explicit provenance record naming the schema and encoder version that
  produced each fixture.

### 11.3 Cross-language gate

A document family is not stable until:

1. the reference Rust implementation decodes and re-encodes every positive
   fixture byte-identically;
2. one independent non-Rust implementation does the same;
3. each implementation rejects every shared negative fixture with the expected
   error class; and
4. repeated generation in fresh processes produces identical bytes and hashes.

FHIR's first independent implementation is expected to be TypeScript. That
implementation proves the wire contract only. Producing a semantically valid
FHIR package from TypeScript source is separate compiler/frontend work.

## 12. Implementation and dependency changes

Encoder or decoder dependency upgrades MUST run the complete generic and
document conformance suites.

- Identical fixture bytes require no profile or schema bump.
- Changed bytes for the same schema value are a contract break, even if both
  versions decode to equal in-memory values.
- A library's claim of MessagePack compliance is insufficient evidence of FCMP
  compliance.
- Conformance code SHOULD exercise low-level MessagePack primitives where a
  high-level serializer cannot enforce the profile.

Reference and independent implementations SHOULD share fixture bytes and
expected outcomes, but MUST NOT share encoding implementation code.

## 13. FHIR migration posture

The FHIR format goal that adopts FCMP follows this migration:

1. Freeze FCMP 1.0 and the first FHIR FCMP unit/package schemas.
2. Add format-owned FHIR wire DTOs and conversion boundaries.
3. Add strict Rust encoding/decoding and generic profile fixtures.
4. Add FHIR unit/package fixtures and an independent TypeScript decoder and
   encoder.
5. Switch FHIR writers to FCMP only.
6. Switch FHIR readers to FCMP only.
7. Reject legacy postcard FHIR with a structured legacy-format diagnostic.
8. Keep postcard only where independently required, including FMIR.

There is no dual writer, codec negotiation, silent fallback, or indefinite
postcard compatibility decoder.

## 14. Review gates before profile 1.0 freezes

The profile is ready to freeze when review confirms:

- exact-byte canonicality is required;
- named string fields are preferred over numeric field IDs;
- strict artifact admission rejects rather than normalizes noncanonical bytes;
- float64 and canonical-NaN rules cover FHIR's numeric domain;
- the 20-byte frame and root map are sufficient without flags;
- profile and document major/minor rules match release policy;
- current-plus-two document-major retention is acceptable; and
- FHIR supplies concrete per-kind resource limits in its own schema.

No implementation should begin while one of these choices is still being
treated as a serializer-library default.

## References

- MessagePack specification:
  <https://github.com/msgpack/msgpack/blob/master/spec.md>
- BCP 14 / RFC 2119:
  <https://www.rfc-editor.org/rfc/rfc2119>
- BCP 14 / RFC 8174:
  <https://www.rfc-editor.org/rfc/rfc8174>
- `rmp-serde` serializer documentation (non-normative implementation
  evidence):
  <https://docs.rs/rmp-serde/latest/rmp_serde/encode/struct.Serializer.html>
