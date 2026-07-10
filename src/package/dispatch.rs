use radix::diagnostics::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

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

pub(crate) fn load_provider_manifests(
    providers: &BTreeSet<String>,
    routes: &BTreeSet<String>,
) -> Result<Vec<ProviderManifest>, Diagnostic> {
    let mut manifests = Vec::new();
    for provider in providers {
        let path = provider_manifest_path(provider);
        let source = fs::read_to_string(&path).map_err(|error| {
            Diagnostic::error(format!(
                "selected host provider `{provider}` has no readable manifest: {error}"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "host_provider_manifest_missing")
        })?;
        let manifest = serde_json::from_str::<ProviderManifest>(&source).map_err(|error| {
            Diagnostic::error(format!(
                "invalid host provider manifest `{provider}`: {error}"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "host_provider_manifest_invalid")
        })?;
        if manifest.manifest_version != 1 || manifest.provider != *provider {
            return Err(Diagnostic::error(format!(
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
                return Err(Diagnostic::error(format!(
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
        return Err(Diagnostic::error(format!(
            "selected host providers do not export required route `{route}`"
        ))
        .with_arg("issue", "host_provider_route_missing")
        .with_arg("route", route.clone()));
    }
    Ok(manifests)
}

pub(crate) fn provider_manifest_path(provider: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../host-providers-rs/crates")
        .join(provider)
        .join("src/manifest.json")
}

pub(crate) fn provider_crate_path(provider: &str) -> PathBuf {
    provider_manifest_path(provider)
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}
