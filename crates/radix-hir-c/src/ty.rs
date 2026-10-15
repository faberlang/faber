//! Type classification for the scalar-and-print subset.

use crate::error::unsupported;
use radix_codegen_shared::CodegenError;
use radix_types::{DefId, NumericWidth, Primitive, SemanticMutability, Type, TypeId, TypeTable};

/// The sized integer widths other than `i64`, which is [`Ty::Int`].
///
/// A sized cell is a plain C integer of that width; the value in it is
/// always in range, because the only way in is a store that checks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum IntWidth {
    I8,
    I16,
    I32,
    U8,
    U16,
    U32,
    U64,
}

impl IntWidth {
    /// The Faber marker, which is also the suffix of the runtime's names
    /// (`faber_store_u8`).
    pub(crate) fn marker(self) -> &'static str {
        match self {
            IntWidth::I8 => "i8",
            IntWidth::I16 => "i16",
            IntWidth::I32 => "i32",
            IntWidth::U8 => "u8",
            IntWidth::U16 => "u16",
            IntWidth::U32 => "u32",
            IntWidth::U64 => "u64",
        }
    }

    pub(crate) fn c_type(self) -> &'static str {
        match self {
            IntWidth::I8 => "int8_t",
            IntWidth::I16 => "int16_t",
            IntWidth::I32 => "int32_t",
            IntWidth::U8 => "uint8_t",
            IntWidth::U16 => "uint16_t",
            IntWidth::U32 => "uint32_t",
            IntWidth::U64 => "uint64_t",
        }
    }

    pub(crate) fn range(self) -> (i128, i128) {
        match self {
            IntWidth::I8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
            IntWidth::I16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
            IntWidth::I32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
            IntWidth::U8 => (0, i128::from(u8::MAX)),
            IntWidth::U16 => (0, i128::from(u16::MAX)),
            IntWidth::U32 => (0, i128::from(u32::MAX)),
            IntWidth::U64 => (0, i128::from(u64::MAX)),
        }
    }
}

/// The classes the subset can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    /// Bare `int` (`i64`): C `int64_t`.
    Int,
    /// `i8`…`u64`: C `intN_t` / `uintN_t`, read as an `int64_t` (an exact
    /// value for `u64`, which does not fit one) and written only through a
    /// range-checked store.
    Sized(IntWidth),
    /// `string`: a NUL-terminated C string of UTF-8 bytes. Text literals
    /// containing NUL are rejected, so the terminator is never ambiguous.
    Text,
    /// `bool`: C `bool` from `<stdbool.h>`.
    Bool,
    /// `float` (binary64): C `double`.
    Float,
    /// `list<int>`, `list<string>` or `list<bool>`: a pointer to an
    /// arena-allocated `faber_list_<elem>`, so every bind aliases (R1).
    List(Elem),
    /// A genus (`class`): a pointer to an arena-allocated C struct, so every
    /// bind aliases (R1).
    Genus(DefId),
}

/// The element kinds a list carries in this subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Elem {
    Int,
    Text,
    Bool,
    Float,
}

impl Elem {
    /// The suffix of the list family's C names (`faber_list_i64_push`).
    pub(crate) fn suffix(self) -> &'static str {
        match self {
            Elem::Int => "i64",
            Elem::Text => "text",
            Elem::Bool => "bool",
            Elem::Float => "f64",
        }
    }

    /// The element's C type inside the list family.
    pub(crate) fn c_type(self) -> &'static str {
        match self {
            Elem::Int => "int64_t",
            Elem::Text => "faber_text",
            Elem::Bool => "bool",
            Elem::Float => "double",
        }
    }

    /// The scalar class an element reads as.
    pub(crate) fn ty(self) -> Ty {
        match self {
            Elem::Int => Ty::Int,
            Elem::Text => Ty::Text,
            Elem::Bool => Ty::Bool,
            Elem::Float => Ty::Float,
        }
    }
}

impl Ty {
    /// The C type of a class whose spelling needs no declaration lookup;
    /// `None` for a genus, which the emitter spells by name
    /// ([`crate::Emitter::c_type`]).
    pub(crate) fn builtin_c_type(self) -> Option<&'static str> {
        match self {
            Ty::Int => Some("int64_t"),
            Ty::Sized(width) => Some(width.c_type()),
            Ty::Text => Some("const char *"),
            Ty::Bool => Some("bool"),
            Ty::Float => Some("double"),
            Ty::List(Elem::Float) => Some("faber_list_f64 *"),
            Ty::List(Elem::Int) => Some("faber_list_i64 *"),
            Ty::List(Elem::Text) => Some("faber_list_text *"),
            Ty::List(Elem::Bool) => Some("faber_list_bool *"),
            Ty::Genus(_) => None,
        }
    }
}

/// Classify a semantic type, rejecting everything outside the subset by name.
pub(crate) fn classify(types: &TypeTable, ty: TypeId) -> Result<Ty, CodegenError> {
    match types.get(ty) {
        Type::Alias(_, inner) => classify(types, *inner),
        Type::Ref(SemanticMutability::Copy, _) => Err(unsupported("copy marker")),
        // A `ref` or `mut` view of a list or a genus is the same pointer: both
        // already alias (R1). A view of a scalar would need a pointer to the
        // caller's cell, which this subset does not emit.
        Type::Ref(_, inner) => match classify(types, *inner)? {
            shared @ (Ty::List(_) | Ty::Genus(_)) => Ok(shared),
            _ => Err(unsupported("ref or mut access to a scalar")),
        },
        Type::Primitive(Primitive::Numerus)
        | Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64) => Ok(Ty::Int),
        Type::SizedNumeric(Primitive::Numerus, width) => match width {
            NumericWidth::I8 => Ok(Ty::Sized(IntWidth::I8)),
            NumericWidth::I16 => Ok(Ty::Sized(IntWidth::I16)),
            NumericWidth::I32 => Ok(Ty::Sized(IntWidth::I32)),
            NumericWidth::U8 => Ok(Ty::Sized(IntWidth::U8)),
            NumericWidth::U16 => Ok(Ty::Sized(IntWidth::U16)),
            NumericWidth::U32 => Ok(Ty::Sized(IntWidth::U32)),
            NumericWidth::U64 => Ok(Ty::Sized(IntWidth::U64)),
            _ => Err(unsupported(describe(types.get(ty)))),
        },
        Type::Primitive(Primitive::Textus) => Ok(Ty::Text),
        Type::Primitive(Primitive::Bivalens) => Ok(Ty::Bool),
        Type::Primitive(Primitive::Fractus)
        | Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => Ok(Ty::Float),
        Type::Array(inner) => match classify(types, *inner)? {
            Ty::Int => Ok(Ty::List(Elem::Int)),
            Ty::Sized(_) => Err(unsupported("list of sized integers")),
            Ty::Text => Ok(Ty::List(Elem::Text)),
            Ty::Bool => Ok(Ty::List(Elem::Bool)),
            Ty::Float => Ok(Ty::List(Elem::Float)),
            Ty::List(_) => Err(unsupported("nested list type")),
            Ty::Genus(_) => Err(unsupported("list of genus")),
        },
        Type::Struct(def_id) => Ok(Ty::Genus(*def_id)),
        other => Err(unsupported(describe(other))),
    }
}

/// The class of a function result: `None` for a function with no result.
pub(crate) fn returns(types: &TypeTable, ty: TypeId) -> Result<Option<Ty>, CodegenError> {
    match types.get(ty) {
        Type::Alias(_, inner) => returns(types, *inner),
        Type::Primitive(Primitive::Vacuum) => Ok(None),
        _ => classify(types, ty).map(Some),
    }
}

fn describe(ty: &Type) -> &'static str {
    match ty {
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
            "float width other than f64"
        }
        Type::Primitive(Primitive::Vacuum) => "value-less type",
        Type::Primitive(Primitive::Regex | Primitive::Match) => "regex type",
        Type::Primitive(Primitive::Ascii) => "ascii text type",
        Type::Primitive(Primitive::Littera) => "character type",
        Type::Primitive(Primitive::Octeti) => "byte buffer type",
        Type::Primitive(Primitive::Json | Primitive::Valor) => "json or data value type",
        Type::Primitive(Primitive::Instans) => "time instant type",
        Type::Primitive(Primitive::Ignotum) => "unknown (ignotum) type",
        Type::Primitive(Primitive::Nihil | Primitive::Numquam) => "null or never type",
        // `inf` needs an arena-backed bignum (a C port of `Magnus`); goal unit C6.
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf) => "unbounded integer type (inf)",
        Type::SizedNumeric(..) => "sized integer type",
        Type::ModularWord(_) | Type::SaturatingWord(_) | Type::PolicyFloat(..) => {
            "wrapping, saturating, or policy numeric type"
        }
        Type::BoundedArray { .. } => "bounded list type",
        Type::Map(..) | Type::Ledger(..) => "map type",
        Type::Tensor(..)
        | Type::Vector(..)
        | Type::Matrix(..)
        | Type::Sparsa(..)
        | Type::TensorEdgePolicy(_) => "tensor type",
        Type::BoundedTextus { .. } | Type::BoundedAscii { .. } | Type::BoundedOcteti { .. } => {
            "bounded text or byte type"
        }
        Type::Record(_) => "record type",
        Type::Promissum(_)
        | Type::PromissumFailable(..)
        | Type::Cursor(_)
        | Type::AsyncCursor(..) => "async or generator type",
        Type::Atomic(_) => "atomic type",
        Type::Intervallum(_) => "range type",
        Type::Bits(_) => "raw-bits type",
        Type::SizedInstans(_) => "time instant type",
        Type::Intersection(_) => "intersection type",
        Type::Set(_) => "set type",
        Type::Queue(_) | Type::BoundedQueue { .. } | Type::Stack(_) | Type::BoundedStack { .. } => {
            "queue or stack type"
        }
        Type::Enum(_) => "enum type",
        Type::Interface(_) => "interface type",
        Type::Option(_) => "optional (nullable) type",
        Type::Union(_) => "union type",
        Type::Tuple(_) => "tuple type",
        Type::Func(_) => "function value type",
        Type::Param(_) | Type::Applied(..) => "generic type",
        _ => "type outside the scalar subset",
    }
}
