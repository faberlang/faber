//! `@ ad '<route>'` handler legality and per-file route collection.
//!
//! A Faber function serves a route by carrying `@ ad '<route>'` (en
//! `@ call '<route>'`). This pass owns the per-file rules of the delivery spec
//! (§2.2 legality, §2.3 handler arity) and turns the analyzed handlers into
//! portable [`RouteExport`] facts for the file interface. The program-wide
//! table and cross-module duplicate detection belong to `radix-program`.
//!
//! RULES
//! =====
//! - Legal only on a top-level, non-generic `functio` with a body; methods,
//!   `@ nucleum` kernels, `@ externa` and bodyless declarations are rejected.
//! - Exactly one `@ ad` per function.
//! - Zero or one parameter (the opener).
//! - Route shape: non-empty ASCII `prefix:name` or `prefix/name`, no
//!   whitespace. The `runtime:` prefix and every builtin route are reserved.
//! - A route served twice in one file is a duplicate definition.
//! - Shadowing a host route is legal (tier 1 wins over tier 3), and a handler
//!   needs no export: the route is its public surface.

use crate::file_interface::{
    FileInterfaceResult, InterfaceLibraryIdentity, RouteExport, RouteHandlerKind,
    snapshot_interface_type_with_identity,
};
use crate::{Resolver, SemanticError, SemanticErrorKind, TypeTable};
use radix_hir::{HirItemKind, HirModule};
use radix_lexer::{Interner, Span, Symbol};
use radix_syntax::{
    Annotation, AnnotationKind, ClassMemberKind, FuncDecl, Program, Stmt, StmtKind,
};
use radix_types::{Primitive, Type};
use std::collections::BTreeMap;

/// Routes answered in-process by the language runtime (tier 2). A Faber
/// handler may not claim them. Mirror of the runtime `is_builtin_route` and
/// the faber plan-time `is_builtin_ad_route`.
pub const BUILTIN_AD_ROUTES: &[&str] = &["runtime:echo", "processus:exi"];

/// Route prefix reserved for the language runtime.
pub const RESERVED_AD_ROUTE_PREFIX: &str = "runtime";

/// Whether `route` has the exact-route shape `prefix:name` / `prefix/name`.
#[must_use]
pub fn ad_route_shape_is_valid(route: &str) -> bool {
    let Some(split) = route.find([':', '/']) else {
        return false;
    };
    let (prefix, name) = (&route[..split], &route[split + 1..]);
    !prefix.is_empty()
        && !name.is_empty()
        && route
            .chars()
            .all(|c| c.is_ascii_graphic() && c != '\'' && c != '"')
}

/// Whether `route` is reserved for the runtime (its prefix or a builtin).
#[must_use]
pub fn ad_route_is_reserved(route: &str) -> bool {
    BUILTIN_AD_ROUTES.contains(&route)
        || route
            .split([':', '/'])
            .next()
            .is_some_and(|prefix| prefix == RESERVED_AD_ROUTE_PREFIX)
}

fn invalid(span: Span, issue: &'static str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::AnnotationInvalid, span).with_arg("issue", issue)
}

fn ad_annotations(annotations: &[Annotation]) -> impl Iterator<Item = (&Annotation, Symbol)> {
    annotations
        .iter()
        .filter_map(|annotation| match &annotation.kind {
            AnnotationKind::Ad(ad) => Some((annotation, ad.route)),
            _ => None,
        })
}

fn annotation_named(annotation: &Annotation, interner: &Interner, family: &str) -> bool {
    match &annotation.kind {
        AnnotationKind::Statement(stmt) => interner.resolve(stmt.name.name) == family,
        AnnotationKind::Braced(braced) => interner.resolve(braced.name.name) == family,
        _ => false,
    }
}

/// Validate every `@ ad` placement and handler shape in one parsed file.
///
/// # Errors
///
/// Returns every legality violation; the file keeps no partial route set.
pub fn check(program: &Program, interner: &Interner) -> Result<(), Vec<SemanticError>> {
    let mut errors = Vec::new();
    // route → first handler name, for the in-file duplicate rule.
    let mut served: BTreeMap<String, String> = BTreeMap::new();
    for stmt in &program.statements {
        check_stmt(stmt, interner, &mut served, &mut errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn check_stmt(
    stmt: &Stmt,
    interner: &Interner,
    served: &mut BTreeMap<String, String>,
    errors: &mut Vec<SemanticError>,
) {
    match &stmt.kind {
        StmtKind::Func(func) => check_handler(stmt, func, interner, served, errors),
        StmtKind::Class(class) => {
            reject_misplaced(&stmt.annotations, errors);
            for member in &class.members {
                reject_misplaced(&member.annotations, errors);
                if let ClassMemberKind::Method(method) = &member.kind {
                    reject_misplaced(&method.annotations, errors);
                }
            }
        }
        StmtKind::Interface(iface) => {
            reject_misplaced(&stmt.annotations, errors);
            for method in &iface.methods {
                reject_misplaced(&method.annotations, errors);
            }
        }
        _ => reject_misplaced(&stmt.annotations, errors),
    }
}

fn reject_misplaced(annotations: &[Annotation], errors: &mut Vec<SemanticError>) {
    for (annotation, _) in ad_annotations(annotations) {
        errors.push(invalid(annotation.span, "ad_handler_not_top_level_functio"));
    }
}

fn check_handler(
    stmt: &Stmt,
    func: &FuncDecl,
    interner: &Interner,
    served: &mut BTreeMap<String, String>,
    errors: &mut Vec<SemanticError>,
) {
    let all = || stmt.annotations.iter().chain(func.annotations.iter());
    let ads: Vec<(&Annotation, Symbol)> = ad_annotations(&stmt.annotations)
        .chain(ad_annotations(&func.annotations))
        .collect();
    let Some(&(first, route_sym)) = ads.first() else {
        return;
    };
    let span = first.span;
    for (extra, _) in ads.iter().skip(1) {
        errors.push(invalid(extra.span, "ad_handler_multiple_routes"));
    }
    if !func.type_params.is_empty() {
        errors.push(invalid(span, "ad_handler_generic"));
    }
    if all().any(|a| matches!(a.kind, AnnotationKind::Nucleum(_))) {
        errors.push(invalid(span, "ad_handler_kernel"));
    }
    if all().any(|a| annotation_named(a, interner, "externa")) {
        errors.push(invalid(span, "ad_handler_externa"));
    } else if func.body.is_none() {
        errors.push(invalid(span, "ad_handler_bodyless"));
    }
    if func.params.len() > 1 {
        errors.push(
            invalid(span, "ad_handler_arity").with_arg("count", func.params.len().to_string()),
        );
    }

    let route = interner.resolve(route_sym);
    if !ad_route_shape_is_valid(route) {
        errors.push(invalid(span, "ad_route_shape").with_arg("route", route));
        return;
    }
    if ad_route_is_reserved(route) {
        errors.push(invalid(span, "ad_route_reserved").with_arg("route", route));
        return;
    }
    let handler = interner.resolve(func.name.name).to_owned();
    if let Some(first_handler) = served.get(route) {
        errors.push(
            SemanticError::structured(SemanticErrorKind::DefinitionDuplicate, span)
                .with_arg("issue", "ad_route_duplicate")
                .with_arg("route", route)
                .with_arg("first", first_handler.clone())
                .with_arg("second", handler),
        );
        return;
    }
    served.insert(route.to_owned(), handler);
}

/// Collect the routes served by an analyzed module's handlers.
///
/// Reads the `ad` annotation that lowering recorded on each top-level
/// function item; [`check`] already rejected every illegal placement, so a
/// function carrying one is a valid handler.
///
/// # Errors
///
/// Returns [`crate::file_interface::FileInterfaceError`] when an opener or
/// reply type cannot be snapshotted portably.
pub fn route_exports(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
    identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<BTreeMap<String, RouteExport>> {
    let mut routes = BTreeMap::new();
    for item in &hir.items {
        let HirItemKind::Function(func) = &item.kind else {
            continue;
        };
        let Some((annotation, route)) = item
            .annotations
            .iter()
            .find_map(|annotation| {
                annotation
                    .ad_route(interner)
                    .map(|route| (annotation, route))
            })
        else {
            continue;
        };
        let snapshot =
            |ty| snapshot_interface_type_with_identity(ty, types, interner, resolver, identity);
        let opener = func
            .params
            .first()
            .map(|param| snapshot(param.ty))
            .transpose()?;
        let reply = func
            .ret_ty
            .filter(|ty| !matches!(types.get(*ty), Type::Primitive(Primitive::Vacuum)))
            .map(snapshot)
            .transpose()?;
        let kind = match (func.is_async, func.is_generator) {
            (false, false) => RouteHandlerKind::Sync,
            (true, false) => RouteHandlerKind::Async,
            (false, true) => RouteHandlerKind::Cursor,
            (true, true) => RouteHandlerKind::AsyncCursor,
        };
        let route = interner.resolve(route).to_owned();
        routes.insert(
            route.clone(),
            RouteExport {
                route,
                handler: interner.resolve(func.name).to_owned(),
                opener,
                reply,
                kind,
                failable: func.err_ty.is_some(),
                span: annotation.span,
            },
        );
    }
    Ok(routes)
}

#[cfg(test)]
#[path = "ad_routes_test.rs"]
mod tests;
