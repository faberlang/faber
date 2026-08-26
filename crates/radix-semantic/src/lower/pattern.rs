//! Pattern lowering for `discerne` and other binding-sensitive forms.
//!
//! Patterns sit on a different boundary than ordinary expressions: an
//! identifier may either refer to an existing variant or introduce a fresh local
//! binding. This module makes that distinction explicit by resolving known
//! variants through the resolver while allocating new `DefId`s for names that
//! the pattern binds.
//!
//! INVARIANTS
//! ==========
//! - Fresh pattern bindings are immediately added to the lowering scope so guard
//!   and arm bodies can resolve them like ordinary locals.
//! - Variant patterns preserve the resolved variant `DefId`; constructor arity
//!   and payload typing remain typecheck responsibilities.
//! - Literal patterns lower through the same HIR literal taxonomy used by
//!   expressions, keeping match analysis and expression analysis aligned.
//! - Unsupported pattern policies still emit diagnostics and choose a recoverable
//!   HIR shape instead of aborting lowering.
//!
//! ERROR STRATEGY
//! ==============
//! Mutable pattern bindings are parsed but not semantically implemented here.
//! Lowering records the diagnostic and keeps the binding structure so later
//! phases can still report useful errors in the rest of the arm.

use super::Lowerer;
use radix_hir::{HirLiteral, HirPattern};
use radix_lexer::Span;
use radix_syntax::{
    AstMutability, Literal, PathPattern, Pattern, PatternBind, TypeExpr, TypeExprKind,
};

/// Lower a parser pattern into HIR while establishing any new bindings.
///
/// This entry point preserves the central pattern distinction: variant names
/// refer to existing definitions, while binding names allocate fresh `DefId`s
/// in the current scope. It intentionally does not validate constructor payload
/// types or exhaustiveness; those remain later analysis responsibilities.
pub fn lower_pattern(lowerer: &mut Lowerer, pattern: &Pattern) -> HirPattern {
    match pattern {
        Pattern::Wildcard(span) => {
            lowerer.current_span = *span;
            HirPattern::Wildcard
        }
        Pattern::Ident(ident, bind) => lower_ident_pattern(lowerer, ident, bind.as_ref()),
        Pattern::Type(ident, bind) => lower_type_pattern(lowerer, ident, bind.as_ref()),
        Pattern::Literal(lit, span) => lower_literal(lowerer, lit, *span),
        Pattern::Path(path) => lower_path_pattern(lowerer, path),
    }
}

fn lower_type_pattern(
    lowerer: &mut Lowerer,
    ident: &radix_syntax::Ident,
    bind: Option<&PatternBind>,
) -> HirPattern {
    lowerer.current_span = ident.span;
    let ty_expr = TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Named(ident.clone(), Vec::new()),
        span: ident.span,
        iuncta_label: None,
    };
    let ty = lowerer.lower_type(&ty_expr);
    let candidate = match lowerer.resolver.lookup_variant_bare(ident.name) {
        crate::BareVariantLookup::Single(def_id) => Some(def_id),
        crate::BareVariantLookup::Ambiguous { .. } | crate::BareVariantLookup::None => None,
    };
    let binding = match bind {
        None => None,
        Some(PatternBind::Alias(alias)) => {
            let def_id = lowerer.next_def_id();
            lowerer.bind_local(alias.name, def_id);
            Some((def_id, alias.name))
        }
        Some(PatternBind::Bindings {
            mutability,
            bindings,
        }) => {
            if *mutability == AstMutability::Mutable {
                lowerer.error("mutable_pattern_binding");
            }
            let Some(field) = bindings.first() else {
                lowerer.error("type_pattern_binding_required");
                return HirPattern::TypeTest {
                    ty,
                    candidate,
                    binding: None,
                };
            };
            if bindings.len() > 1 {
                lowerer.error("type_pattern_binding_arity");
            }
            let name = field.alias.as_ref().unwrap_or(&field.name);
            let def_id = lowerer.next_def_id();
            lowerer.bind_local(name.name, def_id);
            Some((def_id, name.name))
        }
    };
    HirPattern::TypeTest {
        ty,
        candidate,
        binding,
    }
}

fn lower_ident_pattern(
    lowerer: &mut Lowerer,
    ident: &radix_syntax::Ident,
    bind: Option<&PatternBind>,
) -> HirPattern {
    lowerer.current_span = ident.span;
    // Ident patterns are ambiguous until resolver state is consulted: a known
    // variant is a constructor pattern, otherwise the name introduces a binding.
    // Variants no longer occupy the bare scope table, so a failed bare lookup
    // falls back to the per-union namespace (exactly one defining union keeps
    // today's single-union behavior; a multi-union bare name is reported by the
    // resolve pass and must never silently become a binding here — it falls
    // through to the non-variant binding path only when the diagnostic already
    // failed closed at resolve time).
    let known_variant = lowerer
        .resolver
        .lookup(ident.name)
        .filter(|def_id| {
            matches!(
                lowerer
                    .resolver
                    .get_symbol(*def_id)
                    .map(|symbol| symbol.kind),
                Some(crate::SymbolKind::Variant)
            )
        })
        .or_else(|| match lowerer.resolver.lookup_variant_bare(ident.name) {
            crate::BareVariantLookup::Single(def_id) => Some(def_id),
            crate::BareVariantLookup::Ambiguous { .. } | crate::BareVariantLookup::None => None,
        });
    if let Some(def_id) = known_variant {
        return match bind {
            None => HirPattern::Variant(def_id, Vec::new()),
            Some(PatternBind::Alias(alias)) => {
                lower_alias_pattern(lowerer, alias, HirPattern::Variant(def_id, Vec::new()))
            }
            Some(PatternBind::Bindings {
                mutability,
                bindings,
            }) => {
                if *mutability == AstMutability::Mutable {
                    lowerer.error("mutable_pattern_binding");
                }
                HirPattern::Variant(def_id, lower_pattern_bindings(lowerer, bindings))
            }
        };
    }

    let def_id = lowerer.next_def_id();
    lowerer.bind_local(ident.name, def_id);

    match bind {
        None => HirPattern::Binding(def_id, ident.name),
        Some(PatternBind::Alias(alias)) => {
            lower_alias_pattern(lowerer, alias, HirPattern::Binding(def_id, ident.name))
        }
        Some(PatternBind::Bindings {
            mutability,
            bindings,
        }) => {
            if *mutability == AstMutability::Mutable {
                lowerer.error("mutable_pattern_binding");
            }
            HirPattern::Variant(def_id, lower_pattern_bindings(lowerer, bindings))
        }
    }
}

/// Lower a path pattern as an explicit variant reference.
///
/// Path syntax is already a reference form, so this does not allocate a binding
/// for the terminal segment. Payload names still allocate fresh locals.
/// A two-segment path (`A.X`) resolves the terminal segment against the
/// penultimate segment's union when it names one. A path with leading path
/// segments (`a.A.X`) resolves the leading path as a real qualifier first — a
/// prefix that is not a real qualifier fails closed as an unknown variant, so
/// `missing.A.X` must never silently bind the local `A.X` (CG2-R07); a
/// qualified imported parent resolves through the imported union's parent link
/// (CTO UVN-1). Otherwise the terminal segment resolves through the bare table
/// and the per-union namespace, so single-union bare variants keep resolving
/// exactly as before.
fn lower_path_pattern(lowerer: &mut Lowerer, path: &PathPattern) -> HirPattern {
    lowerer.current_span = path.span;
    let Some(last) = path.segments.last() else {
        lowerer.error("empty_path_pattern");
        return HirPattern::Wildcard;
    };
    let def_id = if path.segments.len() >= 3 {
        // CG2-R07 mirror: a path with leading path segments must resolve the
        // leading path as a real qualifier before the penultimate segment can
        // name the parent union — `casu missing.A.X { }` must never bind the
        // local `A.X`, and a shadowing local enum must never capture a
        // qualified imported pattern.
        let symbols: Vec<radix_lexer::Symbol> =
            path.segments.iter().map(|ident| ident.name).collect();
        match lowerer.resolver.qualified_variant_path(&symbols) {
            crate::QualifiedVariantPathLookup::Variant(def_id) => def_id,
            crate::QualifiedVariantPathLookup::UnknownVariant
            | crate::QualifiedVariantPathLookup::NotQualified => {
                lowerer.error("unknown_variant");
                return HirPattern::Wildcard;
            }
        }
    } else if path.segments.len() >= 2 {
        let parent = &path.segments[path.segments.len() - 2];
        if let Some(Some(variant_def)) =
            lowerer.resolver.lookup(parent.name).and_then(|parent_def| {
                lowerer
                    .resolver
                    .get_symbol(parent_def)
                    .filter(|symbol| symbol.kind == crate::SymbolKind::Enum)
                    .map(|_| lowerer.resolver.variant_child(parent_def, last.name))
            })
        {
            variant_def
        } else {
            // The penultimate segment names no visible enum (imported types are
            // reached through their namespace, so `token.TokenKind.Palabra`
            // never binds `TokenKind` in scope). A qualified imported parent
            // (`a.A.X`) resolves through the imported union's parent link
            // BEFORE any bare fallback (CTO UVN-1): two imported unions may
            // share the terminal variant name, and a miss against an
            // identified variant-bearing parent fails closed instead of
            // synthesizing an identity or binding through another union.
            // Anything else falls back to the terminal segment's own identity
            // through the bare table and the per-union namespace — exactly the
            // single-segment policy (uvf-u1).
            let symbols: Vec<radix_lexer::Symbol> =
                path.segments.iter().map(|ident| ident.name).collect();
            match lowerer.resolver.qualified_variant_path(&symbols) {
                crate::QualifiedVariantPathLookup::Variant(def_id) => def_id,
                crate::QualifiedVariantPathLookup::UnknownVariant => {
                    lowerer.error("unknown_variant");
                    return HirPattern::Wildcard;
                }
                crate::QualifiedVariantPathLookup::NotQualified => lowerer
                    .resolver
                    .lookup(last.name)
                    .or_else(|| match lowerer.resolver.lookup_variant_bare(last.name) {
                        crate::BareVariantLookup::Single(def_id) => Some(def_id),
                        crate::BareVariantLookup::Ambiguous { .. }
                        | crate::BareVariantLookup::None => None,
                    })
                    .unwrap_or_else(|| lowerer.next_def_id()),
            }
        }
    } else {
        lowerer
            .resolver
            .lookup(last.name)
            .or_else(|| match lowerer.resolver.lookup_variant_bare(last.name) {
                crate::BareVariantLookup::Single(def_id) => Some(def_id),
                crate::BareVariantLookup::Ambiguous { .. } | crate::BareVariantLookup::None => None,
            })
            .unwrap_or_else(|| lowerer.next_def_id())
    };

    match path.bind.as_ref() {
        None => HirPattern::Variant(def_id, Vec::new()),
        Some(PatternBind::Alias(alias)) => {
            lower_alias_pattern(lowerer, alias, HirPattern::Variant(def_id, Vec::new()))
        }
        Some(PatternBind::Bindings {
            mutability,
            bindings,
        }) => {
            if *mutability == AstMutability::Mutable {
                lowerer.error("mutable_pattern_binding");
            }
            HirPattern::Variant(def_id, lower_pattern_bindings(lowerer, bindings))
        }
    }
}

/// Allocate fresh local bindings for a variant payload list.
fn lower_pattern_bindings(
    lowerer: &mut Lowerer,
    bindings: &[radix_syntax::ExField],
) -> Vec<HirPattern> {
    bindings
        .iter()
        .map(|binding| {
            if let Some(alias) = &binding.alias {
                let label_def = lowerer.next_def_id();
                lower_alias_pattern(
                    lowerer,
                    alias,
                    HirPattern::Binding(label_def, binding.name.name),
                )
            } else {
                let def_id = lowerer.next_def_id();
                lowerer.bind_local(binding.name.name, def_id);
                HirPattern::Binding(def_id, binding.name.name)
            }
        })
        .collect()
}

/// Bind an alias name to the whole lowered pattern shape.
///
/// The alias is a fresh local even when the wrapped pattern resolves to a
/// variant. This lets arm bodies refer to both destructured payloads and the
/// original matched value when later phases support that distinction.
fn lower_alias_pattern(
    lowerer: &mut Lowerer,
    alias: &radix_syntax::Ident,
    pattern: HirPattern,
) -> HirPattern {
    lowerer.current_span = alias.span;
    let alias_def_id = lowerer.next_def_id();
    lowerer.bind_local(alias.name, alias_def_id);
    HirPattern::Alias(alias_def_id, alias.name, Box::new(pattern))
}

/// Lower a literal pattern through the expression literal taxonomy.
pub fn lower_literal(lowerer: &mut Lowerer, lit: &Literal, span: Span) -> HirPattern {
    lowerer.current_span = span;
    let literal = match lit {
        Literal::JsonValor(_) => {
            lowerer.error("json_valor_pattern");
            HirLiteral::Nil
        }
        Literal::Forma(_) => {
            lowerer.error("forma_literal_pattern");
            HirLiteral::Nil
        }
        _ => match super::literal::literal_to_hir(lit) {
            Ok(literal) => literal,
            Err(message) => {
                lowerer.error_from_message(message);
                HirLiteral::Nil
            }
        },
    };

    HirPattern::Literal(literal)
}
