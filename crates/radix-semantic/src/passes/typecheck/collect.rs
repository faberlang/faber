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
use crate::passes::filum_independence::is_inlinable_kernel;

/// A struct contract with no fields, methods, or member-visibility overrides.
/// Builtin nominal receivers start from this; a builtin with field contracts
/// fills in `fields` only.
fn empty_struct_info() -> StructInfo {
    StructInfo {
        fields: FxHashMap::default(),
        methods: FxHashMap::default(),
        method_visibility: FxHashMap::default(),
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
            ret = self.types.cursor(ret);
        }
        (ret, err)
    }

    fn param_types<'b>(params: impl IntoIterator<Item = &'b HirParam>) -> Vec<ParamType> {
        params
            .into_iter()
            .map(|param| ParamType {
                ty: param.ty,
                mode: param_mode_from_hir(param.mode),
                optional: param.optional,
                sponte: param.sponte,
            })
            .collect()
    }

    /// Remember which parameters of `func` are declared `axis` (a marker, not
    /// a type: the parameter's type stays an ordinary int).
    fn record_axis_params(&mut self, def_id: radix_hir::DefId, func: &HirFunction) {
        self.axis_params.insert(
            def_id,
            func.params
                .iter()
                .map(|param| param.mode == radix_hir::HirParamMode::Axis)
                .collect(),
        );
    }

    /// Build the callable contract for a lowered function item or method.
    ///
    /// Missing return annotations are treated as `vacuum` at the signature
    /// boundary. Body checking may still synthesize return flow internally, but
    /// call sites need a concrete contract before any body is visited.
    pub fn function_signature(&mut self, func: &HirFunction) -> FuncSig {
        let params = Self::param_types(&func.params);
        let ret = func.ret_ty.unwrap_or_else(|| self.vacuum_type());
        let (ret, err) =
            self.callable_boundary_return(ret, func.err_ty, func.is_async, func.is_generator);
        FuncSig {
            type_params: HirTypeParam::descriptors(&func.type_params),
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
    pub fn collect_struct(
        &mut self,
        def_id: DefId,
        annotations: &[radix_hir::HirAnnotation],
        struct_item: &HirStruct,
        span: radix_lexer::Span,
    ) {
        let mut fields = FxHashMap::default();
        let mut methods = FxHashMap::default();

        if self.annotations_contain_json(annotations) {
            self.json_structs.insert(def_id);
        }

        for field in &struct_item.fields {
            // A `sponte` field's nullable carrier (`Option<T>`) is part of the
            // field's representation contract with the LLVM lane, which lays
            // nullable fields out through a carrier `TypeId` it cannot intern
            // itself (the arena is immutable at emit time). Field access
            // interns exactly this carrier (`Typechecker::field_type_from_type`);
            // interning it here too keeps the carrier present even when no
            // body ever reads the field, instead of refusing emission for a
            // program whose only fault is leaving an optional field unread.
            if field.sponte {
                self.types.option(field.ty);
            }
            fields.insert(
                field.name,
                StructFieldInfo {
                    ty: field.ty,
                    optional: field.sponte,
                    required: !field.sponte && field.init.is_none(),
                    span: field.span,
                    modifier: field.modifier,
                    visibility: self.hir_member_visibility(&field.annotations),
                },
            );
        }

        let mut method_visibility = FxHashMap::default();
        for method in &struct_item.methods {
            let sig = self.function_signature(&method.func);
            self.record_axis_params(method.def_id, &method.func);
            methods.insert(method.func.name, sig);
            self.method_defs
                .insert((def_id, method.func.name), method.def_id);
            self.record_method_availability(def_id, method.func.name, &method.annotations);
            if method.receiver != radix_hir::HirReceiver::None {
                self.instance_method_defs.insert((def_id, method.func.name));
            }
            let visibility = self.hir_member_visibility(&method.annotations);
            if visibility != radix_syntax::MemberVisibility::Publica {
                method_visibility.insert(method.func.name, visibility);
            }
            if let Some(role) = method.func.nucleum_role() {
                self.nucleum_roles.insert(method.def_id, role);
                self.method_nucleum_roles
                    .insert((def_id, method.func.name), role);
                if is_inlinable_kernel(&method.func) {
                    self.inlinable_kernels.insert(method.def_id);
                }
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
        self.instance_field_counts.insert(
            def_id,
            struct_item
                .fields
                .iter()
                .filter(|field| !field.is_static())
                .count(),
        );

        self.struct_implements
            .push((def_id, span, struct_item.implements.clone()));

        self.structs.insert(
            def_id,
            StructInfo {
                fields,
                methods,
                method_visibility,
            },
        );
    }

    fn collect_interface(&mut self, def_id: DefId, interface_item: &radix_hir::HirInterface) {
        let mut methods = FxHashMap::default();

        for method in &interface_item.methods {
            let params = Self::param_types(&method.params);
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
                        params,
                        ret,
                        err,
                        is_async: method.posture.is_async(),
                        is_generator: method.posture.is_stream(),
                    },
                    axis: method
                        .params
                        .iter()
                        .map(|param| param.mode == radix_hir::HirParamMode::Axis)
                        .collect(),
                    availability: method.availability.clone(),
                    span: method.span,
                },
            );
        }

        self.interfaces.insert(def_id, methods);
        // Contract arity and argument substitution (D1.2) read the
        // implendum's own parameter row.
        self.type_params_by_def.insert(
            def_id,
            interface_item
                .type_params
                .iter()
                .map(|param| param.name)
                .collect(),
        );
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
            // Same carrier contract as a user `sponte` field (see
            // `collect_struct`): the LLVM lane lays nullable fields out
            // through the `Option<T>` carrier, so the builtin table interns it
            // for every nullable scrinium field, not only the ones a body
            // reads.
            if sponte {
                self.types.option(ty);
            }
            fields.insert(
                *name,
                StructFieldInfo {
                    ty,
                    optional: sponte,
                    required: !sponte,
                    span,
                    modifier: None,
                    visibility: radix_syntax::MemberVisibility::Publica,
                },
            );
        }
        self.structs.insert(
            def_id,
            StructInfo {
                fields,
                ..empty_struct_info()
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
        self.structs.insert(def_id, empty_struct_info());
    }

    fn collect_builtin_directional_view(&mut self, lookup: fn(&Resolver) -> Option<DefId>) {
        let Some(def_id) = lookup(self.resolver) else {
            return;
        };
        self.structs.insert(def_id, empty_struct_info());
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
                    modifier: None,
                    visibility: radix_syntax::MemberVisibility::Publica,
                },
            );
            fields.insert(
                params,
                StructFieldInfo {
                    ty: params_ty,
                    optional: false,
                    required: true,
                    span,
                    modifier: None,
                    visibility: radix_syntax::MemberVisibility::Publica,
                },
            );
        }

        if !fields.is_empty() {
            self.structs.insert(
                def_id,
                StructInfo {
                    fields,
                    ..empty_struct_info()
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
                    self.record_axis_params(item.def_id, func);
                    self.functions.insert(item.def_id, sig);
                    self.record_function_availability(item.def_id, &item.annotations);
                    if let Some(role) = func.nucleum_role() {
                        self.nucleum_roles.insert(item.def_id, role);
                        if is_inlinable_kernel(func) {
                            self.inlinable_kernels.insert(item.def_id);
                        }
                    }
                }
                HirItemKind::Import(import) if import.target.is_some() => {
                    self.collect_target_import(import, item.span);
                }
                HirItemKind::Import(import) => {
                    // Selective members (EOT-2M) are caller-owned value locals
                    // with interface-supplied types, not namespace receivers;
                    // keeping them out of `import_receivers` lets their real
                    // function/const types govern call and use sites.
                    if import.selective {
                        self.record_ordering_contract_bindings(import);
                    } else {
                        for import_item in &import.items {
                            self.import_receivers.insert(import_item.def_id);
                        }
                    }
                }
                HirItemKind::Struct(struct_item) => {
                    self.collect_struct(item.def_id, &item.annotations, struct_item, item.span);
                }
                HirItemKind::Interface(interface_item) => {
                    self.collect_interface(item.def_id, interface_item);
                    self.register_local_contract_role(
                        item.def_id,
                        &item.annotations,
                        interface_item,
                        item.span,
                    );
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

        // Need: module-level `const int K = ...` bindings are valid
        // `axis` arguments. Values are not folded by `statica` until after
        // typecheck, so fold here; references may precede their declaration,
        // so iterate to a fixpoint. The axis reads the value the declared cell
        // holds (`stored_const_int`), so only an integer cell contributes and
        // a constant chained through another constant reads the stored value;
        // a cycle or a non-constant stays absent.
        loop {
            let mut progressed = false;
            for item in &hir.items {
                // Only a legal module constant (`const ... = ...`) qualifies;
                // a top-level `var` or arrow binding is rejected elsewhere.
                let HirItemKind::Constant(const_item) = &item.kind else {
                    continue;
                };
                if const_item.statica.is_none() || const_item.mutable {
                    continue;
                }
                if self.const_int_values.contains_key(&item.def_id) {
                    continue;
                }
                let value = super::integer_range::const_int_value_resolved(
                    &const_item.value,
                    &self.const_int_values,
                );
                if let Some(value) = value
                    && let Some(stored) = self.stored_const_int(const_item.ty, value)
                {
                    self.const_int_values.insert(item.def_id, stored);
                    self.module_const_ints.insert(item.def_id);
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }

        // Contract kind checks run after every local genus and implendum is
        // collected so they are declaration-order independent.
        self.resolve_struct_implements();
    }

    /// Record the `@ nondum` gates of a local free function so its uses can be
    /// refused. A function without the annotation records nothing.
    fn record_function_availability(
        &mut self,
        def_id: DefId,
        annotations: &[radix_hir::HirAnnotation],
    ) {
        if let Some(gates) = self.nondum_gates(annotations) {
            self.unavailable_functions.insert(def_id, gates);
        }
    }

    /// Record the `@ nondum` gates of a local `genus` method so its calls can
    /// be refused. A method without the annotation records nothing.
    fn record_method_availability(
        &mut self,
        owner: DefId,
        name: Symbol,
        annotations: &[radix_hir::HirAnnotation],
    ) {
        if let Some(gates) = self.nondum_gates(annotations) {
            self.unavailable_methods.insert((owner, name), gates);
        }
    }

    /// The portable `@ nondum` gates written in `annotations`, if any.
    fn nondum_gates(
        &self,
        annotations: &[radix_hir::HirAnnotation],
    ) -> Option<Vec<crate::file_interface::InterfaceUnavailable>> {
        let interner = self.interner?;
        let availability: Vec<radix_hir::HirAvailability> = annotations
            .iter()
            .filter_map(|annotation| crate::try_parse_nondum_annotation(annotation, interner))
            .collect();
        (!availability.is_empty()).then(|| {
            crate::file_interface::InterfaceUnavailable::from_availability(&availability, interner)
        })
    }

    /// Kind-check each genus's `implet` clause and record its contracts.
    ///
    /// `implet` must name an implendum; other targets are dropped with one
    /// diagnostic per genus. There is no class inheritance (DECISIONS D8.1), so
    /// a genus conforms to exactly the contracts it declares.
    fn resolve_struct_implements(&mut self) {
        let declared = std::mem::take(&mut self.struct_implements);
        for (def_id, span, implements) in &declared {
            let contracts: Vec<radix_types::ContractRef> = implements
                .iter()
                .filter(|contract| self.validate_contract_ref(contract, *span))
                .cloned()
                .collect();
            self.implemented_interfaces.insert(
                *def_id,
                contracts.iter().map(|contract| contract.contract).collect(),
            );
            self.check_impled_conformance(*def_id, *span, &contracts);
            self.implemented_contracts.insert(*def_id, contracts);
        }
        self.struct_implements = declared;
    }
}

impl TypeChecker<'_> {
    /// Collect a target-native import (`import python "math" m`).
    ///
    /// Every bound name is a trusted `ignotum` receiver: calls and member
    /// access are admitted unchecked and the target's own compiler or runtime
    /// is the judge. The program is bound to the import's target, so a
    /// different build target is SEM017 `import_target_bound`.
    fn collect_target_import(&mut self, import: &radix_hir::HirImport, span: radix_lexer::Span) {
        for import_item in &import.items {
            self.import_receivers.insert(import_item.def_id);
            self.trusted_imports.insert(import_item.def_id);
        }
        // A FHIR artifact is written for no build target, so analysis for it
        // never binds the program; the consumer's qualification does.
        if self.target == radix_target::Target::HirFhir {
            return;
        }
        let Some(interner) = self.interner else {
            return;
        };
        if let Some(error) =
            crate::passes::import_target::mismatch(import, span, interner, self.target)
        {
            self.errors.push(error);
        }
    }
}
