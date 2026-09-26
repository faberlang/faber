//! Canonical Faber expression emission.
//!
//! This module is the expression half of the source-preserving Faber backend.
//! It prints HIR back into canonical Faber expression text so the compiler can
//! expose a normalized view of code after parsing, lowering, name resolution,
//! and type analysis. The goal is not byte-for-byte reconstruction: comments,
//! original whitespace, alternate spellings, and some syntactic sugar are
//! already gone by the time this writer sees HIR.
//!
//! INVARIANTS
//! ==========
//! - Calls, method calls, fields, indexes, optional chains, and non-null chains
//!   use the canonical postfix spellings accepted by the parser.
//! - Blocks and control forms are emitted as Faber constructs, not translated
//!   through another target backend.
//! - Object and collection entries are delegated to the literal field writer so
//!   literal and conversion forms share one field spelling policy.
//! - Parentheses are a precedence handoff with `ops.rs`; this file asks for the
//!   expression precedence and only adds grouping needed to keep reparsing valid.
//!
//! LIMITS
//! ======
//! Structured `cape` handlers are not reconstructed here yet, and diagnostic
//! placeholder expressions lower to grammar-valid fallback text. Template and
//! string payloads are written from interned text without claiming source escape
//! preservation.
//!
//! Adfirma optional message particles emit `surface.keyword(TokenKind::Mori)`
//! (`panic`). Leftover Secus is PARSE001 `adfirma_else_rejected` (`7805f8e1a`);
//! hardcoded `secus` (`e0fbae339`) is not a valid particle.

use crate::surface::{FrameFamily, KeywordSurface, TypeBindingKind};
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirArrayElement, HirBlock, HirCallArg, HirCape, HirConversioTarget, HirExpression,
    HirExpressionKind, HirNonNullKind, HirObjectField, HirObjectKey, HirOptionalChainKind,
    HirOwnerRef, HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_types::{Primitive, Type, TypeId};
use rustc_hash::FxHashMap;

use crate::ops::binop_left_associative;

#[derive(Clone, Copy)]
struct PostfixChainPrefixes {
    member: &'static str,
    index_open: &'static str,
    call_open: &'static str,
}

const OPTIONAL_CHAIN: PostfixChainPrefixes = PostfixChainPrefixes {
    member: "?.",
    index_open: "?[",
    call_open: "?(",
};

const NON_NULL_CHAIN: PostfixChainPrefixes = PostfixChainPrefixes {
    member: "!.",
    index_open: "![",
    call_open: "!(",
};

/// Bare integer literal for the range-spacing law: an
/// `Int(u64)` literal, or unary minus over one. Negative literals lower as
/// `Unary(Neg, Int)` (see `HirLiteral::Int`), and the `Unary` arm emits
/// `unop` + operand with no inner spacing, so `-1` is one operand.
fn is_bare_int_literal(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Literal(radix_hir::HirLiteral::Int(_)) => true,
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, operand) => matches!(
            operand.kind,
            HirExpressionKind::Literal(radix_hir::HirLiteral::Int(_))
        ),
        _ => false,
    }
}

#[derive(Clone, Copy)]
enum PostfixChainTail<'a> {
    Member(Symbol),
    Index(&'a HirExpression),
    Call(&'a [HirCallArg]),
}

impl<'a> From<&'a HirOptionalChainKind> for PostfixChainTail<'a> {
    fn from(kind: &'a HirOptionalChainKind) -> Self {
        match kind {
            HirOptionalChainKind::Member(name) => Self::Member(*name),
            HirOptionalChainKind::Index(index) => Self::Index(index),
            HirOptionalChainKind::Call(args) => Self::Call(args),
        }
    }
}

impl<'a> From<&'a HirNonNullKind> for PostfixChainTail<'a> {
    fn from(kind: &'a HirNonNullKind) -> Self {
        match kind {
            HirNonNullKind::Member(name) => Self::Member(*name),
            HirNonNullKind::Index(index) => Self::Index(index),
            HirNonNullKind::Call(args) => Self::Call(args),
        }
    }
}

impl super::FaberCodegen<'_> {
    /// Write an expression with enough parentheses to survive a Faber reparse.
    ///
    /// The `parent_prec` contract is shared with `expr_precedence` in `ops.rs`.
    /// Sub-writers pass the binding strength of the surrounding operator or
    /// postfix form; this writer canonicalizes grouping from HIR structure and
    /// does not attempt to preserve original redundant parentheses.
    pub(super) fn write_expr_prec(
        &self,
        expr: &HirExpression,
        parent_prec: u8,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let expr_prec = self.expr_precedence(expr);
        let needs_parens = expr_prec <= parent_prec && parent_prec != 0;
        if needs_parens {
            writer.write("(");
        }

        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                if self.self_def_ids.contains(def_id) {
                    // A `Path` to a struct/enum def in value position can only
                    // come from `ego` (self): a bare type name is not a value.
                    // Print the localized self keyword instead of the type name.
                    writer.write(surface.keyword(TokenKind::Ego));
                } else if let Some(spelling) = self.authored_variant_spellings.get(def_id) {
                    // Lowering keeps only the resolved variant `DefId`; use the
                    // captured source path when value emission must preserve
                    // its authored qualification.
                    writer.write(spelling);
                } else {
                    writer.write(&self.name_for_def_imported(*def_id, names));
                }
            }
            HirExpressionKind::Literal(lit) => self.write_literal(lit, surface, writer),
            HirExpressionKind::ReadLine(_) => writer.write(surface.contextual_keyword("lege")),
            HirExpressionKind::Binary(op, lhs, rhs) => {
                let op_prec = self.binop_precedence(*op);
                // Left operands of associative operators re-parse identically
                // without parens (`a + b + c`); the parenthesized form after a
                // statement-head callee (`scribe (a + b) + c`) is call syntax.
                let lhs_prec = if binop_left_associative(*op) {
                    op_prec.saturating_sub(1)
                } else {
                    op_prec
                };
                self.write_expr_prec(lhs, lhs_prec, names, surface, writer);
                writer.write(" ");
                writer.write(&self.binop_to_faber(*op, surface));
                writer.write(" ");
                self.write_expr_prec(rhs, op_prec, names, surface, writer);
            }
            HirExpressionKind::Unary(op, operand) => {
                writer.write(&self.unop_to_faber(*op, surface));
                self.write_expr_prec(operand, 12, names, surface, writer);
            }
            HirExpressionKind::Call(callee, type_args, args, _) => {
                // Postfix expression forms bind tighter than every infix form
                // currently emitted by this backend.
                let mut wrote_finge = false;
                if let HirExpressionKind::Path(def_id) = &callee.kind
                    && self.variant_def_ids.contains(def_id)
                {
                    self.write_finge_construction(*def_id, args, names, surface, writer);
                    wrote_finge = true;
                }
                if !wrote_finge {
                    self.write_expr_prec(callee, 13, names, surface, writer);
                    self.write_call_type_args(type_args, names, surface, writer);
                    writer.write("(");
                    self.write_call_args(args, names, surface, writer);
                    writer.write(")");
                }
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args, _) => {
                self.write_expr_prec(receiver, 13, names, surface, writer);
                writer.write(".");
                let method_name = self.method_spelling(receiver, *name, names, surface);
                writer.write(&method_name);
                self.write_call_type_args(type_args, names, surface, writer);
                writer.write("(");
                self.write_call_args(args, names, surface, writer);
                writer.write(")");
            }
            HirExpressionKind::Field(object, name) => {
                self.write_expr_prec(object, 13, names, surface, writer);
                writer.write(".");
                writer.write(&self.symbol_to_string(*name));
            }
            HirExpressionKind::Index(object, index) => {
                self.write_expr_prec(object, 13, names, surface, writer);
                // A by-label read on a labeled tuple was lowered to the same
                // constant-index node as `p[1]`; the recorded label restores
                // the authored `.gw`. Index reads keep their bracket form.
                if let Some(label) = self.tuple_label_accesses.get(&expr.id) {
                    writer.write(".");
                    writer.write(&self.symbol_to_string(*label));
                } else {
                    writer.write("[");
                    self.write_expr(index, names, surface, writer);
                    writer.write("]");
                }
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                self.write_expr_prec(call, 12, names, surface, writer);
                writer.write(" ∇");
                if !selected_arguments.is_empty() {
                    writer.write(" [");
                    for (index, selector) in selected_arguments.iter().enumerate() {
                        if index > 0 {
                            writer.write(", ");
                        }
                        self.write_expr(selector, names, surface, writer);
                    }
                    writer.write("]");
                }
            }
            HirExpressionKind::OptionalChain(object, chain) => {
                self.write_expr_prec(object, 13, names, surface, writer);
                self.write_postfix_chain_tail(
                    OPTIONAL_CHAIN,
                    chain.into(),
                    self.receiver_projects_intrinsic(object),
                    names,
                    surface,
                    writer,
                );
            }
            HirExpressionKind::NonNull(object, chain) => {
                self.write_expr_prec(object, 13, names, surface, writer);
                self.write_postfix_chain_tail(
                    NON_NULL_CHAIN,
                    chain.into(),
                    self.receiver_projects_intrinsic(object),
                    names,
                    surface,
                    writer,
                );
            }
            HirExpressionKind::Block(block) => {
                writer.writeln("{");
                writer.indented(|writer| {
                    self.write_block(block, names, surface, writer);
                    self.write_block_end_trivia(block, writer);
                });
                writer.write("}");
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch: None,
                else_block,
            } => {
                // Guard statements (`requirit`/`reice`) lower to this exact
                // shape; the presentation id set marks which `Si` nodes came
                // from a guard so authored look-alikes stay braced.
                if else_block.is_none()
                    && self.guard_si_ids.contains(&expr.id)
                    && self.write_guard_stmt(cond, then_block, names, surface, writer)
                {
                    if needs_parens {
                        writer.write(")");
                    }
                    return;
                }
                if self.write_sic_secus_chain(
                    cond,
                    then_block,
                    else_block.as_ref(),
                    names,
                    surface,
                    writer,
                ) {
                    if needs_parens {
                        writer.write(")");
                    }
                    return;
                }
                self.write_si_chain(
                    cond,
                    then_block,
                    None,
                    else_block.as_ref(),
                    names,
                    surface,
                    writer,
                );
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch: Some(catch),
                else_block,
            } => {
                self.write_si_chain(
                    cond,
                    then_block,
                    Some(catch.as_ref()),
                    else_block.as_ref(),
                    names,
                    surface,
                    writer,
                );
            }
            HirExpressionKind::Handled { body, catch } => {
                self.write_handled_expr(body, catch, names, surface, writer);
            }
            // UT: the capture boundary emits as its own surface form — `capta`
            // followed by the body block. The block's tail expression (the
            // success value) writes as the block's final expression statement.
            HirExpressionKind::Trap(body) => {
                writer.write(surface.keyword(TokenKind::Trap));
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block(body, names, surface, writer);
                    self.write_block_end_trivia(body, writer);
                });
                writer.write("}");
            }
            HirExpressionKind::Discerne {
                scrutinees,
                arms,
                exhaustive,
            } => {
                // Authored `elige` (en `switch`) lowers to this shape; the
                // presentation id set marks which `Discerne` nodes came from
                // one so authored vocabulary re-emits instead of `discerne`.
                let authored_elige =
                    !*exhaustive && scrutinees.len() == 1 && self.elige_expr_ids.contains(&expr.id);
                writer.write(if authored_elige {
                    surface.contextual_keyword("elige")
                } else {
                    surface.contextual_keyword("discerne")
                });
                if *exhaustive {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("omnia"));
                }
                writer.write(" ");
                for (idx, scrutinee) in scrutinees.iter().enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    self.write_expr(scrutinee, names, surface, writer);
                }
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_match_arms(arms, authored_elige, names, surface, writer);
                });
                writer.write("}");
            }
            HirExpressionKind::Loop(block) => {
                if let Some((body_stmts, cond)) = self.as_fac_loop(block) {
                    writer.write(surface.contextual_keyword("fac"));
                    writer.writeln(" {");
                    writer.indented(|writer| {
                        for stmt in body_stmts {
                            self.write_stmt(stmt, names, surface, writer);
                        }
                        self.write_block_end_trivia(block, writer);
                    });
                    writer.write("} ");
                    writer.write(surface.contextual_keyword("dum"));
                    writer.write(" ");
                    self.write_expr(cond, names, surface, writer);
                } else {
                    writer.write(surface.contextual_keyword("dum"));
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("verum"));
                    writer.writeln(" {");
                    writer.indented(|writer| {
                        self.write_block(block, names, surface, writer);
                        self.write_block_end_trivia(block, writer);
                    });
                    writer.write("}");
                }
            }
            HirExpressionKind::Dum(cond, block) => {
                writer.write(surface.contextual_keyword("dum"));
                writer.write(" ");
                self.write_expr(cond, names, surface, writer);
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block(block, names, surface, writer);
                    self.write_block_end_trivia(block, writer);
                });
                writer.write("}");
            }
            HirExpressionKind::Itera(mode, coordinates, _binding, binding_name, iter, block) => {
                writer.write(surface.keyword(TokenKind::Itera));
                writer.write(" ");
                writer.write(match mode {
                    radix_hir::HirIteraMode::Ex => surface.keyword(TokenKind::Ex),
                    radix_hir::HirIteraMode::De => surface.keyword(TokenKind::De),
                    // `ab` is a contextual keyword (tokenless); localize it
                    // through the reader pack like `iuncta`, with Latin
                    // fallback when a pack has no row.
                    radix_hir::HirIteraMode::Ab => surface.contextual_keyword("ab"),
                });
                writer.write(" ");
                self.write_expr(iter, names, surface, writer);
                // Coordinate clause from the shared `apud` production; the
                // bindings are loop-visible, so dropping them breaks bodies
                // that reference the coordinate.
                if !coordinates.is_empty() {
                    writer.write(" ");
                    writer.write(surface.keyword(TokenKind::Apud));
                    writer.write(" [");
                    for (index, (_, name)) in coordinates.iter().enumerate() {
                        if index > 0 {
                            writer.write(", ");
                        }
                        writer.write(&self.symbol_to_string(*name));
                    }
                    writer.write("]");
                }
                writer.write(" ");
                writer.write(surface.contextual_keyword("fixum"));
                writer.write(" ");
                writer.write(&self.symbol_to_string(*binding_name));
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block(block, names, surface, writer);
                    self.write_block_end_trivia(block, writer);
                });
                writer.write("}");
            }
            HirExpressionKind::IteraLockstep {
                sources,
                coordinates,
                bindings,
                body,
            } => {
                // Lockstep series: `itera ex s1, s2 [apud [i]] fixum b1, b2`.
                // One shared coordinate at most; the series binder names keep
                // their comma separation so the emit reparses byte-for-byte.
                writer.write(surface.keyword(TokenKind::Itera));
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ex));
                writer.write(" ");
                for (index, source) in sources.iter().enumerate() {
                    if index > 0 {
                        writer.write(", ");
                    }
                    self.write_expr(source, names, surface, writer);
                }
                if let Some((_, coordinate_name)) = coordinates.first() {
                    writer.write(" ");
                    writer.write(surface.keyword(TokenKind::Apud));
                    writer.write(" [");
                    writer.write(&self.symbol_to_string(*coordinate_name));
                    writer.write("]");
                }
                writer.write(" ");
                writer.write(surface.contextual_keyword("fixum"));
                writer.write(" ");
                for (index, (_, binding_name)) in bindings.iter().enumerate() {
                    if index > 0 {
                        writer.write(", ");
                    }
                    writer.write(&self.symbol_to_string(*binding_name));
                }
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block(body, names, surface, writer);
                    self.write_block_end_trivia(body, writer);
                });
                writer.write("}");
            }
            // NR-6 (nary-range-walk): the Cartesian range product emits as the
            // source-declared `ab` series — extents once in source order
            // (first range outermost), binders in the same order, no `apud`
            // clause (a product never zips). Locale spellings ride the same
            // keyword/contextual-keyword surface as the lockstep arm.
            HirExpressionKind::IteraRange {
                ranges,
                bindings,
                body,
            } => {
                writer.write(surface.keyword(TokenKind::Itera));
                writer.write(" ");
                writer.write(surface.contextual_keyword("ab"));
                writer.write(" ");
                for (index, range) in ranges.iter().enumerate() {
                    if index > 0 {
                        writer.write(", ");
                    }
                    self.write_expr(range, names, surface, writer);
                }
                writer.write(" ");
                writer.write(surface.contextual_keyword("fixum"));
                writer.write(" ");
                for (index, (_, binding_name)) in bindings.iter().enumerate() {
                    if index > 0 {
                        writer.write(", ");
                    }
                    writer.write(&self.symbol_to_string(*binding_name));
                }
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block(body, names, surface, writer);
                    self.write_block_end_trivia(body, writer);
                });
                writer.write("}");
            }
            HirExpressionKind::Intervallum {
                start,
                end,
                step,
                kind,
            } => {
                self.write_expr(start, names, surface, writer);
                // Need (operator-locked): range glyphs are tight iff
                // both endpoints are bare integer literals; every other infix
                // spacing rule pads both sides.
                let glyph = match kind {
                    radix_hir::HirRangeKind::Exclusive => "‥",
                    radix_hir::HirRangeKind::Inclusive => "…",
                };
                if is_bare_int_literal(start) && is_bare_int_literal(end) {
                    writer.write(glyph);
                } else {
                    writer.write(" ");
                    writer.write(glyph);
                    writer.write(" ");
                }
                self.write_expr(end, names, surface, writer);
                if let Some(step) = step {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("per"));
                    writer.write(" ");
                    self.write_expr(step, names, surface, writer);
                }
            }
            HirExpressionKind::Assign(lhs, rhs) => {
                self.write_expr_prec(lhs, 1, names, surface, writer);
                writer.write(" ← ");
                self.write_expr_prec(rhs, 1, names, surface, writer);
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
            } => {
                // Conversion-directed assignment: re-emit the exact `↤`
                // spelling (never `←` or `↦`). `⇥ recovery` is
                // `inlineRecovery := '⇥' unary`, so the recovery operand is
                // written at unary precedence to keep parens that let it
                // reparse attached to this `↤` (the nearest in a chain).
                self.write_expr_prec(target, 1, names, surface, writer);
                writer.write(" ↤ ");
                self.write_expr_prec(source, 1, names, surface, writer);
                if let Some(recovery) = recovery {
                    writer.write(" ⇥ ");
                    self.write_expr_prec(recovery, 12, names, surface, writer);
                }
            }

            HirExpressionKind::Array(elements) => {
                writer.write("[");
                for (idx, elem) in elements.iter().enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    match elem {
                        HirArrayElement::Expr(expr) => {
                            self.write_expr(expr, names, surface, writer);
                        }
                        HirArrayElement::Spread(expr) => {
                            writer.write(surface.keyword(TokenKind::Sparge));
                            writer.write(" ");
                            self.write_expr(expr, names, surface, writer);
                        }
                    }
                }
                writer.write("]");
            }
            HirExpressionKind::Vacua => {
                writer.write(surface.contextual_keyword("vacua"));
            }
            HirExpressionKind::Struct(def_id, fields) => {
                let name = self
                    .imported_def_spellings
                    .get(def_id)
                    .cloned()
                    .unwrap_or_else(|| self.name_for_def(*def_id, names));
                writer.write(&name);
                if fields.is_empty() {
                    writer.write(" {");
                } else {
                    writer.write(" { ");
                }
                for (idx, (name, value)) in fields.iter().enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    writer.write(&self.symbol_to_string(*name));
                    writer.write(" = ");
                    self.write_expr(value, names, surface, writer);
                }
                if !fields.is_empty() {
                    writer.write(" ");
                }
                writer.write("}");
            }
            HirExpressionKind::Tuple(items, type_args) => {
                // Tuples are constructed with an explicit `iuncta<A, B>` prefix
                // so the bracket literal parses back as a tuple rather than an
                // array (bare `[...]` is an array literal in expression
                // position). The prefix is dropped only for internal
                // placeholders that never reach emission. Lowerings that build
                // a tuple without explicit type args (e.g. the objectPattern
                // rest binding) still carry the tuple type on the expression;
                // derive the prefix from it so re-emit typechecks.
                let prefix_tys: Vec<_> = match type_args {
                    Some(args) => args.clone(),
                    None => match expr.ty.map(|ty| self.types.get(ty)) {
                        Some(radix_types::Type::Tuple(elems)) => elems.clone(),
                        _ => Vec::new(),
                    },
                };
                if !prefix_tys.is_empty() {
                    writer.write(surface.contextual_keyword("iuncta"));
                    writer.write("<");
                    for (idx, ty) in prefix_tys.iter().enumerate() {
                        if idx > 0 {
                            writer.write(", ");
                        }
                        let site = HirTypeSite {
                            owner: HirOwnerRef::Hir(expr.id),
                            slot: HirTypeSlot::TupleTypeArgument(
                                self.slot_ordinal(idx, "tuple argument count"),
                            ),
                        };
                        let rendered = self
                            .type_to_faber_at(&site, *ty, names, surface)
                            .unwrap_or_else(|error| self.record_type_emission_error(error));
                        // Each construction type argument is its own authored
                        // root, so the element label rides that root.
                        if let Some(label) = self.authored_element_label(&site) {
                            writer.write(&self.symbol_to_string(label));
                            writer.write(": ");
                        }
                        writer.write(&rendered);
                    }
                    writer.write("> ");
                }
                writer.write("[");
                for (idx, item) in items.iter().enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    self.write_expr(item, names, surface, writer);
                }
                writer.write("]");
            }
            HirExpressionKind::Scribe(kind, args) => {
                writer.write(match kind {
                    radix_hir::HirScribeKind::Vide => surface.contextual_keyword("vide"),
                    radix_hir::HirScribeKind::Mone => surface.contextual_keyword("mone"),
                    // `scribe` is an alias of `nota`; both canonicalize to nota.
                    radix_hir::HirScribeKind::Nota | radix_hir::HirScribeKind::Scribe => {
                        surface.contextual_keyword("nota")
                    }
                });
                writer.write(" ");
                for (idx, arg) in args.iter().enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    self.write_expr(arg, names, surface, writer);
                }
            }
            HirExpressionKind::Scriptum(template, args) => {
                if surface.preserve_template_application_sugar() {
                    // Reader-locale emit keeps the author postfix glyph
                    // `"template"(arg, …)`: keyword spellings localize but the
                    // template-application shape must survive so the semantic
                    // program re-renders with only its spellings changed.
                    writer.write("\"");
                    writer.write(&self.symbol_to_string(*template));
                    writer.write("\"(");
                    for (idx, arg) in args.iter().enumerate() {
                        if idx > 0 {
                            writer.write(", ");
                        }
                        self.write_expr(arg, names, surface, writer);
                    }
                    writer.write(")");
                } else {
                    writer.write(surface.contextual_keyword("scriptum"));
                    writer.write("(\"");
                    writer.write(&self.symbol_to_string(*template));
                    writer.write("\"");
                    for arg in args {
                        writer.write(", ");
                        self.write_expr(arg, names, surface, writer);
                    }
                    writer.write(")");
                }
            }
            HirExpressionKind::Adfirma(cond, message) => {
                writer.write(surface.keyword(TokenKind::Adfirma));
                writer.write(" ");
                self.write_expr(cond, names, surface, writer);
                if let Some(message) = message {
                    writer.write(" ");
                    writer.write(surface.keyword(TokenKind::Mori));
                    writer.write(" ");
                    self.write_expr(message, names, surface, writer);
                }
            }
            HirExpressionKind::Panic(value) => {
                writer.write(surface.keyword(TokenKind::Mori));
                writer.write(" ");
                self.write_expr(value, names, surface, writer);
            }
            HirExpressionKind::Throw(value) => {
                writer.write(surface.keyword(TokenKind::Iace));
                writer.write(" ");
                self.write_expr(value, names, surface, writer);
            }
            HirExpressionKind::Clausura(params, ret, err, body, modifier) => {
                let parenthesized = params.len() != 1;
                if parenthesized {
                    writer.write("(");
                }
                for (idx, param) in params.iter().enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Def(param.def_id),
                        slot: HirTypeSlot::ClosureParameter(
                            self.slot_ordinal(idx, "closure parameter count"),
                        ),
                    };
                    let rendered = self
                        .type_to_faber_at(&site, param.ty, names, surface)
                        .unwrap_or_else(|error| self.record_type_emission_error(error));
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
                if parenthesized {
                    writer.write(")");
                }
                if *modifier != radix_syntax::ClosureModifier::None {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword(modifier.spelling()));
                }
                if let Some(ret) = ret {
                    writer.write(" → ");
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Hir(expr.id),
                        slot: HirTypeSlot::Return,
                    };
                    let rendered = self
                        .type_to_faber_at(&site, *ret, names, surface)
                        .unwrap_or_else(|error| self.record_type_emission_error(error));
                    writer.write(&rendered);
                }
                if let Some(err) = err {
                    writer.write(" ⇥ ");
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Hir(expr.id),
                        slot: HirTypeSlot::Error,
                    };
                    let rendered = self
                        .type_to_faber_at(&site, *err, names, surface)
                        .unwrap_or_else(|error| self.record_type_emission_error(error));
                    writer.write(&rendered);
                }
                writer.write(" ∴ ");
                if matches!(body.kind, HirExpressionKind::Block(_)) {
                    writer.write(surface.contextual_keyword("fac"));
                    writer.write(" ");
                }
                self.write_expr(body, names, surface, writer);
            }
            HirExpressionKind::Cede(_) => {
                unreachable!("legacy context-dependent cede HIR is rejected before Faber emission");
            }
            HirExpressionKind::Yield(inner) => {
                writer.write(surface.keyword(TokenKind::Cede));
                writer.write(" ");
                self.write_expr(inner, names, surface, writer);
            }
            HirExpressionKind::Reddet(inner) => {
                writer.write(surface.contextual_keyword("reddet"));
                writer.write(" ");
                self.write_expr(inner, names, surface, writer);
            }
            HirExpressionKind::Tacebit(inner) => {
                writer.write(surface.contextual_keyword("tacebit"));
                writer.write(" ");
                self.write_expr(inner, names, surface, writer);
            }
            HirExpressionKind::Verte {
                source,
                target,
                entries,
            } => {
                self.write_verte_expr(
                    expr.id,
                    source,
                    *target,
                    entries.as_deref(),
                    names,
                    surface,
                    writer,
                );
            }
            HirExpressionKind::Conversio {
                source,
                target,
                params,
                recovery,
            } => {
                // Emit-arm detection probe: the Faber printer re-emits the
                // `↦` spelling itself, so every conversion is a dedicated
                // arm (no unspecialized fallback exists in this backend).
                // Inert outside the measurement path.
                let arm_target_ty = match target {
                    HirConversioTarget::Type(ty) => Some(*ty),
                    HirConversioTarget::Intervallum(_) => None,
                };
                let _arm_frame = radix_codegen_shared::conversio_arm::conversio_arm_begin(
                    source.ty,
                    arm_target_ty,
                );
                // The type writer may degrade unsupported target types; this
                // layer still preserves source/recovery expression structure.
                //
                // Operand precedence matches the parser: `↦` binds at the
                // range level, tighter than shift/bitwise/comparison but
                // looser than additive. Writing the operand at additive
                // strength regroups shift-or-looser children (`n ⇒ 24 ↦ T`
                // reparses as `n ⇒ (24 ↦ T)` without the parens), and the
                // `⇥` recovery is a unary-level operand.
                self.write_expr_prec(source, 10, names, surface, writer);
                writer.write(" ↦ ");
                match target {
                    HirConversioTarget::Type(ty) => {
                        let site = HirTypeSite {
                            owner: HirOwnerRef::Hir(expr.id),
                            slot: HirTypeSlot::ConversionTarget,
                        };
                        let mut target_str = self
                            .type_to_faber_at(&site, *ty, names, surface)
                            .unwrap_or_else(|error| self.record_type_emission_error(error));
                        if !params.is_empty() {
                            // Conversio markers (`Be` / `Le` / `Bits`) are extra
                            // generic arguments on the target type
                            // (`int<u32, Be>`); a trailing `<Be>` append is not
                            // the source form and fails to re-parse.
                            let markers = params
                                .iter()
                                .map(|param| self.symbol_to_string(*param))
                                .collect::<Vec<_>>()
                                .join(", ");
                            if target_str.ends_with('>') {
                                target_str.truncate(target_str.len() - 1);
                                target_str.push_str(", ");
                            } else {
                                // Bare width marker target: the marker rides a
                                // generic argument, so re-wrap the width in
                                // the float-family head (`float<f32, Be>`).
                                target_str = format!(
                                    "{}<{}",
                                    surface.type_surface(TypeBindingKind::Primitive(
                                        Primitive::Fractus
                                    )),
                                    target_str
                                );
                                target_str.push_str(", ");
                            }
                            target_str.push_str(&markers);
                            target_str.push('>');
                        }
                        writer.write(&target_str);
                    }
                    HirConversioTarget::Intervallum(operand) => {
                        self.write_expr_prec(operand, 10, names, surface, writer);
                    }
                }
                if let Some(recovery) = recovery {
                    writer.write(" ⇥ ");
                    self.write_expr_prec(recovery, 12, names, surface, writer);
                }
            }
            HirExpressionKind::Ref(kind, inner) => {
                match kind {
                    radix_hir::HirRefKind::Shared => {
                        writer.write(surface.keyword(TokenKind::De));
                        writer.write(" ");
                    }
                    radix_hir::HirRefKind::Mutable => {
                        writer.write(surface.keyword(TokenKind::In));
                        writer.write(" ");
                    }
                }
                self.write_expr_prec(inner, 12, names, surface, writer);
            }
            HirExpressionKind::Deref(inner) => {
                writer.write("*");
                self.write_expr_prec(inner, 12, names, surface, writer);
            }
            HirExpressionKind::Ad { route, opener } => {
                writer.write(surface.contextual_keyword("ad"));
                writer.write(" '");
                writer.write(self.interner.resolve(*route));
                writer.write("'");
                if let Some(opener) = opener {
                    writer.write(" (");
                    self.write_expr(opener, names, surface, writer);
                    writer.write(")");
                }
            }
            HirExpressionKind::TypeCheck { positive, expr, ty } => {
                // Precedence matches `HirBinOp::Is` (4) — both spell `est`
                // and bind at the same relational level. Negation is the
                // infix `<expr> non est <ty>` spelling; a prefix `non`
                // re-parses as `(non expr) est ty` and fails the boolean
                // operand gate.
                self.write_expr_prec(expr, 4, names, surface, writer);
                writer.write(" ");
                if !*positive {
                    writer.write(surface.contextual_keyword("non"));
                    writer.write(" ");
                }
                writer.write(surface.contextual_keyword("est"));
                writer.write(" ");
                let site = HirTypeSite {
                    owner: HirOwnerRef::Hir(expr.id),
                    slot: HirTypeSlot::TypeCheckTarget,
                };
                let rendered = self
                    .type_to_faber_at(&site, *ty, names, surface)
                    .unwrap_or_else(|error| self.record_type_emission_error(error));
                writer.write(&rendered);
            }
            HirExpressionKind::Praefixum(inner) => {
                self.write_expr(inner, names, surface, writer);
            }
            HirExpressionKind::Summa {
                source,
                coordinates,
                filum,
                binding,
                body,
            } => {
                // `summa ex src apud [i] filum f fixum s { … }` — the SFR-2
                // source shape; every clause head is surface-projected.
                writer.write(surface.contextual_keyword("summa"));
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ex));
                writer.write(" ");
                // Reduction operand precedence: binary children (`row ⊙
                // scale`) must stay grouped or the tail clauses capture the
                // loose operand on re-parse.
                self.write_expr_prec(source, 10, names, surface, writer);
                if !coordinates.is_empty() {
                    writer.write(" ");
                    writer.write(surface.keyword(TokenKind::Apud));
                    writer.write(" [");
                    for (index, (_, name)) in coordinates.iter().enumerate() {
                        if index > 0 {
                            writer.write(", ");
                        }
                        writer.write(&self.symbol_to_string(*name));
                    }
                    writer.write("]");
                }
                if let Some((_, lane)) = filum {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("filum"));
                    writer.write(" ");
                    writer.write(&self.symbol_to_string(*lane));
                }
                writer.write(" ");
                writer.write(surface.contextual_keyword("fixum"));
                writer.write(" ");
                writer.write(&self.symbol_to_string(binding.1));
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block(body, names, surface, writer);
                    self.write_block_end_trivia(body, writer);
                });
                writer.write("}");
            }
            HirExpressionKind::Extremum {
                op,
                source,
                coordinates,
                coalesce,
            } => {
                writer.write(surface.contextual_keyword(match op {
                    radix_hir::HirExtremumOp::Max => "maxima",
                    radix_hir::HirExtremumOp::Min => "minima",
                }));
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ex));
                writer.write(" ");
                // With a `vel` identity tail the source must bind tighter
                // than the infix `vel` operator; primary-level precedence
                // parenthesizes any operator source (`(row ⊙ scale) vel …`).
                if coalesce.is_some() {
                    self.write_expr_prec(source, 12, names, surface, writer);
                } else {
                    self.write_expr_prec(source, 10, names, surface, writer);
                }
                if !coordinates.is_empty() {
                    writer.write(" ");
                    writer.write(surface.keyword(TokenKind::Apud));
                    writer.write(" [");
                    for (index, (_, name)) in coordinates.iter().enumerate() {
                        if index > 0 {
                            writer.write(", ");
                        }
                        writer.write(&self.symbol_to_string(*name));
                    }
                    writer.write("]");
                }
                if let Some(identity) = coalesce {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("vel"));
                    writer.write(" ");
                    self.write_expr(identity, names, surface, writer);
                }
            }
            HirExpressionKind::FirstMatch { .. } => {
                // AFM-2 is source-shaped only; keep re-emission fail-closed
                // until AFM-3 provides a valid Faber surface.
                writer.write(surface.contextual_keyword("nihil"));
            }
            HirExpressionKind::Transfer { sink, payloads } => {
                // `sink ⇇ p1 ⇇ p2` — the glyph is an operator token, never a
                // keyword, so it is locale-invariant. The chain binds between
                // assignment and the ternary level: the sink and every payload
                // are complete ternaries (emitted at chain precedence 2 / 3),
                // so looser operands parenthesize and re-parse to the same
                // flat chain.
                self.write_expr_prec(sink, 2, names, surface, writer);
                for payload in payloads {
                    writer.write(" ⇇ ");
                    self.write_expr_prec(payload, 3, names, surface, writer);
                }
            }
            HirExpressionKind::Error => writer.write(surface.contextual_keyword("nihil")),
        }

        if needs_parens {
            writer.write(")");
        }
    }

    pub(super) fn write_expr(
        &self,
        expr: &HirExpression,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_expr_prec(expr, 0, names, surface, writer);
    }

    #[allow(clippy::too_many_arguments)]
    fn write_verte_expr(
        &self,
        owner: radix_hir::HirId,
        source: &HirExpression,
        target: TypeId,
        entries: Option<&[HirObjectField]>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let site = HirTypeSite {
            owner: HirOwnerRef::Hir(owner),
            slot: HirTypeSlot::AscriptionTarget,
        };
        let rendered_target = self
            .type_to_faber_at(&site, target, names, surface)
            .unwrap_or_else(|error| self.record_type_emission_error(error));
        if let Some(entries) = entries {
            if self.write_forma_capture(entries, target, names, surface, writer) {
                return;
            }

            match self.types.get(target) {
                Type::Struct(def_id) => {
                    let name = self
                        .imported_type_spellings
                        .get(&target)
                        .cloned()
                        .unwrap_or_else(|| self.name_for_def(*def_id, names));
                    writer.write(&name);
                    writer.write(" {");
                    if !entries.is_empty() {
                        writer.write(" ");
                    }
                    for (idx, field) in entries.iter().enumerate() {
                        if idx > 0 {
                            writer.write(", ");
                        }
                        self.write_genus_field(field, names, surface, writer);
                    }
                    if !entries.is_empty() {
                        writer.write(" ");
                    }
                    writer.write("}");
                    return;
                }
                Type::Record(_) => {
                    writer.write(&rendered_target);
                    let ordered = sorted_record_entries(entries, self.interner);
                    if ordered.is_empty() {
                        writer.write(" {}");
                    } else {
                        writer.write(" { ");
                        for (idx, field) in ordered.into_iter().enumerate() {
                            if idx > 0 {
                                writer.write(", ");
                            }
                            self.write_genus_field(field, names, surface, writer);
                        }
                        writer.write(" }");
                    }
                    return;
                }
                Type::Map(_, _) | Type::Array(_) | Type::Set(_) => {
                    self.write_json_object_entries(entries, names, surface, writer);
                    writer.write(" ∷ ");
                    writer.write(&rendered_target);
                    return;
                }
                Type::Primitive(Primitive::Valor) => {
                    self.write_json_object_entries(entries, names, surface, writer);
                    return;
                }
                _ => {}
            }
        }

        // `∷` is a postfix cast, so its source must be emitted at unary
        // strength. This keeps compound list elements such as `n % 256`
        // grouped as `(n % 256) ∷ T` instead of letting the cast bind to the
        // trailing arithmetic operand after a reparse.
        self.write_expr_prec(source, 12, names, surface, writer);
        writer.write(" ∷ ");
        writer.write(&rendered_target);
    }

    fn write_json_object_entries(
        &self,
        entries: &[HirObjectField],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write("{");
        if !entries.is_empty() {
            writer.write(" ");
            for (idx, field) in entries.iter().enumerate() {
                if idx > 0 {
                    writer.write(", ");
                }
                self.write_json_field(field, names, surface, writer);
            }
            writer.write(" ");
        }
        writer.write("}");
    }

    /// Re-emit builtin `forma` template capture as backtick syntax when possible.
    fn write_forma_capture(
        &self,
        entries: &[HirObjectField],
        target: TypeId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) -> bool {
        let Type::Struct(def_id) = self.types.get(target) else {
            return false;
        };
        if self.name_for_def(*def_id, names) != "forma" {
            return false;
        }

        let mut template = None;
        let mut params = None;
        for field in entries {
            let name = match &field.key {
                HirObjectKey::Ident(sym) | HirObjectKey::String(sym) => self.symbol_to_string(*sym),
                _ => return false,
            };
            match name.as_str() {
                "template" => template = field.value.as_ref(),
                "params" => params = field.value.as_ref(),
                _ => return false,
            }
        }

        let Some(template) = template else {
            return false;
        };
        let HirExpressionKind::Literal(radix_hir::HirLiteral::String(sym)) = &template.kind else {
            return false;
        };

        let param_exprs = match params {
            Some(params) => match &params.kind {
                HirExpressionKind::Array(elements) => {
                    let mut exprs = Vec::with_capacity(elements.len());
                    for element in elements {
                        let HirArrayElement::Expr(expr) = element else {
                            return false;
                        };
                        exprs.push(expr);
                    }
                    Some(exprs)
                }
                _ => return false,
            },
            None => None,
        };

        writer.write("`");
        // Raw payload: `writer.write` would re-indent every content line by the
        // enclosing block level, and the re-indented text is part of the raw
        // string on the next round — the emit would never stabilize.
        writer.write_raw(self.interner.resolve(*sym));
        writer.write("`");
        if let Some(param_exprs) = param_exprs
            && !param_exprs.is_empty()
        {
            writer.write("(");
            for (idx, expr) in param_exprs.iter().enumerate() {
                if idx > 0 {
                    writer.write(", ");
                }
                self.write_expr(expr, names, surface, writer);
            }
            writer.write(")");
        }
        true
    }

    fn write_handled_expr(
        &self,
        body: &HirBlock,
        catch: &HirCape,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(expr) = &body.expr
            && let HirExpressionKind::Loop(loop_block) = &expr.kind
            && let Some((body_stmts, cond)) = self.as_fac_loop(loop_block)
        {
            writer.write(surface.contextual_keyword("fac"));
            writer.writeln(" {");
            writer.indented(|writer| {
                for stmt in body_stmts {
                    self.write_stmt(stmt, names, surface, writer);
                }
                self.write_block_end_trivia(loop_block, writer);
            });
            // Grammar order is `fac block cape_clause? (dum expression)?`:
            // the handler precedes the loop condition. Writing `cape` after
            // `dum cond` is rejected on re-parse (PARSE020 `fac_cape_after_dum`).
            writer.write("}");
            writer.newline();
            writer.write(surface.keyword(TokenKind::Cape));
            writer.write(" ");
            self.write_cape_block_named(
                &catch.body,
                Some(catch.binding_name),
                names,
                surface,
                writer,
            );
            writer.write(" ");
            writer.write(surface.contextual_keyword("dum"));
            writer.write(" ");
            self.write_expr(cond, names, surface, writer);
            return;
        }

        writer.write(surface.contextual_keyword("fac"));
        writer.writeln(" {");
        writer.indented(|writer| {
            self.write_fac_body(body, names, surface, writer);
            self.write_block_end_trivia(body, writer);
        });
        writer.write("}");
        writer.newline();
        writer.write(surface.keyword(TokenKind::Cape));
        writer.write(" ");
        self.write_cape_block_named(
            &catch.body,
            Some(catch.binding_name),
            names,
            surface,
            writer,
        );
    }

    /// Unwrap the synthetic `Block` tail produced when `fac` lowers to `Handled`.
    fn write_fac_body(
        &self,
        body: &HirBlock,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if body.statements.is_empty()
            && let Some(expr) = &body.expr
            && let HirExpressionKind::Block(inner) = &expr.kind
        {
            self.write_block(inner, names, surface, writer);
            return;
        }
        self.write_block(body, names, surface, writer);
    }

    fn write_postfix_chain_tail(
        &self,
        prefixes: PostfixChainPrefixes,
        tail: PostfixChainTail<'_>,
        receiver_projects_intrinsic: bool,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match tail {
            PostfixChainTail::Member(name) => {
                writer.write(prefixes.member);
                let spelling = if receiver_projects_intrinsic {
                    self.method_name_to_surface(name, surface)
                } else {
                    self.symbol_to_string(name)
                };
                writer.write(&spelling);
            }
            PostfixChainTail::Index(index) => {
                writer.write(prefixes.index_open);
                self.write_expr(index, names, surface, writer);
                writer.write("]");
            }
            PostfixChainTail::Call(args) => {
                writer.write(prefixes.call_open);
                self.write_call_args(args, names, surface, writer);
                writer.write(")");
            }
        }
    }

    fn write_call_type_args(
        &self,
        type_args: &[TypeId],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if type_args.is_empty() {
            return;
        }
        writer.write("<");
        for (idx, ty) in type_args.iter().enumerate() {
            if idx > 0 {
                writer.write(", ");
            }
            writer.write(&self.type_to_faber(*ty, names, surface));
        }
        writer.write(">");
    }

    /// Write comma-separated call arguments for all postfix call forms.
    ///
    /// WHY: Direct calls, method calls, optional calls, and non-null calls share
    /// exactly the same argument grammar; keeping `sparge` handling here avoids
    /// target drift between those surface forms.
    fn write_call_args(
        &self,
        args: &[HirCallArg],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for (idx, arg) in args.iter().enumerate() {
            if idx > 0 {
                writer.write(", ");
            }
            if arg.spread {
                writer.write(surface.keyword(TokenKind::Sparge));
                writer.write(" ");
            }
            self.write_expr(&arg.expr, names, surface, writer);
        }
    }

    /// Re-emit a `finge` variant construction.
    ///
    /// `finge` construction lowers to a plain call shape
    /// (`Call(Path(variant_def), [], fields)`), so the callee being a variant
    /// def is the recoverable signal. A bare `<variant>()` re-emit is not
    /// valid source: the type-call constructor spelling is retired.
    fn write_finge_construction(
        &self,
        variant_def: DefId,
        fields: &[HirCallArg],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // An imported fieldless variant is authored as a qualified value path,
        // not a `finge` construction. Keep its exact source path when the HIR
        // has collapsed the path to the variant `DefId`.
        if fields.is_empty()
            && let Some(spelling) = self.authored_variant_spellings.get(&variant_def)
        {
            writer.write(spelling);
            return;
        }
        writer.write(surface.keyword(TokenKind::Finge));
        writer.write(" ");
        // Bare local names that two local unions share (or that collide with
        // an imported variant) re-parse ambiguously; use the union-qualified
        // two-part spelling exactly where the source needed it.
        writer.write(
            &self
                .authored_variant_spellings
                .get(&variant_def)
                .cloned()
                .or_else(|| self.qualified_local_variants.get(&variant_def).cloned())
                .unwrap_or_else(|| self.name_for_def_imported(variant_def, names)),
        );
        if !fields.is_empty() {
            writer.write(" { ");
            for (idx, field) in fields.iter().enumerate() {
                if idx > 0 {
                    writer.write(", ");
                }
                if let Some(name) = field.name {
                    writer.write(&self.symbol_to_string(name));
                    writer.write(" = ");
                }
                self.write_expr(&field.expr, names, surface, writer);
            }
            writer.write(" }");
        }
    }

    /// Localize a canonical intrinsic method name through the target surface.
    ///
    /// HIR method symbols are the canonical registry spellings (the
    /// typechecker rewrites localized surface spellings to canonical before
    /// lowering). Under a non-`la` reader pack those canonical names are not
    /// re-parseable source (the pack rejects the canonical Latin spelling as a
    /// wrong-locale miss), so emission must project them back through the
    /// pack's intrinsic surface rows. Names the pack has no row for keep the
    /// canonical spelling (identity).
    fn method_name_to_surface(&self, name: Symbol, surface: &impl KeywordSurface) -> String {
        let canonical = self.symbol_to_string(name);
        surface.intrinsic_surface(&canonical).unwrap_or(canonical)
    }

    /// Spelling for a method call on the given receiver.
    ///
    /// Intrinsic-capable receivers (collections, tensors, primitives) project
    /// the canonical method name through the pack surface: HIR method symbols
    /// are canonical registry spellings, and under a non-`la` reader pack
    /// those spellings are not re-parseable method names. Nominal
    /// (class/interface/enum) receivers and module-import receivers keep the
    /// canonical symbol instead: the first declares its own method from the
    /// canonical HIR symbol (localizing `longitudo` → `length` produces a name
    /// the class does not declare), and the second names a user callable in
    /// another module whose declaration is also emitted from the canonical
    /// symbol (rewriting `parametrum.muta` → `parametrum.replace` breaks the
    /// qualified call with a namespace-missing-export).
    fn method_spelling(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> String {
        if self.receiver_projects_intrinsic(receiver) {
            self.method_name_to_surface(name, surface)
        } else if let Some(view) = self.sermo_view_method(receiver, name, names) {
            // `s.meus<T>()` / `s.tuus<T>()` share the view type's pack row
            // (DECISIONS D6.16): en re-emits `s.send<T>()` / `s.recv<T>()`.
            surface
                .type_surface(TypeBindingKind::Frame(view))
                .to_owned()
        } else {
            self.symbol_to_string(name)
        }
    }

    /// The directional view a `meus`/`tuus` method call on a builtin `sermo`
    /// receiver constructs, if this is one.
    fn sermo_view_method(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        names: &FxHashMap<DefId, Symbol>,
    ) -> Option<FrameFamily> {
        let view = match FrameFamily::from_latin(self.interner.resolve(name))? {
            view @ (FrameFamily::Meus | FrameFamily::Tuus) => view,
            FrameFamily::Sermo | FrameFamily::Scrinium => return None,
        };
        let mut ty = receiver.ty?;
        loop {
            match self.types.get(ty) {
                Type::Ref(_, inner) | Type::Option(inner) | Type::Alias(_, inner) => ty = *inner,
                Type::Struct(def_id)
                    if self.builtin_frame_family(*def_id, names) == Some(FrameFamily::Sermo) =>
                {
                    return Some(view);
                }
                _ => return None,
            }
        }
    }

    /// Whether a method-call receiver expression denotes an intrinsic-capable
    /// receiver (a collection, a tensor, or a primitive value) whose method
    /// name must project through the target surface.
    ///
    /// Mirrors the semantic intrinsic-receiver classification
    /// (`receivers_for_type`): nominal types, module-import bindings (whose
    /// qualified member calls name user callables in another module — their
    /// receiver type is `ignotum`), and unsupported receivers keep the
    /// canonical symbol. Reference/optional/application wrappers unwrap the
    /// way semantic method lookup does.
    fn receiver_projects_intrinsic(&self, receiver: &HirExpression) -> bool {
        let Some(mut ty) = receiver.ty else {
            return false;
        };
        loop {
            match self.types.get(ty) {
                Type::Ref(_, inner) | Type::Option(inner) | Type::Alias(_, inner) => ty = *inner,
                Type::Applied(base, _) => ty = *base,
                ty => {
                    return matches!(
                        ty,
                        Type::Array(_)
                            | Type::BoundedArray { .. }
                            | Type::Queue(_)
                            | Type::BoundedQueue { .. }
                            | Type::Stack(_)
                            | Type::BoundedStack { .. }
                            | Type::BoundedTextus { .. }
                            | Type::BoundedAscii { .. }
                            | Type::BoundedOcteti { .. }
                            | Type::Map(_, _)
                            | Type::Set(_)
                            | Type::Tensor(_, _)
                            | Type::Matrix(_, _)
                            | Type::Vector(_, _)
                            | Type::Sparsa(_, _)
                            | Type::Intervallum(_)
                            | Type::Atomic(_)
                            | Type::SizedNumeric(_, _)
                            | Type::ModularWord(_)
                            | Type::Primitive(
                                Primitive::Octeti
                                    | Primitive::Textus
                                    | Primitive::Regex
                                    | Primitive::Numerus
                                    | Primitive::Fractus
                            )
                    );
                }
            }
        }
    }
}

/// Canonical ratio construction order: sorted by field label spelling.
fn sorted_record_entries<'a>(
    entries: &'a [HirObjectField],
    interner: &Interner,
) -> Vec<&'a HirObjectField> {
    let mut ordered: Vec<&'a HirObjectField> = entries.iter().collect();
    ordered.sort_by(|left, right| match (&left.key, &right.key) {
        (
            HirObjectKey::Ident(left_name) | HirObjectKey::String(left_name),
            HirObjectKey::Ident(right_name) | HirObjectKey::String(right_name),
        ) => interner
            .resolve(*left_name)
            .cmp(interner.resolve(*right_name)),
        _ => std::cmp::Ordering::Equal,
    });
    ordered
}
