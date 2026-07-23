//! Clean-install proofs for Faber's embedded core-support payload.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = env::temp_dir().join(format!("faber-clean-install-{label}-{nonce}"));
    fs::create_dir_all(&root).expect("create clean-install root");
    root
}

fn installed_faber(root: &Path) -> PathBuf {
    let install = root.join("install").join("bin");
    fs::create_dir_all(&install).expect("create install bin");
    let executable = install.join("faber");
    fs::copy(env!("CARGO_BIN_EXE_faber"), &executable).expect("copy installed Faber executable");
    executable
}

fn command(executable: &Path, package: &Path, home: &Path) -> Output {
    let mut command = Command::new(executable);
    command
        .arg("build")
        .arg(package)
        .current_dir(package)
        .env("HOME", home)
        .envs(platform_cache_env(home))
        // Cargo remains available from the test toolchain while Faber's
        // platform cache is isolated beneath this fresh HOME.
        .env(
            "CARGO_HOME",
            env::var_os("CARGO_HOME").unwrap_or_else(|| {
                PathBuf::from(env::var("HOME").expect("HOME"))
                    .join(".cargo")
                    .into_os_string()
            }),
        )
        .env(
            "RUSTUP_HOME",
            env::var_os("RUSTUP_HOME").unwrap_or_else(|| {
                PathBuf::from(env::var("HOME").expect("HOME"))
                    .join(".rustup")
                    .into_os_string()
            }),
        );
    command.output().expect("run installed Faber")
}

#[cfg(target_os = "macos")]
fn platform_cache_env(_home: &Path) -> Vec<(&'static str, PathBuf)> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn platform_cache_env(home: &Path) -> Vec<(&'static str, PathBuf)> {
    vec![("LOCALAPPDATA", home.join("AppData/Local"))]
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn platform_cache_env(home: &Path) -> Vec<(&'static str, PathBuf)> {
    vec![("XDG_CACHE_HOME", home.join(".cache"))]
}

#[cfg(target_os = "macos")]
fn expected_cache_root(home: &Path) -> PathBuf {
    home.join("Library/Caches/faber/core-support")
}

#[cfg(target_os = "windows")]
fn expected_cache_root(home: &Path) -> PathBuf {
    home.join("AppData/Local/faber/core-support")
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn expected_cache_root(home: &Path) -> PathBuf {
    home.join(".cache/faber/core-support")
}

fn write_package(root: &Path, native: bool) -> PathBuf {
    let package = root.join("package");
    fs::create_dir_all(package.join("src")).expect("create package source");
    let host = if native {
        "\n[target.rust]\nhost = \"native\"\n"
    } else {
        ""
    };
    fs::write(
        package.join("faber.toml"),
        format!(
            "[package]\nname = \"clean-install\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n{host}"
        ),
    )
    .expect("write manifest");
    package
}

fn generated_manifest(package: &Path) -> String {
    fs::read_to_string(package.join("target/faber/Cargo.toml")).expect("generated Cargo manifest")
}

fn assert_only_materialized_paths(manifest: &str, home: &Path) {
    let cache = expected_cache_root(home);
    assert!(
        manifest.contains(cache.to_string_lossy().as_ref()),
        "missing materialized cache path:\n{manifest}"
    );
    for forbidden in [
        "/faber-runtime",
        "/hosts/crates/host-kernel",
        "/hosts/crates/host-native",
        "/host-kernel-rs",
        "/host-native-rs",
        "faberlang/worktrees",
    ] {
        assert!(
            !manifest.contains(&format!("work/faberlang{forbidden}")),
            "generated manifest retained a sibling-checkout path:\n{manifest}"
        );
    }
}

#[test]
fn installed_faber_builds_minimal_package_without_sibling_core_sources() {
    let root = temp_dir("minimal");
    let executable = installed_faber(&root);
    let home = root.join("home");
    fs::create_dir_all(&home).expect("create isolated home");
    let package = write_package(&root, false);
    fs::write(
        package.join("src/main.fab"),
        "incipit { nota \"clean minimal\" }\n",
    )
    .expect("write source");

    let output = command(&executable, &package, &home);
    assert!(
        output.status.success(),
        "installed build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = generated_manifest(&package);
    assert_only_materialized_paths(&manifest, &home);
    assert!(
        !manifest.contains("hosts/crates/solum") && !manifest.contains("host-providers-rs"),
        "minimal build selected a provider:\n{manifest}"
    );
}

#[test]
fn installed_faber_builds_and_runs_native_provider_using_materialized_sources() {
    let root = temp_dir("native-provider");
    let executable = installed_faber(&root);
    let home = root.join("home");
    fs::create_dir_all(&home).expect("create isolated home");
    let package = write_package(&root, true);
    let data = package.join("data.txt");
    fs::write(&data, "clean provider").expect("write provider data");
    fs::write(
        package.join("src/main.fab"),
        format!(
            "incipit {{\n  fixum textus body ← ad 'solum:lege' ({:?}) ↦ textus\n  nota body\n}}\n",
            data.to_string_lossy()
        ),
    )
    .expect("write source");

    let output = command(&executable, &package, &home);
    assert!(
        output.status.success(),
        "installed native build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = generated_manifest(&package);
    assert_only_materialized_paths(&manifest, &home);
    assert!(
        manifest.contains("solum"),
        "native build omitted selected provider:\n{manifest}"
    );
    for provider in ["aleator", "consolum", "processus", "tempus"] {
        assert!(
            !manifest.contains(provider),
            "native build selected unexpected provider {provider}:\n{manifest}"
        );
    }

    let binary = package.join("target/debug/clean-install");
    let run = Command::new(binary)
        .output()
        .expect("run native provider binary");
    assert!(
        run.status.success(),
        "native provider binary failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "clean provider\n");
}
