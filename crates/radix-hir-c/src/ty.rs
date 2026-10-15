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
        }
    }
}

/// Classify a semantic type, rejecting everything outside the subset by name.
pub(crate) fn classify(types: &TypeTable, ty: TypeId) -> Result<Ty, CodegenError> {
    match types.get(ty) {
        Type::Alias(_, inner) => classify(types, *inner),
        Type::Ref(SemanticMutability::Copy, _) => Err(unsupported("copy marker")),
        Type::Primitive(Primitive::Numerus)
        | Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64) => Ok(Ty::Int),
        Type::Primitive(Primitive::Textus) => Ok(Ty::Text),
        other => Err(unsupported(describe(other))),
    }
}

fn describe(ty: &Type) -> &'static str {
    match ty {
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
            "float type"
        }
        Type::Primitive(Primitive::Bivalens) => "boolean type",
        Type::Primitive(Primitive::Vacuum) => "value-less type",
        Type::SizedNumeric(..) => "sized integer type",
        Type::ModularWord(_) | Type::SaturatingWord(_) | Type::PolicyFloat(..) => {
            "wrapping, saturating, or policy numeric type"
        }
        Type::Array(_) | Type::BoundedArray { .. } => "list type",
        Type::Map(..) => "map type",
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
