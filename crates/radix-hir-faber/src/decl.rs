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
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirAnnotation, HirAvailability, HirBlock, HirEnum, HirExpressionKind, HirFunction,
    HirInterface, HirItem, HirItemKind, HirOwnerRef, HirParam, HirStruct, HirTestModifier,
    HirTypeParam, HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_syntax::{CallablePosture, GenericParamKind};
use radix_types::{TypeId, TypeTable};
use rustc_hash::FxHashMap;

impl super::FaberCodegen {
    pub(super) fn generate_item(
        &self,
        item: &HirItem,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match &item.kind {
            HirItemKind::Function(func) => {
                self.generate_function(
                    func,
                    Some(item.def_id),
                    types,
                    names,
                    interner,
                    surface,
                    writer,
                );
            }
            HirItemKind::Struct(s) => {
                self.generate_struct(s, types, names, interner, surface, writer);
            }
            HirItemKind::Enum(e) => {
                self.generate_enum(item.def_id, e, types, names, interner, surface, writer);
            }
            HirItemKind::Interface(i) => {
                self.generate_interface(i, types, names, interner, surface, writer);
            }
            HirItemKind::TypeAlias(a) => {
                writer.write(surface.contextual_keyword("typus"));
                writer.write(" ");
                writer.write(&self.symbol_to_string(a.name, interner));
                self.write_generic_params(&a.type_params, interner, surface, writer);
                writer.write(" = ");
                let site = HirTypeSite {
                    owner: HirOwnerRef::Def(item.def_id),
                    slot: HirTypeSlot::AliasBody,
                };
                let ty = self
                    .type_to_faber_at(&site, a.ty, types, names, interner, surface)
                    .unwrap_or_else(|error| self.record_type_emission_error(error));
                writer.write(&ty);
                writer.newline();
            }
            HirItemKind::Schema(schema) => {
                writer.write(surface.keyword(TokenKind::Schema));
                writer.write(" ");
                writer.write(&self.symbol_to_string(schema.name, interner));
                writer.writeln(" {");
                writer.indented(|writer| {
                    for column in &schema.columns {
                        writer.write(surface.keyword(TokenKind::Columna));
                        writer.write(" ");
                        writer
                            .write(&self.type_to_faber(column.ty, types, names, interner, surface));
                        writer.write(" ");
                        writer.write(&self.symbol_to_string(column.name, interner));
                        if column.source != column.name {
                            writer.write(" : ");
                            writer.write(&self.symbol_to_string(column.source, interner));
                        }
                        writer.newline();
                    }
                });
                writer.write("}");
                writer.newline();
            }
            HirItemKind::Constant(c) => {
                writer.write(surface.contextual_keyword("fixum"));
                writer.write(" ");
                if let Some(ty) = c.ty {
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Def(item.def_id),
                        slot: HirTypeSlot::Declared,
                    };
                    // The authored form at this site is the sole authority for
                    // concrete versus hole spelling. An absent site is a
                    // synthesized/hand-built binding and uses the canonical
                    // concrete fallback inside `type_to_faber_at`.
                    let rendered = self
                        .type_to_faber_at(&site, ty, types, names, interner, surface)
                        .unwrap_or_else(|error| self.record_type_emission_error(error));
                    writer.write(&rendered);
                    writer.write(" ");
                }
                writer.write(&self.symbol_to_string(c.name, interner));
                if let HirExpressionKind::ConversioAssign {
                    source, recovery, ..
                } = &c.value.kind
                {
                    // Typed `↤` constant initializer: lowering stores the
                    // binding as the assignment target, so re-emit as
                    // `fixum T name ↤ source [⇥ recovery]`.
                    writer.write(" ↤ ");
                    self.write_expr_prec(source, 1, types, names, interner, surface, writer);
                    if let Some(recovery) = recovery {
                        writer.write(" ⇥ ");
                        self.write_expr_prec(recovery, 12, types, names, interner, surface, writer);
                    }
                } else {
                    writer.write(" ← ");
                    self.write_expr(&c.value, types, names, interner, surface, writer);
                }
                writer.newline();
            }
            HirItemKind::Import(import) => {
                writer.write(surface.keyword(TokenKind::Importa));
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ex));
                writer.write(" ");
                self.write_symbol_literal(import.path, interner, writer);
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
                        let name = self.symbol_to_string(item.name, interner);
                        match item.alias {
                            Some(alias) if alias != item.name => {
                                writer.write(&name);
                                writer.write(" ");
                                writer.write(surface.keyword(TokenKind::Ut));
                                writer.write(" ");
                                writer.write(&self.symbol_to_string(alias, interner));
                            }
                            _ => writer.write(&name),
                        }
                    }
                    writer.newline();
                    return;
                }
                for item in &import.items {
                    writer.write(" ");
                    let name = self.symbol_to_string(item.name, interner);
                    if item.alias == Some(item.name) {
                        writer.write("* ");
                        writer.write(surface.keyword(TokenKind::Ut));
                        writer.write(" ");
                        writer.write(&name);
                    } else if let Some(alias) = item.alias {
                        let alias = self.symbol_to_string(alias, interner);
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
        }
    }

    pub(super) fn generate_function(
        &self,
        func: &HirFunction,
        owner: Option<DefId>,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if func.test.is_some() {
            self.generate_proba_function(func, types, names, interner, surface, writer);
            return;
        }

        self.write_hir_annotations(
            &func.annotations,
            types,
            names,
            interner,
            surface,
            owner,
            writer,
        );

        writer.write(surface.contextual_keyword("functio"));
        writer.write(" ");
        writer.write(&self.symbol_to_string(func.name, interner));
        self.write_generic_params(&func.type_params, interner, surface, writer);
        writer.write("(");
        self.write_param_list(&func.params, types, names, interner, surface, writer);
        writer.write(")");
        if let Some(cli_args) = &func.cli_args {
            // `functio f() argumenta args` sugar: the CLI bundle param is not
            // part of `params`, so its binding re-emits in the postfix slot.
            writer.write(" ");
            writer.write(surface.keyword(TokenKind::Argumenta));
            writer.write(" ");
            writer.write(&self.symbol_to_string(cli_args.name, interner));
        }
        if !has_legacy_posture_annotation(&func.annotations, interner) {
            self.write_callable_posture(func.posture, surface, writer);
        }
        self.write_function_result_types(
            func.ret_ty,
            func.err_ty,
            owner,
            types,
            names,
            interner,
            surface,
            writer,
        );
        self.write_optional_block_body(func.body.as_ref(), types, names, interner, surface, writer);
    }

    /// Emit a synthetic HIR test function as a source-level `proba` block.
    pub(super) fn generate_proba_function(
        &self,
        func: &HirFunction,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // HIR stores test modifiers separately from the function signature so
        // the backend can recover canonical `proba "name" ... {}` syntax.
        writer.write(surface.keyword(TokenKind::Proba));
        writer.write(" ");
        if let Some(test) = &func.test {
            self.write_quoted_text(interner.resolve(test.name), writer);
            for modifier in &test.modifiers {
                writer.write(" ");
                self.write_test_modifier(modifier, interner, surface, writer);
            }
        } else {
            self.write_quoted_text(interner.resolve(func.name), writer);
        }

        self.write_optional_block_body(func.body.as_ref(), types, names, interner, surface, writer);
    }

    /// Emit leading trivia owned by a member `DefId`.
    ///
    /// Genus fields, genus methods, and enum variants carry one. Interface
    /// methods do not, so they skip this helper.
    ///
    /// `annotation_follows` drops the trivia run's trailing annotation-line
    /// terminator (see [`crate::trivia::without_following_annotation`]) — the
    /// annotation re-emitter writes that line break itself.
    pub(super) fn write_member_trivia(
        &self,
        def_id: DefId,
        annotation_follows: bool,
        interner: &Interner,
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
        crate::trivia::write_leading_trivia(trivia, interner, writer);
    }

    pub(super) fn generate_struct(
        &self,
        s: &HirStruct,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_hir_annotations(
            &s.annotations,
            types,
            names,
            interner,
            surface,
            None,
            writer,
        );

        writer.write(surface.contextual_keyword("genus"));
        writer.write(" ");
        writer.write(&self.symbol_to_string(s.name, interner));

        self.write_generic_params(&s.type_params, interner, surface, writer);

        if let Some(parent) = s.extends {
            writer.write(" ");
            writer.write(surface.contextual_keyword("sub"));
            writer.write(" ");
            writer.write(&self.name_for_def(parent, names, interner));
        }

        if !s.implements.is_empty() {
            writer.write(" ");
            writer.write(surface.contextual_keyword("implet"));
            writer.write(" ");
            for (i, interface) in s.implements.iter().enumerate() {
                if i > 0 {
                    writer.write(", ");
                }
                writer.write(&self.name_for_def(*interface, names, interner));
            }
        }

        writer.writeln(" {");
        writer.indented(|writer| {
            for field in &s.fields {
                self.write_member_trivia(field.def_id, false, interner, writer);
                if field.is_static {
                    writer.write(surface.keyword(TokenKind::Generis));
                    writer.write(" ");
                }
                writer.write(&self.type_to_faber(field.ty, types, names, interner, surface));
                writer.write(" ");
                writer.write(&self.symbol_to_string(field.name, interner));
                self.write_sponte_marker(field.sponte, surface, writer);
                if let Some(init) = &field.init {
                    writer.write(" = ");
                    self.write_expr(init, types, names, interner, surface, writer);
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
                self.write_member_trivia(
                    method.def_id,
                    !method.func.annotations.is_empty(),
                    interner,
                    writer,
                );
                self.generate_function(
                    &method.func,
                    Some(method.def_id),
                    types,
                    names,
                    interner,
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
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match modifier {
            HirTestModifier::Erratur => {
                writer.write(surface.keyword(TokenKind::Erratur));
            }
            HirTestModifier::Omitte(reason) => {
                self.write_quoted_test_token(TokenKind::Omitte, *reason, interner, surface, writer);
            }
            HirTestModifier::Futurum(reason) => {
                self.write_quoted_test_token(
                    TokenKind::Futurum,
                    *reason,
                    interner,
                    surface,
                    writer,
                );
            }
            HirTestModifier::Solum => {
                writer.write(surface.keyword(TokenKind::Solum));
            }
            HirTestModifier::Tag(tag) => {
                self.write_quoted_test_token(TokenKind::Tag, *tag, interner, surface, writer);
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
                self.write_quoted_test_token(TokenKind::SolumIn, *env, interner, surface, writer);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn generate_enum(
        &self,
        def_id: DefId,
        e: &HirEnum,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // Authored `ordo` enumerations lower into the union-shaped HIR; the
        // presentation facts mark which items re-sugar with the `ordo`
        // vocabulary and their member discriminants.
        let ordo_members = self.ordo_enums.get(&def_id);
        // Enum variants are normalized to the block form that remains valid
        // for both plain variants and variants carrying named fields.
        self.write_hir_annotations(
            &e.annotations,
            types,
            names,
            interner,
            surface,
            None,
            writer,
        );

        writer.write(if ordo_members.is_some() {
            surface.keyword(TokenKind::Ordo)
        } else {
            surface.contextual_keyword("discretio")
        });
        writer.write(" ");
        writer.write(&self.symbol_to_string(e.name, interner));

        self.write_generic_params(&e.type_params, interner, surface, writer);

        writer.writeln(" {");
        writer.indented(|writer| {
            if !e.commune_fields.is_empty() {
                writer.write("@ ");
                writer.write(surface.contextual_keyword("commune"));
                writer.newline();
                for field in &e.commune_fields {
                    writer.write(&self.type_to_faber(field.ty, types, names, interner, surface));
                    writer.write(" ");
                    writer.write(&self.symbol_to_string(field.name, interner));
                    writer.newline();
                }
                writer.newline();
            }
            for (idx, variant) in e.variants.iter().enumerate() {
                self.write_member_trivia(variant.def_id, false, interner, writer);
                writer.write(&self.symbol_to_string(variant.name, interner));
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
                            self.write_textus_literal(*text, interner, writer);
                        }
                    }
                }
                if !variant.fields.is_empty() {
                    writer.writeln(" {");
                    writer.indented(|writer| {
                        for field in &variant.fields {
                            writer.write(
                                &self.type_to_faber(field.ty, types, names, interner, surface),
                            );
                            writer.write(" ");
                            writer.write(&self.symbol_to_string(field.name, interner));
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
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // Interface methods are declarations only. Bodies belong to concrete
        // functions and methods, so the backend emits signatures and effects.
        writer.write(surface.contextual_keyword("implendum"));
        writer.write(" ");
        writer.write(&self.symbol_to_string(i.name, interner));

        self.write_generic_params(&i.type_params, interner, surface, writer);

        writer.writeln(" {");
        writer.indented(|writer| {
            for method in &i.methods {
                for availability in &method.availability {
                    self.write_availability(availability, interner, surface, writer);
                }
                writer.write(surface.contextual_keyword("functio"));
                writer.write(" ");
                writer.write(&self.symbol_to_string(method.name, interner));
                writer.write("(");
                self.write_param_list(&method.params, types, names, interner, surface, writer);
                writer.write(")");
                self.write_callable_posture(method.posture, surface, writer);
                self.write_function_result_types(
                    method.ret_ty,
                    method.err_ty,
                    None,
                    types,
                    names,
                    interner,
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
        interner: &Interner,
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
                writer.write(" { ");
                if let Some(target) = target {
                    writer.write("target = ");
                    writer.write(&self.symbol_to_string(*target, interner));
                }
                if target.is_some() && reason.is_some() {
                    writer.write(", ");
                }
                if let Some(reason) = reason {
                    writer.write("ratio = ");
                    self.write_textus_literal(*reason, interner, writer);
                }
                writer.writeln(" }");
            }
        }
    }

    fn write_param(
        &self,
        param: &HirParam,
        slot: HirTypeSlot,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // A type-position mode marker already rides the param type, and
        // `type_to_faber` prints it. Imported/synthetic signatures can still
        // carry a separate HIR mode, so re-home that fact on the type instead
        // of emitting the retired parameter-prefix slot.
        let type_carries_mode = matches!(types.get(param.ty), radix_types::Type::Ref(..));
        if !type_carries_mode {
            let marker = match param.mode {
                radix_hir::HirParamMode::De => Some(TokenKind::De),
                radix_hir::HirParamMode::In => Some(TokenKind::In),
                radix_hir::HirParamMode::Ex => Some(TokenKind::Own),
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
        let rendered = self
            .type_to_faber_at(&site, param.ty, types, names, interner, surface)
            .unwrap_or_else(|error| self.record_type_emission_error(error));
        writer.write(&rendered);
        writer.write(" ");
        writer.write(&self.symbol_to_string(param.name, interner));
        self.write_sponte_marker(param.sponte, surface, writer);
        if let Some(default) = &param.default {
            writer.write(" ");
            writer.write(surface.contextual_keyword("vel"));
            writer.write(" ");
            self.write_expr(default, types, names, interner, surface, writer);
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
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for (idx, param) in params.iter().enumerate() {
            if idx > 0 {
                writer.write(", ");
            }
            self.write_param(
                param,
                HirTypeSlot::Parameter(idx as u32),
                types,
                names,
                interner,
                surface,
                writer,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write_function_result_types(
        &self,
        ret_ty: Option<TypeId>,
        err_ty: Option<TypeId>,
        owner: Option<DefId>,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(ret) = ret_ty {
            writer.write(" → ");
            let rendered = owner
                .map(|owner| {
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Def(owner),
                        slot: HirTypeSlot::Return,
                    };
                    self.type_to_faber_at(&site, ret, types, names, interner, surface)
                        .unwrap_or_else(|error| self.record_type_emission_error(error))
                })
                .unwrap_or_else(|| self.type_to_faber(ret, types, names, interner, surface));
            writer.write(&rendered);
        }
        if let Some(err) = err_ty {
            writer.write(" ⇥ ");
            let rendered = owner
                .map(|owner| {
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Def(owner),
                        slot: HirTypeSlot::Error,
                    };
                    self.type_to_faber_at(&site, err, types, names, interner, surface)
                        .unwrap_or_else(|error| self.record_type_emission_error(error))
                })
                .unwrap_or_else(|| self.type_to_faber(err, types, names, interner, surface));
            writer.write(&rendered);
        }
    }

    fn write_optional_block_body(
        &self,
        body: Option<&HirBlock>,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(body) = body {
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_block(body, types, names, interner, surface, writer);
                self.write_block_end_trivia(body, interner, writer);
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
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(surface.keyword(kind));
        writer.write(" ");
        self.write_quoted_text(interner.resolve(token), writer);
    }

    fn write_generic_params(
        &self,
        params: &[HirTypeParam],
        interner: &Interner,
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
                    writer.write(&self.symbol_to_string(param.name, interner));
                }
                GenericParamKind::Magnitudo => {
                    writer.write(surface.keyword(TokenKind::Magnitudo));
                    writer.write(" ");
                    writer.write(&self.symbol_to_string(param.name, interner));
                }
            }
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
