use super::runtime_sources::{path_override_root, SupportCrateSource};
use radix_module::diagnostics::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderManifest {
    pub manifest_version: u32,
    pub provider: String,
    pub owner: String,
    pub prefixes: Vec<String>,
    pub calls: Vec<ProviderCall>,
    #[serde(default)]
    pub native_dependencies: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderCall {
    pub route: String,
    pub summary: String,
    pub opener: String,
    pub result: String,
}

pub(crate) fn selected_providers_for_routes(
    routes: &BTreeSet<String>,
    explicit: &[String],
) -> BTreeSet<String> {
    let mut providers = explicit.iter().cloned().collect::<BTreeSet<_>>();
    for route in routes {
        if let Some(provider) = route.split([':', '/']).next() {
            if !provider.is_empty() {
                providers.insert(provider.to_owned());
            }
        }
    }
    providers
}

/// Plan-time route classification for the builtin runtime dispatch
/// (dual-backend contract).
///
/// These routes do **not** require `[target.rust] host = "native"`: the
/// builtin runtime covers them, so a native host stays optional. The S1-U3
/// provider split removed the pre-split builtin table from
/// `faber/runtime/rust/src/frame.rs`; only `runtime:echo` and the terminal
/// builtin `processus:exi` are restored in-process. `processus:exi` stays
/// unmanifested on the host (no protocol-visible terminal response yet) and
/// must not be demanded as a provider export. Every other route is a
/// host-required provider route and must be backed by a selected host provider
/// under `[target.rust] host`. This mirror is a radix-owned plan fact and must
/// stay aligned with the runtime-side `is_builtin_route` coverage.
pub(crate) fn is_builtin_ad_route(route: &str) -> bool {
    matches!(route, "runtime:echo" | "processus:exi")
}

/// Non-`runtime:` routes that are **not** covered by builtin dispatch and therefore
/// require `[target.rust] host` (or fail closed at plan time).
pub(crate) fn host_required_routes(routes: &BTreeSet<String>) -> BTreeSet<String> {
    routes
        .iter()
        .filter(|route| !is_builtin_ad_route(route))
        .cloned()
        .collect()
}

pub(crate) fn load_provider_manifests(
    providers: &BTreeSet<String>,
    routes: &BTreeSet<String>,
) -> Result<Vec<ProviderManifest>, Diagnostic> {
    // D1 (2026-08-14): host provider sources are no longer embedded; their
    // manifests are read from the container-local checkouts behind the
    // offline path-override seam.
    let Some(override_root) = path_override_root() else {
        return Err(crate::package_diagnostic_error(format!(
            "host provider manifests require the offline support path override: \
             set {} to the container root (dev/container builds)",
            super::runtime_sources::SUPPORT_PATH_OVERRIDE_ENV
        ))
        .with_arg("issue", "support_path_override_required"));
    };
    let mut manifests = Vec::new();
    for provider in providers {
        let Some(source) = SupportCrateSource::provider(provider) else {
            return Err(crate::package_diagnostic_error(format!(
                "unsupported host provider `{provider}`"
            ))
            .with_arg("issue", "host_provider_unsupported")
            .with_arg("provider", provider.clone()));
        };
        let path = source
            .override_crate_root(&override_root)
            .join("src/manifest.json");
        let source_text = fs::read_to_string(&path).map_err(|error| {
            crate::package_diagnostic_error(format!(
                "selected host provider `{provider}` has no readable manifest: {error}"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "host_provider_manifest_missing")
        })?;
        let manifest = serde_json::from_str::<ProviderManifest>(&source_text).map_err(|error| {
            crate::package_diagnostic_error(format!(
                "invalid host provider manifest `{provider}`: {error}"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "host_provider_manifest_invalid")
        })?;
        if manifest.manifest_version != 1 || manifest.provider != *provider {
            return Err(crate::package_diagnostic_error(format!(
                "host provider manifest `{provider}` has mismatched identity or version"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "host_provider_manifest_identity"));
        }
        manifests.push(manifest);
    }
    let mut exported = BTreeSet::new();
    for manifest in &manifests {
        for call in &manifest.calls {
            if !exported.insert(call.route.as_str()) {
                return Err(crate::package_diagnostic_error(format!(
                    "selected host providers export duplicate route `{}`",
                    call.route
                ))
                .with_arg("issue", "host_provider_route_duplicate")
                .with_arg("route", call.route.clone()));
            }
        }
    }
    if let Some(route) = routes
        .iter()
        .find(|route| !exported.contains(route.as_str()))
    {
        return Err(crate::package_diagnostic_error(format!(
            "selected host providers do not export required route `{route}`"
        ))
        .with_arg("issue", "host_provider_route_missing")
        .with_arg("route", route.clone()));
    }
    Ok(manifests)
}

#[cfg(test)]
mod tests {
    use super::{host_required_routes, is_builtin_ad_route};
    use std::collections::BTreeSet;

    #[test]
    fn builtin_matcher_covers_echo_and_terminal_exi() {
        assert!(is_builtin_ad_route("runtime:echo"));
        assert!(is_builtin_ad_route("processus:exi"));
        assert!(!is_builtin_ad_route("processus:argumenta"));
        assert!(!is_builtin_ad_route("solum:lege"));
    }

    #[test]
    fn host_required_routes_drop_terminal_exi() {
        let routes = BTreeSet::from(["processus:argumenta".to_owned(), "processus:exi".to_owned()]);
        let required = host_required_routes(&routes);
        assert!(required.contains("processus:argumenta"));
        assert!(!required.contains("processus:exi"));
    }
}
