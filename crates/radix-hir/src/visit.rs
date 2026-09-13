//! Traversal contracts for HIR analyses and tree rewrites.
//!
//! This module centralizes the recursive shape of HIR so analysis passes do not
//! each re-encode which child nodes belong to which construct. The default
//! visitor methods are pre-order at the semantic boundary: declaration hooks
//! fire when a definition is encountered, then walking proceeds into the
//! lowered children in source/evaluation order where that order is meaningful.
//!
//! INVARIANTS
//! ==========
//! - Walkers visit HIR children, not type arena contents or symbol interner
//!   data. `TypeId` and `Symbol` values are references into other compiler
//!   state, not subtrees.
//! - `visit_def` is called for definitions introduced by items, parameters,
//!   locals, patterns, imports, methods, fields, handlers, and loop bindings.
//! - Read-only and mutable walkers should preserve the same traversal shape.
//! - Walkers do not perform semantic validation; they only expose traversal
//!   order for passes that choose to validate, collect, or mutate.
//!
//! TRADE-OFFS
//! ==========
//! The walkers are intentionally explicit `match` statements instead of a macro
//! or generated visitor. HIR changes are compiler phase changes, and reviewing
//! traversal updates beside new variants is more valuable than hiding the shape
//! behind abstraction.

use crate::nodes::{
    DefId, HirAnnotation, HirAnnotationValue, HirArrayElement, HirBlock, HirCallArg, HirCape,
    HirCasuArm, HirConversioTarget, HirExpression, HirExpressionKind, HirField, HirFunction,
    HirImportItem, HirItem, HirItemKind, HirLocal, HirMethod, HirModule, HirNonNullKind,
    HirObjectField, HirObjectKey, HirOptionalChainKind, HirParam, HirPattern, HirStatement,
    HirStatementKind, HirTypeParam, HirVariant,
};
use radix_lexer::Symbol;

// ---------------------------------------------------------------------------
// Read-only traversal
// ---------------------------------------------------------------------------

/// Read-only visitor over HIR.
///
/// Override the narrowest hook needed by the analysis. Default implementations
/// recursively walk children, so a pass can observe only expressions, only
/// definitions, or only a specific declaration kind without duplicating the
/// rest of the tree traversal.
pub trait HirVisitor: Sized {
    fn visit_program(&mut self, program: &HirModule) {
        walk_program(self, program);
    }

    fn visit_item(&mut self, item: &HirItem) {
        walk_item(self, item);
    }

    fn visit_function(&mut self, function: &HirFunction) {
        walk_function(self, function);
    }

    fn visit_type_param(&mut self, type_param: &HirTypeParam) {
        self.visit_def(type_param.def_id, type_param.name);
    }

    fn visit_param(&mut self, param: &HirParam) {
        self.visit_def(param.def_id, param.name);
        if let Some(default) = &param.default {
            self.visit_expr(default);
        }
    }

    fn visit_field(&mut self, field: &HirField) {
        walk_field(self, field);
    }

    fn visit_method(&mut self, method: &HirMethod) {
        walk_method(self, method);
    }

    fn visit_variant(&mut self, variant: &HirVariant) {
        self.visit_def(variant.def_id, variant.name);
    }

    fn visit_import_item(&mut self, item: &HirImportItem) {
        self.visit_def(item.def_id, item.alias.unwrap_or(item.name));
    }

    fn visit_block(&mut self, block: &HirBlock) {
        walk_block(self, block);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        walk_stmt(self, stmt);
    }

    fn visit_local(&mut self, local: &HirLocal) {
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        walk_expr(self, expr);
    }

    fn visit_cape(&mut self, cape: &HirCape) {
        walk_cape(self, cape);
    }

    fn visit_casu_arm(&mut self, arm: &HirCasuArm) {
        walk_casu_arm(self, arm);
    }

    fn visit_pattern(&mut self, pattern: &HirPattern) {
        walk_pattern(self, pattern);
    }

    fn visit_object_field(&mut self, field: &HirObjectField) {
        walk_object_field(self, field);
    }

    fn visit_annotation(&mut self, annotation: &HirAnnotation) {
        walk_annotation(self, annotation);
    }

    /// Observe a definition introduced by the current subtree.
    ///
    /// This hook is intentionally lighter than visiting a declaration node:
    /// locals, pattern bindings, handler bindings, imported aliases, and loop
    /// variables also introduce `DefId` values and should be visible to passes
    /// that build definition indexes.
    fn visit_def(&mut self, _def_id: DefId, _name: Symbol) {}
}

/// Walk a whole HIR program in item order, then entry-block order.
pub fn walk_program<V: HirVisitor>(visitor: &mut V, program: &HirModule) {
    for item in &program.items {
        visitor.visit_item(item);
    }
    for annotation in &program.entry_annotations {
        visitor.visit_annotation(annotation);
    }
    if let Some(entry) = &program.entry {
        visitor.visit_block(entry);
    }
}

/// Item-level definitions are reported before child signatures/bodies so
/// definition collectors can see the outer declaration before nested bindings.
pub fn walk_item<V: HirVisitor>(visitor: &mut V, item: &HirItem) {
    for annotation in &item.annotations {
        visitor.visit_annotation(annotation);
    }
    match &item.kind {
        HirItemKind::Function(function) => {
            visitor.visit_def(item.def_id, function.name);
            visitor.visit_function(function);
        }
        HirItemKind::Struct(strukt) => {
            visitor.visit_def(item.def_id, strukt.name);
            for type_param in &strukt.type_params {
                visitor.visit_type_param(type_param);
            }
            for field in &strukt.fields {
                visitor.visit_field(field);
            }
            for method in &strukt.methods {
                visitor.visit_method(method);
            }
        }
        HirItemKind::Enum(enum_item) => {
            visitor.visit_def(item.def_id, enum_item.name);
            for type_param in &enum_item.type_params {
                visitor.visit_type_param(type_param);
            }
            for variant in &enum_item.variants {
                visitor.visit_variant(variant);
            }
        }
        HirItemKind::Interface(interface) => {
            visitor.visit_def(item.def_id, interface.name);
            for type_param in &interface.type_params {
                visitor.visit_type_param(type_param);
            }
            for method in &interface.methods {
                for param in &method.params {
                    visitor.visit_param(param);
                }
            }
        }
        HirItemKind::TypeAlias(alias) => {
            visitor.visit_def(item.def_id, alias.name);
            for type_param in &alias.type_params {
                visitor.visit_type_param(type_param);
            }
        }
        HirItemKind::Schema(schema) => {
            visitor.visit_def(item.def_id, schema.name);
        }
        HirItemKind::Constant(const_item) => {
            visitor.visit_def(item.def_id, const_item.name);
            visitor.visit_expr(&const_item.value);
        }
        HirItemKind::Import(import) => {
            for item in &import.items {
                visitor.visit_import_item(item);
            }
        }
    }
}

pub fn walk_function<V: HirVisitor>(visitor: &mut V, function: &HirFunction) {
    for type_param in &function.type_params {
        visitor.visit_type_param(type_param);
    }
    for param in &function.params {
        visitor.visit_param(param);
    }
    if let Some(param) = &function.cli_args {
        visitor.visit_param(param);
    }
    if let Some(body) = &function.body {
        visitor.visit_block(body);
    }
}

pub fn walk_field<V: HirVisitor>(visitor: &mut V, field: &HirField) {
    visitor.visit_def(field.def_id, field.name);
    if let Some(init) = &field.init {
        visitor.visit_expr(init);
    }
    for annotation in &field.annotations {
        visitor.visit_annotation(annotation);
    }
}

/// Only `HirAnnotationValue::Expr` owns a HIR subtree; other payloads are side-table references.
pub fn walk_annotation<V: HirVisitor>(visitor: &mut V, annotation: &HirAnnotation) {
    for field in &annotation.fields {
        if let HirAnnotationValue::Expr(expr) = &field.value {
            visitor.visit_expr(expr);
        }
    }
}

/// Walk a method as both a method definition and a function payload.
pub fn walk_method<V: HirVisitor>(visitor: &mut V, method: &HirMethod) {
    visitor.visit_def(method.def_id, method.func.name);
    visitor.visit_function(&method.func);
}

/// Walk a block in execution order, with the tail expression last.
pub fn walk_block<V: HirVisitor>(visitor: &mut V, block: &HirBlock) {
    for stmt in &block.statements {
        visitor.visit_stmt(stmt);
    }
    if let Some(expr) = &block.expr {
        visitor.visit_expr(expr);
    }
}

pub fn walk_stmt<V: HirVisitor>(visitor: &mut V, stmt: &HirStatement) {
    match &stmt.kind {
        HirStatementKind::Local(local) => visitor.visit_local(local),
        HirStatementKind::Expr(expr) => visitor.visit_expr(expr),
        HirStatementKind::Redde(value) => {
            if let Some(expr) = value {
                visitor.visit_expr(expr);
            }
        }
        HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::Tacet => {}
        HirStatementKind::IncDec(inc_dec) => visitor.visit_expr(&inc_dec.target),
        HirStatementKind::Custodi(custodi) => {
            for clause in &custodi.clauses {
                visitor.visit_expr(&clause.cond);
                visitor.visit_block(&clause.body);
            }
        }
    }
}

pub fn walk_local<V: HirVisitor>(visitor: &mut V, local: &HirLocal) {
    visitor.visit_def(local.def_id, local.name);
    if let Some(init) = &local.init {
        visitor.visit_expr(init);
    }
}

/// Walk an expression's child expressions, blocks, patterns, and introduced bindings.
///
/// Leaf values such as resolved paths, literals, and type references are not
/// walked because they point into semantic side tables rather than owning HIR
/// subtrees.
#[allow(clippy::too_many_lines)]
pub fn walk_expr<V: HirVisitor>(visitor: &mut V, expr: &HirExpression) {
    match &expr.kind {
        HirExpressionKind::Path(_)
        | HirExpressionKind::Literal(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::ReadLine(_)
        | HirExpressionKind::Error => {}
        HirExpressionKind::Praefixum(inner) => visitor.visit_expr(inner),
        HirExpressionKind::Transfer { sink, payloads } => {
            visitor.visit_expr(sink);
            for payload in payloads {
                visitor.visit_expr(payload);
            }
        }
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            visitor.visit_expr(lhs);
            visitor.visit_expr(rhs);
        }
        HirExpressionKind::ConversioAssign {
            target,
            source,
            recovery,
        } => {
            visitor.visit_expr(target);
            visitor.visit_expr(source);
            if let Some(recovery) = recovery {
                visitor.visit_expr(recovery);
            }
        }
        HirExpressionKind::Unary(_, operand)
        | HirExpressionKind::Cede(operand)
        | HirExpressionKind::Reddet(operand)
        | HirExpressionKind::Tacebit(operand)
        | HirExpressionKind::Yield(operand)
        | HirExpressionKind::Ref(_, operand)
        | HirExpressionKind::Deref(operand)
        | HirExpressionKind::Panic(operand)
        | HirExpressionKind::Throw(operand) => visitor.visit_expr(operand),
        HirExpressionKind::Call(callee, _, args, _) => {
            visitor.visit_expr(callee);
            walk_call_args(visitor, args);
        }
        HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
            visitor.visit_expr(receiver);
            walk_call_args(visitor, args);
        }
        HirExpressionKind::Field(object, _) => visitor.visit_expr(object),
        HirExpressionKind::Index(object, index) => {
            visitor.visit_expr(object);
            visitor.visit_expr(index);
        }
        HirExpressionKind::Gradient {
            call,
            selected_arguments,
        } => {
            visitor.visit_expr(call);
            for selector in selected_arguments {
                visitor.visit_expr(selector);
            }
        }
        HirExpressionKind::OptionalChain(object, chain) => {
            visitor.visit_expr(object);
            walk_optional_chain(visitor, chain);
        }
        HirExpressionKind::NonNull(object, chain) => {
            visitor.visit_expr(object);
            walk_non_null(visitor, chain);
        }
        HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
            visitor.visit_block(block);
        }
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            visitor.visit_expr(cond);
            visitor.visit_block(then_block);
            if let Some(catch) = then_catch {
                visitor.visit_cape(catch);
            }
            if let Some(else_block) = else_block {
                visitor.visit_block(else_block);
            }
        }
        HirExpressionKind::Discerne {
            scrutinees, arms, ..
        } => {
            for scrutinee in scrutinees {
                visitor.visit_expr(scrutinee);
            }
            for arm in arms {
                visitor.visit_casu_arm(arm);
            }
        }
        HirExpressionKind::Dum(cond, block) => {
            visitor.visit_expr(cond);
            visitor.visit_block(block);
        }
        HirExpressionKind::Itera(_, coordinates, def_id, name, iter, block) => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            visitor.visit_def(*def_id, *name);
            visitor.visit_expr(iter);
            visitor.visit_block(block);
        }
        HirExpressionKind::IteraLockstep {
            sources,
            coordinates,
            bindings,
            body,
        } => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            for (binding_def_id, binding_name) in bindings {
                visitor.visit_def(*binding_def_id, *binding_name);
            }
            for source in sources {
                visitor.visit_expr(source);
            }
            visitor.visit_block(body);
        }
        HirExpressionKind::IteraRange {
            ranges,
            bindings,
            body,
        } => {
            // Ranges evaluate outside the binder scope, so they walk before
            // the binder defs (leftmost-outermost order preserved).
            for range in ranges {
                visitor.visit_expr(range);
            }
            for (binding_def_id, binding_name) in bindings {
                visitor.visit_def(*binding_def_id, *binding_name);
            }
            visitor.visit_block(body);
        }
        HirExpressionKind::Summa {
            source,
            coordinates,
            filum,
            binding,
            body,
        } => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            if let Some((filum_def_id, filum_name)) = filum {
                visitor.visit_def(*filum_def_id, *filum_name);
            }
            visitor.visit_def(binding.0, binding.1);
            visitor.visit_expr(source);
            visitor.visit_block(body);
        }
        HirExpressionKind::Extremum {
            source,
            coordinates,
            coalesce,
            ..
        } => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            visitor.visit_expr(source);
            if let Some(identity) = coalesce {
                visitor.visit_expr(identity);
            }
        }
        HirExpressionKind::FirstMatch {
            source,
            coordinates,
            binding,
            body,
        } => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            visitor.visit_def(binding.0, binding.1);
            visitor.visit_expr(source);
            visitor.visit_block(body);
        }
        HirExpressionKind::Intervallum {
            start, end, step, ..
        } => {
            visitor.visit_expr(start);
            visitor.visit_expr(end);
            if let Some(step) = step {
                visitor.visit_expr(step);
            }
        }
        HirExpressionKind::Array(elements) => {
            for element in elements {
                match element {
                    HirArrayElement::Expr(expr) | HirArrayElement::Spread(expr) => {
                        visitor.visit_expr(expr);
                    }
                }
            }
        }
        HirExpressionKind::Struct(_, fields) => {
            for (_, value) in fields {
                visitor.visit_expr(value);
            }
        }
        HirExpressionKind::Tuple(elements, _)
        | HirExpressionKind::Scribe(_, elements)
        | HirExpressionKind::Scriptum(_, elements) => {
            for element in elements {
                visitor.visit_expr(element);
            }
        }
        HirExpressionKind::Adfirma(cond, message) => {
            visitor.visit_expr(cond);
            if let Some(message) = message {
                visitor.visit_expr(message);
            }
        }
        HirExpressionKind::Handled { body, catch } => {
            visitor.visit_block(body);
            visitor.visit_cape(catch);
        }
        HirExpressionKind::Trap(body) => {
            visitor.visit_block(body);
        }
        HirExpressionKind::Clausura(params, _, _, body, _) => {
            for param in params {
                visitor.visit_param(param);
            }
            visitor.visit_expr(body);
        }
        HirExpressionKind::Verte {
            source, entries, ..
        } => {
            visitor.visit_expr(source);
            if let Some(entries) = entries {
                for field in entries {
                    visitor.visit_object_field(field);
                }
            }
        }
        HirExpressionKind::Conversio {
            source,
            target,
            recovery,
            ..
        } => {
            visitor.visit_expr(source);
            if let HirConversioTarget::Intervallum(operand) = target {
                visitor.visit_expr(operand);
            }
            if let Some(recovery) = recovery {
                visitor.visit_expr(recovery);
            }
        }
        HirExpressionKind::Ad { opener, .. } => {
            if let Some(opener) = opener {
                visitor.visit_expr(opener);
            }
        }
        HirExpressionKind::TypeCheck { expr, .. } => visitor.visit_expr(expr),
    }
}

pub fn walk_cape<V: HirVisitor>(visitor: &mut V, cape: &HirCape) {
    visitor.visit_def(cape.binding_def_id, cape.binding_name);
    visitor.visit_block(&cape.body);
}

/// Walk one pattern arm: patterns first, then guard, then body.
pub fn walk_casu_arm<V: HirVisitor>(visitor: &mut V, arm: &HirCasuArm) {
    for pattern in &arm.patterns {
        visitor.visit_pattern(pattern);
    }
    if let Some(guard) = &arm.guard {
        visitor.visit_expr(guard);
    }
    visitor.visit_expr(&arm.body);
}

pub fn walk_pattern<V: HirVisitor>(visitor: &mut V, pattern: &HirPattern) {
    match pattern {
        HirPattern::Wildcard | HirPattern::Literal(_) => {}
        HirPattern::TypeTest { binding, .. } => {
            if let Some((def_id, name)) = binding {
                visitor.visit_def(*def_id, *name);
            }
        }
        HirPattern::Binding(def_id, name) => visitor.visit_def(*def_id, *name),
        HirPattern::Alias(def_id, name, pattern) => {
            visitor.visit_def(*def_id, *name);
            visitor.visit_pattern(pattern);
        }
        HirPattern::Variant(_, patterns) => {
            for pattern in patterns {
                visitor.visit_pattern(pattern);
            }
        }
    }
}

pub fn walk_object_field<V: HirVisitor>(visitor: &mut V, field: &HirObjectField) {
    match &field.key {
        HirObjectKey::Ident(_) | HirObjectKey::String(_) => {}
        HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => visitor.visit_expr(expr),
    }
    if let Some(value) = &field.value {
        visitor.visit_expr(value);
    }
}

fn walk_optional_chain<V: HirVisitor>(visitor: &mut V, chain: &HirOptionalChainKind) {
    match chain {
        HirOptionalChainKind::Member(_) => {}
        HirOptionalChainKind::Index(index) => visitor.visit_expr(index),
        HirOptionalChainKind::Call(args) => walk_call_args(visitor, args),
    }
}

fn walk_non_null<V: HirVisitor>(visitor: &mut V, chain: &HirNonNullKind) {
    match chain {
        HirNonNullKind::Member(_) => {}
        HirNonNullKind::Index(index) => visitor.visit_expr(index),
        HirNonNullKind::Call(args) => walk_call_args(visitor, args),
    }
}

fn walk_call_args<V: HirVisitor>(visitor: &mut V, args: &[HirCallArg]) {
    for arg in args {
        visitor.visit_expr(&arg.expr);
    }
}

// ---------------------------------------------------------------------------
// Mutable traversal
// ---------------------------------------------------------------------------

/// Mutable visitor over HIR.
///
/// This mirrors [`HirVisitor`] but grants mutable access to nodes that own HIR
/// children. Use it for normalization and repair passes that preserve node
/// identity and tree shape contracts; rebuilding IDs or redefining semantic
/// ownership belongs in lowering or a dedicated transformation phase.
pub trait HirVisitorMut: Sized {
    fn visit_program_mut(&mut self, program: &mut HirModule) {
        walk_program_mut(self, program);
    }

    fn visit_item_mut(&mut self, item: &mut HirItem) {
        walk_item_mut(self, item);
    }

    fn visit_function_mut(&mut self, function: &mut HirFunction) {
        walk_function_mut(self, function);
    }

    fn visit_type_param_mut(&mut self, type_param: &mut HirTypeParam) {
        self.visit_def(type_param.def_id, type_param.name);
    }

    fn visit_param_mut(&mut self, param: &mut HirParam) {
        self.visit_def(param.def_id, param.name);
        if let Some(default) = &mut param.default {
            self.visit_expr_mut(default);
        }
    }

    fn visit_field_mut(&mut self, field: &mut HirField) {
        walk_field_mut(self, field);
    }

    fn visit_method_mut(&mut self, method: &mut HirMethod) {
        walk_method_mut(self, method);
    }

    fn visit_variant_mut(&mut self, variant: &mut HirVariant) {
        self.visit_def(variant.def_id, variant.name);
    }

    fn visit_import_item_mut(&mut self, item: &mut HirImportItem) {
        self.visit_def(item.def_id, item.alias.unwrap_or(item.name));
    }

    fn visit_block_mut(&mut self, block: &mut HirBlock) {
        walk_block_mut(self, block);
    }

    fn visit_stmt_mut(&mut self, stmt: &mut HirStatement) {
        walk_stmt_mut(self, stmt);
    }

    fn visit_local_mut(&mut self, local: &mut HirLocal) {
        walk_local_mut(self, local);
    }

    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        walk_expr_mut(self, expr);
    }

    fn visit_cape_mut(&mut self, cape: &mut HirCape) {
        walk_cape_mut(self, cape);
    }

    fn visit_casu_arm_mut(&mut self, arm: &mut HirCasuArm) {
        walk_casu_arm_mut(self, arm);
    }

    fn visit_pattern_mut(&mut self, pattern: &mut HirPattern) {
        walk_pattern_mut(self, pattern);
    }

    fn visit_object_field_mut(&mut self, field: &mut HirObjectField) {
        walk_object_field_mut(self, field);
    }

    fn visit_annotation_mut(&mut self, annotation: &mut HirAnnotation) {
        walk_annotation_mut(self, annotation);
    }

    /// Observe a definition introduced by the current subtree.
    ///
    /// The hook receives copied identity/name data rather than mutable access
    /// because changing definitions in place would invalidate resolver/typecheck
    /// side tables.
    fn visit_def(&mut self, _def_id: DefId, _name: Symbol) {}
}

/// Mutably walk a whole HIR program in item order, then entry-block order.
pub fn walk_program_mut<V: HirVisitorMut>(visitor: &mut V, program: &mut HirModule) {
    for item in &mut program.items {
        visitor.visit_item_mut(item);
    }
    for annotation in &mut program.entry_annotations {
        visitor.visit_annotation_mut(annotation);
    }
    if let Some(entry) = &mut program.entry {
        visitor.visit_block_mut(entry);
    }
}

pub fn walk_item_mut<V: HirVisitorMut>(visitor: &mut V, item: &mut HirItem) {
    for annotation in &mut item.annotations {
        visitor.visit_annotation_mut(annotation);
    }
    match &mut item.kind {
        HirItemKind::Function(function) => {
            visitor.visit_def(item.def_id, function.name);
            visitor.visit_function_mut(function);
        }
        HirItemKind::Struct(strukt) => {
            visitor.visit_def(item.def_id, strukt.name);
            for type_param in &mut strukt.type_params {
                visitor.visit_type_param_mut(type_param);
            }
            for field in &mut strukt.fields {
                visitor.visit_field_mut(field);
            }
            for method in &mut strukt.methods {
                visitor.visit_method_mut(method);
            }
        }
        HirItemKind::Enum(enum_item) => {
            visitor.visit_def(item.def_id, enum_item.name);
            for type_param in &mut enum_item.type_params {
                visitor.visit_type_param_mut(type_param);
            }
            for variant in &mut enum_item.variants {
                visitor.visit_variant_mut(variant);
            }
        }
        HirItemKind::Interface(interface) => {
            visitor.visit_def(item.def_id, interface.name);
            for type_param in &mut interface.type_params {
                visitor.visit_type_param_mut(type_param);
            }
            for method in &mut interface.methods {
                for param in &mut method.params {
                    visitor.visit_param_mut(param);
                }
            }
        }
        HirItemKind::TypeAlias(alias) => {
            visitor.visit_def(item.def_id, alias.name);
            for type_param in &mut alias.type_params {
                visitor.visit_type_param_mut(type_param);
            }
        }
        HirItemKind::Schema(schema) => {
            visitor.visit_def(item.def_id, schema.name);
        }
        HirItemKind::Constant(const_item) => {
            visitor.visit_def(item.def_id, const_item.name);
            visitor.visit_expr_mut(&mut const_item.value);
        }
        HirItemKind::Import(import) => {
            for item in &mut import.items {
                visitor.visit_import_item_mut(item);
            }
        }
    }
}

pub fn walk_function_mut<V: HirVisitorMut>(visitor: &mut V, function: &mut HirFunction) {
    for type_param in &mut function.type_params {
        visitor.visit_type_param_mut(type_param);
    }
    for param in &mut function.params {
        visitor.visit_param_mut(param);
    }
    if let Some(param) = &mut function.cli_args {
        visitor.visit_param_mut(param);
    }
    if let Some(body) = &mut function.body {
        visitor.visit_block_mut(body);
    }
}

pub fn walk_field_mut<V: HirVisitorMut>(visitor: &mut V, field: &mut HirField) {
    visitor.visit_def(field.def_id, field.name);
    if let Some(init) = &mut field.init {
        visitor.visit_expr_mut(init);
    }
    for annotation in &mut field.annotations {
        visitor.visit_annotation_mut(annotation);
    }
}

/// Only `HirAnnotationValue::Expr` owns a HIR subtree; other payloads are side-table references.
pub fn walk_annotation_mut<V: HirVisitorMut>(visitor: &mut V, annotation: &mut HirAnnotation) {
    for field in &mut annotation.fields {
        if let HirAnnotationValue::Expr(expr) = &mut field.value {
            visitor.visit_expr_mut(expr);
        }
    }
}

pub fn walk_method_mut<V: HirVisitorMut>(visitor: &mut V, method: &mut HirMethod) {
    visitor.visit_def(method.def_id, method.func.name);
    visitor.visit_function_mut(&mut method.func);
}

/// Mutably walk a block in execution order, with the tail expression last.
pub fn walk_block_mut<V: HirVisitorMut>(visitor: &mut V, block: &mut HirBlock) {
    for stmt in &mut block.statements {
        visitor.visit_stmt_mut(stmt);
    }
    if let Some(expr) = &mut block.expr {
        visitor.visit_expr_mut(expr);
    }
}

pub fn walk_stmt_mut<V: HirVisitorMut>(visitor: &mut V, stmt: &mut HirStatement) {
    match &mut stmt.kind {
        HirStatementKind::Local(local) => visitor.visit_local_mut(local),
        HirStatementKind::Expr(expr) => visitor.visit_expr_mut(expr),
        HirStatementKind::Redde(value) => {
            if let Some(expr) = value {
                visitor.visit_expr_mut(expr);
            }
        }
        HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::Tacet => {}
        HirStatementKind::IncDec(inc_dec) => visitor.visit_expr_mut(&mut inc_dec.target),
        HirStatementKind::Custodi(custodi) => {
            for clause in &mut custodi.clauses {
                visitor.visit_expr_mut(&mut clause.cond);
                visitor.visit_block_mut(&mut clause.body);
            }
        }
    }
}

pub fn walk_local_mut<V: HirVisitorMut>(visitor: &mut V, local: &mut HirLocal) {
    visitor.visit_def(local.def_id, local.name);
    if let Some(init) = &mut local.init {
        visitor.visit_expr_mut(init);
    }
}

/// Mutably walk an expression's child expressions, blocks, patterns, and bindings.
///
/// Leaf values such as resolved paths, literals, and type references are not
/// walked because they point into semantic side tables rather than owning HIR
/// subtrees.
#[allow(clippy::too_many_lines)]
pub fn walk_expr_mut<V: HirVisitorMut>(visitor: &mut V, expr: &mut HirExpression) {
    match &mut expr.kind {
        HirExpressionKind::Path(_)
        | HirExpressionKind::Literal(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::ReadLine(_)
        | HirExpressionKind::Error => {}
        HirExpressionKind::Praefixum(inner) => visitor.visit_expr_mut(inner),
        HirExpressionKind::Transfer { sink, payloads } => {
            visitor.visit_expr_mut(sink);
            for payload in payloads {
                visitor.visit_expr_mut(payload);
            }
        }
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            visitor.visit_expr_mut(lhs);
            visitor.visit_expr_mut(rhs);
        }
        HirExpressionKind::ConversioAssign {
            target,
            source,
            recovery,
        } => {
            visitor.visit_expr_mut(target);
            visitor.visit_expr_mut(source);
            if let Some(recovery) = recovery {
                visitor.visit_expr_mut(recovery);
            }
        }
        HirExpressionKind::Unary(_, operand)
        | HirExpressionKind::Cede(operand)
        | HirExpressionKind::Reddet(operand)
        | HirExpressionKind::Tacebit(operand)
        | HirExpressionKind::Yield(operand)
        | HirExpressionKind::Ref(_, operand)
        | HirExpressionKind::Deref(operand)
        | HirExpressionKind::Panic(operand)
        | HirExpressionKind::Throw(operand) => visitor.visit_expr_mut(operand),
        HirExpressionKind::Call(callee, _, args, _) => {
            visitor.visit_expr_mut(callee);
            walk_call_args_mut(visitor, args);
        }
        HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
            visitor.visit_expr_mut(receiver);
            walk_call_args_mut(visitor, args);
        }
        HirExpressionKind::Field(object, _) => visitor.visit_expr_mut(object),
        HirExpressionKind::Index(object, index) => {
            visitor.visit_expr_mut(object);
            visitor.visit_expr_mut(index);
        }
        HirExpressionKind::OptionalChain(object, chain) => {
            visitor.visit_expr_mut(object);
            walk_optional_chain_mut(visitor, chain);
        }
        HirExpressionKind::NonNull(object, chain) => {
            visitor.visit_expr_mut(object);
            walk_non_null_mut(visitor, chain);
        }
        HirExpressionKind::Gradient {
            call,
            selected_arguments,
        } => {
            visitor.visit_expr_mut(call);
            for selector in selected_arguments {
                visitor.visit_expr_mut(selector);
            }
        }
        HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
            visitor.visit_block_mut(block);
        }
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            visitor.visit_expr_mut(cond);
            visitor.visit_block_mut(then_block);
            if let Some(catch) = then_catch {
                visitor.visit_cape_mut(catch);
            }
            if let Some(else_block) = else_block {
                visitor.visit_block_mut(else_block);
            }
        }
        HirExpressionKind::Discerne {
            scrutinees, arms, ..
        } => {
            for scrutinee in scrutinees {
                visitor.visit_expr_mut(scrutinee);
            }
            for arm in arms {
                visitor.visit_casu_arm_mut(arm);
            }
        }
        HirExpressionKind::Dum(cond, block) => {
            visitor.visit_expr_mut(cond);
            visitor.visit_block_mut(block);
        }
        HirExpressionKind::Itera(_, coordinates, def_id, name, iter, block) => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            visitor.visit_def(*def_id, *name);
            visitor.visit_expr_mut(iter);
            visitor.visit_block_mut(block);
        }
        HirExpressionKind::IteraLockstep {
            sources,
            coordinates,
            bindings,
            body,
        } => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            for (binding_def_id, binding_name) in bindings {
                visitor.visit_def(*binding_def_id, *binding_name);
            }
            for source in sources {
                visitor.visit_expr_mut(source);
            }
            visitor.visit_block_mut(body);
        }
        HirExpressionKind::IteraRange {
            ranges,
            bindings,
            body,
        } => {
            // Ranges evaluate outside the binder scope, so they walk before
            // the binder defs (leftmost-outermost order preserved).
            for range in ranges {
                visitor.visit_expr_mut(range);
            }
            for (binding_def_id, binding_name) in bindings {
                visitor.visit_def(*binding_def_id, *binding_name);
            }
            visitor.visit_block_mut(body);
        }
        HirExpressionKind::Summa {
            source,
            coordinates,
            filum,
            binding,
            body,
        } => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            if let Some((filum_def_id, filum_name)) = filum {
                visitor.visit_def(*filum_def_id, *filum_name);
            }
            visitor.visit_def(binding.0, binding.1);
            visitor.visit_expr_mut(source);
            visitor.visit_block_mut(body);
        }
        HirExpressionKind::Extremum {
            source,
            coordinates,
            coalesce,
            ..
        } => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            visitor.visit_expr_mut(source);
            if let Some(identity) = coalesce {
                visitor.visit_expr_mut(identity);
            }
        }
        HirExpressionKind::FirstMatch {
            source,
            coordinates,
            binding,
            body,
        } => {
            for (coordinate_def_id, coordinate_name) in coordinates {
                visitor.visit_def(*coordinate_def_id, *coordinate_name);
            }
            visitor.visit_def(binding.0, binding.1);
            visitor.visit_expr_mut(source);
            visitor.visit_block_mut(body);
        }
        HirExpressionKind::Intervallum {
            start, end, step, ..
        } => {
            visitor.visit_expr_mut(start);
            visitor.visit_expr_mut(end);
            if let Some(step) = step {
                visitor.visit_expr_mut(step);
            }
        }
        HirExpressionKind::Array(elements) => {
            for element in elements {
                match element {
                    HirArrayElement::Expr(expr) | HirArrayElement::Spread(expr) => {
                        visitor.visit_expr_mut(expr);
                    }
                }
            }
        }
        HirExpressionKind::Struct(_, fields) => {
            for (_, value) in fields {
                visitor.visit_expr_mut(value);
            }
        }
        HirExpressionKind::Tuple(elements, _)
        | HirExpressionKind::Scribe(_, elements)
        | HirExpressionKind::Scriptum(_, elements) => {
            for element in elements {
                visitor.visit_expr_mut(element);
            }
        }
        HirExpressionKind::Adfirma(cond, message) => {
            visitor.visit_expr_mut(cond);
            if let Some(message) = message {
                visitor.visit_expr_mut(message);
            }
        }
        HirExpressionKind::Handled { body, catch } => {
            visitor.visit_block_mut(body);
            visitor.visit_cape_mut(catch);
        }
        HirExpressionKind::Trap(body) => {
            visitor.visit_block_mut(body);
        }
        HirExpressionKind::Clausura(params, _, _, body, _) => {
            for param in params {
                visitor.visit_param_mut(param);
            }
            visitor.visit_expr_mut(body);
        }
        HirExpressionKind::Verte {
            source, entries, ..
        } => {
            visitor.visit_expr_mut(source);
            if let Some(entries) = entries {
                for field in entries {
                    visitor.visit_object_field_mut(field);
                }
            }
        }
        HirExpressionKind::Conversio {
            source,
            target,
            recovery,
            ..
        } => {
            visitor.visit_expr_mut(source);
            if let HirConversioTarget::Intervallum(operand) = target {
                visitor.visit_expr_mut(operand);
            }
            if let Some(recovery) = recovery {
                visitor.visit_expr_mut(recovery);
            }
        }
        HirExpressionKind::Ad { opener, .. } => {
            if let Some(opener) = opener {
                visitor.visit_expr_mut(opener);
            }
        }
        HirExpressionKind::TypeCheck { expr, .. } => visitor.visit_expr_mut(expr),
    }
}

pub fn walk_cape_mut<V: HirVisitorMut>(visitor: &mut V, cape: &mut HirCape) {
    visitor.visit_def(cape.binding_def_id, cape.binding_name);
    visitor.visit_block_mut(&mut cape.body);
}

/// Mutably walk one pattern arm: patterns first, then guard, then body.
pub fn walk_casu_arm_mut<V: HirVisitorMut>(visitor: &mut V, arm: &mut HirCasuArm) {
    for pattern in &mut arm.patterns {
        visitor.visit_pattern_mut(pattern);
    }
    if let Some(guard) = &mut arm.guard {
        visitor.visit_expr_mut(guard);
    }
    visitor.visit_expr_mut(&mut arm.body);
}

pub fn walk_pattern_mut<V: HirVisitorMut>(visitor: &mut V, pattern: &mut HirPattern) {
    match pattern {
        HirPattern::Wildcard | HirPattern::Literal(_) => {}
        HirPattern::TypeTest { binding, .. } => {
            if let Some((def_id, name)) = binding {
                visitor.visit_def(*def_id, *name);
            }
        }
        HirPattern::Binding(def_id, name) => visitor.visit_def(*def_id, *name),
        HirPattern::Alias(def_id, name, pattern) => {
            visitor.visit_def(*def_id, *name);
            visitor.visit_pattern_mut(pattern);
        }
        HirPattern::Variant(_, patterns) => {
            for pattern in patterns {
                visitor.visit_pattern_mut(pattern);
            }
        }
    }
}

pub fn walk_object_field_mut<V: HirVisitorMut>(visitor: &mut V, field: &mut HirObjectField) {
    match &mut field.key {
        HirObjectKey::Ident(_) | HirObjectKey::String(_) => {}
        HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => visitor.visit_expr_mut(expr),
    }
    if let Some(value) = &mut field.value {
        visitor.visit_expr_mut(value);
    }
}

fn walk_optional_chain_mut<V: HirVisitorMut>(visitor: &mut V, chain: &mut HirOptionalChainKind) {
    match chain {
        HirOptionalChainKind::Member(_) => {}
        HirOptionalChainKind::Index(index) => visitor.visit_expr_mut(index),
        HirOptionalChainKind::Call(args) => walk_call_args_mut(visitor, args),
    }
}

fn walk_non_null_mut<V: HirVisitorMut>(visitor: &mut V, chain: &mut HirNonNullKind) {
    match chain {
        HirNonNullKind::Member(_) => {}
        HirNonNullKind::Index(index) => visitor.visit_expr_mut(index),
        HirNonNullKind::Call(args) => walk_call_args_mut(visitor, args),
    }
}

fn walk_call_args_mut<V: HirVisitorMut>(visitor: &mut V, args: &mut [HirCallArg]) {
    for arg in args {
        visitor.visit_expr_mut(&mut arg.expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::{
        HirAnnotation, HirAnnotationField, HirAnnotationValue, HirBlockEnd, HirExpression,
        HirExpressionKind, HirId, HirLiteral, HirSourceAnchorId,
    };
    use radix_lexer::{Span, Symbol};

    fn annotation_expr() -> HirAnnotation {
        HirAnnotation {
            family: Symbol(0),
            fields: vec![HirAnnotationField {
                name: Symbol(1),
                value: HirAnnotationValue::Expr(Box::new(HirExpression {
                    id: HirId(0),
                    kind: HirExpressionKind::Literal(HirLiteral::Int(42)),
                    ty: None,
                    span: Span::default(),
                })),
                span: Span::default(),
            }],
            brace_authored: true,
            raw_args: Vec::new(),
            contract_id: None,
            span: Span::default(),
        }
    }

    // PRP-016: read-only visitor must walk into annotation Expr payloads.
    struct ExprCollector {
        seen: Vec<u64>,
    }
    impl HirVisitor for ExprCollector {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Literal(HirLiteral::Int(n)) = &expr.kind {
                self.seen.push(*n);
            }
            walk_expr(self, expr);
        }
    }

    #[test]
    fn visitor_sees_annotation_expressions() {
        let ann = annotation_expr();
        let mut collector = ExprCollector { seen: Vec::new() };
        collector.visit_annotation(&ann);
        assert_eq!(
            collector.seen,
            vec![42],
            "visitor must see Expr in annotation"
        );
    }

    // PRP-016 F3: mutable visitor must also walk annotation Expr payloads.
    struct ExprMarker;
    impl HirVisitorMut for ExprMarker {
        fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
            if let HirExpressionKind::Literal(HirLiteral::Int(n)) = &mut expr.kind {
                *n = 99;
            }
            walk_expr_mut(self, expr);
        }
    }

    #[test]
    fn mut_visitor_sees_annotation_expressions() {
        let mut ann = annotation_expr();
        let mut marker = ExprMarker;
        marker.visit_annotation_mut(&mut ann);
        match &ann.fields[0].value {
            HirAnnotationValue::Expr(expr) => match &expr.kind {
                HirExpressionKind::Literal(HirLiteral::Int(n)) => {
                    assert_eq!(*n, 99, "mutable visitor must modify Expr in annotation");
                }
                _ => panic!("expected literal"),
            },
            _ => panic!("expected expr"),
        }
    }

    // PRP-016 integration: walk_item must drive envelope annotations through
    // visit_program, not just the standalone helper.
    fn program_with_annotated_function() -> HirModule {
        use crate::nodes::{HirItem, HirItemKind};
        let func = HirFunction {
            name: Symbol(0),
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: None,
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        };
        HirModule {
            items: vec![HirItem {
                annotations: vec![annotation_expr()],
                id: HirId(0),
                def_id: DefId(0),
                kind: HirItemKind::Function(Box::new(func)),
                span: Span::default(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        }
    }

    #[test]
    fn visit_program_drives_function_annotations() {
        let program = program_with_annotated_function();
        let mut collector = ExprCollector { seen: Vec::new() };
        collector.visit_program(&program);
        assert_eq!(
            collector.seen,
            vec![42],
            "visit_program must reach function annotation Expr"
        );
    }

    #[test]
    fn visit_program_mut_drives_function_annotations() {
        let mut program = program_with_annotated_function();
        let mut marker = ExprMarker;
        marker.visit_program_mut(&mut program);
        assert!(matches!(program.items[0].kind, HirItemKind::Function(_)));
        match &program.items[0].annotations[0].fields[0].value {
            HirAnnotationValue::Expr(expr) => match &expr.kind {
                HirExpressionKind::Literal(HirLiteral::Int(n)) => {
                    assert_eq!(
                        *n, 99,
                        "visit_program_mut must reach function annotation Expr"
                    );
                }
                _ => panic!("expected literal"),
            },
            _ => panic!("expected expr"),
        }
    }

    // PRP-075: walk_program/walk_program_mut must visit entry_annotations.
    fn program_with_entry_annotations() -> HirModule {
        HirModule {
            items: Vec::new(),
            entry: None,
            entry_annotations: vec![annotation_expr()],
            entry_args_name: None,
            entry_is_async: false,
        }
    }

    #[test]
    fn visit_program_drives_entry_annotations() {
        let program = program_with_entry_annotations();
        let mut collector = ExprCollector { seen: Vec::new() };
        collector.visit_program(&program);
        assert_eq!(
            collector.seen,
            vec![42],
            "visit_program must reach entry_annotation Expr",
        );
    }

    #[test]
    fn visit_program_mut_drives_entry_annotations() {
        let mut program = program_with_entry_annotations();
        let mut marker = ExprMarker;
        marker.visit_program_mut(&mut program);
        match &program.entry_annotations[0].fields[0].value {
            HirAnnotationValue::Expr(expr) => match &expr.kind {
                HirExpressionKind::Literal(HirLiteral::Int(n)) => {
                    assert_eq!(*n, 99, "visit_program_mut must reach entry_annotation Expr");
                }
                _ => panic!("expected literal"),
            },
            _ => panic!("expected expr"),
        }
    }

    // U-2 Stage 2: `HirBlock.end` is a structural field, not a statement.
    // A visitor counting statements must neither count nor recurse into it.
    #[derive(Default)]
    struct StatementCounter {
        statements: usize,
        blocks: usize,
    }
    impl HirVisitor for StatementCounter {
        fn visit_stmt(&mut self, stmt: &HirStatement) {
            self.statements += 1;
            walk_stmt(self, stmt);
        }
        fn visit_block(&mut self, block: &HirBlock) {
            self.blocks += 1;
            walk_block(self, block);
        }
    }

    #[test]
    fn visitor_does_not_count_block_ends_as_statements() {
        let entry = HirBlock {
            statements: vec![HirStatement {
                id: HirId(1),
                kind: HirStatementKind::Tacet,
                span: Span::default(),
            }],
            expr: None,
            span: Span::default(),
            breakable: None,
            end: Some(HirBlockEnd {
                id: HirSourceAnchorId(7),
                span: Span::default(),
            }),
        };
        let program = HirModule {
            items: Vec::new(),
            entry: Some(entry),
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };
        let mut counter = StatementCounter::default();
        counter.visit_program(&program);
        assert_eq!(
            counter.statements, 1,
            "the block end must not be visited as a statement"
        );
        assert_eq!(counter.blocks, 1, "the entry block is visited once");
    }

    // Traversal symmetry contract: `Praefixum` wraps a real inner expression
    // subtree, so both walkers must recurse into it. The artifact
    // collector/validator mirrors this shape (artifact collector↔validator↔
    // visitor symmetry).
    fn praefixum_expr(inner_id: HirId) -> HirExpression {
        HirExpression {
            id: HirId(0),
            kind: HirExpressionKind::Praefixum(Box::new(HirExpression {
                id: inner_id,
                kind: HirExpressionKind::Literal(HirLiteral::Int(7)),
                ty: None,
                span: Span::default(),
            })),
            ty: None,
            span: Span::default(),
        }
    }

    #[test]
    fn visitor_recurses_into_praefixum_operand() {
        let expr = praefixum_expr(HirId(5));
        let mut collector = ExprCollector { seen: Vec::new() };
        collector.visit_expr(&expr);
        assert_eq!(
            collector.seen,
            vec![7],
            "visit_expr must recurse into the Praefixum operand"
        );
    }

    #[test]
    fn mut_visitor_recurses_into_praefixum_operand() {
        let mut expr = praefixum_expr(HirId(5));
        let mut marker = ExprMarker;
        marker.visit_expr_mut(&mut expr);
        let HirExpressionKind::Praefixum(inner) = &expr.kind else {
            panic!("expected praefixum");
        };
        let HirExpressionKind::Literal(HirLiteral::Int(n)) = &inner.kind else {
            panic!("expected literal");
        };
        assert_eq!(*n, 99, "visit_expr_mut must reach the Praefixum operand");
    }

    // Type-alias generic parameters introduce DefIds, same as struct/enum/interface.
    fn program_with_generic_type_alias() -> HirModule {
        use crate::nodes::{
            HirItem, HirItemKind, HirTypeAlias, HirTypeParam, HirTypeParamConstraint,
        };
        use radix_syntax::GenericParamKind;
        use radix_types::TypeId;
        HirModule {
            items: vec![HirItem {
                annotations: Vec::new(),
                id: HirId(0),
                def_id: DefId(1),
                kind: HirItemKind::TypeAlias(HirTypeAlias {
                    name: Symbol(0),
                    type_params: vec![HirTypeParam {
                        def_id: DefId(2),
                        kind: GenericParamKind::Typus,
                        name: Symbol(3),
                        span: Span::default(),
                        constraint: HirTypeParamConstraint::Any,
                        default: None,
                    }],
                    ty: TypeId(0),
                }),
                span: Span::default(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        }
    }

    #[derive(Default)]
    struct DefCollector {
        defs: Vec<u32>,
    }
    impl HirVisitor for DefCollector {
        fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
            self.defs.push(def_id.0);
        }
    }
    impl HirVisitorMut for DefCollector {
        fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
            self.defs.push(def_id.0);
        }
    }

    #[test]
    fn visit_program_reports_type_alias_type_params() {
        let program = program_with_generic_type_alias();
        let mut collector = DefCollector::default();
        collector.visit_program(&program);
        assert_eq!(
            collector.defs,
            vec![1, 2],
            "visit_program must report the alias, then its type parameter"
        );
    }

    #[test]
    fn visit_program_mut_reports_type_alias_type_params() {
        let mut program = program_with_generic_type_alias();
        let mut collector = DefCollector::default();
        collector.visit_program_mut(&mut program);
        assert_eq!(
            collector.defs,
            vec![1, 2],
            "visit_program_mut must report the alias, then its type parameter"
        );
    }
}
