//! Named-field `MessagePack` codec for the FHIR unit artifact (FPF-2).
//!
//! Implements the field spec in
//! `faber/docs/factory/fhir-package-format/spec.md` (FPF-1, faber `0a305c4`)
//! for the unit schema v6:
//!
//! - **§1.1 named fields** — every struct is a `MessagePack` map keyed by the
//!   serde field names of the defining type; never a compact array.
//! - **§1.2 versioning** — `schema_version` is a named `uint` field read
//!   first; mismatch fails closed before the payload is interpreted.
//! - **§1.3 no float-opcode facts** — the codec emits float opcodes only for
//!   genuine `f32`/`f64` values; widths, indices, and dtype/shape ids are
//!   `uint`/`int` per the Rust field type.
//! - **§1.4 enums** — `{ "tag": <variant name>, ...payload }`; struct-variant
//!   payload fields are inlined next to `"tag"`, newtype/tuple payloads ride
//!   under the `"value"` key.
//! - **§1.5 options** — bare payload for `Some`, `nil` for `None`.
//! - **§1.6 scalar types** — `uint`/`int` matching the Rust width, `bool`,
//!   `str`, `bin` for byte payloads.
//! - **§2 rows 10/13** — the `(DefId, …)` tuple families (`function_facts`
//!   entries, `gpu_builtins`) are carried as named maps `{ def_id, facts }`
//!   and `{ def_id, builtin }` per the spec's `MessagePack` type rules.
//!
//! Postcard envelope bytes are the retired legacy v1 wire: they fail closed
//! with the named legacy error
//! ([`crate::package::FhirPackageError::LegacyPostcardPackage`]) — there is
//! no fallback decode. The legacy postcard envelope codec itself stays in
//! [`crate::package`] pinned to v1 for migration reads only.
//!
//! The codec is hand-rolled (no `rmp`/`rmp-serde` dependency): a generic
//! serde `Serializer` over `MessagePack` bytes and a generic `Deserializer`
//! over a parsed [`Value`] tree, so the whole directly-serde'd `HirModule`
//! tree maps without per-type code. Multi-byte scalars are big-endian per
//! the `MessagePack` specification.

use serde::de::{DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};
use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::{Deserialize, Deserializer, Serialize};

use crate::artifact::{
    CliProgramWire, ContractRolesWire, DisplayTokensWire, FunctionFactTableWire, FunctionFactWire,
    FunctionProvenanceTableWire, GpuBuiltinWire, HirArtifact, KernelRequirementsWire,
    ProviderCarriageEntryWire, RadixLaneEntryWire, ResolvedUseWire, ResolverSnapshot,
    SCHEMA_VERSION, ShapeObligationTableWire, SourceIdentity, TensorEdgePolicyTableWire,
};
use crate::decode::{decode as decode_unit_bytes, encode as encode_unit_bytes};
use crate::error::HirArtifactError;
use crate::package::{
    FhirPackage, FhirPackageError, FhirPackageModule, LibraryImportWire, LocalLinkWire,
    PACKAGE_SCHEMA_VERSION, PackageDependencyWire, PackageIdentityWire, validate_dependencies,
    validate_local_links, validate_module_table,
};
use crate::shape::wire_shape_hash;
use crate::validate::validate_referential_integrity;
use radix_hir::{HirModule, HirPresentation, LibraryRegistry};
use radix_types::{DefId, ResolvedGenericCall, TypeTableSnapshot};
use std::collections::BTreeMap;

/// Maximum nesting depth accepted when parsing a `MessagePack` document.
///
/// Fail-closed guard against stack exhaustion on hostile input; real unit
/// artifacts nest far below this.
const MAX_DEPTH: usize = 1024;

/// `MessagePack` extension type id of a signed 128-bit integer.
///
/// `MessagePack` integers stop at 64 bits, so a Rust `i128` rides the one
/// project-owned application extension (type ids 0..=127 are application
/// owned): `fixext 16` (marker `0xd8`), this type id, then the value as 16
/// bytes of big-endian two's complement. Every `i128` field is written this
/// way, whatever its magnitude, so the wire form is a function of the Rust
/// type alone (spec §1.6) and a value never reads back as a narrower `int`.
/// Any other extension type fails closed.
const I128_EXT_TYPE: u8 = 1;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Structured error for the FHIR `MessagePack` unit codec (fail-closed, F7).
#[derive(Debug)]
pub enum MsgpackError {
    /// Serialization failed (unsupported data shape or oversized length).
    Encode(String),
    /// The payload ended before a complete `MessagePack` value was read.
    Truncated,
    /// The payload is not a spec-conformant `MessagePack` unit value
    /// (bad marker, non-UTF-8 string, wrong shape, missing named field).
    Decode(String),
    /// The unit value carries a schema version this codec does not support
    /// (F3 ratchet: refuse forward and backward).
    UnsupportedSchemaVersion {
        /// Version encoded in the unit value.
        found: u32,
        /// Version this codec accepts.
        expected: u32,
    },
    /// The unit value carries a wire-shape hash that differs from this
    /// codec's own derived hash: a serialized type changed without a
    /// `SCHEMA_VERSION` bump (fail-closed, mirrors the postcard gate).
    UnsupportedWireShape {
        /// Shape hash encoded in the unit value.
        found: u64,
        /// Shape hash of this codec's type definitions.
        expected: u64,
    },
    /// The decoded artifact failed the referential-integrity gate (U2.3).
    Invalid(HirArtifactError),
    /// The package value carries a package schema version this codec does
    /// not support (fail-closed, mirrors `FhirPackageError`).
    UnsupportedPackageSchemaVersion {
        /// Version encoded in the package value.
        found: u32,
        /// Version this codec accepts.
        expected: u32,
    },
    /// The package value failed a package-level gate: embedded-unit version
    /// manifest, unit validation, source-hash consistency, or the
    /// module/dependency/link-table checks shared with the postcard path.
    InvalidPackage(crate::package::FhirPackageError),
}

impl std::fmt::Display for MsgpackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(detail) => write!(f, "msgpack encode failed: {detail}"),
            Self::Truncated => write!(f, "truncated msgpack unit value: unexpected end of data"),
            Self::Decode(detail) => write!(f, "malformed msgpack unit value: {detail}"),
            Self::UnsupportedSchemaVersion { found, expected } => write!(
                f,
                "unsupported msgpack unit schema version {found} (expected {expected})"
            ),
            Self::UnsupportedWireShape { found, expected } => write!(
                f,
                "msgpack unit wire shape {found:#018x} differs from this build's {expected:#018x} \
                 (a serialized type changed without a schema version bump); rebuild the artifact"
            ),
            Self::Invalid(inner) => write!(f, "invalid msgpack artifact: {inner}"),
            Self::UnsupportedPackageSchemaVersion { found, expected } => write!(
                f,
                "unsupported msgpack package schema version {found} (expected {expected})"
            ),
            Self::InvalidPackage(inner) => write!(f, "invalid msgpack package: {inner}"),
        }
    }
}

impl std::error::Error for MsgpackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Invalid(inner) => Some(inner),
            Self::InvalidPackage(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<FhirPackageError> for MsgpackError {
    fn from(inner: FhirPackageError) -> Self {
        Self::InvalidPackage(inner)
    }
}

impl serde::ser::Error for MsgpackError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self::Encode(msg.to_string())
    }
}

impl serde::de::Error for MsgpackError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self::Decode(msg.to_string())
    }
}

// ---------------------------------------------------------------------------
// Owned `MessagePack` value tree
// ---------------------------------------------------------------------------

/// Owned, self-describing `MessagePack` value (decode intermediate).
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Nil,
    Bool(bool),
    Uint(u64),
    Int(i64),
    /// The project `i128` extension ([`I128_EXT_TYPE`]); never a plain `int`.
    Int128(i128),
    F32(f32),
    F64(f64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// Parses one complete `MessagePack` document; trailing bytes are rejected.
    fn from_bytes(bytes: &[u8]) -> Result<Self, MsgpackError> {
        let mut pos = 0;
        let value = read_value(bytes, &mut pos, 0)?;
        if pos != bytes.len() {
            return Err(MsgpackError::Decode(format!(
                "{} trailing byte(s) after msgpack value",
                bytes.len() - pos
            )));
        }
        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// Writer primitives (big-endian, minimal-width per the `MessagePack` spec)
// ---------------------------------------------------------------------------

fn write_uint(out: &mut Vec<u8>, n: u64) {
    // Minimal-width uint encoding; each marker carries exactly its own width.
    let bytes = n.to_be_bytes();
    if n <= 0x7f {
        out.push(bytes[7]);
    } else if u8::try_from(n).is_ok() {
        out.push(0xcc);
        out.push(bytes[7]);
    } else if u16::try_from(n).is_ok() {
        out.push(0xcd);
        out.extend_from_slice(&bytes[6..8]);
    } else if u32::try_from(n).is_ok() {
        out.push(0xce);
        out.extend_from_slice(&bytes[4..8]);
    } else {
        out.push(0xcf);
        out.extend_from_slice(&bytes);
    }
}

/// Signed opcodes only: the Rust signedness of a field is a wire fact (§1.6).
fn write_int(out: &mut Vec<u8>, n: i64) {
    if (-32..0).contains(&n) {
        // Negative fixint 0xe0..=0xff is the low byte of -32..=-1 itself.
        out.push(n.to_be_bytes()[7]);
    } else if i8::try_from(n).is_ok() {
        out.push(0xd0);
        out.extend_from_slice(&n.to_be_bytes()[7..8]);
    } else if i16::try_from(n).is_ok() {
        out.push(0xd1);
        out.extend_from_slice(&n.to_be_bytes()[6..8]);
    } else if i32::try_from(n).is_ok() {
        out.push(0xd2);
        out.extend_from_slice(&n.to_be_bytes()[4..8]);
    } else {
        out.push(0xd3);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

/// Signed 128-bit integer as the [`I128_EXT_TYPE`] `fixext 16` value.
fn write_i128(out: &mut Vec<u8>, n: i128) {
    out.push(0xd8);
    out.push(I128_EXT_TYPE);
    out.extend_from_slice(&n.to_be_bytes());
}

fn write_f32(out: &mut Vec<u8>, n: f32) {
    out.push(0xca);
    out.extend_from_slice(&n.to_be_bytes());
}

fn write_f64(out: &mut Vec<u8>, n: f64) {
    out.push(0xcb);
    out.extend_from_slice(&n.to_be_bytes());
}

/// Low byte of a length already known to fit one byte.
fn len_byte(len: u32) -> u8 {
    len.to_be_bytes()[3]
}

/// Low two bytes (big-endian) of a length already known to fit a `u16`.
fn len_u16_be(len: u32) -> [u8; 2] {
    let bytes = len.to_be_bytes();
    [bytes[2], bytes[3]]
}

fn write_str(out: &mut Vec<u8>, s: &str) -> Result<(), MsgpackError> {
    let len = u32::try_from(s.len())
        .map_err(|_| MsgpackError::Encode("string length exceeds msgpack u32".to_string()))?;
    if len <= 31 {
        out.push(0xa0 | len_byte(len));
    } else if len <= 0xff {
        out.push(0xd9);
        out.push(len_byte(len));
    } else if len <= 0xffff {
        out.push(0xda);
        out.extend_from_slice(&len_u16_be(len));
    } else {
        out.push(0xdb);
        out.extend_from_slice(&len.to_be_bytes());
    }
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

fn write_bin(out: &mut Vec<u8>, b: &[u8]) -> Result<(), MsgpackError> {
    let len = u32::try_from(b.len())
        .map_err(|_| MsgpackError::Encode("byte length exceeds msgpack u32".to_string()))?;
    if len <= 0xff {
        out.push(0xc4);
        out.push(len_byte(len));
    } else if len <= 0xffff {
        out.push(0xc5);
        out.extend_from_slice(&len_u16_be(len));
    } else {
        out.push(0xc6);
        out.extend_from_slice(&len.to_be_bytes());
    }
    out.extend_from_slice(b);
    Ok(())
}

fn write_array_len(out: &mut Vec<u8>, len: usize) -> Result<(), MsgpackError> {
    let len = u32::try_from(len)
        .map_err(|_| MsgpackError::Encode("array length exceeds msgpack u32".to_string()))?;
    if len <= 15 {
        out.push(0x90 | len_byte(len));
    } else if len <= 0xffff {
        out.push(0xdc);
        out.extend_from_slice(&len_u16_be(len));
    } else {
        out.push(0xdd);
        out.extend_from_slice(&len.to_be_bytes());
    }
    Ok(())
}

fn write_map_len(out: &mut Vec<u8>, len: usize) -> Result<(), MsgpackError> {
    let len = u32::try_from(len)
        .map_err(|_| MsgpackError::Encode("map length exceeds msgpack u32".to_string()))?;
    if len <= 15 {
        out.push(0x80 | len_byte(len));
    } else if len <= 0xffff {
        out.push(0xde);
        out.extend_from_slice(&len_u16_be(len));
    } else {
        out.push(0xdf);
        out.extend_from_slice(&len.to_be_bytes());
    }
    Ok(())
}

/// Serializes a [`Value`] tree back to bytes (test seam for wire-shape pins).
#[cfg(test)]
fn write_value(out: &mut Vec<u8>, value: &Value) -> Result<(), MsgpackError> {
    match value {
        Value::Nil => out.push(0xc0),
        Value::Bool(b) => out.push(if *b { 0xc3 } else { 0xc2 }),
        Value::Uint(n) => write_uint(out, *n),
        Value::Int(n) => write_int(out, *n),
        Value::Int128(n) => write_i128(out, *n),
        Value::F32(n) => write_f32(out, *n),
        Value::F64(n) => write_f64(out, *n),
        Value::Str(s) => write_str(out, s)?,
        Value::Bin(b) => write_bin(out, b)?,
        Value::Array(items) => {
            write_array_len(out, items.len())?;
            for item in items {
                write_value(out, item)?;
            }
        }
        Value::Map(entries) => {
            write_map_len(out, entries.len())?;
            for (key, val) in entries {
                write_value(out, key)?;
                write_value(out, val)?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reader primitives
// ---------------------------------------------------------------------------

fn read_u8(bytes: &[u8], pos: &mut usize) -> Result<u8, MsgpackError> {
    let b = *bytes.get(*pos).ok_or(MsgpackError::Truncated)?;
    *pos += 1;
    Ok(b)
}

fn read_exact<const N: usize>(bytes: &[u8], pos: &mut usize) -> Result<[u8; N], MsgpackError> {
    let end = pos
        .checked_add(N)
        .ok_or_else(|| MsgpackError::Decode("length overflow".to_string()))?;
    let slice = bytes.get(*pos..end).ok_or(MsgpackError::Truncated)?;
    let mut buf = [0u8; N];
    buf.copy_from_slice(slice);
    *pos = end;
    Ok(buf)
}

fn read_len(bytes: &[u8], pos: &mut usize, marker: u8) -> Result<usize, MsgpackError> {
    // Byte widths: bin8/uint8/str8 → 1; bin16/uint16/str16/array16/map16 → 2;
    // bin32/uint32/str32/array32/map32 → 4; uint64 → 8.
    let len = match marker {
        0xc4 | 0xcc | 0xd9 => u64::from(u8::from_be_bytes(read_exact::<1>(bytes, pos)?)),
        0xc5 | 0xcd | 0xda | 0xdc | 0xde => {
            u64::from(u16::from_be_bytes(read_exact::<2>(bytes, pos)?))
        }
        0xc6 | 0xce | 0xdb | 0xdd | 0xdf => {
            u64::from(u32::from_be_bytes(read_exact::<4>(bytes, pos)?))
        }
        0xcf => u64::from_be_bytes(read_exact::<8>(bytes, pos)?),
        _ => return Err(MsgpackError::Decode("not a length marker".to_string())),
    };
    usize::try_from(len).map_err(|_| MsgpackError::Decode("length exceeds usize".to_string()))
}

fn read_value(bytes: &[u8], pos: &mut usize, depth: usize) -> Result<Value, MsgpackError> {
    if depth > MAX_DEPTH {
        return Err(MsgpackError::Decode("nesting depth exceeded".to_string()));
    }
    let marker = read_u8(bytes, pos)?;
    match marker {
        0x00..=0x7f => Ok(Value::Uint(u64::from(marker))),
        0xe0..=0xff => Ok(Value::Int(i64::from(i8::from_ne_bytes([marker])))),
        0x80..=0x8f => read_map(bytes, pos, usize::from(marker & 0x0f), depth),
        0x90..=0x9f => read_array(bytes, pos, usize::from(marker & 0x0f), depth),
        0xa0..=0xbf => read_str(bytes, pos, usize::from(marker & 0x1f)),
        0xc0 => Ok(Value::Nil),
        0xc2 => Ok(Value::Bool(false)),
        0xc3 => Ok(Value::Bool(true)),
        0xc4..=0xc6 => {
            let len = read_len(bytes, pos, marker)?;
            Ok(Value::Bin(read_body(bytes, pos, len)?.to_vec()))
        }
        0xca => Ok(Value::F32(f32::from_be_bytes(read_exact::<4>(bytes, pos)?))),
        0xcb => Ok(Value::F64(f64::from_be_bytes(read_exact::<8>(bytes, pos)?))),
        0xcc..=0xcf => Ok(Value::Uint(read_len(bytes, pos, marker)? as u64)),
        0xd0 => Ok(Value::Int(i64::from(i8::from_be_bytes(read_exact::<1>(
            bytes, pos,
        )?)))),
        0xd1 => Ok(Value::Int(i64::from(i16::from_be_bytes(read_exact::<2>(
            bytes, pos,
        )?)))),
        0xd2 => Ok(Value::Int(i64::from(i32::from_be_bytes(read_exact::<4>(
            bytes, pos,
        )?)))),
        0xd3 => Ok(Value::Int(i64::from_be_bytes(read_exact::<8>(bytes, pos)?))),
        0xd8 => {
            let ext_type = read_u8(bytes, pos)?;
            if ext_type != I128_EXT_TYPE {
                return Err(MsgpackError::Decode(format!(
                    "unsupported msgpack extension type {ext_type}"
                )));
            }
            Ok(Value::Int128(i128::from_be_bytes(read_exact::<16>(
                bytes, pos,
            )?)))
        }
        0xd9..=0xdb => {
            let len = read_len(bytes, pos, marker)?;
            read_str(bytes, pos, len)
        }
        0xdc | 0xdd => {
            let len = read_len(bytes, pos, marker)?;
            read_array(bytes, pos, len, depth)
        }
        0xde | 0xdf => {
            let len = read_len(bytes, pos, marker)?;
            read_map(bytes, pos, len, depth)
        }
        _ => Err(MsgpackError::Decode(format!(
            "unsupported msgpack marker 0x{marker:02x}"
        ))),
    }
}

/// Borrows the next `len` bytes and advances `pos`; a short tail is a
/// truncation error.
fn read_body<'b>(bytes: &'b [u8], pos: &mut usize, len: usize) -> Result<&'b [u8], MsgpackError> {
    let body = bytes
        .get(*pos..pos.saturating_add(len))
        .ok_or(MsgpackError::Truncated)?;
    *pos += len;
    Ok(body)
}

fn read_str(bytes: &[u8], pos: &mut usize, len: usize) -> Result<Value, MsgpackError> {
    let body = read_body(bytes, pos, len)?;
    let s = std::str::from_utf8(body)
        .map_err(|_| MsgpackError::Decode("msgpack str is not UTF-8".to_string()))?;
    Ok(Value::Str(s.to_string()))
}

fn read_array(
    bytes: &[u8],
    pos: &mut usize,
    len: usize,
    depth: usize,
) -> Result<Value, MsgpackError> {
    let mut items = Vec::with_capacity(len.min(1024));
    for _ in 0..len {
        items.push(read_value(bytes, pos, depth + 1)?);
    }
    Ok(Value::Array(items))
}

fn read_map(
    bytes: &[u8],
    pos: &mut usize,
    len: usize,
    depth: usize,
) -> Result<Value, MsgpackError> {
    let mut entries = Vec::with_capacity(len.min(1024));
    for _ in 0..len {
        let key = read_value(bytes, pos, depth + 1)?;
        let val = read_value(bytes, pos, depth + 1)?;
        entries.push((key, val));
    }
    Ok(Value::Map(entries))
}

// ---------------------------------------------------------------------------
// Generic serde Serializer over `MessagePack` bytes
// ---------------------------------------------------------------------------

struct Serializer<'a> {
    out: &'a mut Vec<u8>,
}

impl Serializer<'_> {
    fn elem<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), MsgpackError> {
        value.serialize(Serializer {
            out: &mut *self.out,
        })
    }
}

/// Drives element/field serialization into the same output buffer for every
/// serde compound kind (seq, tuple, map, struct, struct variant).
struct Compound<'a> {
    out: &'a mut Vec<u8>,
}

impl Compound<'_> {
    fn elem<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), MsgpackError> {
        value.serialize(Serializer {
            out: &mut *self.out,
        })
    }
}

impl<'a> serde::Serializer for Serializer<'a> {
    type Ok = ();
    type Error = MsgpackError;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    fn serialize_bool(self, v: bool) -> Result<(), MsgpackError> {
        (*self.out).push(if v { 0xc3 } else { 0xc2 });
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<(), MsgpackError> {
        write_int(&mut *self.out, i64::from(v));
        Ok(())
    }
    fn serialize_i16(self, v: i16) -> Result<(), MsgpackError> {
        write_int(&mut *self.out, i64::from(v));
        Ok(())
    }
    fn serialize_i32(self, v: i32) -> Result<(), MsgpackError> {
        write_int(&mut *self.out, i64::from(v));
        Ok(())
    }
    fn serialize_i64(self, v: i64) -> Result<(), MsgpackError> {
        write_int(&mut *self.out, v);
        Ok(())
    }
    fn serialize_i128(self, v: i128) -> Result<(), MsgpackError> {
        write_i128(&mut *self.out, v);
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<(), MsgpackError> {
        write_uint(&mut *self.out, u64::from(v));
        Ok(())
    }
    fn serialize_u16(self, v: u16) -> Result<(), MsgpackError> {
        write_uint(&mut *self.out, u64::from(v));
        Ok(())
    }
    fn serialize_u32(self, v: u32) -> Result<(), MsgpackError> {
        write_uint(&mut *self.out, u64::from(v));
        Ok(())
    }
    fn serialize_u64(self, v: u64) -> Result<(), MsgpackError> {
        write_uint(&mut *self.out, v);
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<(), MsgpackError> {
        write_f32(&mut *self.out, v);
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> Result<(), MsgpackError> {
        write_f64(&mut *self.out, v);
        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<(), MsgpackError> {
        let mut buf = [0u8; 4];
        write_str(&mut *self.out, v.encode_utf8(&mut buf))
    }

    fn serialize_str(self, v: &str) -> Result<(), MsgpackError> {
        write_str(&mut *self.out, v)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<(), MsgpackError> {
        write_bin(&mut *self.out, v)
    }

    fn serialize_none(self) -> Result<(), MsgpackError> {
        (*self.out).push(0xc0);
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(mut self, value: &T) -> Result<(), MsgpackError> {
        self.elem(value)
    }

    fn serialize_unit(self) -> Result<(), MsgpackError> {
        (*self.out).push(0xc0);
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), MsgpackError> {
        (*self.out).push(0xc0);
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant_name: &'static str,
    ) -> Result<(), MsgpackError> {
        write_map_len(&mut *self.out, 1)?;
        write_str(&mut *self.out, "tag")?;
        write_str(&mut *self.out, variant_name)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        mut self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), MsgpackError> {
        self.elem(value)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        mut self,
        _name: &'static str,
        _index: u32,
        variant_name: &'static str,
        value: &T,
    ) -> Result<(), MsgpackError> {
        write_map_len(&mut *self.out, 2)?;
        write_str(&mut *self.out, "tag")?;
        write_str(&mut *self.out, variant_name)?;
        write_str(&mut *self.out, "value")?;
        self.elem(value)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Compound<'a>, MsgpackError> {
        write_array_len(
            &mut *self.out,
            len.ok_or_else(|| {
                MsgpackError::Encode("msgpack requires a sequence length".to_string())
            })?,
        )?;
        Ok(Compound { out: self.out })
    }

    fn serialize_tuple(self, len: usize) -> Result<Compound<'a>, MsgpackError> {
        write_array_len(&mut *self.out, len)?;
        Ok(Compound { out: self.out })
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Compound<'a>, MsgpackError> {
        write_array_len(&mut *self.out, len)?;
        Ok(Compound { out: self.out })
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant_name: &'static str,
        len: usize,
    ) -> Result<Compound<'a>, MsgpackError> {
        write_map_len(&mut *self.out, 2)?;
        write_str(&mut *self.out, "tag")?;
        write_str(&mut *self.out, variant_name)?;
        write_str(&mut *self.out, "value")?;
        write_array_len(&mut *self.out, len)?;
        Ok(Compound { out: self.out })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Compound<'a>, MsgpackError> {
        write_map_len(
            &mut *self.out,
            len.ok_or_else(|| MsgpackError::Encode("msgpack requires a map length".to_string()))?,
        )?;
        Ok(Compound { out: self.out })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Compound<'a>, MsgpackError> {
        write_map_len(&mut *self.out, len)?;
        Ok(Compound { out: self.out })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant_name: &'static str,
        len: usize,
    ) -> Result<Compound<'a>, MsgpackError> {
        // "tag" plus the inlined struct-variant payload fields (§1.4).
        write_map_len(
            &mut *self.out,
            len.checked_add(1).ok_or_else(|| {
                MsgpackError::Encode("struct variant field count overflow".to_string())
            })?,
        )?;
        write_str(&mut *self.out, "tag")?;
        write_str(&mut *self.out, variant_name)?;
        Ok(Compound { out: self.out })
    }
}

impl SerializeSeq for Compound<'_> {
    type Ok = ();
    type Error = MsgpackError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), MsgpackError> {
        self.elem(value)
    }
    fn end(self) -> Result<(), MsgpackError> {
        Ok(())
    }
}

impl SerializeTuple for Compound<'_> {
    type Ok = ();
    type Error = MsgpackError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), MsgpackError> {
        self.elem(value)
    }
    fn end(self) -> Result<(), MsgpackError> {
        Ok(())
    }
}

impl SerializeTupleStruct for Compound<'_> {
    type Ok = ();
    type Error = MsgpackError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), MsgpackError> {
        self.elem(value)
    }
    fn end(self) -> Result<(), MsgpackError> {
        Ok(())
    }
}

impl SerializeTupleVariant for Compound<'_> {
    type Ok = ();
    type Error = MsgpackError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), MsgpackError> {
        self.elem(value)
    }
    fn end(self) -> Result<(), MsgpackError> {
        Ok(())
    }
}

impl SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = MsgpackError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), MsgpackError> {
        self.elem(key)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), MsgpackError> {
        self.elem(value)
    }
    fn end(self) -> Result<(), MsgpackError> {
        Ok(())
    }
}

impl SerializeStruct for Compound<'_> {
    type Ok = ();
    type Error = MsgpackError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), MsgpackError> {
        write_str(&mut *self.out, key)?;
        self.elem(value)
    }
    fn end(self) -> Result<(), MsgpackError> {
        Ok(())
    }
}

impl SerializeStructVariant for Compound<'_> {
    type Ok = ();
    type Error = MsgpackError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), MsgpackError> {
        write_str(&mut *self.out, key)?;
        self.elem(value)
    }
    fn end(self) -> Result<(), MsgpackError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Generic serde Deserializer over the Value tree
// ---------------------------------------------------------------------------

impl Value {
    fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Uint(n) => i64::try_from(*n).ok(),
            _ => None,
        }
    }

    fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Uint(n) => Some(*n),
            Value::Int(n) => u64::try_from(*n).ok(),
            _ => None,
        }
    }

    fn expect_map(self, expected: &str) -> Result<Vec<(Value, Value)>, MsgpackError> {
        match self {
            Value::Map(entries) => Ok(entries),
            other => Err(MsgpackError::Decode(format!(
                "expected a map for {expected}, found {other:?}"
            ))),
        }
    }
}

struct SeqDeserializer {
    iter: std::vec::IntoIter<Value>,
}

impl<'de> SeqAccess<'de> for SeqDeserializer {
    type Error = MsgpackError;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, MsgpackError> {
        match self.iter.next() {
            Some(value) => Ok(Some(seed.deserialize(value)?)),
            None => Ok(None),
        }
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct MapDeserializer {
    iter: std::vec::IntoIter<(Value, Value)>,
    pending_value: Option<Value>,
}

impl<'de> MapAccess<'de> for MapDeserializer {
    type Error = MsgpackError;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, MsgpackError> {
        match self.iter.next() {
            Some((key, value)) => {
                self.pending_value = Some(value);
                Ok(Some(seed.deserialize(key)?))
            }
            None => Ok(None),
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, MsgpackError> {
        let value = self
            .pending_value
            .take()
            .ok_or_else(|| MsgpackError::Decode("map value requested before key".to_string()))?;
        seed.deserialize(value)
    }
}

struct EnumDeserializer {
    tag: String,
    entries: Vec<(Value, Value)>,
}

impl<'de> EnumAccess<'de> for EnumDeserializer {
    type Error = MsgpackError;
    type Variant = VariantDeserializer;
    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, VariantDeserializer), MsgpackError> {
        let variant = seed.deserialize(Value::Str(self.tag))?;
        Ok((
            variant,
            VariantDeserializer {
                entries: self.entries,
            },
        ))
    }
}

struct VariantDeserializer {
    entries: Vec<(Value, Value)>,
}

impl VariantDeserializer {
    fn take_value(self) -> Result<Value, MsgpackError> {
        self.entries
            .into_iter()
            .find(|(key, _)| matches!(key, Value::Str(k) if k == "value"))
            .map(|(_, value)| value)
            .ok_or_else(|| {
                MsgpackError::Decode(
                    "enum variant payload must ride under the \"value\" key".to_string(),
                )
            })
    }
}

impl<'de> VariantAccess<'de> for VariantDeserializer {
    type Error = MsgpackError;
    fn unit_variant(self) -> Result<(), MsgpackError> {
        if self.entries.is_empty() {
            Ok(())
        } else {
            Err(MsgpackError::Decode(
                "unexpected payload fields on a unit variant".to_string(),
            ))
        }
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, MsgpackError> {
        seed.deserialize(self.take_value()?)
    }
    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        self.take_value()?.deserialize_tuple(len, visitor)
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        visitor.visit_map(MapDeserializer {
            iter: self.entries.into_iter(),
            pending_value: None,
        })
    }
}

impl<'de> Deserializer<'de> for Value {
    type Error = MsgpackError;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Nil => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(b),
            Value::Uint(n) => visitor.visit_u64(n),
            Value::Int(n) => visitor.visit_i64(n),
            Value::Int128(n) => visitor.visit_i128(n),
            Value::F32(n) => visitor.visit_f32(n),
            Value::F64(n) => visitor.visit_f64(n),
            Value::Str(s) => visitor.visit_string(s),
            Value::Bin(b) => visitor.visit_byte_buf(b),
            Value::Array(items) => visitor.visit_seq(SeqDeserializer {
                iter: items.into_iter(),
            }),
            Value::Map(entries) => visitor.visit_map(MapDeserializer {
                iter: entries.into_iter(),
                pending_value: None,
            }),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Bool(b) => visitor.visit_bool(b),
            other => Err(MsgpackError::Decode(format!(
                "expected a bool, found {other:?}"
            ))),
        }
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self.as_i64() {
            Some(n) => visitor.visit_i64(n),
            None => Err(MsgpackError::Decode(format!(
                "expected an int, found {self:?}"
            ))),
        }
    }

    fn deserialize_i128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Int128(n) => visitor.visit_i128(n),
            other => Err(MsgpackError::Decode(format!(
                "expected an i128 extension value, found {other:?}"
            ))),
        }
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self.as_u64() {
            Some(n) => visitor.visit_u64(n),
            None => Err(MsgpackError::Decode(format!(
                "expected a uint, found {self:?}"
            ))),
        }
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::F32(n) => visitor.visit_f32(n),
            Value::F64(n) => visitor.visit_f64(n),
            other => Err(MsgpackError::Decode(format!(
                "expected a float, found {other:?}"
            ))),
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::F32(n) => visitor.visit_f32(n),
            Value::F64(n) => visitor.visit_f64(n),
            other => Err(MsgpackError::Decode(format!(
                "expected a float, found {other:?}"
            ))),
        }
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Str(s) => {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => visitor.visit_char(c),
                    _ => Err(MsgpackError::Decode(format!(
                        "expected a single-char str, found {s:?}"
                    ))),
                }
            }
            other => Err(MsgpackError::Decode(format!(
                "expected a char, found {other:?}"
            ))),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Str(s) => visitor.visit_string(s),
            other => Err(MsgpackError::Decode(format!(
                "expected a str, found {other:?}"
            ))),
        }
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_byte_buf(visitor)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Bin(b) => visitor.visit_byte_buf(b),
            other => Err(MsgpackError::Decode(format!(
                "expected bin bytes, found {other:?}"
            ))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Nil => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Nil => visitor.visit_unit(),
            other => Err(MsgpackError::Decode(format!(
                "expected nil, found {other:?}"
            ))),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        match self {
            Value::Array(items) => visitor.visit_seq(SeqDeserializer {
                iter: items.into_iter(),
            }),
            other => Err(MsgpackError::Decode(format!(
                "expected an array, found {other:?}"
            ))),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        match &self {
            Value::Array(items) if items.len() == len => self.deserialize_seq(visitor),
            _ => Err(MsgpackError::Decode(format!(
                "expected a tuple of length {len}"
            ))),
        }
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        let entries = self.expect_map("a map value")?;
        visitor.visit_map(MapDeserializer {
            iter: entries.into_iter(),
            pending_value: None,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        let entries = self.expect_map("a struct value")?;
        visitor.visit_map(MapDeserializer {
            iter: entries.into_iter(),
            pending_value: None,
        })
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        let mut entries = self.expect_map("an enum value")?;
        let tag = entries
            .iter()
            .position(|(key, _)| matches!(key, Value::Str(k) if k == "tag"))
            .map(|at| entries.remove(at).1)
            .and_then(|value| match value {
                Value::Str(s) => Some(s),
                _ => None,
            })
            .ok_or_else(|| {
                MsgpackError::Decode(
                    "enum value must carry its variant name under the \"tag\" key".to_string(),
                )
            })?;
        visitor.visit_enum(EnumDeserializer { tag, entries })
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, MsgpackError> {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, MsgpackError> {
        visitor.visit_unit()
    }
}

// ---------------------------------------------------------------------------
// Unit artifact DTO (spec §2 rows 10/13 tuple-family conversions)
// ---------------------------------------------------------------------------

/// Spec §2 row 13: one `gpu_builtins` entry as a named map on the wire.
#[derive(Serialize, Deserialize)]
struct GpuBuiltinEntryDto {
    def_id: DefId,
    builtin: GpuBuiltinWire,
}

/// Spec §2 row 10: one `function_facts.entries` element as a named map.
#[derive(Serialize, Deserialize)]
struct FunctionFactEntryDto {
    def_id: DefId,
    facts: FunctionFactWire,
}

/// Spec §2 row 10: the function-fact table with tuple entries widened to
/// named maps.
#[derive(Serialize, Deserialize)]
struct FunctionFactTableDto {
    entries: Vec<FunctionFactEntryDto>,
    entry: Option<FunctionFactWire>,
}

/// Unit-value wire shape: the unit's named keys in declaration order
/// (`schema_version` first, §1.2), with the two tuple families converted.
#[derive(Serialize, Deserialize)]
struct HirArtifactDto {
    schema_version: u32,
    shape_hash: u64,
    source_identity: SourceIdentity,
    hir: HirModule,
    presentation: HirPresentation,
    cli_program: Option<CliProgramWire>,
    types: TypeTableSnapshot,
    resolved_generic_calls: Vec<ResolvedGenericCall>,
    radix_lanes: Vec<RadixLaneEntryWire>,
    interner: Vec<String>,
    resolver: ResolverSnapshot,
    libraries: LibraryRegistry,
    function_facts: FunctionFactTableDto,
    resolved_uses: Vec<ResolvedUseWire>,
    analysis_stamp: u64,
    gpu_builtins: Vec<GpuBuiltinEntryDto>,
    capability_provenance: FunctionProvenanceTableWire,
    provider_units: BTreeMap<String, ProviderCarriageEntryWire>,
    shape_obligations: ShapeObligationTableWire,
    contract_roles: ContractRolesWire,
    kernel_requirements: KernelRequirementsWire,
    tensor_edge_policies: TensorEdgePolicyTableWire,
    display_tokens: DisplayTokensWire,
    conversation_constructor: Option<DefId>,
}

impl From<HirArtifact> for HirArtifactDto {
    /// Consuming form: the package encode path has just decoded the unit
    /// and needs the DTO once, so every field moves instead of cloning the
    /// largest object in the compiler.
    fn from(artifact: HirArtifact) -> Self {
        Self {
            schema_version: artifact.schema_version,
            shape_hash: artifact.shape_hash,
            source_identity: artifact.source_identity,
            hir: artifact.hir,
            presentation: artifact.presentation,
            cli_program: artifact.cli_program,
            types: artifact.types,
            resolved_generic_calls: artifact.resolved_generic_calls,
            radix_lanes: artifact.radix_lanes,
            interner: artifact.interner,
            resolver: artifact.resolver,
            libraries: artifact.libraries,
            function_facts: FunctionFactTableDto {
                entries: artifact
                    .function_facts
                    .entries
                    .into_iter()
                    .map(|(def_id, facts)| FunctionFactEntryDto { def_id, facts })
                    .collect(),
                entry: artifact.function_facts.entry,
            },
            resolved_uses: artifact.resolved_uses,
            analysis_stamp: artifact.analysis_stamp,
            gpu_builtins: artifact
                .gpu_builtins
                .into_iter()
                .map(|(def_id, builtin)| GpuBuiltinEntryDto { def_id, builtin })
                .collect(),
            capability_provenance: artifact.capability_provenance,
            provider_units: artifact.provider_units,
            shape_obligations: artifact.shape_obligations,
            contract_roles: artifact.contract_roles,
            kernel_requirements: artifact.kernel_requirements,
            tensor_edge_policies: artifact.tensor_edge_policies,
            display_tokens: artifact.display_tokens,
            conversation_constructor: artifact.conversation_constructor,
        }
    }
}

impl From<&HirArtifact> for HirArtifactDto {
    fn from(artifact: &HirArtifact) -> Self {
        Self {
            schema_version: artifact.schema_version,
            shape_hash: artifact.shape_hash,
            source_identity: artifact.source_identity.clone(),
            hir: artifact.hir.clone(),
            presentation: artifact.presentation.clone(),
            cli_program: artifact.cli_program.clone(),
            types: artifact.types.clone(),
            resolved_generic_calls: artifact.resolved_generic_calls.clone(),
            radix_lanes: artifact.radix_lanes.clone(),
            interner: artifact.interner.clone(),
            resolver: artifact.resolver.clone(),
            libraries: artifact.libraries.clone(),
            function_facts: FunctionFactTableDto {
                entries: artifact
                    .function_facts
                    .entries
                    .iter()
                    .map(|(def_id, facts)| FunctionFactEntryDto {
                        def_id: *def_id,
                        facts: facts.clone(),
                    })
                    .collect(),
                entry: artifact.function_facts.entry.clone(),
            },
            resolved_uses: artifact.resolved_uses.clone(),
            analysis_stamp: artifact.analysis_stamp,
            gpu_builtins: artifact
                .gpu_builtins
                .iter()
                .map(|(def_id, builtin)| GpuBuiltinEntryDto {
                    def_id: *def_id,
                    builtin: *builtin,
                })
                .collect(),
            capability_provenance: artifact.capability_provenance.clone(),
            provider_units: artifact.provider_units.clone(),
            shape_obligations: artifact.shape_obligations.clone(),
            contract_roles: artifact.contract_roles.clone(),
            kernel_requirements: artifact.kernel_requirements.clone(),
            tensor_edge_policies: artifact.tensor_edge_policies.clone(),
            display_tokens: artifact.display_tokens.clone(),
            conversation_constructor: artifact.conversation_constructor,
        }
    }
}

impl From<HirArtifactDto> for HirArtifact {
    fn from(dto: HirArtifactDto) -> Self {
        Self {
            schema_version: dto.schema_version,
            shape_hash: dto.shape_hash,
            source_identity: dto.source_identity,
            hir: dto.hir,
            presentation: dto.presentation,
            cli_program: dto.cli_program,
            types: dto.types,
            resolved_generic_calls: dto.resolved_generic_calls,
            radix_lanes: dto.radix_lanes,
            interner: dto.interner,
            resolver: dto.resolver,
            libraries: dto.libraries,
            function_facts: FunctionFactTableWire {
                entries: dto
                    .function_facts
                    .entries
                    .into_iter()
                    .map(|entry| (entry.def_id, entry.facts))
                    .collect(),
                entry: dto.function_facts.entry,
            },
            resolved_uses: dto.resolved_uses,
            analysis_stamp: dto.analysis_stamp,
            gpu_builtins: dto
                .gpu_builtins
                .into_iter()
                .map(|entry| (entry.def_id, entry.builtin))
                .collect(),
            capability_provenance: dto.capability_provenance,
            provider_units: dto.provider_units,
            shape_obligations: dto.shape_obligations,
            contract_roles: dto.contract_roles,
            kernel_requirements: dto.kernel_requirements,
            tensor_edge_policies: dto.tensor_edge_policies,
            display_tokens: dto.display_tokens,
            conversation_constructor: dto.conversation_constructor,
        }
    }
}

// ---------------------------------------------------------------------------
// Package DTO (FPF-3): envelope with nested named unit values (OQ-3)
// ---------------------------------------------------------------------------

/// One module record on the package wire. Identical to
/// [`FhirPackageModule`] except `unit`, which is the embedded unit's
/// **nested named value** (a [`HirArtifactDto`] map, spec §1.1), not opaque
/// nested bytes — the whole package is one `MessagePack` document (OQ-3
/// ruling, confirmed at FPF-3 dispatch).
#[derive(Serialize, Deserialize)]
struct FhirPackageModuleDto {
    path: String,
    module_segments: Vec<String>,
    is_entry: bool,
    export_names: Vec<String>,
    local_links: Vec<LocalLinkWire>,
    library_imports: Vec<LibraryImportWire>,
    source_hash: String,
    unit_schema_version: u32,
    unit: HirArtifactDto,
}

/// Package wire shape: the envelope's named keys in declaration order
/// (`package_schema_version` first, the version prefix gate), modules
/// carrying nested named unit values.
#[derive(Serialize, Deserialize)]
struct FhirPackageDto {
    package_schema_version: u32,
    identity: PackageIdentityWire,
    entry_path: String,
    entry_frontmatter: Option<String>,
    modules: Vec<FhirPackageModuleDto>,
    dependencies: Vec<PackageDependencyWire>,
}

impl FhirPackageDto {
    /// Decodes each module's postcard unit bytes and embeds it as a nested
    /// named value. Fail-closed: a module whose unit bytes do not decode
    /// through the unit codec's own gates cannot be embedded.
    fn from_package(pkg: &FhirPackage) -> Result<Self, MsgpackError> {
        let mut modules = Vec::with_capacity(pkg.modules.len());
        for module in &pkg.modules {
            let artifact = decode_unit_bytes(&module.unit).map_err(|error| {
                MsgpackError::Encode(format!(
                    "module `{}` unit failed to decode for embedding: {error}",
                    module.path
                ))
            })?;
            modules.push(FhirPackageModuleDto {
                path: module.path.clone(),
                module_segments: module.module_segments.clone(),
                is_entry: module.is_entry,
                export_names: module.export_names.clone(),
                local_links: module.local_links.clone(),
                library_imports: module.library_imports.clone(),
                source_hash: module.source_hash.clone(),
                unit_schema_version: module.unit_schema_version,
                unit: HirArtifactDto::from(artifact),
            });
        }
        Ok(Self {
            package_schema_version: pkg.package_schema_version,
            identity: pkg.identity.clone(),
            entry_path: pkg.entry_path.clone(),
            entry_frontmatter: pkg.entry_frontmatter.clone(),
            modules,
            dependencies: pkg.dependencies.clone(),
        })
    }

    /// Rehydrates the wire [`FhirPackage`]: per-unit gates first (referential
    /// integrity, source-hash consistency, postcard re-encode of the unit
    /// bytes), then the package-level checks shared with the postcard path.
    fn into_package(self) -> Result<FhirPackage, MsgpackError> {
        use crate::package::DecodedFhirPackageModule;

        let mut modules = Vec::with_capacity(self.modules.len());
        let mut decoded = Vec::with_capacity(self.modules.len());
        for module in self.modules {
            let artifact = HirArtifact::from(module.unit);
            let type_count = artifact.types.type_count();
            let interner_len = artifact.interner.len();
            validate_referential_integrity(&artifact, type_count, interner_len).map_err(
                |error| {
                    MsgpackError::InvalidPackage(FhirPackageError::MalformedUnit {
                        module: module.path.clone(),
                        detail: error.to_string(),
                    })
                },
            )?;
            if artifact.source_identity.content_hash != module.source_hash {
                return Err(MsgpackError::InvalidPackage(
                    FhirPackageError::MismatchedContentHash {
                        module: module.path.clone(),
                        expected: module.source_hash.clone(),
                        actual: artifact.source_identity.content_hash.clone(),
                    },
                ));
            }
            // The unit stays postcard bytes in the in-memory wire form (the
            // unit artifact's own codec); the nested named value is
            // re-encoded deterministically.
            let unit = encode_unit_bytes(&artifact).map_err(|error| {
                MsgpackError::Encode(format!(
                    "module `{}` unit failed to re-encode: {error}",
                    module.path
                ))
            })?;
            modules.push(FhirPackageModule {
                path: module.path.clone(),
                module_segments: module.module_segments.clone(),
                is_entry: module.is_entry,
                export_names: module.export_names.clone(),
                local_links: module.local_links.clone(),
                library_imports: module.library_imports.clone(),
                source_hash: module.source_hash.clone(),
                unit_schema_version: module.unit_schema_version,
                unit,
            });
            decoded.push(DecodedFhirPackageModule {
                path: module.path,
                module_segments: module.module_segments,
                is_entry: module.is_entry,
                export_names: module.export_names,
                local_links: module.local_links,
                library_imports: module.library_imports,
                unit: artifact,
            });
        }
        let pkg = FhirPackage {
            package_schema_version: self.package_schema_version,
            identity: self.identity,
            entry_path: self.entry_path,
            entry_frontmatter: self.entry_frontmatter,
            modules,
            dependencies: self.dependencies,
        };
        validate_module_table(&pkg, &decoded)?;
        validate_dependencies(&pkg)?;
        validate_local_links(&pkg)?;
        Ok(pkg)
    }
}

/// Reads a named field from a map value.
fn named_field<'a>(entries: &'a [(Value, Value)], key: &str) -> Result<&'a Value, MsgpackError> {
    entries
        .iter()
        .find(|(k, _)| matches!(k, Value::Str(s) if s == key))
        .map(|(_, v)| v)
        .ok_or_else(|| MsgpackError::Decode(format!("value must carry {key} as a named field")))
}

/// Version gates read from the parsed package value **before** payload
/// interpretation (§1.2, fail-closed):
///
/// 1. `package_schema_version` must equal [`PACKAGE_SCHEMA_VERSION`];
/// 2. every module's `unit_schema_version` manifest copy and its nested
///    unit's own `schema_version` must equal [`SCHEMA_VERSION`], with
///    module attribution.
fn gate_package_versions(value: &Value) -> Result<(), MsgpackError> {
    let Value::Map(entries) = value else {
        return Err(MsgpackError::Decode(
            "the package value must be a map of named fields".to_string(),
        ));
    };
    let found = named_field(entries, "package_schema_version")?
        .as_u64()
        .ok_or_else(|| MsgpackError::Decode("package_schema_version must be a uint".to_string()))?;
    if found != u64::from(PACKAGE_SCHEMA_VERSION) {
        return Err(MsgpackError::UnsupportedPackageSchemaVersion {
            found: u32::try_from(found).unwrap_or(u32::MAX),
            expected: PACKAGE_SCHEMA_VERSION,
        });
    }

    let modules = match named_field(entries, "modules")? {
        Value::Array(modules) => modules,
        other => {
            return Err(MsgpackError::Decode(format!(
                "modules must be an array, found {other:?}"
            )));
        }
    };
    for module in modules {
        gate_module_versions(module)?;
    }
    Ok(())
}

/// Gates one module record: the `unit_schema_version` manifest copy and the
/// nested unit's own `schema_version`, both read before payload
/// interpretation, with module attribution.
fn gate_module_versions(module: &Value) -> Result<(), MsgpackError> {
    let module_entries = match module {
        Value::Map(entries) => entries,
        other => {
            return Err(MsgpackError::Decode(format!(
                "a module record must be a map, found {other:?}"
            )));
        }
    };
    let path = match named_field(module_entries, "path")? {
        Value::Str(path) => path.clone(),
        other => {
            return Err(MsgpackError::Decode(format!(
                "module path must be a str, found {other:?}"
            )));
        }
    };
    let manifest = named_field(module_entries, "unit_schema_version")?
        .as_u64()
        .ok_or_else(|| MsgpackError::Decode("unit_schema_version must be a uint".to_string()))?;
    if manifest != u64::from(SCHEMA_VERSION) {
        return Err(MsgpackError::InvalidPackage(
            FhirPackageError::UnsupportedUnitSchemaVersion {
                module: path,
                found: u32::try_from(manifest).unwrap_or(u32::MAX),
                expected: SCHEMA_VERSION,
            },
        ));
    }
    let unit_entries = match named_field(module_entries, "unit")? {
        Value::Map(entries) => entries,
        other => {
            return Err(MsgpackError::Decode(format!(
                "the embedded unit must be a nested named map, found {other:?}"
            )));
        }
    };
    let unit_version = named_field(unit_entries, "schema_version")?
        .as_u64()
        .ok_or_else(|| MsgpackError::Decode("unit schema_version must be a uint".to_string()))?;
    if unit_version != u64::from(SCHEMA_VERSION) {
        return Err(MsgpackError::InvalidPackage(
            FhirPackageError::UnsupportedUnitSchemaVersion {
                module: path,
                found: u32::try_from(unit_version).unwrap_or(u32::MAX),
                expected: SCHEMA_VERSION,
            },
        ));
    }
    // The derived wire-shape hash rides beside the version; a unit written by
    // a build whose serialized types differ fails closed with attribution.
    let unit_shape = named_field(unit_entries, "shape_hash")?
        .as_u64()
        .ok_or_else(|| MsgpackError::Decode("unit shape_hash must be a uint".to_string()))?;
    if unit_shape != wire_shape_hash() {
        return Err(MsgpackError::InvalidPackage(
            FhirPackageError::UnsupportedUnitWireShape {
                module: path,
                found: unit_shape,
                expected: wire_shape_hash(),
            },
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Legacy postcard rejection (FPF-5)
// ---------------------------------------------------------------------------

/// Detects the retired postcard envelope prefix (OQ-1 version ratchet).
///
/// A schema-v2+ package value is a top-level `MessagePack` **map**; the retired
/// postcard envelope begins with its `u32 package_schema_version` varint — a
/// bare uint followed by the rest of the document. A varint prefix can never
/// be a v2+ map value, so this shape is unambiguously legacy (no magic bytes
/// needed). Returns the legacy version number when the payload has that
/// shape.
fn legacy_postcard_version(bytes: &[u8]) -> Option<u32> {
    // The first marker byte decides: a v2+ package is a top-level map
    // (`0x80..=0x8f`, `0xde`, `0xdf`), a legacy envelope is a bare uint
    // (the version varint) followed by the rest of the document, and
    // everything else is neither. Walking the document to learn this costs
    // a full parse plus a discarded DOM per load.
    let (&marker, rest) = bytes.split_first()?;
    let (width, value) = match marker {
        0x00..=0x7f => (0usize, u64::from(marker)),
        0xcc => (1, u64::from(*rest.first()?)),
        0xcd => (
            2,
            u64::from(u16::from_be_bytes(rest.get(..2)?.try_into().ok()?)),
        ),
        0xce => (
            4,
            u64::from(u32::from_be_bytes(rest.get(..4)?.try_into().ok()?)),
        ),
        0xcf => (8, u64::from_be_bytes(rest.get(..8)?.try_into().ok()?)),
        _ => return None,
    };
    // A legacy envelope is never a single complete uint — the version
    // varint is only its prefix; a bare uint takes the ordinary
    // malformed path instead.
    if rest.len() > width {
        u32::try_from(value).ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Public codec entry points
// ---------------------------------------------------------------------------

/// Serializes a [`HirArtifact`] to a spec-conformant named-field `MessagePack`
/// unit value (FPF-2). Postcard remains the live wire; this codec is additive.
///
/// # Errors
///
/// Returns [`MsgpackError::Encode`] if serialization fails (unsupported data
/// shape or a length exceeding `MessagePack`'s `u32` range); a valid artifact
/// is expected to succeed.
//
// FPF-5 owns the unit-codec switch; the unit codec is consumed by the test
// suite only until then (the package codec is the reachable entry point).
#[allow(dead_code)]
pub fn encode_msgpack(artifact: &HirArtifact) -> Result<Vec<u8>, MsgpackError> {
    let dto = HirArtifactDto::from(artifact);
    let mut out = Vec::new();
    dto.serialize(Serializer { out: &mut out })?;
    Ok(out)
}

/// Deserializes and validates a [`HirArtifact`] from a `MessagePack` unit
/// value, mirroring the postcard [`crate::decode::decode`] gates.
///
/// # Version gate (§1.2, F3)
///
/// The named `schema_version` field is read from the parsed value before the
/// payload is deserialized; any mismatch fails closed with
/// [`MsgpackError::UnsupportedSchemaVersion`]. There is no compatibility
/// decoder.
///
/// # Fail-closed (F7)
///
/// Malformed or truncated `MessagePack` input returns a structured
/// [`MsgpackError`]; the referential-integrity gate (U2.3) then validates
/// every arena index before the artifact is returned.
///
/// # Errors
///
/// Returns [`MsgpackError::Truncated`] on short input,
/// [`MsgpackError::Decode`] on malformed or non-conformant values,
/// [`MsgpackError::UnsupportedSchemaVersion`] on a version mismatch, and
/// [`MsgpackError::Invalid`] when the referential-integrity gate rejects the
/// decoded artifact.
//
// FPF-5 owns the unit-codec switch; the unit codec is consumed by the test
// suite only until then (the package codec is the reachable entry point).
#[allow(dead_code)]
pub fn decode_msgpack(bytes: &[u8]) -> Result<HirArtifact, MsgpackError> {
    let value = Value::from_bytes(bytes)?;

    // §1.2: the version is a named field read first; it gates the payload.
    let Value::Map(version_entries) = &value else {
        return Err(MsgpackError::Decode(
            "the unit value must be a map of named fields".to_string(),
        ));
    };
    let found = version_entries
        .iter()
        .find(|(key, _)| matches!(key, Value::Str(k) if k == "schema_version"))
        .and_then(|(_, v)| v.as_u64())
        .ok_or_else(|| {
            MsgpackError::Decode(
                "unit value must carry schema_version as a named uint field".to_string(),
            )
        })?;
    if found != u64::from(SCHEMA_VERSION) {
        return Err(MsgpackError::UnsupportedSchemaVersion {
            found: u32::try_from(found).unwrap_or(u32::MAX),
            expected: SCHEMA_VERSION,
        });
    }

    // The derived wire-shape hash is the second named header field.
    let found_shape = version_entries
        .iter()
        .find(|(key, _)| matches!(key, Value::Str(k) if k == "shape_hash"))
        .and_then(|(_, v)| v.as_u64())
        .ok_or_else(|| {
            MsgpackError::Decode(
                "unit value must carry shape_hash as a named uint field".to_string(),
            )
        })?;
    if found_shape != wire_shape_hash() {
        return Err(MsgpackError::UnsupportedWireShape {
            found: found_shape,
            expected: wire_shape_hash(),
        });
    }

    // Payload interpretation begins only after the version gate accepts.
    let dto = HirArtifactDto::deserialize(value)?;

    // U2.3: referential-integrity gate, mirroring the postcard decode path.
    let artifact = HirArtifact::from(dto);
    let type_count = artifact.types.type_count();
    let interner_len = artifact.interner.len();
    validate_referential_integrity(&artifact, type_count, interner_len)
        .map_err(MsgpackError::Invalid)?;

    Ok(artifact)
}

/// Serializes a [`FhirPackage`] to a spec-conformant named-field `MessagePack`
/// package value (FPF-3): the envelope's named keys with each embedded unit
/// carried as a **nested named value** (OQ-3), not opaque nested bytes.
/// This is the live `.fhirpkg` wire since the FPF-5 switch (package schema
/// v2).
///
/// # Errors
///
/// Returns [`MsgpackError::Encode`] if serialization fails or a module's
/// postcard unit bytes fail the unit decode gates (a package whose units
/// cannot be embedded cannot be encoded); a valid package is expected to
/// succeed.
pub fn encode_package_msgpack(pkg: &FhirPackage) -> Result<Vec<u8>, MsgpackError> {
    let dto = FhirPackageDto::from_package(pkg)?;
    let mut out = Vec::new();
    dto.serialize(Serializer { out: &mut out })?;
    Ok(out)
}

/// Deserializes and validates a [`FhirPackage`] from a `MessagePack` package
/// value (the live `.fhirpkg` wire since the FPF-5 switch, package schema
/// v2), mirroring the gates of the retired postcard envelope codec:
///
/// 0. legacy postcard rejection: bytes with the retired v1 postcard varint
///    prefix fail closed with
///    [`crate::package::FhirPackageError::LegacyPostcardPackage`] — no
///    fallback decode;
/// 1. `package_schema_version` prefix gate (read before payload
///    interpretation; mismatch fails closed, no compatibility decoder);
/// 2. per-module `unit_schema_version` manifest gate and each nested unit's
///    own `schema_version` gate, with module attribution;
/// 3. full DTO deserialize, then per-unit referential-integrity validation
///    and source-hash consistency;
/// 4. package-level checks (sorted/unique module table and dependencies,
///    entry presence/flag consistency, link-table targets).
///
/// The returned package's unit bytes are deterministically re-encoded
/// postcard (the in-memory unit representation), so structural equality
/// against the pre-encode package is the round-trip proof.
///
/// # Errors
///
/// Returns [`MsgpackError::InvalidPackage`] holding
/// [`crate::package::FhirPackageError::LegacyPostcardPackage`] on legacy
/// postcard bytes, [`MsgpackError::Truncated`] on short input,
/// [`MsgpackError::Decode`] on malformed or non-conformant values,
/// [`MsgpackError::UnsupportedPackageSchemaVersion`] on an envelope version
/// mismatch, and [`MsgpackError::InvalidPackage`] for every package-level
/// gate failure (unit version manifest, unit validation, hash consistency,
/// module/dependency/link-table checks).
pub fn decode_package_msgpack(bytes: &[u8]) -> Result<FhirPackage, MsgpackError> {
    // FPF-5 legacy gate (OQ-1 version ratchet): retired postcard envelope
    // bytes fail closed with the named legacy error before any payload
    // interpretation — never a fallback postcard decode.
    if let Some(found) = legacy_postcard_version(bytes) {
        return Err(MsgpackError::InvalidPackage(
            crate::package::FhirPackageError::LegacyPostcardPackage {
                found,
                expected: PACKAGE_SCHEMA_VERSION,
            },
        ));
    }

    let value = Value::from_bytes(bytes)?;

    // §1.2: both version gates run before payload interpretation.
    gate_package_versions(&value)?;

    // Payload interpretation begins only after the version gates accept.
    let dto = FhirPackageDto::deserialize(value)?;

    dto.into_package()
}

#[cfg(test)]
#[path = "msgpack_test.rs"]
mod tests;
