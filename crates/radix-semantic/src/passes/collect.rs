//! AST declaration collection for the semantic front end.
//!
//! `collect` is the first semantic pass after parsing. It performs a shallow
//! scan of top-level AST statements and reserves [`DefId`](radix_hir::DefId)
//! entries in the resolver for declarations that later phases must be able to
//! reference. It intentionally does not inspect function bodies, lower type
//! expressions, or infer values; its job is to make forward references possible
//! and make duplicate top-level names unambiguous before resolution starts.
//!
//! INVARIANTS
//! ==========
//! - The input is still parser AST; HIR does not exist yet.
//! - Every accepted top-level declaration receives exactly one resolver symbol.
//! - Duplicate names in the current resolver scope are hard semantic errors.
//! - Enum and union variants are registered as named symbols so pattern
//!   resolution can validate variant spelling without typechecking.
//! - Imported names bind the user-visible name: an alias when present, otherwise
//!   the imported name itself.
//!
//! ERROR STRATEGY
//! ==============
//! The pass accumulates duplicate-definition and invalid-top-level-binding
//! diagnostics instead of stopping at the first failure. Later semantic phases
//! should not run after collect errors because ambiguous or missing `DefId`s
//! would make name resolution unreliable.

use crate::{Resolver, SemanticError, SemanticErrorKind, Symbol, SymbolKind, TypeTable};
use radix_syntax::{Program, StmtKind};

/// Register top-level AST declarations in the resolver before name resolution.
///
/// This pass establishes the global symbol set that makes same-file forward
/// references legal. The `TypeTable` parameter is intentionally unused here:
/// collect creates identities, while resolve/typecheck are responsible for
/// lowering type expressions and assigning type information to symbols.
///
/// # Errors
///
/// Returns duplicate definition and related collection errors as a list of
/// [`SemanticError`]s.
pub fn collect(
    program: &Program,
    resolver: &mut Resolver,
    _types: &mut TypeTable,
) -> Result<(), Vec<SemanticError>> {
    let mut errors = Vec::new();
    // Namespace bindings introduced by this file's imports, with whether each
    // is a default (path- or `regio`-derived) name. Two imports that would
    // bind the same default name are ambiguous (D7.7); alias one.
    let mut import_bindings: Vec<(radix_lexer::Symbol, bool)> = Vec::new();

    for stmt in &program.statements {
        match &stmt.kind {
            StmtKind::Var(decl) => {
                if let radix_syntax::BindingPattern::Ident(ident) = &decl.binding {
                    define_symbol(
                        resolver,
                        &mut errors,
                        ident.name,
                        ident.span,
                        SymbolKind::Local,
                    );
                } else {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::LoweringError, stmt.span)
                            .with_arg("issue", "top_level_identifier_binding"),
                    );
                }
            }
            StmtKind::Func(decl) => {
                define_symbol(
                    resolver,
                    &mut errors,
                    decl.name.name,
                    decl.name.span,
                    SymbolKind::Function,
                );
            }
            StmtKind::Class(decl) => {
                define_symbol(
                    resolver,
                    &mut errors,
                    decl.name.name,
                    decl.name.span,
                    SymbolKind::Struct,
                );
            }
            StmtKind::Interface(decl) => {
                define_symbol(
                    resolver,
                    &mut errors,
                    decl.name.name,
                    decl.name.span,
                    SymbolKind::Interface,
                );
            }
            StmtKind::TypeAlias(decl) => {
                define_symbol(
                    resolver,
                    &mut errors,
                    decl.name.name,
                    decl.name.span,
                    SymbolKind::TypeAlias,
                );
            }
            StmtKind::Schema(decl) => {
                // The schema name is a type-namespace identity (per the
                // typus/genus declared-type precedent) and never a runtime
                // value; resolve builds the ordered column metadata.
                define_symbol(
                    resolver,
                    &mut errors,
                    decl.name.name,
                    decl.name.span,
                    SymbolKind::Schema,
                );
            }
            StmtKind::Enum(decl) => {
                let enum_def = define_symbol(
                    resolver,
                    &mut errors,
                    decl.name.name,
                    decl.name.span,
                    SymbolKind::Enum,
                );
                for member in &decl.members {
                    if let Some(enum_def) = enum_def
                        && resolver
                            .define_variant(enum_def, member.name.name, member.name.span)
                            .is_none()
                    {
                        duplicate_definition_error(&mut errors, member.name.span);
                    }
                }
            }
            StmtKind::Union(decl) => {
                let enum_def = define_symbol(
                    resolver,
                    &mut errors,
                    decl.name.name,
                    decl.name.span,
                    SymbolKind::Enum,
                );
                for variant in &decl.variants {
                    if let Some(enum_def) = enum_def
                        && resolver
                            .define_variant(enum_def, variant.name.name, variant.name.span)
                            .is_none()
                    {
                        duplicate_definition_error(&mut errors, variant.name.span);
                    }
                }
            }
            StmtKind::Import(decl) => match &decl.kind {
                radix_syntax::ImportKind::Named {
                    name,
                    alias,
                    inferred,
                } => {
                    let binding = alias.as_ref().unwrap_or(name);
                    let is_default = *inferred && alias.is_none();
                    if default_import_name_collides(&import_bindings, binding.name, is_default) {
                        errors.push(
                            SemanticError::structured(
                                SemanticErrorKind::DefinitionDuplicate,
                                binding.span,
                            )
                            .with_arg("issue", "default_import_name_collision"),
                        );
                        continue;
                    }
                    import_bindings.push((binding.name, is_default));
                    define_import_symbol(
                        resolver,
                        &mut errors,
                        binding.name,
                        binding.span,
                        is_default,
                    );
                }
                radix_syntax::ImportKind::Wildcard { alias } => {
                    if default_import_name_collides(&import_bindings, alias.name, false) {
                        errors.push(
                            SemanticError::structured(
                                SemanticErrorKind::DefinitionDuplicate,
                                alias.span,
                            )
                            .with_arg("issue", "default_import_name_collision"),
                        );
                        continue;
                    }
                    import_bindings.push((alias.name, false));
                    define_import_symbol(resolver, &mut errors, alias.name, alias.span, false);
                }
                // EOT-2M: each selective member binds a caller-owned immutable
                // local named by the alias when spelled, the exported name
                // otherwise. Member resolution against the imported file
                // interface happens in resolve; here the locals must exist and
                // be distinct within the list.
                radix_syntax::ImportKind::Selective { members } => {
                    let mut bound = Vec::new();
                    for member in members {
                        let binding = member.alias.as_ref().unwrap_or(&member.name);
                        if bound.contains(&binding.name) {
                            errors.push(
                                SemanticError::structured(
                                    SemanticErrorKind::DefinitionDuplicate,
                                    binding.span,
                                )
                                .with_arg("issue", "selective_import_duplicate_alias"),
                            );
                            continue;
                        }
                        bound.push(binding.name);
                        define_symbol(
                            resolver,
                            &mut errors,
                            binding.name,
                            binding.span,
                            SymbolKind::Local,
                        );
                    }
                }
            },
            _ => {}
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Whether an import binding repeats an earlier import's name where at least
/// one of the two is a default name nobody spelled (D7.7: "alias one").
fn default_import_name_collides(
    earlier: &[(radix_lexer::Symbol, bool)],
    name: radix_lexer::Symbol,
    is_default: bool,
) -> bool {
    earlier
        .iter()
        .any(|&(bound, bound_default)| bound == name && (is_default || bound_default))
}

fn define_import_symbol(
    resolver: &mut Resolver,
    errors: &mut Vec<SemanticError>,
    name: radix_lexer::Symbol,
    span: radix_lexer::Span,
    inferred: bool,
) {
    // Package assembly can pre-seed module imports. Re-collecting the same
    // module binding should remain idempotent, while user declarations still
    // collide normally through `define_symbol`.
    if let Some(existing) = resolver.lookup(name) {
        let existing_kind = resolver.get_symbol(existing).map(|symbol| symbol.kind);
        if existing_kind == Some(SymbolKind::Module) {
            return;
        }
        // A reader-pack type spelling (`json` in the `en` pack) occupies the
        // name in type position; a module import binds the value namespace and
        // may replace it (`bind_name_in_current_scope` allows Module over a
        // type-kind entry). Any other collision still fails closed.
        if !existing_kind.is_some_and(SymbolKind::is_type) && inferred {
            errors.push(
                SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, span)
                    .with_arg("issue", "inferred_import_name_collision"),
            );
            return;
        }
    }

    define_symbol(resolver, errors, name, span, SymbolKind::Module);
}

fn define_symbol(
    resolver: &mut Resolver,
    errors: &mut Vec<SemanticError>,
    name: radix_lexer::Symbol,
    span: radix_lexer::Span,
    kind: SymbolKind,
) -> Option<radix_hir::DefId> {
    let def_id = resolver.fresh_def_id();
    let symbol = Symbol {
        def_id,
        name,
        kind,
        ty: None,
        mutable: false,
        span,
    };

    if resolver.define(symbol).is_err() {
        duplicate_definition_error(errors, span);
        None
    } else {
        Some(def_id)
    }
}

/// Raise SEM005 for a duplicate top-level or variant definition name.
fn duplicate_definition_error(errors: &mut Vec<SemanticError>, span: radix_lexer::Span) {
    errors.push(
        SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, span)
            .with_arg("issue", "duplicate_definition"),
    );
}
