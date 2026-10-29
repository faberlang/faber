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

fn linear_variant_child(
    resolver: &Resolver,
    parent_def_id: DefId,
    variant_name: LexSymbol,
) -> Option<DefId> {
    resolver
        .variant_parents_all()
        .into_iter()
        .find_map(|(variant_def_id, candidate_parent)| {
            if candidate_parent != parent_def_id {
                return None;
            }
            let symbol = resolver.get_symbol(variant_def_id)?;
            (symbol.kind == SymbolKind::Variant && symbol.name == variant_name)
                .then_some(variant_def_id)
        })
}

fn linear_lookup_variant_bare(resolver: &Resolver, name: LexSymbol) -> super::BareVariantLookup {
    let mut local_variant: Option<DefId> = None;
    let mut local_unions = 0usize;
    let mut single: Option<DefId> = None;
    let mut defining_unions = 0usize;
    for (variant_def_id, _parent_def_id) in resolver.variant_parents_all() {
        let Some(symbol) = resolver.get_symbol(variant_def_id) else {
            continue;
        };
        if symbol.kind == SymbolKind::Variant && symbol.name == name {
            defining_unions += 1;
            single = Some(variant_def_id);
            local_unions += 1;
            local_variant = Some(variant_def_id);
        }
    }
    if local_unions > 0 {
        return match local_variant {
            Some(def_id) if local_unions == 1 => super::BareVariantLookup::Single(def_id),
            _ => super::BareVariantLookup::Ambiguous {
                defining_unions: local_unions,
            },
        };
    }
    match (defining_unions, single) {
        (0, _) => super::BareVariantLookup::None,
        (1, Some(def_id)) => super::BareVariantLookup::Single(def_id),
        _ => super::BareVariantLookup::Ambiguous { defining_unions },
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
fn exit_scope_restores_shadow_deltas_in_reverse() {
    let mut resolver = Resolver::new();
    let name = LexSymbol(20);
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
    let inner_only_name = LexSymbol(21);
    let inner_only = resolver.fresh_def_id();
    assert_eq!(
        resolver.define(symbol(inner_only, inner_only_name, SymbolKind::Local)),
        Ok(inner_only)
    );
    assert_eq!(resolver.lookup(name), Some(inner));
    assert_eq!(resolver.lookup(inner_only_name), Some(inner_only));
    resolver.exit_scope();
    assert_eq!(resolver.lookup(name), Some(outer));
    assert_eq!(resolver.lookup(inner_only_name), None);

    resolver.enter_scope(ScopeKind::Block);
    let type_def = resolver.fresh_def_id();
    assert_eq!(
        resolver.define(symbol(type_def, name, SymbolKind::Struct)),
        Ok(type_def)
    );
    let module_def = resolver.fresh_def_id();
    assert_eq!(
        resolver.define(symbol(module_def, name, SymbolKind::Module)),
        Ok(module_def)
    );
    assert_eq!(resolver.lookup(name), Some(module_def));
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
    assert!(
        resolver
            .define_variant(union, x_name, Span::default())
            .is_none()
    );
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

#[test]
fn indexed_variant_lookups_match_linear_reference() {
    let mut resolver = Resolver::new();
    let union_a = resolver.fresh_def_id();
    let union_b = resolver.fresh_def_id();
    let union_c = resolver.fresh_def_id();
    let x_name = LexSymbol(104);
    let y_name = LexSymbol(105);

    let a_x = resolver
        .define_variant(union_a, x_name, Span::default())
        .expect("A.X registers");
    let b_x = resolver
        .define_variant(union_b, x_name, Span::default())
        .expect("B.X registers");
    let c_y = resolver
        .define_variant(union_c, y_name, Span::default())
        .expect("C.Y registers");

    for (parent_def_id, variant_name) in [
        (union_a, x_name),
        (union_b, x_name),
        (union_c, y_name),
        (union_a, y_name),
    ] {
        assert_eq!(
            resolver.variant_child(parent_def_id, variant_name),
            linear_variant_child(&resolver, parent_def_id, variant_name),
        );
    }
    for name in [x_name, y_name, LexSymbol(106)] {
        assert_eq!(
            resolver.lookup_variant_bare(name),
            linear_lookup_variant_bare(&resolver, name),
        );
    }
    assert_eq!(resolver.variant_child(union_a, x_name), Some(a_x));
    assert_eq!(resolver.variant_child(union_b, x_name), Some(b_x));
    assert_eq!(resolver.variant_child(union_c, y_name), Some(c_y));
}

/// Producer-shaped interface whose exported enum carries one well-formed
/// variant and one whose payload field names a nominal the consumer cannot
/// resolve. The second variant's row stays in the enum's table but is flagged
/// `payload_unresolved` by the phase-2 remap.
fn imported_enum_with_unresolved_payload() -> crate::file_interface::FileInterface {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceNominalExport,
        InterfaceNominalKind, InterfaceNominalType, InterfaceStructField, InterfaceTypeSnapshot,
    };

    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Kind".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            name: "Kind".to_owned(),
            params: Vec::new(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![
                InterfaceEnumVariant {
                    name: "Fine".to_owned(),
                    fields: Vec::new(),
                },
                InterfaceEnumVariant {
                    name: "Broken".to_owned(),
                    fields: vec![InterfaceStructField {
                        name: "payload".to_owned(),
                        ty: InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
                            kind: InterfaceNominalKind::Struct,
                            name: "Missing".to_owned(),
                            identity: None,
                            kind_guessed: false,
                        }),
                        optional: false,
                        required: true,
                        default: None,
                        modifier: None,
                        visibility: radix_syntax::MemberVisibility::Publica,
                    }],
                },
            ],
        }),
    });
    interface
}

#[test]
fn namespace_variant_value_position_flagged_row_is_pinned() {
    use crate::TypeTable;
    use std::collections::BTreeMap;

    let mut resolver = Resolver::new();
    let mut interner = radix_lexer::Interner::new();
    let mut types = TypeTable::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("mod".to_owned(), imported_enum_with_unresolved_payload())]),
            &mut interner,
            &mut types,
        )
        .expect("enum interface installs");

    let namespace = interner.intern("mod");
    let type_name = interner.intern("Kind");
    let fine = interner.intern("Fine");
    let broken = interner.intern("Broken");

    // The flagged row stays in the variant parent table (so the commune
    // projection can still arm it) but its payload identity is unknown.
    let enum_def = resolver
        .variant_parents_all()
        .into_iter()
        .find_map(|(variant_def, parent_def)| {
            let symbol = resolver.get_symbol(variant_def)?;
            (symbol.name == fine).then_some(parent_def)
        })
        .expect("the enum's parent link survives install");
    let broken_def = resolver
        .variant_parents_all()
        .into_iter()
        .find_map(|(variant_def, parent_def)| {
            let symbol = resolver.get_symbol(variant_def)?;
            (parent_def == enum_def && symbol.name == broken).then_some(variant_def)
        })
        .expect("the flagged row is not dropped from the variant table");
    let fine_def = resolver
        .variant_child(enum_def, fine)
        .expect("the well-formed variant resolves");

    // Fail-closed pin: the flagged row must not resolve in value position on
    // any namespace path, matching `variant_child`.
    assert_eq!(resolver.variant_child(enum_def, broken), None);
    assert_eq!(resolver.namespace_file_variant(namespace, broken), None);
    assert_eq!(
        resolver.namespace_file_type_variant(namespace, type_name, broken),
        None
    );

    // ...while the well-formed sibling still resolves on every path.
    assert_eq!(
        resolver
            .namespace_file_variant(namespace, fine)
            .map(|(def_id, _)| def_id),
        Some(fine_def)
    );
    assert_eq!(
        resolver
            .namespace_file_type_variant(namespace, type_name, fine)
            .map(|(def_id, _)| def_id),
        Some(fine_def)
    );
    let nameable = resolver.non_local_enum_variants();
    let kind = nameable
        .iter()
        .find(|(def_id, _)| *def_id == enum_def)
        .expect("the enum is a non-local enum with variant lists");
    assert!(kind.1.contains(&fine_def));
    assert!(!kind.1.contains(&broken_def));
}

/// A generic struct export installs its ordered declaration parameter names,
/// which the MIR lowering context reads back through `imported_file_structs`
/// to register `struct_type_params` for applied imported nominals.
#[test]
fn imported_generic_struct_carries_decl_param_names() {
    use crate::TypeTable;
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceGenericParam, InterfaceStructExport,
    };
    use std::collections::BTreeMap;

    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Box".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            name: "Box".to_owned(),
            params: vec![InterfaceGenericParam {
                name: "T".to_owned(),
                kind: radix_syntax::GenericParamKind::Typus,
            }],
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });

    let mut resolver = Resolver::new();
    let mut interner = radix_lexer::Interner::new();
    let mut types = TypeTable::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("mod".to_owned(), interface)]),
            &mut interner,
            &mut types,
        )
        .expect("generic struct interface installs");

    let exported = resolver.imported_file_structs();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].3, vec![interner.intern("T")]);
}

#[cfg(feature = "hir-fhir")]
#[test]
fn resolver_snapshot_round_trips_conversion_rows() {
    use crate::scope::ConversionPair;
    use radix_types::TypeId;

    let mut resolver = Resolver::new();
    let pair = ConversionPair::new(TypeId(17), TypeId(23));
    resolver
        .register_builtin_conversion(pair, Span::default())
        .expect("builtin row enrolls");
    let user_pair = ConversionPair::new(TypeId(31), TypeId(23));
    let user_fn = DefId(900);
    resolver
        .register_user_conversion(user_pair, user_fn, Span::new(4, 8))
        .expect("user row enrolls");

    let restored = Resolver::from_snapshot(resolver.to_snapshot());
    let restored_user = restored
        .conversion_row_for(TypeId(31), TypeId(23))
        .expect("user row survives the snapshot boundary");
    assert_eq!(restored_user.fn_def_id, Some(DefId(900)));
    assert!(!restored_user.builtin);

    let restored_builtin = restored
        .conversion_row_for(TypeId(17), TypeId(23))
        .expect("builtin row survives the snapshot boundary");
    assert!(restored_builtin.builtin);
    assert_eq!(restored_builtin.fn_def_id, None);

    assert!(
        restored
            .conversion_row_for(TypeId(23), TypeId(17))
            .is_none(),
        "ordered pair identity is preserved, not reversed"
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

#[cfg(feature = "hir-fhir")]
#[test]
fn resolver_snapshot_orders_import_state_independently_of_insertion_order() {
    use radix_hir_fhir::artifact::{FileInterfaceSnapshot, NominalKind};
    use radix_types::TypeId;

    fn resolver_with_order(reversed: bool) -> Resolver {
        let mut snapshot = Resolver::new().to_snapshot();
        snapshot.namespace_exports = vec![
            (LexSymbol(8), vec![LexSymbol(4), LexSymbol(2)]),
            (LexSymbol(3), vec![LexSymbol(9), LexSymbol(1)]),
        ];
        snapshot.file_interfaces = vec![
            (
                LexSymbol(8),
                FileInterfaceSnapshot {
                    exports: Vec::new(),
                    unavailable: Vec::new(),
                    const_values: Vec::new(),
                },
            ),
            (
                LexSymbol(3),
                FileInterfaceSnapshot {
                    exports: Vec::new(),
                    unavailable: Vec::new(),
                    const_values: Vec::new(),
                },
            ),
        ];
        snapshot.used_namespaces = vec![LexSymbol(8), LexSymbol(3), LexSymbol(5)];
        snapshot.imported_nominal_types = vec![
            ((NominalKind::Interface, LexSymbol(7)), TypeId(17)),
            ((NominalKind::Struct, LexSymbol(9)), TypeId(19)),
            ((NominalKind::Enum, LexSymbol(2)), TypeId(12)),
        ];
        snapshot.ambiguous_imported_nominal_types = vec![
            (NominalKind::Interface, LexSymbol(7)),
            (NominalKind::Struct, LexSymbol(9)),
            (NominalKind::Enum, LexSymbol(2)),
        ];

        if reversed {
            snapshot.namespace_exports.reverse();
            for (_, members) in &mut snapshot.namespace_exports {
                members.reverse();
            }
            snapshot.file_interfaces.reverse();
            snapshot.used_namespaces.reverse();
            snapshot.imported_nominal_types.reverse();
            snapshot.ambiguous_imported_nominal_types.reverse();
        }
        Resolver::from_snapshot(snapshot)
    }

    assert_eq!(
        resolver_with_order(false).to_snapshot(),
        resolver_with_order(true).to_snapshot()
    );
}

#[cfg(feature = "hir-fhir")]
#[test]
fn resolver_snapshot_round_trips_imported_json_opt_in() {
    use radix_hir_fhir::artifact::{FileExportSnapshot, FileInterfaceSnapshot, ResolverSnapshot};
    use radix_types::TypeId;

    let snapshot = ResolverSnapshot {
        file_interfaces: vec![(
            LexSymbol(0),
            FileInterfaceSnapshot {
                exports: vec![(
                    LexSymbol(1),
                    FileExportSnapshot::Struct {
                        def_id: DefId(7),
                        ty: TypeId(8),
                        fields: Vec::new(),
                        methods: Vec::new(),
                        is_json: true,
                        implements: Vec::new(),
                    },
                )],
                unavailable: Vec::new(),
                const_values: Vec::new(),
            },
        )],
        ..Resolver::new().to_snapshot()
    };
    let resolver = Resolver::from_snapshot(snapshot);
    assert_eq!(
        resolver.imported_json_structs().collect::<Vec<_>>(),
        vec![DefId(7)]
    );
    let restored = Resolver::from_snapshot(resolver.to_snapshot());
    assert_eq!(
        restored.imported_json_structs().collect::<Vec<_>>(),
        vec![DefId(7)]
    );
}

#[cfg(feature = "hir-fhir")]
#[test]
fn resolver_snapshot_round_trips_nondum_gates_for_functions_and_methods() {
    use radix_hir_fhir::artifact::{
        FileExportSnapshot, FileInterfaceSnapshot, ResolverSnapshot, StructMethodSnapshot,
        UnavailableWire,
    };
    use radix_types::{FuncSig, TypeId};

    let namespace = LexSymbol(0);
    let function = LexSymbol(1);
    let genus = LexSymbol(2);
    let method = LexSymbol(3);
    let sig = FuncSig {
        type_params: Vec::new(),
        params: Vec::new(),
        ret: TypeId(8),
        err: None,
        is_async: false,
        is_generator: false,
    };
    let gate = || {
        vec![UnavailableWire {
            target: None,
            reason: Some("pending host dispatch".to_owned()),
        }]
    };
    let snapshot = ResolverSnapshot {
        file_interfaces: vec![(
            namespace,
            FileInterfaceSnapshot {
                exports: vec![
                    (function, FileExportSnapshot::Function(sig.clone())),
                    (
                        genus,
                        FileExportSnapshot::Struct {
                            def_id: DefId(7),
                            ty: TypeId(8),
                            fields: Vec::new(),
                            methods: vec![StructMethodSnapshot {
                                def_id: DefId(9),
                                name: method,
                                surface_name: method,
                                sig,
                                visibility: radix_syntax::MemberVisibility::Publica,
                                unavailable: gate(),
                            }],
                            is_json: false,
                            implements: Vec::new(),
                        },
                    ),
                ],
                unavailable: vec![(function, gate())],
                const_values: Vec::new(),
            },
        )],
        ..Resolver::new().to_snapshot()
    };
    let resolver = Resolver::from_snapshot(snapshot);

    let function_gates = resolver
        .namespace_file_unavailable(namespace, function)
        .expect("nondum free-function gate survives from_snapshot");
    assert_eq!(function_gates.len(), 1);
    assert_eq!(
        function_gates[0].reason.as_deref(),
        Some("pending host dispatch")
    );
    let again = resolver.to_snapshot();
    let genus_export = again.file_interfaces[0]
        .1
        .exports
        .iter()
        .find_map(|(member, export)| match export {
            FileExportSnapshot::Struct { methods, .. } if *member == genus => Some(methods),
            _ => None,
        })
        .expect("struct export restored");
    assert_eq!(genus_export[0].unavailable, gate());
    assert_eq!(
        again.file_interfaces[0].1.unavailable,
        vec![(function, gate())]
    );
}

#[cfg(feature = "hir-fhir")]
#[test]
fn resolver_snapshot_round_trips_const_values() {
    use crate::file_interface::InterfaceConstValue;
    use radix_hir_fhir::artifact::{
        ConstValueWire, FileExportSnapshot, FileInterfaceSnapshot, ResolverSnapshot,
    };
    use radix_types::TypeId;

    let namespace = LexSymbol(0);
    let members = [
        LexSymbol(1),
        LexSymbol(2),
        LexSymbol(3),
        LexSymbol(4),
        LexSymbol(5),
    ];
    let values = vec![
        (
            members[0],
            ConstValueWire::Int {
                high: -1,
                low: u64::MAX - 6,
            },
        ),
        (members[1], ConstValueWire::Float(2.5)),
        (members[2], ConstValueWire::Bool(true)),
        (members[3], ConstValueWire::Text("salve".to_owned())),
        (
            members[4],
            ConstValueWire::List(vec![
                ConstValueWire::Int {
                    high: i64::MAX,
                    low: u64::MAX,
                },
                ConstValueWire::Text("x".to_owned()),
            ]),
        ),
    ];
    let snapshot = ResolverSnapshot {
        file_interfaces: vec![(
            namespace,
            FileInterfaceSnapshot {
                exports: members
                    .iter()
                    .map(|&member| {
                        (
                            member,
                            FileExportSnapshot::Type {
                                ty: TypeId(8),
                                def_id: None,
                            },
                        )
                    })
                    .collect(),
                unavailable: Vec::new(),
                const_values: values.clone(),
            },
        )],
        ..Resolver::new().to_snapshot()
    };
    let resolver = Resolver::from_snapshot(snapshot);

    assert_eq!(
        resolver.namespace_file_const_value_canonical(namespace, members[0]),
        Some(&InterfaceConstValue::Int(-7))
    );
    assert_eq!(
        resolver.namespace_file_const_value_canonical(namespace, members[4]),
        Some(&InterfaceConstValue::List(vec![
            InterfaceConstValue::Int(i128::from(i64::MAX) << 64 | i128::from(u64::MAX)),
            InterfaceConstValue::Text("x".to_owned()),
        ]))
    );
    assert_eq!(
        resolver.to_snapshot().file_interfaces[0].1.const_values,
        values
    );
}

#[cfg(feature = "hir-fhir")]
#[test]
fn resolver_snapshot_round_trips_struct_field_initializers() {
    use radix_hir::{HirExpression, HirExpressionKind, HirId, HirLiteral};
    use radix_hir_fhir::artifact::{
        FileExportSnapshot, FileInterfaceSnapshot, ResolverSnapshot, StructFieldSnapshot,
    };
    use radix_types::TypeId;

    let namespace = LexSymbol(0);
    let genus = LexSymbol(1);
    let limit = LexSymbol(2);
    let init = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Literal(HirLiteral::Int(7)),
        ty: Some(TypeId(3)),
        span: Span::default(),
    };
    let snapshot = ResolverSnapshot {
        file_interfaces: vec![(
            namespace,
            FileInterfaceSnapshot {
                exports: vec![(
                    genus,
                    FileExportSnapshot::Struct {
                        def_id: DefId(7),
                        ty: TypeId(8),
                        fields: vec![StructFieldSnapshot {
                            name: limit,
                            ty: TypeId(3),
                            optional: false,
                            required: false,
                            modifier: Some(radix_syntax::FieldModifier::Const),
                            visibility: radix_syntax::MemberVisibility::Publica,
                            init: Some(init.clone()),
                        }],
                        methods: Vec::new(),
                        is_json: false,
                        implements: Vec::new(),
                    },
                )],
                unavailable: Vec::new(),
                const_values: Vec::new(),
            },
        )],
        ..Resolver::new().to_snapshot()
    };
    let resolver = Resolver::from_snapshot(snapshot);

    let structs = resolver.imported_file_structs();
    assert_eq!(structs[0].1[0].init, Some(init.clone()));
    let again = resolver.to_snapshot();
    let FileExportSnapshot::Struct { fields, .. } = &again.file_interfaces[0].1.exports[0].1 else {
        panic!("struct export restored");
    };
    assert_eq!(fields[0].init, Some(init));
}

/// A-189: an imported enum's consumer-side variant rows (`imported_enum_variants`)
/// and `@ commune` shared fields (`commune_fields`) must survive the FHIR
/// snapshot boundary, so a scope rebuilt from a snapshot matches the scope that
/// produced it.
///
/// Blocked (A-189, both halves): `Resolver::to_snapshot` serializes an enum
/// export as `FileExportSnapshot::Type { ty, def_id }` — variant names,
/// variant DefIds (consumer-fresh, unreproducible), payload fields, and
/// commune shared fields exist nowhere on the FHIR wire. Restoring either
/// table needs new fields in the `radix-hir-fhir` wire schema (a
/// `SCHEMA_VERSION` bump; decode enforces exact equality) and is outside this
/// unit's write scope. Re-enable when the wire carries enum payloads.
#[cfg(feature = "hir-fhir")]
#[ignore = "A-189 blocked: imported_enum_variants/commune_fields need enum payload on the FHIR wire (ResolverSnapshot/FileExportSnapshot) = SCHEMA_VERSION bump, outside unit scope"]
#[test]
fn resolver_snapshot_round_trips_imported_enum_variants_and_commune_fields() {
    use crate::TypeTable;
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceNominalExport,
        InterfaceStructField, InterfaceTypeSnapshot,
    };
    use std::collections::BTreeMap;

    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Kind".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            name: "Kind".to_owned(),
            params: Vec::new(),
            methods: Vec::new(),
            shared_fields: vec![InterfaceStructField {
                name: "message".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(crate::Primitive::Textus),
                optional: false,
                required: true,
                default: None,
                modifier: None,
                visibility: radix_syntax::MemberVisibility::Publica,
            }],
            radix_contract: None,
            variants: vec![
                InterfaceEnumVariant {
                    name: "Fine".to_owned(),
                    fields: Vec::new(),
                },
                InterfaceEnumVariant {
                    name: "Sized".to_owned(),
                    fields: vec![InterfaceStructField {
                        name: "payload".to_owned(),
                        ty: InterfaceTypeSnapshot::Primitive(crate::Primitive::Numerus),
                        optional: false,
                        required: true,
                        default: None,
                        modifier: None,
                        visibility: radix_syntax::MemberVisibility::Publica,
                    }],
                },
            ],
        }),
    });

    let mut resolver = Resolver::new();
    let mut interner = radix_lexer::Interner::new();
    let mut types = TypeTable::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("mod".to_owned(), interface)]),
            &mut interner,
            &mut types,
        )
        .expect("enum interface installs");

    let fine = interner.intern("Fine");
    let sized = interner.intern("Sized");
    let message = interner.intern("message");

    // Live install: both tables are populated for the enum.
    let live_enums = resolver.imported_file_enums();
    assert_eq!(
        live_enums.len(),
        1,
        "live install keeps the enum's variant rows"
    );
    let (enum_def, _, live_variants) = &live_enums[0];
    assert_eq!(live_variants.len(), 2, "both variants install");
    assert_eq!(live_variants[0].name, fine);
    assert_eq!(live_variants[1].name, sized);
    assert_eq!(live_variants[1].fields.len(), 1, "variant payload remaps");
    assert!(!live_variants[1].payload_unresolved);
    let live_commune = resolver.commune_fields(*enum_def);
    assert_eq!(
        live_commune.len(),
        1,
        "live install keeps the commune region"
    );
    assert_eq!(live_commune[0].name, message);

    // Round trip through the FHIR snapshot.
    let restored = Resolver::from_snapshot(resolver.to_snapshot());
    let restored_enums = restored.imported_file_enums();
    assert_eq!(
        restored_enums.len(),
        live_enums.len(),
        "A-189 half 1: imported_enum_variants survive the snapshot boundary"
    );
    let restored_commune = restored.commune_fields(*enum_def);
    assert_eq!(
        restored_commune.len(),
        live_commune.len(),
        "A-189 half 2: commune_fields survive the snapshot boundary"
    );
    if let Some(field) = restored_commune.first() {
        assert_eq!(field.name, live_commune[0].name);
        assert_eq!(field.ty, live_commune[0].ty);
    }
    if let Some((_, _, restored_variants)) = restored_enums.first() {
        let live_names: Vec<_> = live_variants.iter().map(|v| (v.def_id, v.name)).collect();
        let restored_names: Vec<_> = restored_variants
            .iter()
            .map(|v| (v.def_id, v.name))
            .collect();
        assert_eq!(restored_names, live_names);
        assert_eq!(
            restored_variants[1].fields.len(),
            live_variants[1].fields.len(),
            "variant payload fields survive"
        );
    }
}

/// Producer-shaped interface exporting the given enums (name, variants) and
/// primitive-bodied type aliases. Aliases install as `Type` exports without a
/// definition identity, so they are not enums.
fn imported_enums_and_aliases(
    enums: &[(&str, &[&str])],
    aliases: &[&str],
) -> crate::file_interface::FileInterface {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceAliasExport, InterfaceEnumVariant,
        InterfaceNominalExport, InterfaceTypeSnapshot,
    };

    let mut interface = FileInterface::new();
    for alias in aliases {
        interface.insert(FileExport {
            name: (*alias).to_owned(),
            kind: FileExportKind::TypeAlias(InterfaceAliasExport {
                name: (*alias).to_owned(),
                params: Vec::new(),
                body: InterfaceTypeSnapshot::Primitive(crate::Primitive::Numerus),
            }),
        });
    }
    for (name, variants) in enums {
        interface.insert(FileExport {
            name: (*name).to_owned(),
            kind: FileExportKind::Enum(InterfaceNominalExport {
                name: (*name).to_owned(),
                params: Vec::new(),
                methods: Vec::new(),
                shared_fields: Vec::new(),
                radix_contract: None,
                variants: variants
                    .iter()
                    .map(|variant| InterfaceEnumVariant {
                        name: (*variant).to_owned(),
                        fields: Vec::new(),
                    })
                    .collect(),
            }),
        });
    }
    interface
}

fn install_namespace(
    interface: crate::file_interface::FileInterface,
) -> (Resolver, radix_lexer::Interner) {
    use crate::TypeTable;
    use std::collections::BTreeMap;

    let mut resolver = Resolver::new();
    let mut interner = radix_lexer::Interner::new();
    let mut types = TypeTable::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("lib".to_owned(), interface)]),
            &mut interner,
            &mut types,
        )
        .expect("interface installs");
    (resolver, interner)
}

/// A bare `lib.X` naming a variant of two exported enums must not bind to
/// whichever enum the export table happens to yield first.
#[test]
fn namespace_variant_shared_across_enums_fails_closed() {
    let (resolver, mut interner) = install_namespace(imported_enums_and_aliases(
        &[("Alpha", &["X", "Y"]), ("Beta", &["X", "Z"])],
        &[],
    ));
    let namespace = interner.intern("lib");
    let shared = interner.intern("X");
    let alpha_only = interner.intern("Y");
    let beta_only = interner.intern("Z");

    assert_eq!(resolver.namespace_file_variant(namespace, shared), None);
    assert!(
        resolver
            .namespace_file_variant(namespace, alpha_only)
            .is_some()
    );
    assert!(
        resolver
            .namespace_file_variant(namespace, beta_only)
            .is_some()
    );
}

/// A non-enum `Type` export (an alias carries no definition identity) must
/// not end the scan before the namespace's enums are reached.
#[test]
fn namespace_variant_scan_skips_non_enum_type_exports() {
    let (resolver, mut interner) = install_namespace(imported_enums_and_aliases(
        &[("Alpha", &["X", "Y"])],
        &["First", "Second", "Third"],
    ));
    let namespace = interner.intern("lib");
    let variant = interner.intern("X");

    assert!(
        resolver
            .namespace_file_variant(namespace, variant)
            .is_some(),
        "an alias export must not hide the enum's variant"
    );
}

/// An imported function's and method's declared `axis` markers survive the
/// file-interface install and are readable by the canonical accessors
/// (polish A-202). The echo rule reads these at every imported call site.
#[test]
fn imported_axis_markers_round_trip_through_the_resolver() {
    use crate::Primitive;
    use crate::SemanticParamMode;
    use crate::TypeTable;
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceMethodExport,
        InterfaceParam, InterfaceStructExport, InterfaceTypeSnapshot,
    };
    use std::collections::BTreeMap;

    let number = || InterfaceTypeSnapshot::Primitive(Primitive::Numerus);
    let param = |axis| InterfaceParam {
        ty: number(),
        mode: SemanticParamMode::Owned,
        optional: false,
        sponte: false,
        axis,
    };
    let callable = |params| InterfaceCallable {
        type_params: Vec::new(),
        params,
        ret: Box::new(number()),
        err: None,
        is_async: false,
        is_generator: false,
        tensor_edge_policy_requirements: Vec::new(),
        device: None,
    };

    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "rows".to_owned(),
        kind: FileExportKind::Function(callable(vec![param(true), param(false)])),
    });
    interface.insert(FileExport {
        name: "Row".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            name: "Row".to_owned(),
            params: Vec::new(),
            fields: Vec::new(),
            methods: vec![InterfaceMethodExport {
                name: "rows".to_owned(),
                callable: callable(vec![param(true)]),
                visibility: radix_syntax::MemberVisibility::Publica,
                unavailable: Vec::new(),
            }],
            annotation_contract: None,
        }),
    });

    let mut resolver = Resolver::new();
    let mut interner = radix_lexer::Interner::new();
    let mut types = TypeTable::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("lib".to_owned(), interface)]),
            &mut interner,
            &mut types,
        )
        .expect("axis interface installs");

    let namespace = interner.intern("lib");
    let rows = interner.intern("rows");
    assert_eq!(
        resolver.namespace_file_axis_params_canonical(namespace, rows),
        Some(&[true, false][..]),
        "imported function axis flags stay on the exported parameter records"
    );

    let exported = resolver.imported_file_structs();
    assert_eq!(exported.len(), 1);
    let methods = &exported[0].2;
    assert_eq!(methods.len(), 1);
    assert_eq!(
        methods[0].axis,
        vec![true],
        "imported method axis flags stay on the imported method record"
    );
}
