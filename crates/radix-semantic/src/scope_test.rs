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
