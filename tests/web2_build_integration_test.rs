//! CLI-level regression: `faber build` on a TS browser-app product must write
//! deterministic static assets + manifest.
//!
//! On 73713d0 the compile step returned `package_target_assembly_pending`
//! before the product static-asset hook ran, so the user-facing build never
//! created dist assets. This test exercises the actual binary to prove the
//! product-build path is reached.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn faber_web_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../faber-web")
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("faber-web2-build-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Set up a browser-app product package identical in shape to the WEB3 unit
/// fixture, then run `faber build` as a subprocess.
fn write_browser_app(app: &Path) {
    let lib = faber_web_root();
    assert!(lib.is_dir(), "faber-web missing at {}", lib.display());

    fs::create_dir_all(app.join("src")).expect("app src");
    fs::create_dir_all(app.join("pages")).expect("pages");
    fs::create_dir_all(app.join("styles")).expect("styles");
    fs::create_dir_all(app.join("public")).expect("public");

    fs::write(
        app.join("faber.toml"),
        r#"[package]
name = "web2-cli-regression"
version = "0.1.0"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"

[dependencies]
web = "0.1.0"
"#,
    )
    .expect("manifest");

    let interface_root = lib.join("src");
    fs::write(
        app.join("faber.lock"),
        format!(
            r#"
[[package]]
name = "web"
version = "0.1.0"
source = "path"
package_root = "{package_root}"
kind = "lib"
target_language = "ts"
target_triple = "browser"
target_manifest = ""
interface_root = "{interface_root}"
artifact = ""
crate = "web"
rustc = ""
"#,
            package_root = lib.display(),
            interface_root = interface_root.display(),
        ),
    )
    .expect("lock");

    fs::write(
        app.join("src/main.fab"),
        r#"
importa ex "web:web" privata web
importa ex "web:dom" privata dom

@ WebController { selector = "[data-faber=shell]" }
functio shell(dom.Scope scope) → vacuum {
  nota dom.require(scope, "button")
}
"#,
    )
    .expect("entry");

    fs::write(
        app.join("pages/index.html"),
        "<main data-faber=shell><button>ok</button></main>\n",
    )
    .expect("page");
    fs::write(app.join("styles/main.css"), "body { margin: 0 }\n").expect("css");
    fs::write(app.join("public/favicon.ico"), b"icon-bytes").expect("favicon");
}

#[test]
fn faber_build_writes_static_assets_and_manifest_for_browser_product() {
    if Command::new("tsc").arg("--version").output().is_err() {
        eprintln!("tsc not found on PATH; skipping WEB2 CLI regression");
        return;
    }

    let root = temp_dir("browser-product");
    let app = root.join("app");
    write_browser_app(&app);

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("build")
        .arg(&app)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn faber build");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "faber build failed:\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Static assets copied deterministically.
    let page = app.join("dist/pages/index.html");
    let css = app.join("dist/styles/main.css");
    let favicon = app.join("dist/public/favicon.ico");
    assert!(page.is_file(), "static asset missing: {}", page.display());
    assert!(css.is_file(), "static asset missing: {}", css.display());
    assert!(
        favicon.is_file(),
        "static asset missing: {}",
        favicon.display()
    );

    // Asset manifest exists and is valid JSON with expected entries.
    let manifest_path = app.join("dist/assets.json");
    assert!(
        manifest_path.is_file(),
        "asset manifest missing: {}",
        manifest_path.display()
    );
    let manifest = fs::read_to_string(&manifest_path).expect("read asset manifest");
    assert!(
        manifest.contains("\"version\": 1"),
        "manifest version: {manifest}"
    );
    assert!(
        manifest.contains("pages/index.html"),
        "manifest must list page asset: {manifest}"
    );
    assert!(
        manifest.contains("\"sha256\""),
        "manifest must include sha256: {manifest}"
    );
}
