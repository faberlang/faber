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
    HirInterface, HirItem, HirItemKind, HirParam, HirStruct, HirTestModifier, HirTypeParam,
};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_syntax::{CallablePosture, GenericParamKind};
use radix_types::TypeId;
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

impl super::FaberCodegen {
    /// Return whether a function is compiler-carried test syntax.
    ///
    /// `proba` lowers through HIR as a function with test metadata. Re-emission
    /// must restore the user-facing test declaration so canonical Faber output
    /// does not expose that internal representation.
    pub(super) fn is_synthetic_proba_function(&self, func: &HirFunction) -> bool {
        func.test.is_some()
    }

    /// Emit a top-level declaration in canonical Faber syntax.
    ///
    /// The match mirrors the declaration surface understood by HIR: functions,
    /// type aliases, constants, imports, structs, enums, and interfaces. Each
    /// arm writes normalized Faber rather than original source trivia.
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
                self.generate_function(func, types, names, interner, surface, writer);
            }
            HirItemKind::Struct(s) => {
                self.generate_struct(s, types, names, interner, surface, writer);
            }
            HirItemKind::Enum(e) => {
                self.generate_enum(e, types, names, interner, surface, writer);
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
                writer.write(&self.type_to_faber(a.ty, types, names, interner, surface));
                writer.newline();
            }
            HirItemKind::Constant(c) => {
                writer.write(surface.contextual_keyword("fixum"));
                writer.write(" ");
                if let Some(ty) = c.ty {
                    writer.write(&self.type_to_faber(ty, types, names, interner, surface));
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
                writer.write(" ");
                writer.write(match import.visibility {
                    radix_syntax::Visibility::Privata => surface.keyword(TokenKind::Privata),
                    radix_syntax::Visibility::Publica => surface.keyword(TokenKind::Publica),
                });
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

    /// Emit a function or method signature plus its optional body.
    ///
    /// Async and generator flags return to their canonical annotation tokens.
    /// Parameter mode, optionality, result type, and error type are emitted from
    /// semantic HIR, so this function should not invent defaults for missing
    /// type information.
    pub(super) fn generate_function(
        &self,
        func: &HirFunction,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if self.is_synthetic_proba_function(func) {
            self.generate_proba_function(func, types, names, interner, surface, writer);
            return;
        }

        self.write_hir_annotations(&func.annotations, types, names, interner, surface, writer);

        writer.write(surface.contextual_keyword("functio"));
        writer.write(" ");
        writer.write(&self.symbol_to_string(func.name, interner));
        self.write_generic_params(&func.type_params, interner, surface, writer);
        writer.write("(");
        self.write_param_list(&func.params, types, names, interner, surface, writer);
        writer.write(")");
        if !has_legacy_posture_annotation(&func.annotations, interner) {
            self.write_callable_posture(func.posture, surface, writer);
        }
        self.write_function_result_types(
            func.ret_ty,
            func.err_ty,
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

    /// Emit member leading trivia for a genus field or method.
    ///
    /// Member owners are back-referenced by [`DefId`] (`HirField.def_id`,
    /// `HirMethod.def_id`). Interface methods carry no `DefId` in the current
    /// HIR model, so their member anchoring is U-3's contract to provide.
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
        // Structs preserve their inheritance/interface relationships and
        // member order, but not author layout inside the original declaration.
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
                self.generate_function(&method.func, types, names, interner, surface, writer);
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
        // Test modifiers are emitted as the canonical surface tokens accepted
        // after a `proba` name; no target-specific test runner syntax leaks out.
        match modifier {
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
            HirTestModifier::Requirit(req) => {
                self.write_quoted_test_token(TokenKind::Requirit, *req, interner, surface, writer);
            }
            HirTestModifier::SolumIn(env) => {
                self.write_quoted_test_token(TokenKind::SolumIn, *env, interner, surface, writer);
            }
        }
    }

    pub(super) fn generate_enum(
        &self,
        e: &HirEnum,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // Enum variants are normalized to the block form that remains valid
        // for both plain variants and variants carrying named fields.
        writer.write(surface.contextual_keyword("discretio"));
        writer.write(" ");
        writer.write(&self.symbol_to_string(e.name, interner));

        self.write_generic_params(&e.type_params, interner, surface, writer);

        writer.writeln(" {");
        writer.indented(|writer| {
            for variant in &e.variants {
                self.write_member_trivia(variant.def_id, false, interner, writer);
                writer.write(&self.symbol_to_string(variant.name, interner));
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
                writer.writeln(",");
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
                    self.write_availability(availability, interner, writer);
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
        writer: &mut CodeWriter,
    ) {
        match availability {
            HirAvailability::Nondum { target, reason } => {
                writer.write("@ nondum { ");
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

    /// Emit one type-first parameter with canonical post-name declaration markers.
    fn write_param(
        &self,
        param: &HirParam,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match param.mode {
            radix_hir::HirParamMode::De => {
                writer.write(surface.keyword(TokenKind::De));
                writer.write(" ");
            }
            radix_hir::HirParamMode::In => {
                writer.write(surface.keyword(TokenKind::In));
                writer.write(" ");
            }
            radix_hir::HirParamMode::Ex => {
                writer.write(surface.keyword(TokenKind::Ex));
                writer.write(" ");
            }
            radix_hir::HirParamMode::Owned => {}
        }
        writer.write(&self.type_to_faber(param.ty, types, names, interner, surface));
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

    /// Emit post-name declaration markers in the grammar's canonical order.
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
            self.write_param(param, types, names, interner, surface, writer);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write_function_result_types(
        &self,
        ret_ty: Option<TypeId>,
        err_ty: Option<TypeId>,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(ret) = ret_ty {
            writer.write(" → ");
            writer.write(&self.type_to_faber(ret, types, names, interner, surface));
        }
        if let Some(err) = err_ty {
            writer.write(" ⇥ ");
            writer.write(&self.type_to_faber(err, types, names, interner, surface));
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

fn has_legacy_posture_annotation(annotations: &[HirAnnotation], interner: &Interner) -> bool {
    annotations.iter().any(|annotation| {
        let family = interner.resolve(annotation.family);
        family == "futura" || family == "cursor"
    })
}
