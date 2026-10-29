//! Canonical Faber type spelling from semantic [`Type`] values.
//!
//! Shared by the Faber pretty-printer and target backends that attach Faber
//! type annotations (Rust tensor comments in Stage 7).

use super::{MarkerFamily, Primitive, SemanticMutability, Type, TypeId, TypeTable, format_figura};
use handroll::FxHashMap;
use radix_hir::DefId;
use radix_lexer::{Interner, Symbol};
use radix_types::{AppliedArg, IndexExpr, IndexId};

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
        Type::TensorEdgePolicy(policy) => policy.latin().to_owned(),

        Type::SizedInstans(precision) => format!("instans<{}>", precision.faber_marker()),

        Type::Array(elem) => format!(
            "lista<{}>",
            format_faber_type(*elem, types, interner, resolve_def)
        ),

        Type::BoundedArray { element, capacity } => format!(
            "lista<{}, {}>",
            format_faber_type(*element, types, interner, resolve_def),
            format_capacity(*capacity, types, interner)
        ),
        Type::RangedArray { element, min, max } => format!(
            "lista<{}, {min}…{}>",
            format_faber_type(*element, types, interner, resolve_def),
            max.map_or_else(|| "∞".to_owned(), |max| max.to_string())
        ),
        Type::Queue(elem) => format!(
            "queue<{}>",
            format_faber_type(*elem, types, interner, resolve_def)
        ),
        Type::BoundedQueue { element, capacity } => format!(
            "queue<{}, {}>",
            format_faber_type(*element, types, interner, resolve_def),
            format_capacity(*capacity, types, interner)
        ),
        Type::Stack(elem) => format!(
            "stack<{}>",
            format_faber_type(*elem, types, interner, resolve_def)
        ),
        Type::BoundedStack { element, capacity } => format!(
            "stack<{}, {}>",
            format_faber_type(*element, types, interner, resolve_def),
            format_capacity(*capacity, types, interner)
        ),
        Type::BoundedTextus { capacity } => {
            format!("textus<{}>", format_capacity(*capacity, types, interner))
        }
        Type::BoundedAscii { capacity } => {
            format!("ascii<{}>", format_capacity(*capacity, types, interner))
        }
        Type::BoundedOcteti { capacity } => {
            format!("lista<b8, {}>", format_capacity(*capacity, types, interner))
        }

        Type::Map(key, value) => format!(
            "tabula<{}, {}>",
            format_faber_type(*key, types, interner, resolve_def),
            format_faber_type(*value, types, interner, resolve_def)
        ),

        Type::Ledger(key, value) => format!(
            "tabulae<{}, {}>",
            format_faber_type(*key, types, interner, resolve_def),
            format_faber_type(*value, types, interner, resolve_def)
        ),

        Type::Record(fields) => format_record_type(fields, types, interner, resolve_def),

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

        Type::Intervallum(bound, step) => {
            let bound = format_faber_type(*bound, types, interner, resolve_def);
            // A literal step other than 1 renders as `intervallum<T, S>`; the
            // canonical step 1 and any symbolic term keep the bare spelling.
            match types.get_index(*step) {
                IndexExpr::Literal(s) if *s != 1 => format!("intervallum<{bound}, {s}>"),
                _ => format!("intervallum<{bound}>"),
            }
        }

        // Bare width marker: the wrapped `numerus<W>` / `fractus<W>` spellings
        // are retired (N7c).
        Type::SizedNumeric(prim, width) => match prim {
            Primitive::Numerus | Primitive::Fractus => width.faber_marker().to_owned(),
            _ => "ignotum".to_owned(),
        },

        Type::Bits(width) => width.faber_marker().to_owned(),

        Type::ModularWord(width) => format!("modulus<{}>", width.faber_marker()),
        Type::SaturatingWord(width) => format!("saturatus<{}>", width.faber_marker()),
        Type::RangedInt {
            policy,
            width,
            lo,
            hi,
        } => format!("{}<{}, {lo}…{hi}>", policy.latin(), width.faber_marker()),
        Type::PolicyFloat(policy, width) => {
            let word = match policy {
                radix_types::FloatPolicy::Trapping => "exactus",
                radix_types::FloatPolicy::Saturating => "saturatus",
            };
            format!("{word}<{}>", width.faber_marker())
        }

        Type::Option(inner) => {
            let inner =
                format_faber_type(flatten_option(*inner, types), types, interner, resolve_def);
            format!("{inner} ∪ nihil")
        }

        Type::Ref(mutability, inner) => {
            let prefix = match mutability {
                SemanticMutability::Immutable => "de",
                SemanticMutability::Mutable => "in",
                SemanticMutability::Own => "own",
                SemanticMutability::Copy => "copy",
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
                .map(|arg| match arg {
                    AppliedArg::Type(id) => format_faber_type(*id, types, interner, resolve_def),
                    AppliedArg::Size(id) => format_figura(*id, types, interner),
                })
                .collect();
            format!("{}<{}>", base_str, args_str.join(", "))
        }

        Type::Infer(_) => "_".to_owned(),
        Type::InferUnion(_) => "∪".to_owned(),
        Type::MarkerHole { family, .. } => match family {
            MarkerFamily::Modulus => "modulus<_>",
            MarkerFamily::Saturatus => "saturatus<_>",
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
        Type::Intersection(members) => members
            .iter()
            .map(|member| format_faber_type(*member, types, interner, resolve_def))
            .collect::<Vec<_>>()
            .join(" ∩ "),
        Type::Error => "<error>".to_owned(),
    }
}

/// Canonical ratio field order: sorted by label spelling.
fn sorted_record_fields(
    fields: &FxHashMap<Symbol, TypeId>,
    interner: &Interner,
) -> Vec<(Symbol, TypeId)> {
    let mut ordered: Vec<_> = fields.iter().map(|(&name, &ty)| (name, ty)).collect();
    ordered.sort_by(|(left, _), (right, _)| interner.resolve(*left).cmp(interner.resolve(*right)));
    ordered
}

fn format_record_type<F: Fn(DefId) -> String>(
    fields: &FxHashMap<Symbol, TypeId>,
    types: &TypeTable,
    interner: &Interner,
    resolve_def: &F,
) -> String {
    let entries: Vec<String> = sorted_record_fields(fields, interner)
        .into_iter()
        .map(|(name, ty)| {
            format!(
                "{}: {}",
                interner.resolve(name),
                format_faber_type(ty, types, interner, resolve_def)
            )
        })
        .collect();
    format!("ratio<{}>", entries.join(", "))
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
        Primitive::Littera => "littera",
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
        Primitive::Octeti => "lista<b8>",
        Primitive::Regex => "regex",
        // RD-3: internal display name only; there is no source spelling.
        Primitive::Match => "match",
        Primitive::Instans => "instans",
    }
    .to_owned()
}

/// A bounded head's capacity as written after the `size` marker
/// (`queue<int, size 4>`, `ascii<size 1>`); the `_` hole stays bare.
fn format_capacity(capacity: IndexId, types: &TypeTable, interner: &Interner) -> String {
    let figura = format_figura(capacity, types, interner);
    if figura == "_" {
        figura
    } else {
        format!("size {figura}")
    }
}
