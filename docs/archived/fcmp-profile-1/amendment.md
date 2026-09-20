# FCMP draft amendment — optional/default identity + envelope-prefix limits

**Status**: draft — planning locks only; not a freeze; not implementation
**Goal**: `docs/factory/fcmp-profile-1/goal.md` (`gol_58147dc9ef95b704`)
**Authority**: CTO `5a4e974a` `correct_before_next_phase`; need `d4ed1d7f`; planner `3fc06e37`
**Protocol target (out of this write scope)**: `docs/faber-messagepack-profile-v1.md` @ `b7bfffe`
**Do not**: freeze; implement; reopen unit 2; dispatch unit 3; dispatch postcard → rmp-serde

After a later fold of this text into the protocol file, a §14 re-check is
still not a freeze.

---

## Interpreted theme

The amended FCMP 1.0 draft is real protocol law. Two durable holes block any
freeze:

1. Optional-field vs default is not a single encoding.
2. Kind/schema prefix has no profile-level limits, so §6 cannot be implemented
   for the bytes a decoder must read before a kind exists.

This artifact names the missing BCP-14 rules and the numeric prefix caps. It
does not edit the protocol file.

---

## Normalized spec

### 1. Optional vs default identity — choice (a)

Absence of an OPTIONAL field and a present encoding whose value equals that
field's schema default are the **same schema value**.

An encoder **MUST** omit an OPTIONAL field when its schema value equals the
named default. A decoder **MUST** apply the named default when the field is
absent. A present OPTIONAL field whose decoded value equals the schema default
is `noncanonical` and **MUST** be rejected.

`nil` is distinct. A decoder **MUST NOT** treat `nil` as the encoding of an
omitted OPTIONAL field. `nil` **MUST** be emitted only when the field's type
lists null as a legal value. FCMP 1.0 OPTIONAL fields **MUST** name a default
that is not implicit null. A field whose type admits null is **REQUIRED** and
always carries the key (`nil` or a value).

Rejected alternative (b): treat absence and present-at-default as distinct
schema values. That forks exact-byte canonicality, contradicts landed §4.1
("absent optional MUST use the schema default"), and breaks §7.2 minor
evolution (old documents omit a newly added optional+default field).

### 2. Envelope-prefix limits

These profile-level limits apply to every decoder **before** kind admission.
They are not kind-specific and exist even when no kind is enabled.

| Limit | Value | Rule |
| --- | ---: | --- |
| Kind-string bytes | 64 | Decoder **MUST** reject a longer `kind` from the string header and **MUST NOT** allocate the payload. |
| Root map entries | 3 (exact) | Decoder **MUST** reject any other count from the map header and **MUST NOT** allocate entries. |
| Schema array elements | 2 (exact) | Decoder **MUST** reject any other count from the array header and **MUST NOT** allocate elements. |
| No-enabled-kind payload | 256 | A decoder with no enabled document kind **MUST** reject a declared payload length greater than 256 bytes. It **MUST NOT** allocate `value`. |

A decoder **MUST** read root fields in canonical order (`kind`, then `schema`,
then `value`). If the first key is not `kind` or the second is not `schema`,
that is an error **before** `value` allocation.

Enabled-kind declared-length checks stay as in §1 rules 5–6 (greatest enabled
kind, then the admitted kind). The 256-byte figure is only the empty-set cap
and a sufficient window to parse `kind`+`schema` under the table above
(canonical prefix is 85 bytes when `kind` is 64 bytes and schema integers use
uint16).

---

## Repo-aware baseline

- `docs/faber-messagepack-profile-v1.md` §3.1, §4.1, §5 item 10, §7.2 already
  require omit-when-absent, schema default on absence, distinct `nil`,
  exact-byte identity, and optional+default as a document-minor add. They do
  not say whether present-at-default is a second legal encoding.
- §1 rules 5–6 and §2 rule 4 require admitting `kind`/`schema` before `value`,
  but every finite limit in §6 is kind-specific. An empty enabled-kind set has
  no declared-length cap.
- Live FHIR is postcard + serde, not rmp-serde (`radix-hir-fhir`
  `decode.rs` / `package.rs`). Unit ratchet is `SCHEMA_VERSION = 3`; envelope
  ratchet is `PACKAGE_SCHEMA_VERSION = 1`. Not in this fold.
- Root shape is already exactly three named fields; kind grammar is short
  dotted ASCII (`fhir.unit` is 9 bytes). The numeric caps are those facts made
  fail-closed, not a new document family.

---

## Fold-in clauses (for a later protocol edit; not applied here)

Keep protocol **Status** as draft. Do not freeze.

### §3.1 / §4.1 — identity

Replace the optional-field bullets and the lowercase null sentence with:

- OPTIONAL fields **MUST** be omitted when the schema value equals the named
  default, including when the field is absent.
- Absence of an OPTIONAL field and a present encoding equal to that default
  **MUST** be treated as the same schema value.
- A present OPTIONAL field whose decoded value equals the schema default
  **MUST** be rejected as `noncanonical`.
- An absent OPTIONAL field **MUST** use the default named by the schema.
- A decoder **MUST NOT** rewrite a present field into an absent field, or an
  absent field into a present field, except by applying the named default in
  the schema value (never on the admitted bytes).
- `nil` **MUST** be emitted only when null is a legal, distinct field value.
- A decoder **MUST NOT** treat `nil` as the encoding of an omitted OPTIONAL
  field.
- FCMP 1.0 OPTIONAL fields **MUST** name a default that is not implicit null.
- A field whose type admits null **MUST** be REQUIRED and **MUST** always
  carry the key.

### §1 / §2 / §6 — prefix limits

Add the table in §2 above as profile-level limits. Amend §1 rule 5: when the
enabled-kind set is empty, the declared-payload cap is 256 bytes. State that
`kind`, root map, and schema array **MUST** be checked against those limits
from their MessagePack headers before allocation, and that root keys **MUST**
be read in canonical order so `value` is not allocated to learn `kind`.

### §11.1 — vectors

Add:

- Positive pair: a record whose OPTIONAL field is omitted (schema value =
  default) and the same record with that field present-and-equal-to-default
  (`noncanonical`).
- Negative: `kind` string longer than 64 bytes (header-only reject).
- Negative: declared payload length > 256 with no enabled kind.
- Negative: root map entry count ≠ 3, or schema array length ≠ 2.

### §14

Do not change §14 into a freeze. After the fold, a re-check of §14 is still
not a freeze.

---

## Hand unit graph

Unit 2 stays complete. Do not reopen it.

| Field | `U-2a` |
| --- | --- |
| `id` | `U-2a` |
| `outcome` | Fold the clauses above into `docs/faber-messagepack-profile-v1.md`. Protocol Status remains draft. |
| `write_scope` | `faber/docs/faber-messagepack-profile-v1.md` only |
| `done_when` | §4.1/§3.1 state choice (a); §1/§2/§6 state the four prefix limits; §11.1 names the vectors; Status is still draft; no freeze language. |
| `depends_on` | none (unit 2 already landed) |
| `sanity` | `rg -n 'noncanonical|256|kind-string|OPTIONAL' docs/faber-messagepack-profile-v1.md` plus a read of the Status line |
| `non_goals` | Freeze; implementation; unit 3; FHIR schemas; postcard → rmp-serde; kind-reuse leftover; error-class taxonomy; fixture kind `fcmp.test` |
| `risk` | low — docs-only fold of already-locked text |
| `integrable` | yes |

Mind prepares any Hand from this id. This planner does not file or dispatch it.

---

## Integration / merge gate

None. `U-2a` is integrable alone.

## Lane-owned validation

Docs read of the folded protocol. No package, source, compile, or suite gate.

## Open questions for Mind

- After `U-2a` lands, route CTO §14 re-check. That re-check is still not a freeze.
- Residuals (not this amend): kind-reuse **MUST NOT**; shared error-class
  taxonomy before unit 3; reserved fixture kind before unit 3; `policy.md`
  still does not carry the FCMP window.
- Implementation remains closed on freeze + RTR2. RTR3b remains the crate-home
  gate.
