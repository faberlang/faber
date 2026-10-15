//! Type classification for the scalar-and-print subset.

use crate::error::unsupported;
use radix_codegen_shared::CodegenError;
use radix_types::{NumericWidth, Primitive, SemanticMutability, Type, TypeId, TypeTable};

/// The classes the subset can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    /// Bare `int` (`i64`): C `int64_t`.
    Int,
    /// `string`: a NUL-terminated C string of UTF-8 bytes. Text literals
    /// containing NUL are rejected, so the terminator is never ambiguous.
    Text,
    /// `bool`: C `bool` from `<stdbool.h>`.
    Bool,
    /// `list<int>`, `list<string>` or `list<bool>`: a pointer to an
    /// arena-allocated `faber_list_<elem>`, so every bind aliases (R1).
    List(Elem),
}

/// The element kinds a list carries in this subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Elem {
    Int,
    Text,
    Bool,
}

impl Elem {
    /// The suffix of the list family's C names (`faber_list_i64_push`).
    pub(crate) fn suffix(self) -> &'static str {
        match self {
            Elem::Int => "i64",
            Elem::Text => "text",
            Elem::Bool => "bool",
        }
    }

    /// The element's C type inside the list family.
    pub(crate) fn c_type(self) -> &'static str {
        match self {
            Elem::Int => "int64_t",
            Elem::Text => "faber_text",
            Elem::Bool => "bool",
        }
    }

    /// The scalar class an element reads as.
    pub(crate) fn ty(self) -> Ty {
        match self {
            Elem::Int => Ty::Int,
            Elem::Text => Ty::Text,
            Elem::Bool => Ty::Bool,
        }
    }
}

impl Ty {
    /// The C declaration of `name` with this class. An immutable binding is
    /// `const` all the way down: a text binding is a const pointer to const
    /// chars.
    pub(crate) fn declare(self, name: &str, mutable: bool) -> String {
        match (self, mutable) {
            (Ty::Int, false) => format!("const int64_t {name}"),
            (Ty::Int, true) => format!("int64_t {name}"),
            (Ty::Text, false) => format!("const char *const {name}"),
            (Ty::Text, true) => format!("const char *{name}"),
            (Ty::Bool, false) => format!("const bool {name}"),
            (Ty::Bool, true) => format!("bool {name}"),
            (Ty::List(elem), false) => format!("faber_list_{} *const {name}", elem.suffix()),
            (Ty::List(elem), true) => format!("faber_list_{} *{name}", elem.suffix()),
        }
    }

    /// The C type name of a parameter or return value.
    pub(crate) fn c_type(self) -> &'static str {
        match self {
            Ty::Int => "int64_t",
            Ty::Text => "const char *",
            Ty::Bool => "bool",
            Ty::List(Elem::Int) => "faber_list_i64 *",
            Ty::List(Elem::Text) => "faber_list_text *",
            Ty::List(Elem::Bool) => "faber_list_bool *",
        }
    }
}

/// Classify a semantic type, rejecting everything outside the subset by name.
pub(crate) fn classify(types: &TypeTable, ty: TypeId) -> Result<Ty, CodegenError> {
    match types.get(ty) {
        Type::Alias(_, inner) => classify(types, *inner),
        Type::Ref(SemanticMutability::Copy, _) => Err(unsupported("copy marker")),
        // A `ref` or `mut` view of a list is the same list pointer: a list
        // already aliases (R1). A view of a scalar would need a pointer to the
        // caller's cell, which this subset does not emit.
        Type::Ref(_, inner) => match classify(types, *inner)? {
            list @ Ty::List(_) => Ok(list),
            _ => Err(unsupported("ref or mut access to a scalar")),
        },
        Type::Primitive(Primitive::Numerus)
        | Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64) => Ok(Ty::Int),
        Type::Primitive(Primitive::Textus) => Ok(Ty::Text),
        Type::Primitive(Primitive::Bivalens) => Ok(Ty::Bool),
        Type::Array(inner) => match classify(types, *inner)? {
            Ty::Int => Ok(Ty::List(Elem::Int)),
            Ty::Text => Ok(Ty::List(Elem::Text)),
            Ty::Bool => Ok(Ty::List(Elem::Bool)),
            Ty::List(_) => Err(unsupported("nested list type")),
        },
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
            "float type"
        }
        Type::Primitive(Primitive::Vacuum) => "value-less type",
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
        Type::Struct(_) => "genus type",
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
