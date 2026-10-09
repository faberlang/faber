use std::collections::BTreeSet;

/// Plan-time route classification for the builtin runtime dispatch.
///
/// Only `runtime:echo` and the terminal builtin `processus:exi` are served
/// in-process. Every other route is host-required: the native host route and
/// its provider crates were dropped with `hosts` (single-repo-development Q3),
/// so a package that needs one fails closed at plan time until core-norma N2
/// supplies native I/O. This mirror is a radix-owned plan fact and must stay
/// aligned with the runtime-side `is_builtin_route` coverage.
pub(crate) fn is_builtin_ad_route(route: &str) -> bool {
    matches!(route, "runtime:echo" | "processus:exi")
}

/// Non-`runtime:` routes that are **not** covered by builtin dispatch and
/// therefore fail closed at plan time.
pub(crate) fn host_required_routes(routes: &BTreeSet<String>) -> BTreeSet<String> {
    routes
        .iter()
        .filter(|route| !is_builtin_ad_route(route))
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "dispatch_test.rs"]
mod tests;
