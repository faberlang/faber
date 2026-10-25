//! Pattern-matrix coverage for `discerne` (D4.8).
//!
//! Coverage is the standard usefulness question (Maranget, "Warnings for
//! pattern matching", JFP 2007): is the all-wildcard row useful against the
//! match's rows? If it is, the recursion yields a witness — one uncovered
//! value per scrutinee — which the exhaustiveness pass names in its error.
//!
//! Each scrutinee column is a value space:
//!
//! - an enum (`discretio`/`ordo`) is the closed set of its variants;
//! - `bivalens` is the closed set `{verum, falsum}`;
//! - a union (`A ∪ B`, `T ∪ nihil`) is the closed set of its members, covered
//!   by type-test patterns (including width families) and `casu nihil`;
//! - any other type is open: literal and partial type-test patterns never
//!   complete it, so only a wildcard (`_`, a binding, `ceterum`) does.
//!
//! A pattern whose kind does not fit its column (a payload subpattern that is
//! not a binding, a literal in an enum column, a variant in a union column,
//! an enum whose variant list is unknown) makes coverage uncomputable. The
//! caller reports that instead of guessing — never silence.

use crate::{Primitive, Type, TypeId, TypeTable};
use handroll::FxHashMap;
use radix_hir::{DefId, HirCasuArm, HirExpression, HirLiteral, HirPattern};

/// One constructor of a closed column space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ctor {
    Variant(DefId),
    Bool(bool),
    Member(TypeId),
}

/// One uncovered value per scrutinee; `None` is "any other value" (`_`).
pub(crate) type Witness = Vec<Option<Ctor>>;

/// The value space a scrutinee column ranges over.
#[derive(Debug, Clone)]
pub(crate) enum Space {
    Enum(Vec<DefId>),
    Bool,
    Union(Vec<TypeId>),
    Open,
    /// An enum whose variant list is not visible: only a catchall covers it,
    /// and a variant head makes coverage uncomputable.
    UnknownEnum,
    /// Untyped scrutinee: typecheck owns the diagnostic.
    Unknown,
}

/// Outcome of a coverage computation.
#[derive(Debug, Clone)]
pub(crate) enum Coverage {
    Covered,
    Missing(Witness),
    /// A pattern kind the space cannot be computed for.
    Unchecked,
    /// An upstream pass owns the problem (untyped scrutinee, pattern arity).
    Skipped,
}

/// A normalized row head.
#[derive(Debug, Clone, Copy)]
enum Head {
    Wild,
    Variant(DefId),
    Bool(bool),
    TypeTest(TypeId),
    Nil,
    /// A non-bool, non-nil literal: one value of an open space.
    Value,
}

/// Compute coverage of `arms` over `scrutinees`.
pub(crate) fn coverage(
    scrutinees: &[HirExpression],
    arms: &[HirCasuArm],
    types: &TypeTable,
    enum_variants: &FxHashMap<DefId, Vec<DefId>>,
) -> Coverage {
    let columns: Vec<(Space, Option<TypeId>)> = scrutinees
        .iter()
        .map(|scrutinee| match scrutinee.ty {
            Some(ty) => (space_for(ty, types, enum_variants), Some(ty)),
            None => (Space::Unknown, None),
        })
        .collect();

    if arms.iter().any(|arm| arm.patterns.len() != columns.len()) {
        return Coverage::Skipped;
    }
    // An all-catchall row covers every value, whatever the other rows hold.
    if arms.iter().any(|arm| arm.patterns.iter().all(is_catchall)) {
        return Coverage::Covered;
    }

    let mut rows = Vec::with_capacity(arms.len());
    for arm in arms {
        // An Or is one source arm but several coverage rows: each alternative
        // independently covers its constructor while sharing the arm body.
        let mut arm_rows = vec![Vec::with_capacity(columns.len())];
        for (pattern, (space, ty)) in arm.patterns.iter().zip(&columns) {
            let alternatives = pattern_alternatives(pattern);
            let mut next_rows = Vec::with_capacity(arm_rows.len() * alternatives.len());
            for row in arm_rows {
                for alternative in &alternatives {
                    let Some(head) = normalize(alternative, space, *ty, types) else {
                        return Coverage::Unchecked;
                    };
                    match admit(head, space) {
                        Admit::Yes => {
                            let mut row = row.clone();
                            row.push(head);
                            next_rows.push(row);
                        }
                        Admit::Unchecked => return Coverage::Unchecked,
                        Admit::Skipped => return Coverage::Skipped,
                    }
                }
            }
            arm_rows = next_rows;
        }
        rows.extend(arm_rows);
    }

    let spaces: Vec<Space> = columns.into_iter().map(|(space, _)| space).collect();
    match missing(&rows, &spaces, types) {
        Some(witness) => Coverage::Missing(witness),
        None => Coverage::Covered,
    }
}

/// Classify a scrutinee type into its value space.
pub(crate) fn space_for(
    ty: TypeId,
    types: &TypeTable,
    enum_variants: &FxHashMap<DefId, Vec<DefId>>,
) -> Space {
    if let Some(enum_def) = super::exhaustive::enum_def_from_type(ty, types) {
        return match enum_variants.get(&enum_def) {
            Some(variants) => Space::Enum(variants.clone()),
            // A variant list we cannot see is not a closed set we can check.
            None => Space::UnknownEnum,
        };
    }
    if is_bivalens(ty, types) {
        return Space::Bool;
    }
    if let Some(members) = super::exhaustive::union_members_from_type(ty, types) {
        return Space::Union(members);
    }
    Space::Open
}

impl Space {
    fn ctors(&self) -> Option<Vec<Ctor>> {
        match self {
            Space::Enum(variants) => Some(variants.iter().copied().map(Ctor::Variant).collect()),
            Space::Bool => Some(vec![Ctor::Bool(true), Ctor::Bool(false)]),
            Space::Union(members) => Some(members.iter().copied().map(Ctor::Member).collect()),
            Space::Open | Space::UnknownEnum | Space::Unknown => None,
        }
    }
}

fn is_bivalens(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Primitive(Primitive::Bivalens) => true,
        Type::Alias(_, inner) | Type::Ref(_, inner) => is_bivalens(*inner, types),
        _ => false,
    }
}

fn is_catchall(pattern: &HirPattern) -> bool {
    match pattern {
        HirPattern::Wildcard | HirPattern::Binding(_, _) => true,
        HirPattern::Alias(_, _, inner) => is_catchall(inner),
        HirPattern::Or(alternatives) => alternatives.iter().any(is_catchall),
        HirPattern::Variant(_, _)
        | HirPattern::TypeTest { .. }
        | HirPattern::Literal(_)
        | HirPattern::NegatedLiteral(_) => false,
    }
}

/// Reduce a pattern to its coverage head. `None` means the pattern's shape
/// cannot be reasoned about (a payload subpattern that is not a binding).
fn normalize(
    pattern: &HirPattern,
    space: &Space,
    column_ty: Option<TypeId>,
    types: &TypeTable,
) -> Option<Head> {
    Some(match pattern {
        HirPattern::Wildcard | HirPattern::Binding(_, _) => Head::Wild,
        HirPattern::Alias(_, _, inner) => return normalize(inner, space, column_ty, types),
        HirPattern::Or(_) => return None,
        HirPattern::Variant(def_id, payload) => {
            if !payload.iter().all(is_catchall) {
                return None;
            }
            Head::Variant(*def_id)
        }
        HirPattern::TypeTest { ty, candidate, .. } => match (space, candidate) {
            (Space::Enum(_), Some(def_id)) => Head::Variant(*def_id),
            _ if column_ty.is_some_and(|column| types.equals(*ty, column)) => Head::Wild,
            _ => Head::TypeTest(*ty),
        },
        HirPattern::Literal(HirLiteral::Bool(value)) => Head::Bool(*value),
        HirPattern::Literal(HirLiteral::Nil) => Head::Nil,
        // A signed number literal is one value of an open space.
        HirPattern::Literal(_) | HirPattern::NegatedLiteral(_) => Head::Value,
    })
}

enum Admit {
    Yes,
    Unchecked,
    Skipped,
}

/// Whether a head is a pattern kind its column's space can be computed for.
fn admit(head: Head, space: &Space) -> Admit {
    let fits = match (space, head) {
        (_, Head::Wild)
        | (Space::Bool, Head::Bool(_))
        | (Space::Union(_), Head::TypeTest(_) | Head::Nil)
        | (Space::Open, Head::Value | Head::TypeTest(_) | Head::Nil) => true,
        (Space::Enum(variants), Head::Variant(def_id)) => variants.contains(&def_id),
        (Space::Unknown, _) => return Admit::Skipped,
        _ => false,
    };
    if fits { Admit::Yes } else { Admit::Unchecked }
}

fn is_wild(head: Head) -> bool {
    matches!(head, Head::Wild)
}

/// Whether a (non-wildcard or wildcard) head matches every value of `ctor`.
fn covers(head: Head, ctor: Ctor, types: &TypeTable) -> bool {
    match (head, ctor) {
        (Head::Wild, _) => true,
        (Head::Variant(a), Ctor::Variant(b)) => a == b,
        (Head::Bool(a), Ctor::Bool(b)) => a == b,
        (Head::TypeTest(ty), Ctor::Member(member)) => types.equals(ty, member),
        (Head::Nil, Ctor::Member(member)) => {
            matches!(types.get(member), Type::Primitive(Primitive::Nihil))
        }
        _ => false,
    }
}

/// Maranget's usefulness of the all-wildcard vector, returning a witness.
///
/// When the first column's heads name every constructor of a closed space,
/// the matrix is specialized per constructor; otherwise the default matrix
/// (wildcard rows only) decides, and the witness names the first constructor
/// no head covers. An open space's witness is `_` ("any other value").
fn missing(rows: &[Vec<Head>], spaces: &[Space], types: &TypeTable) -> Option<Witness> {
    let Some((space, rest)) = spaces.split_first() else {
        return rows.is_empty().then(Vec::new);
    };

    let default = || -> Vec<Vec<Head>> {
        rows.iter()
            .filter(|row| is_wild(row[0]))
            .map(|row| row[1..].to_vec())
            .collect()
    };

    let Some(ctors) = space.ctors() else {
        let mut witness = missing(&default(), rest, types)?;
        witness.insert(0, None);
        return Some(witness);
    };

    let named = |ctor: Ctor| {
        rows.iter()
            .any(|row| !is_wild(row[0]) && covers(row[0], ctor, types))
    };
    if let Some(uncovered) = ctors.iter().copied().find(|ctor| !named(*ctor)) {
        let mut witness = missing(&default(), rest, types)?;
        witness.insert(0, Some(uncovered));
        return Some(witness);
    }

    for ctor in ctors {
        let specialized: Vec<Vec<Head>> = rows
            .iter()
            .filter(|row| covers(row[0], ctor, types))
            .map(|row| row[1..].to_vec())
            .collect();
        if let Some(mut witness) = missing(&specialized, rest, types) {
            witness.insert(0, Some(ctor));
            return Some(witness);
        }
    }
    None
}

fn pattern_alternatives(pattern: &HirPattern) -> Vec<&HirPattern> {
    match pattern {
        HirPattern::Or(alternatives) => alternatives.iter().collect(),
        _ => vec![pattern],
    }
}
