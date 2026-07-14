use crate::core_support::materialize::materialize;
use radix::diagnostics::Diagnostic;
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

/// Routes covered by `faber-runtime` `BuiltinRuntimeDispatch` / `builtin_route_frames`.
///
/// Dual-backend contract: these do **not** require `[target.rust] host = "native"`.
/// Keep in sync with `faber-runtime/src/frame.rs` `builtin_route_frames` match arms.
pub(crate) fn is_builtin_ad_route(route: &str) -> bool {
    matches!(
        route,
        "runtime:echo"
            | "tempus:nunc"
            | "tempus:monotonicum"
            | "tempus:activum"
            | "tempus:dormiet"
            | "tempus:expectet"
            | "solum:scribe"
            | "solum:scribet"
            | "solum:appone"
            | "solum:apponet"
            | "solum:funde"
            | "solum:dele"
            | "solum:delet"
            | "solum:parens"
            | "solum:nomen"
            | "solum:suffixum"
            | "solum:iunge"
            | "solum:absolve"
            | "solum:temporarium"
            | "solum:domus"
            | "solum:partem"
            | "processus:exsequi"
            | "processus:exsequetur"
            | "processus:captura"
            | "processus:dimitte"
            | "processus:lege"
            | "processus:scribe"
            | "processus:sedes"
            | "processus:muta"
            | "processus:identitas"
            | "processus:argumenta"
            | "processus:exi"
            | "consolum:dic"
            | "consolum:dicet"
            | "consolum:scribe"
            | "consolum:scribet"
            | "consolum:mone"
            | "consolum:monet"
            | "consolum:vide"
            | "consolum:videbit"
            | "consolum:lege"
            | "consolum:leget"
            | "consolum:hauri"
            | "consolum:hauriet"
            | "consolum:funde"
            | "consolum:audit"
            | "consolum:loquitur"
            | "consolum:admonet"
            | "solum:lege"
            | "solum:hauri"
            | "solum:hauriet"
            | "solum:carpe"
            | "solum:carpiet"
            | "solum:mensura"
            | "solum:inveni"
            | "solum:crea"
            | "solum:creabit"
            | "solum:enumera"
            | "solum:enumerabit"
            | "solum:amputa"
            | "solum:amputabit"
            | "solum:exscribe"
            | "solum:exscribet"
            | "solum:renomina"
            | "solum:renominabit"
            | "solum:tange"
            | "solum:tanget"
            | "solum:sequere"
            | "solum:sequetur"
            | "solum:vincula"
            | "solum:modum"
            | "solum:modus"
            | "solum:exstat"
            | "solum:exstabit"
            | "solum:directoriumne"
            | "solum:regularene"
            | "solum:legibilene"
            | "solum:vinculumne"
            | "aleator:fractum"
            | "aleator:sortire"
            | "aleator:octetos"
            | "aleator:uuid"
            | "aleator:semina"
    )
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

#[allow(clippy::result_large_err)]
pub(crate) fn load_provider_manifests(
    providers: &BTreeSet<String>,
    routes: &BTreeSet<String>,
) -> Result<Vec<ProviderManifest>, Diagnostic> {
    let support = materialize().map_err(|error| {
        crate::package_diagnostic_error(format!("verified core support is unavailable: {error}"))
            .with_arg("issue", "core_support_materialization_failed")
    })?;
    let mut manifests = Vec::new();
    for provider in providers {
        let path = support
            .provider(provider)
            .map_err(|error| {
                crate::package_diagnostic_error(format!(
                    "unsupported host provider `{provider}`: {error}"
                ))
                .with_arg("issue", "host_provider_unsupported")
                .with_arg("provider", provider.clone())
            })?
            .join("src/manifest.json");
        let source = fs::read_to_string(&path).map_err(|error| {
            crate::package_diagnostic_error(format!(
                "selected host provider `{provider}` has no readable manifest: {error}"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "host_provider_manifest_missing")
        })?;
        let manifest = serde_json::from_str::<ProviderManifest>(&source).map_err(|error| {
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
