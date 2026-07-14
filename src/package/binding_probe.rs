//! Rust compile-probe orchestration for Faber library bindings.
//!
//! Radix renders exact ABI wrappers. This module owns the temporary Cargo
//! crate, target dependencies, shim attachment, timeout, and diagnostics.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use radix::diagnostics::Diagnostic;

use super::runtime_dependency::{
    normalize_dependency_value, parse_dependency_requirement, runtime_path_for_target_dependencies,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
static NEXT_PROBE_ID: AtomicU64 = AtomicU64::new(0);
// Cargo probes share registry and build locks across test threads. Gate probe
// children so the timeout measures compile time, not waiting behind siblings.
static PROBE_GATE: Mutex<()> = Mutex::new(());
static SUCCESSFUL_PROBES: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();

struct ProbeChild {
    child: Child,
}

impl ProbeChild {
    fn new(child: Child) -> Self {
        Self { child }
    }
}

impl Drop for ProbeChild {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                // WHY: Drop is the last-resort cleanup for early diagnostic
                // returns; the normal timeout path reports kill/wait failures.
                drop(self.child.kill());
                drop(self.child.wait());
            }
        }
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn run_rust_binding_probe(
    package_root: &Path,
    anchor: &Path,
    dependencies: &BTreeMap<String, String>,
    shim: Option<&Path>,
    probes: &[String],
) -> Result<(), Diagnostic> {
    let key = probe_key(package_root, anchor, dependencies, shim, probes);
    if successful_probes()
        .lock()
        .map(|cache| cache.contains(&key))
        .unwrap_or(false)
    {
        return Ok(());
    }

    let _gate = PROBE_GATE.lock().map_err(|_| {
        Diagnostic::error("Rust binding probe gate is poisoned")
            .with_file(anchor.display().to_string())
            .with_arg("issue", "binding_probe_gate_poisoned")
    })?;
    if successful_probes()
        .lock()
        .map(|cache| cache.contains(&key))
        .unwrap_or(false)
    {
        return Ok(());
    }

    let root = probe_root();
    let result = run_probe_in(&root, package_root, anchor, dependencies, shim, probes);
    if result.is_ok() {
        if let Ok(mut cache) = successful_probes().lock() {
            cache.insert(key);
        }
    }
    match fs::remove_dir_all(&root) {
        Ok(()) => result,
        Err(cleanup_error) => match result {
            Ok(()) => Err(Diagnostic::io_error(&root, cleanup_error)
                .with_arg("issue", "binding_probe_cleanup_failed")),
            Err(mut diagnostic) => {
                diagnostic.message.push_str(&format!(
                    "\nfailed to remove probe directory {}: {cleanup_error}",
                    root.display()
                ));
                Err(diagnostic)
            }
        },
    }
}

fn successful_probes() -> &'static Mutex<BTreeSet<String>> {
    SUCCESSFUL_PROBES.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn probe_key(
    package_root: &Path,
    anchor: &Path,
    dependencies: &BTreeMap<String, String>,
    shim: Option<&Path>,
    probes: &[String],
) -> String {
    let mut key = String::new();
    key.push_str(&canonical_probe_path(package_root));
    key.push('\n');
    key.push_str(&canonical_probe_path(anchor));
    key.push('\n');
    if let Some(shim) = shim {
        key.push_str(&canonical_probe_path(shim));
    }
    key.push('\n');
    for (name, requirement) in dependencies {
        key.push_str(name);
        key.push('=');
        key.push_str(requirement);
        key.push('\n');
    }
    for probe in probes {
        key.push_str(probe);
        key.push('\n');
    }
    key
}

fn canonical_probe_path(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

#[allow(clippy::result_large_err)]
fn run_probe_in(
    root: &Path,
    package_root: &Path,
    anchor: &Path,
    dependencies: &BTreeMap<String, String>,
    shim: Option<&Path>,
    probes: &[String],
) -> Result<(), Diagnostic> {
    let source_dir = root.join("src");
    fs::create_dir_all(&source_dir).map_err(|error| Diagnostic::io_error(&source_dir, error))?;
    let manifest_path = root.join("Cargo.toml");
    let manifest = probe_manifest(package_root, dependencies).map_err(|mut error| {
        error.file = anchor.display().to_string();
        error
    })?;
    fs::write(&manifest_path, manifest)
        .map_err(|error| Diagnostic::io_error(&manifest_path, error))?;
    let source_path = source_dir.join("main.rs");
    fs::write(&source_path, probe_source(shim, probes))
        .map_err(|error| Diagnostic::io_error(&source_path, error))?;

    let stdout_path = root.join("cargo.stdout");
    let stderr_path = root.join("cargo.stderr");
    let stdout =
        File::create(&stdout_path).map_err(|error| Diagnostic::io_error(&stdout_path, error))?;
    let stderr =
        File::create(&stderr_path).map_err(|error| Diagnostic::io_error(&stderr_path, error))?;
    let child = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--target-dir")
        .arg(root.join("target"))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| {
            Diagnostic::error(format!("failed to spawn Rust binding probe: {error}"))
                .with_file(anchor.display().to_string())
                .with_arg("issue", "binding_probe_spawn_failed")
        })?;
    let mut child = ProbeChild::new(child);

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        if let Some(status) = child.child.try_wait().map_err(|error| {
            Diagnostic::error(format!("failed to inspect Rust binding probe: {error}"))
                .with_file(anchor.display().to_string())
                .with_arg("issue", "binding_probe_wait_failed")
        })? {
            break status;
        }
        if Instant::now() >= deadline {
            child.child.kill().map_err(|error| {
                Diagnostic::error(format!(
                    "failed to terminate timed-out Rust binding probe: {error}"
                ))
                .with_file(anchor.display().to_string())
                .with_arg("issue", "binding_probe_kill_failed")
            })?;
            child.child.wait().map_err(|error| {
                Diagnostic::error(format!(
                    "failed to reap timed-out Rust binding probe: {error}"
                ))
                .with_file(anchor.display().to_string())
                .with_arg("issue", "binding_probe_wait_failed")
            })?;
            return Err(
                Diagnostic::error("Rust binding probe timed out after 60 seconds")
                    .with_file(anchor.display().to_string())
                    .with_arg("issue", "binding_probe_timeout"),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    if status.success() {
        return Ok(());
    }
    let stderr = read_output(&stderr_path)?;
    let stdout = read_output(&stdout_path)?;
    Err(Diagnostic::error(format!(
        "Rust binding contract probe failed\n{}{}",
        truncate_output(&stderr),
        truncate_output(&stdout)
    ))
    .with_file(anchor.display().to_string())
    .with_arg("issue", "binding_rust_probe_failed"))
}

#[allow(clippy::result_large_err)]
fn probe_manifest(
    package_root: &Path,
    dependencies: &BTreeMap<String, String>,
) -> Result<String, Diagnostic> {
    let runtime_path = runtime_path_for_target_dependencies(package_root, dependencies)?;
    let mut package = toml::map::Map::new();
    package.insert(
        "name".to_owned(),
        toml::Value::String("faber-binding-probe".to_owned()),
    );
    package.insert(
        "version".to_owned(),
        toml::Value::String("0.0.0".to_owned()),
    );
    package.insert("edition".to_owned(), toml::Value::String("2021".to_owned()));

    let mut dependencies = dependencies
        .iter()
        .map(|(name, requirement)| {
            let value = parse_dependency_requirement(requirement);
            let value = normalize_dependency_value(package_root, value);
            (name.clone(), value)
        })
        .collect::<toml::map::Map<_, _>>();
    let mut runtime = toml::map::Map::new();
    runtime.insert(
        "package".to_owned(),
        toml::Value::String("faber-runtime".to_owned()),
    );
    runtime.insert(
        "path".to_owned(),
        toml::Value::String(runtime_path.display().to_string()),
    );
    let runtime = toml::Value::Table(runtime);
    for (name, value) in &mut dependencies {
        let is_runtime = name == "faber"
            || value
                .as_table()
                .and_then(|table| table.get("package"))
                .and_then(toml::Value::as_str)
                == Some("faber-runtime");
        if is_runtime {
            *value = runtime.clone();
        }
    }
    dependencies.entry("faber".to_owned()).or_insert(runtime);
    let mut manifest = toml::map::Map::new();
    manifest.insert("package".to_owned(), toml::Value::Table(package));
    manifest.insert("dependencies".to_owned(), toml::Value::Table(dependencies));
    toml::to_string(&manifest).map_err(|error| {
        Diagnostic::error(format!(
            "failed to serialize Rust binding probe manifest: {error}"
        ))
        .with_arg("issue", "binding_probe_manifest_serialize_failed")
    })
}

fn probe_source(shim: Option<&Path>, probes: &[String]) -> String {
    let mut source = String::new();
    if let Some(shim) = shim {
        let shim = fs::canonicalize(shim).unwrap_or_else(|_| shim.to_path_buf());
        source.push_str(&format!(
            "#[path = {:?}]\nmod shim;\n\n",
            shim.display().to_string()
        ));
    }
    for probe in probes {
        source.push_str(probe);
        source.push('\n');
    }
    source.push_str("fn main() {}\n");
    source
}

#[allow(clippy::result_large_err)]
fn read_output(path: &Path) -> Result<String, Diagnostic> {
    let mut file = File::open(path).map_err(|error| Diagnostic::io_error(path, error))?;
    let mut output = String::new();
    file.read_to_string(&mut output)
        .map_err(|error| Diagnostic::io_error(path, error))?;
    Ok(output)
}

fn truncate_output(output: &str) -> String {
    output.chars().take(8_000).collect()
}

fn probe_root() -> PathBuf {
    let id = NEXT_PROBE_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos(),
        Err(_) => 0,
    };
    std::env::temp_dir().join(format!(
        "faber-binding-probe-{}-{timestamp}-{id}",
        std::process::id(),
    ))
}

#[cfg(test)]
#[path = "binding_probe_test.rs"]
mod tests;
