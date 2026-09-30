//! Kernel value-type legality: which types a kernel may name (gpu-reset
//! rule 12 point 2 and 3).
//!
//! Boundary: pure classification over the shared type table. It returns the
//! family of the first host-only type inside a value type, so a diagnostic can
//! name what to remove; it decides nothing about numeric widths, which belong
//! to the per-target requirement inference.

use super::kernel_legality::ModuleFacts;
use crate::{Primitive, Type, TypeId, TypeTable};
use radix_hir::DefId;
use rustc_hash::FxHashSet;

/// Family label of an atomic cell: compiler-internal workgroup state, not a
/// host-only value.
pub(super) const FAMILY_ATOMIC: &str = "atomic";

/// Family label of the unbounded integer `inf`: host-only (ruling 39(b)).
pub(super) const FAMILY_INF: &str = "inf";

/// First host-only family found inside `ty`, or `None` when the whole type is
/// legal in a kernel.
pub(super) fn host_only_family(
    ty: TypeId,
    types: &TypeTable,
    facts: &ModuleFacts,
) -> Option<&'static str> {
    Walk {
        types,
        facts,
        seen_types: FxHashSet::default(),
        seen_defs: FxHashSet::default(),
    }
    .family(ty)
}

struct Walk<'a> {
    types: &'a TypeTable,
    facts: &'a ModuleFacts,
    seen_types: FxHashSet<TypeId>,
    seen_defs: FxHashSet<DefId>,
}

impl Walk<'_> {
    fn family(&mut self, ty: TypeId) -> Option<&'static str> {
        if !self.seen_types.insert(ty) {
            return None;
        }
        match self.types.get(ty) {
            Type::Primitive(primitive) => primitive_family(*primitive),
            Type::Array(_) => Some("lista"),
            Type::Queue(_) => Some("queue"),
            Type::Stack(_) => Some("stack"),
            Type::Map(..) => Some("tabula"),
            Type::Set(_) => Some("copia"),
            Type::Promissum(_) | Type::PromissumFailable(..) => Some("promissum"),
            Type::Cursor(_) | Type::AsyncCursor(..) => Some("cursor"),
            Type::Atomic(_) => Some(FAMILY_ATOMIC),
            Type::SizedInstans(_) => Some("instans"),
            Type::Interface(_) => Some("pactum"),
            Type::BoundedArray { element, .. }
            | Type::BoundedQueue { element, .. }
            | Type::BoundedStack { element, .. }
            | Type::Tensor(element, _)
            | Type::Vector(element, _)
            | Type::Matrix(element, _)
            | Type::Sparsa(element, _)
            | Type::Intervallum(element)
            | Type::Option(element)
            | Type::Ref(_, element)
            | Type::Alias(_, element) => self.family(*element),
            Type::Struct(def) => self.nominal(*def, false),
            Type::Enum(def) => self.nominal(*def, true),
            Type::Applied(base, args) => self.family(*base).or_else(|| {
                args.iter().find_map(|arg| match arg {
                    radix_types::AppliedArg::Type(inner) => self.family(*inner),
                    radix_types::AppliedArg::Size(_) => None,
                })
            }),
            // Ruling 39(b): the unbounded integer is a heap value with no
            // device layout, so it is host-only, named by its marker. Every
            // other width stays legal here (per-target requirement inference
            // owns the width questions).
            Type::SizedNumeric(_, width) if width.is_unbounded() => Some(FAMILY_INF),
            Type::Union(members) | Type::Intersection(members) | Type::Tuple(members) => {
                members.iter().find_map(|member| self.family(*member))
            }
            Type::Record(fields) => fields.values().find_map(|field| self.family(*field)),
            // Bounded buffers have a static capacity; closures are the
            // compiler's inlining question; the rest carry no host value.
            Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. }
            | Type::BoundedOcteti { .. }
            | Type::SizedNumeric(..)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::Func(_)
            | Type::Param(_)
            | Type::Infer(_)
            | Type::InferUnion(_)
            | Type::MarkerHole { .. }
            | Type::Error => None,
        }
    }

    /// A nominal type is legal when every member is legal. Imported nominals
    /// have no local member table and stay admitted (the exporter checked
    /// them where they are declared).
    fn nominal(&mut self, def: DefId, is_enum: bool) -> Option<&'static str> {
        if !self.seen_defs.insert(def) {
            return None;
        }
        let members = if is_enum {
            self.facts.enum_fields.get(&def)
        } else {
            self.facts.struct_fields.get(&def)
        }?;
        members.iter().find_map(|member| self.family(*member))
    }
}

fn primitive_family(primitive: Primitive) -> Option<&'static str> {
    match primitive {
        Primitive::Ascii => Some("ascii"),
        Primitive::Textus => Some("textus"),
        Primitive::Octeti => Some("octeti"),
        Primitive::Regex => Some("regex"),
        Primitive::Json => Some("json"),
        Primitive::Valor => Some("valor"),
        Primitive::Ignotum => Some("ignotum"),
        Primitive::Instans => Some("instans"),
        Primitive::Littera => Some("littera"),
        Primitive::Numerus
        | Primitive::Fractus
        | Primitive::Bivalens
        | Primitive::Nihil
        | Primitive::Vacuum
        | Primitive::Numquam => None,
    }
}
