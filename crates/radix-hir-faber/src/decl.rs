//! Declaration emission for the canonical Faber backend.
//!
//! This backend prints analyzed HIR back into canonical Faber declaration text.
//! It is source-preserving in the semantic sense: declarations keep their
//! language shape, names, types, annotations, imports, and declaration bodies
//! where that information survives in HIR. It is not a pretty-printer for
//! original source files; comments, author spacing, and non-canonical layout are
//! intentionally not reconstructed.
//!
//! INVARIANTS
//! ==========
//! - Output uses canonical Faber declaration forms, including type-first
//!   parameters and fields.
//! - Synthetic test functions are rendered as `proba` declarations, not as
//!   ordinary `functio` declarations.
//! - Imports are normalized to the canonical `importa ex ... <visibility> ...`
//!   shape accepted by the parser.
//! - Missing semantic information is not guessed here; declaration codegen only
//!   emits facts present in HIR and the type/name tables.

use crate::surface::KeywordSurface;
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirAnnotation, HirAvailability, HirBlock, HirConst, HirEnum, HirExpressionKind,
    HirFunction, HirFunctionModifierKind, HirImport, HirInterface, HirItem, HirItemKind,
    HirOwnerRef, HirParam, HirStatementKind, HirStruct, HirTestModifier, HirTypeParam,
    HirTypeParamConstraint, HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_syntax::{CallablePosture, GenericParamKind};
use radix_types::{ContractRef, SemanticGenericParamDefault, TypeId};

impl super::FaberCodegen<'_> {
    pub(super) fn generate_item(
        &self,
        item: &HirItem,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_hir_annotations(&item.annotations, names, surface, Some(item.def_id), writer);
        match &item.kind {
            HirItemKind::Function(func) => {
                self.generate_function(
                    func,
                    &item.annotations,
                    Some(item.def_id),
                    names,
                    surface,
                    writer,
                );
            }
            HirItemKind::Struct(s) => {
                self.generate_struct(item.def_id, s, names, surface, writer);
            }
            HirItemKind::Enum(e) => {
                self.generate_enum(item.def_id, e, names, surface, writer);
            }
            HirItemKind::Interface(i) => {
                self.generate_interface(i, names, surface, writer);
            }
            HirItemKind::TypeAlias(a) => {
                writer.write(surface.contextual_keyword("typus"));
                writer.write(" ");
                writer.write(&self.symbol_to_string(a.name));
                self.write_generic_params(&a.type_params, names, surface, writer);
                writer.write(" = ");
                let site = HirTypeSite {
                    owner: HirOwnerRef::Def(item.def_id),
                    slot: HirTypeSlot::AliasBody,
                };
                let ty = self.render_type_at(&site, a.ty, names, surface);
                writer.write(&ty);
                writer.newline();
            }
            HirItemKind::Schema(schema) => {
                writer.write(surface.keyword(TokenKind::Schema));
                writer.write(" ");
                writer.write(&self.symbol_to_string(schema.name));
                writer.writeln(" {");
                writer.indented(|writer| {
                    for (ordinal, column) in schema.columns.iter().enumerate() {
                        writer.write(surface.keyword(TokenKind::Columna));
                        writer.write(" ");
                        let site = HirTypeSite {
                            owner: HirOwnerRef::Def(item.def_id),
                            slot: HirTypeSlot::Field(
                                self.slot_ordinal(ordinal, "schema column count"),
                            ),
                        };
                        let rendered = self.render_type_at(&site, column.ty, names, surface);
                        writer.write(&rendered);
                        writer.write(" ");
                        writer.write(&self.symbol_to_string(column.name));
                        if column.source != column.name {
                            writer.write(" : ");
                            writer.write(&self.symbol_to_string(column.source));
                        }
                        writer.newline();
                    }
                });
                writer.write("}");
                writer.newline();
            }
            HirItemKind::Constant(c) => {
                self.write_constant_item(c, item.def_id, names, surface, writer);
            }
            HirItemKind::Import(import) => {
                self.write_import_item(import, surface, writer);
            }
        }
    }

    /// Emit one module constant item, including its `↤` typed initializer and
    /// the `↢` awaited-binding spelling.
    fn write_constant_item(
        &self,
        constant: &HirConst,
        def_id: DefId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let await_assignment = self.await_assignment_bindings.contains(&def_id);
        if constant.statica.is_some() {
            // A module-level constant is spelled `fixum` (st1 R15); a genus
            // constant member takes the same word.
            writer.write(surface.contextual_keyword("fixum"));
        } else {
            writer.write(surface.contextual_keyword(if constant.mutable {
                "varia"
            } else {
                "fixum"
            }));
        }
        writer.write(" ");
        if let Some(ty) = constant.ty {
            let site = HirTypeSite {
                owner: HirOwnerRef::Def(def_id),
                slot: HirTypeSlot::Declared,
            };
            // The authored form at this site is the sole authority for
            // concrete versus hole spelling. An absent site is a
            // synthesized/hand-built binding and uses the canonical
            // concrete fallback inside `type_to_faber_at`.
            let rendered = self.render_type_at(&site, ty, names, surface);
            writer.write(&rendered);
            writer.write(" ");
        }
        writer.write(&self.symbol_to_string(constant.name));
        if let HirExpressionKind::ConversioAssign {
            source,
            recovery,
            params,
            ..
        } = &constant.value.kind
        {
            // Typed `↤` constant initializer: lowering stores the
            // binding as the assignment target, so re-emit as
            // `sit T name ↤ source [⊥ default]`.
            writer.write(" ↤ ");
            self.write_expr_prec(source, 1, names, surface, writer);
            self.write_conversio_assign_via(params, surface, writer);
            if let Some(recovery) = recovery {
                writer.write(" ⊥ ");
                self.write_expr_prec(recovery, 12, names, surface, writer);
            }
        } else if let Some(statica) = &constant.statica {
            // A module constant is a compile-time fact: `fixum T NAME =
            // value`, re-emitted from its authored (unfolded) initializer.
            writer.write(" = ");
            let value = statica.authored.as_deref().unwrap_or(&constant.value);
            self.write_expr(value, names, surface, writer);
        } else {
            writer.write(if await_assignment { " ↢ " } else { " ← " });
            self.write_expr(&constant.value, names, surface, writer);
        }
        writer.newline();
    }

    /// Emit one import declaration: the canonical `importa ex … <visibility>`
    /// prefix plus either the selective member list or the namespace form.
    fn write_import_item(
        &self,
        import: &HirImport,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(surface.keyword(TokenKind::Importa));
        writer.write(" ");
        writer.write(surface.keyword(TokenKind::Ex));
        writer.write(" ");
        self.write_symbol_literal(import.path, writer);
        // The parser retired explicit import `privata` (VM-U3); the
        // absence of `publica` is the private default. Emit only the
        // non-default tier, mirroring the author/pretty printers.
        if import.visibility == radix_syntax::Visibility::Publica {
            writer.write(" ");
            writer.write(surface.keyword(TokenKind::Publica));
        }
        if import.selective {
            // EOT-2 selective import: `fixum member [ut alias], …`.
            // `name` is the exported spelling (source order preserved),
            // `alias` the caller-owned local. An explicit identity
            // alias collapses to the bare exported name: the member
            // list is not a namespace form, so the `* ut` wildcard
            // shape never applies here.
            writer.write(" ");
            writer.write(surface.keyword(TokenKind::Fixum));
            for (idx, item) in import.items.iter().enumerate() {
                if idx > 0 {
                    writer.write(",");
                }
                writer.write(" ");
                let name = self.symbol_to_string(item.name);
                match item.alias {
                    Some(alias) if alias != item.name => {
                        writer.write(&name);
                        writer.write(" ");
                        writer.write(surface.keyword(TokenKind::Ut));
                        writer.write(" ");
                        writer.write(&self.symbol_to_string(alias));
                    }
                    _ => writer.write(&name),
                }
            }
            writer.newline();
            return;
        }
        for item in &import.items {
            writer.write(" ");
            let name = self.symbol_to_string(item.name);
            if item.alias == Some(item.name) {
                writer.write("* ");
                writer.write(surface.keyword(TokenKind::Ut));
                writer.write(" ");
                writer.write(&name);
            } else if let Some(alias) = item.alias {
                let alias = self.symbol_to_string(alias);
                writer.write(&name);
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ut));
                writer.write(" ");
                writer.write(&alias);
            } else {
                writer.write(&name);
            }
        }
        writer.newline();
    }

    pub(super) fn generate_function(
        &self,
        func: &HirFunction,
        annotations: &[HirAnnotation],
        owner: Option<DefId>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        *self.runtime_parameter_names.borrow_mut() = func
            .params
            .iter()
            .enumerate()
            .filter_map(|(ordinal, param)| Some((u32::try_from(ordinal).ok()?, param.name)))
            .collect();
        if func.test.is_some() {
            self.generate_proba_function(func, names, surface, writer);
            self.runtime_parameter_names.borrow_mut().clear();
            return;
        }

        writer.write(surface.contextual_keyword("functio"));
        writer.write(" ");
        writer.write(&self.symbol_to_string(func.name));
        self.write_generic_params(&func.type_params, names, surface, writer);
        writer.write("(");
        self.write_param_list(&func.params, names, surface, writer);
        writer.write(")");
        if let Some(cli_args) = &func.cli_args {
            // `functio f() argumenta args` sugar: the CLI bundle param is not
            // part of `params`, so its binding re-emits in the postfix slot.
            writer.write(" ");
            writer.write(surface.keyword(TokenKind::Argumenta));
            writer.write(" ");
            writer.write(&self.symbol_to_string(cli_args.name));
        }
        self.write_function_modifier_bindings(func, surface, writer);
        if !has_legacy_posture_annotation(annotations, self.interner) {
            self.write_callable_posture(func.posture, surface, writer);
        }
        self.write_function_result_types(func.ret_ty, func.err_ty, owner, names, surface, writer);
        self.write_optional_block_body(func.body.as_ref(), names, surface, writer);
        self.runtime_parameter_names.borrow_mut().clear();
    }

    /// Re-emit `errors e` / `options o` / `exit code` signature modifiers.
    ///
    /// Each binds a runtime-provided local at the head of the body; the body
    /// writer skips those locals, so the authored modifier is written here, in
    /// the order the locals were lowered (which is the authored order).
    fn write_function_modifier_bindings(
        &self,
        func: &HirFunction,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if self.function_modifier_bindings.is_empty() {
            return;
        }
        let Some(body) = &func.body else {
            return;
        };
        for stmt in &body.statements {
            let HirStatementKind::Local(local) = &stmt.kind else {
                continue;
            };
            if !local.runtime_provided {
                continue;
            }
            let Some(kind) = self.function_modifier_bindings.get(&local.def_id) else {
                continue;
            };
            writer.write(" ");
            writer.write(surface.keyword(match kind {
                HirFunctionModifierKind::Errata => TokenKind::Errata,
                HirFunctionModifierKind::Optiones => TokenKind::Optiones,
                HirFunctionModifierKind::Exitus => TokenKind::Exitus,
            }));
            writer.write(" ");
            writer.write(&self.symbol_to_string(local.name));
        }
    }

    /// Emit a synthetic HIR test function as a source-level `proba` block.
    pub(super) fn generate_proba_function(
        &self,
        func: &HirFunction,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // HIR stores test modifiers separately from the function signature so
        // the backend can recover canonical `proba "name" ... {}` syntax.
        writer.write(surface.keyword(TokenKind::Proba));
        writer.write(" ");
        if let Some(test) = &func.test {
            self.write_quoted_text(self.interner.resolve(test.name), writer);
            for modifier in &test.modifiers {
                writer.write(" ");
                self.write_test_modifier(modifier, surface, writer);
            }
        } else {
            self.write_quoted_text(self.interner.resolve(func.name), writer);
        }

        self.write_optional_block_body(func.body.as_ref(), names, surface, writer);
    }

    /// Emit leading trivia owned by a member `DefId`.
    ///
    /// Genus fields and methods, enum members/variants, and interface methods
    /// carry one. `annotation_follows` drops the trivia run's trailing
    /// annotation-line terminator (see [`crate::trivia::without_following_annotation`])
    /// — the annotation re-emitter writes that line break itself.
    pub(super) fn write_member_trivia(
        &self,
        def_id: DefId,
        annotation_follows: bool,
        writer: &mut CodeWriter,
    ) {
        let Some(trivia) = self.trivia.for_def(def_id) else {
            return;
        };
        let trivia = if annotation_follows {
            crate::trivia::without_following_annotation(trivia)
        } else {
            trivia
        };
        crate::trivia::write_leading_trivia(trivia, self.interner, writer);
    }

    /// Emit leading trivia owned by a shared field in a union declaration.
    pub(super) fn write_union_shared_field_trivia(
        &self,
        def_id: DefId,
        ordinal: u32,
        writer: &mut CodeWriter,
    ) {
        if let Some(trivia) = self.trivia.for_union_shared_field(def_id, ordinal) {
            crate::trivia::write_leading_trivia(trivia, self.interner, writer);
        }
    }

    /// `def_id` is the struct item's own definition; struct field type sites
    /// are keyed by it with the field ordinal (see the presentation witness
    /// registry), so field annotations re-emit their authored forms.
    pub(super) fn generate_struct(
        &self,
        def_id: DefId,
        s: &HirStruct,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(surface.contextual_keyword("genus"));
        writer.write(" ");
        writer.write(&self.symbol_to_string(s.name));

        self.write_generic_params(&s.type_params, names, surface, writer);

        if !s.implements.is_empty() {
            writer.write(" ");
            writer.write(surface.contextual_keyword("implet"));
            writer.write(" ");
            for (i, contract) in s.implements.iter().enumerate() {
                if i > 0 {
                    writer.write(", ");
                }
                self.write_contract_ref(contract, names, surface, writer);
            }
        }

        writer.writeln(" {");
        writer.indented(|writer| {
            for (ordinal, field) in s.fields.iter().enumerate() {
                self.write_member_trivia(field.def_id, !field.annotations.is_empty(), writer);
                self.write_hir_annotations(
                    &field.annotations,
                    names,
                    surface,
                    Some(field.def_id),
                    writer,
                );
                if let Some(modifier) = field.modifier {
                    writer.write(surface.keyword(match modifier {
                        radix_syntax::FieldModifier::Fixum => TokenKind::Sit,
                        radix_syntax::FieldModifier::Varia => TokenKind::Varia,
                        radix_syntax::FieldModifier::Const => TokenKind::Fixum,
                    }));
                    writer.write(" ");
                }
                let site = HirTypeSite {
                    owner: HirOwnerRef::Def(def_id),
                    slot: HirTypeSlot::Field(self.slot_ordinal(ordinal, "struct field count")),
                };
                let rendered = self.render_type_at(&site, field.ty, names, surface);
                writer.write(&rendered);
                writer.write(" ");
                writer.write(&self.symbol_to_string(field.name));
                self.write_sponte_marker(field.sponte, surface, writer);
                if let Some(init) = &field.init {
                    writer.write(" = ");
                    self.write_expr(init, names, surface, writer);
                }
                writer.newline();
            }

            for method in &s.methods {
                // Canonical member separator; when the method's trivia run owns
                // the boundary, this separator would double the line break the
                // run already reproduces (boundary-newline normalization).
                if self.trivia.for_def(method.def_id).is_none() {
                    writer.newline();
                }
                self.write_member_trivia(method.def_id, !method.annotations.is_empty(), writer);
                self.write_hir_annotations(
                    &method.annotations,
                    names,
                    surface,
                    Some(method.def_id),
                    writer,
                );
                self.generate_function(
                    &method.func,
                    &method.annotations,
                    Some(method.def_id),
                    names,
                    surface,
                    writer,
                );
            }
        });
        writer.writeln("}");
    }

    fn write_test_modifier(
        &self,
        modifier: &HirTestModifier,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match modifier {
            HirTestModifier::Erratur => {
                writer.write(surface.keyword(TokenKind::Erratur));
            }
            HirTestModifier::Omitte(reason) => {
                self.write_quoted_test_token(TokenKind::Omitte, *reason, surface, writer);
            }
            HirTestModifier::Futurum(reason) => {
                self.write_quoted_test_token(TokenKind::Futurum, *reason, surface, writer);
            }
            HirTestModifier::Solum => {
                writer.write(surface.keyword(TokenKind::Solum));
            }
            HirTestModifier::Tag(tag) => {
                self.write_quoted_test_token(TokenKind::Tag, *tag, surface, writer);
            }
            HirTestModifier::Temporis(n) => {
                writer.write(surface.keyword(TokenKind::Temporis));
                writer.write(" ");
                writer.write(&n.to_string());
            }
            HirTestModifier::Metior => {
                writer.write(surface.keyword(TokenKind::Metior));
            }
            HirTestModifier::Repete(n) => {
                writer.write(surface.keyword(TokenKind::Repete));
                writer.write(" ");
                writer.write(&n.to_string());
            }
            HirTestModifier::Fragilis(n) => {
                writer.write(surface.keyword(TokenKind::Fragilis));
                writer.write(" ");
                writer.write(&n.to_string());
            }
            HirTestModifier::SolumIn(env) => {
                self.write_quoted_test_token(TokenKind::SolumIn, *env, surface, writer);
            }
        }
    }

    pub(super) fn generate_enum(
        &self,
        def_id: DefId,
        e: &HirEnum,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // Authored `ordo` enumerations lower into the union-shaped HIR; the
        // presentation facts mark which items re-sugar with the `ordo`
        // vocabulary and their member discriminants.
        let ordo_members = self.ordo_enums.get(&def_id);
        // Enum variants are normalized to the block form that remains valid
        // for both plain variants and variants carrying named fields.
        writer.write(if ordo_members.is_some() {
            surface.keyword(TokenKind::Ordo)
        } else {
            surface.contextual_keyword("discretio")
        });
        writer.write(" ");
        writer.write(&self.symbol_to_string(e.name));

        self.write_generic_params(&e.type_params, names, surface, writer);

        writer.writeln(" {");
        writer.indented(|writer| {
            if !e.commune_fields.is_empty() {
                // The source's `@ commune` annotation belongs to the first
                // shared-field declaration. Its leading comments must precede
                // that annotation so they remain in a legal attachment slot.
                self.write_union_shared_field_trivia(def_id, 0, writer);
                writer.write("@ ");
                writer.write(surface.contextual_keyword("commune"));
                writer.newline();
                for (ordinal, field) in e.commune_fields.iter().enumerate() {
                    if ordinal > 0 {
                        self.write_union_shared_field_trivia(
                            def_id,
                            self.slot_ordinal(ordinal, "commune field count"),
                            writer,
                        );
                    }
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Def(def_id),
                        slot: HirTypeSlot::Field(self.slot_ordinal(ordinal, "commune field count")),
                    };
                    let rendered = self.render_type_at(&site, field.ty, names, surface);
                    writer.write(&rendered);
                    writer.write(" ");
                    writer.write(&self.symbol_to_string(field.name));
                    writer.newline();
                }
                if e.variants
                    .first()
                    .is_none_or(|variant| self.trivia.for_def(variant.def_id).is_none())
                {
                    writer.newline();
                }
            }
            for (idx, variant) in e.variants.iter().enumerate() {
                self.write_member_trivia(variant.def_id, false, writer);
                writer.write(&self.symbol_to_string(variant.name));
                if let Some(value) = ordo_members
                    .and_then(|members| members.iter().find(|m| m.def_id == variant.def_id))
                    .and_then(|member| member.value.as_ref())
                {
                    writer.write(" = ");
                    match value {
                        radix_hir::HirOrdoValue::Integer(int) => {
                            writer.write(&int.to_string());
                        }
                        radix_hir::HirOrdoValue::String(text) => {
                            self.write_textus_literal(*text, writer);
                        }
                    }
                }
                if !variant.fields.is_empty() {
                    writer.writeln(" {");
                    writer.indented(|writer| {
                        for (ordinal, field) in variant.fields.iter().enumerate() {
                            let site = HirTypeSite {
                                owner: HirOwnerRef::Def(variant.def_id),
                                slot: HirTypeSlot::Field(
                                    self.slot_ordinal(ordinal, "variant field count"),
                                ),
                            };
                            let rendered = self.render_type_at(&site, field.ty, names, surface);
                            writer.write(&rendered);
                            writer.write(" ");
                            writer.write(&self.symbol_to_string(field.name));
                            writer.newline();
                        }
                    });
                    writer.write("}");
                }
                // Canonical union layout separates variants with a comma; the
                // last variant carries none (mirror of the author/pretty
                // printers, which the parser round-trips unchanged).
                if idx + 1 < e.variants.len() {
                    writer.writeln(",");
                } else {
                    writer.newline();
                }
            }
        });
        writer.writeln("}");
    }

    pub(super) fn generate_interface(
        &self,
        i: &HirInterface,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // Interface methods are declarations only. Bodies belong to concrete
        // functions and methods, so the backend emits signatures and effects.
        writer.write(surface.contextual_keyword("implendum"));
        writer.write(" ");
        writer.write(&self.symbol_to_string(i.name));

        self.write_generic_params(&i.type_params, names, surface, writer);

        writer.writeln(" {");
        writer.indented(|writer| {
            for method in &i.methods {
                self.write_member_trivia(method.def_id, !method.availability.is_empty(), writer);
                for availability in &method.availability {
                    self.write_availability(availability, surface, writer);
                }
                writer.write(surface.contextual_keyword("functio"));
                writer.write(" ");
                writer.write(&self.symbol_to_string(method.name));
                writer.write("(");
                self.write_param_list(&method.params, names, surface, writer);
                writer.write(")");
                self.write_callable_posture(method.posture, surface, writer);
                self.write_function_result_types(
                    method.ret_ty,
                    method.err_ty,
                    None,
                    names,
                    surface,
                    writer,
                );
                writer.newline();
            }
        });
        writer.writeln("}");
    }

    fn write_availability(
        &self,
        availability: &HirAvailability,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match availability {
            HirAvailability::Nondum { target, reason } => {
                writer.write("@ ");
                // Surface-projected spelling: the availability family is a
                // contextual keyword (`unstable` under `en`, `nondum` under
                // `la`); a hardcoded Latin spelling fails PARSE018 on re-parse.
                writer.write(surface.contextual_keyword("nondum"));
                if target.is_none() && reason.is_none() {
                    // Bare `@ nondum` is the legal spelling when neither field
                    // is authored; an empty braced record is not the canonical
                    // shape and would print noise (`{  }`) after the keyword.
                    writer.newline();
                    return;
                }
                writer.write(" { ");
                if let Some(target) = target {
                    writer.write("target = ");
                    writer.write(&self.symbol_to_string(*target));
                }
                if target.is_some() && reason.is_some() {
                    writer.write(", ");
                }
                if let Some(reason) = reason {
                    writer.write("ratio = ");
                    self.write_textus_literal(*reason, writer);
                }
                writer.writeln(" }");
            }
        }
    }

    fn write_param(
        &self,
        param: &HirParam,
        slot: HirTypeSlot,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // A type-position mode marker already rides the param type, and
        // `type_to_faber` prints it. Imported/synthetic signatures can still
        // carry a separate HIR mode, so re-home that fact on the type instead
        // of emitting the retired parameter-prefix slot.
        let type_carries_mode = matches!(self.types.get(param.ty), radix_types::Type::Ref(..));
        if !type_carries_mode {
            let marker = match param.mode {
                radix_hir::HirParamMode::De => Some(TokenKind::De),
                radix_hir::HirParamMode::In => Some(TokenKind::In),
                radix_hir::HirParamMode::Ex => Some(TokenKind::Own),
                radix_hir::HirParamMode::Axis => Some(TokenKind::Axis),
                radix_hir::HirParamMode::Owned => None,
            };
            if let Some(marker) = marker {
                writer.write(surface.keyword(marker));
                writer.write(" ");
            }
        }
        let site = HirTypeSite {
            owner: HirOwnerRef::Def(param.def_id),
            slot,
        };
        let rendered = self.render_type_at(&site, param.ty, names, surface);
        writer.write(&rendered);
        writer.write(" ");
        writer.write(&self.symbol_to_string(param.name));
        self.write_sponte_marker(param.sponte, surface, writer);
        if let Some(default) = &param.default {
            writer.write(" ");
            writer.write(surface.contextual_keyword("vel"));
            writer.write(" ");
            self.write_expr(default, names, surface, writer);
        }
    }

    pub(super) fn write_sponte_marker(
        &self,
        sponte: bool,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if sponte {
            writer.write(" ");
            writer.write(surface.contextual_keyword("sponte"));
        }
    }

    fn write_param_list(
        &self,
        params: &[HirParam],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for (idx, param) in params.iter().enumerate() {
            if idx > 0 {
                writer.write(", ");
            }
            self.write_param(
                param,
                HirTypeSlot::Parameter(self.slot_ordinal(idx, "parameter count")),
                names,
                surface,
                writer,
            );
        }
    }

    fn write_function_result_types(
        &self,
        ret_ty: Option<TypeId>,
        err_ty: Option<TypeId>,
        owner: Option<DefId>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(ret) = ret_ty {
            writer.write(" → ");
            let rendered = owner.map_or_else(
                || self.type_to_faber(ret, names, surface),
                |owner| {
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Def(owner),
                        slot: HirTypeSlot::Return,
                    };
                    self.render_type_at(&site, ret, names, surface)
                },
            );
            writer.write(&rendered);
        }
        if let Some(err) = err_ty {
            writer.write(" ⇥ ");
            let rendered = owner.map_or_else(
                || self.type_to_faber(err, names, surface),
                |owner| {
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Def(owner),
                        slot: HirTypeSlot::Error,
                    };
                    self.render_type_at(&site, err, names, surface)
                },
            );
            writer.write(&rendered);
        }
    }

    fn write_optional_block_body(
        &self,
        body: Option<&HirBlock>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(body) = body {
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_block(body, names, surface, writer);
                self.write_block_end_trivia(body, writer);
            });
            writer.writeln("}");
        } else {
            writer.newline();
        }
    }

    fn write_callable_posture(
        &self,
        posture: CallablePosture,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let Some(keyword) = callable_posture_keyword(posture) else {
            return;
        };
        writer.write(" ");
        writer.write(surface.contextual_keyword(keyword));
    }

    fn write_quoted_test_token(
        &self,
        kind: TokenKind,
        token: Symbol,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(surface.keyword(kind));
        writer.write(" ");
        self.write_quoted_text(self.interner.resolve(token), writer);
    }

    fn write_generic_params(
        &self,
        params: &[HirTypeParam],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if params.is_empty() {
            return;
        }
        writer.write("<");
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                writer.write(", ");
            }
            match param.kind {
                GenericParamKind::Typus => {
                    writer.write(&self.symbol_to_string(param.name));
                    if let HirTypeParamConstraint::Implet(bounds) = &param.constraint {
                        writer.write(" ");
                        writer.write(surface.contextual_keyword("implet"));
                        writer.write(" ");
                        for (i, bound) in bounds.iter().enumerate() {
                            if i > 0 {
                                writer.write(" ∩ ");
                            }
                            self.write_contract_ref(bound, names, surface, writer);
                        }
                    }
                }
                GenericParamKind::Magnitudo => {
                    writer.write(surface.keyword(TokenKind::Magnitudo));
                    writer.write(" ");
                    writer.write(&self.symbol_to_string(param.name));
                }
            }
            if let Some(default) = &param.default {
                writer.write(" = ");
                match default {
                    SemanticGenericParamDefault::Typus(ty) => {
                        writer.write(&self.type_to_faber(*ty, names, surface));
                    }
                    SemanticGenericParamDefault::Magnitudo(value) => {
                        writer.write(&value.to_string());
                    }
                }
            }
        }
        writer.write(">");
    }

    /// `Contract` or `Contract<A, B>` as written in an `implet` clause.
    fn write_contract_ref(
        &self,
        contract: &ContractRef,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(&self.name_for_def_imported(contract.contract, names));
        if contract.args.is_empty() {
            return;
        }
        writer.write("<");
        for (i, arg) in contract.args.iter().enumerate() {
            if i > 0 {
                writer.write(", ");
            }
            writer.write(&self.type_to_faber(*arg, names, surface));
        }
        writer.write(">");
    }
}

fn callable_posture_keyword(posture: CallablePosture) -> Option<&'static str> {
    match posture {
        CallablePosture::SyncFinite => None,
        CallablePosture::AsyncFinite => Some("fiet"),
        CallablePosture::SyncStream => Some("fiunt"),
        CallablePosture::AsyncStream => Some("fient"),
    }
}

/// True when a legacy `@ futura` / `@ cursor` annotation already spells posture.
fn has_legacy_posture_annotation(annotations: &[HirAnnotation], interner: &Interner) -> bool {
    annotations.iter().any(|annotation| {
        let family = interner.resolve(annotation.family);
        family == "futura" || family == "cursor"
    })
}
