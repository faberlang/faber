use std::collections::BTreeSet;

/// Plan-time route classification for the builtin runtime dispatch.
///
/// `runtime:echo` and the terminal builtin `processus:exi` are served
/// in-process. Shared leaves (`solum:*`, `consolum:*`, `processus:*`,
/// `tempus:*`, `aleator:*`) are `faber-leaf` on both runtimes (core-norma N2).
/// Every other route is host-required: the native host route and its
/// provider crates were dropped with `hosts` (single-repo-development Q3).
/// This mirror is a radix-owned plan fact and must stay aligned with the
/// runtime-side `is_builtin_route` coverage plus `faber_leaf::is_route`.
pub(crate) fn is_builtin_ad_route(route: &str) -> bool {
    matches!(route, "runtime:echo" | "processus:exi")
}

/// Whether the shared native leaf crate serves this route.
#[must_use]
pub(crate) fn is_leaf_ad_route(route: &str) -> bool {
    faber_leaf::is_route(route)
}

/// Non-`runtime:` routes that are **not** covered by builtin dispatch or the
/// shared leaf and therefore fail closed at plan time.
pub(crate) fn host_required_routes(routes: &BTreeSet<String>) -> BTreeSet<String> {
    routes
        .iter()
        .filter(|route| !is_builtin_ad_route(route) && !is_leaf_ad_route(route))
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "dispatch_test.rs"]
mod tests;
