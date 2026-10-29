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
use handroll::FxHashMap;
use radix_codegen_shared::{CodeWriter, CodegenError};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{
    DefId, HirArrayElement, HirBlock, HirCallArg, HirCape, HirConversioTarget, HirExpression,
    HirExpressionKind, HirLiteral, HirNonNullKind, HirObjectField, HirObjectKey,
    HirOptionalChainKind, HirOwnerRef, HirStatementKind, HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_types::{Primitive, Type, TypeId};

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

/// Binder keyword of a loop header: `varia` when the authored binder is
/// mutable, `sit` otherwise (binders take `let`/`mut`; `ref` is refused),
/// spelled in the source's own locale.
fn loop_binder_keyword(mutable: bool, surface: &impl KeywordSurface) -> &str {
    surface.contextual_keyword(if mutable { "varia" } else { "sit" })
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

impl<'a> PostfixChainTail<'a> {
    /// The tail after `!`; the bare assertion has none (a method call writes
    /// its own `!.`).
    fn after_assertion(kind: &'a HirNonNullKind) -> Option<Self> {
        match kind {
            HirNonNullKind::Value => None,
            HirNonNullKind::Member(name) => Some(Self::Member(*name)),
            HirNonNullKind::Index(index) => Some(Self::Index(index)),
            HirNonNullKind::Call(args) => Some(Self::Call(args)),
        }
    }
}

impl super::FaberCodegen<'_> {
    /// Whether `object` is a `tabula` keyed by an `iuncta` (composite key).
    fn is_tuple_keyed_map(&self, object: &HirExpression) -> bool {
        let Some(mut ty) = object.ty else {
            return false;
        };
        loop {
            match self.types.get(ty) {
                radix_types::Type::Alias(_, inner) | radix_types::Type::Ref(_, inner) => {
                    ty = *inner;
                }
                radix_types::Type::Map(key, _) => {
                    let mut key = *key;
                    while let radix_types::Type::Alias(_, inner) = self.types.get(key) {
                        key = *inner;
                    }
                    return matches!(self.types.get(key), radix_types::Type::Tuple(_));
                }
                _ => return false,
            }
        }
    }

    /// Write the ` via HINT` clause of a `↤` conversion-directed assignment.
    ///
    /// The clause sits after the source and before the `⊥` default, the same
    /// single-hint slot a `↦ T via HINT` conversion prints.
    pub(super) fn write_conversio_assign_via(
        &self,
        params: &[Symbol],
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(hint) = params.first() {
            writer.write(" ");
            writer.write(surface.keyword(TokenKind::Via));
            writer.write(" ");
            writer.write(&self.symbol_to_string(*hint));
        }
    }

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
        // An implicit store node (d11-6 ruling 17) is the checker's, not the
        // author's: the printer keeps the authored spelling of its source.
        if let HirExpressionKind::Conversio {
            implicit: Some(_),
            source,
            ..
        } = &expr.kind
        {
            self.write_expr_prec(source, parent_prec, names, surface, writer);
            return;
        }
        // The implicit bounded → `inf` widening is the checker's too (inf-wn).
        if let HirExpressionKind::Widen { operand, .. } = &expr.kind {
            self.write_expr_prec(operand, parent_prec, names, surface, writer);
            return;
        }
        // An inferred-constructor hole `_ { … }` the checker rewrote prints as
        // the hole: the tensor zero is `_ {}`, a map or `json` literal is the
        // literal behind a leading `_`, and a genus or record constructor
        // writes `_` for its type name (see `write_verte_expr`).
        if self.inferred_constructor_ids.contains(&expr.id) {
            match &expr.kind {
                HirExpressionKind::MethodCall(..) | HirExpressionKind::Vacua => {
                    writer.write("_ {}");
                    return;
                }
                HirExpressionKind::Literal(HirLiteral::JsonValor(_)) => writer.write("_ "),
                // `_ [ … ]` the checker rewrote to an array or tuple: the
                // hole's type form still sits under the node id. A bare
                // `[ … ]` into a tuple cell has no such form and prints bare.
                HirExpressionKind::Array(_) | HirExpressionKind::Tuple(..)
                    if self.authored_type_forms.contains_key(&HirTypeSite {
                        owner: HirOwnerRef::Hir(expr.id),
                        slot: HirTypeSlot::AscriptionTarget,
                    }) =>
                {
                    writer.write("_ ");
                }
                // A bracket literal the checker rewrote into the
                // `array ↦ set` conversion: print the authored bracket form —
                // `_ [ … ]` when the hole's type form sits under this node id,
                // the bare `[ … ]` otherwise — never the conversion spelling.
                HirExpressionKind::Conversio { source, .. } => {
                    if self.authored_type_forms.contains_key(&HirTypeSite {
                        owner: HirOwnerRef::Hir(expr.id),
                        slot: HirTypeSlot::AscriptionTarget,
                    }) {
                        writer.write("_ ");
                    }
                    self.write_expr_prec(source, parent_prec, names, surface, writer);
                    return;
                }
                _ => {}
            }
        }
        // An `empty` the checker rewrote into the typed zero prints as the
        // authored literal, not as the `Type {}` it became.
        if self.empty_dense_ids.contains(&expr.id) {
            writer.write(surface.contextual_keyword("vacua"));
            return;
        }
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
                } else if let Some(spelling) = self
                    .authored_variant_spellings
                    .get(def_id)
                    .or_else(|| self.qualified_local_variants.get(def_id))
                {
                    // Lowering keeps only the resolved variant `DefId`; use the
                    // captured source path when present, or the necessary union
                    // qualifier when a local variant name is ambiguous.
                    writer.write(spelling);
                } else {
                    writer.write(&self.name_for_def_imported(*def_id, names));
                }
            }
            HirExpressionKind::Literal(_) if let Some(path) = self.insere_exprs.get(&expr.id) => {
                // D8.10: a folded build-time embed re-emits as authored.
                self.write_insere(*path, surface, writer);
            }
            HirExpressionKind::Insere(path) => self.write_insere(*path, surface, writer),
            // Handled before the match (transparent); kept total.
            HirExpressionKind::Widen { operand, .. } => {
                self.write_expr_prec(operand, parent_prec, names, surface, writer);
            }
            HirExpressionKind::Literal(lit) => self.write_literal(lit, surface, writer),
            // `lege`/`lineam` are two distinct reads: bare whole-input `lege`
            // versus the line-oriented `lineam`. The lowered flag carries the
            // authored mode, so emission must restore the matching word or the
            // re-emitted source silently changes meaning.
            HirExpressionKind::ReadLine(is_line) => {
                let key = if *is_line { "lineam" } else { "lege" };
                writer.write(surface.contextual_keyword(key));
            }
            HirExpressionKind::Assign(..)
            | HirExpressionKind::Binary(..)
            | HirExpressionKind::Unary(..)
                if self.numeric_twin_call(expr).is_some() =>
            {
                // Authored numeric twin (`a.add(2)`, `x.added(y)`,
                // `x.negated()`): typecheck desugared the call to the
                // assignment or operator and kept the node id; restore the
                // method spelling in the reader locale.
                if let Some((receiver, canonical, argument)) = self.numeric_twin_call(expr) {
                    self.write_twin_call(receiver, canonical, argument, names, surface, writer);
                }
            }
            HirExpressionKind::Binary(..)
            | HirExpressionKind::Reduction { .. }
            | HirExpressionKind::Scan { .. }
                if self.axis_sugar_call(expr).is_some() =>
            {
                // Authored axis reduction (`t.summa(axis n)` /
                // `t.media(axis n)`): typecheck kept the authored call's id on
                // the expansion node; print the call, not the reduction.
                if let Some((source, sugar)) = self.axis_sugar_call(expr) {
                    self.write_axis_sugar_call(source, &sugar, names, surface, writer);
                }
            }
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
                if let HirExpressionKind::Path(def_id) = &callee.kind
                    && self.variant_def_ids.contains(def_id)
                {
                    self.write_finge_construction(*def_id, args, names, surface, writer);
                } else {
                    self.write_expr_prec(callee, 13, names, surface, writer);
                    self.write_call_type_args(type_args, names, surface, writer);
                    writer.write("(");
                    self.write_call_args(args, names, surface, writer);
                    writer.write(")");
                }
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args, _)
                if type_args.is_empty()
                    && args.is_empty()
                    && self.glyph_ids.contains(&expr.id)
                    && self.rounding_delimiters(*name, surface).is_some() =>
            {
                // Authored `⌊x⌋` / `⌈x⌉` / `⌊x⌉`: lowering recorded the node
                // id, typecheck rewrote the marker to the rounding twin and
                // kept the id; the delimiters are the authored surface.
                if let Some((open, close)) = self.rounding_delimiters(*name, surface) {
                    writer.write(open);
                    self.write_expr(receiver, names, surface, writer);
                    writer.write(close);
                }
            }
            HirExpressionKind::MethodCall(receiver, _, type_args, args, _)
                if type_args.is_empty()
                    && args.is_empty()
                    && self.radical_quarta_ids.contains(&expr.id)
                    && Self::radical_quarta_operand(receiver).is_some() =>
            {
                // Authored `∜ x`: lowering nested two `radix` calls and
                // recorded only the outer id. The operand sits under the
                // inner call, and the glyph is the authored surface. A
                // radical binds like unary `-`.
                if let Some(operand) = Self::radical_quarta_operand(receiver) {
                    writer.write("∜ ");
                    self.write_expr_prec(operand, 12, names, surface, writer);
                }
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args, _)
                if type_args.is_empty()
                    && args.is_empty()
                    && self.glyph_ids.contains(&expr.id)
                    && !self.radical_quarta_ids.contains(&expr.id)
                    && self.radical_prefix(*name, surface).is_some() =>
            {
                // Authored `√ x` / `∛ x`: lowering recorded the node id and
                // typecheck rewrote the marker to the radical twin; the prefix
                // glyph is the authored surface. The `radical_quarta_ids`
                // exclusion keeps an authored `∜` (nested `.sqrt().sqrt()`
                // under the same `radix` name) out of this arm so it never
                // prints as `√ √`. A radical binds like unary `-`, so a looser
                // operand keeps its parentheses.
                if let Some(prefix) = self.radical_prefix(*name, surface) {
                    writer.write(prefix);
                    writer.write(" ");
                    self.write_expr_prec(receiver, 12, names, surface, writer);
                }
            }
            HirExpressionKind::MethodCall(receiver, _, type_args, args, _)
                if type_args.is_empty() && args.is_empty() && self.glyph_ids.contains(&expr.id) =>
            {
                // Authored `xᵀ`: typecheck rewrote the glyph marker to the
                // transpose intrinsic and kept the node id; a compound
                // receiver parenthesizes through the postfix precedence.
                self.write_expr_prec(receiver, 13, names, surface, writer);
                writer.write("ᵀ");
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args, _)
                if self
                    .typed_zero_tensor(receiver, *name, type_args, args)
                    .is_some() =>
            {
                // The typed zero `Type {}`: lowering synthesizes a `crea`
                // call on a pre-typed `vacua` receiver; restore the authored
                // empty-braces spelling.
                if let Some(ty) = self.typed_zero_tensor(receiver, *name, type_args, args) {
                    self.write_typed_zero(expr.id, ty, names, surface, writer);
                }
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args, _) => {
                // `base!.method(..)`: the receiver is the bare assertion, so
                // the authored `!.` boundary is written in place of `.`.
                if let HirExpressionKind::NonNull(base, HirNonNullKind::Value) = &receiver.kind {
                    self.write_expr_prec(base, 13, names, surface, writer);
                    writer.write("!.");
                } else {
                    self.write_expr_prec(receiver, 13, names, surface, writer);
                    writer.write(".");
                }
                let method_name = self.method_spelling(receiver, *name, names, surface);
                writer.write(&method_name);
                self.write_call_type_args(type_args, names, surface, writer);
                writer.write("(");
                self.write_call_args(args, names, surface, writer);
                writer.write(")");
            }
            HirExpressionKind::Field(object, name) => {
                match &object.kind {
                    // `ego` and the type name both lower to `Path(genus)`; a
                    // read of a static field through it is the authored
                    // type-qualified read, so the type name is the receiver.
                    HirExpressionKind::Path(def_id)
                        if self.static_fields.contains(&(*def_id, *name)) =>
                    {
                        writer.write(&self.name_for_def_imported(*def_id, names));
                    }
                    _ => self.write_expr_prec(object, 13, names, surface, writer),
                }
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
                } else if let HirExpressionKind::Tuple(parts, None) = &index.kind
                    && self.is_tuple_keyed_map(object)
                {
                    // Composite-key lookup `m[[k1, k2]]` (D3.3): the checker
                    // rewrote the authored bracket list into the key tuple;
                    // restore the bracket list, not an `iuncta<…>` prefix.
                    writer.write("[[");
                    for (idx, part) in parts.iter().enumerate() {
                        if idx > 0 {
                            writer.write(", ");
                        }
                        self.write_expr(part, names, surface, writer);
                    }
                    writer.write("]]");
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
                if let Some(tail) = PostfixChainTail::after_assertion(chain) {
                    self.write_postfix_chain_tail(
                        NON_NULL_CHAIN,
                        tail,
                        self.receiver_projects_intrinsic(object),
                        names,
                        surface,
                        writer,
                    );
                }
            }
            HirExpressionKind::Block(..) if self.hoisted_numeric_twin(expr).is_some() => {
                // Authored effect-hoisted in-place twin (`xs[f()].add(2)`):
                // typecheck pinned the place's effects to temporaries inside
                // a block; inline them back and restore the method spelling.
                if let Some((receiver, canonical, argument)) = self.hoisted_numeric_twin(expr) {
                    self.write_twin_call(
                        &receiver,
                        canonical,
                        argument.as_ref(),
                        names,
                        surface,
                        writer,
                    );
                }
            }
            HirExpressionKind::Block(block) => {
                // A plain `fac { }` body is a breakable scope: `rumpe` exits
                // the block, not the enclosing loop. Dropping the keyword
                // would change what `rumpe`/`perge` target (D8.9).
                if let Some(kind) = block.breakable
                    && kind.is_scope()
                {
                    writer.write(surface.contextual_keyword("fac"));
                    if let Some(modifier) = kind.fac_modifier_key() {
                        writer.write(" ");
                        writer.write(surface.contextual_keyword(modifier));
                    }
                    writer.write(" ");
                }
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
                if self.write_check_ballot(
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
            // D17.7: `call ⊥ default`. The trap body holds only the call as
            // its tail; the default is a unary-level operand like the
            // conversion default.
            HirExpressionKind::CallDefault { body, default, .. } => {
                if let Some(call) = body.expr.as_deref() {
                    self.write_expr_prec(call, 13, names, surface, writer);
                }
                writer.write(" ⊥ ");
                self.write_expr_prec(default, 12, names, surface, writer);
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
                        writer.write(" ");
                        writer.write(surface.contextual_keyword("et"));
                        writer.write(" ");
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
                    self.write_fac_omnia_marker(block, surface, writer);
                    writer.writeln(" {");
                    writer.indented(|writer| {
                        self.write_stmts(body_stmts, names, surface, writer);
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
            HirExpressionKind::Itera {
                mode,
                coordinates,
                filum,
                binding_name,
                iter,
                body: block,
                mutable,
                ..
            } => {
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
                // An ordo enumeration iterable lowers to a bare type `Path`,
                // but in value position that arm prints `ego` (the def is a
                // self def). Print the type name here instead: the iteratee is
                // a type, not a value, and `ego` is illegal outside a method.
                if let radix_hir::HirExpressionKind::Path(def_id) = &iter.kind
                    && self.ordo_enums.contains_key(def_id)
                {
                    writer.write(&self.name_for_def_imported(*def_id, names));
                } else {
                    self.write_expr(iter, names, surface, writer);
                }
                self.write_apud_coordinates(coordinates, surface, writer);
                self.write_filum_clause(*filum, surface, writer);
                writer.write(" ");
                // A destructuring binder lowers to a synthetic element local
                // plus a leading run of pattern locals; re-sugar that run.
                let pattern = self.destructure_run(&block.statements);
                let start = pattern.map_or(0, |(binding, _)| binding.stmt_ids.len());
                if let Some((binding, _)) = pattern {
                    writer.write(super::stmt::destructure_keyword(binding, surface));
                    writer.write(" ");
                    self.write_destructure_pattern(&binding.pattern, surface, writer);
                } else {
                    writer.write(loop_binder_keyword(*mutable, surface));
                    writer.write(" ");
                    writer.write(&self.symbol_to_string(*binding_name));
                }
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block_from(block, start, names, surface, writer);
                    self.write_block_end_trivia(block, writer);
                });
                writer.write("}");
            }
            HirExpressionKind::IteraLockstep {
                sources,
                coordinates,
                bindings,
                body,
                mutable,
                ..
            } => {
                // Lockstep series: `itera ex s1, s2 [apud [i]] sit b1, b2`.
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
                writer.write(loop_binder_keyword(*mutable, surface));
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
                mutable,
                ..
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
                writer.write(loop_binder_keyword(*mutable, surface));
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
                // Need (operator ruling 2026-10-17, option A): range
                // glyphs are always space-padded like every other infix glyph;
                // this replaces the 91dd7cb9 literal exception.
                let glyph = match kind {
                    radix_hir::HirRangeKind::Exclusive => "‥",
                    radix_hir::HirRangeKind::Inclusive => "…",
                };
                writer.write(" ");
                writer.write(glyph);
                writer.write(" ");
                self.write_expr(end, names, surface, writer);
                if let Some(step) = step {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("per"));
                    writer.write(" ");
                    self.write_expr(step, names, surface, writer);
                }
            }
            HirExpressionKind::Marked {
                marker,
                expr: inner,
            } => {
                writer.write(surface.keyword(match marker {
                    radix_hir::HirArgMarker::Ref => TokenKind::De,
                    radix_hir::HirArgMarker::Mut => TokenKind::In,
                    radix_hir::HirArgMarker::Own => TokenKind::Own,
                    radix_hir::HirArgMarker::Copy => TokenKind::Copy,
                    radix_hir::HirArgMarker::Axis => TokenKind::Axis,
                }));
                writer.write(" ");
                self.write_expr_prec(inner, 12, names, surface, writer);
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
                params,
            } => {
                // Conversion-directed assignment: re-emit the exact `↤`
                // spelling (never `←` or `↦`). The `⊥` default is
                // `inlineDefault := '⊥' unary`, so the default operand is
                // written at unary precedence to keep parens that let it
                // reparse attached to this `↤` (the nearest in a chain).
                self.write_expr_prec(target, 1, names, surface, writer);
                writer.write(" ↤ ");
                self.write_expr_prec(source, 1, names, surface, writer);
                self.write_conversio_assign_via(params, surface, writer);
                if let Some(recovery) = recovery {
                    writer.write(" ⊥ ");
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
                        HirArrayElement::Infer(_) => writer.write("_"),
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
                    // A checker-built tuple (`_ [ … ]` or a bare `[ … ]` at a
                    // tuple cell) re-emits as authored, with no type prefix.
                    None if self.inferred_constructor_ids.contains(&expr.id) => Vec::new(),
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
                    // A statement-head output keyword followed by `(` reads as
                    // a user call, so a first argument whose receiver needs
                    // parentheses cannot print as `(recv).method()`.
                    if idx == 0
                        && let Some((receiver, open, close)) =
                            self.parenthesized_rounding_receiver(arg, surface)
                    {
                        writer.write(open);
                        self.write_expr(receiver, names, surface, writer);
                        writer.write(close);
                        continue;
                    }
                    self.write_expr(arg, names, surface, writer);
                }
            }
            HirExpressionKind::Scriptum(template, args) => {
                // Named holes: the authored text (with its `§{label}` holes)
                // and the authored actual labels replace the erased form.
                let authored = self.template_holes.get(&expr.id);
                let template = authored.map_or(*template, |holes| holes.template);
                if surface.preserve_template_application_sugar() {
                    // Reader-locale emit keeps the author postfix glyph
                    // `"template"(arg, …)`: keyword spellings localize but the
                    // template-application shape must survive so the semantic
                    // program re-renders with only its spellings changed.
                    // The template keeps `«…»` when its text holds `"` or a
                    // newline, exactly like a plain `textus` literal.
                    self.write_textus_literal(template, writer);
                    let refs: Vec<&HirExpression> = args.iter().collect();
                    self.write_template_actuals(&refs, authored, names, surface, writer);
                } else {
                    writer.write(surface.contextual_keyword("scriptum"));
                    writer.write("(");
                    self.write_textus_literal(template, writer);
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
            HirExpressionKind::Clausura(params, ret, err, body) => {
                // Compact block bodies require `fac`, which adds a break
                // target. Preserve a plain lexical body via the live keyword
                // grammar instead of changing its HIR meaning (D8.9).
                let lexical_body = matches!(&body.kind, HirExpressionKind::Block(block) if block.breakable.is_none());
                if lexical_body {
                    writer.write(surface.keyword(TokenKind::Clausura));
                    if !params.is_empty() {
                        writer.write(" ");
                    }
                }
                let rendered_params: Vec<_> = params
                    .iter()
                    .enumerate()
                    .map(|(idx, param)| {
                        let site = HirTypeSite {
                            owner: HirOwnerRef::Def(param.def_id),
                            slot: HirTypeSlot::ClosureParameter(
                                self.slot_ordinal(idx, "closure parameter count"),
                            ),
                        };
                        self.type_to_faber_at(&site, param.ty, names, surface)
                            .unwrap_or_else(|error| self.record_type_emission_error(error))
                    })
                    .collect();
                // WHY: the parser's compact-closure lookahead cannot distinguish an
                // unparenthesized union annotation from the enclosing argument
                // expression. A parenthesized signature makes the closure claim
                // explicit before `∪` is reached.
                let union_typed = rendered_params.iter().any(|ty| ty.contains('∪'));
                let parenthesized = !lexical_body && (params.len() != 1 || union_typed);
                if parenthesized {
                    writer.write("(");
                }
                for (idx, (param, rendered)) in params.iter().zip(&rendered_params).enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    writer.write(rendered);
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
                writer.write(if lexical_body { " " } else { " ∴ " });
                self.write_expr(body, names, surface, writer);
            }
            HirExpressionKind::Cede(_) => {
                // Rejected up front by `reject_unsupported_legacy_hir`; if a
                // caller bypasses that gate, fail closed through the recorded
                // emission error instead of emitting bad source.
                let placeholder = self.record_type_emission_error(CodegenError {
                    message: "cannot emit legacy context-dependent cede HIR".to_owned(),
                    args: vec![DiagnosticArg::new(
                        "issue",
                        "legacy_cede_reached_faber_codegen",
                    )],
                });
                writer.write(&placeholder);
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
                entries: None,
            } if self.is_typed_zero_register(expr.id, source, *target) => {
                // The typed zero of a vector or matrix: lowering builds the
                // zero array and converts it to the claimed type; no authored
                // ascription site exists for the synthesized node.
                self.write_typed_zero(expr.id, *target, names, surface, writer);
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
                route: _,
                source,
                target,
                params,
                recovery,
                implicit: _,
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
                // `⊥` default is a unary-level operand.
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
                        if let Some(hint) = params.first() {
                            // D11.9: a convert hint is a `via` clause after
                            // the target, never a type argument. The parser
                            // admits at most one hint per conversion, so one
                            // clause carries the whole hint slot.
                            target_str.push(' ');
                            target_str.push_str(surface.keyword(TokenKind::Via));
                            target_str.push(' ');
                            target_str.push_str(&self.symbol_to_string(*hint));
                        }
                        writer.write(&target_str);
                    }
                    HirConversioTarget::Intervallum(operand) => {
                        self.write_expr_prec(operand, 10, names, surface, writer);
                    }
                }
                if let Some(recovery) = recovery {
                    writer.write(" ⊥ ");
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
            HirExpressionKind::TypeCheck {
                positive,
                expr: operand,
                ty,
            } => {
                // Precedence matches `HirBinOp::Is` (4) — both spell `est`
                // and bind at the same relational level. Negation is the
                // infix `<expr> non est <ty>` spelling; a prefix `non`
                // re-parses as `(non expr) est ty` and fails the boolean
                // operand gate.
                self.write_expr_prec(operand, 4, names, surface, writer);
                writer.write(" ");
                if !*positive {
                    writer.write(surface.contextual_keyword("non"));
                    writer.write(" ");
                }
                writer.write(surface.contextual_keyword("est"));
                writer.write(" ");
                if matches!(self.types.get(*ty), Type::Primitive(Primitive::Nihil)) {
                    // A type test against the null type is the null-presence
                    // probe. D23.2 names the null *type* `none` (en) / `nihil`
                    // (la), so emit the type surface here — not the value
                    // literal.
                    writer
                        .write(surface.type_surface(TypeBindingKind::Primitive(Primitive::Nihil)));
                } else {
                    // The target type occurrence belongs to this TypeCheck
                    // node, rather than the operand's expression site.
                    let site = HirTypeSite {
                        owner: HirOwnerRef::Hir(expr.id),
                        slot: HirTypeSlot::TypeCheckTarget,
                    };
                    let rendered = self
                        .type_to_faber_at(&site, *ty, names, surface)
                        .unwrap_or_else(|error| self.record_type_emission_error(error));
                    writer.write(&rendered);
                }
            }
            // `praefixum { … }` (D8.4): the keyword, then the body block.
            HirExpressionKind::Praefixum(inner) => {
                writer.write(surface.keyword(TokenKind::Praefixum));
                writer.write(" ");
                self.write_expr(inner, names, surface, writer);
            }
            HirExpressionKind::Reduction {
                op,
                source,
                coordinates,
                group,
                filum,
                binding,
                body,
                identity,
                mutable,
            } => {
                writer.write(surface.contextual_keyword("reducta"));
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Via));
                writer.write(" ");
                writer.write(surface.contextual_keyword(op.keyword_key()));
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ex));
                writer.write(" ");
                self.write_expr_prec(source, 12, names, surface, writer);
                self.write_apud_coordinates(coordinates, surface, writer);
                if let Some(group) = group {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("pro"));
                    writer.write(" ");
                    self.write_expr_prec(group, 12, names, surface, writer);
                }
                self.write_filum_clause(*filum, surface, writer);
                self.write_binder_body_clause(
                    *binding,
                    body.as_ref(),
                    *mutable,
                    names,
                    surface,
                    writer,
                );
                if let Some(identity) = identity {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("vel"));
                    writer.write(" ");
                    self.write_expr(identity, names, surface, writer);
                }
            }
            HirExpressionKind::Scan {
                op,
                total,
                source,
                coordinates,
                group,
                order,
                filum,
                binding,
                body,
                ..
            } => {
                writer.write(surface.contextual_keyword("scan"));
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Via));
                writer.write(" ");
                writer.write(surface.contextual_keyword(op.keyword_key()));
                if !matches!(total, radix_hir::HirScanTotal::Inclusive) {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("scan_total"));
                    writer.write(" ");
                    writer.write(surface.contextual_keyword(total.keyword_key()));
                }
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ex));
                writer.write(" ");
                self.write_expr_prec(source, 12, names, surface, writer);
                self.write_apud_coordinates(coordinates, surface, writer);
                if let Some(group) = group {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("pro"));
                    writer.write(" ");
                    self.write_expr_prec(group, 12, names, surface, writer);
                }
                writer.write(" ");
                writer.write(surface.contextual_keyword("scan_order"));
                writer.write(" ");
                writer.write(surface.contextual_keyword(order.keyword_key()));
                self.write_filum_clause(Some(*filum), surface, writer);
                self.write_binder_body_clause(
                    Some(*binding),
                    Some(body),
                    false,
                    names,
                    surface,
                    writer,
                );
            }
            HirExpressionKind::Sort {
                direction,
                source_mode,
                source,
                binding,
                body,
                mutable,
            } => {
                writer.write(surface.contextual_keyword("ordinata"));
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Via));
                writer.write(" ");
                writer.write(surface.contextual_keyword(direction.keyword_key()));
                writer.write(" ");
                writer.write(surface.keyword(match source_mode {
                    radix_hir::HirSortSource::Values => TokenKind::Ex,
                    radix_hir::HirSortSource::Keys => TokenKind::De,
                }));
                writer.write(" ");
                self.write_expr_prec(source, 12, names, surface, writer);
                self.write_binder_body_clause(
                    *binding,
                    body.as_ref(),
                    *mutable,
                    names,
                    surface,
                    writer,
                );
            }
            HirExpressionKind::CopyInto { target, value } => {
                // `target ⇇ value` — the glyph is an operator token, never a
                // keyword, so it is locale-invariant. It binds between
                // assignment and the ternary level: the target and the value
                // are complete ternaries (emitted at precedence 2 / 3), so
                // looser operands parenthesize and re-parse to the same form.
                self.write_expr_prec(target, 2, names, surface, writer);
                writer.write(" ⇇ ");
                self.write_expr_prec(value, 3, names, surface, writer);
            }
            HirExpressionKind::Format { value, spec } => {
                // `value ¶ "spec"` (D2.1): the glyph is the same in every
                // locale; the value binds tighter than the format tier.
                self.write_expr_prec(value, 5, names, surface, writer);
                writer.write(" ¶ ");
                self.write_textus_literal(*spec, writer);
            }
            HirExpressionKind::SinkInto { sink, item } => {
                // `sink ⇜ item` — like `⇇`, the glyph is an operator token and
                // never a keyword. Both operands are complete ternaries.
                self.write_expr_prec(sink, 2, names, surface, writer);
                writer.write(" ⇜ ");
                self.write_expr_prec(item, 3, names, surface, writer);
            }
            HirExpressionKind::Error => writer.write(surface.contextual_keyword("nulla")),
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

    /// The `apud [i, j]` coordinate clause from the shared `apud`
    /// production: the coordinate bindings are visible to the construct's
    /// body, so dropping the clause breaks bodies that reference them.
    fn write_apud_coordinates(
        &self,
        coordinates: &[(DefId, Symbol)],
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
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
    }

    /// The lane-binder clause from the shared `filum` production; dropping
    /// it rewrites the declared source head.
    fn write_filum_clause(
        &self,
        filum: Option<(DefId, Symbol)>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some((_, lane)) = filum {
            writer.write(" ");
            writer.write(surface.contextual_keyword("filum"));
            writer.write(" ");
            writer.write(&self.symbol_to_string(lane));
        }
    }

    /// The `sit binder { body }` tail shared by `reducta` and `ordinata`:
    /// HIR keeps binder and body jointly present or absent, so one clause
    /// writes both or neither.
    fn write_binder_body_clause(
        &self,
        binding: Option<(DefId, Symbol)>,
        body: Option<&HirBlock>,
        mutable: bool,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let (Some((_, binder)), Some(body)) = (binding, body) {
            writer.write(" ");
            writer.write(loop_binder_keyword(mutable, surface));
            writer.write(" ");
            writer.write(&self.symbol_to_string(binder));
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_block(body, names, surface, writer);
                self.write_block_end_trivia(body, writer);
            });
            writer.write("}");
        }
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
            if self.write_forma_capture(owner, entries, target, names, surface, writer) {
                return;
            }

            match self.types.get(target) {
                Type::Struct(def_id) => {
                    let name = self
                        .imported_type_spellings
                        .get(&target)
                        .cloned()
                        .unwrap_or_else(|| self.name_for_def(*def_id, names));
                    // The copy-with-changes source (D15.1) is the `Spread`
                    // entry; it re-emits after the braces as `ex source`.
                    let (sources, fields): (Vec<_>, Vec<_>) = entries
                        .iter()
                        .partition(|field| matches!(field.key, HirObjectKey::Spread(_)));
                    if self.inferred_constructor_ids.contains(&owner) {
                        writer.write("_");
                    } else {
                        writer.write(&name);
                    }
                    writer.write(" {");
                    if !fields.is_empty() {
                        writer.write(" ");
                    }
                    for (idx, field) in fields.iter().enumerate() {
                        if idx > 0 {
                            writer.write(", ");
                        }
                        self.write_genus_field(field, names, surface, writer);
                    }
                    if !fields.is_empty() {
                        writer.write(" ");
                    }
                    writer.write("}");
                    for source in sources {
                        if let HirObjectKey::Spread(expr) = &source.key {
                            writer.write(" ");
                            writer.write(surface.keyword(TokenKind::Ex));
                            writer.write(" ");
                            self.write_expr_prec(expr, 12, names, surface, writer);
                        }
                    }
                    return;
                }
                Type::Record(_) => {
                    if self.inferred_constructor_ids.contains(&owner) {
                        writer.write("_");
                    } else {
                        writer.write(&rendered_target);
                    }
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
        owner: radix_hir::HirId,
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

        let authored = self.template_holes.get(&owner);
        let sym = authored.map_or(sym, |holes| &holes.template);
        writer.write("`");
        // Raw payload: `writer.write` would re-indent every content line by the
        // enclosing block level, and the re-indented text is part of the raw
        // string on the next round — the emit would never stabilize.
        writer.write_raw(self.interner.resolve(*sym));
        writer.write("`");
        if let Some(param_exprs) = param_exprs
            && !param_exprs.is_empty()
        {
            self.write_template_actuals(&param_exprs, authored, names, surface, writer);
        }
        true
    }

    /// The actual list of a template application: `(a, b)` positional, or
    /// `(label: a, …)` in authored order when the application recorded labels.
    fn write_template_actuals(
        &self,
        args: &[&HirExpression],
        authored: Option<&radix_hir::HirTemplateHoles>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write("(");
        match authored {
            Some(holes) if !holes.labeled_actuals.is_empty() => {
                for (idx, (label, position)) in holes.labeled_actuals.iter().enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    writer.write(&self.symbol_to_string(*label));
                    writer.write(": ");
                    if let Some(arg) = args.get(*position as usize).copied() {
                        self.write_expr(arg, names, surface, writer);
                    }
                }
            }
            _ => {
                for (idx, arg) in args.iter().enumerate() {
                    if idx > 0 {
                        writer.write(", ");
                    }
                    self.write_expr(arg, names, surface, writer);
                }
            }
        }
        writer.write(")");
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
            self.write_fac_omnia_marker(loop_block, surface, writer);
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_stmts(body_stmts, names, surface, writer);
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

        // Attached handlers (`elige … cape`, `dum … cape`, `itera … cape`):
        // the handled body's tail is the construct itself. Re-emitting it as
        // `fac { … } cape` would demote that tail to a statement.
        if body.statements.is_empty()
            && let Some(expr) = &body.expr
            && self.accepts_attached_cape(expr)
        {
            self.write_expr(expr, names, surface, writer);
            writer.write(" ");
            writer.write(surface.keyword(TokenKind::Cape));
            writer.write(" ");
            self.write_cape_block_named(
                &catch.body,
                Some(catch.binding_name),
                names,
                surface,
                writer,
            );
            return;
        }

        // `fac omnia` rides the synthetic `Block` tail the same way
        // `write_fac_body` unwraps it; the handler keyword follows the body.
        let modifier = if body.statements.is_empty() {
            match body.expr.as_deref().map(|expr| &expr.kind) {
                Some(HirExpressionKind::Block(inner)) => inner
                    .breakable
                    .and_then(radix_hir::HirBreakableKind::fac_modifier_key),
                _ => None,
            }
        } else {
            None
        };
        writer.write(surface.contextual_keyword("fac"));
        if let Some(modifier) = modifier {
            writer.write(" ");
            writer.write(surface.contextual_keyword(modifier));
        }
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

    /// Constructs whose grammar takes a trailing `cape` clause directly.
    fn accepts_attached_cape(&self, expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Discerne { .. } => self.elige_expr_ids.contains(&expr.id),
            HirExpressionKind::Dum(..)
            | HirExpressionKind::Itera { .. }
            | HirExpressionKind::IteraRange { .. }
            | HirExpressionKind::IteraLockstep { .. } => true,
            _ => false,
        }
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
            // A marked size argument travels as a `Param` spelled with the
            // size marker (`radix_types::parse_explicit_size`): re-emit the
            // marker in the reader surface, then its operand.
            if let Type::Param(sym) = self.types.get(*ty)
                && let Some(operand) = self
                    .interner
                    .resolve(*sym)
                    .strip_prefix(radix_types::EXPLICIT_SIZE_MARKER)
            {
                writer.write(surface.keyword(TokenKind::Magnitudo));
                writer.write(" ");
                writer.write(operand);
                continue;
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
            if let Some(marker) = arg.marker {
                writer.write(surface.keyword(match marker {
                    radix_hir::HirArgMarker::Ref => TokenKind::De,
                    radix_hir::HirArgMarker::Mut => TokenKind::In,
                    radix_hir::HirArgMarker::Own => TokenKind::Own,
                    radix_hir::HirArgMarker::Copy => TokenKind::Copy,
                    radix_hir::HirArgMarker::Axis => TokenKind::Axis,
                }));
                writer.write(" ");
            }
            if arg.spread {
                writer.write(surface.keyword(TokenKind::Sparge));
                writer.write(" ");
            }
            self.write_expr(&arg.expr, names, surface, writer);
        }
    }

    /// The claimed tensor type of a typed-zero `Type {}` expression, when the
    /// call has the exact shape lowering synthesizes: a `vacua` receiver
    /// pre-typed with the tensor, the `crea` method, fill then extents.
    /// Authored `.crea(..)` on a tensor is a retired spelling the checker
    /// refuses, so this shape never comes from user source.
    fn typed_zero_tensor(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        type_args: &[TypeId],
        args: &[radix_hir::HirCallArg],
    ) -> Option<TypeId> {
        if !matches!(receiver.kind, HirExpressionKind::Vacua)
            || !type_args.is_empty()
            || args.len() != 2
        {
            return None;
        }
        let ty = receiver.ty?;
        if !matches!(self.types.get(ty), Type::Tensor(..)) {
            return None;
        }
        // Typecheck rewrites the synthesized call to the canonical spelling.
        (self.interner.resolve(name) == "crea").then_some(ty)
    }

    /// A vector or matrix zero: a `Verte` of a literal array of zeros whose
    /// target is the register type and which carries no authored ascription
    /// form (an authored `[0, 0] ∷ vf32[2]` records one at its site).
    fn is_typed_zero_register(
        &self,
        id: radix_hir::HirId,
        source: &HirExpression,
        target: TypeId,
    ) -> bool {
        fn zero_leaves(expr: &HirExpression) -> bool {
            match &expr.kind {
                HirExpressionKind::Array(items) => items.iter().all(|item| match item {
                    HirArrayElement::Expr(inner) => zero_leaves(inner),
                    _ => false,
                }),
                HirExpressionKind::Literal(HirLiteral::Int(0)) => true,
                HirExpressionKind::Literal(HirLiteral::Float(value)) => *value == 0.0,
                _ => false,
            }
        }
        if !matches!(self.types.get(target), Type::Vector(..) | Type::Matrix(..)) {
            return false;
        }
        let site = HirTypeSite {
            owner: HirOwnerRef::Hir(id),
            slot: HirTypeSlot::AscriptionTarget,
        };
        if self.authored_type_forms.contains_key(&site) {
            return false;
        }
        matches!(source.kind, HirExpressionKind::Array(_)) && zero_leaves(source)
    }

    /// Write `Type {}`: the type as authored where the sidecar kept a form,
    /// else canonically, then the empty braces.
    fn write_typed_zero(
        &self,
        owner: radix_hir::HirId,
        ty: TypeId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let site = HirTypeSite {
            owner: HirOwnerRef::Hir(owner),
            slot: HirTypeSlot::TypedZero,
        };
        let rendered = self
            .type_to_faber_at(&site, ty, names, surface)
            .unwrap_or_else(|error| self.record_type_emission_error(error));
        writer.write(&rendered);
        writer.write(" {}");
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

    /// The `(receiver, canonical method, argument)` of an authored numeric
    /// twin call whose desugar left the call's node id on the node itself: an
    /// in-place `Assign` (`place ← place op arg`, `place ← op place`) or a
    /// value twin `Binary` / `Unary` (`x.added(y)`, `x.negated()`). `None`
    /// when `expr` is not a recorded twin or no longer has the desugared
    /// shape. The effect-hoisted `Block` spelling is
    /// [`Self::hoisted_numeric_twin`].
    pub(super) fn numeric_twin_call<'e>(
        &self,
        expr: &'e HirExpression,
    ) -> Option<(&'e HirExpression, &'static str, Option<&'e HirExpression>)> {
        if !self.numeric_twin_ids.contains(&expr.id) {
            return None;
        }
        match &expr.kind {
            // The store's implicit conversion and widening nodes are the
            // checker's, not part of the authored call; the stored place is
            // the authored receiver.
            HirExpressionKind::Assign(place, value) => {
                let value = strip_store_wrappers(value);
                match &value.kind {
                    HirExpressionKind::Binary(op, _, argument) => Some((
                        place,
                        radix_semantic::passes::in_place_binary_twin_name(*op)?,
                        Some(argument),
                    )),
                    HirExpressionKind::Unary(op, _) => Some((
                        place,
                        radix_semantic::passes::in_place_unary_twin_name(*op)?,
                        None,
                    )),
                    _ => None,
                }
            }
            // A value twin is the operator node the rewrite left at the
            // call's id; the operator's operand is the authored receiver.
            HirExpressionKind::Binary(op, receiver, argument) => Some((
                receiver,
                radix_semantic::passes::binary_twin_name(*op)?,
                Some(argument),
            )),
            HirExpressionKind::Unary(op, receiver) => Some((
                receiver,
                radix_semantic::passes::unary_twin_name(*op)?,
                None,
            )),
            _ => None,
        }
    }

    /// The authored axis-reduction call an expansion node carries: the `Sum`
    /// rewrite records its `Reduction` node id, the `Mean` rewrite the outer
    /// `Div` (whose numerator may sit behind the integer-element `Conversio`).
    /// The returned receiver is the tensor the author wrote before the axis
    /// list. `None` when the node no longer has that shape, so emission falls
    /// back to the ordinary spelling.
    pub(super) fn axis_sugar_call<'e>(
        &self,
        expr: &'e HirExpression,
    ) -> Option<(&'e HirExpression, radix_hir::HirAxisSugar)> {
        let sugar = self.axis_sugar.get(&expr.id)?.clone();
        let reduction = match sugar.method {
            radix_hir::HirAxisMethod::Mean => {
                let radix_hir::HirExpressionKind::Binary(radix_hir::HirBinOp::Div, numerator, _) =
                    &expr.kind
                else {
                    return None;
                };
                strip_conversio_wrappers(numerator)
            }
            radix_hir::HirAxisMethod::Sum
            | radix_hir::HirAxisMethod::Max
            | radix_hir::HirAxisMethod::Min => expr,
        };
        let radix_hir::HirExpressionKind::Reduction { source, .. } = &reduction.kind else {
            return None;
        };
        Some((source, sugar))
    }

    /// `receiver.method(axis n)` for an authored axis-reduction call, in the
    /// reader locale's surface spelling.
    fn write_axis_sugar_call(
        &self,
        source: &HirExpression,
        sugar: &radix_hir::HirAxisSugar,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_expr_prec(source, 13, names, surface, writer);
        writer.write(".");
        let canonical = match sugar.method {
            radix_hir::HirAxisMethod::Sum => "summa",
            radix_hir::HirAxisMethod::Mean => "media",
            radix_hir::HirAxisMethod::Max => "maximus",
            radix_hir::HirAxisMethod::Min => "minimus",
        };
        writer.write(
            &surface
                .intrinsic_surface(canonical)
                .unwrap_or_else(|| canonical.to_owned()),
        );
        writer.write("(");
        writer.write(surface.keyword(TokenKind::Axis));
        writer.write(" ");
        writer.write(&sugar.axis.to_string());
        writer.write(")");
    }

    /// The authored in-place twin call an effect-hoisted desugar wrapped in a
    /// block (`xs[f()].add(2)` desugars to a `__place` temporary plus the
    /// store): each temporary is inlined back into the store so re-emission
    /// prints `place.method(arg)`, not the checker's temporaries. `None` when
    /// `expr` is not a recorded twin or no longer has that shape.
    pub(super) fn hoisted_numeric_twin(
        &self,
        expr: &HirExpression,
    ) -> Option<(HirExpression, &'static str, Option<HirExpression>)> {
        if !self.numeric_twin_ids.contains(&expr.id) {
            return None;
        }
        let HirExpressionKind::Block(block) = &expr.kind else {
            return None;
        };
        if block.expr.is_some() || block.statements.len() < 2 {
            return None;
        }
        let (assign_stmt, local_stmts) = block.statements.split_last()?;
        let HirStatementKind::Expr(assign) = &assign_stmt.kind else {
            return None;
        };
        let HirExpressionKind::Assign(place, value) = &assign.kind else {
            return None;
        };
        let mut temps = FxHashMap::default();
        for stmt in local_stmts {
            let HirStatementKind::Local(local) = &stmt.kind else {
                return None;
            };
            let Some(init) = &local.init else {
                return None;
            };
            temps.insert(local.def_id, init.clone());
        }
        let inlined_value = inline_place_temps(value, &temps);
        let value = strip_store_wrappers(&inlined_value);
        let (name, argument) = match &value.kind {
            HirExpressionKind::Binary(op, _, argument) => (
                radix_semantic::passes::in_place_binary_twin_name(*op)?,
                Some((**argument).clone()),
            ),
            HirExpressionKind::Unary(op, _) => {
                (radix_semantic::passes::in_place_unary_twin_name(*op)?, None)
            }
            _ => return None,
        };
        Some((inline_place_temps(place, &temps), name, argument))
    }

    /// A rounding method call (`floor`, `ceil`, `round` twins) whose receiver
    /// binds looser than the postfix chain, with its glyph delimiters. The
    /// method form would print `(receiver).round()`, which cannot follow a
    /// statement-head output keyword; the glyph form `⌊receiver⌉` is the same
    /// operation and starts with its own delimiter.
    fn parenthesized_rounding_receiver<'e>(
        &self,
        expr: &'e HirExpression,
        surface: &impl KeywordSurface,
    ) -> Option<(&'e HirExpression, &'static str, &'static str)> {
        let HirExpressionKind::MethodCall(receiver, name, type_args, args, _) = &expr.kind else {
            return None;
        };
        if !type_args.is_empty() || !args.is_empty() || self.glyph_ids.contains(&expr.id) {
            return None;
        }
        if self.expr_precedence(receiver) > 13 {
            return None;
        }
        let (open, close) = self.rounding_delimiters(*name, surface)?;
        Some((&**receiver, open, close))
    }

    /// The glyph delimiters of a rounding twin (`pavimentum` floor, `tectum`
    /// ceiling, `rotunda` nearest), matched by canonical or localized name.
    fn rounding_delimiters(
        &self,
        name: Symbol,
        surface: &impl KeywordSurface,
    ) -> Option<(&'static str, &'static str)> {
        let spelling = self.symbol_to_string(name);
        let delimiters = [
            ("pavimentum", "⌊", "⌋"),
            ("tectum", "⌈", "⌉"),
            ("rotunda", "⌊", "⌉"),
        ];
        delimiters.into_iter().find_map(|(canonical, open, close)| {
            let localized = surface.intrinsic_surface(canonical);
            (spelling == canonical || localized.as_deref() == Some(spelling.as_str()))
                .then_some((open, close))
        })
    }

    /// The operand of an authored `∜`: the receiver of the inner `radix` call
    /// under the recorded outer call, or `None` when the shape no longer
    /// matches (the id then falls back to the nested method-call spelling).
    fn radical_quarta_operand(inner: &HirExpression) -> Option<&HirExpression> {
        match &inner.kind {
            HirExpressionKind::MethodCall(operand, _, type_args, args, _)
                if type_args.is_empty() && args.is_empty() =>
            {
                Some(operand)
            }
            _ => None,
        }
    }

    /// The glyph prefix of a radical twin (`radix` square root, `radix_cubica`
    /// cube root), matched by canonical or localized name.
    fn radical_prefix(&self, name: Symbol, surface: &impl KeywordSurface) -> Option<&'static str> {
        let spelling = self.symbol_to_string(name);
        let glyphs = [("radix", "√"), ("radix_cubica", "∛")];
        glyphs.into_iter().find_map(|(canonical, glyph)| {
            let localized = surface.intrinsic_surface(canonical);
            (spelling == canonical || localized.as_deref() == Some(spelling.as_str()))
                .then_some(glyph)
        })
    }

    /// `receiver.method(argument)` for an authored numeric twin call, in the
    /// reader locale's surface spelling.
    fn write_twin_call(
        &self,
        receiver: &HirExpression,
        canonical: &'static str,
        argument: Option<&HirExpression>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_expr_prec(receiver, 13, names, surface, writer);
        writer.write(".");
        writer.write(
            &surface
                .intrinsic_surface(canonical)
                .unwrap_or_else(|| canonical.to_owned()),
        );
        writer.write("(");
        if let Some(argument) = argument {
            self.write_expr(argument, names, surface, writer);
        }
        writer.write(")");
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
        } else if self.directional_view_projects_method(receiver, name, names) {
            // `t.accipe()` / `t.fini()` on a directional view re-emit as the
            // pack's `[intrinsics]` row (en: `t.get()` / `t.finish()`).
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

    /// Whether `name` is a locale-projected method (`radix_intrinsics::SERMO_VIEW_METHODS`)
    /// called on a `meus`/`tuus` directional view receiver.
    fn directional_view_projects_method(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        names: &FxHashMap<DefId, Symbol>,
    ) -> bool {
        let spelling = self.interner.resolve(name);
        if !radix_intrinsics::SERMO_VIEW_METHODS
            .iter()
            .any(|entry| entry.faber_name == spelling)
        {
            return false;
        }
        let Some(mut ty) = receiver.ty else {
            return false;
        };
        loop {
            match self.types.get(ty) {
                Type::Ref(_, inner) | Type::Option(inner) | Type::Alias(_, inner) => ty = *inner,
                Type::Applied(base, _) => ty = *base,
                Type::Struct(def_id) => {
                    return matches!(
                        self.builtin_frame_family(*def_id, names),
                        Some(FrameFamily::Meus | FrameFamily::Tuus)
                    );
                }
                _ => return false,
            }
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
                            | Type::RangedArray { .. }
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
                            | Type::Intervallum(..)
                            | Type::Atomic(_)
                            | Type::SizedNumeric(_, _)
                            | Type::ModularWord(_)
                            | Type::Primitive(
                                Primitive::Octeti
                                    | Primitive::Textus
                                    | Primitive::Regex
                                    | Primitive::Match
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

/// The stored value with the checker's implicit conversion and widening
/// nodes stripped: those wrap the desugared twin, not the authored call.
fn strip_store_wrappers(mut value: &HirExpression) -> &HirExpression {
    loop {
        match &value.kind {
            HirExpressionKind::Conversio {
                implicit: Some(_),
                source,
                ..
            } => value = source,
            HirExpressionKind::Widen { operand, .. } => value = operand,
            _ => return value,
        }
    }
}

/// Peel the checker's conversion node off an axis-sugar numerator: the
/// integer-element `mean` converts the exact sum before dividing.
fn strip_conversio_wrappers(mut expr: &HirExpression) -> &HirExpression {
    while let HirExpressionKind::Conversio { source, .. } = &expr.kind {
        expr = source;
    }
    expr
}

/// A copy of `expr` with each hoisted place temporary (`__placeN`) replaced
/// by the effectful expression it pins, restoring the authored place. Only
/// the shapes a place chain or a twin value can take carry temporaries.
fn inline_place_temps(
    expr: &HirExpression,
    temps: &FxHashMap<DefId, HirExpression>,
) -> HirExpression {
    if let HirExpressionKind::Path(def_id) = &expr.kind
        && let Some(init) = temps.get(def_id)
    {
        return init.clone();
    }
    let kind = match &expr.kind {
        HirExpressionKind::Field(base, name) => {
            HirExpressionKind::Field(Box::new(inline_place_temps(base, temps)), *name)
        }
        HirExpressionKind::Deref(base) => {
            HirExpressionKind::Deref(Box::new(inline_place_temps(base, temps)))
        }
        HirExpressionKind::Index(base, index) => HirExpressionKind::Index(
            Box::new(inline_place_temps(base, temps)),
            Box::new(inline_place_temps(index, temps)),
        ),
        HirExpressionKind::Binary(op, lhs, rhs) => HirExpressionKind::Binary(
            *op,
            Box::new(inline_place_temps(lhs, temps)),
            Box::new(inline_place_temps(rhs, temps)),
        ),
        HirExpressionKind::Unary(op, operand) => {
            HirExpressionKind::Unary(*op, Box::new(inline_place_temps(operand, temps)))
        }
        other => other.clone(),
    };
    HirExpression {
        id: expr.id,
        kind,
        ty: expr.ty,
        span: expr.span,
    }
}
