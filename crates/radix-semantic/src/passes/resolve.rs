//! AST name resolution and type-alias lowering for the semantic front end.
//!
//! `resolve` is the second AST-phase semantic pass. It runs after
//! [collect](super::collect) has populated the resolver with top-level
//! declaration identities and before HIR lowering tries to encode semantic
//! identities into lowered nodes. This pass validates lexical availability of
//! names and types, creates nested scopes for local bindings, checks
//! return/loop-control placement, and resolves type aliases into the shared
//! [`TypeTable`].
//!
//! BOUNDARY
//! ========
//! The pass is deliberately pre-HIR and non-inferential. It can prove that a
//! name exists and that a type name refers to a type-like symbol, but it does
//! not assign expression types, validate assignability, run borrow checks, or
//! decide target-specific warning policy. Those responsibilities belong to HIR
//! lowering, typecheck, and later analysis passes.
//!
//! INVARIANTS
//! ==========
//! - Global declarations already exist in the resolver when this pass starts.
//! - Scopes are entered around functions, blocks, loops, matches, and module-ish
//!   declaration bodies so lookup follows lexical containment.
//! - Variable declarations resolve their annotation and initializer before the
//!   declared binding enters the current scope; parameter and pattern bindings
//!   enter the scope that owns the body they make available.
//! - `redde`, `rumpe`, and `perge` placement is checked from the scope stack.
//! - Type aliases are the only AST type expressions lowered here; their
//!   resulting `TypeId`s are stored on resolver symbols for later HIR/typecheck
//!   use.
//!
//! TYPE-ALIAS FIXPOINT
//! ===================
//! Aliases can reference earlier or later aliases, so this file lowers them
//! with a small fixed-point loop. An alias whose dependency is not yet typed is
//! deferred; an iteration with no progress reports the remaining aliases as a
//! circular dependency. This keeps cycle handling local to the one pre-HIR type
//! lowering responsibility resolve owns.
//!
//! ERROR STRATEGY
//! ==============
//! Resolution accumulates hard semantic errors and lets the driver stop before
//! HIR lowering. Warning/error policy for target-sensitive analysis is handled
//! by the later HIR passes and driver.

use crate::instans_precision::InstansPrecisionError;
use crate::numeric_width::WidthLowerIssue;
use crate::{
    BareVariantLookup, CollectionKind, FuncSig, GenericParamDescriptor, GenericParamSort,
    IndexExpr, ParamType, Primitive, QualifiedVariantPathLookup, Resolver, SchemaColumnDef,
    SchemaDef, ScopeKind, SemanticError, SemanticErrorKind, SemanticGenericParamDefault,
    SemanticParamMode, Symbol, SymbolKind, Type, TypeId, TypeTable,
};
use radix_hir::DefId;
use radix_lexer::Interner;
use radix_syntax::{
    AssignOp, BindingPattern, BlockStmt, CapeClause, ClausuraBody, ConversioTarget, DiscerneStmt,
    Expr, ExprKind, FiguraExpr, GenericArgExpr, IfBody, Pattern, PatternBind, ProbandumDecl,
    Program, SiStmt, Stmt, StmtKind, TypeExpr, TypeExprKind, TypeParam,
};
use radix_types::AppliedArg;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

const ISSUE_DUPLICATE_DEFINITION: &str = "duplicate_definition";
const ISSUE_EMPTY_QUALIFIED_TYPE_PATH: &str = "empty_qualified_type_path";
const ISSUE_EMPTY_UNION_TYPE: &str = "empty_union_type";
const ISSUE_EMPTY_INTERSECTION_TYPE: &str = "empty_intersection_type";
const ISSUE_INTERSECTION_PRIMITIVE_OPERAND: &str = "intersection_primitive_operand";
const ISSUE_ATOMIC_ARITY: &str = "atomic_arity";
const ISSUE_ATOMIC_ELEMENT: &str = "atomic_element";
const ISSUE_ATOMIC_INCOMPLETE: &str = "atomic_incomplete";
const ISSUE_FIGURA_UNKNOWN_INDEX: &str = "figura_unknown_index";
const ISSUE_FIGURA_TYPE_PARAM_IN_INDEX: &str = "figura_type_param_in_index";
const ISSUE_FIGURA_ARITHMETIC_INVALID: &str = "figura_arithmetic_invalid";
const ISSUE_INVALID_PRECISION_MARKER: &str = "invalid_precision_marker";
const ISSUE_INTERVALLUM_ARITY: &str = "intervallum_arity";
const ISSUE_INTERVALLUM_BOUND: &str = "intervallum_bound";
const ISSUE_INTERVALLUM_STEP_MUST_BE_CONSTANT: &str = "intervallum_step_must_be_constant";
const ISSUE_INTERVALLUM_STEP_BOUND_NOT_INTEGER: &str = "intervallum_step_bound_not_integer";
const ISSUE_MATRIX_INCOMPLETE: &str = "matrix_incomplete";
const ISSUE_MATRIX_MISSING_FIGURA: &str = "matrix_missing_figura";
const ISSUE_MATRIX_SHAPE_TWO_DIM: &str = "matrix_shape_two_dim";
const ISSUE_MISSING_TYPE_ALIAS_SYMBOL: &str = "missing_type_alias_symbol";
const ISSUE_MISSING_TYPE_SYMBOL_INFO: &str = "missing_type_symbol_info";
const ISSUE_NAME_NOT_VARIANT: &str = "name_not_variant";
const ISSUE_NAME_NOT_TYPE: &str = "name_not_type";
const ISSUE_PRECISION_MARKER_PARAMS: &str = "precision_marker_params";
const ISSUE_PRECISION_MARKER_SHAPE: &str = "precision_marker_shape";
const ISSUE_PRIMITIVE_PARAMS: &str = "primitive_params";
const ISSUE_ASCII_FORMAT_HINT_NOT_CAPACITY: &str = "ascii_format_hint_not_capacity";
const ISSUE_BOUNDED_CAPACITY_NOT_NATURAL: &str = "bounded_capacity_not_natural";
const ISSUE_QUALIFIED_TYPE_NOT_EXPORTED: &str = "qualified_type_not_exported";
const ISSUE_QUALIFIED_TYPE_MIDDLE_NOT_NAMESPACE: &str = "qualified_type_middle_not_namespace";
const ISSUE_QUALIFIED_TYPE_PREFIX_NOT_NAMESPACE: &str = "qualified_type_prefix_not_namespace";
const ISSUE_FIGURA_NOT_INDEX_PARAM: &str = "figura_not_index_param";
const ISSUE_SCRINIUM_ARITY: &str = "scrinium_arity";
const ISSUE_SERMO_ARITY: &str = "sermo_arity";
const ISSUE_SERMO_MISSING_BUILTIN: &str = "sermo_missing_builtin";
const ISSUE_SCRINIUM_MISSING_BUILTIN: &str = "scrinium_missing_builtin";
const ISSUE_SERIES_PARAMS: &str = "series_params";
const ISSUE_SCHEMA_DUPLICATE_VISIBLE: &str = "schema_duplicate_visible";
const ISSUE_SCHEMA_DUPLICATE_SOURCE: &str = "schema_duplicate_source";
const ISSUE_SCHEMA_UNKNOWN_TYPE: &str = "schema_unknown_type";
const ISSUE_SCHEMA_NESTED_COLUMN: &str = "schema_nested_column";
const ISSUE_SPARSA_ELEMENT_NON_NUMERIC: &str = "sparsa_element_non_numeric";
const ISSUE_SPARSA_INCOMPLETE: &str = "sparsa_incomplete";
const ISSUE_SPARSA_MISSING_FIGURA: &str = "sparsa_missing_figura";
const ISSUE_TENSOR_INCOMPLETE: &str = "tensor_incomplete";
const ISSUE_TENSOR_MISSING_FIGURA: &str = "tensor_missing_figura";
const ISSUE_TYPE_ALIAS_CYCLE: &str = "type_alias_cycle";
const ISSUE_TYPE_NAME_NOT_TYPE: &str = "type_name_not_type";
const ISSUE_VARIANT_AMBIGUOUS: &str = "variant_ambiguous";
const ISSUE_DISCERNE_LITERAL_ARM_NOT_IN_CLOSED_SET: &str = "discerne_literal_arm_not_in_closed_set";
const ISSUE_DISCERNE_ARM_NOT_MEMBER: &str = "discerne_arm_not_member";
const ISSUE_UNION_ONLY_NIHIL: &str = "union_only_nihil";
/// `x est V` where `V` names a value, not a type (DECISIONS D23.1).
const ISSUE_EST_VALUE_RHS: &str = "est_value_rhs";
const ISSUE_UNKNOWN_TYPE_LOWERING: &str = "unknown_type_lowering_error";
const ISSUE_NAME_NOT_VALUE: &str = "name_not_value";
const ISSUE_NAMESPACE_MISSING_EXPORT: &str = "namespace_missing_export";
const ISSUE_SCRIBE_CALLEE: &str = "scribe_callee";
const ISSUE_SELECTIVE_IMPORT_MISSING_MEMBER: &str = "selective_import_missing_member";
const ISSUE_UNKNOWN_IDENTIFIER: &str = "unknown_identifier";
const ISSUE_UNKNOWN_NAMESPACE: &str = "unknown_namespace";
const ISSUE_UNKNOWN_QUALIFIED_TYPE: &str = "unknown_qualified_type";
const ISSUE_UNKNOWN_TYPE_NAME: &str = "unknown_type_name";
const ISSUE_UNKNOWN_TYPE: &str = "unknown_type";
const ISSUE_UNKNOWN_VARIANT: &str = "unknown_variant";
const ISSUE_VECTOR_INCOMPLETE: &str = "vector_incomplete";
const ISSUE_VECTOR_MISSING_WIDTH: &str = "vector_missing_width";
const ISSUE_VECTOR_WIDTH_SINGLE_DIM: &str = "vector_width_single_dim";

/// Resolve AST-level names and type aliases after declaration collection.
///
/// The resulting resolver contains the lexical scope tree and any type-alias
/// `TypeId`s needed by lowering/typecheck. Errors are accumulated so a single
/// source file can report several undefined names, invalid control-flow forms,
/// or alias cycles in one compiler run.
///
/// # Errors
///
/// Returns undefined-name, duplicate-binding, and alias-cycle errors as a list
/// of [`SemanticError`]s.
pub fn resolve(
    program: &Program,
    resolver: &mut Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<(), Vec<SemanticError>> {
    let mut errors = Vec::new();
    let mut aliases = Vec::new();

    // Bind ordered parameter kinds to declaration identities before any named
    // application is lowered. This is the resolver/HIR identity seam; source
    // spelling is never consulted for kind selection.
    for stmt in &program.statements {
        let (name, params) = match &stmt.kind {
            StmtKind::TypeAlias(decl) => (&decl.name, &decl.type_params),
            StmtKind::Class(decl) => (&decl.name, &decl.type_params),
            StmtKind::Interface(decl) => (&decl.name, &decl.type_params),
            StmtKind::Union(decl) => (&decl.name, &decl.type_params),
            _ => continue,
        };
        if let Some(def_id) = resolver.lookup(name.name) {
            resolver.register_generic_param_kinds(def_id, params.iter().map(|param| param.kind));
        }
    }

    // Gather alias declarations before resolving bodies so later alias lowering
    // can run against the full set of collected symbols.
    for stmt in &program.statements {
        if let StmtKind::TypeAlias(decl) = &stmt.kind {
            let Some(def_id) = resolver.lookup(decl.name.name) else {
                errors.push(lowering_error(ISSUE_MISSING_TYPE_ALIAS_SYMBOL, stmt.span));
                continue;
            };
            aliases.push(AliasEntry {
                def_id,
                type_params: &decl.type_params,
                ty: &decl.ty,
                span: stmt.span,
            });
        }
    }

    // Selective bindings resolve first: a binding that names an imported
    // type (D22.5) must be visible to the type positions resolved below.
    resolve_selective_imports(program, resolver, interner, types, &mut errors);
    for stmt in &program.statements {
        resolve_stmt(resolver, interner, stmt, &mut errors);
    }

    resolve_alias_types(&aliases, resolver, interner, types, &mut errors);
    register_named_generic_descriptors(program, resolver, interner, types, &mut errors);
    resolve_schema_decls(program, resolver, interner, types, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn resolve_stmt(
    resolver: &mut Resolver,
    interner: &Interner,
    stmt: &Stmt,
    errors: &mut Vec<SemanticError>,
) {
    let stmt_span = stmt.span;
    match &stmt.kind {
        StmtKind::SizeDecl(decl) => {
            // A lifted size is immutable and snapshots its source expression
            // before its own name enters scope. A size is a natural, so a
            // negative literal fails the lift store at compile time; runtime
            // negatives are checked at the store in MIR (size-type Z1).
            resolve_expr(resolver, interner, &decl.init, errors);
            if let radix_syntax::ExprKind::Unary(unary) = &decl.init.kind
                && unary.op == radix_syntax::UnOp::Neg
                && matches!(unary.operand.kind, radix_syntax::ExprKind::Literal(..))
            {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::ConversionInvalid, decl.init.span)
                        .with_arg("issue", "size_lift_negative"),
                );
            }
            if !matches!(resolver.current().kind, ScopeKind::Global) {
                define_symbol(
                    resolver,
                    decl.name.name,
                    decl.name.span,
                    if decl.is_static {
                        SymbolKind::IndexParam
                    } else {
                        SymbolKind::Local
                    },
                    false,
                    errors,
                );
            }
        }
        StmtKind::Var(decl) => {
            if let Some(ty) = &decl.ty {
                resolve_type_expr(resolver, interner, ty, errors);
            }
            let local_scope = !matches!(resolver.current().kind, ScopeKind::Global);
            // A typed `↤` initializer has the binding being declared as its own
            // assignment target (the parser synthesized it), so the binding
            // must be in scope while the initializer resolves. Define it up
            // front and skip the post-init definition so one def_id serves
            // both the target and the declaration.
            let conversio_assign_init = decl.init.as_ref().is_some_and(|init| {
                matches!(
                    &init.kind,
                    ExprKind::Assign(assign) if matches!(assign.op, AssignOp::ConversioAssign)
                )
            });
            if conversio_assign_init && local_scope {
                define_binding_pattern(
                    resolver,
                    &decl.binding,
                    decl.mutability == radix_syntax::AstMutability::Mutable,
                    errors,
                );
                record_iuncta_labels_for_var(resolver, interner, decl);
            }
            if let Some(init) = &decl.init {
                resolve_expr(resolver, interner, init, errors);
            }
            if local_scope && !conversio_assign_init {
                define_binding_pattern(
                    resolver,
                    &decl.binding,
                    decl.mutability == radix_syntax::AstMutability::Mutable,
                    errors,
                );
                record_iuncta_labels_for_var(resolver, interner, decl);
            }
        }
        StmtKind::Func(decl) => {
            resolver.enter_scope(ScopeKind::Function);
            for param in &decl.type_params {
                define_generic_param(resolver, param, errors);
            }
            resolve_generic_bounds(resolver, interner, &decl.type_params, errors);
            for param in &decl.params {
                resolve_type_expr(resolver, interner, &param.ty, errors);
                let binding_name = param.alias.as_ref().unwrap_or(&param.name);
                define_symbol(
                    resolver,
                    binding_name.name,
                    binding_name.span,
                    SymbolKind::Param,
                    param.mode == radix_syntax::AstParamMode::In
                        || param.ty.mode == Some(radix_syntax::TypeMode::In),
                    errors,
                );
                if let Some(def_id) = resolver.lookup(binding_name.name) {
                    crate::tuple_labels::record_from_type_expr(
                        resolver, interner, def_id, &param.ty,
                    );
                    record_subject_member_set(resolver, interner, def_id, &param.ty);
                }
                if let Some(default) = &param.default {
                    resolve_expr(resolver, interner, default, errors);
                }
            }
            for ident in modifier_bindings(&decl.modifiers) {
                define_symbol(
                    resolver,
                    ident.name,
                    ident.span,
                    SymbolKind::Local,
                    true,
                    errors,
                );
            }
            define_shader_stage_locals_for_resolution(
                resolver,
                interner,
                &stmt.annotations,
                stmt_span,
                errors,
            );
            if let Some(ret) = &decl.ret {
                resolve_type_expr(resolver, interner, ret, errors);
            }
            if let Some(body) = &decl.body {
                resolve_block(resolver, interner, body, errors);
            }
            resolver.exit_scope();
        }
        StmtKind::Class(decl) => {
            resolver.enter_scope(ScopeKind::Module);
            for param in &decl.type_params {
                define_generic_param(resolver, param, errors);
            }
            for contract in &decl.implements {
                resolve_contract_ref(resolver, interner, contract, errors);
            }
            for member in &decl.members {
                match &member.kind {
                    radix_syntax::ClassMemberKind::Field(field) => {
                        resolve_type_expr(resolver, interner, &field.ty, errors);
                        if let Some(init) = &field.init {
                            resolve_expr(resolver, interner, init, errors);
                        }
                    }
                    radix_syntax::ClassMemberKind::Method(method) => {
                        resolver.enter_scope(ScopeKind::Function);
                        for param in &method.type_params {
                            define_generic_param(resolver, param, errors);
                        }
                        resolve_generic_bounds(resolver, interner, &method.type_params, errors);
                        for param in &method.params {
                            resolve_type_expr(resolver, interner, &param.ty, errors);
                            let binding_name = param.alias.as_ref().unwrap_or(&param.name);
                            define_symbol(
                                resolver,
                                binding_name.name,
                                binding_name.span,
                                SymbolKind::Param,
                                param.mode == radix_syntax::AstParamMode::In
                                    || param.ty.mode == Some(radix_syntax::TypeMode::In),
                                errors,
                            );
                            record_iuncta_labels_for_name(
                                resolver,
                                interner,
                                binding_name.name,
                                &param.ty,
                            );
                            if let Some(default) = &param.default {
                                resolve_expr(resolver, interner, default, errors);
                            }
                        }
                        for ident in modifier_bindings(&method.modifiers) {
                            define_symbol(
                                resolver,
                                ident.name,
                                ident.span,
                                SymbolKind::Local,
                                true,
                                errors,
                            );
                        }
                        if let Some(ret) = &method.ret {
                            resolve_type_expr(resolver, interner, ret, errors);
                        }
                        if let Some(body) = &method.body {
                            resolve_block(resolver, interner, body, errors);
                        }
                        resolver.exit_scope();
                    }
                }
            }
            resolver.exit_scope();
        }
        StmtKind::Interface(decl) => {
            resolver.enter_scope(ScopeKind::Module);
            for param in &decl.type_params {
                define_generic_param(resolver, param, errors);
            }
            for method in &decl.methods {
                resolver.enter_scope(ScopeKind::Function);
                for param in &method.params {
                    resolve_type_expr(resolver, interner, &param.ty, errors);
                    define_symbol(
                        resolver,
                        param.name.name,
                        param.name.span,
                        SymbolKind::Param,
                        param.mode == radix_syntax::AstParamMode::In
                            || param.ty.mode == Some(radix_syntax::TypeMode::In),
                        errors,
                    );
                    record_iuncta_labels_for_name(resolver, interner, param.name.name, &param.ty);
                }
                if let Some(ret) = &method.ret {
                    resolve_type_expr(resolver, interner, ret, errors);
                }
                resolver.exit_scope();
            }
            resolver.exit_scope();
        }
        StmtKind::TypeAlias(decl) => {
            resolver.enter_scope(ScopeKind::Module);
            for param in &decl.type_params {
                define_generic_param(resolver, param, errors);
            }
            resolve_type_expr(resolver, interner, &decl.ty, errors);
            resolver.exit_scope();
        }
        // Relational schema declarations are resolved by `resolve_schema_decls`
        // (after alias lowering so column types may reference aliases): a
        // schema introduces no value bindings or local scopes for this pass,
        // and its ordered column metadata is type-level only.
        StmtKind::Schema(_decl) => {}
        StmtKind::Enum(_) | StmtKind::Tacet(_) => {}
        StmtKind::Union(decl) => {
            resolver.enter_scope(ScopeKind::Module);
            for param in &decl.type_params {
                define_generic_param(resolver, param, errors);
            }
            for variant in &decl.variants {
                for field in &variant.fields {
                    resolve_type_expr(resolver, interner, &field.ty, errors);
                }
            }
            resolver.exit_scope();
        }
        StmtKind::Import(_decl) => {}
        StmtKind::Block(block) => resolve_block(resolver, interner, block, errors),
        StmtKind::Expr(expr) => resolve_expr(resolver, interner, &expr.expr, errors),
        StmtKind::IncDec(stmt) => resolve_expr(resolver, interner, &stmt.target, errors),
        StmtKind::Si(stmt) => resolve_si_stmt(resolver, interner, stmt, errors),
        StmtKind::Dum(stmt) => {
            resolve_expr(resolver, interner, &stmt.cond, errors);
            resolver.enter_scope(ScopeKind::Loop);
            resolve_if_body(resolver, interner, &stmt.body, errors);
            resolver.exit_scope();
            if let Some(catch) = &stmt.catch {
                resolve_catch_binding(resolver, interner, catch, errors);
            }
        }
        StmtKind::Itera(stmt) => {
            let enum_source = !matches!(stmt.mode, radix_syntax::IteraMode::Ab);
            for source in &stmt.sources {
                // An `ordo` name in source position names the enum's own
                // member series, not a value, so the
                // type-kind-name expression rejection below must not fire for
                // it. Every other type-kind name in expression position still
                // rejects, and typecheck keeps `de` over an enum rejected.
                if enum_source && is_enum_iteration_source(resolver, source) {
                    continue;
                }
                resolve_expr(resolver, interner, source, errors);
            }
            resolver.enter_scope(ScopeKind::Loop);
            for ident in &stmt.apud {
                define_symbol(
                    resolver,
                    ident.name,
                    ident.span,
                    SymbolKind::Local,
                    false,
                    errors,
                );
            }
            for binding in &stmt.bindings {
                define_binding_pattern(
                    resolver,
                    binding,
                    stmt.mutability == radix_syntax::AstMutability::Mutable,
                    errors,
                );
            }
            resolve_if_body(resolver, interner, &stmt.body, errors);
            resolver.exit_scope();
            if let Some(catch) = &stmt.catch {
                resolve_catch_binding(resolver, interner, catch, errors);
            }
        }
        StmtKind::Elige(stmt) => {
            resolve_expr(resolver, interner, &stmt.expr, errors);
            let subject_members = subject_member_set(resolver, &stmt.expr);
            for case in &stmt.cases {
                resolve_elige_case_value(
                    resolver,
                    interner,
                    &case.value,
                    subject_members.as_deref(),
                    errors,
                );
                resolve_if_body(resolver, interner, &case.body, errors);
            }
            if let Some(default) = &stmt.default {
                resolve_if_body(resolver, interner, &default.body, errors);
            }
            if let Some(catch) = &stmt.catch {
                resolve_catch_binding(resolver, interner, catch, errors);
            }
        }
        StmtKind::Discerne(stmt) => resolve_discerne(resolver, interner, stmt, errors),
        StmtKind::Custodi(stmt) => {
            for clause in &stmt.clauses {
                resolve_expr(resolver, interner, &clause.cond, errors);
                resolve_if_body(resolver, interner, &clause.body, errors);
            }
        }
        StmtKind::Fac(stmt) => {
            resolver.enter_scope(fac_scope_kind(stmt));
            resolve_block(resolver, interner, &stmt.body, errors);
            resolver.exit_scope();
            if let Some(catch) = &stmt.catch {
                resolve_catch_binding(resolver, interner, catch, errors);
            }
            if let Some(cond) = &stmt.while_ {
                resolve_expr(resolver, interner, cond, errors);
            }
        }
        StmtKind::Redde(stmt) => {
            if !resolver.in_function() {
                errors.push(
                    SemanticError::structured(
                        SemanticErrorKind::ReturnOutsideFunction,
                        stmt.value.as_ref().map_or(stmt_span, |value| value.span),
                    )
                    .with_arg("issue", "return_outside_function")
                    .with_arg("keyword", "redde")
                    .with_arg("context", "function"),
                );
            }
            if let Some(value) = &stmt.value {
                resolve_expr(resolver, interner, value, errors);
            }
        }
        StmtKind::ReddeFilum(stmt) => {
            if let Some(value) = &stmt.value {
                resolve_expr(resolver, interner, value, errors);
            }
        }
        StmtKind::Rumpe(stmt) => {
            if !resolver.in_breakable() {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::BreakOutsideBreakable, stmt.span)
                        .with_arg("issue", "break_outside_breakable")
                        .with_arg("keyword", "rumpe")
                        .with_arg("context", "breakable block"),
                );
            }
        }
        StmtKind::Perge(stmt) => {
            if !resolver.in_loop() {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::ContinueOutsideLoop, stmt.span)
                        .with_arg("issue", "continue_outside_loop")
                        .with_arg("keyword", "perge")
                        .with_arg("context", "loop"),
                );
            }
        }
        StmtKind::Iace(stmt) => resolve_expr(resolver, interner, &stmt.value, errors),
        StmtKind::Mori(stmt) => resolve_expr(resolver, interner, &stmt.value, errors),
        StmtKind::Reddet(stmt) => resolve_expr(resolver, interner, &stmt.future, errors),
        StmtKind::Tacebit(stmt) => resolve_expr(resolver, interner, &stmt.future, errors),
        StmtKind::Adfirma(stmt) => {
            resolve_expr(resolver, interner, &stmt.cond, errors);
            if let Some(message) = &stmt.message {
                resolve_expr(resolver, interner, message, errors);
            }
        }
        StmtKind::Require(stmt) => {
            resolve_expr(resolver, interner, &stmt.cond, errors);
            resolve_expr(resolver, interner, &stmt.error, errors);
        }
        StmtKind::Reject(stmt) => {
            resolve_expr(resolver, interner, &stmt.cond, errors);
            resolve_expr(resolver, interner, &stmt.error, errors);
        }
        StmtKind::Scribe(stmt) => {
            for arg in &stmt.args {
                resolve_expr(resolver, interner, arg, errors);
            }
        }
        StmtKind::Incipit(stmt) => {
            resolver.enter_scope(ScopeKind::Function);
            if let Some(args) = &stmt.args {
                define_symbol(
                    resolver,
                    args.name,
                    args.span,
                    SymbolKind::Param,
                    false,
                    errors,
                );
            }
            if let Some(exitus) = &stmt.exitus {
                resolve_expr(resolver, interner, exitus, errors);
            }
            resolve_if_body(resolver, interner, &stmt.body, errors);
            resolver.exit_scope();
        }
        StmtKind::Probandum(test) => resolve_probandum(resolver, interner, test, errors),
        StmtKind::Proba(case) => {
            resolve_block(resolver, interner, &case.body, errors);
        }
        StmtKind::Ex(stmt) => {
            resolve_expr(resolver, interner, &stmt.source, errors);
            for field in &stmt.fields {
                if let Some(alias) = &field.alias {
                    define_symbol(
                        resolver,
                        alias.name,
                        alias.span,
                        SymbolKind::Local,
                        stmt.mutability == radix_syntax::AstMutability::Mutable,
                        errors,
                    );
                } else {
                    define_symbol(
                        resolver,
                        field.name.name,
                        field.name.span,
                        SymbolKind::Local,
                        stmt.mutability == radix_syntax::AstMutability::Mutable,
                        errors,
                    );
                }
            }
            if let Some(rest) = &stmt.is_rest {
                define_symbol(
                    resolver,
                    rest.name,
                    rest.span,
                    SymbolKind::Local,
                    stmt.mutability == radix_syntax::AstMutability::Mutable,
                    errors,
                );
            }
        }
    }
}

/// Resolve each selective import member against the imported file interface.
///
/// A selective import binds no namespace: the driver installs the target
/// interface under the import path, and each member's complete function or
/// const value type lands on the caller-owned immutable local `DefId`
/// collected for the binding. The exported spelling resolves through the
/// interface's locale/canonical projection; the local alias is caller text
/// and is never translated. Without an installed interface the binding stays
/// untyped, matching namespace imports of unresolved libraries.
fn resolve_selective_imports(
    program: &Program,
    resolver: &mut Resolver,
    interner: &Interner,
    types: &mut TypeTable,
    errors: &mut Vec<SemanticError>,
) {
    for stmt in &program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let radix_syntax::ImportKind::Selective { members } = &decl.kind else {
            continue;
        };
        if !resolver.namespace_has_file_interface(decl.path) {
            // No installed interface for the path: the member cannot be
            // verified (same leniency as a namespace import of an unresolved
            // library). Bind `ignotum` so use sites stay checkable instead of
            // poisoning every later expression with unknown_identifier.
            let ignotum = types.primitive(Primitive::Ignotum);
            for member in members {
                let binding = member.alias.as_ref().unwrap_or(&member.name);
                let Some(def_id) = resolver.lookup(binding.name) else {
                    continue;
                };
                if let Some(symbol) = resolver.get_symbol_mut(def_id) {
                    symbol.ty = Some(ignotum);
                }
            }
            continue;
        }
        for member in members {
            let binding = member.alias.as_ref().unwrap_or(&member.name);
            let Some(def_id) = resolver.lookup(binding.name) else {
                continue;
            };
            let exported = member.name.name;
            if let Some(sig) = resolver.namespace_file_function(decl.path, exported) {
                let ty = types.function(sig.clone());
                let gates = resolver
                    .namespace_file_unavailable(decl.path, exported)
                    .map(<[_]>::to_vec);
                if let Some(symbol) = resolver.get_symbol_mut(def_id) {
                    symbol.ty = Some(ty);
                }
                if let Some(gates) = gates {
                    resolver.bind_unavailable_import(def_id, gates);
                }
            } else if let Some(ty) = resolver.namespace_file_const(decl.path, exported) {
                if let Some(symbol) = resolver.get_symbol_mut(def_id) {
                    symbol.ty = Some(ty);
                }
            } else if resolver.namespace_file_type(decl.path, exported).is_some() {
                resolver.bind_selective_type_import(def_id, decl.path, exported);
            } else {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::ImportAbsent, member.span)
                        .with_arg("issue", ISSUE_SELECTIVE_IMPORT_MISSING_MEMBER)
                        .with_arg("member", interner.resolve(exported))
                        .with_arg("path", interner.resolve(decl.path)),
                );
            }
        }
    }
}

fn resolve_block(
    resolver: &mut Resolver,
    interner: &Interner,
    block: &BlockStmt,
    errors: &mut Vec<SemanticError>,
) {
    resolver.enter_scope(ScopeKind::Block);
    for stmt in &block.statements {
        resolve_stmt(resolver, interner, stmt, errors);
    }
    resolver.exit_scope();
}

/// Scope kind for a `fac` body: `fac … dum` repeats, so it is a loop; a
/// plain `fac` block (with or without `cape`) is breakable only, so `rumpe`
/// stops there and `perge` passes through to the enclosing loop (ruling R3).
fn fac_scope_kind(stmt: &radix_syntax::FacStmt) -> ScopeKind {
    if stmt.while_.is_some() {
        ScopeKind::Loop
    } else {
        ScopeKind::Breakable
    }
}

/// Resolve one `cape` clause: bind its error local in a fresh block scope and
/// resolve the handler body.
fn resolve_catch_binding(
    resolver: &mut Resolver,
    interner: &Interner,
    catch: &CapeClause,
    errors: &mut Vec<SemanticError>,
) {
    resolver.enter_scope(ScopeKind::Block);
    define_symbol(
        resolver,
        catch.binding.name,
        catch.binding.span,
        SymbolKind::Local,
        false,
        errors,
    );
    resolve_block(resolver, interner, &catch.body, errors);
    resolver.exit_scope();
}

fn resolve_if_body(
    resolver: &mut Resolver,
    interner: &Interner,
    body: &IfBody,
    errors: &mut Vec<SemanticError>,
) {
    match body {
        IfBody::Block(block) => resolve_block(resolver, interner, block, errors),
        // `ergo` bodies lower to a one-arm block (`lower_ergo_body`), so resolve
        // them in a nested block scope too: bindings must not leak into the
        // enclosing scope and local shadowing must match lowering.
        IfBody::Ergo(stmt) => {
            resolver.enter_scope(ScopeKind::Block);
            resolve_stmt(resolver, interner, stmt, errors);
            resolver.exit_scope();
        }
    }
}

fn resolve_si_stmt(
    resolver: &mut Resolver,
    interner: &Interner,
    stmt: &SiStmt,
    errors: &mut Vec<SemanticError>,
) {
    resolve_expr(resolver, interner, &stmt.cond, errors);
    resolve_if_body(resolver, interner, &stmt.then, errors);
    if let Some(catch) = &stmt.catch {
        resolve_catch_binding(resolver, interner, catch, errors);
    }
    if let Some(else_) = &stmt.else_ {
        resolve_secus_clause(resolver, interner, else_, errors);
    }
}

fn resolve_probandum(
    resolver: &mut Resolver,
    interner: &Interner,
    test: &ProbandumDecl,
    errors: &mut Vec<SemanticError>,
) {
    resolver.enter_scope(ScopeKind::Module);
    for setup in &test.body.setup {
        resolve_block(resolver, interner, &setup.body, errors);
    }
    for case in &test.body.tests {
        resolve_block(resolver, interner, &case.body, errors);
    }
    for nested in &test.body.nested {
        resolve_probandum(resolver, interner, nested, errors);
    }
    resolver.exit_scope();
}

fn resolve_secus_clause(
    resolver: &mut Resolver,
    interner: &Interner,
    clause: &radix_syntax::SecusClause,
    errors: &mut Vec<SemanticError>,
) {
    match clause {
        radix_syntax::SecusClause::Sin { stmt, .. } => {
            resolve_si_stmt(resolver, interner, stmt, errors);
        }
        radix_syntax::SecusClause::Block { body, catch, .. } => {
            resolve_block(resolver, interner, body, errors);
            if let Some(catch) = catch {
                resolve_catch_binding(resolver, interner, catch, errors);
            }
        }
        radix_syntax::SecusClause::Stmt { stmt, catch, .. } => {
            resolve_stmt(resolver, interner, stmt, errors);
            if let Some(catch) = catch {
                resolve_catch_binding(resolver, interner, catch, errors);
            }
        }
    }
}

/// Resolve one `elige` `casu` value expression.
///
/// The casu value is an expression position, but HIR lowering treats it as a
/// variant pattern. A member chain with leading path segments must therefore
/// resolve the leading path as a real qualifier before the penultimate
/// segment can name a local enum/union — `casu missing.A.X { }` must never
/// silently bind the local `A.X` through the bare-name fallback. A prefix
/// that is not a real qualifier fails closed as an unknown variant;
/// two-segment paths (`casu A.X { }`) keep the existing member-resolution
/// behavior, and every other value resolves as an ordinary expression.
///
/// When the subject's annotation names a closed set, a bare name that
/// resolves to nothing is a misspelled member: it reports the set's members
/// and the nearest spelling instead of the generic unknown-identifier error.
fn resolve_elige_case_value(
    resolver: &mut Resolver,
    interner: &Interner,
    value: &Expr,
    subject_members: Option<&[String]>,
    errors: &mut Vec<SemanticError>,
) {
    if let ExprKind::Ident(ident) = &value.kind
        && let Some(members) = subject_members
        && !members.is_empty()
        && resolver.lookup(ident.name).is_none()
        && matches!(
            resolver.lookup_variant_bare(ident.name),
            BareVariantLookup::None
        )
    {
        errors.push(elige_case_not_member_error(interner, ident, members));
        return;
    }
    if let ExprKind::Member(member) = &value.kind {
        let last_span = member.member.span;
        if let Some(segments) = member_segments(member)
            && segments.len() >= 3
        {
            match resolver.qualified_variant_path(&segments) {
                QualifiedVariantPathLookup::Variant(_) => return,
                QualifiedVariantPathLookup::UnknownVariant
                | QualifiedVariantPathLookup::NotQualified => {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::VariableUndefined, last_span)
                            .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
                    );
                    return;
                }
            }
        }
    }
    resolve_expr(resolver, interner, value, errors);
}

/// Collect the identifier segments of a right-leaning member chain
/// (`a.b.c` → `[a, b, c]`). Returns `None` when any step is not a plain
/// identifier (index, call, and optional-chain steps are not variant paths).
fn member_segments(member: &radix_syntax::MemberExpr) -> Option<Vec<radix_lexer::Symbol>> {
    let mut segments = Vec::new();
    segments.push(member.member.name);
    let mut object = member.object.as_ref();
    loop {
        match &object.kind {
            ExprKind::Ident(ident) => {
                segments.push(ident.name);
                break;
            }
            ExprKind::Member(inner) => {
                segments.push(inner.member.name);
                object = inner.object.as_ref();
            }
            _ => return None,
        }
    }
    segments.reverse();
    Some(segments)
}

fn resolve_discerne(
    resolver: &mut Resolver,
    interner: &Interner,
    stmt: &DiscerneStmt,
    errors: &mut Vec<SemanticError>,
) {
    for subject in &stmt.subjects {
        resolve_expr(resolver, interner, subject, errors);
    }

    let member_sets = discerne_member_sets(resolver, interner, stmt);
    resolver.enter_scope(ScopeKind::Match);
    for arm in &stmt.arms {
        resolver.enter_scope(ScopeKind::Block);
        for (position, pattern) in arm.patterns.iter().enumerate() {
            let members = member_sets.get(position).map_or(&[][..], Vec::as_slice);
            resolve_pattern(resolver, interner, pattern, members, errors);
        }
        resolve_if_body(resolver, interner, &arm.body, errors);
        resolver.exit_scope();
    }
    if let Some(default) = &stmt.default {
        resolve_if_body(resolver, interner, &default.body, errors);
    }
    resolver.exit_scope();
}

fn push_closed_set_refusal(span: radix_lexer::Span, errors: &mut Vec<SemanticError>) {
    errors.push(
        SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
            .with_arg("issue", ISSUE_DISCERNE_LITERAL_ARM_NOT_IN_CLOSED_SET)
            .with_arg("pattern", "literal"),
    );
}

/// Parent definition of the closed set one arm pattern names a member of.
///
/// Only a pattern that already resolves to a variant (bare or through its
/// enum/union parent) identifies a set; a misspelled name identifies nothing.
fn pattern_member_parent(
    resolver: &Resolver,
    pattern: &Pattern,
    parents: &handroll::FxHashMap<DefId, DefId>,
) -> Option<DefId> {
    let bare_parent = |name: radix_lexer::Symbol| match resolver.lookup_variant_bare(name) {
        BareVariantLookup::Single(variant) => parents.get(&variant).copied(),
        BareVariantLookup::Ambiguous { .. } | BareVariantLookup::None => None,
    };
    match pattern {
        Pattern::Ident(ident, _) => bare_parent(ident.name),
        Pattern::Type(ty, _) => match &ty.kind {
            TypeExprKind::Named(ident, params) if params.is_empty() => bare_parent(ident.name),
            _ => None,
        },
        Pattern::Path(path) => match path.segments.as_slice() {
            [.., parent, _] => resolver
                .lookup(parent.name)
                .filter(|def| {
                    resolver
                        .get_symbol(*def)
                        .is_some_and(|symbol| symbol.kind == SymbolKind::Enum)
                })
                .or_else(|| bare_parent(path.segments.last()?.name)),
            _ => None,
        },
        Pattern::Or(alternatives) => alternatives
            .iter()
            .find_map(|alternative| pattern_member_parent(resolver, alternative, parents)),
        Pattern::Wildcard(_) | Pattern::Literal(..) | Pattern::NegatedLiteral(..) => None,
    }
}

/// Member names of the closed set each subject position of a `discerne`
/// matches, in declaration order.
///
/// Resolution runs before value types are known. A binding whose annotation
/// names a union or an enum/discretio carries its members on the resolver (see
/// [`Resolver::subject_member_set`]); otherwise the set is read from the
/// variants the arms of that position already name. A position with neither
/// source has no known set and yields an empty list.
fn discerne_member_sets(
    resolver: &Resolver,
    interner: &Interner,
    stmt: &DiscerneStmt,
) -> Vec<Vec<String>> {
    let parents: handroll::FxHashMap<DefId, DefId> =
        resolver.variant_parents_all().into_iter().collect();
    (0..stmt.subjects.len())
        .map(|position| {
            if let Some(members) = stmt
                .subjects
                .get(position)
                .and_then(|subject| subject_member_set(resolver, subject))
            {
                return members;
            }
            let parent = stmt
                .arms
                .iter()
                .filter_map(|arm| arm.patterns.get(position))
                .find_map(|pattern| pattern_member_parent(resolver, pattern, &parents));
            let Some(parent) = parent else {
                return Vec::new();
            };
            parent_variant_names(resolver, interner, parent).unwrap_or_default()
        })
        .collect()
}

/// The member spelling nearest to a misspelled arm name, if any is close.
fn nearest_member<'a>(spelling: &str, members: &'a [String]) -> Option<&'a str> {
    let folded = name_suggestion_key(spelling);
    members
        .iter()
        .filter_map(|member| {
            let candidate = name_suggestion_key(member);
            let distance = edit_distance(&folded, &candidate);
            (distance <= name_suggestion_distance_threshold(&candidate))
                .then_some((distance, member.as_str()))
        })
        .min()
        .map(|(_, member)| member)
}

/// Attach the matched set's members and the nearest member spelling to a
/// member-miss diagnostic; a set with no spellable members stays bare.
fn with_member_suggestion(
    mut error: SemanticError,
    interner: &Interner,
    ident: &radix_syntax::Ident,
    members: &[String],
) -> SemanticError {
    if !members.is_empty() {
        error = error.with_arg("members", members.join(", "));
        let spelling = interner.resolve(ident.name);
        if let Some(suggestion) = nearest_member(spelling, members) {
            error = error.with_arg("suggestion", suggestion.to_owned());
        }
    }
    error
}

/// An arm name that is not a member of the matched set.
///
/// `discerne` iterates a closed set, so a bare arm name is a member or an
/// error; it never binds. `_` and the default arm are the only catch-alls.
fn arm_not_member_error(
    interner: &Interner,
    ident: &radix_syntax::Ident,
    members: &[String],
) -> SemanticError {
    let spelling = interner.resolve(ident.name);
    let error = SemanticError::structured(SemanticErrorKind::TypeMismatch, ident.span)
        .with_arg("issue", ISSUE_DISCERNE_ARM_NOT_MEMBER)
        .with_arg("name", spelling.to_owned());
    with_member_suggestion(error, interner, ident, members)
}

/// An `elige` case name that resolves to nothing while the subject's
/// annotation names a closed set.
///
/// `elige` cases are value expressions; over a known enum or union annotation
/// an unresolved bare name is a misspelled member rather than an open-set
/// value, so the diagnostic carries the member list and nearest spelling.
fn elige_case_not_member_error(
    interner: &Interner,
    ident: &radix_syntax::Ident,
    members: &[String],
) -> SemanticError {
    let error = SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
        .with_arg("issue", ISSUE_UNKNOWN_VARIANT);
    with_member_suggestion(error, interner, ident, members)
}

fn resolve_pattern(
    resolver: &mut Resolver,
    interner: &Interner,
    pattern: &Pattern,
    members: &[String],
    errors: &mut Vec<SemanticError>,
) {
    match pattern {
        Pattern::Wildcard(_) => {}
        // `discerne` matches a closed set (union members, enum variants). The
        // two-valued `bivalens` literals and the absent literal discriminate a
        // closed set; any other literal is a value test over an open set and
        // belongs to `elige` (operator ruling 2026-10-02). `elige` has its own
        // statement and never reaches this resolver arm.
        Pattern::Literal(literal, span) => {
            if !matches!(
                literal,
                radix_syntax::Literal::Bool(_) | radix_syntax::Literal::Nil
            ) {
                push_closed_set_refusal(*span, errors);
            }
        }
        Pattern::NegatedLiteral(_, span) => push_closed_set_refusal(*span, errors),
        Pattern::Type(_ty, bind) => {
            // The type pattern is classified before resolution. Only its
            // optional value binding enters the arm scope; the type name and
            // its width/shape arguments are not locals and therefore must not
            // be shadowed into one.
            resolve_pattern_bind(resolver, bind.as_ref(), errors);
        }
        Pattern::Ident(ident, bind) => {
            // A bare arm name is a member of the matched set, never a
            // binding. A name that two or more unions define is an ambiguous
            // variant pattern. A name that exactly one union defines is a
            // variant constructor pattern (lowering owns the variant shape),
            // so it defines no local: the same variant may appear in several
            // subject positions of one multi-subject arm without a duplicate
            // definition. A name that resolves as a type is classified at
            // lowering as a type pattern. Anything else is not a member and
            // is an error; `_` and the default arm are the only catch-alls.
            match resolver.lookup_variant_bare(ident.name) {
                BareVariantLookup::Single(_) => {}
                BareVariantLookup::Ambiguous { .. } => {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
                            .with_arg("issue", ISSUE_VARIANT_AMBIGUOUS),
                    );
                }
                BareVariantLookup::None => {
                    let names_type = resolver.lookup_type(ident.name).is_some()
                        || resolver.lookup_selective_import_type(ident.name).is_some();
                    let names_variant = resolver.lookup(ident.name).is_some_and(|def| {
                        resolver
                            .get_symbol(def)
                            .is_some_and(|symbol| symbol.kind == SymbolKind::Variant)
                    });
                    if !names_type && !names_variant {
                        errors.push(arm_not_member_error(interner, ident, members));
                    }
                }
            }
            resolve_pattern_bind(resolver, bind.as_ref(), errors);
        }
        Pattern::Or(alternatives) => {
            let Some((first, rest)) = alternatives.split_first() else {
                return;
            };
            resolve_pattern(resolver, interner, first, members, errors);
            for alternative in rest {
                // Alternative-only bindings must not collide with the arm's
                // canonical first-alternative bindings. Lowering/typecheck
                // compare the alternatives and expose the common environment.
                resolver.enter_scope(ScopeKind::Match);
                resolve_pattern(resolver, interner, alternative, members, errors);
                resolver.exit_scope();
            }
        }
        Pattern::Path(path) => {
            let segments = path.segments.as_slice();
            match segments {
                [.., parent, last] => {
                    if segments.len() >= 3 {
                        // A path with leading path segments must resolve the
                        // leading path as a real qualifier before the
                        // penultimate segment can name the parent enum/union —
                        // a pattern like `casu missing.A.X { }` must never
                        // silently bind the local `A.X`.
                        let symbols: Vec<radix_lexer::Symbol> =
                            segments.iter().map(|ident| ident.name).collect();
                        match resolver.qualified_variant_path(&symbols) {
                            QualifiedVariantPathLookup::Variant(_) => {}
                            QualifiedVariantPathLookup::UnknownVariant
                            | QualifiedVariantPathLookup::NotQualified => {
                                errors.push(
                                    SemanticError::structured(
                                        SemanticErrorKind::VariableUndefined,
                                        last.span,
                                    )
                                    .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
                                );
                            }
                        }
                    } else {
                        // Qualified path pattern `A.X`: resolve the terminal
                        // segment against the penultimate segment's union when it
                        // names one; a bare last-segment fallback preserves the
                        // legacy module-path behavior.
                        let parent_enum = resolver.lookup(parent.name).and_then(|parent_def| {
                            resolver
                                .get_symbol(parent_def)
                                .filter(|symbol| symbol.kind == SymbolKind::Enum)
                                .map(|_| parent_def)
                        });
                        if let Some(parent_enum) = parent_enum {
                            if resolver.variant_child(parent_enum, last.name).is_none() {
                                report_non_variant_suffix(resolver, last, errors);
                            }
                        } else {
                            resolve_variant_path_without_enum_parent(
                                resolver, segments, last, errors,
                            );
                        }
                    }
                }
                [last] => resolve_variant_ident(resolver, last, errors),
                [] => {}
            }
            resolve_pattern_bind(resolver, path.bind.as_ref(), errors);
        }
    }
}

/// Resolve a qualified variant path whose penultimate segment is not an
/// ordinary Enum binding.
///
/// A qualified imported parent (`a.A.X`) resolves BEFORE any bare-variant
/// fallback: two imported unions may share the terminal variant
/// name without collapsing into a false ambiguity, and a miss against an
/// identified variant-bearing parent fails closed as an unknown variant
/// instead of binding through another union. Paths that are not qualified
/// imported variant paths at all keep the legacy bare-table / per-union
/// policy.
fn resolve_variant_path_without_enum_parent(
    resolver: &mut Resolver,
    segments: &[radix_syntax::Ident],
    last: &radix_syntax::Ident,
    errors: &mut Vec<SemanticError>,
) {
    let symbols: Vec<radix_lexer::Symbol> = segments.iter().map(|ident| ident.name).collect();
    match resolver.qualified_variant_path(&symbols) {
        QualifiedVariantPathLookup::Variant(_) => {}
        QualifiedVariantPathLookup::UnknownVariant => {
            errors.push(
                SemanticError::structured(SemanticErrorKind::VariableUndefined, last.span)
                    .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
            );
        }
        QualifiedVariantPathLookup::NotQualified => {
            // A two-segment path naming an imported type (`geo.Circle`) is a
            // type pattern, not a variant reference (BUG-82852379): skip the
            // bare-variant resolution exactly as an authored type pattern
            // resolves. A namespace member that is not a type keeps the
            // bare-variant resolution, so unknown members still fail closed.
            if segments.len() == 2
                && resolver
                    .namespace_file_type(segments[0].name, last.name)
                    .is_some()
            {
                return;
            }
            resolve_variant_ident(resolver, last, errors);
        }
    }
}

/// Report the diagnostic for a suffix that names no variant of its known
/// enum/union parent.
///
/// A known non-variant name is `name_not_variant`; a multi-union bare name is
/// an explicit ambiguity; anything else is an unknown variant.
fn report_non_variant_suffix(
    resolver: &Resolver,
    suffix: &radix_syntax::Ident,
    errors: &mut Vec<SemanticError>,
) {
    match resolver.lookup(suffix.name) {
        Some(_) => {
            errors.push(
                SemanticError::structured(SemanticErrorKind::VariableUndefined, suffix.span)
                    .with_arg("issue", ISSUE_NAME_NOT_VARIANT),
            );
        }
        None => match resolver.lookup_variant_bare(suffix.name) {
            BareVariantLookup::Ambiguous { .. } => {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::VariableUndefined, suffix.span)
                        .with_arg("issue", ISSUE_VARIANT_AMBIGUOUS),
                );
            }
            BareVariantLookup::Single(_) | BareVariantLookup::None => {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::VariableUndefined, suffix.span)
                        .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
                );
            }
        },
    }
}

fn resolve_pattern_bind(
    resolver: &mut Resolver,
    bind: Option<&PatternBind>,
    errors: &mut Vec<SemanticError>,
) {
    if let Some(bind) = bind {
        match bind {
            PatternBind::Alias(alias) => {
                define_symbol(
                    resolver,
                    alias.name,
                    alias.span,
                    SymbolKind::Local,
                    false,
                    errors,
                );
            }
            PatternBind::Bindings {
                mutability,
                bindings,
            } => {
                for binding in bindings {
                    let name = binding.alias.as_ref().unwrap_or(&binding.name);
                    define_symbol(
                        resolver,
                        name.name,
                        name.span,
                        SymbolKind::Local,
                        *mutability == radix_syntax::AstMutability::Mutable,
                        errors,
                    );
                }
            }
        }
    }
}

/// Whether `expr` is exactly a bare name resolving to an `ordo` definition.
///
/// `itera ex Color` names the enum's member series, so it is the one place a
/// type-kind name stands in expression position without the ordinary
/// `name_not_value` rejection.
fn is_enum_iteration_source(resolver: &Resolver, expr: &Expr) -> bool {
    let ExprKind::Ident(ident) = &expr.kind else {
        return false;
    };
    resolver
        .lookup(ident.name)
        .and_then(|def_id| resolver.get_symbol(def_id))
        .is_some_and(|symbol| symbol.kind == SymbolKind::Enum)
}

fn resolve_expr(
    resolver: &mut Resolver,
    interner: &Interner,
    expr: &Expr,
    errors: &mut Vec<SemanticError>,
) {
    match &expr.kind {
        ExprKind::Ident(ident) => {
            let Some(def_id) = resolver.lookup(ident.name) else {
                // Bare names may still name a union variant. When exactly one
                // union defines the name the reference is valid; two or more is
                // an explicit ambiguity, never a silent pick.
                match resolver.lookup_variant_bare(ident.name) {
                    BareVariantLookup::Single(_) => {}
                    BareVariantLookup::Ambiguous { .. } => {
                        errors.push(
                            SemanticError::structured(
                                SemanticErrorKind::VariableUndefined,
                                ident.span,
                            )
                            .with_arg("issue", ISSUE_VARIANT_AMBIGUOUS),
                        );
                    }
                    BareVariantLookup::None => {
                        errors.push(undefined_identifier_error(resolver, interner, ident));
                    }
                }
                return;
            };
            // Type-kind names (structs, enums, interfaces, type aliases, type
            // params, schemas) must not be accepted in expression position.
            // A size param (`magnitudo`) is the one exception: it names a
            // read-only int value (size-type Z1 force).
            if let Some(symbol) = resolver.get_symbol(def_id)
                && !symbol.kind.is_value_name()
                && symbol.kind != SymbolKind::IndexParam
            {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
                        .with_arg("issue", ISSUE_NAME_NOT_VALUE),
                );
            }
        }
        ExprKind::Literal(_) | ExprKind::Ego(_) | ExprKind::Insere(_) => {}
        // A value-literal keyword is the fallback when no same-named value
        // binding is live; a binding in scope wins and lowering emits its
        // name load. Functions never shadow (they stay callable as `f()`).
        ExprKind::KeywordLiteral(keyword) => {
            record_shadowed_marker(resolver, expr.id, keyword.ident.name);
        }
        ExprKind::Binary(expr) => {
            resolve_expr(resolver, interner, &expr.lhs, errors);
            resolve_expr(resolver, interner, &expr.rhs, errors);
        }
        ExprKind::Unary(expr) => resolve_expr(resolver, interner, &expr.operand, errors),
        ExprKind::Ternary(expr) => {
            resolve_expr(resolver, interner, &expr.cond, errors);
            resolve_expr(resolver, interner, &expr.then, errors);
            resolve_expr(resolver, interner, &expr.else_, errors);
        }
        ExprKind::Call(expr) => {
            // WHY: V-5 — the callee is an ordinary expression position. The
            // marker is only a fallback when no same-named value binding is
            // live, so resolving it here keeps calls consistent with every
            // other position (`arg`, receiver, index base, and chain base).
            //
            // em1: a call position is the one place a function named like the
            // marker still wins — `vacua()` calls the function, while the
            // bare `vacua` beside it stays the marker.
            if let ExprKind::Vacua(ident) = &expr.callee.kind {
                record_shadowed_builtin(resolver, expr.callee.id, ident.name);
            } else if let ExprKind::Member(callee) = &expr.callee.kind {
                // A call through a type name (`Tab.f()`) is not a static read.
                resolve_member_expr(resolver, interner, callee, false, errors);
            } else {
                resolve_expr(resolver, interner, &expr.callee, errors);
            }
            for arg in &expr.args {
                resolve_expr(resolver, interner, &arg.value, errors);
            }
        }
        ExprKind::Member(expr) => resolve_member_expr(resolver, interner, expr, true, errors),
        ExprKind::Index(expr) => {
            resolve_expr(resolver, interner, &expr.object, errors);
            resolve_expr(resolver, interner, &expr.index, errors);
        }
        ExprKind::Gradient(expr) => {
            resolve_expr(resolver, interner, &expr.call, errors);
            for selector in &expr.selectors {
                resolve_expr(resolver, interner, selector, errors);
            }
        }
        ExprKind::OptionalChain(expr) => {
            resolve_expr(resolver, interner, &expr.object, errors);
            match &expr.chain {
                radix_syntax::OptionalChainKind::Member(_) => {}
                radix_syntax::OptionalChainKind::Index(expr) => {
                    resolve_expr(resolver, interner, expr, errors);
                }
                radix_syntax::OptionalChainKind::Call(args) => {
                    for arg in args {
                        resolve_expr(resolver, interner, &arg.value, errors);
                    }
                }
            }
        }
        ExprKind::NonNull(expr) => {
            resolve_expr(resolver, interner, &expr.object, errors);
            match &expr.chain {
                radix_syntax::NonNullKind::Member(_) => {}
                radix_syntax::NonNullKind::Index(expr) => {
                    resolve_expr(resolver, interner, expr, errors);
                }
                radix_syntax::NonNullKind::Call(args) => {
                    for arg in args {
                        resolve_expr(resolver, interner, &arg.value, errors);
                    }
                }
            }
        }
        ExprKind::Assign(expr) => {
            resolve_expr(resolver, interner, &expr.target, errors);
            resolve_expr(resolver, interner, &expr.value, errors);
        }
        ExprKind::Verte(expr) => {
            resolve_expr(resolver, interner, &expr.expr, errors);
            resolve_type_expr(resolver, interner, &expr.ty, errors);
        }
        ExprKind::Finge(expr) => {
            resolve_variant_path(resolver, &expr.variant, errors);
            for field in &expr.fields {
                resolve_expr(resolver, interner, &field.value, errors);
            }
            if let Some(cast) = &expr.cast {
                resolve_type_expr(resolver, interner, cast, errors);
            }
        }
        ExprKind::Clausura(expr) => {
            resolver.enter_scope(ScopeKind::Function);
            for param in &expr.params {
                resolve_type_expr(resolver, interner, &param.ty, errors);
                define_symbol(
                    resolver,
                    param.name.name,
                    param.name.span,
                    SymbolKind::Param,
                    false,
                    errors,
                );
                record_iuncta_labels_for_name(resolver, interner, param.name.name, &param.ty);
            }
            if let Some(ret) = &expr.ret {
                resolve_type_expr(resolver, interner, ret, errors);
            }
            if let Some(err) = &expr.err {
                resolve_type_expr(resolver, interner, err, errors);
            }
            match &expr.body {
                ClausuraBody::Expr(expr) => resolve_expr(resolver, interner, expr, errors),
                ClausuraBody::Block(block) => resolve_block(resolver, interner, block, errors),
                ClausuraBody::Fac(stmt) => {
                    resolver.enter_scope(fac_scope_kind(stmt));
                    resolve_block(resolver, interner, &stmt.body, errors);
                    resolver.exit_scope();
                    if let Some(catch) = &stmt.catch {
                        resolve_catch_binding(resolver, interner, catch, errors);
                    }
                    if let Some(cond) = &stmt.while_ {
                        resolve_expr(resolver, interner, cond, errors);
                    }
                }
            }
            resolver.exit_scope();
        }
        ExprKind::Reducta(expr) => {
            resolve_expr(resolver, interner, &expr.source, errors);
            if let Some(identity) = &expr.identity {
                resolve_expr(resolver, interner, identity, errors);
            }
            resolver.enter_scope(ScopeKind::Block);
            for coordinate in &expr.apud {
                define_symbol(
                    resolver,
                    coordinate.name,
                    coordinate.span,
                    SymbolKind::Local,
                    false,
                    errors,
                );
            }
            if let Some(group) = &expr.group {
                resolve_expr(resolver, interner, group, errors);
            }
            if let Some(filum) = &expr.filum {
                define_symbol(
                    resolver,
                    filum.name,
                    filum.span,
                    SymbolKind::Local,
                    false,
                    errors,
                );
            }
            if let Some(binding) = &expr.binding {
                define_symbol(
                    resolver,
                    binding.name,
                    binding.span,
                    SymbolKind::Local,
                    expr.mutability == radix_syntax::AstMutability::Mutable,
                    errors,
                );
            }
            if let Some(body) = &expr.body {
                resolve_block(resolver, interner, body, errors);
            }
            resolver.exit_scope();
        }
        ExprKind::Scan(expr) => {
            resolve_expr(resolver, interner, &expr.source, errors);
            resolver.enter_scope(ScopeKind::Block);
            for coordinate in &expr.apud {
                define_symbol(
                    resolver,
                    coordinate.name,
                    coordinate.span,
                    SymbolKind::Local,
                    false,
                    errors,
                );
            }
            if let Some(group) = &expr.group {
                resolve_expr(resolver, interner, group, errors);
            }
            define_symbol(
                resolver,
                expr.filum.name,
                expr.filum.span,
                SymbolKind::Local,
                false,
                errors,
            );
            define_symbol(
                resolver,
                expr.binding.name,
                expr.binding.span,
                SymbolKind::Local,
                false,
                errors,
            );
            resolve_block(resolver, interner, &expr.body, errors);
            resolver.exit_scope();
        }
        ExprKind::Sort(expr) => {
            resolve_expr(resolver, interner, &expr.source, errors);
            resolver.enter_scope(ScopeKind::Block);
            if let Some(binding) = &expr.binding {
                define_symbol(
                    resolver,
                    binding.name,
                    binding.span,
                    SymbolKind::Local,
                    expr.mutability == radix_syntax::AstMutability::Mutable,
                    errors,
                );
            }
            if let Some(body) = &expr.body {
                resolve_block(resolver, interner, body, errors);
            }
            resolver.exit_scope();
        }
        ExprKind::Cede(expr) => resolve_expr(resolver, interner, &expr.expr, errors),
        ExprKind::Array(expr) => {
            for element in &expr.elements {
                match element {
                    radix_syntax::ArrayElement::Expr(expr)
                    | radix_syntax::ArrayElement::Spread(expr) => {
                        resolve_expr(resolver, interner, expr, errors);
                    }
                    radix_syntax::ArrayElement::Infer(_) => {}
                }
            }
        }
        ExprKind::Object(expr) => {
            for field in &expr.fields {
                match &field.key {
                    radix_syntax::ObjectKey::Computed(expr)
                    | radix_syntax::ObjectKey::Spread(expr) => {
                        resolve_expr(resolver, interner, expr, errors);
                    }
                    _ => {}
                }
                if let Some(value) = &field.value {
                    resolve_expr(resolver, interner, value, errors);
                }
            }
        }
        ExprKind::Intervallum(expr) => {
            resolve_expr(resolver, interner, &expr.start, errors);
            resolve_expr(resolver, interner, &expr.end, errors);
            if let Some(step) = &expr.step {
                resolve_expr(resolver, interner, step, errors);
            }
        }
        ExprKind::Conversio(expr) => {
            resolve_expr(resolver, interner, &expr.expr, errors);
            // WHY: The runtime conversion target is a real type expression.
            // Conversio parameters such as `Hex` in `↦ i32 via Hex` are
            // codegen hints, not normal type annotations that must resolve in scope.
            match &expr.target {
                ConversioTarget::Explicit(ty) => {
                    resolve_conversio_target_type(resolver, interner, ty, errors);
                }
                ConversioTarget::Intervallum(operand) => {
                    resolve_expr(resolver, interner, operand, errors);
                }
            }
            if let Some(recovery) = &expr.recovery {
                resolve_expr(resolver, interner, recovery, errors);
            }
        }
        ExprKind::Scriptum(scriptum) => {
            record_shadowed_builtin(resolver, expr.id, scriptum.symbol);
            for arg in &scriptum.args {
                resolve_expr(resolver, interner, arg, errors);
            }
        }
        ExprKind::Lege(lege) => {
            record_shadowed_builtin(resolver, expr.id, lege.symbol);
        }
        ExprKind::Ad(ad_expr) => {
            if let Some(opener) = &ad_expr.opener {
                resolve_expr(resolver, interner, opener, errors);
            }
        }

        // WHY: V-5 — the parser claims the marker spelling before resolution,
        // but the marker is only the fallback when no same-named value binding
        // is live. This makes binding lookup win uniformly in expression
        // position while preserving the marker when the name is unbound.
        // A *function* named like the marker does not shadow it (em1): the bare
        // marker stays the marker, and the function stays callable as `f()`.
        ExprKind::Vacua(ident) => record_shadowed_marker(resolver, expr.id, ident.name),
        // A `praefixum` body is a function body: `redde` yields the static's
        // value (D8.4).
        ExprKind::Praefixum(expr) => {
            resolver.enter_scope(ScopeKind::Function);
            resolve_block(resolver, interner, &expr.body, errors);
            resolver.exit_scope();
        }
        ExprKind::Paren(expr) => resolve_expr(resolver, interner, expr, errors),
        ExprKind::TypeCheck(expr) => {
            resolve_expr(resolver, interner, &expr.expr, errors);
            resolve_est_type_target(resolver, interner, &expr.ty, errors);
        }
        ExprKind::Iuncta(expr) => {
            for ty in &expr.type_args {
                resolve_type_expr(resolver, interner, ty, errors);
            }
            for element in &expr.elements {
                resolve_expr(resolver, interner, element, errors);
            }
        }
        // `⇇` copy-into: name resolution walks target and value so ordinary
        // binding-wins lookup applies to both.
        ExprKind::Format(format) => resolve_expr(resolver, interner, &format.value, errors),
        // Constructor claim (size-type Z2): the claimed surface is a type
        // expression, so its extent figuras resolve like an annotation's.
        // Synthesis is the follow-up unit; the placeholder lowering errors.
        ExprKind::Construct(ty) => resolve_type_expr(resolver, interner, ty, errors),
        ExprKind::CopyInto(expr) => {
            resolve_expr(resolver, interner, &expr.target, errors);
            resolve_expr(resolver, interner, &expr.value, errors);
        }
        // `⇜` sink-into: name resolution walks sink and item so ordinary
        // binding-wins lookup applies to both. Lowering is the follow-up unit.
        ExprKind::SinkInto(expr) => {
            resolve_expr(resolver, interner, &expr.sink, errors);
            resolve_expr(resolver, interner, &expr.item, errors);
        }
        // UT: the capture boundary's body is an ordinary block — binding-wins
        // lookup applies to every subexpression inside it.
        ExprKind::Trap(trap) => {
            resolve_block(resolver, interner, &trap.body, errors);
        }
        ExprKind::CallDefault(call_default) => {
            resolve_expr(resolver, interner, &call_default.call, errors);
            resolve_expr(resolver, interner, &call_default.default, errors);
        }
    }
}

/// If a builtin-claimed spelling is a visible value binding, record that the
/// expression is shadowed so lowering emits a name load instead of the builtin.
fn record_shadowed_builtin(
    resolver: &mut Resolver,
    node: radix_syntax::NodeId,
    name: radix_lexer::Symbol,
) {
    let Some(def_id) = resolver.lookup(name) else {
        return;
    };
    if resolver
        .get_symbol(def_id)
        .is_some_and(|symbol| !symbol.kind.is_value_name())
    {
        return;
    }
    resolver.record_shadowed_builtin(node, def_id);
}

/// Like [`record_shadowed_builtin`] for the empty-collection marker, except a
/// function (declared or imported) never shadows the bare marker: only locals,
/// params, constants and other non-function value bindings do.
fn record_shadowed_marker(
    resolver: &mut Resolver,
    node: radix_syntax::NodeId,
    name: radix_lexer::Symbol,
) {
    let Some(def_id) = resolver.lookup(name) else {
        return;
    };
    if resolver
        .get_symbol(def_id)
        .is_some_and(|symbol| !symbol.kind.is_value_name() || symbol.kind == SymbolKind::Function)
    {
        return;
    }
    resolver.record_shadowed_builtin(node, def_id);
}

/// The enum-variant definition an `est` right-hand side names, if any.
///
/// The right-hand side of `est`/`non est` is always a type (DECISIONS D23.1),
/// so an enum variant name is not a value path any more. It is also not a
/// nominal type the type namespace carries: `r est DetailCarrier` targets the
/// variant's member type. This classifies that case for both name resolution
/// and the HIR type lowering of the test target. Bare names and
/// module-qualified imported variant paths (`aux.Carrier.Detail`) are covered;
/// a locally-qualified `Carrier.Detail` was already rejected as a type before
/// D23.1 and stays rejected.
pub(crate) fn est_target_variant(resolver: &Resolver, ty: &TypeExpr) -> Option<DefId> {
    match &ty.kind {
        TypeExprKind::Named(name, params) if params.is_empty() => {
            resolver.lookup_variant_bare(name.name).def_id()
        }
        TypeExprKind::Qualified(path, params) if path.len() >= 2 && params.is_empty() => {
            match resolver
                .qualified_variant_path(&path.iter().map(|ident| ident.name).collect::<Vec<_>>())
            {
                QualifiedVariantPathLookup::Variant(def_id) => Some(def_id),
                QualifiedVariantPathLookup::NotQualified
                | QualifiedVariantPathLookup::UnknownVariant => None,
            }
        }
        _ => None,
    }
}

/// Resolve the right-hand side of `est` / `non est` (DECISIONS D23.1).
///
/// The right-hand side is always a type, so this is ordinary type resolution
/// with one addition and one subtraction. An enum-variant target (`r est M`)
/// is legal — it tests the variant's member type. A name that resolves to a
/// value — a declared constant, or the `verum`/`falsum` literals — is rejected
/// by name: value comparison belongs to the equality family.
fn resolve_est_type_target(
    resolver: &mut Resolver,
    interner: &Interner,
    ty: &TypeExpr,
    errors: &mut Vec<SemanticError>,
) {
    if est_target_variant(resolver, ty).is_some() {
        return;
    }
    match &ty.kind {
        TypeExprKind::Named(name, params) if params.is_empty() => {
            let source = interner.resolve(name.name);
            if !is_builtin_type(source)
                && resolver.lookup_type(name.name).is_none()
                && resolver.lookup_selective_import_type(name.name).is_none()
            {
                // `verum` / `falsum` are boolean literal keywords, never types.
                if matches!(
                    radix_lexer::lookup_keyword_spec(source)
                        .and_then(|spec| spec.token_kind.clone()),
                    Some(radix_lexer::TokenKind::Verum | radix_lexer::TokenKind::Falsum)
                ) {
                    errors.push(est_value_rhs_error(name.span));
                    return;
                }
                if let Some(def_id) = resolver.lookup(name.name) {
                    if resolver
                        .get_symbol(def_id)
                        .is_some_and(|symbol| symbol.kind.is_value_name())
                    {
                        errors.push(est_value_rhs_error(name.span));
                        return;
                    }
                } else if let BareVariantLookup::Ambiguous { .. } =
                    resolver.lookup_variant_bare(name.name)
                {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::VariableUndefined, name.span)
                            .with_arg("issue", ISSUE_VARIANT_AMBIGUOUS),
                    );
                    return;
                }
            }
        }
        TypeExprKind::Qualified(path, params) if params.is_empty() => {
            if let Some((namespace, member)) = path.first().zip(path.last())
                && resolver.namespace_exports_member(namespace.name, member.name)
                && (resolver
                    .namespace_file_const(namespace.name, member.name)
                    .is_some()
                    || resolver
                        .namespace_file_function(namespace.name, member.name)
                        .is_some())
            {
                errors.push(est_value_rhs_error(member.span));
                return;
            }
        }
        _ => {}
    }
    resolve_type_expr(resolver, interner, ty, errors);
}

/// The long form `vector<T, size N>` writes its lane count bare. The count takes the
/// `size` marker (`vector<T, size N>`); the `_` hole and a bracketed extent
/// (`vf32[4]`, `vector<T, [4]>`) are unchanged, because the brackets already
/// mark an extent.
fn vector_width_is_bare_count(figura: &radix_syntax::FiguraExpr) -> bool {
    use radix_syntax::FiguraExpr;
    matches!(
        figura,
        FiguraExpr::Literal(_)
            | FiguraExpr::Var(_)
            | FiguraExpr::Add(..)
            | FiguraExpr::Subtract(..)
            | FiguraExpr::Divide(..)
    )
}

fn est_value_rhs_error(span: radix_lexer::Span) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::OperandTypesInvalid, span)
        .with_arg("issue", ISSUE_EST_VALUE_RHS)
}

fn resolve_type_expr(
    resolver: &mut Resolver,
    interner: &Interner,
    ty: &TypeExpr,
    errors: &mut Vec<SemanticError>,
) {
    match &ty.kind {
        TypeExprKind::Infer | TypeExprKind::FieldListHole | TypeExprKind::UnionHole => {}
        TypeExprKind::Vector(elem, figura) => {
            resolve_type_expr(resolver, interner, elem, errors);
            if vector_width_is_bare_count(figura) {
                errors.push(lowering_error(
                    crate::numeric_width::WidthLowerIssue::VECTOR_WIDTH_NEEDS_SIZE,
                    ty.span,
                ));
            }
            resolve_figura_expr(resolver, interner, figura, errors);
        }
        TypeExprKind::Tensor(elem, figura)
        | TypeExprKind::Matrix(elem, figura)
        | TypeExprKind::Sparsa(elem, figura) => {
            resolve_type_expr(resolver, interner, elem, errors);
            resolve_figura_expr(resolver, interner, figura, errors);
        }
        TypeExprKind::Atomic(elem) => resolve_type_expr(resolver, interner, elem, errors),
        TypeExprKind::PromissumFailable(success, alternate) => {
            resolve_type_expr(resolver, interner, success, errors);
            resolve_type_expr(resolver, interner, alternate, errors);
        }
        TypeExprKind::Named(name, params) => {
            resolve_type_ident(resolver, interner, name, errors);
            let name_str = interner.resolve(name.name);
            if let Some(prim) = Primitive::from_name(name_str) {
                if matches!(prim, Primitive::Instans) {
                    for param in params {
                        validate_instans_precision_param(interner, param, errors);
                    }
                    return;
                }
                if matches!(prim, Primitive::Ascii | Primitive::Textus) {
                    match params.as_slice() {
                        [] => {}
                        [capacity]
                            if prim == Primitive::Ascii
                                && named_type_is_ascii_format_hint(capacity, interner) =>
                        {
                            errors.push(lowering_error(
                                ISSUE_ASCII_FORMAT_HINT_NOT_CAPACITY,
                                generic_arg_span(capacity),
                            ));
                        }
                        [capacity] => {
                            validate_count_param(
                                resolver,
                                interner,
                                capacity,
                                crate::numeric_width::WidthLowerIssue::CAPACITY_NEEDS_SIZE,
                                errors,
                            );
                        }
                        _ => errors.push(lowering_error(ISSUE_PRIMITIVE_PARAMS, name.span)),
                    }
                    return;
                }
            }
            if name_str == "intervallum" {
                // The first argument is the bound type; the optional second is
                // the compile-time step, which the type-lowering sites validate
                // by shape. A bare name there is not an unknown type — it is
                // `intervallum_step_must_be_constant`.
                if let Some(bound) = params.first() {
                    resolve_generic_arg(resolver, interner, bound, errors);
                }
                return;
            }
            if let Some(collection) = CollectionKind::from_name(name_str) {
                match collection {
                    CollectionKind::Lista | CollectionKind::Queue | CollectionKind::Stack => {
                        match params.as_slice() {
                            [elem] => resolve_generic_arg(resolver, interner, elem, errors),
                            // A count range, `lista<T, magnitudo lo‥hi>`, is not a
                            // capacity: its ends fold when the type lowers. A bare
                            // range bounds a value, so a list refuses it by name.
                            [elem, GenericArgExpr::Range(range)]
                                if collection == CollectionKind::Lista =>
                            {
                                resolve_generic_arg(resolver, interner, elem, errors);
                                if !range.sized {
                                    errors.push(lowering_error(
                                        crate::numeric_width::WidthLowerIssue::LIST_COUNT_NEEDS_SIZE,
                                        range.span,
                                    ));
                                }
                            }
                            [elem, capacity] if collection == CollectionKind::Lista => {
                                resolve_generic_arg(resolver, interner, elem, errors);
                                validate_count_param(
                                    resolver,
                                    interner,
                                    capacity,
                                    crate::numeric_width::WidthLowerIssue::LIST_COUNT_NEEDS_SIZE,
                                    errors,
                                );
                            }
                            [elem, capacity] => {
                                resolve_generic_arg(resolver, interner, elem, errors);
                                validate_count_param(
                                    resolver,
                                    interner,
                                    capacity,
                                    crate::numeric_width::WidthLowerIssue::CAPACITY_NEEDS_SIZE,
                                    errors,
                                );
                            }
                            _ => {
                                for param in params {
                                    resolve_generic_arg(resolver, interner, param, errors);
                                }
                            }
                        }
                    }
                    _ => {
                        for param in params {
                            resolve_generic_arg(resolver, interner, param, errors);
                        }
                    }
                }
                return;
            }
            // Tensor-family heads report incomplete/shape errors after their
            // arguments resolve, so unknown element types are reported too.
            let family_issues = match name_str {
                "tensor" => Some((ISSUE_TENSOR_INCOMPLETE, ISSUE_TENSOR_MISSING_FIGURA)),
                "sparsa" => Some((ISSUE_SPARSA_INCOMPLETE, ISSUE_SPARSA_MISSING_FIGURA)),
                "atomic" => Some((ISSUE_ATOMIC_INCOMPLETE, ISSUE_ATOMIC_ARITY)),
                "vector" => Some((ISSUE_VECTOR_INCOMPLETE, ISSUE_VECTOR_MISSING_WIDTH)),
                "matrix" => Some((ISSUE_MATRIX_INCOMPLETE, ISSUE_MATRIX_MISSING_FIGURA)),
                _ => None,
            };
            if let Some((incomplete, missing_shape)) = family_issues {
                for param in params {
                    resolve_generic_arg(resolver, interner, param, errors);
                }
                let issue = if params.is_empty() {
                    incomplete
                } else {
                    missing_shape
                };
                errors.push(type_mismatch_error(issue, name.span));
                return;
            }
            for param in params {
                resolve_generic_arg(resolver, interner, param, errors);
            }
        }
        TypeExprKind::Qualified(path, params) => {
            resolve_qualified_type_path(resolver, interner, path, errors);
            for param in params {
                resolve_generic_arg(resolver, interner, param, errors);
            }
        }
        TypeExprKind::Func(func) => {
            for param in &func.params {
                resolve_type_expr(resolver, interner, param, errors);
            }
            resolve_type_expr(resolver, interner, &func.ret, errors);
            // The error type in a Func type expression must be name-resolved
            // too; otherwise an unknown type name in the error position is
            // silently accepted.
            if let Some(err_ty) = &func.err {
                resolve_type_expr(resolver, interner, err_ty, errors);
            }
        }
        TypeExprKind::Union(members) | TypeExprKind::Intersection(members) => {
            for m in members {
                resolve_type_expr(resolver, interner, m, errors);
            }
        }
    }
}

fn resolve_generic_arg(
    resolver: &mut Resolver,
    interner: &Interner,
    arg: &GenericArgExpr,
    errors: &mut Vec<SemanticError>,
) {
    match arg {
        GenericArgExpr::Type(ty) => match &ty.kind {
            // A bare named argument may bind a magnitudo/index slot, so it
            // validates leniently exactly like the `Name` shape.
            TypeExprKind::Named(ident, nested) if nested.is_empty() => {
                resolve_named_generic_arg_ident(resolver, interner, ident, errors);
            }
            _ => resolve_type_expr(resolver, interner, ty, errors),
        },
        GenericArgExpr::Size(figura) => resolve_figura_expr(resolver, interner, figura, errors),
        GenericArgExpr::Name(ident) => {
            resolve_named_generic_arg_ident(resolver, interner, ident, errors);
        }
        // A range bound that is a name is refused when the type lowers
        // (`ranged_policy_bound_not_constant`); nothing to resolve here.
        GenericArgExpr::Range(_) => {}
    }
}

fn resolve_named_generic_arg_ident(
    resolver: &mut Resolver,
    interner: &Interner,
    ident: &radix_syntax::Ident,
    errors: &mut Vec<SemanticError>,
) {
    if is_builtin_type(interner.resolve(ident.name)) {
        return;
    }
    let Some(def_id) = resolver.lookup(ident.name) else {
        errors.push(unknown_type_name_error(
            interner.resolve(ident.name),
            ident.span,
        ));
        return;
    };
    let Some(symbol) = resolver.get_symbol(def_id) else {
        errors.push(undefined_type_error(ISSUE_UNKNOWN_TYPE, ident.span));
        return;
    };
    if !matches!(
        symbol.kind,
        SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Interface
            | SymbolKind::TypeAlias
            | SymbolKind::TypeParam
            | SymbolKind::IndexParam
            | SymbolKind::Schema
    ) {
        errors.push(undefined_type_error(ISSUE_NAME_NOT_TYPE, ident.span));
    }
}

fn resolve_figura_expr(
    resolver: &mut Resolver,
    interner: &Interner,
    figura: &radix_syntax::FiguraExpr,
    errors: &mut Vec<SemanticError>,
) {
    match figura {
        radix_syntax::FiguraExpr::Infer | radix_syntax::FiguraExpr::Literal(_) => {}
        radix_syntax::FiguraExpr::Add(value, _)
        | radix_syntax::FiguraExpr::Subtract(value, _)
        | radix_syntax::FiguraExpr::Divide(value, _) => {
            resolve_figura_expr(resolver, interner, value, errors);
        }
        radix_syntax::FiguraExpr::Tuple(dims) => {
            for dim in dims {
                resolve_figura_expr(resolver, interner, dim, errors);
            }
        }
        radix_syntax::FiguraExpr::Var(ident) => {
            let Some(def_id) = resolver.lookup(ident.name) else {
                errors.push(undefined_type_error(ISSUE_FIGURA_UNKNOWN_INDEX, ident.span));
                return;
            };
            let Some(symbol) = resolver.get_symbol(def_id) else {
                return;
            };
            // An index parameter remains a compile-time term. A local or
            // formal value can also supply an extent at runtime; lowering
            // records its binding/parameter identity for boundary checks.
            if !matches!(
                symbol.kind,
                SymbolKind::IndexParam | SymbolKind::Local | SymbolKind::Param
            ) {
                errors.push(type_mismatch_error(
                    ISSUE_FIGURA_NOT_INDEX_PARAM,
                    ident.span,
                ));
            }
        }
        radix_syntax::FiguraExpr::Lift(extent) => {
            resolve_lift_extent(resolver, interner, extent, errors);
        }
    }
}

/// Resolve a spelled size lift payload. The lift is a store: a negative
/// literal fails at compile time (`size_lift_negative`), mirroring the
/// size-declaration check.
fn resolve_lift_extent(
    resolver: &mut Resolver,
    interner: &Interner,
    extent: &radix_syntax::LiftExtent,
    errors: &mut Vec<SemanticError>,
) {
    match extent {
        radix_syntax::LiftExtent::Literal(_) => {}
        radix_syntax::LiftExtent::Negative { span, .. } => {
            errors.push(
                SemanticError::structured(SemanticErrorKind::ConversionInvalid, *span)
                    .with_arg("issue", "size_lift_negative"),
            );
        }
        radix_syntax::LiftExtent::Name(ident) => {
            resolve_figura_expr(
                resolver,
                interner,
                &radix_syntax::FiguraExpr::Var(ident.clone()),
                errors,
            );
        }
        radix_syntax::LiftExtent::Paren(term) => {
            resolve_figura_expr(resolver, interner, term, errors);
        }
        radix_syntax::LiftExtent::Shape(dims) => {
            for dim in dims {
                resolve_figura_expr(resolver, interner, dim, errors);
            }
        }
    }
}

fn generic_arg_span(arg: &GenericArgExpr) -> radix_lexer::Span {
    match arg {
        GenericArgExpr::Type(ty) => ty.span,
        GenericArgExpr::Size(FiguraExpr::Infer | FiguraExpr::Literal(_)) => {
            radix_lexer::Span::default()
        }
        GenericArgExpr::Size(FiguraExpr::Var(ident)) | GenericArgExpr::Name(ident) => ident.span,
        GenericArgExpr::Range(range) => range.span,
        GenericArgExpr::Size(FiguraExpr::Lift(extent)) => lift_extent_span(extent),
        GenericArgExpr::Size(
            FiguraExpr::Add(value, _)
            | FiguraExpr::Subtract(value, _)
            | FiguraExpr::Divide(value, _),
        ) => generic_figura_span(value),
        GenericArgExpr::Size(FiguraExpr::Tuple(dims)) => {
            dims.first().map(generic_figura_span).unwrap_or_default()
        }
    }
}

fn generic_figura_span(figura: &FiguraExpr) -> radix_lexer::Span {
    match figura {
        FiguraExpr::Infer | FiguraExpr::Literal(_) => radix_lexer::Span::default(),
        FiguraExpr::Var(ident) => ident.span,
        FiguraExpr::Lift(extent) => lift_extent_span(extent),
        FiguraExpr::Add(value, _)
        | FiguraExpr::Subtract(value, _)
        | FiguraExpr::Divide(value, _) => generic_figura_span(value),
        FiguraExpr::Tuple(dims) => dims.first().map(generic_figura_span).unwrap_or_default(),
    }
}

fn lift_extent_span(extent: &radix_syntax::LiftExtent) -> radix_lexer::Span {
    match extent {
        radix_syntax::LiftExtent::Name(ident) => ident.span,
        radix_syntax::LiftExtent::Negative { span, .. } => *span,
        radix_syntax::LiftExtent::Literal(_) => radix_lexer::Span::default(),
        radix_syntax::LiftExtent::Paren(term) => generic_figura_span(term),
        radix_syntax::LiftExtent::Shape(dims) => {
            dims.first().map(generic_figura_span).unwrap_or_default()
        }
    }
}

fn generic_arg_named_shape(arg: &GenericArgExpr) -> Option<(&radix_syntax::Ident, bool)> {
    match arg {
        GenericArgExpr::Name(ident) => Some((ident, false)),
        GenericArgExpr::Type(ty) => match &ty.kind {
            TypeExprKind::Named(ident, nested) => Some((ident, !nested.is_empty())),
            _ => None,
        },
        GenericArgExpr::Size(_) | GenericArgExpr::Range(_) => None,
    }
}

fn validate_instans_precision_param(
    interner: &Interner,
    param: &GenericArgExpr,
    errors: &mut Vec<SemanticError>,
) {
    let Some((ident, nested)) = generic_arg_named_shape(param) else {
        if matches!(param, GenericArgExpr::Type(ty) if matches!(ty.kind, TypeExprKind::Infer)) {
            return;
        }
        errors.push(undefined_type_error(
            ISSUE_PRECISION_MARKER_SHAPE,
            generic_arg_span(param),
        ));
        return;
    };
    if nested {
        errors.push(undefined_type_error(
            ISSUE_PRECISION_MARKER_PARAMS,
            generic_arg_span(param),
        ));
        return;
    }
    let marker = interner.resolve(ident.name);
    if let Err(err) = crate::InstansPrecision::from_marker(marker) {
        errors.push(instans_precision_marker_error(err, ident.span));
    }
}

fn named_type_is_ascii_format_hint(param: &GenericArgExpr, interner: &Interner) -> bool {
    generic_arg_named_shape(param).is_some_and(|(ident, nested)| {
        !nested && crate::numeric_width::is_ascii_format_hint(interner.resolve(ident.name))
    })
}

/// A bounded head's count slot (`list<T, size 4>`, `queue<T, size 4>`,
/// `string<size 8>`): the capacity checks of every bounded head, plus the
/// `size` marker each requires. A bare number or name (`queue<T, 4>`) is
/// retired by name with `needs_size`; the `_` hole stays bare until its own
/// need moves it.
fn validate_count_param(
    resolver: &Resolver,
    interner: &Interner,
    param: &GenericArgExpr,
    needs_size: &'static str,
    errors: &mut Vec<SemanticError>,
) {
    let issue = match param {
        // A spelled literal count is the sugar for `size 0…N`.
        GenericArgExpr::Size(FiguraExpr::Lift(extent))
            if matches!(**extent, radix_syntax::LiftExtent::Literal(_)) =>
        {
            return;
        }
        _ => capacity_index_issue(param, resolver, interner),
    };
    let issue = issue.or_else(|| {
        let marked_or_hole = matches!(
            param,
            GenericArgExpr::Size(FiguraExpr::Lift(_) | FiguraExpr::Infer)
                | GenericArgExpr::Type(TypeExpr {
                    kind: TypeExprKind::Infer,
                    ..
                })
        );
        (!marked_or_hole).then_some(needs_size)
    });
    if let Some(issue) = issue {
        let span = generic_arg_span(param);
        errors.push(if issue == needs_size {
            lowering_error(issue, span)
        } else {
            undefined_type_error(issue, span)
        });
    }
}

fn capacity_index_issue(
    param: &GenericArgExpr,
    resolver: &Resolver,
    interner: &Interner,
) -> Option<&'static str> {
    let (ident, nested) = match param {
        GenericArgExpr::Size(FiguraExpr::Infer | FiguraExpr::Literal(_))
        | GenericArgExpr::Type(TypeExpr {
            kind: TypeExprKind::Infer,
            ..
        }) => return None,
        GenericArgExpr::Size(FiguraExpr::Var(ident)) | GenericArgExpr::Name(ident) => {
            (ident, false)
        }
        GenericArgExpr::Size(FiguraExpr::Lift(extent)) => match &**extent {
            // A spelled lift as a capacity behaves like its name payload.
            radix_syntax::LiftExtent::Name(ident) => (ident, false),
            _ => return Some(ISSUE_BOUNDED_CAPACITY_NOT_NATURAL),
        },
        GenericArgExpr::Size(FiguraExpr::Tuple(_)) | GenericArgExpr::Range(_) => {
            return Some(ISSUE_BOUNDED_CAPACITY_NOT_NATURAL);
        }
        GenericArgExpr::Type(TypeExpr {
            kind: TypeExprKind::Named(ident, nested),
            ..
        }) => (ident, !nested.is_empty()),
        GenericArgExpr::Size(
            FiguraExpr::Add(_, _) | FiguraExpr::Subtract(_, _) | FiguraExpr::Divide(_, _),
        )
        | GenericArgExpr::Type(_) => return Some(ISSUE_BOUNDED_CAPACITY_NOT_NATURAL),
    };
    if nested {
        return Some(ISSUE_BOUNDED_CAPACITY_NOT_NATURAL);
    }
    let name = interner.resolve(ident.name);
    if radix_types::parse_natural_capacity(name).is_some() {
        return None;
    }
    let Some(def_id) = resolver.lookup(ident.name) else {
        return Some(ISSUE_BOUNDED_CAPACITY_NOT_NATURAL);
    };
    let Some(symbol) = resolver.get_symbol(def_id) else {
        return Some(ISSUE_BOUNDED_CAPACITY_NOT_NATURAL);
    };
    match symbol.kind {
        SymbolKind::IndexParam => None,
        SymbolKind::TypeParam => Some(ISSUE_FIGURA_TYPE_PARAM_IN_INDEX),
        _ => Some(ISSUE_BOUNDED_CAPACITY_NOT_NATURAL),
    }
}

/// Unresolved type name: the removed `d32` spelling gets its own issue.
fn unknown_type_name_error(name: &str, span: radix_lexer::Span) -> SemanticError {
    let issue = if name == crate::numeric_width::REMOVED_D32_MARKER {
        WidthLowerIssue::REMOVED_DECIMAL_WIDTH
    } else {
        ISSUE_UNKNOWN_TYPE
    };
    undefined_type_error(issue, span)
}

fn undefined_type_error(issue: &'static str, span: radix_lexer::Span) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::TypeUndefined, span).with_arg("issue", issue)
}

fn instans_precision_marker_error(
    err: InstansPrecisionError,
    span: radix_lexer::Span,
) -> SemanticError {
    let issue = match err {
        InstansPrecisionError::UnknownMarker => ISSUE_INVALID_PRECISION_MARKER,
        InstansPrecisionError::TooManyParams => ISSUE_PRECISION_MARKER_PARAMS,
    };
    undefined_type_error(issue, span)
}

fn type_mismatch_error(issue: &'static str, span: radix_lexer::Span) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::TypeMismatch, span).with_arg("issue", issue)
}

fn lowering_error(issue: &'static str, span: radix_lexer::Span) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::LoweringError, span).with_arg("issue", issue)
}

fn lowering_collection_arity_error(
    collection: CollectionKind,
    span: radix_lexer::Span,
) -> SemanticError {
    lowering_error(collection_arity_issue(collection), span)
}

fn collection_arity_issue(collection: CollectionKind) -> &'static str {
    match collection {
        CollectionKind::Lista => "lista_arity",
        CollectionKind::Tabula => "tabula_arity",
        CollectionKind::Tabulae => "tabulae_arity",
        CollectionKind::Copia => "copia_arity",
        CollectionKind::Promissum => "promissum_arity",
        CollectionKind::Cursor => "cursor_arity",
        CollectionKind::Queue => "queue_arity",
        CollectionKind::Stack => "stack_arity",
    }
}

fn resolve_conversio_target_type(
    resolver: &mut Resolver,
    interner: &Interner,
    ty: &TypeExpr,
    errors: &mut Vec<SemanticError>,
) {
    match &ty.kind {
        TypeExprKind::Named(name, params)
            if let Some(prim) = Primitive::from_name(interner.resolve(name.name)) =>
        {
            resolve_type_ident(resolver, interner, name, errors);
            // WHY: `numerus`/`fractus` arguments are width markers exactly as
            // in a declaration, so they take the declaration's validation
            // (an unexplained extra argument is `invalid_*_width_marker`).
            // A convert hint is a `via` clause, never a type argument
            // (D11.9), so no other primitive's arguments are skipped for that.
            if !params.is_empty() && !matches!(prim, Primitive::Numerus | Primitive::Fractus) {
                return;
            }
        }
        _ => {}
    }

    resolve_type_expr(resolver, interner, ty, errors);
}

fn resolve_type_ident(
    resolver: &mut Resolver,
    interner: &Interner,
    ident: &radix_syntax::Ident,
    errors: &mut Vec<SemanticError>,
) {
    let name = interner.resolve(ident.name);

    if is_builtin_type(name) {
        return;
    }

    let Some(def_id) = resolver.lookup_type(ident.name) else {
        if resolver.lookup_selective_import_type(ident.name).is_some() {
            // A selective type binding used in type position counts as a
            // use of the import, like a namespace named by a qualified path.
            resolver.mark_namespace_used(ident.name);
        } else {
            errors.push(unknown_type_name_error(
                interner.resolve(ident.name),
                ident.span,
            ));
        }
        return;
    };

    let Some(symbol) = resolver.get_symbol(def_id) else {
        errors.push(undefined_type_error(ISSUE_UNKNOWN_TYPE, ident.span));
        return;
    };

    match symbol.kind {
        SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Interface
        | SymbolKind::TypeAlias
        | SymbolKind::TypeParam
        | SymbolKind::Schema => {}
        _ => errors.push(undefined_type_error(ISSUE_NAME_NOT_TYPE, ident.span)),
    }
}

fn resolve_qualified_type_path(
    resolver: &mut Resolver,
    interner: &Interner,
    path: &[radix_syntax::Ident],
    errors: &mut Vec<SemanticError>,
) {
    let Some((namespace, type_name)) = path.first().zip(path.last()) else {
        return;
    };
    // Middle segments (indices 1..len-1) must be Module-kind bindings, not
    // just successful lookups.
    for segment in &path[1..path.len().saturating_sub(1)] {
        let Some(def_id) = resolver.lookup(segment.name) else {
            errors.push(undefined_type_error(ISSUE_UNKNOWN_NAMESPACE, segment.span));
            return;
        };
        if !matches!(
            resolver.get_symbol(def_id).map(|symbol| symbol.kind),
            Some(SymbolKind::Module)
        ) {
            errors.push(undefined_type_error(
                ISSUE_QUALIFIED_TYPE_MIDDLE_NOT_NAMESPACE,
                segment.span,
            ));
            return;
        }
    }
    let Some(namespace_def_id) = resolver.lookup(namespace.name) else {
        errors.push(undefined_type_error(
            ISSUE_UNKNOWN_NAMESPACE,
            namespace.span,
        ));
        return;
    };
    if !matches!(
        resolver
            .get_symbol(namespace_def_id)
            .map(|symbol| symbol.kind),
        Some(SymbolKind::Module)
    ) {
        errors.push(undefined_type_error(
            ISSUE_QUALIFIED_TYPE_PREFIX_NOT_NAMESPACE,
            namespace.span,
        ));
        return;
    }
    resolver.mark_namespace_used(namespace.name);
    if !resolver.namespace_exports_member(namespace.name, type_name.name) {
        if let Some(issue) =
            resolver.namespace_member_rejection_issue(namespace.name, type_name.name)
        {
            errors.push(crate::passes::import_seam::diagnostic(
                type_name.span,
                issue,
                interner.resolve(type_name.name),
            ));
        } else {
            errors.push(undefined_type_error(
                ISSUE_QUALIFIED_TYPE_NOT_EXPORTED,
                type_name.span,
            ));
        }
        return;
    }
    if resolver
        .namespace_file_type(namespace.name, type_name.name)
        .is_some()
    {
        return;
    }
    errors.push(undefined_type_error(
        ISSUE_UNKNOWN_QUALIFIED_TYPE,
        type_name.span,
    ));
}

/// Resolve a member expression.
///
/// `allow_struct_base` lets a genus name stand as the base of a read
/// (`Tab.Z`): the resolver keeps no field table, so typecheck decides from the
/// field's modifier whether the member is a static field and names the misuse
/// otherwise. A call callee passes `false` and keeps `name_not_value`.
fn resolve_member_expr(
    resolver: &mut Resolver,
    interner: &Interner,
    expr: &radix_syntax::MemberExpr,
    allow_struct_base: bool,
    errors: &mut Vec<SemanticError>,
) {
    // Enum-variant paths (e.g. `Color.rubrum`) must be accepted in
    // expression position even though the bare enum name is not a
    // value. Mirror the HIR `lower_membrum` logic.
    if let ExprKind::Ident(base) = &expr.object.kind
        && resolve_enum_variant_member(resolver, base, &expr.member, errors)
    {
        return;
    }
    // A value read through an import binding (`module.NAME`, `module.Tab.Z`) is
    // a use of the import, even when later folding or lowering drops the
    // namespace path from the HIR.
    if let ExprKind::Ident(base) = &expr.object.kind
        && let Some(def_id) = resolver.lookup(base.name)
        && resolver
            .get_symbol(def_id)
            .is_some_and(|symbol| symbol.kind == SymbolKind::Module)
    {
        resolver.mark_namespace_used(base.name);
    }
    if allow_struct_base
        && let ExprKind::Ident(base) = &expr.object.kind
        && let Some(def_id) = resolver.lookup(base.name)
        && resolver
            .get_symbol(def_id)
            .is_some_and(|symbol| symbol.kind == SymbolKind::Struct)
    {
        return;
    }
    resolve_expr(resolver, interner, &expr.object, errors);
    // Strict library-pack surfaces are total for the library they
    // define: only the listed surface spellings resolve at the source
    // boundary. Post-HIR canonical lookups could resolve the installed
    // member, so reject the non-listed spelling here (SEM010).
    if let ExprKind::Ident(base) = &expr.object.kind
        && let Some(def_id) = resolver.lookup(base.name)
        && let Some(symbol) = resolver.get_symbol(def_id)
        && symbol.kind == SymbolKind::Module
        && !resolver.namespace_member_surface_valid(symbol.name, expr.member.name)
    {
        errors.push(
            SemanticError::structured(SemanticErrorKind::MemberUndefined, expr.member.span)
                .with_arg("issue", ISSUE_NAMESPACE_MISSING_EXPORT)
                .with_arg("member", interner.resolve(expr.member.name).to_owned()),
        );
    }
}

fn resolve_enum_variant_member(
    resolver: &mut Resolver,
    enum_ident: &radix_syntax::Ident,
    variant_ident: &radix_syntax::Ident,
    errors: &mut Vec<SemanticError>,
) -> bool {
    let Some(enum_def) = resolver.lookup(enum_ident.name) else {
        return false;
    };
    let Some(enum_symbol) = resolver.get_symbol(enum_def) else {
        return false;
    };
    if enum_symbol.kind != SymbolKind::Enum {
        return false;
    }

    if resolver
        .variant_child(enum_def, variant_ident.name)
        .is_some()
    {
        return true;
    }

    // `A.X` where the suffix is not one of A's variants: if the bare name is
    // a variant of several unions, the ambiguity is the honest diagnostic;
    // otherwise fall back to the existing bare-table + unknown-variant path.
    if matches!(
        resolver.lookup_variant_bare(variant_ident.name),
        BareVariantLookup::Ambiguous { .. }
    ) {
        errors.push(
            SemanticError::structured(SemanticErrorKind::VariableUndefined, variant_ident.span)
                .with_arg("issue", ISSUE_VARIANT_AMBIGUOUS),
        );
        return true;
    }

    let Some(variant_def) = resolver.lookup(variant_ident.name) else {
        errors.push(
            SemanticError::structured(SemanticErrorKind::VariableUndefined, variant_ident.span)
                .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
        );
        return true;
    };
    let Some(variant_symbol) = resolver.get_symbol(variant_def) else {
        errors.push(
            SemanticError::structured(SemanticErrorKind::VariableUndefined, variant_ident.span)
                .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
        );
        return true;
    };
    if variant_symbol.kind != SymbolKind::Variant {
        errors.push(
            SemanticError::structured(SemanticErrorKind::VariableUndefined, variant_ident.span)
                .with_arg("issue", ISSUE_NAME_NOT_VARIANT),
        );
        return true;
    }
    errors.push(
        SemanticError::structured(SemanticErrorKind::VariableUndefined, variant_ident.span)
            .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
    );
    true
}

/// Resolve the variant path of a `finge` construction.
///
/// A single-segment path (`finge Bonum { … }`) resolves exactly as before.
/// A two-segment path (`finge pkg.Bonum { … }` or `finge Exitus.Bonum { … }`)
/// resolves the terminal segment against the penultimate segment's union when
/// it names one. A path with leading path segments (`finge a.A.X { … }`)
/// resolves the leading path as a real qualifier before anything else: a
/// qualified imported parent resolves through the imported union's parent
/// link, and a prefix that is not a real qualifier fails closed —
/// `missing.A.X` must never silently resolve as the local `A.X`.
fn resolve_variant_path(
    resolver: &mut Resolver,
    path: &[radix_syntax::Ident],
    errors: &mut Vec<SemanticError>,
) {
    let Some((last, init)) = path.split_last() else {
        return;
    };
    if path.len() == 1 {
        return resolve_variant_ident(resolver, last, errors);
    }
    if path.len() >= 3 {
        // The leading path must resolve as a real qualifier before the
        // penultimate segment can name the parent enum/union — a
        // construction like `finge missing.A.X { … }` must never silently
        // resolve as the local `A.X`.
        let symbols: Vec<radix_lexer::Symbol> = path.iter().map(|ident| ident.name).collect();
        match resolver.qualified_variant_path(&symbols) {
            QualifiedVariantPathLookup::Variant(_) => return,
            QualifiedVariantPathLookup::UnknownVariant
            | QualifiedVariantPathLookup::NotQualified => {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::VariableUndefined, last.span)
                        .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
                );
                return;
            }
        }
    }
    let Some(parent) = init.last() else {
        return;
    };
    let parent_enum = resolver.lookup(parent.name).and_then(|parent_def| {
        resolver
            .get_symbol(parent_def)
            .filter(|symbol| symbol.kind == SymbolKind::Enum)
            .map(|_| parent_def)
    });
    if let Some(parent_enum) = parent_enum {
        if resolver.variant_child(parent_enum, last.name).is_none() {
            report_non_variant_suffix(resolver, last, errors);
        }
    } else {
        // The penultimate segment names no ordinary Enum binding. A qualified
        // imported parent (`a.A.X`) resolves BEFORE any bare-variant fallback;
        // paths that are not qualified imported variant paths keep the
        // legacy bare-table / per-union policy.
        resolve_variant_path_without_enum_parent(resolver, path, last, errors);
    }
}

fn resolve_variant_ident(
    resolver: &mut Resolver,
    ident: &radix_syntax::Ident,
    errors: &mut Vec<SemanticError>,
) {
    let Some(def_id) = resolver.lookup(ident.name) else {
        // Variants no longer occupy the bare scope table: consult the
        // per-union namespace. Exactly one defining union resolves; two or
        // more is an explicit ambiguity, never a silent pick.
        match resolver.lookup_variant_bare(ident.name) {
            BareVariantLookup::Single(_) => return,
            BareVariantLookup::Ambiguous { .. } => {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
                        .with_arg("issue", ISSUE_VARIANT_AMBIGUOUS),
                );
                return;
            }
            BareVariantLookup::None => {}
        }
        errors.push(
            SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
                .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
        );
        return;
    };

    let Some(symbol) = resolver.get_symbol(def_id) else {
        errors.push(
            SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
                .with_arg("issue", ISSUE_UNKNOWN_VARIANT),
        );
        return;
    };

    if symbol.kind != SymbolKind::Variant {
        errors.push(
            SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
                .with_arg("issue", ISSUE_NAME_NOT_VARIANT),
        );
    }
}

fn undefined_identifier_error(
    resolver: &Resolver,
    interner: &Interner,
    ident: &radix_syntax::Ident,
) -> SemanticError {
    let spelling = interner.resolve(ident.name);
    if resolver.is_scribe_family_surface(spelling) {
        return SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
            .with_arg("issue", ISSUE_SCRIBE_CALLEE)
            .with_arg("spelling", spelling.to_owned());
    }

    let mut error = SemanticError::structured(SemanticErrorKind::VariableUndefined, ident.span)
        .with_arg("issue", ISSUE_UNKNOWN_IDENTIFIER);

    if let Some(suggestion) = semantic_name_suggestion(resolver, interner, spelling) {
        error = error
            .with_arg("spelling", spelling.to_owned())
            .with_arg("suggestion", suggestion);
    }

    error
}

struct NameSuggestionCandidate<'a> {
    spelling: &'a str,
    distance: usize,
}

/// Fold an interned symbol's spelling for the per-Symbol suggestion cache.
///
/// Runs only on a cache miss, i.e. at most once per distinct symbol per
/// resolver lifetime, because the caller serves folds from
/// `Resolver::name_suggestion_folds` (the fold is a pure function of the
/// interned string, so the cache never goes stale across scope changes).
fn fold_interned_symbol(interner: &Interner, name: radix_lexer::Symbol) -> String {
    name_suggestion_key(interner.resolve(name))
}

pub(super) fn semantic_name_suggestion(
    resolver: &Resolver,
    interner: &Interner,
    spelling: &str,
) -> Option<String> {
    let folded = name_suggestion_key(spelling);

    // Candidate folds are served from the per-Symbol cache (a pure function of
    // the interned string, so the cache never goes stale across scope changes)
    // and candidate spellings are borrowed from the interner, so the candidate
    // loop allocates no owned Strings. Exact fold match is O(1) via the map;
    // edit-distance near-miss still scans the pre-folded candidate list.
    //
    // Populate the cache in a first pass (mutation only, no held borrows);
    // the second pass reads the folds back as `&str` so each distinct symbol
    // normalizes at most once per resolver lifetime.
    let visible = resolver.visible_value_names();
    let mut folds = resolver.name_suggestion_folds.borrow_mut();
    for &name in &visible {
        let candidate: &str = interner.resolve(name);
        if candidate == spelling {
            continue;
        }
        folds
            .entry(name)
            .or_insert_with(|| fold_interned_symbol(interner, name));
    }

    let mut exact_by_fold: handroll::FxHashMap<&str, &str> = handroll::FxHashMap::default();
    let mut folded_candidates: Vec<(&str, &str)> = Vec::new();
    for &name in &visible {
        let candidate: &str = interner.resolve(name);
        if candidate == spelling {
            continue;
        }
        let candidate_fold: &str = folds
            .get(&name)
            .expect("visible-name fold cached in the first pass");
        exact_by_fold
            .entry(candidate_fold)
            .and_modify(|best| {
                if candidate < *best {
                    *best = candidate;
                }
            })
            .or_insert(candidate);
        folded_candidates.push((candidate_fold, candidate));
    }

    if let Some(&exact) = exact_by_fold.get(folded.as_str()) {
        return Some(exact.to_owned());
    }

    let input_len = folded.chars().count();
    folded_candidates
        .into_iter()
        .filter_map(|(candidate_fold, candidate)| {
            let threshold = name_suggestion_distance_threshold(candidate_fold);
            let candidate_len = candidate_fold.chars().count();
            if input_len.abs_diff(candidate_len) > threshold {
                return None;
            }
            let distance = edit_distance(&folded, candidate_fold);
            (distance <= threshold).then_some(NameSuggestionCandidate {
                spelling: candidate,
                distance,
            })
        })
        .min_by(|left, right| {
            left.distance
                .cmp(&right.distance)
                .then_with(|| left.spelling.cmp(right.spelling))
        })
        .map(|candidate| candidate.spelling.to_owned())
}

fn name_suggestion_key(text: &str) -> String {
    text.nfkc()
        .flat_map(|ch| ch.to_string().nfd().collect::<Vec<_>>())
        .filter(|ch| !is_combining_mark(*ch))
        .flat_map(char::to_lowercase)
        .collect()
}

fn name_suggestion_distance_threshold(folded_spelling: &str) -> usize {
    if folded_spelling.chars().count() <= 4 {
        1
    } else {
        2
    }
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];

    for (left_index, left_ch) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_ch) in right.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_ch != right_ch);
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            current[right_index + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right.len()]
}

fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "ascii"
            | "erratum"
            | "trapped"
            | "forma"
            | "scrinium"
            | "status"
            | "sermo"
            | "textus"
            | "numerus"
            | "modulus"
            | "saturatus"
            | "exactus"
            | "fractus"
            | "bivalens"
            | "nihil"
            | "vacuum"
            | "numquam"
            | "ignotum"
            | "littera"
            | "regex"
            | "objectum"
            | "quidlibet"
            | "lista"
            | "tabula"
            | "tabulae"
            | "copia"
            | "promissum"
            | "cursor"
            | "queue"
            | "stack"
            | "tensor"
            | "matrix"
            | "intervallum"
            | "iuncta"
            | "ratio"
            | "sparsa"
            | "series"
            | "json"
            | "valor"
            | "instans"
    ) || crate::numeric_width::is_numeric_width_type_sugar(name)
}

fn define_binding_pattern(
    resolver: &mut Resolver,
    pattern: &BindingPattern,
    mutable: bool,
    errors: &mut Vec<SemanticError>,
) {
    match pattern {
        BindingPattern::Ident(ident) => {
            define_symbol(
                resolver,
                ident.name,
                ident.span,
                SymbolKind::Local,
                mutable,
                errors,
            );
        }
        BindingPattern::Wildcard(_) => {}
        BindingPattern::Array {
            elements,
            is_rest: rest,
            ..
        } => {
            for element in elements {
                define_binding_pattern(resolver, element, mutable, errors);
            }
            if let Some(rest) = rest {
                define_symbol(
                    resolver,
                    rest.name,
                    rest.span,
                    SymbolKind::Local,
                    mutable,
                    errors,
                );
            }
        }
        BindingPattern::Object {
            fields,
            is_rest: rest,
            ..
        } => {
            for field in fields {
                let binding = field.alias.as_ref().unwrap_or(&field.name);
                define_symbol(
                    resolver,
                    binding.name,
                    binding.span,
                    SymbolKind::Local,
                    mutable,
                    errors,
                );
            }
            if let Some(rest) = rest {
                define_symbol(
                    resolver,
                    rest.name,
                    rest.span,
                    SymbolKind::Local,
                    mutable,
                    errors,
                );
            }
        }
    }
}

fn define_shader_stage_locals_for_resolution(
    resolver: &mut Resolver,
    interner: &Interner,
    annotations: &[radix_syntax::Annotation],
    span: radix_lexer::Span,
    errors: &mut Vec<SemanticError>,
) {
    let has_annotation = |family: &str| {
        annotations.iter().any(|annotation| match &annotation.kind {
            radix_syntax::AnnotationKind::Statement(stmt) => {
                interner.resolve(stmt.name.name) == family
            }
            radix_syntax::AnnotationKind::Braced(braced) => {
                interner.resolve(braced.name.name) == family
            }
            _ => false,
        })
    };

    if has_annotation("vertex") {
        for (name, mutable) in [("position", false), ("uv", false), ("out_position", true)] {
            if let Some(symbol) = interner.lookup(name) {
                define_symbol(resolver, symbol, span, SymbolKind::Local, mutable, errors);
            }
        }
    }
    if has_annotation("fragment") {
        for (name, mutable) in [
            ("normal", false),
            ("color", false),
            ("uv", false),
            ("out_color", true),
        ] {
            if let Some(symbol) = interner.lookup(name) {
                define_symbol(resolver, symbol, span, SymbolKind::Local, mutable, errors);
            }
        }
    }
}

fn record_iuncta_labels_for_name(
    resolver: &mut Resolver,
    interner: &Interner,
    name: radix_lexer::Symbol,
    ty: &TypeExpr,
) {
    if let Some(def_id) = resolver.lookup(name) {
        crate::tuple_labels::record_from_type_expr(resolver, interner, def_id, ty);
        record_subject_member_set(resolver, interner, def_id, ty);
    }
}

/// Record the closed-set members a binding's annotation names, keyed by the
/// binding identity, so a later `discerne` subject lookup can spell the set
/// even when no arm names a member correctly.
fn record_subject_member_set(
    resolver: &mut Resolver,
    interner: &Interner,
    def_id: DefId,
    ty: &TypeExpr,
) {
    let members = match &ty.kind {
        TypeExprKind::Union(members) => union_annotation_member_names(interner, members),
        TypeExprKind::Named(name, params) if params.is_empty() => resolver
            .lookup_type(name.name)
            .and_then(|parent| parent_variant_names(resolver, interner, parent)),
        _ => None,
    };
    if let Some(members) = members {
        resolver.record_subject_member_set(def_id, members);
    }
}

/// Source spellings of an explicit union annotation's members, or `None` when
/// no member is a nameable identifier. Qualified members contribute their
/// terminal spelling; family members keep the arm-derived fallback.
fn union_annotation_member_names(interner: &Interner, members: &[TypeExpr]) -> Option<Vec<String>> {
    let names: Vec<String> = members
        .iter()
        .filter_map(|member| match &member.kind {
            TypeExprKind::Named(ident, _) => Some(interner.resolve(ident.name).to_owned()),
            TypeExprKind::Qualified(path, _) => path
                .last()
                .map(|ident| interner.resolve(ident.name).to_owned()),
            _ => None,
        })
        .collect();
    (!names.is_empty()).then_some(names)
}

/// Variant spellings of one enum/discretio parent in declaration order, or
/// `None` when the parent is not a variant-bearing enum.
fn parent_variant_names(
    resolver: &Resolver,
    interner: &Interner,
    parent: DefId,
) -> Option<Vec<String>> {
    if resolver.get_symbol(parent).map(|symbol| symbol.kind) != Some(SymbolKind::Enum) {
        return None;
    }
    let mut variants: Vec<DefId> = resolver
        .variant_parents_all()
        .into_iter()
        .filter(|(_, owner)| *owner == parent)
        .map(|(variant, _)| variant)
        .collect();
    if variants.is_empty() {
        return None;
    }
    variants.sort_by_key(|variant| variant.0);
    Some(
        variants
            .into_iter()
            .filter_map(|variant| resolver.get_symbol(variant))
            .map(|symbol| interner.resolve(symbol.name).to_owned())
            .collect(),
    )
}

/// Member names the subject's own annotation names, when the subject is a
/// binding with a recorded closed-set annotation.
fn subject_member_set(resolver: &Resolver, subject: &Expr) -> Option<Vec<String>> {
    let ExprKind::Ident(ident) = &subject.kind else {
        return None;
    };
    let def_id = resolver.lookup(ident.name)?;
    resolver.subject_member_set(def_id).map(<[String]>::to_vec)
}

fn record_iuncta_labels_for_var(
    resolver: &mut Resolver,
    interner: &Interner,
    decl: &radix_syntax::VarDecl,
) {
    let Some(ty) = &decl.ty else {
        return;
    };
    let BindingPattern::Ident(ident) = &decl.binding else {
        return;
    };
    record_iuncta_labels_for_name(resolver, interner, ident.name, ty);
}

fn define_symbol(
    resolver: &mut Resolver,
    name: radix_lexer::Symbol,
    span: radix_lexer::Span,
    kind: SymbolKind,
    mutable: bool,
    errors: &mut Vec<SemanticError>,
) {
    let def_id = resolver.fresh_def_id();
    let symbol = Symbol {
        def_id,
        name,
        kind,
        ty: None,
        mutable,
        span,
    };

    if resolver.define(symbol).is_err() {
        errors.push(
            SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, span)
                .with_arg("issue", ISSUE_DUPLICATE_DEFINITION),
        );
    }
}

struct AliasEntry<'a> {
    def_id: DefId,
    type_params: &'a [TypeParam],
    ty: &'a TypeExpr,
    span: radix_lexer::Span,
}

enum TypeLowerError {
    UnboundAlias,
    Issue(&'static str),
    IssueWithArg {
        issue: &'static str,
        name: &'static str,
        value: &'static str,
    },
    CollectionArity(CollectionKind),
}

/// Register complete generic descriptors for named declarations after aliases
/// have been resolved. The resolver's kind rows predate declaration defaults;
/// this identity-keyed row lets HIR lowering apply the same ordered
/// explicit-prefix/default-suffix law used by callable binding.
fn register_named_generic_descriptors(
    program: &Program,
    resolver: &mut Resolver,
    interner: &Interner,
    types: &mut TypeTable,
    errors: &mut Vec<SemanticError>,
) {
    for stmt in &program.statements {
        let (name, params) = match &stmt.kind {
            StmtKind::TypeAlias(decl) => (&decl.name, &decl.type_params),
            StmtKind::Class(decl) => (&decl.name, &decl.type_params),
            StmtKind::Interface(decl) => (&decl.name, &decl.type_params),
            StmtKind::Union(decl) => (&decl.name, &decl.type_params),
            _ => continue,
        };
        let Some(def_id) = resolver.lookup(name.name) else {
            continue;
        };

        resolver.enter_scope(ScopeKind::Module);
        for param in params {
            define_generic_param(resolver, param, errors);
        }
        let mut descriptors = Vec::with_capacity(params.len());
        for param in params {
            let default = match &param.default {
                Some(radix_syntax::GenericParamDefault::Typus(ty)) => {
                    match lower_type_expr(ty, resolver, interner, types) {
                        Ok(default) => Some(SemanticGenericParamDefault::Typus(default)),
                        Err(error) => {
                            record_type_lowering_error(error, ty.span, errors);
                            None
                        }
                    }
                }
                Some(radix_syntax::GenericParamDefault::Magnitudo { value, .. }) => {
                    Some(SemanticGenericParamDefault::Magnitudo(*value))
                }
                None => None,
            };
            descriptors.push(GenericParamDescriptor {
                name: param.name.name,
                sort: match param.kind {
                    radix_syntax::GenericParamKind::Typus => GenericParamSort::Typus,
                    radix_syntax::GenericParamKind::Magnitudo => GenericParamSort::Magnitudo,
                },
                constraint: crate::TypeParamConstraint::Any,
                default,
            });
        }
        resolver.exit_scope();
        resolver.register_generic_param_descriptors(def_id, descriptors);
    }
}

fn record_type_lowering_error(
    error: TypeLowerError,
    span: radix_lexer::Span,
    errors: &mut Vec<SemanticError>,
) {
    match error {
        TypeLowerError::UnboundAlias => errors.push(lowering_error(ISSUE_TYPE_ALIAS_CYCLE, span)),
        TypeLowerError::Issue(issue) => errors.push(lowering_error(issue, span)),
        TypeLowerError::IssueWithArg { issue, name, value } => errors.push(
            SemanticError::structured(SemanticErrorKind::LoweringError, span)
                .with_arg("issue", issue)
                .with_arg(name, value),
        ),
        TypeLowerError::CollectionArity(collection) => {
            errors.push(lowering_collection_arity_error(collection, span));
        }
    }
}

fn resolve_alias_types(
    aliases: &[AliasEntry<'_>],
    resolver: &mut Resolver,
    interner: &Interner,
    types: &mut TypeTable,
    errors: &mut Vec<SemanticError>,
) {
    let mut pending: Vec<&AliasEntry<'_>> = aliases.iter().collect();

    // Alias symbols must carry concrete TypeIds before HIR/typecheck consume
    // them. A simple fixed point is enough because unresolved aliases are the
    // only recoverable dependency failure from `lower_type_expr`.
    while !pending.is_empty() {
        let mut next_pending = Vec::new();
        let mut progress = false;

        for alias in pending {
            // The alias's own type parameters are scoped during body lowering,
            // mirroring hir/lower/decl.rs::lower_typus (which push_type_params).
            // `resolve_stmt` entered and exited a module scope around the alias
            // earlier, so the params must be re-defined in a fresh module scope
            // here or `lower_type_expr` resolves the body's `Type::Param`
            // references against an out-of-scope symbol (SEM008).
            resolver.enter_scope(ScopeKind::Module);
            for param in alias.type_params {
                define_generic_param(resolver, param, errors);
            }
            let lowered = lower_type_expr(alias.ty, resolver, interner, types);
            resolver.exit_scope();

            match lowered {
                Ok(type_id) => {
                    if let Some(symbol) = resolver.get_symbol_mut(alias.def_id) {
                        symbol.ty = Some(type_id);
                    } else {
                        errors.push(lowering_error(ISSUE_MISSING_TYPE_ALIAS_SYMBOL, alias.span));
                    }
                    progress = true;
                }
                Err(TypeLowerError::UnboundAlias) => {
                    next_pending.push(alias);
                }
                Err(TypeLowerError::Issue(issue)) => {
                    errors.push(lowering_error(issue, alias.ty.span));
                    // Mark this alias as consumed (placeholder type) so
                    // dependent aliases do not keep deferring and produce a
                    // false CircularDependency error in a later iteration.
                    if let Some(symbol) = resolver.get_symbol_mut(alias.def_id) {
                        symbol.ty = Some(types.primitive(Primitive::Vacuum));
                    }
                    progress = true;
                }
                Err(TypeLowerError::IssueWithArg { issue, name, value }) => {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::LoweringError, alias.ty.span)
                            .with_arg("issue", issue)
                            .with_arg(name, value),
                    );
                    if let Some(symbol) = resolver.get_symbol_mut(alias.def_id) {
                        symbol.ty = Some(types.primitive(Primitive::Vacuum));
                    }
                    progress = true;
                }
                Err(TypeLowerError::CollectionArity(collection)) => {
                    errors.push(lowering_collection_arity_error(collection, alias.ty.span));
                    if let Some(symbol) = resolver.get_symbol_mut(alias.def_id) {
                        symbol.ty = Some(types.primitive(Primitive::Vacuum));
                    }
                    progress = true;
                }
            }
        }

        if !progress {
            for alias in next_pending {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::CircularDependency, alias.ty.span)
                        .with_arg("issue", ISSUE_TYPE_ALIAS_CYCLE),
                );
            }
            break;
        }

        pending = next_pending;
    }
}

/// Resolve relational schema declarations and build their ordered metadata.
///
/// Runs after [`resolve_alias_types`] so a column type may reference a `typus`
/// alias (the fixpoint has interned every resolvable alias by then). The
/// schema name was registered by `collect` as a type-namespace symbol; this
/// pass raises the semantic rejections, lowers every column type into the
/// shared [`TypeTable`], and stores the ordered five-field column metadata on
/// the resolver. The path is purely source-level: no database, no catalog, no
/// driver dependency anywhere (a partial schema resolves without a DB
/// connection).
///
/// Rejections:
/// - `schema_duplicate_visible`: two columns with the same public name.
/// - `schema_duplicate_source`: two visible names resolving to one source
///   name after alias resolution.
/// - `schema_unknown_type`: a column type that cannot be established in the
///   type namespace.
/// - `schema_nested_column`: a column whose type is itself a schema.
/// - `schema_method` / `schema_inheritance` are grammar-level: the parser
///   rejects method-like members and `sub`/`implet` inheritance before the
///   AST exists.
fn resolve_schema_decls(
    program: &Program,
    resolver: &mut Resolver,
    interner: &Interner,
    types: &mut TypeTable,
    errors: &mut Vec<SemanticError>,
) {
    for stmt in &program.statements {
        let StmtKind::Schema(decl) = &stmt.kind else {
            continue;
        };
        let Some(def_id) = resolver.lookup(decl.name.name) else {
            // `collect` should have registered the schema symbol; a miss means
            // collection already reported the failure.
            continue;
        };

        let name = decl.name.name;
        let mut seen_visible: handroll::FxHashSet<radix_lexer::Symbol> =
            handroll::FxHashSet::default();
        let mut seen_source: handroll::FxHashSet<radix_lexer::Symbol> =
            handroll::FxHashSet::default();
        let mut columns = Vec::with_capacity(decl.columns.len());

        for (order, column) in decl.columns.iter().enumerate() {
            if !seen_visible.insert(column.name.name) {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, column.span)
                        .with_arg("issue", ISSUE_SCHEMA_DUPLICATE_VISIBLE)
                        .with_arg("column", interner.resolve(column.name.name)),
                );
            }

            // Identity mapping when no alias row: two visible names must not
            // collapse to one source name.
            let source = column
                .source_alias
                .as_ref()
                .map_or(column.name.name, |alias| alias.name);
            if !seen_source.insert(source) {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, column.span)
                        .with_arg("issue", ISSUE_SCHEMA_DUPLICATE_SOURCE)
                        .with_arg("column", interner.resolve(column.name.name))
                        .with_arg("source", interner.resolve(source)),
                );
            }

            // Faber type contract: lower through the existing type resolver so
            // `T ∪ nihil` spells nullability. Every failure to establish the
            // column type in the type namespace raises the pinned
            // `schema_unknown_type` issue and interns the recovery sentinel so
            // the ordered metadata stays complete.
            let ty = match lower_type_expr(&column.ty, resolver, interner, types) {
                Ok(ty) => ty,
                Err(
                    TypeLowerError::UnboundAlias
                    | TypeLowerError::Issue(_)
                    | TypeLowerError::IssueWithArg { .. }
                    | TypeLowerError::CollectionArity(_),
                ) => {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::TypeUndefined, column.ty.span)
                            .with_arg("issue", ISSUE_SCHEMA_UNKNOWN_TYPE)
                            .with_arg("column", interner.resolve(column.name.name)),
                    );
                    types.intern(Type::Error)
                }
            };

            if is_schema_type(types, ty, resolver) {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::LoweringError, column.span)
                        .with_arg("issue", ISSUE_SCHEMA_NESTED_COLUMN)
                        .with_arg("column", interner.resolve(column.name.name)),
                );
            }

            // Required/nullability derived from `T ∪ nihil` in the type
            // position: nullable columns lower to `Type::Option(_)`.
            let required = !matches!(types.get(ty), Type::Option(_));
            columns.push(SchemaColumnDef {
                name: column.name.name,
                source,
                ty,
                required,
                order,
            });
        }

        resolver.register_schema(def_id, SchemaDef { name, columns });
    }
}

/// Return whether `ty` names a relational schema (as opposed to a genus) in
/// the type arena: the schema carrier is the declared-type intern slot, and
/// the resolver symbol kind is the disambiguator.
fn is_schema_type(types: &TypeTable, ty: TypeId, resolver: &Resolver) -> bool {
    if let Type::Struct(def_id) = types.get(ty) {
        return matches!(
            resolver.get_symbol(*def_id).map(|symbol| symbol.kind),
            Some(SymbolKind::Schema)
        );
    }
    false
}

fn lower_type_expr(
    ty: &TypeExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    lower_type_expr_with_context(ty, resolver, interner, types, false)
}

fn lower_parameter_type_expr(
    ty: &TypeExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    lower_type_expr_with_context(ty, resolver, interner, types, true)
}

fn lower_type_expr_with_context(
    ty: &TypeExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
    allow_scalar_ref_mut: bool,
) -> Result<TypeId, TypeLowerError> {
    // This is intentionally narrower than full typechecking: it interns the
    // declared shape of a type expression so aliases have stable TypeIds, but
    // assignability and expression typing remain HIR-phase responsibilities.
    let mut ty_id = match &ty.kind {
        TypeExprKind::Infer | TypeExprKind::FieldListHole => {
            types.intern(Type::Infer(crate::InferVar(ty.span.start)))
        }
        TypeExprKind::UnionHole => types.intern(Type::InferUnion(crate::InferVar(ty.span.start))),
        TypeExprKind::Tensor(elem, figura) => {
            lower_tensor_type_expr(elem, figura, resolver, interner, types)?
        }
        TypeExprKind::Vector(elem, figura) => {
            lower_vector_type_expr(elem, figura, resolver, interner, types)?
        }
        TypeExprKind::Matrix(elem, figura) => {
            lower_matrix_type_expr(elem, figura, resolver, interner, types)?
        }
        TypeExprKind::Sparsa(elem, figura) => {
            lower_sparsa_type_expr(elem, figura, resolver, interner, types)?
        }
        TypeExprKind::Atomic(elem) => lower_atomic_type_expr(elem, resolver, interner, types)?,
        TypeExprKind::PromissumFailable(success, alternate) => {
            let success = lower_type_expr(success, resolver, interner, types)?;
            let alternate = lower_type_expr(alternate, resolver, interner, types)?;
            types.promissum_failable(success, alternate)
        }
        TypeExprKind::Named(name, params) => {
            lower_named_type(name, params, resolver, interner, types)?
        }
        TypeExprKind::Qualified(path, params) => {
            lower_qualified_type(path, params, resolver, interner, types)?
        }
        TypeExprKind::Func(func) => {
            let params = func
                .params
                .iter()
                .map(|param| {
                    lower_parameter_type_expr(param, resolver, interner, types).map(|ty| {
                        ParamType {
                            ty,
                            mode: param
                                .mode
                                .map_or(SemanticParamMode::Owned, |mode| match mode {
                                    radix_syntax::TypeMode::De => SemanticParamMode::Ref,
                                    radix_syntax::TypeMode::In => SemanticParamMode::MutRef,
                                    radix_syntax::TypeMode::Own | radix_syntax::TypeMode::Copy => {
                                        SemanticParamMode::Owned
                                    }
                                }),
                            optional: false,
                            sponte: false,
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let ret = lower_type_expr(&func.ret, resolver, interner, types)?;
            let err = func
                .err
                .as_ref()
                .map(|err| lower_type_expr(err, resolver, interner, types))
                .transpose()?;
            types.function(FuncSig {
                type_params: Vec::new(),
                params,
                ret,
                err,
                is_async: false,
                is_generator: false,
            })
        }
        TypeExprKind::Intersection(members) => {
            // BO-S-4-identical to HIR-side intersection lowering: nominal
            // operands only (R2), sorted dedup, single-member collapse.
            if members.is_empty() {
                return Err(TypeLowerError::Issue(ISSUE_EMPTY_INTERSECTION_TYPE));
            }
            let mut cleaned: Vec<TypeId> = Vec::with_capacity(members.len());
            for m in members {
                let lowered = lower_type_expr(m, resolver, interner, types)?;
                if matches!(types.get(lowered), Type::Primitive(_)) {
                    return Err(TypeLowerError::Issue(ISSUE_INTERSECTION_PRIMITIVE_OPERAND));
                }
                cleaned.push(lowered);
            }
            cleaned.sort_by_key(|id| (types.union_member_sort_key(*id).0, id.0));
            cleaned.dedup_by(|a, b| types.equals(*a, *b));
            if cleaned.len() == 1 {
                cleaned[0]
            } else {
                types.intern(Type::Intersection(cleaned))
            }
        }
        TypeExprKind::Union(members) => {
            if members.is_empty() {
                return Err(TypeLowerError::Issue(ISSUE_EMPTY_UNION_TYPE));
            }

            // Collect → sort → adjacent dedup (O(n log n + n·equals)) instead
            // of O(n²) pairwise is_distinct_union_member scans.
            //
            // The sort key must cluster `equals`-true members
            // (union_member_sort_key): a `typus` alias and the type it resolves
            // to are distinct arena entries whose raw TypeId ordinals need not
            // be adjacent — an unrelated member listed between them survives
            // dedup_by(equals), so the union carries a redundant member and two
            // spellings of the same union no longer resolve to equal types.
            let mut cleaned: Vec<TypeId> = Vec::with_capacity(members.len());
            let mut had_nihil = false;

            for m in members {
                let lowered = lower_type_expr(m, resolver, interner, types)?;
                if let Type::Primitive(Primitive::Nihil) = types.get(lowered) {
                    had_nihil = true;
                    continue;
                }
                cleaned.push(lowered);
            }

            cleaned.sort_by_key(|id| (types.union_member_sort_key(*id).0, id.0));
            cleaned.dedup_by(|a, b| types.equals(*a, *b));

            if cleaned.is_empty() {
                return Err(TypeLowerError::Issue(ISSUE_UNION_ONLY_NIHIL));
            }

            let base = if cleaned.len() == 1 {
                cleaned[0]
            } else {
                types.intern(Type::Union(cleaned))
            };
            if had_nihil { types.option(base) } else { base }
        }
    };

    if let Some(mode) = ty.mode {
        let marker = crate::access_marker_from_type_mode(mode);
        if types.is_scalar_for_access_marker(ty_id)
            && !(allow_scalar_ref_mut
                && matches!(
                    mode,
                    radix_syntax::TypeMode::De | radix_syntax::TypeMode::In
                ))
        {
            return Err(TypeLowerError::Issue(crate::ISSUE_SCALAR_ACCESS_MARKER));
        }
        ty_id = types.access_marker(marker, ty_id);
    }

    if ty.nullable {
        ty_id = types.option(ty_id);
    }

    Ok(ty_id)
}

/// Map residual shared-helper messages to issue identity.
///
/// Width-family helpers report typed `WidthLowerIssue` values, so this
/// handles only helpers that still cross the phase boundary as message
/// constants (figura, matrix, vector, atomic) or instans precision prose.
fn type_lower_error_from_message(message: &str) -> TypeLowerError {
    if let Some(reason) = crate::figura::figura_arithmetic_reason(message) {
        return TypeLowerError::IssueWithArg {
            issue: ISSUE_FIGURA_ARITHMETIC_INVALID,
            name: "reason",
            value: reason,
        };
    }
    let issue = match message {
        crate::FIGURA_NOT_INDEX_PARAM_MSG => ISSUE_FIGURA_NOT_INDEX_PARAM,
        crate::FIGURA_TYPE_PARAM_IN_INDEX_MSG => ISSUE_FIGURA_TYPE_PARAM_IN_INDEX,
        crate::FIGURA_UNKNOWN_INDEX_MSG => ISSUE_FIGURA_UNKNOWN_INDEX,
        crate::MATRIX_SHAPE_TWO_DIM_MSG => ISSUE_MATRIX_SHAPE_TWO_DIM,
        crate::ATOMIC_ELEMENT_MSG => ISSUE_ATOMIC_ELEMENT,
        crate::VECTOR_WIDTH_SINGLE_DIM_MSG => ISSUE_VECTOR_WIDTH_SINGLE_DIM,
        "instans accepts at most one precision marker" => "instans_precision_params",
        "instans accepts only ms, us, or ns as a precision marker" => {
            ISSUE_INVALID_PRECISION_MARKER
        }
        "precision marker must be a bare spelling such as ms or us" => ISSUE_PRECISION_MARKER_SHAPE,
        _ => ISSUE_UNKNOWN_TYPE_LOWERING,
    };
    TypeLowerError::Issue(issue)
}

/// Ruling 39(a): `inf` is never a tensor-family element; it has no device layout.
fn reject_unbounded_element(types: &TypeTable, elem_ty: TypeId) -> Result<(), TypeLowerError> {
    if crate::numeric_width::is_unbounded_integer_type(types, elem_ty) {
        return Err(TypeLowerError::Issue(
            crate::numeric_width::TENSOR_ELEMENT_UNBOUNDED_ISSUE,
        ));
    }
    Ok(())
}

fn lower_tensor_type_expr(
    elem: &TypeExpr,
    figura: &radix_syntax::FiguraExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    let elem_ty = lower_type_expr(elem, resolver, interner, types)?;
    reject_unbounded_element(types, elem_ty)?;
    let shape =
        crate::lower_figura_expr(figura, resolver, types).map_err(type_lower_error_from_message)?;
    Ok(types.tensor_with_shape(elem_ty, shape))
}

fn lower_vector_type_expr(
    elem: &TypeExpr,
    width: &FiguraExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    let elem_ty = lower_type_expr(elem, resolver, interner, types)?;
    reject_unbounded_element(types, elem_ty)?;
    let width_id = crate::lower_vector_width_expr(width, resolver, types)
        .map_err(|error| type_lower_error_from_message(error.message))?;
    Ok(types.vector_with_width(elem_ty, width_id))
}

fn lower_matrix_type_expr(
    elem: &TypeExpr,
    shape: &FiguraExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    let elem_ty = lower_type_expr(elem, resolver, interner, types)?;
    reject_unbounded_element(types, elem_ty)?;
    let shape_id = crate::lower_matrix_shape_expr(shape, resolver, types)
        .map_err(type_lower_error_from_message)?;
    Ok(types.matrix_with_shape(elem_ty, shape_id))
}

fn lower_sparsa_type_expr(
    elem: &TypeExpr,
    figura: &radix_syntax::FiguraExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    let elem_ty = lower_type_expr(elem, resolver, interner, types)?;
    reject_unbounded_element(types, elem_ty)?;
    if !crate::numeric_width::is_tensor_numeric_element(types, elem_ty) {
        return Err(TypeLowerError::Issue(ISSUE_SPARSA_ELEMENT_NON_NUMERIC));
    }
    let shape =
        crate::lower_figura_expr(figura, resolver, types).map_err(type_lower_error_from_message)?;
    Ok(types.sparsa_with_shape(elem_ty, shape))
}

fn lower_atomic_type_expr(
    elem: &TypeExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    let elem_ty = lower_type_expr(elem, resolver, interner, types)?;
    if !crate::is_atomic_element_type(types, elem_ty) {
        return Err(TypeLowerError::Issue(ISSUE_ATOMIC_ELEMENT));
    }
    Ok(types.atomic(elem_ty))
}

fn lower_bounded_primitive(
    prim: Primitive,
    params: &[GenericArgExpr],
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    match params {
        [] => Ok(types.primitive(prim)),
        [capacity]
            if prim == Primitive::Ascii && named_type_is_ascii_format_hint(capacity, interner) =>
        {
            Err(TypeLowerError::Issue(ISSUE_ASCII_FORMAT_HINT_NOT_CAPACITY))
        }
        [capacity] => {
            let capacity = lower_generic_index_arg(capacity, resolver, interner, types)?;
            match prim {
                Primitive::Ascii => Ok(types.bounded_ascii(capacity)),
                Primitive::Textus => Ok(types.bounded_textus(capacity)),
                _ => Err(TypeLowerError::Issue(ISSUE_PRIMITIVE_PARAMS)),
            }
        }
        _ => Err(TypeLowerError::Issue(ISSUE_PRIMITIVE_PARAMS)),
    }
}

fn lower_lista_type(
    params: &[GenericArgExpr],
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    match params {
        [elem] => {
            let elem = lower_generic_type_arg(elem, resolver, interner, types)?;
            Ok(types.array(elem))
        }
        [elem, GenericArgExpr::Range(range)] => {
            let elem = lower_generic_type_arg(elem, resolver, interner, types)?;
            let mut issue = None;
            let ty = radix_types::lower_ranged_list(elem, range, types, &mut |found| {
                issue.get_or_insert(found);
            });
            match issue {
                Some(issue) => Err(TypeLowerError::Issue(issue.issue())),
                None => Ok(ty),
            }
        }
        [elem, capacity] => {
            let elem = lower_generic_type_arg(elem, resolver, interner, types)?;
            let capacity = lower_generic_index_arg(capacity, resolver, interner, types)?;
            Ok(types.bounded_array(elem, capacity))
        }
        _ => Err(TypeLowerError::CollectionArity(CollectionKind::Lista)),
    }
}

fn lower_queue_stack_type(
    collection: CollectionKind,
    params: &[GenericArgExpr],
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    match params {
        [elem] => {
            let elem = lower_generic_type_arg(elem, resolver, interner, types)?;
            match collection {
                CollectionKind::Queue => Ok(types.queue(elem)),
                CollectionKind::Stack => Ok(types.stack(elem)),
                _ => Err(TypeLowerError::CollectionArity(collection)),
            }
        }
        [elem, capacity] => {
            let elem = lower_generic_type_arg(elem, resolver, interner, types)?;
            let capacity = lower_generic_index_arg(capacity, resolver, interner, types)?;
            match collection {
                CollectionKind::Queue => Ok(types.bounded_queue(elem, capacity)),
                CollectionKind::Stack => Ok(types.bounded_stack(elem, capacity)),
                _ => Err(TypeLowerError::CollectionArity(collection)),
            }
        }
        _ => Err(TypeLowerError::CollectionArity(collection)),
    }
}

fn generic_type_expr(arg: &GenericArgExpr) -> Option<TypeExpr> {
    match arg {
        GenericArgExpr::Type(ty) => Some(ty.clone()),
        GenericArgExpr::Name(name) => Some(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(name.clone(), Vec::new()),
            span: name.span,
            form: radix_syntax::TypeFormLevel::LONG,
            iuncta_label: None,
        }),
        GenericArgExpr::Size(_) | GenericArgExpr::Range(_) => None,
    }
}

fn generic_type_exprs(params: &[GenericArgExpr]) -> Option<Vec<TypeExpr>> {
    params.iter().map(generic_type_expr).collect()
}

fn lower_generic_type_arg(
    arg: &GenericArgExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    match arg {
        GenericArgExpr::Type(ty) => lower_type_expr(ty, resolver, interner, types),
        GenericArgExpr::Name(name) => lower_named_type(name, &[], resolver, interner, types),
        GenericArgExpr::Size(_) => Err(TypeLowerError::Issue(
            "generic_size_argument_in_type_position",
        )),
        GenericArgExpr::Range(_) => Err(TypeLowerError::Issue(
            crate::numeric_width::WidthLowerIssue::RANGED_ARITY,
        )),
    }
}

/// Lower the compile-time step argument of `intervallum<T, S>`.
///
/// The step is a positive literal only: a zero literal reports the value-level
/// `range_step_not_positive` law, and any other shape (a name, an arithmetic
/// expression, `_`) reports `intervallum_step_must_be_constant` rather than
/// guessing a run-time step.
pub(crate) fn intervallum_step_index(
    arg: &GenericArgExpr,
    types: &mut TypeTable,
) -> Result<radix_types::IndexId, &'static str> {
    let GenericArgExpr::Size(FiguraExpr::Literal(value)) = arg else {
        return Err(ISSUE_INTERVALLUM_STEP_MUST_BE_CONSTANT);
    };
    if *value == 0 {
        return Err("range_step_not_positive");
    }
    Ok(types.intern_index(IndexExpr::Literal(*value)))
}

/// True when an `intervallum` bound is an integer type.
///
/// A stepped interval takes integer bounds only; `instans` is the one legal
/// non-integer bound and carries no step.
#[must_use]
pub(crate) fn is_integer_intervallum_bound(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(ty),
        Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
    )
}

fn lower_generic_index_arg(
    arg: &GenericArgExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<radix_types::IndexId, TypeLowerError> {
    match arg {
        GenericArgExpr::Size(figura) => {
            crate::lower_figura_expr(figura, resolver, types).map_err(type_lower_error_from_message)
        }
        GenericArgExpr::Type(TypeExpr {
            kind: TypeExprKind::Infer,
            ..
        }) => {
            let var = types.fresh_index_infer_var();
            Ok(types.intern_index(IndexExpr::Infer(var)))
        }
        GenericArgExpr::Name(ident) => lower_named_index_ident(ident, resolver, types),
        // Bounded-capacity applications keep the explicit `Type` shape, so a
        // bare named index argument arrives here as `Type(Named(…))`; digit
        // spellings and index params lower exactly like the `Name` shape.
        GenericArgExpr::Type(TypeExpr {
            kind: TypeExprKind::Named(ident, nested),
            ..
        }) if nested.is_empty() => {
            if let Some(value) = radix_types::parse_natural_capacity(interner.resolve(ident.name)) {
                return Ok(types.intern_index(IndexExpr::Literal(value)));
            }
            lower_named_index_ident(ident, resolver, types)
        }
        GenericArgExpr::Type(_) => Err(TypeLowerError::Issue(ISSUE_FIGURA_TYPE_PARAM_IN_INDEX)),
        GenericArgExpr::Range(_) => Err(TypeLowerError::Issue(
            crate::numeric_width::WidthLowerIssue::RANGED_ARITY,
        )),
    }
}

fn lower_named_index_ident(
    ident: &radix_syntax::Ident,
    resolver: &Resolver,
    types: &mut TypeTable,
) -> Result<radix_types::IndexId, TypeLowerError> {
    let kind = resolver
        .lookup(ident.name)
        .and_then(|def_id| resolver.get_symbol(def_id))
        .map(|symbol| symbol.kind);
    match kind {
        Some(SymbolKind::IndexParam) => Ok(types.intern_index(IndexExpr::Param(ident.name))),
        Some(SymbolKind::TypeParam) => Err(TypeLowerError::Issue(ISSUE_FIGURA_TYPE_PARAM_IN_INDEX)),
        _ => Err(TypeLowerError::Issue(ISSUE_BOUNDED_CAPACITY_NOT_NATURAL)),
    }
}

fn lower_applied_arg(
    arg: &GenericArgExpr,
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<AppliedArg, TypeLowerError> {
    match arg {
        GenericArgExpr::Size(_) => {
            lower_generic_index_arg(arg, resolver, interner, types).map(AppliedArg::Size)
        }
        GenericArgExpr::Type(ty) if matches!(ty.kind, TypeExprKind::Infer) => {
            lower_type_expr(ty, resolver, interner, types).map(AppliedArg::Type)
        }
        GenericArgExpr::Name(ident) => {
            let is_index = resolver
                .lookup(ident.name)
                .and_then(|def_id| resolver.get_symbol(def_id))
                .is_some_and(|symbol| symbol.kind == SymbolKind::IndexParam);
            if is_index {
                lower_generic_index_arg(arg, resolver, interner, types).map(AppliedArg::Size)
            } else {
                lower_generic_type_arg(arg, resolver, interner, types).map(AppliedArg::Type)
            }
        }
        GenericArgExpr::Type(ty) => {
            lower_type_expr(ty, resolver, interner, types).map(AppliedArg::Type)
        }
        GenericArgExpr::Range(_) => Err(TypeLowerError::Issue(
            crate::numeric_width::WidthLowerIssue::RANGED_ARITY,
        )),
    }
}

fn lower_named_type(
    name: &radix_syntax::Ident,
    params: &[GenericArgExpr],
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    let name_str = interner.resolve(name.name);

    if name_str == "saturatus" {
        let mut ranged_issue = None;
        if let Some(ty) = crate::numeric_width::lower_ranged_word(
            radix_types::IntPolicy::Saturating,
            params,
            interner,
            types,
            &mut |issue| {
                ranged_issue.get_or_insert(issue);
            },
        ) {
            return match ranged_issue {
                Some(issue) => Err(TypeLowerError::Issue(issue.issue())),
                None => Ok(ty),
            };
        }
        let Some(type_params) = generic_type_exprs(params) else {
            return Err(TypeLowerError::Issue(
                "generic_size_argument_in_type_position",
            ));
        };
        let mut error = None;
        let ty = crate::numeric_width::lower_saturating_word(
            &type_params,
            interner,
            types,
            &mut |issue| {
                error = Some(issue);
            },
        );
        if let Some(issue) = error {
            return Err(TypeLowerError::Issue(issue.issue()));
        }
        return Ok(ty);
    }

    if name_str == "exactus" {
        let mut ranged_issue = None;
        if let Some(ty) = crate::numeric_width::lower_ranged_word(
            radix_types::IntPolicy::Trapping,
            params,
            interner,
            types,
            &mut |issue| {
                ranged_issue.get_or_insert(issue);
            },
        ) {
            return match ranged_issue {
                Some(issue) => Err(TypeLowerError::Issue(issue.issue())),
                None => Ok(ty),
            };
        }
        let Some(type_params) = generic_type_exprs(params) else {
            return Err(TypeLowerError::Issue(
                "generic_size_argument_in_type_position",
            ));
        };
        let mut error = None;
        let ty = crate::numeric_width::lower_trapping_word(
            &type_params,
            interner,
            types,
            &mut |issue| {
                error = Some(issue);
            },
        );
        if let Some(issue) = error {
            return Err(TypeLowerError::Issue(issue.issue()));
        }
        return Ok(ty);
    }

    if name_str == "modulus" {
        let mut ranged_issue = None;
        if let Some(ty) = crate::numeric_width::lower_ranged_word(
            radix_types::IntPolicy::Wrapping,
            params,
            interner,
            types,
            &mut |issue| {
                ranged_issue.get_or_insert(issue);
            },
        ) {
            return match ranged_issue {
                Some(issue) => Err(TypeLowerError::Issue(issue.issue())),
                None => Ok(ty),
            };
        }
        let Some(type_params) = generic_type_exprs(params) else {
            return Err(TypeLowerError::Issue(
                "generic_size_argument_in_type_position",
            ));
        };
        let mut error = None;
        let ty =
            crate::numeric_width::lower_modular_word(&type_params, interner, types, &mut |issue| {
                error = Some(issue);
            });
        if let Some(issue) = error {
            return Err(TypeLowerError::Issue(issue.issue()));
        }
        return Ok(ty);
    }

    if let Some(prim) = Primitive::from_name(name_str) {
        // Bounded textus/ascii capacities are index arguments, so they
        // must bypass the width/type-only generic argument path.
        if matches!(prim, Primitive::Ascii | Primitive::Textus) {
            return lower_bounded_primitive(prim, params, resolver, interner, types);
        }
        let Some(type_params) = generic_type_exprs(params) else {
            if matches!(
                prim,
                Primitive::Numerus | Primitive::Fractus | Primitive::Instans
            ) {
                return Err(TypeLowerError::Issue(
                    "generic_size_argument_in_type_position",
                ));
            }
            return Err(TypeLowerError::Issue(ISSUE_PRIMITIVE_PARAMS));
        };
        if matches!(prim, Primitive::Instans) {
            let mut error = None;
            let ty = crate::instans_precision::lower_sized_instans(
                &type_params,
                interner,
                types,
                &mut |message| {
                    error = Some(message.to_owned());
                },
            );
            if let Some(message) = error {
                return Err(type_lower_error_from_message(&message));
            }
            return Ok(ty);
        }
        if !params.is_empty() {
            return Err(TypeLowerError::Issue(ISSUE_PRIMITIVE_PARAMS));
        }
        return Ok(types.primitive(prim));
    }

    if let Some(collection) = CollectionKind::from_name(name_str) {
        if collection == CollectionKind::Lista {
            return lower_lista_type(params, resolver, interner, types);
        }
        if matches!(collection, CollectionKind::Queue | CollectionKind::Stack) {
            return lower_queue_stack_type(collection, params, resolver, interner, types);
        }
        if !collection.accepts_arity(params.len()) {
            return Err(TypeLowerError::CollectionArity(collection));
        }
        let lowered = params
            .iter()
            .map(|param| lower_generic_type_arg(param, resolver, interner, types))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(collection.lower(types, &lowered));
    }

    // `intervallum<T, S>` takes a type bound and an optional compile-time
    // natural step. Its second argument is not a type position, so this arm
    // sits ahead of the type-only generic-argument guard below; the step shape
    // is validated when the type is lowered.
    if name_str == "intervallum" {
        if params.len() != 1 && params.len() != 2 {
            return Err(TypeLowerError::Issue(ISSUE_INTERVALLUM_ARITY));
        }
        let bound = lower_generic_type_arg(&params[0], resolver, interner, types)?;
        if !crate::is_intervallum_bound_type(types, bound) {
            return Err(TypeLowerError::Issue(ISSUE_INTERVALLUM_BOUND));
        }
        let Some(step_arg) = params.get(1) else {
            return Ok(types.intervallum(bound));
        };
        let step = intervallum_step_index(step_arg, types).map_err(TypeLowerError::Issue)?;
        if !is_integer_intervallum_bound(types, bound) {
            return Err(TypeLowerError::Issue(
                ISSUE_INTERVALLUM_STEP_BOUND_NOT_INTEGER,
            ));
        }
        return Ok(types.intervallum_with_step(bound, step));
    }

    let Some(type_params) = generic_type_exprs(params) else {
        return Err(TypeLowerError::Issue(
            "generic_size_argument_in_type_position",
        ));
    };
    let mut width_error = None;
    if let Some(ty) = crate::numeric_width::lower_standalone_width_type(
        name_str,
        &type_params,
        types,
        &mut |issue| {
            width_error = Some(issue);
        },
    ) {
        if let Some(issue) = width_error {
            return Err(TypeLowerError::Issue(issue.issue()));
        }
        return Ok(ty);
    }

    // SQL row: dynamic column map (faber::Valor per column).
    if name_str == "series" {
        if !params.is_empty() {
            return Err(TypeLowerError::Issue(ISSUE_SERIES_PARAMS));
        }
        let textus = types.primitive(Primitive::Textus);
        let valor = types.primitive(Primitive::Valor);
        return Ok(types.map(textus, valor));
    }

    if name_str == "sermo" && !params.is_empty() {
        let Some(def_id) = crate::builtins::frame_types::sermo_def_id(resolver) else {
            return Err(TypeLowerError::Issue(ISSUE_SERMO_MISSING_BUILTIN));
        };
        let [opener, reply] = params else {
            return Err(TypeLowerError::Issue(ISSUE_SERMO_ARITY));
        };
        let opener = lower_generic_type_arg(opener, resolver, interner, types)?;
        let reply = lower_generic_type_arg(reply, resolver, interner, types)?;
        return Ok(crate::builtins::frame_types::sermo_typed_type_id(
            types, def_id, opener, reply,
        ));
    }

    if name_str == "scrinium" {
        let Some(def_id) = crate::builtins::frame_types::scrinium_def_id(resolver) else {
            return Err(TypeLowerError::Issue(ISSUE_SCRINIUM_MISSING_BUILTIN));
        };
        let payload_ty = match params {
            [] => types.primitive(Primitive::Valor),
            [payload] => lower_generic_type_arg(payload, resolver, interner, types)?,
            _ => {
                return Err(TypeLowerError::Issue(ISSUE_SCRINIUM_ARITY));
            }
        };
        return Ok(crate::builtins::frame_types::scrinium_payload_type_id(
            types, def_id, payload_ty,
        ));
    }

    if name_str == "atomic" {
        if params.is_empty() {
            return Err(TypeLowerError::Issue(ISSUE_ATOMIC_INCOMPLETE));
        }
        return Err(TypeLowerError::Issue(ISSUE_ATOMIC_ARITY));
    }

    if name_str == "tensor" {
        if params.is_empty() {
            return Err(TypeLowerError::Issue(ISSUE_TENSOR_INCOMPLETE));
        }
        return Err(TypeLowerError::Issue(ISSUE_TENSOR_MISSING_FIGURA));
    }

    if name_str == "sparsa" {
        if params.is_empty() {
            return Err(TypeLowerError::Issue(ISSUE_SPARSA_INCOMPLETE));
        }
        return Err(TypeLowerError::Issue(ISSUE_SPARSA_MISSING_FIGURA));
    }

    if name_str == "matrix" {
        if params.is_empty() {
            return Err(TypeLowerError::Issue(ISSUE_MATRIX_INCOMPLETE));
        }
        return Err(TypeLowerError::Issue(ISSUE_MATRIX_MISSING_FIGURA));
    }

    if name_str == "ratio" {
        let mut fields = Vec::with_capacity(params.len());
        for arg in params {
            let GenericArgExpr::Type(param) = arg else {
                continue;
            };
            let Some(label) = param.iuncta_label.as_ref() else {
                continue;
            };
            let field_ty = lower_type_expr(param, resolver, interner, types)?;
            fields.push((label.name, field_ty));
        }
        return Ok(types.record(fields));
    }

    let Some(def_id) = resolver.lookup_type(name.name) else {
        if let Some(type_id) = resolver.lookup_selective_import_type(name.name) {
            return lower_selective_import_type(type_id, params, resolver, interner, types);
        }
        return Err(TypeLowerError::Issue(ISSUE_UNKNOWN_TYPE_NAME));
    };

    let Some(symbol) = resolver.get_symbol(def_id) else {
        return Err(TypeLowerError::Issue(ISSUE_MISSING_TYPE_SYMBOL_INFO));
    };

    let base = match symbol.kind {
        SymbolKind::Struct => types.intern(Type::Struct(def_id)),
        SymbolKind::Enum => types.intern(Type::Enum(def_id)),
        SymbolKind::Interface => types.intern(Type::Interface(def_id)),
        SymbolKind::TypeAlias => match symbol.ty {
            Some(alias_ty) => {
                let body = crate::index::freshen_bounded_capacity_holes(types, alias_ty);
                types.intern(Type::Alias(def_id, body))
            }
            None => return Err(TypeLowerError::UnboundAlias),
        },
        SymbolKind::TypeParam => types.intern(Type::Param(symbol.name)),
        SymbolKind::Schema => {
            // A schema name is a type-namespace identity per the
            // typus/genus declared-type precedent — usable as a type argument —
            // with no runtime semantics (zero MIR/backend lowering). The
            // declared-type intern slot is the carrier; schema vs genus is
            // disambiguated by the symbol kind, so the schema's ordered
            // relational metadata never becomes a genus field list.
            types.intern(Type::Struct(def_id))
        }
        _ => return Err(TypeLowerError::Issue(ISSUE_TYPE_NAME_NOT_TYPE)),
    };

    if params.is_empty() {
        return Ok(base);
    }

    let args = params
        .iter()
        .map(|param| lower_applied_arg(param, resolver, interner, types))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(types.intern(Type::Applied(base, args)))
}

fn lower_qualified_type(
    path: &[radix_syntax::Ident],
    params: &[GenericArgExpr],
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    let Some((namespace, type_name)) = path.first().zip(path.last()) else {
        return Err(TypeLowerError::Issue(ISSUE_EMPTY_QUALIFIED_TYPE_PATH));
    };
    // Middle segments (indices 1..len-1) must be Module-kind bindings, not
    // just successful lookups.
    for segment in &path[1..path.len().saturating_sub(1)] {
        let Some(def_id) = resolver.lookup(segment.name) else {
            return Err(TypeLowerError::Issue(ISSUE_UNKNOWN_NAMESPACE));
        };
        if !matches!(
            resolver.get_symbol(def_id).map(|symbol| symbol.kind),
            Some(SymbolKind::Module)
        ) {
            return Err(TypeLowerError::Issue(
                ISSUE_QUALIFIED_TYPE_MIDDLE_NOT_NAMESPACE,
            ));
        }
    }
    let Some(namespace_def_id) = resolver.lookup(namespace.name) else {
        return Err(TypeLowerError::Issue(ISSUE_UNKNOWN_NAMESPACE));
    };
    if !matches!(
        resolver
            .get_symbol(namespace_def_id)
            .map(|symbol| symbol.kind),
        Some(SymbolKind::Module)
    ) {
        return Err(TypeLowerError::Issue(
            ISSUE_QUALIFIED_TYPE_PREFIX_NOT_NAMESPACE,
        ));
    }
    if !resolver.namespace_exports_member(namespace.name, type_name.name) {
        if let Some(issue) =
            resolver.namespace_member_rejection_issue(namespace.name, type_name.name)
        {
            return Err(TypeLowerError::Issue(issue));
        }
        return Err(TypeLowerError::Issue(ISSUE_QUALIFIED_TYPE_NOT_EXPORTED));
    }
    if let Some(type_id) = resolver.namespace_file_type(namespace.name, type_name.name) {
        if params.is_empty() {
            return Ok(type_id);
        }
        let args = params
            .iter()
            .map(|param| lower_applied_arg(param, resolver, interner, types))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(types.intern(Type::Applied(type_id, args)));
    }
    Err(TypeLowerError::Issue(ISSUE_UNKNOWN_QUALIFIED_TYPE))
}

/// A selectively imported type (D22.5) lowers exactly like its qualified
/// `module.Type` path: the imported `TypeId`, applied when arguments follow.
fn lower_selective_import_type(
    type_id: TypeId,
    params: &[GenericArgExpr],
    resolver: &Resolver,
    interner: &Interner,
    types: &mut TypeTable,
) -> Result<TypeId, TypeLowerError> {
    if params.is_empty() {
        return Ok(type_id);
    }
    let args = params
        .iter()
        .map(|param| lower_applied_arg(param, resolver, interner, types))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(types.intern(Type::Applied(type_id, args)))
}

fn define_generic_param(
    resolver: &mut Resolver,
    param: &radix_syntax::TypeParam,
    errors: &mut Vec<SemanticError>,
) {
    let kind = match param.kind {
        radix_syntax::GenericParamKind::Typus => SymbolKind::TypeParam,
        radix_syntax::GenericParamKind::Magnitudo => SymbolKind::IndexParam,
    };
    define_symbol(
        resolver,
        param.name.name,
        param.name.span,
        kind,
        false,
        errors,
    );
}

/// Resolve the `implet` bounds of one generic row (D1.1).
///
/// Runs after every parameter of the row is defined, so a bound may name a
/// later parameter (`<T implet R<U>, U>`).
fn resolve_generic_bounds(
    resolver: &mut Resolver,
    interner: &Interner,
    params: &[radix_syntax::TypeParam],
    errors: &mut Vec<SemanticError>,
) {
    for param in params {
        for bound in &param.bounds {
            resolve_contract_ref(resolver, interner, bound, errors);
        }
    }
}

/// Resolve one contract reference: its name and each written type argument.
fn resolve_contract_ref(
    resolver: &mut Resolver,
    interner: &Interner,
    contract: &radix_syntax::ContractRefExpr,
    errors: &mut Vec<SemanticError>,
) {
    resolve_type_ident(resolver, interner, &contract.name, errors);
    for arg in &contract.args {
        resolve_type_expr(resolver, interner, arg, errors);
    }
}

fn modifier_bindings(modifiers: &[radix_syntax::FuncModifier]) -> Vec<&radix_syntax::Ident> {
    let mut out = Vec::new();
    for modifier in modifiers {
        match modifier {
            radix_syntax::FuncModifier::Argumenta(ident)
            | radix_syntax::FuncModifier::Errata(ident)
            | radix_syntax::FuncModifier::Optiones(ident) => out.push(ident),
            radix_syntax::FuncModifier::Exitus(radix_syntax::ExitusValue::Name(ident)) => {
                out.push(ident);
            }
            _ => {}
        }
    }
    out
}
