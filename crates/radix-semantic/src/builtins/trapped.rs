//! Builtin compiler-owned generic genus `trapped<R, T>` (two variants,
//! `returned { value: R }` and `thrown { error: T }`).
//!
//! The carrier of the `trap { … }` outcome: the `returned` variant holds the
//! value the block produced, the `thrown` variant holds the error it raised.
//! Registered in the resolver like `erratum`; the HIR enum is injected when
//! the program applies it. Variant names are contextual per union and are NOT
//! reserved identifiers elsewhere. Reader spellings come from the locale
//! packs; this module owns the canonical name.

use crate::{Resolver, SymbolKind, Type, TypeId, TypeTable};
use radix_hir::{
    DefId, HirEnum, HirId, HirItem, HirItemKind, HirTypeParam, HirTypeParamConstraint, HirVariant,
    HirVariantField,
};
use radix_lexer::{Interner, Span};

const TRAPPED_NAME: &str = "trapped";
const RETURNED_VARIANT: &str = "returned";
const THROWN_VARIANT: &str = "thrown";
const VALUE_FIELD: &str = "value";
const ERROR_FIELD: &str = "error";
const RESULT_PARAM: &str = "R";
const ERROR_PARAM: &str = "T";

/// Register the builtin `trapped` genus in the resolver.
///
/// `trapped` is reserved like `erratum`: user source cannot shadow it. The def
/// id comes from the builtin low range, so user declaration ids stay stable.
/// The variant names live only in the per-union namespace
/// (`define_builtin_variant`), never as bare resolver symbols.
pub fn register(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    let name = interner.intern(TRAPPED_NAME);
    if resolver.lookup_type(name).is_some() {
        return;
    }

    let def_id = resolver.fresh_builtin_def_id();
    resolver.register_builtin_trapped_def_id(def_id);
    let defined = resolver.define(crate::Symbol {
        def_id,
        name,
        kind: SymbolKind::Enum,
        ty: Some(types.intern(Type::Enum(def_id))),
        mutable: false,
        span: Span::default(),
    });
    debug_assert!(defined.is_ok(), "builtin trapped registers exactly once");
    for variant in [RETURNED_VARIANT, THROWN_VARIANT] {
        let defined =
            resolver.define_builtin_variant(def_id, interner.intern(variant), Span::default());
        debug_assert!(
            defined.is_some(),
            "builtin trapped defines each variant exactly once"
        );
    }
}

/// Lookup the builtin `trapped` definition id.
#[must_use]
pub fn def_id(resolver: &Resolver, interner: &mut Interner) -> Option<DefId> {
    if let Some(def_id) = resolver.builtin_trapped_def_id() {
        return Some(def_id);
    }
    let name = interner.intern(TRAPPED_NAME);
    resolver
        .lookup_type(name)
        .filter(|def_id| def_id.is_builtin())
}

/// The `trapped<R, T>` application for one capture boundary: `R` is the
/// body's success type, `T` the solved error channel.
#[must_use]
pub fn applied_type(
    types: &mut TypeTable,
    def_id: DefId,
    success: TypeId,
    error: TypeId,
) -> TypeId {
    let base = types.intern(Type::Enum(def_id));
    types.intern(Type::Applied(
        base,
        vec![
            radix_types::AppliedArg::Type(success),
            radix_types::AppliedArg::Type(error),
        ],
    ))
}

/// Return whether any type applied the builtin `trapped` genus.
///
/// WHY: lowering interns `trapped<R, T>` as `Applied(Enum(def), [R, T])`, so
/// the table already answers "does the program name it" without a second
/// syntax walk that could disagree with type lowering about what a type
/// position is. A `capta { ... }` boundary applies the genus implicitly
/// through its re-typed result, so its lowering marks the resolver flag
/// beside the table scan.
fn is_applied(types: &TypeTable, def_id: DefId, resolver: &Resolver) -> bool {
    resolver.builtin_trapped_applied()
        || (0..types.type_count())
            .filter_map(|index| u32::try_from(index).ok())
            .any(|index| {
                let Type::Applied(base, _) = types.get(TypeId(index)) else {
                    return false;
                };
                matches!(types.get(*base), Type::Enum(found) if *found == def_id)
            })
}

/// Append the builtin `trapped<R, T>` enum item to lowered HIR when a type
/// applies it.
pub fn inject_hir_item(
    items: &mut Vec<HirItem>,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) {
    let Some(def_id) = def_id(resolver, interner) else {
        return;
    };
    if !is_applied(types, def_id, resolver) || items.iter().any(|item| item.def_id == def_id) {
        return;
    }

    let result_param = interner.intern(RESULT_PARAM);
    let error_param = interner.intern(ERROR_PARAM);
    let value = HirVariantField {
        name: interner.intern(VALUE_FIELD),
        ty: types.intern(Type::Param(result_param)),
        span: Span::default(),
    };
    let error = HirVariantField {
        name: interner.intern(ERROR_FIELD),
        ty: types.intern(Type::Param(error_param)),
        span: Span::default(),
    };
    let (Some(returned_def), Some(thrown_def)) = (
        resolver.variant_child(def_id, interner.intern(RETURNED_VARIANT)),
        resolver.variant_child(def_id, interner.intern(THROWN_VARIANT)),
    ) else {
        return;
    };

    items.push(HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id,
        kind: HirItemKind::Enum(HirEnum {
            name: interner.intern(TRAPPED_NAME),
            type_params: vec![
                HirTypeParam {
                    def_id: resolver.fresh_def_id(),
                    kind: radix_syntax::GenericParamKind::Typus,
                    name: result_param,
                    span: Span::default(),
                    constraint: HirTypeParamConstraint::Any,
                    default: None,
                },
                HirTypeParam {
                    def_id: resolver.fresh_def_id(),
                    kind: radix_syntax::GenericParamKind::Typus,
                    name: error_param,
                    span: Span::default(),
                    constraint: HirTypeParamConstraint::Any,
                    default: None,
                },
            ],
            commune_fields: Vec::new(),
            variants: vec![
                HirVariant {
                    def_id: returned_def,
                    name: interner.intern(RETURNED_VARIANT),
                    fields: vec![value],
                    span: Span::default(),
                },
                HirVariant {
                    def_id: thrown_def,
                    name: interner.intern(THROWN_VARIANT),
                    fields: vec![error],
                    span: Span::default(),
                },
            ],
        }),
        span: Span::default(),
    });
}

#[cfg(test)]
#[path = "trapped_test.rs"]
mod trapped_test;
