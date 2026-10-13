//! Compiler-owned frame stream types (`scrinium`, `status`, `sermo`, `meus`,
//! `tuus`).
//!
//! Registers frame types in the resolver and injects corresponding HIR items
//! when the program uses frame operations (`ad`, `meus`, `tuus`). Does not
//! own lowering or codegen of frame operations; those live in the respective
//! MIR and codegen crates.

use crate::{Primitive, Resolver, SymbolKind, Type, TypeId, TypeTable};
use radix_hir::{DefId, HirEnum, HirField, HirId, HirItem, HirItemKind, HirStruct, HirVariant};
use radix_lexer::{Interner, Span};

const SCRINIUM_NAME: &str = "scrinium";
const STATUS_NAME: &str = "status";
const SERMO_NAME: &str = "sermo";
const MEUS_NAME: &str = "meus";
const TUUS_NAME: &str = "tuus";

/// WHY: alias keeps the field-table signature readable; the function-pointer
/// type is structural and reused across every builtin frame field.
type FieldTypeFn = fn(&mut TypeTable, &Resolver) -> TypeId;

const SCRINIUM_FIELDS: &[(&str, FieldTypeFn)] = &[
    ("id", |types, _| types.primitive(Primitive::Textus)),
    ("parent_id", |types, _| {
        let textus = types.primitive(Primitive::Textus);
        let nihil = types.primitive(Primitive::Nihil);
        types.intern(Type::Union(vec![textus, nihil]))
    }),
    ("call", |types, _| types.primitive(Primitive::Ascii)),
    ("status", |types, resolver| {
        status_type_id(types, resolver).unwrap_or_else(|| types.primitive(Primitive::Ignotum))
    }),
    ("data", |types, _| types.primitive(Primitive::Valor)),
    ("created_ms", |types, _| types.primitive(Primitive::Numerus)),
    ("from", |types, _| {
        let textus = types.primitive(Primitive::Textus);
        let nihil = types.primitive(Primitive::Nihil);
        types.intern(Type::Union(vec![textus, nihil]))
    }),
    ("trace", |types, _| {
        let valor = types.primitive(Primitive::Valor);
        let nihil = types.primitive(Primitive::Nihil);
        types.intern(Type::Union(vec![valor, nihil]))
    }),
];

fn define_builtin_symbol(resolver: &mut Resolver, symbol: crate::Symbol) {
    debug_assert!(resolver.define(symbol).is_ok());
}

const STATUS_VARIANTS: &[&str] = &["request", "item", "byte", "bulk", "done", "error", "cancel"];

#[must_use]
pub fn scrinium_field_sponte(name: &str) -> bool {
    !matches!(name, "status" | "data")
}

/// Register compiler-owned frame types in the resolver.
pub fn register(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    register_scrinium(resolver, types, interner);
    register_status(resolver, types, interner);
    register_sermo(resolver, types, interner);
    register_meus(resolver, types, interner);
    register_tuus(resolver, types, interner);
}

fn register_scrinium(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    let name = interner.intern(SCRINIUM_NAME);
    if resolver.lookup_type(name).is_some() {
        return;
    }
    let def_id = resolver.fresh_builtin_def_id();
    let symbol = crate::Symbol {
        def_id,
        name,
        kind: SymbolKind::Struct,
        ty: Some(types.intern(Type::Struct(def_id))),
        mutable: false,
        span: Span::default(),
    };
    define_builtin_symbol(resolver, symbol);
    let field_symbols = SCRINIUM_FIELDS
        .iter()
        .map(|(name, _)| interner.intern(name))
        .collect();
    resolver.register_builtin_scrinium(def_id, field_symbols);
}

fn register_status(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    let name = interner.intern(STATUS_NAME);
    if resolver.lookup_type(name).is_some() {
        return;
    }
    let def_id = resolver.fresh_builtin_def_id();
    let symbol = crate::Symbol {
        def_id,
        name,
        kind: SymbolKind::Enum,
        ty: Some(types.intern(Type::Enum(def_id))),
        mutable: false,
        span: Span::default(),
    };
    define_builtin_symbol(resolver, symbol);
    for variant in STATUS_VARIANTS {
        let variant_name = interner.intern(variant);
        let variant_def = resolver.fresh_builtin_def_id();
        let variant_symbol = crate::Symbol {
            def_id: variant_def,
            name: variant_name,
            kind: SymbolKind::Variant,
            ty: None,
            mutable: false,
            span: Span::default(),
        };
        define_builtin_symbol(resolver, variant_symbol);
        resolver.register_builtin_status_variant(def_id, variant_def, variant_name);
    }
    resolver.register_builtin_status(def_id);
}

fn register_directional_view(
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
    name: &str,
    register_id: fn(&mut Resolver, DefId),
) {
    let symbol = interner.intern(name);
    if resolver.lookup_type(symbol).is_some() {
        return;
    }
    let def_id = resolver.fresh_builtin_def_id();
    let ty = types.intern(Type::Struct(def_id));
    define_builtin_symbol(
        resolver,
        crate::Symbol {
            def_id,
            name: symbol,
            kind: SymbolKind::Struct,
            ty: Some(ty),
            mutable: false,
            span: Span::default(),
        },
    );
    register_id(resolver, def_id);
}

fn register_meus(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    register_directional_view(resolver, types, interner, MEUS_NAME, |resolver, def_id| {
        resolver.register_builtin_meus(def_id);
    });
}

fn register_tuus(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    register_directional_view(resolver, types, interner, TUUS_NAME, |resolver, def_id| {
        resolver.register_builtin_tuus(def_id);
    });
}

fn register_sermo(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    let name = interner.intern(SERMO_NAME);
    if resolver.lookup_type(name).is_some() {
        return;
    }
    let def_id = resolver.fresh_builtin_def_id();
    let symbol = crate::Symbol {
        def_id,
        name,
        kind: SymbolKind::Struct,
        ty: Some(types.intern(Type::Struct(def_id))),
        mutable: false,
        span: Span::default(),
    };
    define_builtin_symbol(resolver, symbol);
    resolver.register_builtin_sermo(def_id);
}

#[must_use]
pub fn scrinium_def_id(resolver: &Resolver) -> Option<DefId> {
    resolver.builtin_scrinium_def_id()
}

#[must_use]
pub fn status_def_id(resolver: &Resolver) -> Option<DefId> {
    resolver.builtin_status_def_id()
}

#[must_use]
pub fn sermo_def_id(resolver: &Resolver) -> Option<DefId> {
    resolver.builtin_sermo_def_id()
}

#[must_use]
pub fn meus_def_id(resolver: &Resolver) -> Option<DefId> {
    resolver.builtin_meus_def_id()
}

#[must_use]
pub fn tuus_def_id(resolver: &Resolver) -> Option<DefId> {
    resolver.builtin_tuus_def_id()
}

pub fn scrinium_type_id(types: &mut TypeTable, def_id: DefId) -> TypeId {
    types.intern(Type::Struct(def_id))
}

pub fn scrinium_payload_type_id(
    types: &mut TypeTable,
    def_id: DefId,
    payload_ty: TypeId,
) -> TypeId {
    let base = scrinium_type_id(types, def_id);
    types.intern(Type::Applied(
        base,
        vec![radix_types::AppliedArg::Type(payload_ty)],
    ))
}

pub fn status_type_id(types: &mut TypeTable, resolver: &Resolver) -> Option<TypeId> {
    status_def_id(resolver).map(|def_id| types.intern(Type::Enum(def_id)))
}

pub fn sermo_type_id(types: &mut TypeTable, def_id: DefId) -> TypeId {
    types.intern(Type::Struct(def_id))
}

/// `sermo<O, R>` (DECISIONS D6.11). `sermo<valor, valor>` is the bare
/// carrier (D6.12), so the loose form has exactly one `TypeId`.
pub fn sermo_typed_type_id(
    types: &mut TypeTable,
    def_id: DefId,
    opener: TypeId,
    reply: TypeId,
) -> TypeId {
    let base = sermo_type_id(types, def_id);
    let valor = types.primitive(Primitive::Valor);
    if opener == valor && reply == valor {
        return base;
    }
    types.intern(Type::Applied(
        base,
        vec![
            radix_types::AppliedArg::Type(opener),
            radix_types::AppliedArg::Type(reply),
        ],
    ))
}

/// Static view of a conversation type: bare `sermo` or `sermo<O, R>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SermoShape {
    /// Bare `sermo`, meaning `sermo<valor, valor>`.
    Bare,
    /// `sermo<O, R>` from a Faber route or a written annotation.
    Typed { opener: TypeId, reply: TypeId },
}

/// Classify `ty` (already resolved past inference) as a conversation type.
#[must_use]
pub fn sermo_shape(types: &TypeTable, sermo_def: DefId, ty: TypeId) -> Option<SermoShape> {
    match types.get(ty) {
        Type::Struct(def_id) if *def_id == sermo_def => Some(SermoShape::Bare),
        Type::Applied(base, args) => {
            if !matches!(types.get(*base), Type::Struct(def_id) if *def_id == sermo_def) {
                return None;
            }
            match args.as_slice() {
                [
                    radix_types::AppliedArg::Type(opener),
                    radix_types::AppliedArg::Type(reply),
                ] => Some(SermoShape::Typed {
                    opener: *opener,
                    reply: *reply,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Marker argument on a `meus<T>` view whose peer is a Faber handler: the
/// handler cannot read later frames (two-way handlers are deferred, D6.11),
/// so `da` on it is a compile error. Typecheck-only; finalize erases it.
#[must_use]
pub fn closed_meus_marker(types: &mut TypeTable) -> TypeId {
    types.primitive(Primitive::Numquam)
}

/// `meus<T>` whose outbound side is closed toward a Faber handler.
pub fn closed_meus_type_id(types: &mut TypeTable, meus_def: DefId, payload_ty: TypeId) -> TypeId {
    let base = types.intern(Type::Struct(meus_def));
    let marker = closed_meus_marker(types);
    types.intern(Type::Applied(
        base,
        vec![
            radix_types::AppliedArg::Type(payload_ty),
            radix_types::AppliedArg::Type(marker),
        ],
    ))
}

/// Compile-time-only erasure of conversation typing (D6.11: the wire is
/// unchanged). `sermo<O, R>` becomes the bare `sermo` carrier and a closed
/// `meus<T>` becomes `meus<T>`, so every backend keeps matching the shapes it
/// already knows. `None` when `ty` needs no erasure.
#[must_use]
pub fn erase_conversation_type(
    types: &mut TypeTable,
    resolver: &Resolver,
    ty: TypeId,
) -> Option<TypeId> {
    let Type::Applied(base, args) = types.get(ty).clone() else {
        return None;
    };
    let Type::Struct(def_id) = types.get(base).clone() else {
        return None;
    };
    if Some(def_id) == sermo_def_id(resolver) {
        return Some(base);
    }
    if Some(def_id) == meus_def_id(resolver) && args.len() == 2 {
        return Some(types.intern(Type::Applied(base, vec![args[0]])));
    }
    None
}

pub fn directional_view_payload_type_id(
    types: &mut TypeTable,
    view_def: DefId,
    payload_ty: TypeId,
) -> TypeId {
    let base = types.intern(Type::Struct(view_def));
    types.intern(Type::Applied(
        base,
        vec![radix_types::AppliedArg::Type(payload_ty)],
    ))
}

/// Return whether the parsed program needs compiler-owned frame types registered.
///
/// Answers fail-closed: if the walker refuses to descend past its depth limit
/// the scan is incomplete, so the builtins are registered rather than reporting
/// "not used" for a subtree that was never read. The refused depth is recorded
/// on the visitor for callers that can report it.
#[must_use]
///
/// `locale_pack` supplies reader spellings of the sermo view methods
/// (`s.recv<T>()` under en is `s.tuus<T>()`, DECISIONS D6.16); member names
/// are not canonicalized before semantic analysis.
pub fn program_needs_frame_types(
    program: &radix_syntax::Program,
    interner: &Interner,
    locale_pack: Option<&crate::locale::LocalePack>,
) -> bool {
    use radix_syntax::{DepthExceeded, ExprKind, Program, Visitor, walk_stmt};

    struct FrameUsageVisitor<'a> {
        interner: &'a Interner,
        locale_pack: Option<&'a crate::locale::LocalePack>,
        found: bool,
        /// Depth the walker refused to enter, if it stopped short.
        depth_exceeded: Option<DepthExceeded>,
    }

    impl FrameUsageVisitor<'_> {
        /// Walk the program, stopping at the first `ad`/`meus`/`tuus` use.
        fn scan(&mut self, program: &Program) {
            for stmt in &program.statements {
                self.visit_stmt(stmt);
                if self.found {
                    break;
                }
            }
        }
    }

    impl Visitor for FrameUsageVisitor<'_> {
        fn on_depth_exceeded(&mut self, exceeded: DepthExceeded) {
            self.depth_exceeded = Some(exceeded);
        }

        fn visit_stmt(&mut self, stmt: &radix_syntax::Stmt) {
            if self.found {
                return;
            }
            walk_stmt(self, stmt);
        }

        fn visit_expr(&mut self, expr: &radix_syntax::Expr) {
            if self.found {
                return;
            }
            if matches!(&expr.kind, ExprKind::Ad(_)) {
                self.found = true;
                return;
            }
            if let ExprKind::Member(member) = &expr.kind {
                let name = self.interner.resolve(member.member.name);
                let pack_view = self
                    .locale_pack
                    .and_then(|pack| pack.type_binding_kind(name));
                if matches!(name, "meus" | "tuus")
                    || matches!(
                        pack_view,
                        Some(crate::locale::TypeBindingKind::Frame(
                            crate::locale::FrameTypeKind::Meus | crate::locale::FrameTypeKind::Tuus
                        ))
                    )
                {
                    self.found = true;
                    return;
                }
            }
            radix_syntax::walk_expr(self, expr);
        }
    }

    let mut visitor = FrameUsageVisitor {
        interner,
        locale_pack,
        found: false,
        depth_exceeded: None,
    };
    visitor.scan(program);
    visitor.found || visitor.depth_exceeded.is_some()
}

/// Append builtin frame type items to lowered HIR when the program uses them.
pub fn inject_hir_items(
    items: &mut Vec<HirItem>,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) {
    // WHY: `status` must precede `scrinium` in generated Rust so derived impls
    // and smoke compiles see the enum before the struct that references it.
    inject_status(items, resolver, types, interner);
    inject_scrinium(items, resolver, types, interner);
    inject_sermo(items, resolver, types, interner);
    inject_directional_view(items, resolver, types, interner, MEUS_NAME, meus_def_id);
    inject_directional_view(items, resolver, types, interner, TUUS_NAME, tuus_def_id);
}

fn inject_scrinium(
    items: &mut Vec<HirItem>,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) {
    let Some(def_id) = scrinium_def_id(resolver) else {
        return;
    };
    if items.iter().any(|item| item.def_id == def_id) {
        return;
    }

    let mut fields = Vec::new();
    let payload_param = interner.intern("T");
    for (field_name, field_ty) in SCRINIUM_FIELDS {
        let sponte = scrinium_field_sponte(field_name);
        let ty = if *field_name == "data" {
            types.intern(Type::Param(payload_param))
        } else {
            field_ty(types, resolver)
        };
        fields.push(HirField {
            def_id: resolver.fresh_def_id(),
            name: interner.intern(field_name),
            ty,
            sponte,
            modifier: None,
            annotations: Vec::new(),
            init: None,
            span: Span::default(),
        });
    }

    items.push(HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id,
        kind: HirItemKind::Struct(HirStruct {
            name: interner.intern(SCRINIUM_NAME),
            type_params: vec![radix_hir::HirTypeParam {
                def_id: resolver.fresh_def_id(),
                kind: radix_syntax::GenericParamKind::Typus,
                name: payload_param,
                span: Span::default(),
                constraint: radix_hir::HirTypeParamConstraint::Any,
                default: None,
            }],
            fields,
            methods: Vec::new(),
            implements: Vec::new(),
        }),
        span: Span::default(),
    });
}

fn inject_status(
    items: &mut Vec<HirItem>,
    resolver: &mut Resolver,
    _types: &mut TypeTable,
    interner: &mut Interner,
) {
    let Some(def_id) = status_def_id(resolver) else {
        return;
    };
    if items.iter().any(|item| item.def_id == def_id) {
        return;
    }

    let variants = resolver
        .builtin_status_variants(def_id)
        .into_iter()
        .map(|(variant_def, name)| HirVariant {
            def_id: variant_def,
            name,
            fields: Vec::new(),
            span: Span::default(),
        })
        .collect();

    items.push(HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id,
        kind: HirItemKind::Enum(HirEnum {
            name: interner.intern(STATUS_NAME),
            type_params: Vec::new(),
            commune_fields: Vec::new(),
            variants,
        }),
        span: Span::default(),
    });
}

fn inject_directional_view(
    items: &mut Vec<HirItem>,
    resolver: &mut Resolver,
    _types: &mut TypeTable,
    interner: &mut Interner,
    name: &str,
    def_id_lookup: fn(&Resolver) -> Option<DefId>,
) {
    let Some(def_id) = def_id_lookup(resolver) else {
        return;
    };
    if items.iter().any(|item| item.def_id == def_id) {
        return;
    }
    let payload_param = interner.intern("T");
    items.push(HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id,
        kind: HirItemKind::Struct(HirStruct {
            name: interner.intern(name),
            type_params: vec![radix_hir::HirTypeParam {
                def_id: resolver.fresh_def_id(),
                kind: radix_syntax::GenericParamKind::Typus,
                name: payload_param,
                span: Span::default(),
                constraint: radix_hir::HirTypeParamConstraint::Any,
                default: None,
            }],
            fields: Vec::new(),
            methods: Vec::new(),
            implements: Vec::new(),
        }),
        span: Span::default(),
    });
}

fn inject_sermo(
    items: &mut Vec<HirItem>,
    resolver: &mut Resolver,
    _types: &mut TypeTable,
    interner: &mut Interner,
) {
    let Some(def_id) = sermo_def_id(resolver) else {
        return;
    };
    if items.iter().any(|item| item.def_id == def_id) {
        return;
    }

    items.push(HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id,
        kind: HirItemKind::Struct(HirStruct {
            name: interner.intern(SERMO_NAME),
            type_params: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            implements: Vec::new(),
        }),
        span: Span::default(),
    });
}

// In-file pin for the scan above.
#[cfg(test)]
#[path = "frame_usage_budget_test.rs"]
mod frame_usage_budget_test;
