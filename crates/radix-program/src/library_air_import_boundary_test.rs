//! GT02: the AIR lane's import boundary, pinned at its first failing phase.
//!
//! A module-qualified call to an imported library function (`nn.scale_layer(x,
//! w)`) lowers to a HIR `MethodCall` (`radix-semantic` `flatten_dotted_method_callee`),
//! and the AIR purity pass admits a method call only when its name is on the
//! tensor-intrinsic allowlist. `scale_layer` is deliberately not an intrinsic,
//! so a library layer called module-qualified inside an
//! `@ radix { lane = "air" }` function declines with `SEM059.method_call`.
//!
//! This pins the phase that declines, not the message text: the assertion is on
//! the diagnostic code and issue name.

use super::*;
use crate::analyze::analyze_package;
use std::fs;
use std::path::Path;

const LIB_MANIFEST: &str = r#"
[package]
name = "libgt"
version = "1.0.0"
edition = "2026"

[library]
provider = "libgt"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fmir"]

[reader]
locale = "en"
"#;

const NN_SOURCE: &str = r#"
@ public
fn scale_layer(tensor<f32, [2, 2]> x, tensor<f32, [2, 2]> w) → tensor<f32, [2, 2]> {
    return x ⊙ w
}
"#;

const MAIN_SOURCE: &str = r#"+++
locale = "en"
+++
import from "libgt:nn" nn

@ radix { lane = "air" }
fn probe(tensor<f32, [2, 2]> x, tensor<f32, [2, 2]> w) → f32 {
    let _ y ← nn.scale_layer(x, w)
    return (y ⊙ y).mean()
}

main {
    let list<f32> x_data ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [2, 2]> x ← x_data ↦ tensor<f32, [2, 2]>
    let tensor<f32, [2, 2]> w ← x_data ↦ tensor<f32, [2, 2]>
    print probe(x, w)
}
"#;

/// Write the one-module library plus a consuming package; return the package
/// entry path.
fn write_fixture(dir: &Path) -> std::path::PathBuf {
    let lib_root = dir.join("libhome").join("libgt");
    let lib_src = lib_root.join("src");
    fs::create_dir_all(&lib_src).expect("create libgt src");
    fs::write(lib_root.join("faber.toml"), LIB_MANIFEST).expect("write libgt manifest");
    fs::write(lib_src.join("nn.fab"), NN_SOURCE).expect("write nn.fab");
    let app = dir.join("app");
    fs::create_dir_all(&app).expect("create app");
    let entry = app.join("main.fab");
    fs::write(&entry, MAIN_SOURCE).expect("write main.fab");
    entry
}

fn diagnostic_pairs(dir: &Path) -> Vec<(Option<String>, Option<String>)> {
    let entry = write_fixture(dir);
    let config = Config::default().with_library_home(dir.join("libhome"));
    let seams = crate::test_support::test_analysis_seams();
    let diagnostics = match analyze_package(&config, &entry, &seams) {
        Ok(package) => package.diagnostics.clone(),
        Err(diagnostics) => diagnostics,
    };
    diagnostics
        .iter()
        .filter(|diag| diag.is_error())
        .map(|diag| {
            (
                diag.code.map(str::to_owned),
                diag.issue().map(str::to_owned),
            )
        })
        .collect()
}

#[test]
fn air_import_boundary_module_qualified_call_declines_method_call() {
    let dir = crate::test_support::test_temp_dir("gt02-module-qualified");
    let pairs = diagnostic_pairs(&dir);
    assert!(
        pairs
            .iter()
            .any(|(code, issue)| code.as_deref() == Some("SEM059")
                && issue.as_deref() == Some("method_call")),
        "a module-qualified imported layer call in the air lane must decline as \
         SEM059.method_call: {pairs:?}"
    );
}
