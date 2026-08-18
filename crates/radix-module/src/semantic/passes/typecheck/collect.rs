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

impl TypeChecker<'_> {
    pub(super) fn callable_boundary_return(
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
            .map(|param| ParamType { ty: param.ty, mode: param_mode_from_hir(param.mode), optional: param.optional })
            .collect()
    }

    /// Build the callable contract for a lowered function item or method.
    ///
    /// Missing return annotations are treated as `vacuum` at the signature
    /// boundary. Body checking may still synthesize return flow internally, but
    /// call sites need a concrete contract before any body is visited.
    pub(super) fn function_signature(&mut self, func: &HirFunction) -> FuncSig {
        let params = self.param_types(&func.params);
        let ret = func.ret_ty.unwrap_or_else(|| self.vacuum_type());
        let (ret, err) = self.callable_boundary_return(ret, func.err_ty, func.is_async, func.is_generator);
        FuncSig {
            type_params: func.type_params.iter().map(|param| param.name).collect(),
            type_param_constraints: func
                .type_params
                .iter()
                .map(|param| match &param.constraint {
                    crate::hir::HirTypeParamConstraint::Any => TypeParamConstraint::Any,
                    crate::hir::HirTypeParamConstraint::OneOf(allowed) => TypeParamConstraint::OneOf(allowed.clone()),
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
    pub(super) fn collect_struct(&mut self, def_id: DefId, struct_item: &HirStruct) {
        let mut fields = FxHashMap::default();
        let mut methods = FxHashMap::default();

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
        self.structs.insert(def_id, StructInfo { fields, methods });
    }

    fn collect_interface(&mut self, def_id: DefId, interface_item: &crate::hir::HirInterface) {
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
        let span = crate::lexer::Span::default();
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
            fields.insert(*name, StructFieldInfo { ty, optional: sponte, required: !sponte, span });
        }
        self.structs
            .insert(def_id, StructInfo { fields, methods: FxHashMap::default() });
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
            StructInfo { fields: FxHashMap::default(), methods: FxHashMap::default() },
        );
    }

    fn collect_builtin_directional_view(&mut self, lookup: fn(&Resolver) -> Option<DefId>) {
        let Some(def_id) = lookup(self.resolver) else {
            return;
        };
        self.structs.insert(
            def_id,
            StructInfo { fields: FxHashMap::default(), methods: FxHashMap::default() },
        );
    }

    fn collect_builtin_forma(&mut self) {
        let Some(def_id) = self.resolver.builtin_forma_def_id() else {
            return;
        };

        let textus = self.types.primitive(Primitive::Textus);
        let valor = self.types.primitive(Primitive::Valor);
        let params_ty = self.types.array(valor);
        let span = crate::lexer::Span::default();

        let mut fields = FxHashMap::default();
        if let (Some(template), Some(params)) = (
            self.resolver.builtin_forma_template_field(),
            self.resolver.builtin_forma_params_field(),
        ) {
            fields.insert(template, StructFieldInfo { ty: textus, optional: false, required: true, span });
            fields.insert(params, StructFieldInfo { ty: params_ty, optional: false, required: true, span });
        }

        if !fields.is_empty() {
            self.structs
                .insert(def_id, StructInfo { fields, methods: FxHashMap::default() });
        }
    }

    fn collect_enum(&mut self, enum_item: &crate::hir::HirEnum, enum_def: DefId) {
        for variant in &enum_item.variants {
            let fields = variant.fields.iter().map(|f| f.ty).collect();
            let field_names = variant.fields.iter().map(|f| f.name).collect();
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
    pub(super) fn collect_items(&mut self, hir: &HirModule) {
        self.collect_builtin_scrinium();
        self.collect_builtin_status();
        self.collect_builtin_sermo();
        self.collect_builtin_directional_view(crate::semantic::scope::Resolver::builtin_meus_def_id);
        self.collect_builtin_directional_view(crate::semantic::scope::Resolver::builtin_tuus_def_id);
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
                    for import_item in &import.items {
                        self.import_receivers.insert(import_item.def_id);
                    }
                }
                HirItemKind::Struct(struct_item) => self.collect_struct(item.def_id, struct_item),
                HirItemKind::Interface(interface_item) => self.collect_interface(item.def_id, interface_item),
                HirItemKind::Enum(enum_item) => self.collect_enum(enum_item, item.def_id),
                HirItemKind::Constant(const_item) => {
                    if let Some(ty) = const_item.ty {
                        self.consts.insert(item.def_id, ty);
                    }
                }
                HirItemKind::TypeAlias(alias) => {
                    self.alias_type_params
                        .insert(item.def_id, alias.type_params.iter().map(|param| param.name).collect());
                }
                HirItemKind::Schema(_) => {
                    // Schema decls are type-level relational metadata with no
                    // runtime semantics; there is nothing to collect for
                    // typecheck. Semantic type-namespace integration is the
                    // resolution unit's (S2-U2).
                }
            }
        }
    }
}
