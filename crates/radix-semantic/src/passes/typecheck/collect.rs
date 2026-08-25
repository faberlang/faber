//! Declaration collection for the HIR typechecker.
//!
//! Typecheck needs a complete table of callable and aggregate contracts before
//! it checks expression bodies. This file extracts those contracts from resolved
//! HIR without validating bodies, so declaration order does not affect calls,
//! method lookup, enum variant constructors, or interface checks.
//!
//! INVARIANTS
//! ==========
//! - Collected `TypeId`s are already lowered into the shared semantic type
//!   table by earlier passes.
//! - Function and method signatures preserve parameter modes, optional slots,
//!   async/generator flags, and alternate-exit types for later call checking.
//! - Struct fields record requiredness from `sponte` and initializers; struct
//!   literal validation depends on that distinction.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

/// Memberless view for builtin nominal receivers (sermo, directional views).
fn empty_struct_info() -> StructInfo {
    StructInfo {
        fields: FxHashMap::default(),
        methods: FxHashMap::default(),
        method_spans: FxHashMap::default(),
        is_abstract: false,
        own_bodyless: FxHashSet::default(),
        own_bodied: FxHashSet::default(),
        bodyless_methods: FxHashSet::default(),
    }
}

impl TypeChecker<'_> {
    pub fn callable_boundary_return(
        &mut self,
        ret: TypeId,
        err: Option<TypeId>,
        is_async: bool,
        is_generator: bool,
    ) -> (TypeId, Option<TypeId>) {
        if is_async && is_generator {
            let alternate = err.unwrap_or_else(|| self.types.primitive(Primitive::Numquam));
            return (self.types.async_cursor(ret, alternate), None);
        }

        let mut ret = ret;
        let mut err = err;
        if is_async {
            ret = if let Some(err_ty) = err.take() {
                self.types.promissum_failable(ret, err_ty)
            } else {
                self.types.promissum(ret)
            };
        }
        if is_generator {
            ret = self.types.array(ret);
        }
        (ret, err)
    }

    fn param_types<'b>(&self, params: impl IntoIterator<Item = &'b HirParam>) -> Vec<ParamType> {
        params
            .into_iter()
            .map(|param| ParamType {
                ty: param.ty,
                mode: param_mode_from_hir(param.mode),
                optional: param.optional,
            })
            .collect()
    }

    /// Build the callable contract for a lowered function item or method.
    ///
    /// Missing return annotations are treated as `vacuum` at the signature
    /// boundary. Body checking may still synthesize return flow internally, but
    /// call sites need a concrete contract before any body is visited.
    pub fn function_signature(&mut self, func: &HirFunction) -> FuncSig {
        let params = self.param_types(&func.params);
        let ret = func.ret_ty.unwrap_or_else(|| self.vacuum_type());
        let (ret, err) =
            self.callable_boundary_return(ret, func.err_ty, func.is_async, func.is_generator);
        FuncSig {
            type_params: func.type_params.iter().map(|param| param.name).collect(),
            type_param_constraints: func
                .type_params
                .iter()
                .map(|param| match &param.constraint {
                    radix_hir::HirTypeParamConstraint::Any => TypeParamConstraint::Any,
                    radix_hir::HirTypeParamConstraint::OneOf(allowed) => {
                        TypeParamConstraint::OneOf(allowed.clone())
                    }
                })
                .collect(),
            params,
            ret,
            err,
            is_async: func.is_async,
            is_generator: func.is_generator,
        }
    }

    /// Collect field and method contracts for a struct definition.
    ///
    /// Methods are stored as signatures rather than checked here so recursive
    /// and forward calls see the same declaration table as ordinary functions.
    /// Inherited members are merged later by [`Self::resolve_struct_heritage`]
    /// so parent views are complete regardless of declaration order.
    pub fn collect_struct(&mut self, def_id: DefId, struct_item: &HirStruct, span: radix_lexer::Span) {
        let mut fields = FxHashMap::default();
        let mut methods = FxHashMap::default();
        let mut method_spans = FxHashMap::default();
        let mut own_bodyless = FxHashSet::default();
        let mut own_bodied = FxHashSet::default();

        if self.annotations_contain_json(&struct_item.annotations) {
            self.json_structs.insert(def_id);
        }

        for field in &struct_item.fields {
            fields.insert(
                field.name,
                StructFieldInfo {
                    ty: field.ty,
                    optional: field.sponte,
                    required: !field.sponte && field.init.is_none(),
                    span: field.span,
                },
            );
        }

        for method in &struct_item.methods {
            let sig = self.function_signature(&method.func);
            methods.insert(method.func.name, sig);
            method_spans.insert(method.func.name, method.span);
            if method.func.body.is_some() {
                own_bodied.insert(method.func.name);
            } else {
                own_bodyless.insert(method.func.name);
            }
            if let Some(role) = method.func.nucleum_role() {
                self.nucleum_roles.insert(method.def_id, role);
                self.method_nucleum_roles
                    .insert((def_id, method.func.name), role);
            }
        }

        self.type_params_by_def.insert(
            def_id,
            struct_item
                .type_params
                .iter()
                .map(|param| param.name)
                .collect(),
        );

        self.struct_heritage.insert(
            def_id,
            StructHeritage {
                extends: struct_item.extends,
                implements: struct_item.implements.clone(),
            },
        );
        self.struct_order.push((def_id, span));

        self.structs.insert(
            def_id,
            StructInfo {
                fields,
                methods,
                method_spans,
                is_abstract: struct_item.is_abstract,
                own_bodyless,
                own_bodied,
                bodyless_methods: FxHashSet::default(),
            },
        );
    }

    fn collect_interface(&mut self, def_id: DefId, interface_item: &radix_hir::HirInterface) {
        let mut methods = FxHashMap::default();

        for method in &interface_item.methods {
            let params = self.param_types(&method.params);
            let ret = method.ret_ty.unwrap_or_else(|| self.vacuum_type());
            let (ret, err) = self.callable_boundary_return(
                ret,
                method.err_ty,
                method.posture.is_async(),
                method.posture.is_stream(),
            );
            methods.insert(
                method.name,
                InterfaceMethodInfo {
                    sig: FuncSig {
                        type_params: Vec::new(),
                        type_param_constraints: Vec::new(),
                        params,
                        ret,
                        err,
                        is_async: method.posture.is_async(),
                        is_generator: method.posture.is_stream(),
                    },
                    availability: method.availability.clone(),
                    span: method.span,
                },
            );
        }

        self.interfaces.insert(def_id, methods);
    }

    fn collect_builtin_scrinium(&mut self) {
        let Some(def_id) = self.resolver.builtin_scrinium_def_id() else {
            return;
        };
        let span = radix_lexer::Span::default();
        let textus = self.types.primitive(Primitive::Textus);
        let ascii = self.types.primitive(Primitive::Ascii);
        let valor = self.types.primitive(Primitive::Valor);
        let numerus = self.types.primitive(Primitive::Numerus);
        let status_ty = crate::builtins::frame_types::status_type_id(self.types, self.resolver)
            .unwrap_or_else(|| self.vacuum_type());
        let nihil = self.types.primitive(Primitive::Nihil);
        let textus_nihil = self.types.intern(Type::Union(vec![textus, nihil]));
        let valor_nihil = self.types.intern(Type::Union(vec![valor, nihil]));

        let mut fields = FxHashMap::default();
        let field_tys = [
            textus,
            textus_nihil,
            ascii,
            status_ty,
            valor,
            numerus,
            textus_nihil,
            valor_nihil,
        ];
        let field_names = [
            "id",
            "parent_id",
            "call",
            "status",
            "data",
            "created_ms",
            "from",
            "trace",
        ];
        for ((name, ty), field_name) in self
            .resolver
            .builtin_scrinium_field_symbols()
            .iter()
            .zip(field_tys)
            .zip(field_names)
        {
            let sponte = crate::builtins::frame_types::scrinium_field_sponte(field_name);
            fields.insert(
                *name,
                StructFieldInfo {
                    ty,
                    optional: sponte,
                    required: !sponte,
                    span,
                },
            );
        }
        self.structs.insert(
            def_id,
            StructInfo {
                fields,
                methods: FxHashMap::default(),
                method_spans: FxHashMap::default(),
                is_abstract: false,
                own_bodyless: FxHashSet::default(),
                own_bodied: FxHashSet::default(),
                bodyless_methods: FxHashSet::default(),
            },
        );
    }

    fn collect_builtin_status(&mut self) {
        let Some(def_id) = self.resolver.builtin_status_def_id() else {
            return;
        };
        for (variant_def, _name) in self.resolver.builtin_status_variants(def_id) {
            self.variant_fields.insert(variant_def, Vec::new());
            self.variant_field_names.insert(variant_def, Vec::new());
            self.variant_parent.insert(variant_def, def_id);
            self.types.variant_parent.insert(variant_def, def_id);
        }
    }

    fn collect_builtin_sermo(&mut self) {
        let Some(def_id) = self.resolver.builtin_sermo_def_id() else {
            return;
        };
        self.structs.insert(
            def_id,
            empty_struct_info(),
        );
    }

    fn collect_builtin_directional_view(&mut self, lookup: fn(&Resolver) -> Option<DefId>) {
        let Some(def_id) = lookup(self.resolver) else {
            return;
        };
        self.structs.insert(
            def_id,
            empty_struct_info(),
        );
    }

    fn collect_builtin_forma(&mut self) {
        let Some(def_id) = self.resolver.builtin_forma_def_id() else {
            return;
        };

        let textus = self.types.primitive(Primitive::Textus);
        let valor = self.types.primitive(Primitive::Valor);
        let params_ty = self.types.array(valor);
        let span = radix_lexer::Span::default();

        let mut fields = FxHashMap::default();
        if let (Some(template), Some(params)) = (
            self.resolver.builtin_forma_template_field(),
            self.resolver.builtin_forma_params_field(),
        ) {
            fields.insert(
                template,
                StructFieldInfo {
                    ty: textus,
                    optional: false,
                    required: true,
                    span,
                },
            );
            fields.insert(
                params,
                StructFieldInfo {
                    ty: params_ty,
                    optional: false,
                    required: true,
                    span,
                },
            );
        }

        if !fields.is_empty() {
            self.structs.insert(
                def_id,
                StructInfo {
                    fields,
                    methods: FxHashMap::default(),
                    method_spans: FxHashMap::default(),
                    is_abstract: false,
                    own_bodyless: FxHashSet::default(),
                    own_bodied: FxHashSet::default(),
                    bodyless_methods: FxHashSet::default(),
                },
            );
        }
    }

    fn collect_enum(&mut self, enum_item: &radix_hir::HirEnum, enum_def: DefId) {
        let commune = self.resolver.commune_fields(enum_def);
        for variant in &enum_item.variants {
            let mut fields: Vec<TypeId> = commune.iter().map(|field| field.ty).collect();
            fields.extend(variant.fields.iter().map(|f| f.ty));
            let mut field_names: Vec<Symbol> = commune.iter().map(|field| field.name).collect();
            field_names.extend(variant.fields.iter().map(|f| f.name));
            self.variant_fields.insert(variant.def_id, fields);
            self.variant_field_names.insert(variant.def_id, field_names);
            self.variant_parent.insert(variant.def_id, enum_def);
            self.types.variant_parent.insert(variant.def_id, enum_def);
        }
        self.type_params_by_def.insert(
            enum_def,
            enum_item
                .type_params
                .iter()
                .map(|param| param.name)
                .collect(),
        );
    }

    /// Populate all top-level typecheck lookup tables before checking bodies.
    ///
    /// This pass is intentionally shallow. It records declarations that later
    /// expression checks need, but leaves initializer, method body, and function
    /// body validation to the main traversal so errors are reported with the
    /// correct local scope and return/error context.
    pub fn collect_items(&mut self, hir: &HirModule) {
        self.collect_builtin_scrinium();
        self.collect_builtin_status();
        self.collect_builtin_sermo();
        self.collect_builtin_directional_view(crate::scope::Resolver::builtin_meus_def_id);
        self.collect_builtin_directional_view(crate::scope::Resolver::builtin_tuus_def_id);
        self.collect_builtin_forma();

        for item in &hir.items {
            match &item.kind {
                HirItemKind::Function(func) => {
                    let sig = self.function_signature(func);
                    self.functions.insert(item.def_id, sig);
                    if let Some(role) = func.nucleum_role() {
                        self.nucleum_roles.insert(item.def_id, role);
                    }
                }
                HirItemKind::Import(import) => {
                    // Selective members (EOT-2M) are caller-owned value locals
                    // with interface-supplied types, not namespace receivers;
                    // keeping them out of `import_receivers` lets their real
                    // function/const types govern call and use sites.
                    if !import.selective {
                        for import_item in &import.items {
                            self.import_receivers.insert(import_item.def_id);
                        }
                    }
                }
                HirItemKind::Struct(struct_item) => {
                    self.collect_struct(item.def_id, struct_item, item.span)
                }
                HirItemKind::Interface(interface_item) => {
                    self.collect_interface(item.def_id, interface_item)
                }
                HirItemKind::Enum(enum_item) => self.collect_enum(enum_item, item.def_id),
                HirItemKind::Constant(const_item) => {
                    if let Some(ty) = const_item.ty {
                        self.consts.insert(item.def_id, ty);
                    }
                }
                HirItemKind::TypeAlias(alias) => {
                    self.alias_type_params.insert(
                        item.def_id,
                        alias.type_params.iter().map(|param| param.name).collect(),
                    );
                }
                HirItemKind::Schema(_) => {
                    // Schema decls are type-level relational metadata with no
                    // runtime semantics; there is nothing to collect for
                    // typecheck. Semantic type-namespace integration is the
                    // resolution unit's (S2-U2).
                }
            }
        }

        // U1: heritage resolution runs after every local genus and implendum
        // is collected so kind checks, the transitive walk, and the merged
        // views are declaration-order independent.
        self.resolve_struct_heritage();
    }

    /// Check and consume `HirStruct` heritage: kind checks, cycle detection,
    /// merged member views, override/field-redeclaration diagnostics, and
    /// concrete-completeness.
    ///
    /// Merging is compositional: each genus merges its parent's already-merged
    /// view, so a parent's own ancestors arrive transitively and nearest
    /// declarations win. Kind-invalid edges are dropped before the walk so a
    /// bad clause does not cascade into member-merge noise.
    fn resolve_struct_heritage(&mut self) {
        let order = self.struct_order.clone();

        // KIND: `extends` names a genus; `implet` names an implendum.
        for (def_id, span) in &order {
            let Some(mut heritage) = self.struct_heritage.get(def_id).cloned() else {
                continue;
            };
            if let Some(parent) = heritage.extends {
                if !self.structs.contains_key(&parent) {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_EXTENDS_REQUIRES_GENUS,
                        *span,
                    );
                    heritage.extends = None;
                }
            }
            let mut implements = heritage.implements.clone();
            implements.retain(|iface| self.interfaces.contains_key(iface));
            let dropped = heritage.implements.len() - implements.len();
            heritage.implements = implements;
            if dropped > 0 {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_IMPLET_REQUIRES_IMPLENDUM,
                    *span,
                );
            }
            self.struct_heritage.insert(*def_id, heritage);
        }

        // CYCLE: transitive extends chain per genus, cut at the closing edge.
        for (def_id, span) in &order {
            let mut chain: Vec<DefId> = Vec::new();
            let mut seen: FxHashSet<DefId> = FxHashSet::default();
            let mut cursor = *def_id;
            loop {
                let Some(parent) = self
                    .struct_heritage
                    .get(&cursor)
                    .and_then(|heritage| heritage.extends)
                else {
                    break;
                };
                if parent == *def_id || seen.contains(&parent) {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_HERITAGE_CYCLE,
                        *span,
                    );
                    // Cut the walk at the closing edge; the cycle members
                    // already carry the diagnostic.
                    if let Some(heritage) = self.struct_heritage.get_mut(&cursor) {
                        heritage.extends = None;
                    }
                    break;
                }
                seen.insert(parent);
                chain.push(parent);
                cursor = parent;
            }
            self.ancestor_structs.insert(*def_id, chain);
        }

        // MERGE: parent-first compositional views (own members win).
        let mut merged: FxHashSet<DefId> = FxHashSet::default();
        for (def_id, _) in &order {
            self.merge_heritage_into(*def_id, &mut merged);
        }

        // IMPLEMENTS closure: own plus every ancestor's declaration.
        for (def_id, _) in &order {
            let mut interfaces: FxHashSet<DefId> = FxHashSet::default();
            if let Some(heritage) = self.struct_heritage.get(def_id) {
                interfaces.extend(heritage.implements.iter().copied());
            }
            if let Some(ancestors) = self.ancestor_structs.get(def_id) {
                for ancestor in ancestors {
                    if let Some(heritage) = self.struct_heritage.get(ancestor) {
                        interfaces.extend(heritage.implements.iter().copied());
                    }
                }
            }
            self.implemented_interfaces.insert(*def_id, interfaces);
        }

        // COMPLETENESS: a concrete genus implements every bodyless method in
        // its full heritage closure.
        for (def_id, span) in &order {
            let Some(info) = self.structs.get(def_id) else {
                continue;
            };
            if info.is_abstract || info.bodyless_methods.is_empty() {
                continue;
            }
            let unsatisfied: Vec<Symbol> = info.bodyless_methods.iter().copied().collect();
            for name in unsatisfied {
                let name_text = self
                    .interner
                    .map(|interner| interner.resolve(name).to_owned())
                    .unwrap_or_default();
                self.errors.push(
                    SemanticError::structured(SemanticErrorKind::TypeMismatch, *span)
                        .with_arg("issue", ISSUE_ABSTRACT_METHOD_UNIMPLEMENTED)
                        .with_arg("method", name_text),
                );
            }
        }
    }

    /// Merge the parent's (already-merged) view into one genus's view.
    ///
    /// Own field redeclarations and incompatible method overrides are
    /// diagnosed here; own declarations always win the merged slot. The
    /// child's view is taken out of the table while merging so diagnostics can
    /// be emitted mid-merge without aliasing the member maps.
    fn merge_heritage_into(&mut self, def_id: DefId, merged: &mut FxHashSet<DefId>) {
        if merged.contains(&def_id) {
            return;
        }
        merged.insert(def_id);
        let parent = self
            .struct_heritage
            .get(&def_id)
            .and_then(|heritage| heritage.extends)
            .filter(|parent| self.structs.contains_key(parent));
        if let Some(parent) = parent {
            self.merge_heritage_into(parent, merged);
        }

        let mut info = self
            .structs
            .remove(&def_id)
            .expect("collected struct present during heritage merge");

        let mut diagnostics: Vec<SemanticError> = Vec::new();
        if let Some(parent) = parent {
            let parent_info = self.structs.get(&parent).expect("parent collected");
            let parent_fields = parent_info.fields.clone();
            let parent_methods = parent_info.methods.clone();
            let parent_method_spans = parent_info.method_spans.clone();
            let parent_bodyless = parent_info.bodyless_methods.clone();
            for (name, field) in parent_fields {
                if info.fields.contains_key(&name) {
                    // (a): no field shadowing under `sub` — the child's own
                    // declaration is the diagnostic site.
                    let span = info
                        .fields
                        .get(&name)
                        .map(|own| own.span)
                        .unwrap_or_default();
                    diagnostics.push(
                        SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, span)
                            .with_arg("issue", ISSUE_FIELD_REDECLARATION),
                    );
                } else {
                    info.fields.insert(name, field);
                }
            }
            for (name, sig) in parent_methods {
                if let Some(own_sig) = info.methods.get(&name).cloned() {
                    if !self.override_compatible(&own_sig, &sig) {
                        let span = info
                            .method_spans
                            .get(&name)
                            .copied()
                            .unwrap_or_default();
                        diagnostics.push(
                            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                                .with_arg("issue", ISSUE_OVERRIDE_SIGNATURE_INCOMPATIBLE),
                        );
                    }
                } else {
                    info.method_spans
                        .entry(name)
                        .or_insert_with(|| parent_method_spans.get(&name).copied().unwrap_or_default());
                    info.methods.insert(name, sig);
                }
            }
            // Bodyless propagation: inherited unsatisfied names minus the ones
            // this genus provides bodies for.
            let mut bodyless = info.own_bodyless.clone();
            bodyless.extend(parent_bodyless.iter().copied());
            for name in info.own_bodied.iter() {
                bodyless.remove(name);
            }
            info.bodyless_methods = bodyless;
        } else {
            info.bodyless_methods = info.own_bodyless.clone();
        }

        self.structs.insert(def_id, info);
        self.errors.extend(diagnostics);
    }

    /// (a): override compatibility — same parameter count and modes, and
    /// assignable return/error channels. Parameter types follow the ruling's
    /// explicit list and are not compared.
    fn override_compatible(&self, child: &FuncSig, parent: &FuncSig) -> bool {
        if child.params.len() != parent.params.len() {
            return false;
        }
        for (child_param, parent_param) in child.params.iter().zip(parent.params.iter()) {
            if child_param.mode != parent_param.mode
                || child_param.optional != parent_param.optional
            {
                return false;
            }
        }
        if !self.assignable_with_heritage(child.ret, parent.ret) {
            return false;
        }
        match (child.err, parent.err) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(child_err), Some(parent_err)) => {
                self.assignable_with_heritage(child_err, parent_err)
            }
        }
    }
}

const ISSUE_EXTENDS_REQUIRES_GENUS: &str = "extends_requires_genus";
const ISSUE_IMPLET_REQUIRES_IMPLENDUM: &str = "implet_requires_implendum";
const ISSUE_HERITAGE_CYCLE: &str = "heritage_cycle";
const ISSUE_FIELD_REDECLARATION: &str = "field_redeclaration";
const ISSUE_OVERRIDE_SIGNATURE_INCOMPATIBLE: &str = "override_signature_incompatible";
const ISSUE_ABSTRACT_METHOD_UNIMPLEMENTED: &str = "abstract_method_unimplemented";
