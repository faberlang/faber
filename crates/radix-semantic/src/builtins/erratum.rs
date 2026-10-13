//! Builtin compiler-owned generic genus `erratum<T>` (one field, `payload: T`).
//!
//! The carrier of an error channel: `⇥ T` names the payload type and `catch`
//! binds the payload, so `erratum<T>` is the type of what travels, visible
//! where types are written. Registered in the resolver like `forma`; the HIR
//! struct is injected like `scrinium<T>` when the program applies it. Reader
//! spellings come from the locale packs; this module owns the canonical name.

use crate::{Resolver, SymbolKind, Type, TypeId, TypeTable};
use radix_hir::{
    DefId, HirField, HirId, HirItem, HirItemKind, HirStruct, HirTypeParam, HirTypeParamConstraint,
};
use radix_lexer::{Interner, Span};

const ERRATUM_NAME: &str = "erratum";
const PAYLOAD_FIELD: &str = "payload";
const PAYLOAD_PARAM: &str = "T";

/// Register the builtin `erratum` genus in the resolver.
///
/// `erratum` is reserved like `forma`: user source cannot shadow it. The def id
/// comes from the builtin low range, so user declaration ids stay stable.
pub fn register(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    let name = interner.intern(ERRATUM_NAME);
    if resolver.lookup_type(name).is_some() {
        return;
    }

    let def_id = resolver.fresh_builtin_def_id();
    let defined = resolver.define(crate::Symbol {
        def_id,
        name,
        kind: SymbolKind::Struct,
        ty: Some(types.intern(Type::Struct(def_id))),
        mutable: false,
        span: Span::default(),
    });
    debug_assert!(defined.is_ok(), "builtin erratum registers exactly once");
}

/// Lookup the builtin `erratum` definition id.
#[must_use]
pub fn def_id(resolver: &Resolver, interner: &mut Interner) -> Option<DefId> {
    let name = interner.intern(ERRATUM_NAME);
    resolver
        .lookup_type(name)
        .filter(|def_id| def_id.is_builtin())
}

/// Return whether any type applied the builtin `erratum` genus.
///
/// WHY: lowering interns `erratum<T>` as `Applied(Struct(def), [T])`, so the
/// table already answers "does the program name it" without a second syntax
/// walk that could disagree with type lowering about what a type position is.
fn is_applied(types: &TypeTable, def_id: DefId) -> bool {
    (0..types.type_count()).any(|index| {
        let Type::Applied(base, _) = types.get(TypeId(
            u32::try_from(index).expect("type table index fits u32"),
        )) else {
            return false;
        };
        matches!(types.get(*base), Type::Struct(found) if *found == def_id)
    })
}

/// Append the builtin `erratum<T>` struct item to lowered HIR when a type
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
    if !is_applied(types, def_id) || items.iter().any(|item| item.def_id == def_id) {
        return;
    }

    let payload_param = interner.intern(PAYLOAD_PARAM);
    let payload_ty = types.intern(Type::Param(payload_param));
    let payload = HirField {
        def_id: resolver.fresh_def_id(),
        name: interner.intern(PAYLOAD_FIELD),
        ty: payload_ty,
        sponte: false,
        modifier: None,
        annotations: Vec::new(),
        init: None,
        span: Span::default(),
    };

    items.push(HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id,
        kind: HirItemKind::Struct(HirStruct {
            name: interner.intern(ERRATUM_NAME),
            type_params: vec![HirTypeParam {
                def_id: resolver.fresh_def_id(),
                kind: radix_syntax::GenericParamKind::Typus,
                name: payload_param,
                span: Span::default(),
                constraint: HirTypeParamConstraint::Any,
                default: None,
            }],
            fields: vec![payload],
            methods: Vec::new(),
            implements: Vec::new(),
        }),
        span: Span::default(),
    });
}

#[cfg(test)]
#[path = "erratum_test.rs"]
mod erratum_test;
