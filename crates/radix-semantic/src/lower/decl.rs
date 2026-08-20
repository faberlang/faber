//! Declaration lowering for item-shaped HIR.
//!
//! This file owns the item boundary of HIR lowering: top-level Faber
//! declarations become `HirItem`s with stable names, resolver-backed or
//! synthetic definition ids, lowered type annotations, and bodies normalized
//! enough for type checking. Statement and expression lowering remain in their
//! sibling modules; declarations coordinate those pieces because item shape,
//! local parameter scope, and member ownership have to be established together.
//!
//! INVARIANTS
//! ==========
//! - Top-level item names prefer resolver `DefId`s; missing resolver entries use
//!   synthetic ids so lowering can keep producing diagnostics.
//! - Function and method parameters are bound before bodies lower, because body
//!   expressions resolve locals through the lowerer scope stack.
//! - Type syntax is lowered into semantic type ids, but type compatibility and
//!   interface conformance are deferred to later semantic/typecheck passes.
//! - Declaration spans stay attached to the resulting HIR item, while fields,
//!   variants, parameters, and synthesized locals keep their narrower spans.
//! - `@vertex` / `@fragment` functions receive implicit runtime-provided locals
//!   so bodies can name stage I/O through the resolver.
//!
//! ERROR STRATEGY
//! ==============
//! This pass reports declaration-shape errors, such as unsupported top-level
//! bindings or missing base/interface names, then builds the best HIR it can.
//! Top-level variables without source initializers receive an error expression
//! so downstream passes can continue from the declaration.
//!
//! MEMBER POLICY
//! =============
//! `gens` members are lowered in one pass so fields, methods, current `ego`
//! context, modifier locals, and inherited/implemented type references stay on
//! the same HIR struct. `ordo` and `discretio` both lower to enum-shaped HIR:
//! plain `ordo` variants have no payload fields, while `discretio` preserves
//! payload type information for later pattern and type analysis.

use super::Lowerer;
use crate::{IndexExpr, NumericWidth, Primitive, Type, TypeId};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{
    DefId, HirAnnotation, HirBlock, HirConst, HirEnum, HirExpression, HirExpressionKind, HirField,
    HirFunction, HirImport, HirImportItem, HirInterface, HirInterfaceMethod, HirItem, HirItemKind,
    HirLocal, HirMethod, HirOwnerRef, HirParam, HirParamMode, HirReceiver, HirSchema,
    HirSchemaColumn, HirStatement, HirStatementKind, HirStruct, HirTriviaOwnerKind, HirTypeAlias,
    HirTypeParam, HirTypeParamConstraint, HirVariant, HirVariantField,
};
use radix_lexer::{Span, Symbol};
use radix_syntax::{
    AstMutability, AstParamMode, BindingPattern, CallablePosture, ClassDecl, ClassMemberKind,
    EnumDecl, FuncDecl, ImportDecl, ImportKind, InterfaceDecl, InterfaceMethod, Param, SchemaDecl,
    Stmt, TypeAliasDecl, TypeParam, UnionDecl, VarDecl, Variant,
};

type ModifierLocal = (DefId, Symbol, Span);
type ShaderLocal = (DefId, Symbol, Span, TypeId, bool);
const ISSUE_CALLABLE_POSTURE_CONFLICT: &str = "callable_posture_conflict";

fn error_expr(lowerer: &mut Lowerer, span: Span) -> HirExpression {
    HirExpression {
        id: lowerer.next_hir_id(),
        kind: HirExpressionKind::Error,
        ty: None,
        span,
    }
}

fn lower_param_mode(mode: AstParamMode) -> HirParamMode {
    match mode {
        AstParamMode::Owned => HirParamMode::Owned,
        AstParamMode::De => HirParamMode::De,
        AstParamMode::In => HirParamMode::In,
        AstParamMode::Ex => HirParamMode::Ex,
    }
}

fn callable_posture_label(posture: CallablePosture) -> &'static str {
    match posture {
        CallablePosture::SyncFinite => "sync_finite",
        CallablePosture::AsyncFinite => "async_finite",
        CallablePosture::SyncStream => "sync_stream",
        CallablePosture::AsyncStream => "async_stream",
    }
}

impl Lowerer<'_> {
    // CALLABLE POSTURE

    fn resolve_callable_posture(
        &mut self,
        written: CallablePosture,
        annotations: &[HirAnnotation],
        span: Span,
    ) -> CallablePosture {
        let annotation_async = Self::annotation_family_is(annotations, self.interner, "futura");
        let annotation_generator = Self::annotation_family_is(annotations, self.interner, "cursor");
        let annotated = match (annotation_async, annotation_generator) {
            (false, false) => None,
            (true, false) => Some(CallablePosture::AsyncFinite),
            (false, true) => Some(CallablePosture::SyncStream),
            (true, true) => Some(CallablePosture::AsyncStream),
        };

        let Some(annotated) = annotated else {
            return written;
        };

        if written == CallablePosture::SyncFinite || written == annotated {
            return annotated;
        }

        self.error_with_args_at(
            vec![
                DiagnosticArg::new("issue", ISSUE_CALLABLE_POSTURE_CONFLICT),
                DiagnosticArg::new("annotation_posture", callable_posture_label(annotated)),
                DiagnosticArg::new("written_posture", callable_posture_label(written)),
            ],
            span,
        );
        written
    }

    // PARAMETER AND MODIFIER SCOPE

    fn lower_type_params(&mut self, params: &[TypeParam]) -> Vec<HirTypeParam> {
        params
            .iter()
            .map(|param| HirTypeParam {
                def_id: self.next_def_id(),
                kind: param.kind,
                name: param.name.name,
                span: param.span,
                constraint: HirTypeParamConstraint::Any,
            })
            .collect()
    }

    fn lower_param(&mut self, param: &Param, use_alias: bool) -> HirParam {
        let def_id = self.next_def_id();
        let binding_name = if use_alias {
            param.alias.as_ref().unwrap_or(&param.name)
        } else {
            &param.name
        };
        let ty = self.lower_type(&param.ty);
        let mode = match param.ty.mode {
            Some(type_mode) => super::type_mode_hir_param_mode(type_mode),
            None => lower_param_mode(param.mode),
        };
        HirParam {
            def_id,
            name: binding_name.name,
            ty,
            mode,
            optional: param.sponte || param.default.is_some(),
            sponte: param.sponte,

            default: param
                .default
                .as_deref()
                .map(|default| self.lower_expr(default)),
            span: param.span,
        }
    }

    fn lower_cli_args_param(&mut self, decl: &FuncDecl) -> Option<HirParam> {
        command_argument_binding(&decl.modifiers).map(|ident| HirParam {
            def_id: self.next_def_id(),
            name: ident.name,
            ty: self.command_args_type(decl.name.name),
            mode: HirParamMode::Owned,
            optional: false,
            sponte: false,

            default: None,
            span: ident.span,
        })
    }

    fn bind_modifier_locals(
        &mut self,
        modifiers: &[radix_syntax::FuncModifier],
    ) -> Vec<ModifierLocal> {
        modifier_bindings(modifiers)
            .into_iter()
            .map(|ident| {
                let def_id = self.next_def_id();
                self.bind_local(ident.name, def_id);
                (def_id, ident.name, ident.span)
            })
            .collect()
    }

    fn prepend_modifier_locals(
        &mut self,
        mut body: HirBlock,
        modifier_locals: Vec<ModifierLocal>,
    ) -> HirBlock {
        for (def_id, name, span) in modifier_locals.into_iter().rev() {
            body.statements.insert(
                0,
                HirStatement {
                    id: self.next_hir_id(),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id,
                        name,
                        ty: Some(self.types.primitive(Primitive::Ignotum)),
                        init: None,
                        mutable: true,
                        is_await: false,
                        runtime_provided: true,
                    }),
                    span,
                },
            );
        }
        body
    }

    // SHADER STAGE LOCALS

    fn inject_vertex_shader_locals(&mut self) -> Vec<ShaderLocal> {
        let (vec3_ty, vec4_ty) = self.shader_stage_vec_types();
        let span = Span::default();
        vec![
            self.define_shader_local("position", vec3_ty, false, span),
            self.define_shader_local("out_position", vec4_ty, true, span),
        ]
    }

    fn inject_fragment_shader_locals(&mut self) -> Vec<ShaderLocal> {
        let (vec3_ty, vec4_ty) = self.shader_stage_vec_types();
        let span = Span::default();
        vec![
            self.define_shader_local("normal", vec3_ty, false, span),
            self.define_shader_local("color", vec3_ty, false, span),
            self.define_shader_local("out_color", vec4_ty, true, span),
        ]
    }

    fn shader_stage_vec_types(&mut self) -> (TypeId, TypeId) {
        let f32_ty = self
            .types
            .sized_numeric(Primitive::Fractus, NumericWidth::F32);
        let width3 = self.types.intern_index(IndexExpr::Literal(3));
        let width4 = self.types.intern_index(IndexExpr::Literal(4));
        (
            self.types.vector_with_width(f32_ty, width3),
            self.types.vector_with_width(f32_ty, width4),
        )
    }

    fn define_shader_local(
        &mut self,
        name: &str,
        ty: TypeId,
        mutable: bool,
        span: Span,
    ) -> ShaderLocal {
        let def_id = self.next_def_id();
        let name = self.interner.intern(name);
        // Register so typechecker's `check_path` fallback can `get_symbol(def_id)`.
        let _ = self.resolver.define(crate::Symbol {
            def_id,
            name,
            kind: crate::SymbolKind::Local,
            ty: Some(ty),
            mutable,
            span,
        });
        (def_id, name, span, ty, mutable)
    }

    fn prepend_runtime_provided_locals(
        &mut self,
        body: &mut HirBlock,
        locals: impl IntoIterator<Item = ShaderLocal>,
    ) {
        for (def_id, name, span, ty, mutable) in locals {
            body.statements.insert(
                0,
                HirStatement {
                    id: self.next_hir_id(),
                    kind: HirStatementKind::Local(HirLocal {
                        def_id,
                        name,
                        ty: Some(ty),
                        init: None,
                        mutable,
                        is_await: false,
                        runtime_provided: true,
                    }),
                    span,
                },
            );
        }
    }

    // ITEM LOWERING

    /// HIR has no top-level variable item, so `varia`/`fixum` become constants.
    /// Missing initializers still produce an error expression so later passes
    /// have a value node.
    pub fn lower_varia(&mut self, stmt: &Stmt, decl: &VarDecl) -> Option<HirItem> {
        let (name, name_span) = if let BindingPattern::Ident(ident) = &decl.binding {
            (ident.name, ident.span)
        } else {
            self.error("top_level_identifier_binding");
            return None;
        };

        let def_id = self.def_id_for(name);
        let ty = decl.ty.as_ref().map(|ty| self.lower_type(ty));
        let value = if let Some(init) = &decl.init {
            self.lower_expr(init)
        } else {
            self.error("top_level_initializer");
            error_expr(self, name_span)
        };

        let item = HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Constant(HirConst {
                name,
                ty,
                value,
                mutable: decl.mutability == AstMutability::Mutable,
                is_await: decl.is_await,
            }),
            span: stmt.span,
        };

        Some(item)
    }

    /// Bind parameters and modifiers before the body so locals resolve. CLI
    /// argument metadata becomes a synthetic parameter, not a separate HIR concept.
    pub fn lower_functio(&mut self, stmt: &Stmt, decl: &FuncDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);

        let annotations = self.lower_annotations(&stmt.annotations);
        let mut type_params = self.lower_type_params(&decl.type_params);
        self.push_type_params(type_params.iter().map(|param| (param.name, param.kind)));
        self.apply_type_param_constraints_from_annotations(&mut type_params, &annotations);
        let mut params = Vec::new();
        for param in &decl.params {
            params.push(self.lower_param(param, true));
        }
        let cli_args = self.lower_cli_args_param(decl);

        let ret_ty = decl.ret.as_ref().map(|ty| self.lower_type(ty));
        let err_ty = decl.err.as_ref().map(|ty| self.lower_type(ty));
        self.push_scope();
        for param in &params {
            self.bind_local(param.name, param.def_id);
        }
        if let Some(param) = &cli_args {
            self.bind_local(param.name, param.def_id);
        }
        let modifier_locals = self.bind_modifier_locals(&decl.modifiers);

        let is_vertex = stmt
            .annotations
            .iter()
            .any(|annotation| self.annotation_ast_family_is(annotation, "vertex"));
        let is_fragment = stmt
            .annotations
            .iter()
            .any(|annotation| self.annotation_ast_family_is(annotation, "fragment"));

        let vertex_locals = if is_vertex {
            self.inject_vertex_shader_locals()
        } else {
            Vec::new()
        };
        for &(def_id, name, ..) in &vertex_locals {
            self.bind_local(name, def_id);
        }

        let fragment_locals = if is_fragment {
            self.inject_fragment_shader_locals()
        } else {
            Vec::new()
        };
        for &(def_id, name, ..) in &fragment_locals {
            self.bind_local(name, def_id);
        }

        let body = decl.body.as_ref().map(|body| self.lower_block(body));
        self.pop_scope();
        self.pop_type_params();
        let mut body = body.map(|body| self.prepend_modifier_locals(body, modifier_locals));
        // Modifier locals sit innermost; stage I/O is prepended after them.
        if let Some(ref mut body) = body {
            self.prepend_runtime_provided_locals(body, vertex_locals.into_iter().rev());
            self.prepend_runtime_provided_locals(body, fragment_locals.into_iter().rev());
        }

        let posture = self.resolve_callable_posture(decl.posture, &annotations, stmt.span);
        let (nucleum_entry, nucleum_fragment) =
            Self::nucleum_role_bits(&annotations, self.interner);

        let func = HirFunction {
            name: decl.name.name,
            type_params,
            params,
            cli_args,
            ret_ty,
            err_ty,
            body,
            posture,
            is_async: posture.is_async(),
            is_generator: posture.is_stream(),
            is_kernel: nucleum_entry,
            nucleum_fragment,
            is_vertex,
            is_fragment,
            test: None,
            annotations,
        };

        Some(HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Function(Box::new(func)),
            span: stmt.span,
        })
    }

    /// Method bodies need parameter locals and the current `ego` struct. Inherited
    /// and implemented names are looked up here; semantic validity is later.
    pub fn lower_gens(&mut self, stmt: &Stmt, decl: &ClassDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);

        let type_params = self.lower_type_params(&decl.type_params);

        self.push_type_params(type_params.iter().map(|param| (param.name, param.kind)));
        let mut annotations = Vec::new();
        for annotation in &stmt.annotations {
            if !self.annotation_ast_family_is(annotation, "annotatio") {
                annotations.push(self.lower_annotation(annotation));
            }
        }

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        for member in &decl.members {
            match &member.kind {
                ClassMemberKind::Field(field) => {
                    let annotations = self.lower_annotations(&member.annotations);
                    let init = field.init.as_ref().map(|expr| self.lower_expr(expr));
                    let def_id = self.next_def_id();
                    let member_trivia = self.hir_trivia(&member.leading_trivia);
                    if !member_trivia.is_empty() {
                        self.register_attachment(
                            HirTriviaOwnerKind::Member,
                            Some(HirOwnerRef::Def(def_id)),
                            member_trivia,
                        );
                    }
                    fields.push(HirField {
                        def_id,
                        name: field.name.name,
                        ty: self.lower_type(&field.ty),
                        is_static: field.is_static,
                        sponte: field.sponte,
                        annotations,
                        init,
                        span: member.span,
                    });
                }
                ClassMemberKind::Method(method) => {
                    let annotations = self.lower_annotations(&member.annotations);
                    let mut method_type_params = self.lower_type_params(&method.type_params);
                    self.push_type_params(
                        method_type_params
                            .iter()
                            .map(|param| (param.name, param.kind)),
                    );
                    self.apply_type_param_constraints_from_annotations(
                        &mut method_type_params,
                        &annotations,
                    );

                    let mut params = Vec::new();
                    for param in &method.params {
                        params.push(self.lower_param(param, true));
                    }

                    self.push_scope();
                    for param in &params {
                        self.bind_local(param.name, param.def_id);
                    }
                    let prev_ego = self.current_ego_struct;
                    self.current_ego_struct = Some(def_id);
                    let modifier_locals = self.bind_modifier_locals(&method.modifiers);
                    let body = method.body.as_ref().map(|block| self.lower_block(block));
                    self.pop_scope();
                    self.current_ego_struct = prev_ego;
                    let body = body.map(|body| self.prepend_modifier_locals(body, modifier_locals));

                    let ret_ty = method.ret.as_ref().map(|ty| self.lower_type(ty));
                    let err_ty = method.err.as_ref().map(|ty| self.lower_type(ty));
                    self.pop_type_params();

                    let posture =
                        self.resolve_callable_posture(method.posture, &annotations, member.span);
                    let (nucleum_entry, nucleum_fragment) =
                        Self::nucleum_role_bits(&annotations, self.interner);

                    let func = HirFunction {
                        name: method.name.name,
                        type_params: method_type_params,
                        params,
                        cli_args: None,
                        ret_ty,
                        err_ty,
                        body,
                        posture,
                        is_async: posture.is_async(),
                        is_generator: posture.is_stream(),
                        is_kernel: nucleum_entry,
                        nucleum_fragment,
                        is_vertex: Self::annotation_family_is(
                            &annotations,
                            self.interner,
                            "vertex",
                        ),
                        is_fragment: Self::annotation_family_is(
                            &annotations,
                            self.interner,
                            "fragment",
                        ),
                        test: None,
                        annotations,
                    };

                    let method_def_id = self.next_def_id();
                    // `ClassMember` and the nested `FuncDecl` each own trivia;
                    // merge in source order so no claimed comment is dropped.
                    let mut member_trivia = self.hir_trivia(&member.leading_trivia);
                    member_trivia.extend(self.hir_trivia(&method.leading_trivia));
                    if !member_trivia.is_empty() {
                        self.register_attachment(
                            HirTriviaOwnerKind::Member,
                            Some(HirOwnerRef::Def(method_def_id)),
                            member_trivia,
                        );
                    }

                    methods.push(HirMethod {
                        def_id: method_def_id,
                        func,
                        receiver: HirReceiver::None,
                        span: member.span,
                    });
                }
            }
        }

        self.pop_type_params();

        let extends = decl
            .extends
            .as_ref()
            .and_then(|ident| self.resolver.lookup(ident.name));

        if decl.extends.is_some() && extends.is_none() {
            self.error("unknown_base_class");
        }

        let implements = decl
            .implements
            .iter()
            .filter_map(|ident| {
                let def_id = self.resolver.lookup(ident.name);
                if def_id.is_none() {
                    self.error("unknown_interface");
                }
                def_id
            })
            .collect();

        let struct_item = HirStruct {
            name: decl.name.name,
            type_params,
            annotations,
            fields,
            methods,
            extends,
            implements,
        };

        Some(HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Struct(struct_item),
            span: stmt.span,
        })
    }

    /// Plain `ordo` variants carry names and `DefId`s; payload fields belong to `discretio`.
    pub fn lower_ordo(&mut self, stmt: &Stmt, decl: &EnumDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);

        let variants = decl
            .members
            .iter()
            .map(|member| HirVariant {
                def_id: self.variant_def_for(def_id, member.name.name),
                name: member.name.name,
                fields: Vec::new(),
                span: member.span,
            })
            .collect();

        let enum_item = HirEnum {
            name: decl.name.name,
            type_params: Vec::new(),
            annotations: self.lower_annotations(&stmt.annotations),
            variants,
        };

        Some(HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Enum(enum_item),
            span: stmt.span,
        })
    }

    /// Variants are parent-qualified (`parent::variant`) and invisible in the
    /// bare scope table. Look up through `variant_child` so the HIR `DefId`
    /// matches the resolver; `def_id_for` covers imported or synthetic variants
    /// that never entered the parent map.
    fn variant_def_for(&mut self, parent_def_id: DefId, name: Symbol) -> DefId {
        self.resolver
            .variant_child(parent_def_id, name)
            .unwrap_or_else(|| self.def_id_for(name))
    }

    fn lower_variant(&mut self, enum_def_id: DefId, variant: &Variant) -> HirVariant {
        let fields = variant
            .fields
            .iter()
            .map(|field| HirVariantField {
                name: field.name.name,
                ty: self.lower_type(&field.ty),
                span: field.span,
            })
            .collect();

        HirVariant {
            def_id: self.variant_def_for(enum_def_id, variant.name.name),
            name: variant.name.name,
            fields,
            span: variant.span,
        }
    }

    pub fn lower_discretio(&mut self, stmt: &Stmt, decl: &UnionDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);

        let type_params = self.lower_type_params(&decl.type_params);

        self.push_type_params(type_params.iter().map(|param| (param.name, param.kind)));

        let variants = decl
            .variants
            .iter()
            .map(|variant| self.lower_variant(def_id, variant))
            .collect();

        self.pop_type_params();

        let enum_item = HirEnum {
            name: decl.name.name,
            type_params,
            annotations: self.lower_annotations(&stmt.annotations),
            variants,
        };

        Some(HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Enum(enum_item),
            span: stmt.span,
        })
    }

    fn lower_interface_method(&mut self, method: &InterfaceMethod) -> HirInterfaceMethod {
        let annotations = self.lower_annotations(&method.annotations);
        let posture = self.resolve_callable_posture(method.posture, &annotations, method.span);
        HirInterfaceMethod {
            availability: self.lower_availability_from_annotations(&annotations),
            name: method.name.name,
            // Interface signatures keep the declared name; aliases are binding-only.
            params: method
                .params
                .iter()
                .map(|param| self.lower_param(param, false))
                .collect(),
            ret_ty: method.ret.as_ref().map(|ty| self.lower_type(ty)),
            err_ty: method.err.as_ref().map(|ty| self.lower_type(ty)),
            posture,
            span: method.span,
        }
    }

    /// Signatures only. Conformance against an implementing `gens` waits for
    /// later passes that have fully lowered types.
    pub fn lower_implendum(&mut self, stmt: &Stmt, decl: &InterfaceDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);

        let type_params = self.lower_type_params(&decl.type_params);

        self.push_type_params(type_params.iter().map(|param| (param.name, param.kind)));

        let methods = decl
            .methods
            .iter()
            .map(|method| {
                let hir_method = self.lower_interface_method(method);
                // Interface signatures have no node or def id to back-reference.
                let method_trivia = self.hir_trivia(&method.leading_trivia);
                if !method_trivia.is_empty() {
                    self.register_attachment(HirTriviaOwnerKind::Member, None, method_trivia);
                }
                hir_method
            })
            .collect();

        self.pop_type_params();

        let interface = HirInterface {
            name: decl.name.name,
            type_params,
            methods,
        };

        Some(HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Interface(interface),
            span: stmt.span,
        })
    }

    pub fn lower_typus(&mut self, stmt: &Stmt, decl: &TypeAliasDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);
        let type_params = self.lower_type_params(&decl.type_params);
        self.push_type_params(type_params.iter().map(|param| (param.name, param.kind)));
        let ty = self.lower_type(&decl.ty);
        self.pop_type_params();

        Some(HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::TypeAlias(HirTypeAlias {
                name: decl.name.name,
                type_params,
                ty,
            }),
            span: stmt.span,
        })
    }

    /// Schema names stay in the type namespace and never lower to MIR. Each
    /// column is the five-field heading row: public name, resolved source
    /// (`name : source` or the public name), Faber type, nullability from
    /// `T ∪ nihil`, and source order. See docs/design/relational-views.md §2.
    pub fn lower_schema(&mut self, stmt: &Stmt, decl: &SchemaDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);

        let columns = decl
            .columns
            .iter()
            .enumerate()
            .map(|(order, column)| {
                let name = column.name.name;
                let source = column
                    .source_alias
                    .as_ref()
                    .map_or(name, |ident| ident.name);
                let ty = self.lower_type(&column.ty);
                let nullable = matches!(self.types.get(ty), Type::Option(_));
                HirSchemaColumn {
                    name,
                    source,
                    ty,
                    nullable,
                    order: u32::try_from(order).expect("schema column count exceeds u32::MAX"),
                }
            })
            .collect();

        Some(HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Schema(HirSchema {
                name: decl.name.name,
                columns,
            }),
            span: stmt.span,
        })
    }

    // IMPORTS

    /// Preserve the names collection/resolution already made visible. Package
    /// and provider loading are not decided here.
    pub fn lower_importa(&mut self, stmt: &Stmt, decl: &ImportDecl) -> Option<HirItem> {
        let def_id = self.next_def_id();

        let path = self.interner.resolve(decl.path);
        let items = if path == crate::kernel::KERNEL_GLOB_IMPORT {
            match &decl.kind {
                ImportKind::Named { name, alias, .. } => {
                    let alias = alias.as_ref().map_or(name.name, |ident| ident.name);
                    crate::kernel::kernel_manifest_entries()
                        .iter()
                        .map(|entry| {
                            let module = self.interner.intern(entry.module.name());
                            let def_id = self
                                .kernel_glob_imports
                                .get(&(alias, module))
                                .copied()
                                .unwrap_or_else(|| self.next_def_id());
                            HirImportItem {
                                def_id,
                                name: module,
                                alias: Some(alias),
                            }
                        })
                        .collect()
                }
                ImportKind::Wildcard { alias } => {
                    self.error("kernel_glob_named_alias");
                    vec![HirImportItem {
                        def_id: self.next_def_id(),
                        name: alias.name,
                        alias: Some(alias.name),
                    }]
                }
            }
        } else {
            match &decl.kind {
                ImportKind::Named { name, alias, .. } => {
                    let bound_name = alias.as_ref().map_or(name.name, |ident| ident.name);
                    let item_def_id = self
                        .lookup_name(bound_name)
                        .unwrap_or_else(|| self.next_def_id());
                    vec![HirImportItem {
                        def_id: item_def_id,
                        name: name.name,
                        alias: alias.as_ref().map(|ident| ident.name),
                    }]
                }
                ImportKind::Wildcard { alias } => {
                    let item_def_id = self
                        .lookup_name(alias.name)
                        .unwrap_or_else(|| self.next_def_id());
                    vec![HirImportItem {
                        def_id: item_def_id,
                        name: alias.name,
                        alias: Some(alias.name),
                    }]
                }
            }
        };

        Some(HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Import(HirImport {
                path: decl.path,
                visibility: decl.visibility,
                items,
            }),
            span: stmt.span,
        })
    }

    fn annotation_ast_family_is(
        &self,
        annotation: &radix_syntax::Annotation,
        family: &str,
    ) -> bool {
        match &annotation.kind {
            radix_syntax::AnnotationKind::Statement(stmt) => {
                self.interner.resolve(stmt.name.name) == family
            }
            radix_syntax::AnnotationKind::Braced(braced) => {
                self.interner.resolve(braced.name.name) == family
            }
            radix_syntax::AnnotationKind::Nucleum(_) => family == "nucleum",
            _ => false,
        }
    }
}

fn modifier_bindings(modifiers: &[radix_syntax::FuncModifier]) -> Vec<&radix_syntax::Ident> {
    let mut out = Vec::new();
    for modifier in modifiers {
        match modifier {
            radix_syntax::FuncModifier::Curata { required, alias } => {
                out.push(alias.as_ref().unwrap_or(required));
            }
            radix_syntax::FuncModifier::Errata(ident)
            | radix_syntax::FuncModifier::Optiones(ident) => {
                out.push(ident);
            }
            radix_syntax::FuncModifier::Exitus(radix_syntax::ExitusValue::Name(ident)) => {
                out.push(ident)
            }
            _ => {}
        }
    }
    out
}

fn command_argument_binding(
    modifiers: &[radix_syntax::FuncModifier],
) -> Option<&radix_syntax::Ident> {
    modifiers.iter().find_map(|modifier| match modifier {
        radix_syntax::FuncModifier::Argumenta(ident) => Some(ident),
        _ => None,
    })
}
