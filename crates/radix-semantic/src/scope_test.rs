//! Tests for the Resolver scope system.
//!
//! These tests prove that Symbol definition, name lookup, duplicate
//! rejection, and scope nesting behave correctly in the resolver.

use super::{Resolver, ScopeKind, Symbol, SymbolKind};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol as LexSymbol};

fn symbol(def_id: DefId, name: LexSymbol, kind: SymbolKind) -> Symbol {
    Symbol {
        def_id,
        name,
        kind,
        ty: None,
        mutable: false,
        span: Span::default(),
    }
}

#[test]
fn rejects_duplicate_names_in_same_scope() {
    let mut resolver = Resolver::new();
    let name = LexSymbol(1);
    let first = resolver.fresh_def_id();
    let second = resolver.fresh_def_id();

    assert_eq!(
        resolver.define(symbol(first, name, SymbolKind::Local)),
        Ok(first)
    );
    assert_eq!(
        resolver.define(symbol(second, name, SymbolKind::Param)),
        Err(name)
    );
    assert_eq!(resolver.lookup(name), Some(first));
    assert!(resolver.get_symbol(second).is_none());
}

#[test]
fn lookup_uses_nearest_shadowing_scope() {
    let mut resolver = Resolver::new();
    let name = LexSymbol(2);
    let outer = resolver.fresh_def_id();
    assert_eq!(
        resolver.define(symbol(outer, name, SymbolKind::Local)),
        Ok(outer)
    );

    resolver.enter_scope(ScopeKind::Block);
    let inner = resolver.fresh_def_id();
    assert_eq!(
        resolver.define(symbol(inner, name, SymbolKind::Param)),
        Ok(inner)
    );

    assert_eq!(resolver.lookup(name), Some(inner));
    resolver.exit_scope();
    assert_eq!(resolver.lookup(name), Some(outer));
}

#[test]
fn lookup_type_skips_value_shadowing() {
    let mut resolver = Resolver::new();
    let name = LexSymbol(3);
    let type_def = resolver.fresh_def_id();
    assert_eq!(
        resolver.define(symbol(type_def, name, SymbolKind::Struct)),
        Ok(type_def)
    );

    resolver.enter_scope(ScopeKind::Block);
    let value_def = resolver.fresh_def_id();
    assert_eq!(
        resolver.define(symbol(value_def, name, SymbolKind::Local)),
        Ok(value_def)
    );

    assert_eq!(resolver.lookup(name), Some(value_def));
    assert_eq!(resolver.lookup_type(name), Some(type_def));
}

#[test]
fn loop_context_does_not_cross_function_boundary() {
    let mut resolver = Resolver::new();

    resolver.enter_scope(ScopeKind::Loop);
    assert!(resolver.in_loop());

    resolver.enter_scope(ScopeKind::Function);
    assert!(resolver.in_function());
    assert!(!resolver.in_loop());
}

#[test]
fn exit_scope_keeps_global_scope_current() {
    let mut resolver = Resolver::new();

    resolver.exit_scope();

    assert_eq!(resolver.current().kind, ScopeKind::Global);
}

#[test]
fn lookup_returns_none_for_undefined_name() {
    let resolver = Resolver::new();
    let name = LexSymbol(999);

    assert_eq!(resolver.lookup(name), None);
    assert_eq!(resolver.lookup_type(name), None);
}

#[test]
fn lookup_in_empty_child_scope_falls_through_to_global() {
    let mut resolver = Resolver::new();
    let name = LexSymbol(10);
    let outer = resolver.fresh_def_id();
    assert_eq!(
        resolver.define(symbol(outer, name, SymbolKind::Local)),
        Ok(outer)
    );

    resolver.enter_scope(ScopeKind::Block);
    // Should still find the outer definition.
    assert_eq!(resolver.lookup(name), Some(outer));
    resolver.exit_scope();
}

#[test]
fn define_variant_scopes_identity_by_parent_and_keeps_bare_name_free() {
    let mut resolver = Resolver::new();
    let union_a = resolver.fresh_def_id();
    let union_b = resolver.fresh_def_id();
    let x_name = LexSymbol(100);

    let a_x = resolver
        .define_variant(union_a, x_name, Span::default())
        .expect("A.X registers");
    let b_x = resolver
        .define_variant(union_b, x_name, Span::default())
        .expect("B.X registers");

    // Parent-qualified identity: distinct DefIds under distinct parents.
    assert_ne!(a_x, b_x);
    assert_eq!(resolver.variant_child(union_a, x_name), Some(a_x));
    assert_eq!(resolver.variant_child(union_b, x_name), Some(b_x));

    // The bare name is NOT entered into the scope table.
    assert_eq!(resolver.lookup(x_name), None);

    // Bare resolution through the per-union namespace sees the ambiguity.
    assert_eq!(
        resolver.lookup_variant_bare(x_name),
        super::BareVariantLookup::Ambiguous { defining_unions: 2 }
    );
}

#[test]
fn define_variant_rejects_same_parent_duplicate() {
    let mut resolver = Resolver::new();
    let union = resolver.fresh_def_id();
    let x_name = LexSymbol(101);

    let first = resolver
        .define_variant(union, x_name, Span::default())
        .expect("first registers");
    assert!(resolver
        .define_variant(union, x_name, Span::default())
        .is_none());
    assert_eq!(resolver.variant_child(union, x_name), Some(first));
}

#[test]
fn lookup_variant_bare_is_single_for_one_defining_union() {
    let mut resolver = Resolver::new();
    let union = resolver.fresh_def_id();
    let x_name = LexSymbol(102);

    let x = resolver
        .define_variant(union, x_name, Span::default())
        .expect("registers");
    assert_eq!(
        resolver.lookup_variant_bare(x_name),
        super::BareVariantLookup::Single(x)
    );

    let missing = LexSymbol(103);
    assert_eq!(
        resolver.lookup_variant_bare(missing),
        super::BareVariantLookup::None
    );
}

#[cfg(feature = "hir-fhir")]
#[test]
fn resolver_snapshot_round_trips_conversion_arms() {
    use super::ConversionArm;
    use radix_types::TypeId;

    let mut resolver = Resolver::new();
    resolver.register_conversion_arms(
        DefId(700),
        vec![ConversionArm {
            source: TypeId(17),
            fn_def_id: DefId(900),
            span: Span::default(),
        }],
    );

    let restored = Resolver::from_snapshot(resolver.to_snapshot());
    assert_eq!(
        restored.conversion_arm_for(DefId(700), TypeId(17)),
        Some(DefId(900))
    );
}

#[test]
fn conversion_registry_holds_builtin_and_user_rows_by_pair_identity() {
    use crate::scope::{ConversionPair, ConversionRow, ScopeId};
    use radix_types::TypeId;

    let mut resolver = Resolver::new();
    let pair = ConversionPair::new(TypeId(3), TypeId(9));
    resolver
        .register_builtin_conversion(pair, Span::new(1, 5))
        .expect("builtin row enrolls");
    let user_fn = resolver.fresh_def_id();
    let other = ConversionPair::new(TypeId(4), TypeId(9));
    resolver
        .register_user_conversion(other, user_fn, Span::new(6, 10))
        .expect("user row enrolls");

    let builtin = resolver.conversion_rows_for(pair);
    assert_eq!(builtin.len(), 1);
    assert!(builtin[0].builtin);
    assert_eq!(builtin[0].fn_def_id, None);
    assert_eq!(builtin[0].scope, ScopeId(0));

    let user = resolver.conversion_rows_for(other);
    assert_eq!(user.len(), 1);
    assert!(
        matches!(user[0], ConversionRow { builtin: false, fn_def_id: Some(id), .. } if id == user_fn)
    );
}

#[test]
fn conversion_registry_rejects_duplicate_pair_in_one_scope_with_both_spans() {
    use crate::scope::{ConversionPair, ConversionRowConflict};
    use radix_types::TypeId;

    let mut resolver = Resolver::new();
    let pair = ConversionPair::new(TypeId(3), TypeId(9));
    let first_fn = resolver.fresh_def_id();
    resolver
        .register_user_conversion(pair, first_fn, Span::new(1, 5))
        .expect("first row enrolls");
    let second_fn = resolver.fresh_def_id();
    let err = resolver
        .register_user_conversion(pair, second_fn, Span::new(20, 24))
        .expect_err("duplicate pair rejected");
    assert_eq!(
        err,
        ConversionRowConflict::Duplicate {
            first_span: Span::new(1, 5),
            second_span: Span::new(20, 24),
        }
    );
    assert_eq!(resolver.conversion_rows_for(pair).len(), 1);
}

#[test]
fn conversion_registry_rejects_builtin_row_shadowing_from_any_scope() {
    use crate::scope::{ConversionPair, ConversionRowConflict};
    use radix_types::TypeId;

    let mut resolver = Resolver::new();
    let pair = ConversionPair::new(TypeId(3), TypeId(9));
    resolver
        .register_builtin_conversion(pair, Span::new(2, 6))
        .expect("builtin row enrolls");

    resolver.enter_scope(ScopeKind::Function);
    let shadow_fn = resolver.fresh_def_id();
    let err = resolver
        .register_user_conversion(pair, shadow_fn, Span::new(30, 34))
        .expect_err("user row may not shadow a builtin");
    assert_eq!(
        err,
        ConversionRowConflict::BuiltinShadow {
            builtin_span: Span::new(2, 6),
            user_span: Span::new(30, 34),
        }
    );
    assert_eq!(resolver.conversion_rows_for(pair).len(), 1);
}

#[test]
fn conversion_registry_allows_same_pair_in_disjoint_scopes_and_lookup_is_scope_aware() {
    use crate::scope::ConversionPair;
    use radix_types::TypeId;

    let mut resolver = Resolver::new();
    let pair = ConversionPair::new(TypeId(3), TypeId(9));
    let global_fn = resolver.fresh_def_id();
    resolver
        .register_user_conversion(pair, global_fn, Span::new(1, 5))
        .expect("global row enrolls");

    resolver.enter_scope(ScopeKind::Function);
    let inner_fn = resolver.fresh_def_id();
    resolver
        .register_user_conversion(pair, inner_fn, Span::new(10, 14))
        .expect("inner scope owns its own row");

    let inner = resolver.conversion_row_for(pair.source, pair.destination);
    assert_eq!(inner.map(|row| row.fn_def_id), Some(Some(inner_fn)));

    resolver.exit_scope();
    let outer = resolver.conversion_row_for(pair.source, pair.destination);
    assert_eq!(outer.map(|row| row.fn_def_id), Some(Some(global_fn)));
}

#[test]
fn conversion_registry_pair_identity_is_ordered() {
    use crate::scope::ConversionPair;
    use radix_types::TypeId;

    let forward = ConversionPair::new(TypeId(3), TypeId(9));
    let backward = ConversionPair::new(TypeId(9), TypeId(3));
    assert_ne!(forward, backward);
}

#[test]
fn conversion_registry_rejects_duplicate_builtin_pair() {
    use crate::scope::{ConversionPair, ConversionRowConflict};
    use radix_types::TypeId;

    let mut resolver = Resolver::new();
    let pair = ConversionPair::new(TypeId(3), TypeId(9));
    resolver
        .register_builtin_conversion(pair, Span::new(1, 5))
        .expect("first builtin enrolls");
    assert_eq!(
        resolver.register_builtin_conversion(pair, Span::new(9, 13)),
        Err(ConversionRowConflict::Duplicate {
            first_span: Span::new(1, 5),
            second_span: Span::new(9, 13),
        })
    );
}
