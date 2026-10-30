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
use crate::{CommuneField, IndexExpr, NumericWidth, Primitive, Type, TypeId};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{
    DefId, HirAnnotation, HirBlock, HirConst, HirEnum, HirExpression, HirExpressionKind, HirField,
    HirFunction, HirImport, HirImportItem, HirInterface, HirInterfaceMethod, HirItem, HirItemKind,
    HirLocal, HirMethod, HirOwnerRef, HirParam, HirParamMode, HirReceiver, HirSchema,
    HirSchemaColumn, HirStatement, HirStatementKind, HirStruct, HirTriviaOwnerKind, HirTypeAlias,
    HirTypeParam, HirTypeParamConstraint, HirTypeSlot, HirVariant, HirVariantField,
    visit::{HirVisitor, walk_expr, walk_stmt},
};
use radix_lexer::{Span, Symbol};
use radix_syntax::{
    AstMutability, AstParamMode, BindingPattern, CallablePosture, ClassDecl, ClassMember,
    ClassMemberKind, EnumDecl, EnumValue, FuncDecl, ImportDecl, ImportKind, InterfaceDecl,
    InterfaceMethod, Param, SchemaDecl, Stmt, TypeAliasDecl, TypeParam, UnionDecl, VarDecl,
    Variant,
};

type ModifierLocal = (DefId, Symbol, Span);
type ShaderLocal = (DefId, Symbol, Span, TypeId, bool);
const ISSUE_CALLABLE_POSTURE_CONFLICT: &str = "callable_posture_conflict";
/// A type declaration parameter carries an `implet` bound (bounds are for
/// callables; D1.1).
const ISSUE_IMPLET_BOUND_ON_TYPE_DECLARATION: &str = "implet_bound_on_type_declaration";

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
        AstParamMode::Axis => HirParamMode::Axis,
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
                // The parser guarantees the default variant matches the
                // declared sort, so lowering maps the payload without
                // re-checking the pairing.
                default: param.default.as_ref().map(|default| match default {
                    radix_syntax::GenericParamDefault::Typus(ty) => {
                        radix_types::SemanticGenericParamDefault::Typus(
                            self.lower_parameter_type(ty),
                        )
                    }
                    radix_syntax::GenericParamDefault::Magnitudo { value, .. } => {
                        radix_types::SemanticGenericParamDefault::Magnitudo(*value)
                    }
                }),
            })
            .collect()
    }

    /// Lower the `implet` bounds of one callable's generic row (D1.1).
    ///
    /// Runs after the row is pushed so a bound may name any parameter of the
    /// row. A bounded parameter's constraint becomes
    /// [`HirTypeParamConstraint::Implet`]; typecheck validates each contract.
    fn lower_type_param_bounds(&mut self, lowered: &mut [HirTypeParam], params: &[TypeParam]) {
        for (hir_param, param) in lowered.iter_mut().zip(params) {
            if param.bounds.is_empty() {
                continue;
            }
            let bounds: Vec<_> = param
                .bounds
                .iter()
                .filter_map(|bound| self.lower_contract_ref(bound))
                .collect();
            if bounds.len() == param.bounds.len() {
                hir_param.constraint = HirTypeParamConstraint::Implet(bounds);
            }
        }
    }

    /// Bounds belong on callables only: a bound on a type declaration's
    /// parameter would need checking at every construction and use, which
    /// D1.1 does not rule. Reject it by name rather than drop it silently.
    fn reject_type_declaration_bounds(&mut self, params: &[TypeParam]) {
        for param in params {
            if let Some(bound) = param.bounds.first() {
                self.error_with_args_at(
                    vec![DiagnosticArg::new(
                        "issue",
                        ISSUE_IMPLET_BOUND_ON_TYPE_DECLARATION,
                    )],
                    bound.span,
                );
            }
        }
    }

    /// Resolve one `implet` contract name and lower its written arguments.
    fn lower_contract_ref(
        &mut self,
        contract: &radix_syntax::ContractRefExpr,
    ) -> Option<radix_types::ContractRef> {
        let Some(def_id) = self.resolver.lookup(contract.name.name) else {
            self.current_span = contract.span;
            self.error("unknown_interface");
            return None;
        };
        // A selectively imported contract (`importa ex "norma:order" fixum
        // Orderable`) names the imported implendum, not the local binding.
        let def_id = match self
            .resolver
            .selective_import_type(def_id)
            .map(|ty| self.types.get(ty))
        {
            Some(radix_types::Type::Interface(imported)) => *imported,
            _ => def_id,
        };
        let args = contract
            .args
            .iter()
            .map(|arg| self.lower_type(arg))
            .collect();
        Some(radix_types::ContractRef {
            contract: def_id,
            args,
        })
    }

    fn lower_param(
        &mut self,
        param: &Param,
        use_alias: bool,
        param_slot: Option<HirTypeSlot>,
    ) -> HirParam {
        let def_id = self.next_def_id();
        crate::tuple_labels::record_from_type_expr(self.resolver, self.interner, def_id, &param.ty);
        self.signature_type_nesting += 1;
        let binding_name = if use_alias {
            param.alias.as_ref().unwrap_or(&param.name)
        } else {
            &param.name
        };
        let ty = match param_slot {
            Some(slot) => self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner: radix_hir::HirOwnerRef::Def(def_id),
                    slot,
                },
                &param.ty,
                true,
                false,
            ),
            None => self.lower_parameter_type(&param.ty),
        };
        // A list-typed value parameter keeps the whole-shape value reading in
        // figuras; scalar value parameters do not (size-type Z3).
        if radix_types::is_integer_index_vector(self.types, ty) {
            self.figura_extent_admissible_locals.insert(def_id);
        }
        self.signature_type_nesting -= 1;
        let mode = match param.ty.mode {
            _ if param.mode == AstParamMode::Axis => HirParamMode::Axis,
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
            .map(|(kind, ident)| {
                let def_id = self.next_def_id();
                self.bind_local(ident.name, def_id);
                self.presentation.function_modifier_bindings.push(
                    radix_hir::HirFunctionModifierBinding {
                        local: def_id,
                        kind,
                    },
                );
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
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span,
                },
            );
        }
        body
    }

    // SHADER STAGE LOCALS

    fn inject_vertex_shader_locals(&mut self) -> Vec<ShaderLocal> {
        let (vec2_ty, vec3_ty, vec4_ty) = self.shader_stage_vec_types();
        let span = Span::default();
        vec![
            self.define_shader_local("position", vec3_ty, false, span),
            self.define_shader_local("uv", vec2_ty, false, span),
            self.define_shader_local("out_position", vec4_ty, true, span),
        ]
    }

    /// Fragment stage locals, skipping any name the vertex stage already bound.
    ///
    /// A function carrying both stage annotations is diagnosed later, at MIR
    /// lowering; it must reach that diagnostic rather than fail analysis on a
    /// doubly-bound stage local.
    fn inject_fragment_shader_locals(&mut self, bound: &[ShaderLocal]) -> Vec<ShaderLocal> {
        let (vec2_ty, vec3_ty, vec4_ty) = self.shader_stage_vec_types();
        let span = Span::default();
        let wanted = [
            ("normal", vec3_ty, false),
            ("color", vec3_ty, false),
            ("uv", vec2_ty, false),
            ("out_color", vec4_ty, true),
        ];
        let mut locals = Vec::new();
        for (name, ty, mutable) in wanted {
            let name_sym = self.interner.intern(name);
            if bound
                .iter()
                .any(|&(_, bound_name, ..)| bound_name == name_sym)
            {
                continue;
            }
            locals.push(self.define_shader_local(name, ty, mutable, span));
        }
        locals
    }

    /// Bind injected stage I/O names into the function's local scope so bodies
    /// resolve them like authored locals.
    fn bind_shader_locals(&mut self, locals: &[ShaderLocal]) {
        for &(def_id, name, ..) in locals {
            self.bind_local(name, def_id);
        }
    }

    fn shader_stage_vec_types(&mut self) -> (TypeId, TypeId, TypeId) {
        let f32_ty = self
            .types
            .sized_numeric(Primitive::Fractus, NumericWidth::F32);
        let width2 = self.types.intern_index(IndexExpr::Literal(2));
        let width3 = self.types.intern_index(IndexExpr::Literal(3));
        let width4 = self.types.intern_index(IndexExpr::Literal(4));
        (
            self.types.vector_with_width(f32_ty, width2),
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
        // Function-local names bind in the lowerer's scope, not the resolver's
        // global frame (which already contains builtin stage names). Keep the
        // fresh symbol's type available to `check_path` by DefId only.
        self.resolver.register_unbound_symbol(crate::Symbol {
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
                        await_store: None,
                        size_decl: false,
                        size_static: false,
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
        if matches!(
            decl.await_form,
            Some(radix_syntax::AwaitBindingForm::Assignment)
        ) {
            self.presentation.await_assignment_bindings.push(def_id);
        }
        let annotations = self.lower_item_annotations(def_id, &stmt.annotations);
        let ty = decl.ty.as_ref().map(|ty| {
            self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner: radix_hir::HirOwnerRef::Def(def_id),
                    slot: HirTypeSlot::Declared,
                },
                ty,
                false,
                false,
            )
        });
        // A top-level `=` constant (`const T X = e`) is the module constant
        // (st1 R1/R3): it carries the statica marker and lowers through the
        // compile-time initializer path.
        let is_constant = decl.compile_time;
        let value = if let Some(init) = &decl.init {
            if is_constant {
                self.lower_compile_time_initializer(init)
            } else {
                self.lower_expr(init)
            }
        } else {
            self.error("top_level_initializer");
            error_expr(self, name_span)
        };

        Some(HirItem {
            annotations,
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Constant(HirConst {
                name,
                ty,
                value,
                mutable: decl.mutability == AstMutability::Mutable,
                is_await: decl.is_await(),
                statica: is_constant.then_some(radix_hir::HirStatic { authored: None }),
            }),
            span: stmt.span,
        })
    }

    /// Bind parameters and modifiers before the body so locals resolve. CLI
    /// argument metadata becomes a synthetic parameter, not a separate HIR concept.
    pub fn lower_functio(&mut self, stmt: &Stmt, decl: &FuncDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);

        let annotations = self.lower_item_annotations(def_id, &stmt.annotations);
        let previous_parameter_ordinals = std::mem::take(&mut self.parameter_ordinals);
        let mut type_params = self.lower_type_params(&decl.type_params);
        self.push_type_params(
            type_params
                .iter()
                .map(|param| (param.def_id, param.name, param.kind)),
        );
        self.lower_type_param_bounds(&mut type_params, &decl.type_params);
        self.apply_type_param_constraints_from_annotations(&mut type_params, &annotations);
        // Earlier runtime parameters may supply extents in later parameter
        // shapes and in the return contract. Bind each one after lowering its
        // own type so dependent positions resolve by DefId, not by spelling.
        self.push_scope();
        let mut params = Vec::new();
        for (ordinal, param) in decl.params.iter().enumerate() {
            let lowered =
                self.lower_param(param, true, Some(HirTypeSlot::Parameter(ordinal as u32)));
            self.bind_local(lowered.name, lowered.def_id);
            self.parameter_ordinals
                .insert(lowered.def_id, u32::try_from(ordinal).unwrap_or(u32::MAX));
            params.push(lowered);
        }
        let cli_args = self.lower_cli_args_param(decl);
        if let Some(param) = &cli_args {
            self.bind_local(param.name, param.def_id);
        }

        self.signature_type_nesting += 1;
        let ret_ty = decl.ret.as_ref().map(|ty| {
            self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner: radix_hir::HirOwnerRef::Def(def_id),
                    slot: HirTypeSlot::Return,
                },
                ty,
                false,
                true,
            )
        });
        let err_ty = decl.err.as_ref().map(|ty| {
            self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner: radix_hir::HirOwnerRef::Def(def_id),
                    slot: HirTypeSlot::Error,
                },
                ty,
                false,
                true,
            )
        });
        self.signature_type_nesting -= 1;
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
        self.bind_shader_locals(&vertex_locals);

        let fragment_locals = if is_fragment {
            self.inject_fragment_shader_locals(&vertex_locals)
        } else {
            Vec::new()
        };
        self.bind_shader_locals(&fragment_locals);

        let body = decl.body.as_ref().map(|body| self.lower_block(body));
        self.pop_scope();
        self.pop_type_params();
        self.parameter_ordinals = previous_parameter_ordinals;
        let (nucleum_entry, nucleum_fragment) =
            Self::nucleum_role_bits(&annotations, self.interner);
        let mut body = body.map(|body| self.prepend_modifier_locals(body, modifier_locals));
        // Modifier locals sit innermost; stage I/O is prepended after them.
        if let Some(ref mut body) = body {
            self.prepend_runtime_provided_locals(body, vertex_locals.into_iter().rev());
            self.prepend_runtime_provided_locals(body, fragment_locals.into_iter().rev());
        }

        let posture = self.resolve_callable_posture(decl.posture, &annotations, stmt.span);

        if let Some(marker_span) = stmt
            .annotations
            .iter()
            .chain(decl.annotations.iter())
            .find(|annotation| matches!(annotation.kind, radix_syntax::AnnotationKind::Conversio))
            .map(|annotation| annotation.span)
        {
            self.enroll_conversio_function(
                decl,
                def_id,
                &params,
                ret_ty,
                body.as_ref(),
                marker_span,
            );
        }

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
        };

        Some(HirItem {
            annotations,
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Function(Box::new(func)),
            span: stmt.span,
        })
    }

    /// Derive the ordered concrete pair from one `@ conversio` function and
    /// enroll it in the coherence registry (CTR-04).
    ///
    /// Validation order: generic-row rejection, signature shape, purity,
    /// self-pair, orphan ownership, then registry enrollment (duplicate and
    /// builtin-shadow conflicts carry both spans). The function stays an
    /// ordinary callable whether or not it is selected as the row.
    fn enroll_conversio_function(
        &mut self,
        decl: &FuncDecl,
        fn_def_id: radix_hir::DefId,
        params: &[HirParam],
        ret_ty: Option<TypeId>,
        body: Option<&HirBlock>,
        marker_span: Span,
    ) {
        let issue = |name: &str| DiagnosticArg::new("issue", name);
        if !decl.type_params.is_empty() {
            self.error_with_args_at(vec![issue("conversio_generic_row")], marker_span);
            return;
        }
        let (Some(source_ty), Some(destination_ty)) = (
            params.first().filter(|_| params.len() == 1).map(|p| p.ty),
            ret_ty,
        ) else {
            self.error_with_args_at(vec![issue("conversio_signature")], marker_span);
            return;
        };
        if let Some(span) = body.and_then(check_conversio_purity) {
            self.error_with_args_at(vec![issue("conversio_impure_body")], span);
            return;
        }
        if self.types.equals(source_ty, destination_ty) {
            self.error_with_args_at(vec![issue("conversio_self_pair")], marker_span);
            return;
        }
        if conversion_pair_is_orphan(self.resolver, self.types, source_ty, destination_ty) {
            self.error_with_args_at(vec![issue("conversio_both_ends_foreign")], marker_span);
            return;
        }
        let pair = crate::scope::ConversionPair::new(source_ty, destination_ty);
        if let Err(conflict) = self
            .resolver
            .register_user_conversion(pair, fn_def_id, marker_span)
        {
            let error = crate::passes::typecheck::convert::conversion_registry_conflict_error(
                pair, conflict,
            );
            self.error_with_args_at(error.args, error.span);
        }
    }

    /// Method bodies need parameter locals and the current `ego` struct. Inherited
    /// and implemented names are looked up here; semantic validity is later.
    pub fn lower_gens(&mut self, stmt: &Stmt, decl: &ClassDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);
        // Every class method has an implicit receiver, even when no source
        // expression constructs or names the class. Record that nominal type
        // at its declaration rather than relying on a later use to intern it.
        self.types.intern(crate::Type::Struct(def_id));

        let type_params = self.lower_type_params(&decl.type_params);
        self.reject_type_declaration_bounds(&decl.type_params);

        self.push_type_params(
            type_params
                .iter()
                .map(|param| (param.def_id, param.name, param.kind)),
        );
        let mut annotations = Vec::new();
        for annotation in &stmt.annotations {
            if !self.annotation_ast_family_is(annotation, "annotatio") {
                annotations.push(self.lower_annotation(annotation));
            }
        }

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut field_ordinal = 0u32;

        for member in &decl.members {
            match &member.kind {
                ClassMemberKind::Field(field) => {
                    let annotations = self.lower_annotations(&member.annotations);
                    // A field default is a compile-time initializer tree under
                    // every modifier (`sit`, `varia`, `fixum`): the one `=`
                    // surface admits `insere` and `praefixum` here as it does
                    // for a module constant.
                    let init = field
                        .init
                        .as_ref()
                        .map(|expr| self.lower_compile_time_initializer(expr));
                    let def_id = self.next_def_id();
                    let member_trivia = Self::hir_trivia(&member.leading_trivia);
                    if !member_trivia.is_empty() {
                        self.register_attachment(
                            HirTriviaOwnerKind::Member,
                            Some(HirOwnerRef::Def(def_id)),
                            member_trivia,
                        );
                    }
                    let field_ty = self.lower_authored_type(
                        radix_hir::HirTypeSite {
                            owner: HirOwnerRef::Def(def_id),
                            slot: HirTypeSlot::Field(field_ordinal),
                        },
                        &field.ty,
                        false,
                        false,
                    );
                    fields.push(HirField {
                        def_id,
                        name: field.name.name,
                        ty: field_ty,
                        modifier: field.modifier,
                        sponte: field.sponte,
                        annotations,
                        init,
                        span: member.span,
                    });
                    field_ordinal += 1;
                }
                ClassMemberKind::Method(method) => {
                    methods.push(self.lower_gens_method(def_id, member, method));
                }
            }
        }

        // Contract arguments may name the genus's own parameters
        // (`genus Arca<T> implet Capsa<T>`), so lower them in that scope.
        let implements = decl
            .implements
            .iter()
            .filter_map(|contract| self.lower_contract_ref(contract))
            .collect();

        self.pop_type_params();

        let struct_item = HirStruct {
            name: decl.name.name,
            type_params,
            fields,
            methods,
            implements,
        };

        Some(HirItem {
            annotations,
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Struct(struct_item),
            span: stmt.span,
        })
    }

    /// One `genus` method: generic row, parameters, and modifier locals bind
    /// before the body lowers, with `ego` pointed at the enclosing struct.
    fn lower_gens_method(
        &mut self,
        struct_def_id: DefId,
        member: &ClassMember,
        method: &FuncDecl,
    ) -> HirMethod {
        let annotations = self.lower_annotations(&member.annotations);
        let mut method_type_params = self.lower_type_params(&method.type_params);
        self.push_type_params(
            method_type_params
                .iter()
                .map(|param| (param.def_id, param.name, param.kind)),
        );
        self.lower_type_param_bounds(&mut method_type_params, &method.type_params);
        self.apply_type_param_constraints_from_annotations(&mut method_type_params, &annotations);

        let mut params = Vec::new();
        for (ordinal, param) in method.params.iter().enumerate() {
            params.push(self.lower_param(
                param,
                true,
                Some(HirTypeSlot::Parameter(ordinal as u32)),
            ));
        }

        self.push_scope();
        for param in &params {
            self.bind_local(param.name, param.def_id);
        }
        let prev_ego = self.current_ego_struct;
        self.current_ego_struct = Some(struct_def_id);
        let modifier_locals = self.bind_modifier_locals(&method.modifiers);
        let body = method.body.as_ref().map(|block| self.lower_block(block));
        self.pop_scope();
        self.current_ego_struct = prev_ego;
        let body = body.map(|body| self.prepend_modifier_locals(body, modifier_locals));

        let ret_ty = method.ret.as_ref().map(|ty| self.lower_signature_type(ty));
        let err_ty = method.err.as_ref().map(|ty| self.lower_signature_type(ty));
        self.pop_type_params();

        let posture = self.resolve_callable_posture(method.posture, &annotations, member.span);
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
            is_vertex: Self::annotation_family_is(&annotations, self.interner, "vertex"),
            is_fragment: Self::annotation_family_is(&annotations, self.interner, "fragment"),
            test: None,
        };

        let method_def_id = self.next_def_id();
        // `ClassMember` and the nested `FuncDecl` each own trivia; merge in
        // source order so no claimed comment is dropped.
        let mut member_trivia = Self::hir_trivia(&member.leading_trivia);
        member_trivia.extend(Self::hir_trivia(&method.leading_trivia));
        if !member_trivia.is_empty() {
            self.register_attachment(
                HirTriviaOwnerKind::Member,
                Some(HirOwnerRef::Def(method_def_id)),
                member_trivia,
            );
        }

        HirMethod {
            annotations,
            def_id: method_def_id,
            func,
            receiver: HirReceiver::None,
            span: member.span,
        }
    }

    /// Plain `ordo` variants carry names and `DefId`s; payload fields belong to `discretio`.
    pub fn lower_ordo(&mut self, stmt: &Stmt, decl: &EnumDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);
        let annotations = self.lower_item_annotations(def_id, &stmt.annotations);

        // Surface fidelity: authored `ordo` members carry discriminants the
        // union-shaped HIR drops; keep them on the presentation sidecar so
        // emission restores the authored enumeration vocabulary and values.
        let members = decl
            .members
            .iter()
            .map(|member| radix_hir::HirOrdoMember {
                def_id: self.variant_def_for(def_id, member.name.name),
                value: member.value.as_ref().map(|value| match value {
                    EnumValue::Integer(int) => radix_hir::HirOrdoValue::Integer(*int),
                    EnumValue::String(text) => radix_hir::HirOrdoValue::String(*text),
                }),
            })
            .collect();
        self.presentation
            .ordo_enums
            .push(radix_hir::HirOrdoEnum { def_id, members });

        let variants = decl
            .members
            .iter()
            .map(|member| {
                let def_id = self.variant_def_for(def_id, member.name.name);
                let trivia = Self::hir_trivia(&member.leading_trivia);
                if !trivia.is_empty() {
                    self.register_attachment(
                        HirTriviaOwnerKind::Member,
                        Some(HirOwnerRef::Def(def_id)),
                        trivia,
                    );
                }
                HirVariant {
                    def_id,
                    name: member.name.name,
                    fields: Vec::new(),
                    span: member.span,
                }
            })
            .collect();

        let enum_item = HirEnum {
            name: decl.name.name,
            type_params: Vec::new(),
            commune_fields: Vec::new(),
            variants,
        };

        Some(HirItem {
            annotations,
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

        let def_id = self.variant_def_for(enum_def_id, variant.name.name);
        let trivia = Self::hir_trivia(&variant.leading_trivia);
        if !trivia.is_empty() {
            self.register_attachment(
                HirTriviaOwnerKind::Member,
                Some(HirOwnerRef::Def(def_id)),
                trivia,
            );
        }
        HirVariant {
            def_id,
            name: variant.name.name,
            fields,
            span: variant.span,
        }
    }

    pub fn lower_discretio(&mut self, stmt: &Stmt, decl: &UnionDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);
        let annotations = self.lower_item_annotations(def_id, &stmt.annotations);

        let type_params = self.lower_type_params(&decl.type_params);
        self.reject_type_declaration_bounds(&decl.type_params);

        self.push_type_params(
            type_params
                .iter()
                .map(|param| (param.def_id, param.name, param.kind)),
        );

        let commune = self.lower_commune_fields(def_id, decl);
        self.resolver
            .register_commune_fields(def_id, commune.clone());

        let variants = decl
            .variants
            .iter()
            .map(|variant| self.lower_variant(def_id, variant))
            .collect();

        self.pop_type_params();

        // CTR-03 retracted in-union conversion arms; a union lowers as pure
        // shape and conversion rows enroll only from `@ conversio` functions.
        let enum_item = HirEnum {
            name: decl.name.name,
            type_params,
            commune_fields: commune
                .into_iter()
                .map(|field| HirVariantField {
                    name: field.name,
                    ty: field.ty,
                    span: field.span,
                })
                .collect(),
            variants,
        };

        Some(HirItem {
            annotations,
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Enum(enum_item),
            span: stmt.span,
        })
    }

    pub(super) fn lower_discretio_items(&mut self, stmt: &Stmt, decl: &UnionDecl) -> Vec<HirItem> {
        self.lower_discretio(stmt, decl).into_iter().collect()
    }

    fn lower_commune_fields(&mut self, def_id: DefId, decl: &UnionDecl) -> Vec<CommuneField> {
        decl.shared_fields
            .iter()
            .enumerate()
            .map(|(ordinal, shared)| {
                let trivia = Self::hir_trivia(&shared.leading_trivia);
                if !trivia.is_empty()
                    && let Ok(ordinal) = u32::try_from(ordinal)
                {
                    // The portable presentation schema uses a u32 ordinal;
                    // do not panic lowering when a source exceeds that range.
                    self.register_attachment(
                        HirTriviaOwnerKind::UnionSharedField { ordinal },
                        Some(HirOwnerRef::Def(def_id)),
                        trivia,
                    );
                }
                CommuneField {
                    name: shared.field.name.name,
                    ty: self.lower_type(&shared.field.ty),
                    span: shared.field.name.span,
                }
            })
            .collect()
    }

    fn lower_interface_method(&mut self, method: &InterfaceMethod) -> HirInterfaceMethod {
        let annotations = self.lower_annotations(&method.annotations);
        let posture = self.resolve_callable_posture(method.posture, &annotations, method.span);
        HirInterfaceMethod {
            def_id: self.next_def_id(),
            availability: self.lower_availability_from_annotations(&annotations),
            name: method.name.name,
            // Interface signatures keep the declared name; aliases are binding-only.
            params: method
                .params
                .iter()
                .enumerate()
                .map(|(ordinal, param)| {
                    self.lower_param(param, false, Some(HirTypeSlot::Parameter(ordinal as u32)))
                })
                .collect(),
            ret_ty: method.ret.as_ref().map(|ty| self.lower_signature_type(ty)),
            err_ty: method.err.as_ref().map(|ty| self.lower_signature_type(ty)),
            posture,
            span: method.span,
        }
    }

    /// Signatures only. Conformance against an implementing `gens` waits for
    /// later passes that have fully lowered types.
    pub fn lower_implendum(&mut self, stmt: &Stmt, decl: &InterfaceDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);
        let annotations = self.lower_item_annotations(def_id, &stmt.annotations);

        let type_params = self.lower_type_params(&decl.type_params);
        self.reject_type_declaration_bounds(&decl.type_params);

        self.push_type_params(
            type_params
                .iter()
                .map(|param| (param.def_id, param.name, param.kind)),
        );

        let methods = decl
            .methods
            .iter()
            .map(|method| {
                let hir_method = self.lower_interface_method(method);
                // Interface signatures retain a definition id, even though
                // they have no body node; use it as the trivia back-reference.
                let method_trivia = Self::hir_trivia(&method.leading_trivia);
                if !method_trivia.is_empty() {
                    self.register_attachment(
                        HirTriviaOwnerKind::Member,
                        Some(HirOwnerRef::Def(hir_method.def_id)),
                        method_trivia,
                    );
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
            annotations,
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Interface(interface),
            span: stmt.span,
        })
    }

    pub fn lower_typus(&mut self, stmt: &Stmt, decl: &TypeAliasDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);
        let annotations = self.lower_item_annotations(def_id, &stmt.annotations);
        let type_params = self.lower_type_params(&decl.type_params);
        self.reject_type_declaration_bounds(&decl.type_params);
        self.push_type_params(
            type_params
                .iter()
                .map(|param| (param.def_id, param.name, param.kind)),
        );
        let ty = self.lower_authored_type(
            radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Def(def_id),
                slot: HirTypeSlot::AliasBody,
            },
            &decl.ty,
            false,
            false,
        );
        self.pop_type_params();

        Some(HirItem {
            annotations,
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
    ///
    /// # Panics
    ///
    /// Panics if a schema declares more columns than `u32` can address.
    pub fn lower_schema(&mut self, stmt: &Stmt, decl: &SchemaDecl) -> Option<HirItem> {
        let def_id = self.def_id_for(decl.name.name);
        let annotations = self.lower_item_annotations(def_id, &stmt.annotations);

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
                let ty = self.lower_authored_type(
                    radix_hir::HirTypeSite {
                        owner: radix_hir::HirOwnerRef::Def(def_id),
                        slot: HirTypeSlot::Field(order as u32),
                    },
                    &column.ty,
                    false,
                    false,
                );
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
            annotations,
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
        let annotations = self.lower_item_annotations(def_id, &stmt.annotations);

        let selective = matches!(&decl.kind, ImportKind::Selective { .. })
            && self.interner.resolve(decl.path) != crate::kernel::KERNEL_GLOB_IMPORT;
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
                // EOT-2M (transitional repair for EOT-2S): the kernel glob
                // is a namespace import surface; selective members are not
                // kernel-glob bindings. Fail closed until resolved members
                // lower as ordinary value locals.
                ImportKind::Selective { .. } => {
                    self.error("selective_import_kernel_glob_unsupported");
                    vec![]
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
                // EOT-2M: selective members lower as ordered value bindings.
                // `name` is the exported spelling (locale surface as spelled),
                // `alias` the caller-owned local when one was spelled; the
                // binding identity is the collect-defined local DefId.
                ImportKind::Selective { members } => members
                    .iter()
                    .map(|member| {
                        let local = member.alias.as_ref().unwrap_or(&member.name);
                        let item_def_id = self
                            .lookup_name(local.name)
                            .unwrap_or_else(|| self.next_def_id());
                        HirImportItem {
                            def_id: item_def_id,
                            name: member.name.name,
                            alias: member.alias.as_ref().map(|ident| ident.name),
                        }
                    })
                    .collect(),
            }
        };

        Some(HirItem {
            annotations,
            id: self.next_hir_id(),
            def_id,
            kind: HirItemKind::Import(HirImport {
                path: decl.path,
                visibility: decl.visibility,
                items,
                selective,
                target: decl.target.as_ref().map(|word| word.name),
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

fn modifier_bindings(
    modifiers: &[radix_syntax::FuncModifier],
) -> Vec<(radix_hir::HirFunctionModifierKind, &radix_syntax::Ident)> {
    use radix_hir::HirFunctionModifierKind as Kind;
    let mut out = Vec::new();
    for modifier in modifiers {
        match modifier {
            radix_syntax::FuncModifier::Errata(ident) => out.push((Kind::Errata, ident)),
            radix_syntax::FuncModifier::Optiones(ident) => out.push((Kind::Optiones, ident)),
            radix_syntax::FuncModifier::Exitus(radix_syntax::ExitusValue::Name(ident)) => {
                out.push((Kind::Exitus, ident));
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

/// Orphan-style ownership for one conversion pair (CTR-04): the declaring
/// module must own at least one end. Ends with no nominal `DefId` are
/// compiler-shipped carriers (primitives, builtins), not foreign types; a
/// nominal end is foreign only when it arrived from an imported file interface.
pub(crate) fn conversion_pair_is_orphan(
    resolver: &crate::Resolver,
    types: &crate::TypeTable,
    source: TypeId,
    destination: TypeId,
) -> bool {
    let foreign = |ty: TypeId| match types.get(ty) {
        Type::Enum(def_id) | Type::Struct(def_id) | Type::Interface(def_id) => {
            resolver.type_def_is_foreign(*def_id)
        }
        _ => false,
    };
    foreign(source) && foreign(destination)
}

/// Purity gate for one `@ conversio` function body (CTR-04): a marked row is a
/// pure re-presentation, so local mutation (assignment, conversion-directed
/// assignment, mutation of a mutable binding, increment/decrement) fails
/// closed wherever it nests in the body. Returns the first offending span.
fn check_conversio_purity(block: &HirBlock) -> Option<Span> {
    let mut gate = ConversioPurityGate { impure: None };
    gate.visit_block(block);
    gate.impure
}

/// Walks the whole body through the shared HIR traversal so a new control-flow
/// construct cannot hide a mutation from the gate.
struct ConversioPurityGate {
    impure: Option<Span>,
}

impl HirVisitor for ConversioPurityGate {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if self.impure.is_some() {
            return;
        }
        match &stmt.kind {
            HirStatementKind::Local(local) if local.mutable => self.impure = Some(stmt.span),
            HirStatementKind::IncDec(_) => self.impure = Some(stmt.span),
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if self.impure.is_some() {
            return;
        }
        match &expr.kind {
            HirExpressionKind::Assign(..) | HirExpressionKind::ConversioAssign { .. } => {
                self.impure = Some(expr.span);
            }
            _ => walk_expr(self, expr),
        }
    }
}

#[cfg(test)]
#[path = "decl_test.rs"]
mod decl_test;
