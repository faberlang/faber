# Faber Canonical MessagePack Profile 1

**Status**: draft 1 — amendments folded; protocol review re-check required before implementation or freeze
**Profile version**: 1.0 (draft; not frozen)
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
same schema value MUST emit identical bytes. A conforming artifact decoder
rejects alternate encodings even when a generic MessagePack decoder would
produce an equivalent value.

FHIR is the first target FCMP document family. This profile is reusable
protocol law, not an instruction to build a generic framework before a second
consumer exists.

## Normative language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**,
**SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **NOT RECOMMENDED**, **MAY**, and
**OPTIONAL** are to be interpreted as described by BCP 14 when, and only when,
they appear in all capitals.

The base MessagePack syntax is the MessagePack specification at revision
`9aa092d6ca81f12005bd7dcbeb6488ad319e5133`:
<https://github.com/msgpack/msgpack/blob/9aa092d6ca81f12005bd7dcbeb6488ad319e5133/spec.md>.
A later change to the upstream specification is not part of FCMP until a
profile revision explicitly adopts it.

When the pinned base specification permits several representations, this
document selects exactly one. FCMP rules override implementation-library
defaults.

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
   greatest limit of its enabled document kinds. When the enabled-kind set is
   empty, that declared-payload cap is 256 bytes. A decoder with no enabled
   document kind MUST reject a declared payload length greater than 256 bytes
   and MUST NOT allocate `value`.
6. After reading `kind`, a decoder MUST validate the declared length against
   that kind's limit before allocating or reading the full payload.
   A decoder MUST check `kind`, the root map, and the schema array against
   the envelope-prefix limits (kind-string 64 bytes, root map exactly 3
   entries, schema array exactly 2 elements) from their MessagePack headers
   before allocation. Root keys MUST be read in canonical order (`kind`,
   then `schema`, then `value`) so `value` is not allocated to learn `kind`.
7. Compression, encryption, and signatures are external containers or
   transports. No FCMP 1.0 frame flag enables them. In-frame flags are
   intentionally not reserved; adding them is a profile-major change, not an
   unreviewed extension point.
8. A payload without the FCMP magic is not FCMP. Product decoders MAY identify
   a known legacy format for a specific diagnostic, but MUST NOT silently
   reinterpret it as FCMP.

The fixed preamble exists so a decoder can distinguish FCMP from legacy or
unrelated bytes without attempting multiple schema decoders.

## 2. Root document

The payload root MUST be a MessagePack map with exactly these named string
fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `kind` | string | Registered document kind. |
| `schema` | two-element array of unsigned integers | Document schema `[major, minor]`. |
| `value` | document-defined | The document schema value. |

The fields appear in canonical key order:

1. `kind`
2. `schema`
3. `value`

These envelope-prefix limits apply before kind admission. They are not
kind-specific and exist even when no kind is enabled:

| Limit | Value | Rule |
| --- | ---: | --- |
| Kind-string bytes | 64 | A decoder MUST reject a longer `kind` from the string header and MUST NOT allocate the string payload. |
| Root map entries | 3 (exact) | A decoder MUST reject any other count from the map header and MUST NOT allocate entries. |
| Schema array elements | 2 (exact) | A decoder MUST reject any other count from the array header and MUST NOT allocate elements. |
| No-enabled-kind payload | 256 | A decoder with no enabled document kind MUST reject a declared payload length greater than 256 bytes. It MUST NOT allocate `value`. |

A decoder MUST read root fields in canonical order (`kind`, then `schema`,
then `value`). If the first key is not `kind` or the second is not `schema`,
that is an error before `value` allocation.

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
   `value`. `kind` MUST be at most 64 bytes, the root map MUST have exactly
   3 entries, and `schema` MUST have exactly 2 elements; those counts MUST
   be checked from the MessagePack headers before allocation.
5. Unknown root fields, duplicate root fields, unsupported kinds, and
   unsupported schema versions are errors.
6. Named string fields are REQUIRED for the root. Numeric field IDs MUST NOT
   be used as an alternate root representation.

FHIR initially reserves `fhir.unit` and `fhir.package`; reservation does not
register or admit either kind.

## 3. Canonical primitive encoding

### 3.1 Nil and booleans

- Null is MessagePack `nil`.
- Boolean values use the MessagePack `false` and `true` opcodes.
- A schema MUST state where null is a legal value.
- OPTIONAL fields MUST be omitted when the schema value equals the named
  default, including when the field is absent.
- Absence of an OPTIONAL field and a present encoding equal to that default
  MUST be treated as the same schema value.
- A present OPTIONAL field whose decoded value equals the schema default
  MUST be rejected as `noncanonical`.
- An absent OPTIONAL field MUST use the default named by the schema.
- A decoder MUST NOT rewrite a present field into an absent field, or an
  absent field into a present field, except by applying the named default in
  the schema value (never on the admitted bytes).
- `nil` MUST be emitted only when null is a legal, distinct field value.
- A decoder MUST NOT treat `nil` as the encoding of an omitted OPTIONAL
  field.
- FCMP 1.0 OPTIONAL fields MUST name a default that is not implicit null.
- A field whose type admits null MUST be REQUIRED and MUST always carry
  the key.

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
IEEE-754 binary64 bits in network byte order. FCMP 1.0 adopts this rule, with
canonical NaN, for the current Faber FHIR carriers represented as `f64`.

- MessagePack float32 (`0xca`) is noncanonical.
- Positive and negative infinity retain their IEEE-754 encodings.
- Negative zero retains the binary64 negative-zero bit pattern.
- Every NaN is normalized to quiet NaN bits `0x7ff8000000000000`.
- The owning FHIR schema MUST define each numeric carrier's exact numeric
  semantics and whether NaN and infinity are admitted.
- A JSON-like numeric domain MUST reject non-finite values when its schema does
  not admit them.

The float64-only rule prevents language runtimes from changing the wire width
during decode and re-encode. It does not by itself define the meaning,
exactness, range, or non-finite-value policy of a future FHIR numeric field.

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

- field names are stable named string schema identifiers, not
  implementation-language member names discovered at runtime;
- named string field keys are REQUIRED for records;
- numeric field IDs MUST NOT be used as an alternate record representation;
- fields MUST be emitted in canonical map-key order;
- required fields MUST always be present;
- OPTIONAL fields MUST be omitted when the schema value equals the named
  default, including when the field is absent;
- absence of an OPTIONAL field and a present encoding equal to that default
  MUST be treated as the same schema value;
- a present OPTIONAL field whose decoded value equals the schema default
  MUST be rejected as `noncanonical`;
- an absent OPTIONAL field MUST use the default named by the schema;
- a decoder MUST NOT rewrite a present field into an absent field, or an
  absent field into a present field, except by applying the named default in
  the schema value (never on the admitted bytes);
- `nil` MUST be emitted only when null is a legal, distinct field value;
- a decoder MUST NOT treat `nil` as the encoding of an omitted OPTIONAL
  field;
- FCMP 1.0 OPTIONAL fields MUST name a default that is not implicit null;
- a field whose type admits null MUST be REQUIRED and MUST always carry
  the key; and
- unknown fields MUST be rejected after the document version has been
  admitted.

Numeric positions are permitted only where a schema intentionally chooses a
tuple or array and documents the resulting evolution cost. Adding a field in
the middle of a source-language struct cannot affect FCMP bytes unless the
document schema itself adds that field.

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
- `value` MUST be absent for a unit variant;
- `value` MUST be present for a payload variant, even when its payload is an
  empty record or array;
- fields MUST appear in canonical order (`tag`, then `value`);
- unknown tags MUST be rejected unless the document schema explicitly declares
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
6. REQUIRED, OPTIONAL, and unknown fields;
7. enum tags and payload shape;
8. integer ranges and document field types;
9. document structural and referential invariants (class `invariant`); and
10. byte canonicality.

A decoder MAY prove byte canonicality while parsing. An initial implementation
MAY decode, validate, canonically re-encode, and compare the complete payload,
provided resource limits are enforced before material allocation. A mismatch is
a `noncanonical` error, not a warning.

Artifact build, load, install, cache, hash, and execution routes MUST use strict
decoding. A permissive normalizer, if one exists, is an explicit developer tool
and never an implicit fallback.

### 5.1 Decoder error classes

Structured decoder errors MUST carry `class` as one of these lowercase ASCII
identifiers. Rust and TypeScript share the identifiers. Each row names a
protocol MUST-reject already written in §1–§7 / §10. Implementations MUST NOT
add classes. If a reject has no row, stop and route a protocol amend.

| `class` | Meaning (already required) |
| --- | --- |
| `truncated` | payload shorter than declared length, or unexpected end (§1.4) |
| `trailing_bytes` | bytes after the declared payload (§1.4) |
| `bad_magic` | preamble is not `FABERMP\0` (§1.8) |
| `profile_unsupported` | profile major/minor the decoder does not accept (§7.1) |
| `payload_limit` | declared length above the enabled-kind / empty-set cap (§1.5–6, §6) |
| `kind_string_limit` | `kind` string header longer than 64 bytes (§6) |
| `root_map` | root map count ≠ 3, or keys not `kind` then `schema` then `value` (§2, §6) |
| `schema_array` | `schema` array length ≠ 2, or elements not unsigned 16-bit (§2, §6) |
| `kind_unregistered` | unknown kind, or reserved-but-unregistered kind admitted (§2, §10) |
| `schema_unsupported` | document schema major/minor the decoder does not accept (§7.2) |
| `noncanonical` | any alternate encoding of a legal schema value, including present-at-default OPTIONAL, overlong primitives, float32, unsorted keys (§3, §5.10) |
| `duplicate_key` | duplicate map key, duplicate canonical set element, or duplicate canonical logical-map key (§3.7, §4.4, §4.5) |
| `unknown_field` | field not in the admitted schema, including present `value` on a unit variant (§4.1, §4.2) |
| `missing_field` | REQUIRED field absent, including absent `value` on a payload variant (§4.1, §4.2) |
| `type` | wrong primitive, `nil` where null is not legal, integer out of declared range, non-string MessagePack map key, or unknown closed-union tag (§3.1, §3.2, §3.7, §4.2, §5.8) |
| `utf8` | string payload is not valid UTF-8 (§3.4) |
| `extension` | MessagePack extension / timestamp (§3.8) |
| `limit` | a kind-specific resource limit; payload names the violated limit (§6) |
| `invariant` | a document-schema structural or referential invariant; payload names the violated invariant (§5.9) |
| `overflow` | length/budget arithmetic overflow (§6) |
| `legacy` | known non-FCMP product bytes (postcard FHIR) named for a diagnostic (§1.8, §13) |

## 6. Resource limits

These profile-level envelope-prefix limits apply to every decoder before
kind admission. They are not kind-specific and exist even when no kind is
enabled:

| Limit | Value | Rule |
| --- | ---: | --- |
| Kind-string bytes | 64 | A decoder MUST reject a longer `kind` from the string header and MUST NOT allocate the string payload. |
| Root map entries | 3 (exact) | A decoder MUST reject any other count from the map header and MUST NOT allocate entries. |
| Schema array elements | 2 (exact) | A decoder MUST reject any other count from the array header and MUST NOT allocate elements. |
| No-enabled-kind payload | 256 | A decoder with no enabled document kind MUST reject a declared payload length greater than 256 bytes. It MUST NOT allocate `value`. |

A decoder MUST check `kind`, the root map, and the schema array against
those limits from their MessagePack headers before allocation. A decoder
MUST read root keys in canonical order (`kind`, then `schema`, then
`value`) so `value` is not allocated to learn `kind`.

Enabled-kind declared-length checks stay as in §1 rules 5–6 (greatest
enabled kind, then the admitted kind). The 256-byte figure is only the
empty-set cap and a sufficient window to parse `kind`+`schema` under the
table above (canonical prefix is 85 bytes when `kind` is 64 bytes and
schema integers use uint16).

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

- MUST check frame and container lengths before allocation;
- MUST reject arithmetic overflow while computing lengths or budgets;
- MUST NOT reserve memory directly from an untrusted length without applying
  the admitted limit;
- MUST fail before semantic reconstruction when a limit is exceeded;
- MUST NOT recurse without a depth bound; and
- MUST report the violated limit as structured data.

FCMP does not assign one universal payload limit because a compiler unit, a
package, and future document families have different legitimate sizes. Missing
kind-specific limits make a registration incomplete.

## 7. Versioning and evolution

### 7.1 Profile version

The frame carries FCMP profile `[major, minor]`. FCMP profile versions are a
wire-protocol version and are separate from Faber product semantic versioning
and from the odd/even Faber release-lane policy in
`radix/docs/release/faber/policy.md`.

- Increment profile major when framing or canonical representation changes.
- Increment profile minor only for an additive rule that a newer decoder can
  apply while retaining complete support for every older minor in the same
  major.
- A decoder accepts its supported profile major and any profile minor less
  than or equal to its implemented minor.
- A decoder MUST reject a greater profile minor before decoding the root.
- When a producer targets a consumer that implements an older minor, the
  producer MUST select a minor the consumer supports or fail with an explicit
  incompatibility. A producer MUST NOT claim that a newer minor is compatible
  merely because the consumer can parse some of its bytes; downgrade requires
  an explicit re-encode under the older minor's rules.

Changing serializer-library output is not an evolution mechanism. If canonical
bytes change, the responsible profile or document version changes.

A profile-major migration MUST be handled at an explicit release boundary.
The release record MUST name the old and new profile majors, migration and
re-encoding behavior, reader and writer support, conformance vectors, and the
retirement date for the old major. During migration, a product MAY retain
readers for more than one profile major, but a document MUST NOT be relabeled
or decoded across profile majors; conversion is an explicit validated
re-encode. The current-plus-two document-major window in §7.3 does not cover
profile-major retention.

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

For each registered document kind, the Faber product SHALL retain readers for
that kind's current stable FCMP document major and the two previous stable
document majors. This current-plus-two window is the FCMP support window; it is
not an automatic consequence of Faber product semver. "Current stable" is
resolved independently for each document kind by its registry entry. A draft
major does not count as stable, and each retained major MUST have its own
format-owned DTOs and validation. Compatibility MUST NOT be simulated by
deserializing old bytes into current compiler structs.

The support window begins with the first stable FCMP schema for each kind.
Legacy postcard FHIR is not FCMP major zero and is not included in this window.
The release policy at `radix/docs/release/faber/policy.md` requires every
release line to state a support window; the release record MUST explicitly
carry this FCMP current-plus-two cost and its ownership rather than implying
that product semver approves it automatically.

Dropping the oldest supported document major for a kind is a release-boundary
change recorded in release notes and that kind's registry entry. It MUST NOT
retire a profile major implicitly; profile-major retirement follows §7.1.

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

The public Faber repository owns the FCMP document-kind registry. Every
registered entry MUST record:

- stable kind string;
- owning schema path;
- current stable schema version;
- supported schema-major window;
- finite resource limits for that kind;
- file extensions, if any;
- reference implementation and independent conformance implementation; and
- fixture-manifest path.

Kind strings MUST NOT be reused. A retired kind remains reserved. A reservation
is a namespace claim, not a registration: `fhir.unit` and `fhir.package` remain
reserved names until their owning FHIR schemas publish concrete finite limits
and the other registry fields. `fcmp.test` is reserved as a generic-vector
fixture kind and is not registered. Product decoders MUST NOT admit `fcmp.test`
while it is reserved. Registration without concrete limits MUST fail,
and an implementation MUST NOT admit a reserved-but-unregistered kind.

Initial reservations:

| Kind | Purpose | Registration state |
| --- | --- | --- |
| `fhir.unit` | Post-RTR2 FHIR module contract: a stable module ID/record with schema-defined contents and import edges | reserved; schema and limits not published |
| `fhir.package` | Post-RTR2/RTR3b FHIR analyzed-program graph with explicit roots, optional entry, stable module records, import edges, library identities, and graph-specific limits | reserved; schema and limits not published |
| `fcmp.test` | Generic-vector fixture kind for profile conformance | reserved; not registered |

This document reserves the names but does not register or freeze the FHIR
schemas. The `fhir.unit` schema MUST be defined against the post-RTR2 module
contract. The `fhir.package` schema MUST be defined against the post-RTR2/RTR3b
`AnalyzedProgram` graph contract; each schema MUST NOT use a legacy vector-era
envelope as its authoritative semantic shape. `fcmp.test` remains reserved
for generic profile vectors; reservation does not register it or admit it
to product decoders.

## 11. Conformance

### 11.1 Generic profile vectors

The profile suite MUST include canonical-byte vectors for:

- integer boundaries around fixint and every integer width;
- negative zero, infinities, ordinary floats, and noncanonical/canonical NaNs;
- string, binary, array, and map header-width boundaries;
- sorted and unsorted maps;
- duplicate map keys;
- records with absent OPTIONAL fields and explicit nullable values;
- a positive pair: a record whose OPTIONAL field is omitted (schema value
  equals the named default) and the same record with that field present and
  equal to the default (`noncanonical`);
- a `kind` string longer than 64 bytes (header-only reject);
- a declared payload length greater than 256 bytes with no enabled kind;
- a root map entry count other than 3, or a schema array length other than 2;
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
   error `class` from §5.1; and
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

1. After RTR2 establishes the module contract and RTR3b establishes the
   analyzed-program graph contract, define the first FHIR FCMP schemas and
   publish their concrete finite limits.
2. Re-check this draft and make the separate operator-approved decision to
   freeze FCMP 1.0; do not treat the current draft as frozen.
3. Add format-owned FHIR wire DTOs and conversion boundaries.
4. Add strict Rust encoding/decoding and generic profile fixtures.
5. Add FHIR unit/package fixtures and an independent TypeScript decoder and
   encoder.
6. Switch FHIR writers to FCMP only.
7. Switch FHIR readers to FCMP only.
8. Reject legacy postcard FHIR with a structured legacy-format diagnostic.
9. Keep postcard only where independently required, including FMIR.

The `fhir.unit` DTO MUST follow the post-RTR2 module contract. The
`fhir.package` DTO MUST follow the post-RTR2/RTR3b `AnalyzedProgram` graph:
explicit roots, an optional entry, stable module records, import edges,
library identities, and graph-specific limits. Current compiler structs and
vector-era envelopes are migration inputs, not DTO templates.

There is no dual writer, codec negotiation, silent fallback, or indefinite
postcard compatibility decoder.

## 14. Review gates before profile 1.0 freezes

The profile is ready to freeze only when review confirms:

- exact-byte canonicality is REQUIRED;
- named string fields for records and roots are REQUIRED, and numeric field
  IDs MUST NOT be alternate record representations;
- strict artifact admission MUST reject rather than normalize noncanonical
  bytes;
- float64 with canonical NaN MUST cover current Faber FHIR `f64` carriers, and
  each future FHIR schema MUST define numeric semantics and NaN/infinity
  admission;
- the 20-byte frame and root map are sufficient without flags, with any future
  in-frame flags requiring a profile-major change;
- profile versions are separate from Faber product semver and profile/document
  major/minor rules match the release policy, including explicit newer-minor
  rejection and profile-major migration boundaries;
- current-plus-two document-major retention is recorded as the per-kind FCMP
  support window and explicitly carried by the release policy record; and
- `fhir.unit` and `fhir.package` remain reservations until their FHIR schemas
  publish concrete finite per-kind limits, because registration without limits
  MUST fail.

Implementation MUST NOT begin while one of these choices is still being
treated as a serializer-library default, and the draft MUST NOT be declared
frozen until this gate is re-checked after the amendments.

## References

- MessagePack specification, pinned at revision
  `9aa092d6ca81f12005bd7dcbeb6488ad319e5133`:
  <https://github.com/msgpack/msgpack/blob/9aa092d6ca81f12005bd7dcbeb6488ad319e5133/spec.md>
- BCP 14 / RFC 2119:
  <https://www.rfc-editor.org/rfc/rfc2119>
- BCP 14 / RFC 8174:
  <https://www.rfc-editor.org/rfc/rfc8174>
- `rmp-serde` serializer documentation (non-normative implementation
  evidence):
  <https://docs.rs/rmp-serde/latest/rmp_serde/encode/struct.Serializer.html>
