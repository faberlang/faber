//! Canonical Faber type spelling from semantic [`Type`] values.
//!
//! Shared by the Faber pretty-printer and target backends that attach Faber
//! type annotations (Rust tensor comments in Stage 7).

use super::{format_figura, MarkerFamily, Primitive, SemanticMutability, Type, TypeId, TypeTable};
use radix_hir::DefId;
use radix_lexer::Interner;

/// Recover Faber source type syntax for one resolved [`TypeId`].
pub fn format_faber_type<F: Fn(DefId) -> String>(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
    resolve_def: &F,
) -> String {
    let ty = types.get(type_id);

    match ty {
        Type::Primitive(prim) => primitive_to_faber(*prim),

        Type::SizedInstans(precision) => format!("instans<{}>", precision.faber_marker()),

        Type::Array(elem) => format!(
            "lista<{}>",
            format_faber_type(*elem, types, interner, resolve_def)
        ),

        Type::BoundedArray { element, capacity } => format!(
            "lista<{}, {}>",
            format_faber_type(*element, types, interner, resolve_def),
            format_figura(*capacity, types, interner)
        ),
        Type::BoundedTextus { capacity } => {
            format!("textus<{}>", format_figura(*capacity, types, interner))
        }
        Type::BoundedAscii { capacity } => {
            format!("ascii<{}>", format_figura(*capacity, types, interner))
        }
        Type::BoundedOcteti { capacity } => {
            format!("octeti<{}>", format_figura(*capacity, types, interner))
        }

        Type::Map(key, value) => format!(
            "tabula<{}, {}>",
            format_faber_type(*key, types, interner, resolve_def),
            format_faber_type(*value, types, interner, resolve_def)
        ),

        Type::Record(_) => "ignotum".to_owned(),

        Type::Set(elem) => format!(
            "copia<{}>",
            format_faber_type(*elem, types, interner, resolve_def)
        ),

        Type::Promissum(inner) => format!(
            "promissum<{}>",
            format_faber_type(*inner, types, interner, resolve_def)
        ),

        Type::PromissumFailable(success, alternate) => format!(
            "promissum<{} ⇥ {}>",
            format_faber_type(*success, types, interner, resolve_def),
            format_faber_type(*alternate, types, interner, resolve_def)
        ),

        Type::Cursor(inner) => format!(
            "cursor<{}>",
            format_faber_type(*inner, types, interner, resolve_def)
        ),

        Type::AsyncCursor(item, alternate) => format!(
            "async cursor<{} ⇥ {}>",
            format_faber_type(*item, types, interner, resolve_def),
            format_faber_type(*alternate, types, interner, resolve_def)
        ),

        Type::Tensor(elem, shape) => format!(
            "tensor<{}, {}>",
            format_faber_type(*elem, types, interner, resolve_def),
            format_figura(*shape, types, interner)
        ),

        Type::Vector(elem, width) => format!(
            "vector<{}, {}>",
            format_faber_type(*elem, types, interner, resolve_def),
            format_figura(*width, types, interner)
        ),

        Type::Matrix(elem, shape) => format!(
            "matrix<{}, {}>",
            format_faber_type(*elem, types, interner, resolve_def),
            format_figura(*shape, types, interner)
        ),

        Type::Sparsa(elem, shape) => format!(
            "sparsa<{}, {}>",
            format_faber_type(*elem, types, interner, resolve_def),
            format_figura(*shape, types, interner)
        ),

        Type::Atomic(elem) => format!(
            "atomic<{}>",
            format_faber_type(*elem, types, interner, resolve_def)
        ),

        Type::Intervallum(bound) => {
            format!(
                "intervallum<{}>",
                format_faber_type(*bound, types, interner, resolve_def)
            )
        }

        Type::SizedNumeric(prim, width) => match prim {
            Primitive::Numerus => format!("numerus<{}>", width.faber_marker()),
            Primitive::Fractus => format!("fractus<{}>", width.faber_marker()),
            _ => "ignotum".to_owned(),
        },

        Type::ModularWord(width) => format!("modulus<{}>", width.faber_marker()),

        Type::Option(inner) => {
            let inner =
                format_faber_type(flatten_option(*inner, types), types, interner, resolve_def);
            format!("{inner} ∪ nihil")
        }

        Type::Ref(mutability, inner) => {
            let prefix = match mutability {
                SemanticMutability::Immutable => "de",
                SemanticMutability::Mutable => "in",
            };
            format!(
                "{} {}",
                prefix,
                format_faber_type(*inner, types, interner, resolve_def)
            )
        }

        Type::Struct(def_id) | Type::Enum(def_id) | Type::Interface(def_id) => resolve_def(*def_id),

        Type::Alias(def_id, _resolved) => resolve_def(*def_id),

        Type::Func(sig) => {
            let params: Vec<String> = sig
                .params
                .iter()
                .map(|p| format_faber_type(p.ty, types, interner, resolve_def))
                .collect();
            let ret = format_faber_type(sig.ret, types, interner, resolve_def);
            if let Some(err) = sig.err {
                let err = format_faber_type(err, types, interner, resolve_def);
                format!("({}) → {} ⇥ {}", params.join(", "), ret, err)
            } else {
                format!("({}) → {}", params.join(", "), ret)
            }
        }

        Type::Param(sym) => interner.resolve(*sym).to_owned(),

        Type::Applied(base, args) => {
            let base_str = format_faber_type(*base, types, interner, resolve_def);
            let args_str: Vec<String> = args
                .iter()
                .map(|a| format_faber_type(*a, types, interner, resolve_def))
                .collect();
            format!("{}<{}>", base_str, args_str.join(", "))
        }

        Type::Infer(_) => "_".to_owned(),
        Type::InferUnion(_) => "∪".to_owned(),
        Type::MarkerHole { family, .. } => match family {
            MarkerFamily::Numerus => "numerus<_>",
            MarkerFamily::Fractus => "fractus<_>",
            MarkerFamily::Modulus => "modulus<_>",
            MarkerFamily::Instans => "instans<_>",
        }
        .to_owned(),
        Type::Tuple(elements) => {
            let args: Vec<String> = elements
                .iter()
                .map(|e| format_faber_type(*e, types, interner, resolve_def))
                .collect();
            format!("iuncta<{}>", args.join(", "))
        }
        Type::Union(_) => "ignotum".to_owned(),
        Type::Error => "<error>".to_owned(),
    }
}

fn flatten_option(mut type_id: TypeId, types: &TypeTable) -> TypeId {
    while let Type::Option(inner) = types.get(type_id) {
        type_id = *inner;
    }
    type_id
}

fn primitive_to_faber(prim: Primitive) -> String {
    match prim {
        Primitive::Ascii => "ascii",
        Primitive::Textus => "textus",
        Primitive::Numerus => "numerus",
        Primitive::Fractus => "fractus",
        Primitive::Bivalens => "bivalens",
        Primitive::Nihil => "nihil",
        Primitive::Vacuum => "vacuum",
        Primitive::Numquam => "numquam",
        Primitive::Ignotum => "ignotum",
        Primitive::Json => "json",
        Primitive::Valor => "valor",
        Primitive::Octeti => "octeti",
        Primitive::Regex => "regex",
        Primitive::Instans => "instans",
    }
    .to_owned()
}
