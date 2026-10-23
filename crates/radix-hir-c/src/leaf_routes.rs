//! The C leaf-header route table: which `ad` routes dispatch to a
//! `faber_route_<prefix>_<name>` wrapper, and which header declares them.
//!
//! The route text is the whole identity here, so this module does not depend
//! on `radix-target`: the leaf gate (`CODEGEN001 leaf_unsupported_on_target`)
//! is the target-level admission, and it runs before the emitter. This table
//! is the second admission, and it is the only one that admits a route on `c`:
//! the leaf unit-set appends a prefix when it ships `runtime/c/<prefix>.h`
//! with its `faber_route_*` wrappers. While the table is empty every route
//! keeps the emitter's refusal, so the arm is wired and stays fail-closed.

/// Leaf prefixes whose header (and route wrappers) the C runtime ships.
///
/// The leaf unit-set adds `"solum"`, `"consolum"`, … here as it lands each
/// `runtime/c/<prefix>.h`; until then no route is declared.
pub(crate) const DECLARED_PREFIXES: &[&str] = &[];

/// The C symbol a route dispatches to, or `None` when the route is not a leaf
/// route.
///
/// `<prefix>:<name>` becomes `faber_route_<prefix>_<name>`. The builtins
/// (`runtime:*`, `processus:exi`) are the runtime's own and never a leaf
/// wrapper, and a route outside the `<prefix>:<name>` shape has no wrapper.
pub(crate) fn route_symbol(route: &str) -> Option<String> {
    if route.starts_with("runtime:") || route == "processus:exi" {
        return None;
    }
    let (prefix, name) = route.split_once(':')?;
    if !is_ident_part(prefix) || !is_ident_part(name) {
        return None;
    }
    Some(format!("faber_route_{prefix}_{name}"))
}

/// The header that declares a leaf prefix's routes (`runtime/c/<prefix>.h`).
pub(crate) fn route_header(prefix: &str) -> String {
    format!("{prefix}.h")
}

/// Whether the leaf unit-set has landed `runtime/c/<prefix>.h`.
pub(crate) fn declared(prefix: &str) -> bool {
    DECLARED_PREFIXES.contains(&prefix)
}

/// The `'static` spelling of a declared prefix, the one a [`Helper`] row can
/// carry.
///
/// [`Helper`]: crate::helpers::Helper
pub(crate) fn declared_prefix(prefix: &str) -> Option<&'static str> {
    DECLARED_PREFIXES
        .iter()
        .copied()
        .find(|declared| *declared == prefix)
}

/// Whether `text` can be one underscore-joined piece of a C identifier.
fn is_ident_part(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
