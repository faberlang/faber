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

    /// The C type of a `list<wrapping<W>>` (`faber_list_<marker> *`).
    pub(crate) fn list_c_pointer(self) -> &'static str {
        match self {
            IntWidth::I8 => "faber_list_i8 *",
            IntWidth::I16 => "faber_list_i16 *",
            IntWidth::I32 => "faber_list_i32 *",
            IntWidth::U8 => "faber_list_u8 *",
            IntWidth::U16 => "faber_list_u16 *",
            IntWidth::U32 => "faber_list_u32 *",
            IntWidth::U64 => "faber_list_u64 *",
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

/// What an optional (`T ∪ none`) carries. Text, lists and genera are already
/// pointers, so `NULL` is their none; the scalars carry a presence flag in a
/// small struct (`faber_opt_i64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Inner {
    Int,
    Text,
    Bool,
    Float,
    /// `f32`: a binary32. It shares the `faber_opt_f64` slot (widened) and is
    /// narrowed back on a read.
    Float32,
    /// `wrapping<W>`: a modular word, widened on read and reduced at the
    /// store. `u64` carries its value in a `uint64_t` optional slot.
    Modular(IntWidth),
    List(Elem),
    Genus(DefId),
}

impl Inner {
    /// The class of the value inside.
    pub(crate) fn ty(self) -> Ty {
        match self {
            Inner::Int => Ty::Int,
            Inner::Text => Ty::Text,
            Inner::Bool => Ty::Bool,
            Inner::Float => Ty::Float,
            Inner::Float32 => Ty::Float32,
            Inner::Modular(width) => Ty::Modular(width),
            Inner::List(elem) => Ty::List(elem),
            Inner::Genus(def_id) => Ty::Genus(def_id),
        }
    }

    pub(crate) fn from_ty(ty: Ty) -> Option<Inner> {
        match ty {
            Ty::Int => Some(Inner::Int),
            Ty::Text => Some(Inner::Text),
            Ty::Bool => Some(Inner::Bool),
            Ty::Float => Some(Inner::Float),
            Ty::Float32 => Some(Inner::Float32),
            Ty::Modular(width) => Some(Inner::Modular(width)),
            Ty::List(elem) => Some(Inner::List(elem)),
            Ty::Genus(def_id) => Some(Inner::Genus(def_id)),
            Ty::Sized(_)
            | Ty::Saturating(_)
            | Ty::Opt(_)
            | Ty::None
            | Ty::Decimal
            | Ty::Set(_)
            | Ty::Map(..) => None,
        }
    }

    /// The value is a pointer and `NULL` means none.
    pub(crate) fn is_pointer(self) -> bool {
        matches!(self, Inner::Text | Inner::List(_) | Inner::Genus(_))
    }

    /// The runtime's option struct for a scalar. A modular word below `u64`
    /// fits `faber_opt_i64`; `u64` needs its own unsigned slot.
    pub(crate) fn scalar_struct(self) -> Option<&'static str> {
        match self {
            Inner::Modular(IntWidth::U64) => Some("faber_opt_u64"),
            Inner::Int | Inner::Modular(_) => Some("faber_opt_i64"),
            Inner::Bool => Some("faber_opt_bool"),
            Inner::Float | Inner::Float32 => Some("faber_opt_f64"),
            Inner::Text | Inner::List(_) | Inner::Genus(_) => None,
        }
    }
}

/// The classes the subset can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    /// `T ∪ none` over a class that has one.
    Opt(Inner),
    /// Bare `int` (`i64`): C `int64_t`.
    Int,
    /// `d64`: an exact decimal. A stored cell holds a `faber_dec` pair at
    /// scale 8; an unstored computation holds its own scale.
    Decimal,
    /// `i8`…`u64`: C `intN_t` / `uintN_t`, read as an `int64_t` (an exact
    /// value for `u64`, which does not fit one) and written only through a
    /// range-checked store.
    Sized(IntWidth),
    /// `wrapping<W>` (a `modulus<W>`): the plain C width, read widened, and
    /// written only through a store that reduces modulo 2^W.
    Modular(IntWidth),
    /// `saturating<W>`: the plain C width, read widened, and written only
    /// through a store that clamps to the width's ends.
    Saturating(IntWidth),
    /// `string`: a NUL-terminated C string of UTF-8 bytes. Text literals
    /// containing NUL are rejected, so the terminator is never ambiguous.
    Text,
    /// `bool`: C `bool` from `<stdbool.h>`.
    Bool,
    /// `float` (binary64): C `double`.
    Float,
    /// `f32` (binary32): C `float`. Arithmetic and display round at the
    /// binary32 width; a literal is the front end's folded binary32 value.
    Float32,
    /// `list<int>`, `list<string>` or `list<bool>`: a pointer to an
    /// arena-allocated `faber_list_<elem>`, so every bind aliases (R1).
    List(Elem),
    /// `set<int>`, `set<string>` or `set<bool>`: the same list storage with
    /// the set contract (`add` is unique, `delete` by value), so a set is a
    /// `faber_list_<elem> *` too.
    Set(Elem),
    /// `map<K, V>` (`tabula`): an arena-allocated `faber_map_<k>_<v>` handled
    /// by pointer, so every bind aliases (R1). Keys and values are `int` or
    /// `text`, the classes the corpus maps use.
    Map(Elem, Elem),
    /// A genus (`class`): a pointer to an arena-allocated C struct, so every
    /// bind aliases (R1).
    Genus(DefId),
    /// A bare `none` value (the type is `none`): the only value is `null`, and
    /// it prints as the locale's none word. C carries a dummy `int` 0.
    None,
}

/// The element kinds a list carries in this subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Elem {
    Int,
    Text,
    Bool,
    Float,
    /// `f32`: the stored cell is a `float`, and its helpers are the f32 list
    /// family.
    Float32,
    /// `wrapping<W>`: the list stores the plain C width and its helpers are
    /// the `FABER_LIST_FAMILY`/`FABER_LIST_OPS` instantiations for that width.
    Modular(IntWidth),
}

impl Elem {
    /// The suffix of the list family's C names (`faber_list_i64_push`).
    pub(crate) fn suffix(self) -> &'static str {
        match self {
            Elem::Int => "i64",
            Elem::Text => "text",
            Elem::Bool => "bool",
            Elem::Float => "f64",
            Elem::Float32 => "f32",
            Elem::Modular(width) => width.marker(),
        }
    }

    /// The element's C type inside the list family.
    pub(crate) fn c_type(self) -> &'static str {
        match self {
            Elem::Int => "int64_t",
            Elem::Text => "faber_text",
            Elem::Bool => "bool",
            Elem::Float => "double",
            Elem::Float32 => "float",
            Elem::Modular(width) => width.c_type(),
        }
    }

    /// The pointer type of this element's list family (`faber_list_u8 *`).
    pub(crate) fn list_c_pointer(self) -> &'static str {
        match self {
            Elem::Int => "faber_list_i64 *",
            Elem::Text => "faber_list_text *",
            Elem::Bool => "faber_list_bool *",
            Elem::Float => "faber_list_f64 *",
            Elem::Float32 => "faber_list_f32 *",
            Elem::Modular(width) => width.list_c_pointer(),
        }
    }

    /// The scalar class an element reads as.
    pub(crate) fn ty(self) -> Ty {
        match self {
            Elem::Int => Ty::Int,
            Elem::Text => Ty::Text,
            Elem::Bool => Ty::Bool,
            Elem::Float => Ty::Float,
            Elem::Float32 => Ty::Float32,
            Elem::Modular(width) => Ty::Modular(width),
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
            Ty::None => Some("int"),
            Ty::Sized(width) | Ty::Modular(width) | Ty::Saturating(width) => Some(width.c_type()),
            Ty::Opt(Inner::Genus(_)) => None,
            Ty::Opt(inner) => inner
                .scalar_struct()
                .or_else(|| inner.ty().builtin_c_type()),
            Ty::Text => Some("const char *"),
            Ty::Bool => Some("bool"),
            Ty::Float => Some("double"),
            Ty::Float32 => Some("float"),
            Ty::Decimal => Some("faber_dec"),
            Ty::List(elem) | Ty::Set(elem) => Some(elem.list_c_pointer()),
            Ty::Map(k, v) => match (k, v) {
                (Elem::Int, Elem::Int) => Some("faber_map_i64_i64 *"),
                (Elem::Int, Elem::Text) => Some("faber_map_i64_text *"),
                (Elem::Text, Elem::Int) => Some("faber_map_text_i64 *"),
                (Elem::Text, Elem::Text) => Some("faber_map_text_text *"),
                _ => None,
            },
            Ty::Genus(_) => None,
        }
    }
}

/// Whether a slot's declared type carries the value-side `copy` marker (R7).
/// A declaration initializer's `copy` lands on the declared type, so the
/// local's own type says the bind deep-duplicates its source.
pub(crate) fn is_copy_marker(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Alias(_, inner) => is_copy_marker(types, *inner),
        Type::Ref(SemanticMutability::Copy, _) => true,
        _ => false,
    }
}

/// The [`IntWidth`] of a numeric width marker, when it is an integer or
/// word width. `f32`/`f64`/`d64`/`inf` are not.
fn int_width(width: NumericWidth) -> Option<IntWidth> {
    match width {
        NumericWidth::I8 => Some(IntWidth::I8),
        NumericWidth::I16 => Some(IntWidth::I16),
        NumericWidth::I32 => Some(IntWidth::I32),
        NumericWidth::U8 => Some(IntWidth::U8),
        NumericWidth::U16 => Some(IntWidth::U16),
        NumericWidth::U32 => Some(IntWidth::U32),
        NumericWidth::U64 => Some(IntWidth::U64),
        _ => None,
    }
}

/// The element class a map key or value may carry: `int` or `text`.
fn map_elem(ty: Ty) -> Option<Elem> {
    match ty {
        Ty::Int => Some(Elem::Int),
        Ty::Text => Some(Elem::Text),
        _ => None,
    }
}

/// Classify a semantic type, rejecting everything outside the subset by name.
pub(crate) fn classify(types: &TypeTable, ty: TypeId) -> Result<Ty, CodegenError> {
    match types.get(ty) {
        Type::Alias(_, inner) => classify(types, *inner),
        // A `copy` parameter's storage is its declared class: the caller
        // passes a deep duplicate (the call-site marker drives `copy_value`),
        // so the callee sees an ordinary value.
        Type::Ref(SemanticMutability::Copy, inner) => classify(types, *inner),
        // A `ref` or `mut` view of a list, a genus or text is the same
        // pointer: all three already alias (R1). A view of a scalar is a
        // pointer to the caller's cell, which `classify_ref` spells for a
        // parameter; anywhere else it is still rejected by name.
        Type::Ref(_, inner) => match classify(types, *inner)? {
            shared @ (Ty::List(_) | Ty::Genus(_) | Ty::Opt(Inner::Genus(_)) | Ty::Text) => {
                Ok(shared)
            }
            _ => Err(unsupported("ref or mut access to a scalar")),
        },
        Type::Primitive(Primitive::Numerus)
        | Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64) => Ok(Ty::Int),
        Type::SizedNumeric(Primitive::Numerus, width) => match width {
            NumericWidth::D64 => Ok(Ty::Decimal),
            NumericWidth::I8 => Ok(Ty::Sized(IntWidth::I8)),
            NumericWidth::I16 => Ok(Ty::Sized(IntWidth::I16)),
            NumericWidth::I32 => Ok(Ty::Sized(IntWidth::I32)),
            NumericWidth::U8 => Ok(Ty::Sized(IntWidth::U8)),
            NumericWidth::U16 => Ok(Ty::Sized(IntWidth::U16)),
            NumericWidth::U32 => Ok(Ty::Sized(IntWidth::U32)),
            NumericWidth::U64 => Ok(Ty::Sized(IntWidth::U64)),
            _ => Err(unsupported(describe(types.get(ty)))),
        },
        Type::ModularWord(width) => match int_width(*width) {
            Some(width) => Ok(Ty::Modular(width)),
            None => Err(unsupported(describe(types.get(ty)))),
        },
        Type::SaturatingWord(width) => match int_width(*width) {
            Some(width) => Ok(Ty::Saturating(width)),
            None => Err(unsupported(describe(types.get(ty)))),
        },
        Type::Primitive(Primitive::Textus) => Ok(Ty::Text),
        // `ascii` is an ASCII-only text with no capacity; `ascii<N>` and
        // `string<N>` are bounded text whose capacity the typechecker proves a
        // literal fits and whose runtime conversion checks the rest. All are
        // the counted UTF-8 text value the target already emits.
        Type::Primitive(Primitive::Ascii) | Type::BoundedAscii { .. } => Ok(Ty::Text),
        // A bare `none` (`const none nothing ← null`): its only value is null.
        Type::Primitive(Primitive::Nihil) => Ok(Ty::None),
        Type::BoundedTextus { .. } => Ok(Ty::Text),
        Type::Primitive(Primitive::Bivalens) => Ok(Ty::Bool),
        Type::Primitive(Primitive::Fractus)
        | Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => Ok(Ty::Float),
        // `f32`: a binary32 whose operations round per operation, the runner's
        // declared-width rule (2026-10-15T1445 ruling).
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => Ok(Ty::Float32),
        // A bounded list is a list whose value carries its capacity at run
        // time (`fixed`), so it is the same class.
        Type::Array(inner) | Type::BoundedArray { element: inner, .. } => {
            match classify(types, *inner)? {
                Ty::Int => Ok(Ty::List(Elem::Int)),
                Ty::Sized(_) => Err(unsupported("list of sized integers")),
                Ty::Opt(_) => Err(unsupported("list of optionals")),
                Ty::Text => Ok(Ty::List(Elem::Text)),
                Ty::Bool => Ok(Ty::List(Elem::Bool)),
                Ty::Float => Ok(Ty::List(Elem::Float)),
                Ty::Float32 => Ok(Ty::List(Elem::Float32)),
                Ty::List(_) => Err(unsupported("nested list type")),
                Ty::Genus(_) => Err(unsupported("list of genus")),
                Ty::Decimal => Err(unsupported("list of decimals")),
                Ty::Modular(width) => Ok(Ty::List(Elem::Modular(width))),
                Ty::Saturating(_) => Err(unsupported("list of saturating words")),
                Ty::None => Err(unsupported("list of none")),
                Ty::Set(_) => Err(unsupported("list of sets")),
                Ty::Map(..) => Err(unsupported("list of maps")),
            }
        }
        // A set is the list storage under the set contract (`add` unique,
        // `delete` by value), so only a scalar element class is carried.
        Type::Set(inner) => match classify(types, *inner)? {
            Ty::Int => Ok(Ty::Set(Elem::Int)),
            Ty::Text => Ok(Ty::Set(Elem::Text)),
            Ty::Bool => Ok(Ty::Set(Elem::Bool)),
            Ty::Float => Ok(Ty::Set(Elem::Float)),
            Ty::Float32 => Err(unsupported("set of f32")),
            Ty::Modular(width) => Ok(Ty::Set(Elem::Modular(width))),
            Ty::List(_) => Err(unsupported("set of lists")),
            Ty::Set(_) => Err(unsupported("set of sets")),
            Ty::Genus(_) => Err(unsupported("set of genus")),
            Ty::Sized(_) => Err(unsupported("set of sized integers")),
            Ty::Saturating(_) => Err(unsupported("set of saturating words")),
            Ty::Opt(_) => Err(unsupported("set of optionals")),
            Ty::Decimal => Err(unsupported("set of decimals")),
            Ty::None => Err(unsupported("set of none")),
            Ty::Map(..) => Err(unsupported("set of maps")),
        },
        // A map carries `int` or `text` keys and values (the corpus classes),
        // backed by parallel lists in `faber_map_<k>_<v>`.
        Type::Map(key, value) => {
            let k = map_elem(classify(types, *key)?)
                .ok_or_else(|| unsupported("map with a key class outside int/text"))?;
            let v = map_elem(classify(types, *value)?)
                .ok_or_else(|| unsupported("map with a value class outside int/text"))?;
            Ok(Ty::Map(k, v))
        }
        // An enumeration is a genus-shaped value: a tagged pointer.
        Type::Struct(def_id) | Type::Enum(def_id) => Ok(Ty::Genus(*def_id)),
        Type::Option(inner) => match Inner::from_ty(classify(types, *inner)?) {
            Some(inner) => Ok(Ty::Opt(inner)),
            None => Err(unsupported("optional of a sized integer or an optional")),
        },
        other => Err(unsupported(describe(other))),
    }
}

/// The class of a `ref`/`mut` parameter whose C storage is a pointer to the
/// caller's cell. `None` when `ty` is not such a view: an ordinary parameter,
/// or a view of a list, genus or text (each already passes its pointer).
pub(crate) fn classify_ref(types: &TypeTable, ty: TypeId) -> Result<Option<Ty>, CodegenError> {
    match types.get(ty) {
        Type::Alias(_, inner) => classify_ref(types, *inner),
        Type::Ref(SemanticMutability::Copy, _) => Ok(None),
        Type::Ref(_, inner) => match classify(types, *inner)? {
            Ty::List(_) | Ty::Genus(_) | Ty::Opt(Inner::Genus(_)) | Ty::Text => Ok(None),
            class => Ok(Some(class)),
        },
        _ => Ok(None),
    }
}

/// The class a guard-proved read narrows to: strip an alias and a `ref`/`mut`
/// view, then classify.
pub(crate) fn read_class(types: &TypeTable, ty: TypeId) -> Option<Ty> {
    match types.get(ty) {
        Type::Alias(_, inner) | Type::Ref(_, inner) => read_class(types, *inner),
        _ => classify(types, ty).ok(),
    }
}

/// The class of a function result: `None` for a function with no result.
pub(crate) fn returns(types: &TypeTable, ty: TypeId) -> Result<Option<Ty>, CodegenError> {
    match types.get(ty) {
        Type::Alias(_, inner) => returns(types, *inner),
        // `→ none` names a function that returns nothing, and `→ never` one
        // that does not return at all (its body panics or traps); a use of
        // either result as a value is rejected where it is used.
        Type::Primitive(Primitive::Vacuum | Primitive::Nihil | Primitive::Numquam) => Ok(None),
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
        Type::Interface(_) => "interface type",
        Type::Option(_) => "optional (nullable) type",
        Type::Union(_) => "union type",
        Type::Tuple(_) => "tuple type",
        Type::Func(_) => "function value type",
        Type::Param(_) | Type::Applied(..) => "generic type",
        _ => "type outside the scalar subset",
    }
}
